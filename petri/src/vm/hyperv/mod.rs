// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod hvc;
mod modify;
pub mod powershell;
use vmsocket::VmAddress;
use vmsocket::VmSocket;

use super::Firmware;
use crate::disk_image::build_agent_image;
use crate::disk_image::ImageType;
use crate::IsolationType;
use anyhow::Context;
use pal_async::socket::PolledSocket;
use pal_async::DefaultDriver;
use petri_artifacts_common::tags::MachineArch;
use petri_artifacts_common::tags::OsFlavor;
use petri_artifacts_core::AsArtifactHandle;
use petri_artifacts_core::TestArtifacts;
use pipette_client::PipetteClient;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use vmm_core_defs::HaltReason;

/// Hyper-V VM configuration and resources
pub struct PetriVmConfigHyperV {
    // Specifies the name of the new virtual machine.
    name: String,
    // Specifies the generation for the virtual machine.
    generation: powershell::HyperVGeneration,
    // Specifies the Guest State Isolation Type
    guest_state_isolation_type: powershell::HyperVGuestStateIsolationType,
    // Specifies the amount of memory, in bytes, to assign to the virtual machine.
    memory: u64,
    // Specifies the directory to store the files for the new virtual machine.
    vm_path: Option<PathBuf>,
    // Specifies the path to a virtual hard disk file(s) to attach to the
    // virtual machine as SCSI (Gen2) or IDE (Gen1) drives.
    vhd_paths: Vec<Vec<PathBuf>>,
    secure_boot_template: powershell::HyperVSecureBootTemplate,
    openhcl_igvm: Option<PathBuf>,

    // Petri test dependency resolver
    resolver: TestArtifacts,
    driver: DefaultDriver,

    arch: MachineArch,
    os_flavor: OsFlavor,

    // Folder to store temporary data for this test
    temp_dir: tempfile::TempDir,
}

/// A running VM that tests can interact with.
pub struct PetriVmHyperV {
    config: PetriVmConfigHyperV,
    agent_disk_path: PathBuf,
}

impl PetriVmConfigHyperV {
    /// Create a new Hyper-V petri VM config
    pub fn new(
        firmware: Firmware,
        arch: MachineArch,
        resolver: TestArtifacts,
        driver: &DefaultDriver,
    ) -> anyhow::Result<Self> {
        let test_name = crate::get_test_name()?;
        let temp_dir = tempfile::tempdir()?;
        eprintln!(
            "Test Name: {test_name} Temp Dir: {}",
            temp_dir.path().display()
        );

        let (guest_state_isolation_type, guest_artifact, igvm_artifact) = match &firmware {
            Firmware::LinuxDirect | Firmware::OpenhclLinuxDirect => {
                panic!("linux direct not supported on hyper-v")
            }
            Firmware::Pcat { guest } => (
                powershell::HyperVGuestStateIsolationType::Disabled,
                guest.artifact(),
                None,
            ),
            Firmware::Uefi { guest } => (
                powershell::HyperVGuestStateIsolationType::Disabled,
                guest.artifact(),
                None,
            ),
            Firmware::OpenhclUefi {
                guest,
                isolation,
                vtl2_nvme_boot: _,
            } => (
                match isolation {
                    Some(IsolationType::Vbs) => powershell::HyperVGuestStateIsolationType::Vbs,
                    Some(IsolationType::Snp) => powershell::HyperVGuestStateIsolationType::Snp,
                    Some(IsolationType::Tdx) => powershell::HyperVGuestStateIsolationType::Tdx,
                    None => powershell::HyperVGuestStateIsolationType::TrustedLaunch,
                },
                guest.artifact(),
                Some(match (arch, isolation) {
                    (MachineArch::X86_64, None) => {
                        petri_artifacts_vmm_test::artifacts::openhcl_igvm::LATEST_STANDARD_X64
                            .erase()
                    }
                    (MachineArch::X86_64, Some(_)) => {
                        petri_artifacts_vmm_test::artifacts::openhcl_igvm::LATEST_CVM_X64.erase()
                    }
                    (MachineArch::Aarch64, None) => {
                        petri_artifacts_vmm_test::artifacts::openhcl_igvm::LATEST_STANDARD_AARCH64
                            .erase()
                    }
                    _ => anyhow::bail!("unsupported arch/isolation combination"),
                }),
            ),
        };

        let reference_os_disk = resolver.resolve(guest_artifact);
        let os_disk = temp_dir.path().join(
            reference_os_disk
                .file_name()
                .context("path has no filename")?,
        );
        fs::copy(&reference_os_disk, &os_disk)?;
        let openhcl_igvm = igvm_artifact.map(|a| resolver.resolve(a));

        Ok(PetriVmConfigHyperV {
            name: test_name,
            generation: powershell::HyperVGeneration::Two,
            guest_state_isolation_type,
            memory: 0x1_0000_0000,
            vm_path: None,
            vhd_paths: vec![vec![os_disk]],
            secure_boot_template: match firmware.os_flavor() {
                OsFlavor::Windows => powershell::HyperVSecureBootTemplate::MicrosoftWindows,
                OsFlavor::Linux => {
                    powershell::HyperVSecureBootTemplate::MicrosoftUEFICertificateAuthority
                }
                OsFlavor::FreeBsd | OsFlavor::Uefi => {
                    powershell::HyperVSecureBootTemplate::SecureBootDisabled
                }
            },
            openhcl_igvm,
            resolver,
            driver: driver.clone(),
            arch,
            os_flavor: firmware.os_flavor(),
            temp_dir,
        })
    }
    /// Build and boot the requested VM
    pub async fn run(self) -> anyhow::Result<(PetriVmHyperV, PipetteClient)> {
        let ps_mod = self.temp_dir.path().join("hyperv.psm1");
        {
            let mut ps_mod_file = fs::File::create_new(&ps_mod)?;
            ps_mod_file
                .write_all(include_bytes!("hyperv.psm1"))
                .context("failed to write imc powershell module")?;
        }

        powershell::run_new_vm(powershell::HyperVNewVMArgs {
            name: &self.name,
            boot_device: None,
            generation: Some(self.generation),
            guest_state_isolation_type: Some(self.guest_state_isolation_type),
            memory_startup_bytes: Some(self.memory),
            path: self.vm_path.as_deref(),
            vhd_path: None,
        })?;

        if let Some(igvm_file) = &self.openhcl_igvm {
            powershell::run_set_openhcl_firmware(&self.name, &ps_mod, igvm_file)?;
        }

        powershell::run_set_vm_firmware(powershell::HyperVSetVMFirmwareArgs {
            name: &self.name,
            secure_boot_template: Some(self.secure_boot_template),
        })?;

        for (controller_number, vhds) in self.vhd_paths.iter().enumerate() {
            powershell::run_add_vm_scsi_controller(&self.name)?;
            for (controller_location, vhd) in vhds.iter().enumerate() {
                powershell::run_add_vm_hard_disk_drive(powershell::HyperVAddVMHardDiskDriveArgs {
                    name: &self.name,
                    controller_location: Some(controller_location as u32),
                    controller_number: Some(controller_number as u32),
                    controller_type: None,
                    path: Some(vhd),
                })?;
            }
        }

        // Construct the agent disk.
        let agent_disk_path = self.temp_dir.path().join("cidata.vhd");
        {
            let _agent_disk = build_agent_image(
                self.arch,
                self.os_flavor,
                &self.resolver,
                Some(&agent_disk_path),
                ImageType::Vhd,
            )
            .context("failed to build agent image")?;
        }

        if matches!(self.os_flavor, OsFlavor::Windows) {
            // Make a file for the IMC hive. It's not guaranteed to be at a fixed
            // location at runtime.
            let imc_hive = self.temp_dir.path().join("imc.hiv");
            {
                let mut imc_hive_file = fs::File::create_new(&imc_hive)?;
                imc_hive_file
                    .write_all(include_bytes!("../../../guest-bootstrap/imc.hiv"))
                    .context("failed to write imc hive")?;
            }

            // Set the IMC
            powershell::run_set_initial_machine_configuration(&self.name, &ps_mod, &imc_hive)?;
        }

        powershell::run_add_vm_scsi_controller(&self.name)?;
        powershell::run_add_vm_hard_disk_drive(powershell::HyperVAddVMHardDiskDriveArgs {
            name: &self.name,
            controller_location: Some(0),
            controller_number: Some(self.vhd_paths.len() as u32),
            controller_type: None,
            path: Some(&agent_disk_path),
        })?;

        hvc::hvc_start(&self.name)?;

        let pipette_output_dir = self.temp_dir.path().join("pipette");
        fs::create_dir(&pipette_output_dir)?;
        let client = wait_for_agent(&self.driver, &self.name, &pipette_output_dir, false).await?;

        Ok((
            PetriVmHyperV {
                config: self,
                agent_disk_path,
            },
            client,
        ))
    }
}

impl PetriVmHyperV {
    /// Wait for VM to stop
    pub fn wait_for_teardown(self) -> anyhow::Result<HaltReason> {
        hvc::hvc_wait_for_power_off(&self.config.name)?;
        powershell::run_remove_vm(&self.config.name)?;
        fs::remove_file(&self.agent_disk_path)?;
        Ok(HaltReason::PowerOff)
    }
}

async fn wait_for_agent(
    driver: &DefaultDriver,
    name: &str,
    output_dir: &Path,
    set_high_vtl: bool,
) -> anyhow::Result<PipetteClient> {
    let vm_id = diag_client::hyperv::vm_id_from_name(name)?;

    let mut socket = VmSocket::new().context("failed to create AF_HYPERV socket")?;

    socket
        .set_connect_timeout(std::time::Duration::from_secs(300))
        .context("failed to set connect timeout")?;

    socket
        .set_high_vtl(set_high_vtl)
        .context("failed to set socket for VTL0")?;

    socket.bind(VmAddress::hyperv_vsock(
        vm_id,
        pipette_client::PIPETTE_VSOCK_PORT,
    ))?;

    let mut socket: PolledSocket<socket2::Socket> =
        PolledSocket::new(driver, socket.into()).context("failed to create polled socket")?;

    socket.listen(1)?;

    let (conn, _) = socket
        .accept()
        .await
        .context("failed to accept pipette connection")?;

    PipetteClient::new(driver, PolledSocket::new(driver, conn)?, output_dir)
        .await
        .context("failed to connect to pipette")
}
