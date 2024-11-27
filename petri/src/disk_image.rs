// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use anyhow::Context;
use fatfs::FormatVolumeOptions;
use fatfs::FsOptions;
use petri_artifacts_common::artifacts as common_artifacts;
use petri_artifacts_common::tags::MachineArch;
use petri_artifacts_common::tags::OsFlavor;
use petri_artifacts_core::AsArtifactHandle;
use petri_artifacts_core::TestArtifacts;
use std::io::Read;
use std::io::Seek;
use std::io::Write;
use std::ops::Range;
use std::path::Path;

pub enum ImageType {
    Raw,
    #[cfg_attr(not(windows), allow(dead_code))]
    Vhd,
}

/// Builds a disk image containing pipette and any files needed for the guest VM
/// to run pipette.
pub fn build_agent_image(
    arch: MachineArch,
    os_flavor: OsFlavor,
    resolver: &TestArtifacts,
    path: Option<&Path>,
    image_type: ImageType,
) -> anyhow::Result<std::fs::File> {
    match os_flavor {
        OsFlavor::Windows => {
            // Windows doesn't use cloud-init, so we only need pipette
            // (which is configured via the IMC hive).
            build_disk_image(
                "PIPETTE",
                &[(
                    "pipette.exe",
                    PathOrBinary::Path(&resolver.resolve(match arch {
                        MachineArch::X86_64 => common_artifacts::PIPETTE_WINDOWS_X64.erase(),
                        MachineArch::Aarch64 => common_artifacts::PIPETTE_WINDOWS_AARCH64.erase(),
                    })),
                )],
                path,
                image_type,
            )
        }
        OsFlavor::Linux => {
            // Linux uses cloud-init, so we need to include the cloud-init
            // configuration files as well.
            build_disk_image(
                // cloud-init looks for a volume label of "CIDATA"
                // volume labels are always all caps when creating VHDs on
                // Windows, so just always use all caps since Linux is case
                // sensitive
                "CIDATA",
                &[
                    (
                        "pipette",
                        PathOrBinary::Path(&resolver.resolve(match arch {
                            MachineArch::X86_64 => common_artifacts::PIPETTE_LINUX_X64.erase(),
                            MachineArch::Aarch64 => common_artifacts::PIPETTE_LINUX_AARCH64.erase(),
                        })),
                    ),
                    (
                        "meta-data",
                        PathOrBinary::Binary(include_bytes!("../guest-bootstrap/meta-data")),
                    ),
                    (
                        "user-data",
                        PathOrBinary::Binary(include_bytes!("../guest-bootstrap/user-data")),
                    ),
                    // Specify a non-present NIC to work around https://github.com/canonical/cloud-init/issues/5511
                    // TODO: support dynamically configuring the network based on vm configuration
                    (
                        "network-config",
                        PathOrBinary::Binary(include_bytes!("../guest-bootstrap/network-config")),
                    ),
                ],
                path,
                image_type,
            )
        }
        OsFlavor::FreeBsd | OsFlavor::Uefi => {
            // No pipette binary yet.
            todo!()
        }
    }
}

enum PathOrBinary<'a> {
    Path(&'a Path),
    Binary(&'a [u8]),
}

fn build_disk_image(
    volume_label: &str,
    files: &[(&str, PathOrBinary<'_>)],
    path: Option<&Path>,
    image_type: ImageType,
) -> anyhow::Result<std::fs::File> {
    match image_type {
        ImageType::Raw => build_disk_image_raw(
            format!("{volume_label:<11}").as_bytes().try_into()?,
            files,
            path,
        ),
        #[cfg(windows)]
        ImageType::Vhd => build_disk_image_vhd(
            volume_label,
            files,
            path.expect("file name required for vhd image"),
        ),
        #[cfg(not(windows))]
        ImageType::Vhd => anyhow::bail!("creating VHDs is only supported on Windows"),
    }
}

fn build_disk_image_raw(
    volume_label: &[u8; 11],
    files: &[(&str, PathOrBinary<'_>)],
    path: Option<&Path>,
) -> anyhow::Result<std::fs::File> {
    let mut file = if let Some(path) = path {
        std::fs::File::create_new(path).context("failed to create disk image file")?
    } else {
        tempfile::tempfile().context("failed to make temp file")?
    };

    file.set_len(64 * 1024 * 1024)
        .context("failed to set file size")?;

    let partition_range =
        build_gpt(&mut file, "CIDATA").context("failed to construct partition table")?;
    build_fat32(
        &mut fscommon::StreamSlice::new(&mut file, partition_range.start, partition_range.end)?,
        volume_label,
        files,
    )
    .context("failed to format volume")?;
    Ok(file)
}

#[cfg(windows)]
fn build_disk_image_vhd(
    volume_label: &str,
    files: &[(&str, PathOrBinary<'_>)],
    vhd_path: &Path,
) -> anyhow::Result<std::fs::File> {
    let disk_letter =
        crate::hyperv::powershell::create_vhd(crate::hyperv::powershell::CreateVhdArgs {
            path: vhd_path,
            label: volume_label,
        })?;
    for (path, src) in files {
        let mut dest = std::fs::File::create_new(format!("{disk_letter}:\\{path}"))
            .context("failed to create file")?;
        match *src {
            PathOrBinary::Path(src_path) => {
                let mut src = fs_err::File::open(src_path)?;
                std::io::copy(&mut src, &mut dest).context("failed to copy file")?;
            }
            PathOrBinary::Binary(src_data) => {
                dest.write_all(src_data).context("failed to write file")?;
            }
        }
    }
    crate::hyperv::powershell::run_dismount_vhd(vhd_path)?;

    Ok(std::fs::File::open(vhd_path)?)
}

fn build_gpt(file: &mut (impl Read + Write + Seek), name: &str) -> anyhow::Result<Range<u64>> {
    const SECTOR_SIZE: u64 = 512;
    // EBD0A0A2-B9E5-4433-87C0-68B6B72699C7
    const BDP_GUID: [u8; 16] = [
        0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99,
        0xC7,
    ];
    const PARTITION_GUID: [u8; 16] = [
        0x55, 0x29, 0x65, 0x69, 0x3A, 0xA7, 0x98, 0x41, 0xBA, 0xBD, 0xB5, 0x50, 0x77, 0x14, 0xA1,
        0xF3,
    ];

    let mut mbr = mbrman::MBR::new_from(file, SECTOR_SIZE as u32, [0xff; 4])?;
    let mut gpt = gptman::GPT::new_from(file, SECTOR_SIZE, [0xff; 16])?;

    // Set up the "Protective" Master Boot Record
    let first_chs = mbrman::CHS::new(0, 0, 2);
    let last_chs = mbrman::CHS::empty(); // This is wrong but doesn't really matter.
    mbr[1] = mbrman::MBRPartitionEntry {
        boot: mbrman::BOOT_INACTIVE,
        first_chs,
        sys: 0xEE, // GPT protective
        last_chs,
        starting_lba: 1,
        sectors: gpt.header.last_usable_lba.try_into().unwrap_or(0xFFFFFFFF),
    };
    mbr.write_into(file)?;

    file.rewind()?;

    // Set up the GPT Partition Table Header
    gpt[1] = gptman::GPTPartitionEntry {
        partition_type_guid: BDP_GUID,
        unique_partition_guid: PARTITION_GUID,
        starting_lba: gpt.header.first_usable_lba,
        ending_lba: gpt.header.last_usable_lba,
        attribute_bits: 0,
        partition_name: name.into(),
    };
    gpt.write_into(file)?;

    // calculate the EFI partition's usable range
    let partition_start_byte = gpt[1].starting_lba * SECTOR_SIZE;
    let partition_num_bytes = (gpt[1].ending_lba - gpt[1].starting_lba) * SECTOR_SIZE;
    Ok(partition_start_byte..partition_start_byte + partition_num_bytes)
}

fn build_fat32(
    file: &mut (impl Read + Write + Seek),
    volume_label: &[u8; 11],
    files: &[(&str, PathOrBinary<'_>)],
) -> anyhow::Result<()> {
    fatfs::format_volume(
        &mut *file,
        FormatVolumeOptions::new()
            .volume_label(*volume_label)
            .fat_type(fatfs::FatType::Fat32),
    )
    .context("failed to format volume")?;
    let fs = fatfs::FileSystem::new(file, FsOptions::new()).context("failed to open fs")?;
    for (path, src) in files {
        let mut dest = fs
            .root_dir()
            .create_file(path)
            .context("failed to create file")?;
        match *src {
            PathOrBinary::Path(src_path) => {
                let mut src = fs_err::File::open(src_path)?;
                std::io::copy(&mut src, &mut dest).context("failed to copy file")?;
            }
            PathOrBinary::Binary(src_data) => {
                dest.write_all(src_data).context("failed to write file")?;
            }
        }
        dest.flush().context("failed to flush file")?;
    }
    fs.unmount().context("failed to unmount fs")?;
    Ok(())
}
