// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Functions for creating Hyper-V VMs.

use anyhow::Context as _;
use core::str;
use std::borrow::Cow;
use std::fmt::Display;
use std::path::Path;
use std::process::Command;

/// Hyper-V VM Generation
#[derive(Clone, Copy)]
pub enum HyperVGeneration {
    /// Generation 1 (with emulated legacy devices and PCAT BIOS)
    One,
    /// Generation 2 (synthetic devices and UEFI)
    Two,
}

impl Display for HyperVGeneration {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HyperVGeneration::One => {
                write!(f, "1")
            }
            HyperVGeneration::Two => {
                write!(f, "2")
            }
        }
    }
}

/// Hyper-V Guest State Isolation Type
#[derive(Clone, Copy)]
pub enum HyperVGuestStateIsolationType {
    /// Trusted Launch (HCL, SecureBoot, TPM)
    TrustedLaunch,
    /// VBS
    Vbs,
    /// SNP
    Snp,
    /// TDX
    Tdx,
    /// HCL but no isolation
    None,
    /// No isolation
    Disabled,
}

impl Display for HyperVGuestStateIsolationType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HyperVGuestStateIsolationType::TrustedLaunch => {
                write!(f, "TrustedLaunch")
            }
            HyperVGuestStateIsolationType::Vbs => {
                write!(f, "VBS")
            }
            HyperVGuestStateIsolationType::Snp => {
                write!(f, "SNP")
            }
            HyperVGuestStateIsolationType::Tdx => {
                write!(f, "TDX")
            }
            HyperVGuestStateIsolationType::None => {
                write!(f, "None")
            }
            HyperVGuestStateIsolationType::Disabled => {
                write!(f, "Disabled")
            }
        }
    }
}

/// Hyper-V Secure Boot Template
pub enum HyperVSecureBootTemplate {
    /// Secure Boot Disabled
    SecureBootDisabled,
    /// Windows Secure Boot Template
    MicrosoftWindows,
    /// Microsoft UEFI Certificate Authority Template
    MicrosoftUEFICertificateAuthority,
    /// Open Source Shielded VM Template
    OpenSourceShieldedVM = 3,
}

impl Display for HyperVSecureBootTemplate {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HyperVSecureBootTemplate::SecureBootDisabled => {
                write!(f, "SecureBootDisabled")
            }
            HyperVSecureBootTemplate::MicrosoftWindows => {
                write!(f, "MicrosoftWindows")
            }
            HyperVSecureBootTemplate::MicrosoftUEFICertificateAuthority => {
                write!(f, "MicrosoftUEFICertificateAuthority")
            }
            HyperVSecureBootTemplate::OpenSourceShieldedVM => {
                write!(f, "OpenSourceShieldedVM")
            }
        }
    }
}

/// Arguments for the New-VM powershell cmdlet
pub struct HyperVNewVMArgs<'a> {
    /// Specifies the name of the new virtual machine.
    pub name: &'a str,
    /// Specifies the device to use as the boot device for the new virtual machine.
    pub boot_device: Option<String>,
    /// Specifies the generation for the virtual machine.
    pub generation: Option<HyperVGeneration>,
    /// Specifies the Guest State Isolation Type
    pub guest_state_isolation_type: Option<HyperVGuestStateIsolationType>,
    /// Specifies the amount of memory, in bytes, to assign to the virtual machine.
    pub memory_startup_bytes: Option<u64>,
    /// Specifies the directory to store the files for the new virtual machine.
    pub path: Option<&'a Path>,
    /// Specifies the path to a virtual hard disk file.
    pub vhd_path: Option<&'a Path>,
}

/// Runs New-VM with the given arguments.
pub fn run_new_vm(args: HyperVNewVMArgs<'_>) -> anyhow::Result<()> {
    run_powershell_cmdlet("New-VM", |cmd| {
        if let Some(generation) = args.generation {
            cmd.arg("-Generation").arg(generation.to_string());
        }
        if let Some(guest_state_isolation_type) = args.guest_state_isolation_type {
            cmd.arg("-GuestStateIsolationType")
                .arg(guest_state_isolation_type.to_string());
        }
        if let Some(memory_startup_bytes) = args.memory_startup_bytes {
            cmd.arg("-MemoryStartupBytes")
                .arg(memory_startup_bytes.to_string());
        }
        if let Some(path) = args.path {
            cmd.arg("-Path").arg(path);
        }
        if let Some(vhd_path) = args.vhd_path {
            cmd.arg("-VHDPath").arg(vhd_path);
        }
        cmd.arg("-Name").arg(args.name).arg("-Force")
    })
}

/// Runs New-VM with the given arguments.
pub fn run_remove_vm(name: &str) -> anyhow::Result<()> {
    run_powershell_cmdlet("Remove-VM", |cmd| cmd.arg("-Name").arg(name).arg("-Force"))
}

/// Arguments for the Add-VMHardDiskDrive powershell cmdlet
pub struct HyperVAddVMHardDiskDriveArgs<'a> {
    /// Specifies the name of the virtual machine to which the hard disk
    /// drive is to be added.
    pub name: &'a str,
    /// Specifies the number of the location on the controller at which the
    /// hard disk drive is to be added. If not specified, the first available
    /// location in the controller specified with the ControllerNumber parameter
    /// is used.
    pub controller_location: Option<u32>,
    /// Specifies the number of the controller to which the hard disk drive is
    /// to be added. If not specified, this parameter assumes the value of the
    /// first available controller at the location specified in the
    /// ControllerLocation parameter.
    pub controller_number: Option<u32>,
    /// Specifies the type of the controller to which the hard disk drive is to
    /// be added. If not specified, IDE is attempted first. If the IDE
    /// controller port at the specified number and location is already
    /// connected to a drive, then it will try to create one on the SCSI
    /// controller specified by ControllerNumber. Allowed values are IDE
    /// and SCSI.
    pub controller_type: Option<String>,
    /// Specifies the full path of the hard disk drive file to be added.
    pub path: Option<&'a Path>,
}

/// Runs Add-VMHardDiskDrive with the given arguments.
pub fn run_add_vm_hard_disk_drive(args: HyperVAddVMHardDiskDriveArgs<'_>) -> anyhow::Result<()> {
    run_powershell_cmdlet("Add-VMHardDiskDrive", |cmd| {
        if let Some(controller_location) = args.controller_location {
            cmd.arg("-ControllerLocation")
                .arg(controller_location.to_string());
        }
        if let Some(controller_number) = args.controller_number {
            cmd.arg("-ControllerNumber")
                .arg(controller_number.to_string());
        }
        if let Some(controller_type) = args.controller_type {
            cmd.arg("-ControllerType").arg(controller_type);
        }
        if let Some(path) = args.path {
            cmd.arg("-Path").arg(path);
        }
        cmd.arg("-VMName").arg(args.name)
    })
}

/// Arguments for the Add-VMDvdDrive powershell cmdlet
pub struct HyperVAddVMDvdDriveArgs<'a> {
    /// Specifies the name of the virtual machine on which the DVD drive
    /// is to be configured.
    pub name: &'a str,
    /// Specifies the IDE controller location of the DVD drives to be
    /// configured. If not specified, DVD drives in all controller locations
    /// are configured.
    pub controller_location: Option<u32>,
    /// Specifies the IDE controller of the DVD drives to be configured.
    /// If not specified, DVD drives attached to all controllers are configured.
    pub controller_number: Option<u32>,
    /// Specifies the path to the ISO file or physical DVD drive that will serv
    /// as media for the virtual DVD drive.
    pub path: Option<&'a Path>,
}

/// Runs Add-VMDvdDrive with the given arguments.
pub fn run_add_vm_dvd_drive(args: HyperVAddVMDvdDriveArgs<'_>) -> anyhow::Result<()> {
    run_powershell_cmdlet("Add-VMDvdDrive", |cmd| {
        if let Some(controller_location) = args.controller_location {
            cmd.arg("-ControllerLocation")
                .arg(controller_location.to_string());
        }
        if let Some(controller_number) = args.controller_number {
            cmd.arg("-ControllerNumber")
                .arg(controller_number.to_string());
        }
        if let Some(path) = args.path {
            cmd.arg("-Path").arg(path);
        }
        cmd.arg("-VMName").arg(args.name)
    })
}

/// Runs Add-VMScsiController with the given arguments.
pub fn run_add_vm_scsi_controller(name: &str) -> anyhow::Result<()> {
    run_powershell_cmdlet("Add-VMScsiController", |cmd| cmd.arg("-VMName").arg(name))
}

/// Arguments for creating a new VHD
pub struct CreateVhdArgs<'a> {
    /// VHD path
    pub path: &'a Path,
    /// Filesystem label
    pub label: &'a str,
}

/// Create a new VHD, mount, initialize, and format. Returns drive letter.
pub fn create_vhd(args: CreateVhdArgs<'_>) -> anyhow::Result<char> {
    let drive_letter = run_powershell_cmdlet_output("New-VHD", |cmd| {
        cmd.arg("-Path")
            .arg(args.path)
            .arg("-Fixed")
            .arg("-SizeBytes")
            .arg("64MB");

        cmd.arg("|").arg("Mount-VHD").arg("-Passthru");

        cmd.arg("|").arg("Initialize-Disk").arg("-Passthru");

        cmd.arg("|")
            .arg("New-Partition")
            .arg("-AssignDriveLetter")
            .arg("-UseMaximumSize");

        cmd.arg("|")
            .arg("Format-Volume")
            .arg("-FileSystem")
            .arg("FAT32")
            .arg("-Force")
            .arg("-NewFileSystemLabel")
            .arg(args.label);

        cmd.arg("|")
            .arg("Select-Object")
            .arg("-ExpandProperty")
            .arg("DriveLetter")
    })?;

    if drive_letter.trim().len() != 1 {
        anyhow::bail!("invalid drive letter: {drive_letter}");
    }

    drive_letter
        .chars()
        .next()
        .context("could not get drive letter")
}

/// Runs Dismount-VHD with the given arguments.
pub fn run_dismount_vhd(path: &Path) -> anyhow::Result<()> {
    run_powershell_cmdlet("Dismount-VHD", |cmd| cmd.arg("-Path").arg(path))
}

/// Arguments for the Set-VMFirmware powershell cmdlet
pub struct HyperVSetVMFirmwareArgs<'a> {
    /// Specifies the name of virtual machines for which you want to modify the
    /// firmware configuration.
    pub name: &'a str,
    /// Specifies the name of the secure boot template. If secure boot is
    /// enabled, you must have a valid secure boot template for the guest
    /// operating system to start.
    pub secure_boot_template: Option<HyperVSecureBootTemplate>,
}

/// Runs Set-VMFirmware with the given arguments.
pub fn run_set_vm_firmware(args: HyperVSetVMFirmwareArgs<'_>) -> anyhow::Result<()> {
    run_powershell_cmdlet("Set-VMFirmware", |cmd| {
        if let Some(secure_boot_template) = args.secure_boot_template {
            cmd.arg("-SecureBootTemplate")
                .arg(secure_boot_template.to_string());
        }
        cmd.arg("-VMName").arg(args.name)
    })
}

/// Runs a powershell cmdlet with the given arguments.
fn run_powershell_cmdlet(
    cmdlet: &str,
    f: impl FnOnce(&mut Command) -> &mut Command,
) -> anyhow::Result<()> {
    let mut cmd = Command::new("powershell.exe");
    cmd.arg(cmdlet);
    f(&mut cmd);
    let status = cmd
        .status()
        .context(format!("failed to launch powershell cmdlet {cmdlet}"))?;
    if !status.success() {
        anyhow::bail!("powershell cmdlet {cmdlet} failed with exit code: {status}");
    }
    Ok(())
}

/// Runs a powershell cmdlet with the given arguments and returns the output
fn run_powershell_cmdlet_output(
    cmdlet: &str,
    f: impl FnOnce(&mut Command) -> &mut Command,
) -> anyhow::Result<String> {
    let mut cmd = Command::new("powershell.exe");
    cmd.arg(cmdlet);
    f(&mut cmd);
    let args: Vec<Cow<'_, str>> = cmd.get_args().map(|x| x.to_string_lossy()).collect();
    let full_cmd = format!("{} {}", cmd.get_program().to_string_lossy(), args.join(" "));
    eprintln!("{full_cmd}");
    let output = cmd
        .output()
        .context(format!("failed to launch powershell cmdlet {cmdlet}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "powershell cmdlet {cmdlet} failed with exit code: {}",
            output.status
        );
    }
    String::from_utf8(output.stdout).context("output is not utf-8")
}