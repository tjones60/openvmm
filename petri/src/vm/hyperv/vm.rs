// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Provides an interface for creating and managing Hyper-V VMs

use super::hvc;
use super::hvc::VmState;
use super::powershell;
use crate::OpenHclServicingFlags;
use crate::PetriHaltReason;
use crate::PetriLogFile;
use crate::PetriVmFramebufferAccess;
use crate::VmScreenshotMeta;
use anyhow::Context;
use async_trait::async_trait;
use get_resources::ged::FirmwareEvent;
use guid::Guid;
use jiff::Timestamp;
use pal_async::DefaultDriver;
use pal_async::timer::PolledTimer;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Weak;
use std::time::Duration;
use tempfile::TempDir;
use thiserror::Error;
use tracing::Level;

/// A Hyper-V VM
pub struct HyperVVM {
    // properties
    vmid: Guid,
    name: String,
    create_time: Timestamp,

    // resources
    temp_dir: Arc<TempDir>,
    ps_mod: PathBuf,
    // TODO: use a trait interface here
    log_file: PetriLogFile,
    driver: DefaultDriver,
    // TODO: move logic using this to petri-specific code
    expected_boot_event: Option<FirmwareEvent>,

    // state
    destroyed: bool,
    last_start_time: Option<Timestamp>,
}

impl HyperVVM {
    /// Create a new Hyper-V VM
    pub async fn new(
        name: &str,
        generation: powershell::HyperVGeneration,
        guest_state_isolation_type: powershell::HyperVGuestStateIsolationType,
        memory: u64,
        log_file: PetriLogFile,
        expected_boot_event: Option<FirmwareEvent>,
        driver: DefaultDriver,
    ) -> anyhow::Result<Self> {
        let create_time = Timestamp::now();
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
        let cleanup = async |vmid: &Guid| -> anyhow::Result<()> {
            hvc::hvc_ensure_off(vmid).await?;
            powershell::run_remove_vm(vmid).await
        };

        if let Ok(vmids) = powershell::vm_id_from_name(&name).await {
            for vmid in vmids {
                match cleanup(&vmid).await {
                    Ok(_) => {
                        tracing::info!("Successfully cleaned up VM from previous test run ({vmid})")
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to clean up VM from previous test run ({vmid}): {e:?}"
                        )
                    }
                }
            }
        }

        let vmid = powershell::run_new_vm(powershell::HyperVNewVMArgs {
            name: &name,
            generation: Some(generation),
            guest_state_isolation_type: Some(guest_state_isolation_type),
            memory_startup_bytes: Some(memory),
            path: None,
            vhd_path: None,
        })
        .await?;

        tracing::info!(name, vmid = vmid.to_string(), "Created Hyper-V VM");

        // Instantiate this now so that its drop runs if there's a failure
        // below.
        let this = Self {
            vmid,
            name,
            create_time,
            temp_dir: Arc::new(temp_dir),
            ps_mod,
            log_file,
            driver,
            expected_boot_event,
            destroyed: false,
            last_start_time: None,
        };

        // Remove the default network adapter
        powershell::run_remove_vm_network_adapter(&vmid)
            .await
            .context("remove default network adapter")?;

        // Remove the default SCSI controller
        powershell::run_remove_vm_scsi_controller(&vmid, 0)
            .await
            .context("remove default SCSI controller")?;

        // Disable dynamic memory
        powershell::run_set_vm_memory(
            &vmid,
            &powershell::HyperVSetVMMemoryArgs {
                dynamic_memory_enabled: Some(false),
                ..Default::default()
            },
        )
        .await?;

        // Disable secure boot for generation 2 VMs
        if generation == powershell::HyperVGeneration::Two {
            powershell::run_set_vm_firmware(powershell::HyperVSetVMFirmwareArgs {
                vmid: &vmid,
                secure_boot_enabled: Some(false),
                secure_boot_template: None,
            })
            .await?;
        }

        Ok(this)
    }

    /// Get the VmId Guid of the VM
    pub fn vmid(&self) -> &Guid {
        &self.vmid
    }

    /// Get Hyper-V logs and write them to the log file
    pub async fn flush_logs(&self) -> anyhow::Result<()> {
        for event in powershell::hyperv_event_logs(&self.vmid, &self.create_time).await? {
            self.log_file.write_entry_fmt(
                Some(event.time_created),
                match event.level {
                    1 | 2 => Level::ERROR,
                    3 => Level::WARN,
                    5 => Level::TRACE,
                    _ => Level::INFO,
                },
                format_args!(
                    "[{}] {}: ({}, {}) {}",
                    event.time_created, event.provider_name, event.level, event.id, event.message,
                ),
            );
        }
        Ok(())
    }

    /// Waits for an event emitted by the firmware about its boot status, and
    /// verifies that it is the expected success value.
    // TODO: move this logic up
    pub async fn wait_for_successful_boot_event(&self) -> anyhow::Result<()> {
        if let Some(expected_boot_event) = self.expected_boot_event {
            self.wait_for(Self::boot_event, Some(expected_boot_event))
                .await
                .context("wait_for_successful_boot_event")?;
        } else {
            tracing::warn!("Configured firmware does not emit a boot event, skipping");
        }

        Ok(())
    }

    /// Waits for an event emitted by the firmware about its boot status, and
    /// returns that status.
    pub async fn wait_for_boot_event(&self) -> anyhow::Result<FirmwareEvent> {
        self.wait_for_some(Self::boot_event).await
    }

    async fn boot_event(&self) -> anyhow::Result<Option<FirmwareEvent>> {
        let events = powershell::hyperv_boot_events(
            &self.vmid,
            self.last_start_time.as_ref().unwrap_or(&self.create_time),
        )
        .await?;

        if events.len() > 1 {
            anyhow::bail!("Got more than one boot event");
        }

        events
            .first()
            .map(|e| match e.id {
                powershell::MSVM_BOOT_RESULTS_SUCCESS => Ok(FirmwareEvent::BootSuccess),
                powershell::MSVM_BOOT_RESULTS_FAILURE => Ok(FirmwareEvent::BootFailed),
                powershell::MSVM_BOOT_RESULTS_FAILURE_NO_DEVICES => Ok(FirmwareEvent::NoBootDevice),
                powershell::MSVM_BOOT_RESULTS_ATTEMPT => Ok(FirmwareEvent::BootAttempt),
                powershell::MSVM_BOOT_RESULTS_FAILURE_SECURE_BOOT_FAILURES => {
                    Ok(FirmwareEvent::BootFailed)
                }
                id => anyhow::bail!("Unexpected event id: {id}"),
            })
            .transpose()
    }

    /// Set the VM processor topology.
    pub async fn set_processor(
        &mut self,
        args: &powershell::HyperVSetVMProcessorArgs,
    ) -> anyhow::Result<()> {
        powershell::run_set_vm_processor(&self.vmid, args).await
    }

    /// Set the OpenHCL firmware file
    pub async fn set_openhcl_firmware(
        &mut self,
        igvm_file: &Path,
        increase_vtl2_memory: bool,
    ) -> anyhow::Result<()> {
        powershell::run_set_openhcl_firmware(
            &self.vmid,
            &self.ps_mod,
            igvm_file,
            increase_vtl2_memory,
        )
        .await
    }

    /// Configure secure boot
    pub async fn set_secure_boot(
        &mut self,
        secure_boot_enabled: bool,
        secure_boot_template: Option<powershell::HyperVSecureBootTemplate>,
    ) -> anyhow::Result<()> {
        powershell::run_set_vm_firmware(powershell::HyperVSetVMFirmwareArgs {
            vmid: &self.vmid,
            secure_boot_enabled: Some(secure_boot_enabled),
            secure_boot_template,
        })
        .await
    }

    /// Add a SCSI controller
    pub async fn add_scsi_controller(&mut self, target_vtl: u32) -> anyhow::Result<u32> {
        let controller_number = powershell::run_add_vm_scsi_controller(&self.vmid).await?;
        if target_vtl != 0 {
            powershell::run_set_vm_scsi_controller_target_vtl(
                &self.ps_mod,
                &self.vmid,
                controller_number,
                target_vtl,
            )
            .await?;
        }
        Ok(controller_number)
    }

    /// Add a VHD
    pub async fn add_vhd(
        &mut self,
        path: &Path,
        controller_type: powershell::ControllerType,
        controller_location: Option<u32>,
        controller_number: Option<u32>,
    ) -> anyhow::Result<()> {
        powershell::run_add_vm_hard_disk_drive(powershell::HyperVAddVMHardDiskDriveArgs {
            vmid: &self.vmid,
            controller_type,
            controller_location,
            controller_number,
            path: Some(path),
        })
        .await
    }

    /// Set the initial machine configuration (IMC hive file)
    pub async fn set_imc(&mut self, imc_hive: &Path) -> anyhow::Result<()> {
        powershell::run_set_initial_machine_configuration(&self.vmid, &self.ps_mod, imc_hive).await
    }

    async fn state(&self) -> anyhow::Result<VmState> {
        hvc::hvc_state(&self.vmid).await
    }

    async fn check_state(&self, expected: VmState) -> anyhow::Result<()> {
        let state = self.state().await?;
        if state != expected {
            anyhow::bail!("unexpected VM state {state:?}, should be {expected:?}");
        }
        Ok(())
    }

    /// Start the VM
    pub async fn start(&mut self) -> anyhow::Result<()> {
        self.check_state(VmState::Off).await?;
        self.last_start_time = Some(Timestamp::now());
        hvc::hvc_start(&self.vmid).await?;
        Ok(())
    }

    /// Attempt to gracefully shut down the VM
    pub async fn stop(&self) -> anyhow::Result<()> {
        self.check_shutdown_ic().await?;
        self.check_state(VmState::Running).await?;
        hvc::hvc_stop(&self.vmid).await?;
        Ok(())
    }

    /// Attempt to gracefully restart the VM
    pub async fn restart(&self) -> anyhow::Result<()> {
        self.check_shutdown_ic().await?;
        self.check_state(VmState::Running).await?;
        hvc::hvc_restart(&self.vmid).await?;
        Ok(())
    }

    /// Kill the VM
    pub async fn kill(&self) -> anyhow::Result<()> {
        hvc::hvc_kill(&self.vmid).await.context("hvc_kill")
    }

    /// Issue a hard reset to the VM
    pub async fn reset(&self) -> anyhow::Result<()> {
        hvc::hvc_reset(&self.vmid).await.context("hvc_reset")
    }

    /// Enable serial output and return the named pipe path
    pub async fn set_vm_com_port(&mut self, port: u8) -> anyhow::Result<String> {
        let pipe_path = format!(r#"\\.\pipe\{}-{}"#, self.vmid, port);
        powershell::run_set_vm_com_port(&self.vmid, port, Path::new(&pipe_path)).await?;
        Ok(pipe_path)
    }

    /// Wait for the VM to stop
    pub async fn wait_for_halt(&mut self, allow_reset: bool) -> anyhow::Result<PetriHaltReason> {
        let halt_reason = self.wait_for_some(Self::halt_event).await?;
        if halt_reason == PetriHaltReason::Reset {
            self.last_start_time = Some(Timestamp::now());
        }
        Ok(halt_reason)
    }

    async fn halt_event(&self) -> anyhow::Result<Option<PetriHaltReason>> {
        let events = powershell::hyperv_halt_events(
            &self.vmid,
            self.last_start_time.as_ref().unwrap_or(&self.create_time),
        )
        .await?;

        if events.len() > 1 {
            anyhow::bail!("Got more than one halt event");
        }

        events
            .first()
            .map(|e| match e.id {
                powershell::MSVM_HOST_STOP_SUCCESS
                | powershell::MSVM_HOST_SHUTDOWN_SUCCESS
                | powershell::MSVM_GUEST_SHUTDOWN_SUCCESS => Ok(PetriHaltReason::PowerOff),
                powershell::MSVM_HOST_RESET_SUCCESS
                | powershell::MSVM_GUEST_RESET_SUCCESS
                | powershell::MSVM_STOP_FOR_GUEST_RESET_SUCCESS => Ok(PetriHaltReason::Reset),
                powershell::MSVM_GUEST_HIBERNATE_SUCCESS => Ok(PetriHaltReason::Hibernate),
                powershell::MSVM_TRIPLE_FAULT_GENERAL_ERROR
                | powershell::MSVM_TRIPLE_FAULT_UNSUPPORTED_FEATURE_ERROR
                | powershell::MSVM_TRIPLE_FAULT_INVALID_VP_REGISTER_ERROR
                | powershell::MSVM_TRIPLE_FAULT_UNRECOVERABLE_EXCEPTION_ERROR => {
                    Ok(PetriHaltReason::TripleFault)
                }
                powershell::MSVM_STOP_CRITICAL_SUCCESS => Ok(PetriHaltReason::Other),
                id => anyhow::bail!("Unexpected event id: {id}"),
            })
            .transpose()
    }

    /// Wait for the VM shutdown ic
    pub async fn wait_for_enlightened_shutdown_ready(&self) -> anyhow::Result<()> {
        self.wait_for(Self::shutdown_ic_status, powershell::VmShutdownIcStatus::Ok)
            .await
            .context("wait_for_enlightened_shutdown_ready")
    }

    async fn shutdown_ic_status(&self) -> anyhow::Result<powershell::VmShutdownIcStatus> {
        powershell::vm_shutdown_ic_status(&self.vmid).await
    }

    async fn check_shutdown_ic(&self) -> anyhow::Result<()> {
        let status = self.shutdown_ic_status().await?;
        if status != powershell::VmShutdownIcStatus::Ok {
            anyhow::bail!("unexpected shutdown ic status {status:?}, should be Ok");
        }
        Ok(())
    }

    async fn wait_for<T: std::fmt::Debug + PartialEq>(
        &self,
        f: impl AsyncFn(&Self) -> anyhow::Result<T>,
        target: T,
    ) -> anyhow::Result<()> {
        loop {
            let state = f(self).await?;
            if state == target {
                break;
            }
            PolledTimer::new(&self.driver)
                .sleep(Duration::from_secs(1))
                .await;
        }

        Ok(())
    }

    async fn wait_for_some<T: std::fmt::Debug + PartialEq>(
        &self,
        f: impl AsyncFn(&Self) -> anyhow::Result<Option<T>>,
    ) -> anyhow::Result<T> {
        loop {
            let state = f(self).await?;
            if let Some(state) = state {
                return Ok(state);
            }
            PolledTimer::new(&self.driver)
                .sleep(Duration::from_secs(1))
                .await;
        }
    }

    /// Remove the VM
    pub async fn remove(mut self) -> anyhow::Result<()> {
        self.remove_inner().await
    }

    async fn remove_inner(&mut self) -> anyhow::Result<()> {
        if !self.destroyed {
            let res_off = hvc::hvc_ensure_off(&self.vmid).await;
            let res_remove = powershell::run_remove_vm(&self.vmid).await;

            self.flush_logs().await?;

            res_off?;
            res_remove?;
            self.destroyed = true;
        }

        Ok(())
    }

    /// Sets the VM firmware  command line.
    pub async fn set_vm_firmware_command_line(
        &self,
        openhcl_command_line: &str,
    ) -> anyhow::Result<()> {
        powershell::run_set_vm_command_line(&self.vmid, &self.ps_mod, openhcl_command_line).await
    }

    /// Enable VMBusRelay
    pub async fn set_vmbus_redirect(&self, enable: bool) -> anyhow::Result<()> {
        powershell::run_set_vmbus_redirect(&self.vmid, &self.ps_mod, enable).await
    }

    /// Perform an OpenHCL servicing operation.
    pub async fn restart_openhcl(&self, flags: OpenHclServicingFlags) -> anyhow::Result<()> {
        powershell::run_restart_openhcl(&self.vmid, &self.ps_mod, flags).await
    }

    /// Get the Framebuffer access
    pub fn get_framebuffer_access(&self) -> HyperVFramebufferAccess {
        HyperVFramebufferAccess {
            vmid: self.vmid,
            temp_dir: Arc::downgrade(&self.temp_dir),
            temp_bin_path: self.temp_dir.path().join("screenshot.bin"),
            ps_mod: self.ps_mod.clone(),
        }
    }
}

impl Drop for HyperVVM {
    fn drop(&mut self) {
        if std::env::var("PETRI_PRESERVE_VM")
            .ok()
            .is_none_or(|v| v.is_empty() || v == "0")
        {
            let _ = futures::executor::block_on(self.remove_inner());
        }
    }
}

pub struct HyperVFramebufferAccess {
    vmid: Guid,
    temp_dir: Weak<TempDir>,
    temp_bin_path: PathBuf,
    ps_mod: PathBuf,
}

#[async_trait]
impl PetriVmFramebufferAccess for HyperVFramebufferAccess {
    async fn screenshot(
        &mut self,
        image: &mut Vec<u8>,
    ) -> anyhow::Result<Option<VmScreenshotMeta>> {
        self.temp_dir.upgrade().context("VM no longer exists")?;
        if hvc::hvc_state(&self.vmid).await? == VmState::Running {
            Ok(Some(
                powershell::run_get_vm_screenshot(
                    &self.vmid,
                    image,
                    &self.ps_mod,
                    &self.temp_bin_path,
                )
                .await?,
            ))
        } else {
            Ok(None)
        }
    }
}

/// Error running command
#[derive(Error, Debug)]
pub(crate) enum CommandError {
    /// failed to launch command
    #[error("failed to launch command")]
    Launch(#[from] std::io::Error),
    /// command exited with non-zero status
    #[error("command exited with non-zero status ({0}): {1}")]
    Command(std::process::ExitStatus, String),
    /// command output is not utf-8
    #[error("command output is not utf-8")]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// Run the PowerShell script and return the output
pub(crate) async fn run_cmd(mut cmd: Command) -> Result<String, CommandError> {
    cmd.stderr(Stdio::piped()).stdin(Stdio::null());

    let cmd_debug = format!("{cmd:?}");
    tracing::debug!(cmd = cmd_debug, "executing command");

    let start = Timestamp::now();
    let output = blocking::unblock(move || cmd.output()).await?;
    let time_elapsed = Timestamp::now() - start;

    let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr_str = String::from_utf8_lossy(&output.stderr).to_string();
    tracing::debug!(
        cmd = cmd_debug,
        stdout_str,
        stderr_str,
        "command exited in {:.3}s with status {}",
        time_elapsed.total(jiff::Unit::Second).unwrap(),
        output.status
    );

    if !output.status.success() {
        return Err(CommandError::Command(output.status, stderr_str));
    }

    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
