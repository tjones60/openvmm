// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Wrappers for Hyper-V Powershell Cmdlets

use anyhow::Context;
use core::str;
use guid::Guid;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::str::FromStr;
use time::OffsetDateTime;

/// Information needed to identify a Hyper-V VM
pub enum VmId<'a> {
    /// The name of the VM
    Name(&'a str),
    /// The VmId Guid
    Id(&'a Guid),
}

/// Hyper-V VM Generation
#[derive(Clone, Copy)]
pub enum HyperVGeneration {
    /// Generation 1 (with emulated legacy devices and PCAT BIOS)
    One,
    /// Generation 2 (synthetic devices and UEFI)
    Two,
}

impl AsRef<OsStr> for HyperVGeneration {
    fn as_ref(&self) -> &OsStr {
        OsStr::new(match self {
            HyperVGeneration::One => "1",
            HyperVGeneration::Two => "2",
        })
    }
}

/// Hyper-V Guest State Isolation Type
#[derive(Clone, Copy)]
pub enum HyperVGuestStateIsolationType {
    /// Trusted Launch (OpenHCL, SecureBoot, TPM)
    TrustedLaunch,
    /// VBS
    Vbs,
    /// SNP
    Snp,
    /// TDX
    Tdx,
    /// OpenHCL but no isolation
    OpenHCL,
    /// No HCL and no isolation
    Disabled,
}

impl AsRef<OsStr> for HyperVGuestStateIsolationType {
    fn as_ref(&self) -> &OsStr {
        OsStr::new(match self {
            HyperVGuestStateIsolationType::TrustedLaunch => "TrustedLaunch",
            HyperVGuestStateIsolationType::Vbs => "VBS",
            HyperVGuestStateIsolationType::Snp => "SNP",
            HyperVGuestStateIsolationType::Tdx => "TDX",
            HyperVGuestStateIsolationType::OpenHCL => "OpenHCL",
            HyperVGuestStateIsolationType::Disabled => "Disabled",
        })
    }
}

/// Hyper-V Secure Boot Template
#[derive(Clone, Copy)]
pub enum HyperVSecureBootTemplate {
    /// Secure Boot Disabled
    SecureBootDisabled,
    /// Windows Secure Boot Template
    MicrosoftWindows,
    /// Microsoft UEFI Certificate Authority Template
    MicrosoftUEFICertificateAuthority,
    /// Open Source Shielded VM Template
    OpenSourceShieldedVM,
}

impl AsRef<OsStr> for HyperVSecureBootTemplate {
    fn as_ref(&self) -> &OsStr {
        OsStr::new(match self {
            HyperVSecureBootTemplate::SecureBootDisabled => "SecureBootDisabled",
            HyperVSecureBootTemplate::MicrosoftWindows => "MicrosoftWindows",
            HyperVSecureBootTemplate::MicrosoftUEFICertificateAuthority => {
                "MicrosoftUEFICertificateAuthority"
            }
            HyperVSecureBootTemplate::OpenSourceShieldedVM => "OpenSourceShieldedVM",
        })
    }
}

/// Arguments for the New-VM powershell cmdlet
pub struct HyperVNewVMArgs<'a> {
    /// Specifies the name of the new virtual machine.
    pub name: &'a str,
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
pub fn run_new_vm(args: HyperVNewVMArgs<'_>) -> anyhow::Result<Guid> {
    let vmid = PowerShellBuilder::new()
        .cmdlet("New-VM")
        .arg("Name", args.name)
        .arg_opt("Generation", args.generation)
        .arg_opt("GuestStateIsolationType", args.guest_state_isolation_type)
        .arg_opt_string("MemoryStartupBytes", args.memory_startup_bytes)
        .arg_opt("Path", args.path)
        .arg_opt("VHDPath", args.vhd_path)
        .flag("Force")
        .pipeline()
        .select_object_property("Id")
        .pipeline()
        .select_object_property("Guid")
        .finish()
        .output(true)
        .context("new_vm")?;

    Guid::from_str(&vmid).context("invalid vmid")
}

/// Runs New-VM with the given arguments.
pub fn run_remove_vm(vmid: VmId<'_>) -> anyhow::Result<()> {
    PowerShellBuilder::new()
        .get_vm(vmid)
        .pipeline()
        .cmdlet("Remove-VM")
        .flag("Force")
        .finish()
        .run()
        .context("remove_vm")
}

/// Arguments for the Add-VMHardDiskDrive powershell cmdlet
pub struct HyperVAddVMHardDiskDriveArgs<'a> {
    /// Specifies the name of the virtual machine to which the hard disk
    /// drive is to be added.
    pub vmid: VmId<'a>,
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
    /// Specifies the full path of the hard disk drive file to be added.
    pub path: Option<&'a Path>,
}

/// Runs Add-VMHardDiskDrive with the given arguments.
pub fn run_add_vm_hard_disk_drive(args: HyperVAddVMHardDiskDriveArgs<'_>) -> anyhow::Result<()> {
    PowerShellBuilder::new()
        .get_vm(args.vmid)
        .pipeline()
        .cmdlet("Add-VMHardDiskDrive")
        .arg_opt_string("ControllerLocation", args.controller_location)
        .arg_opt_string("ControllerNumber", args.controller_number)
        .arg_opt("Path", args.path)
        .finish()
        .run()
        .context("add_vm_hard_disk_drive")
}

/// Arguments for the Add-VMDvdDrive powershell cmdlet
pub struct HyperVAddVMDvdDriveArgs<'a> {
    /// Specifies the name of the virtual machine on which the DVD drive
    /// is to be configured.
    pub vmid: VmId<'a>,
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
    PowerShellBuilder::new()
        .get_vm(args.vmid)
        .pipeline()
        .cmdlet("Add-VMDvdDrive")
        .arg_opt_string("ControllerLocation", args.controller_location)
        .arg_opt_string("ControllerNumber", args.controller_number)
        .arg_opt("Path", args.path)
        .finish()
        .run()
        .context("add_vm_dvd_drive")
}

/// Runs Add-VMScsiController with the given arguments.
pub fn run_add_vm_scsi_controller(vmid: VmId<'_>) -> anyhow::Result<()> {
    PowerShellBuilder::new()
        .get_vm(vmid)
        .pipeline()
        .cmdlet("Add-VMScsiController")
        .finish()
        .run()
        .context("add_vm_scsi_controller")
}

/// Create a new differencing VHD with the provided parent.
pub fn create_child_vhd(path: &Path, parent_path: &Path) -> anyhow::Result<()> {
    PowerShellBuilder::new()
        .cmdlet("New-VHD")
        .arg("Path", path)
        .arg("ParentPath", parent_path)
        .flag("Differencing")
        .finish()
        .run()
        .context("create_child_vhd")
}

/// Runs Dismount-VHD with the given arguments.
pub fn run_dismount_vhd(path: &Path) -> anyhow::Result<()> {
    PowerShellBuilder::new()
        .cmdlet("Dismount-VHD")
        .arg("Path", path)
        .finish()
        .run()
        .context("dismount_vhd")
}

/// Arguments for the Set-VMFirmware powershell cmdlet
pub struct HyperVSetVMFirmwareArgs<'a> {
    /// Specifies the name of virtual machines for which you want to modify the
    /// firmware configuration.
    pub vmid: VmId<'a>,
    /// Specifies the name of the secure boot template. If secure boot is
    /// enabled, you must have a valid secure boot template for the guest
    /// operating system to start.
    pub secure_boot_template: Option<HyperVSecureBootTemplate>,
}

/// Runs Set-VMFirmware with the given arguments.
pub fn run_set_vm_firmware(args: HyperVSetVMFirmwareArgs<'_>) -> anyhow::Result<()> {
    PowerShellBuilder::new()
        .get_vm(args.vmid)
        .pipeline()
        .cmdlet("Set-VMFirmware")
        .arg_opt("SecureBootTemplate", args.secure_boot_template)
        .finish()
        .run()
        .context("set_vm_firmware")
}

/// Runs Set-VMFirmware with the given arguments.
pub fn run_set_openhcl_firmware(
    vmid: VmId<'_>,
    ps_mod: &Path,
    igvm_file: &Path,
    increase_vtl2_memory: bool,
) -> anyhow::Result<()> {
    PowerShellBuilder::new()
        .cmdlet("Import-Module")
        .positional(ps_mod)
        .next()
        .get_vm(vmid)
        .pipeline()
        .cmdlet("Set-OpenHCLFirmware")
        .arg("IgvmFile", igvm_file)
        .flag_opt(increase_vtl2_memory.then_some("IncreaseVtl2Memory"))
        .finish()
        .run()
        .context("set_openhcl_firmware")
}

/// Sets the initial machine configuration for a VM
pub fn run_set_initial_machine_configuration(
    vmid: VmId<'_>,
    ps_mod: &Path,
    imc_hive: &Path,
) -> anyhow::Result<()> {
    PowerShellBuilder::new()
        .cmdlet("Import-Module")
        .positional(ps_mod)
        .next()
        .get_vm(vmid)
        .pipeline()
        .cmdlet("Set-InitialMachineConfiguration")
        .arg("ImcHive", imc_hive)
        .finish()
        .run()
        .context("set_initial_machine_configuration")
}

/// Enables the specified vm com port and binds it to the named pipe path
pub fn run_set_vm_com_port(vmid: VmId<'_>, port: u8, path: &Path) -> anyhow::Result<()> {
    PowerShellBuilder::new()
        .get_vm(vmid)
        .pipeline()
        .cmdlet("Set-VMComPort")
        .arg_string("Number", port)
        .arg("Path", path)
        .finish()
        .run()
        .context("run_set_vm_com_port")
}

/// Get event logs
pub fn run_get_winevent(
    log_name: &str,
    start_time: OffsetDateTime,
    find: &str,
) -> anyhow::Result<Vec<String>> {
    let start_time = format!(
        "{:0>4}-{:0>2}-{:0>2} {:0>2}:{:0>2}:{:0>2}",
        start_time.year(),
        start_time.month() as u8,
        start_time.day(),
        start_time.hour(),
        start_time.minute(),
        start_time.second()
    );
    let logs = PowerShellBuilder::new()
        .cmdlet("Get-WinEvent")
        .flag("Oldest")
        .arg("FilterHashtable",
            format!(
                "@{{ LogName=\"{log_name}\"; StartTime=\"{start_time}\" }}"
            ),
        )
        .pipeline()
        .cmdlet("where")
        .positional("message")
        .arg("Match", find)
        .pipeline()
        .cmdlet("ForEach-Object")
        .positional(r#"{"[{0}] {1}: ({2}, {3}) {4}<END>`n" -f $_.TimeCreated, $_.ProviderName, $_.Level, $_.Id, $_.Message}"#)
        .finish()
        .output(false)?;
    Ok(logs.split("<END>\n").map(|s| s.to_string()).collect())
}

/// A PowerShell script builder
pub struct PowerShellBuilder(Command);

impl PowerShellBuilder {
    /// Create a new PowerShell command
    pub fn new() -> Self {
        let mut cmd = Command::new("powershell.exe");
        cmd.arg("-NoProfile")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        Self(cmd)
    }

    /// Start a new Cmdlet
    pub fn cmdlet<S: AsRef<OsStr>>(mut self, cmdlet: S) -> PowerShellCmdletBuilder {
        self.0.arg(cmdlet);
        PowerShellCmdletBuilder(self.0)
    }

    /// Run the PowerShell script
    pub fn run(self) -> anyhow::Result<()> {
        _ = self.output(true)?;
        Ok(())
    }

    /// Run the PowerShell script and return the output
    pub fn output(mut self, log_stdout: bool) -> anyhow::Result<String> {
        let output = self.0.output().context("failed to launch powershell")?;
        let ps_stdout = log_stdout.then(|| String::from_utf8_lossy(&output.stdout).to_string());
        let ps_stderr = String::from_utf8_lossy(&output.stderr).to_string();
        tracing::debug!(ps_cmd = self.get_cmd(), ps_stdout, ps_stderr);
        if !output.status.success() {
            anyhow::bail!("powershell script failed with exit code: {}", output.status);
        }
        Ok(String::from_utf8(output.stdout)
            .context("powershell output is not utf-8")?
            .trim()
            .to_owned())
    }

    /// Get the command to be run
    pub fn get_cmd(&self) -> String {
        format!(
            "{} {}",
            self.0.get_program().to_string_lossy(),
            self.0
                .get_args()
                .collect::<Vec<_>>()
                .join(OsStr::new(" "))
                .to_string_lossy()
        )
    }

    /// Return a property using `Select-Object`. Usually preceeded by `pipeline()`.
    pub fn select_object_property<S: AsRef<OsStr>>(
        mut self,
        property: S,
    ) -> PowerShellCmdletBuilder {
        self.0
            .arg("Select-Object")
            .arg("-ExpandProperty")
            .arg(property);
        PowerShellCmdletBuilder(self.0)
    }

    /// Get a VM object using `Get-VM`. Usually followed by `pipeline()`.
    pub fn get_vm(mut self, vmid: VmId<'_>) -> PowerShellCmdletBuilder {
        self.0.arg("Get-VM");
        match vmid {
            VmId::Name(name) => self.0.arg("-Name").arg(name),
            VmId::Id(guid) => self.0.arg("-Id").arg(guid.to_string()),
        };
        PowerShellCmdletBuilder(self.0)
    }
}

/// A PowerShell Cmdlet builder
pub struct PowerShellCmdletBuilder(Command);

impl PowerShellCmdletBuilder {
    /// Add a flag to the cmdlet
    pub fn flag<S: AsRef<OsStr>>(mut self, flag: S) -> Self {
        let mut arg = OsString::from("-");
        arg.push(flag);
        self.0.arg(arg);
        self
    }

    /// Optionally add a flag to the cmdlet
    pub fn flag_opt<S: AsRef<OsStr>>(self, flag: Option<S>) -> Self {
        if let Some(flag) = flag {
            self.flag(flag)
        } else {
            self
        }
    }

    /// Add a positional argument to the cmdlet
    pub fn positional<S: AsRef<OsStr>>(mut self, positional: S) -> Self {
        self.0.arg(positional);
        self
    }

    /// Add a positional argument to the cmdlet
    pub fn positional_string<S: ToString>(self, positional: S) -> Self {
        self.positional(positional.to_string())
    }

    /// Optionally add a positional argument to the cmdlet
    pub fn positional_opt<S: AsRef<OsStr>>(self, positional: Option<S>) -> Self {
        if let Some(positional) = positional {
            self.positional(positional)
        } else {
            self
        }
    }

    /// Optionally add a positional argument to the cmdlet
    pub fn positional_opt_string<S: ToString>(self, positional: Option<S>) -> Self {
        self.positional_opt(positional.map(|x| x.to_string()))
    }

    /// Add an argument to the cmdlet
    pub fn arg<S: AsRef<OsStr>, T: AsRef<OsStr>>(self, name: S, value: T) -> Self {
        self.flag(name).positional(value)
    }

    /// Add an argument to the cmdlet
    pub fn arg_string<S: AsRef<OsStr>, T: ToString>(self, name: S, value: T) -> Self {
        self.arg(name, value.to_string())
    }

    /// Optionally add an argument to the cmdlet
    pub fn arg_opt<S: AsRef<OsStr>, T: AsRef<OsStr>>(self, name: S, value: Option<T>) -> Self {
        if let Some(value) = value {
            self.arg(name, value)
        } else {
            self
        }
    }

    /// Optionally add an argument to the cmdlet
    pub fn arg_opt_string<S: AsRef<OsStr>, T: ToString>(self, name: S, value: Option<T>) -> Self {
        self.arg_opt(name, value.map(|x| x.to_string()))
    }

    /// Finish the cmdlet
    pub fn finish(self) -> PowerShellBuilder {
        PowerShellBuilder(self.0)
    }

    /// Finish the cmdlet with a pipeline operator
    pub fn pipeline(mut self) -> PowerShellBuilder {
        self.0.arg("|");
        self.finish()
    }

    /// Finish the cmdlet with a semicolon
    pub fn next(mut self) -> PowerShellBuilder {
        self.0.arg(";");
        self.finish()
    }
}
