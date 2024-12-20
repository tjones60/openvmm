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
use anyhow::Context;
use pal_async::socket::PolledSocket;
use pal_async::DefaultDriver;
use petri_artifacts_common::tags::MachineArch;
use petri_artifacts_common::tags::OsFlavor;
use petri_artifacts_core::TestArtifacts;
use pipette_client::PipetteClient;
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
    // Petri test dependency resolver
    resolver: TestArtifacts,
    arch: MachineArch,
    firmware: Firmware,
    driver: DefaultDriver,
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
        Ok(PetriVmConfigHyperV {
            name: "petritestvm".to_string(),
            generation: powershell::HyperVGeneration::Two,
            guest_state_isolation_type: match &firmware {
                Firmware::LinuxDirect | Firmware::OpenhclLinuxDirect => {
                    panic!("linux direct not supported on hyper-v")
                }
                Firmware::Pcat { .. } | Firmware::Uefi { .. } => {
                    powershell::HyperVGuestStateIsolationType::Disabled
                }
                Firmware::OpenhclUefi { .. } => {
                    powershell::HyperVGuestStateIsolationType::TrustedLaunch
                }
            },
            memory: 0x1_0000_0000,
            vm_path: None,
            vhd_paths: vec![vec![PathBuf::from("C:\\cross\\disk.vhdx")]],
            resolver,
            arch,
            firmware,
            driver: driver.clone(),
        })
    }
    /// Build and boot the requested VM
    pub async fn run(
        self,
        driver: &DefaultDriver,
    ) -> anyhow::Result<(PetriVmHyperV, PipetteClient)> {
        powershell::run_new_vm(powershell::HyperVNewVMArgs {
            name: &self.name,
            boot_device: None,
            generation: Some(self.generation),
            guest_state_isolation_type: Some(self.guest_state_isolation_type),
            memory_startup_bytes: Some(self.memory),
            path: self.vm_path.as_deref(),
            vhd_path: None,
        })?;

        powershell::run_set_vm_firmware(powershell::HyperVSetVMFirmwareArgs {
            name: &self.name,
            secure_boot_template: Some(match &self.firmware.os_flavor() {
                OsFlavor::Windows => powershell::HyperVSecureBootTemplate::MicrosoftWindows,
                OsFlavor::Linux => {
                    powershell::HyperVSecureBootTemplate::MicrosoftUEFICertificateAuthority
                }
                OsFlavor::FreeBsd => powershell::HyperVSecureBootTemplate::SecureBootDisabled,
                OsFlavor::Uefi => {
                    powershell::HyperVSecureBootTemplate::MicrosoftUEFICertificateAuthority
                }
            }),
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
        let agent_disk_path = PathBuf::from("C:\\cross\\cidata.vhd");
        {
            let _agent_disk = build_agent_image(
                MachineArch::Aarch64,
                OsFlavor::Windows,
                &self.resolver,
                Some(&agent_disk_path),
                ImageType::Vhd,
            )
            .context("failed to build agent image")?;
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

        let pipette_output_dir = PathBuf::from("E:\\test\\pipette");
        let client = wait_for_agent(driver, &self.name, &pipette_output_dir, false).await?;

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
        std::fs::remove_file(&self.agent_disk_path)?;
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
