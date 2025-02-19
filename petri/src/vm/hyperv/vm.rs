// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Provides an interface for creating and managing Hyper-V VMs

use super::hvc;
use super::powershell;
use anyhow::Context;
use guid::Guid;
use pal_async::DefaultDriver;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use tempfile::TempDir;

/// A Hyper-V VM
pub struct HyperVVM {
    name: String,
    vmid: Guid,
    destroyed: bool,
    _temp_dir: TempDir,
    ps_mod: PathBuf,
}

impl HyperVVM {
    /// Create a new Hyper-V VM
    pub fn new(
        name: &str,
        generation: powershell::HyperVGeneration,
        guest_state_isolation_type: powershell::HyperVGuestStateIsolationType,
        memory: u64,
    ) -> anyhow::Result<Self> {
        let name = name.to_owned();
        let temp_dir = tempfile::tempdir()?;
        let ps_mod = temp_dir.path().join("hyperv.psm1");
        {
            let mut ps_mod_file = std::fs::File::create_new(&ps_mod)?;
            ps_mod_file
                .write_all(include_bytes!("hyperv.psm1"))
                .context("failed to write hyperv helpers powershell module")?;
        }

        // Delete the VM if it already exists
        if hvc::hvc_list()?.contains(&name) {
            hvc::hvc_ensure_off(&name)?;
            powershell::run_remove_vm(powershell::VmId::Name(&name))?;
        }

        let vmid = powershell::run_new_vm(powershell::HyperVNewVMArgs {
            name: &name,
            generation: Some(generation),
            guest_state_isolation_type: Some(guest_state_isolation_type),
            memory_startup_bytes: Some(memory),
            path: None,
            vhd_path: None,
        })?;

        tracing::info!(name, vmid = vmid.to_string(), "Created Hyper-V VM");

        Ok(Self {
            name,
            vmid,
            destroyed: false,
            _temp_dir: temp_dir,
            ps_mod,
        })
    }

    /// Get the name of the VM
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Get the VmId Guid of the VM
    pub fn get_vmid(&self) -> &Guid {
        &self.vmid
    }

    /// Set the OpenHCL firmware file
    pub fn set_openhcl_firmware(
        &mut self,
        igvm_file: &Path,
        increase_vtl2_memory: bool,
    ) -> anyhow::Result<()> {
        powershell::run_set_openhcl_firmware(
            powershell::VmId::Id(&self.vmid),
            &self.ps_mod,
            igvm_file,
            increase_vtl2_memory,
        )
    }

    /// Set the secure boot template
    pub fn set_secure_boot_template(
        &mut self,
        secure_boot_template: powershell::HyperVSecureBootTemplate,
    ) -> anyhow::Result<()> {
        powershell::run_set_vm_firmware(powershell::HyperVSetVMFirmwareArgs {
            vmid: powershell::VmId::Id(&self.vmid),
            secure_boot_template: Some(secure_boot_template),
        })
    }

    /// Add a SCSI controller
    pub fn add_scsi_controller(&mut self) -> anyhow::Result<()> {
        powershell::run_add_vm_scsi_controller(powershell::VmId::Id(&self.vmid))
    }

    /// Add a VHD
    pub fn add_vhd(
        &mut self,
        path: &Path,
        controller_location: Option<u32>,
        controller_number: Option<u32>,
    ) -> anyhow::Result<()> {
        powershell::run_add_vm_hard_disk_drive(powershell::HyperVAddVMHardDiskDriveArgs {
            vmid: powershell::VmId::Id(&self.vmid),
            controller_location,
            controller_number,
            path: Some(path),
        })
    }

    /// Set the initial machine configuration (IMC hive file)
    pub fn set_imc(&mut self, imc_hive: &Path) -> anyhow::Result<()> {
        powershell::run_set_initial_machine_configuration(
            powershell::VmId::Id(&self.vmid),
            &self.ps_mod,
            imc_hive,
        )
    }

    /// Start the VM
    pub fn start(&self) -> anyhow::Result<()> {
        hvc::hvc_start(&self.vmid.to_string())
    }

    /// Get serial output
    pub fn set_vm_com_port(&mut self) -> anyhow::Result<String> {
        let pipe_path: &str = r#"\\.\pipe\test"#;
        powershell::run_set_vm_com_port(powershell::VmId::Id(&self.vmid), 1, Path::new(pipe_path))?;
        Ok(pipe_path.to_owned())
    }

    /// Wait for the VM to turn off
    pub async fn wait_for_power_off(&self, driver: &DefaultDriver) -> anyhow::Result<()> {
        hvc::hvc_wait_for_power_off(driver, &self.vmid.to_string()).await
    }

    /// Remove the VM
    pub fn remove(mut self) -> anyhow::Result<()> {
        self.remove_inner()
    }

    fn remove_inner(&mut self) -> anyhow::Result<()> {
        if !self.destroyed {
            hvc::hvc_ensure_off(&self.vmid.to_string())?;
            powershell::run_remove_vm(powershell::VmId::Id(&self.vmid))?;
            self.destroyed = true;
        }

        Ok(())
    }
}

impl Drop for HyperVVM {
    fn drop(&mut self) {
        let _ = self.remove_inner();
    }
}
