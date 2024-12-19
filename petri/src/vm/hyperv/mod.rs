// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod hvc;
mod modify;
pub mod powershell;

pub use powershell::HyperVGeneration;
pub use powershell::HyperVGuestStateIsolationType;

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

pub struct PetriVmConfigHyperV {
    /// Specifies the name of the new virtual machine.
    pub name: String,
    /// Specifies the generation for the virtual machine.
    pub generation: Option<HyperVGeneration>,
    /// Specifies the Guest State Isolation Type
    pub guest_state_isolation_type: Option<HyperVGuestStateIsolationType>,
    /// Specifies the amount of memory, in bytes, to assign to the virtual machine.
    pub memory: Option<u64>,
    /// Specifies the directory to store the files for the new virtual machine.
    pub vm_path: Option<PathBuf>,
    /// Specifies the path to a virtual hard disk file(s) to attach to the
    /// virtual machine as SCSI (Gen2) or IDE (Gen1) drives.
    pub vhd_paths: Vec<Vec<PathBuf>>,
    pub resolver: TestArtifacts,
}

pub struct PetriVmHyperV {
    config: PetriVmConfigHyperV,
}

impl PetriVmConfigHyperV {
    /// Build and boot the requested VM
    pub async fn run(
        self,
        driver: &DefaultDriver,
    ) -> anyhow::Result<(PetriVmHyperV, PipetteClient)> {
        powershell::run_new_vm(powershell::HyperVNewVMArgs {
            name: &self.name,
            boot_device: None,
            generation: self.generation,
            guest_state_isolation_type: self.guest_state_isolation_type,
            memory_startup_bytes: self.memory,
            path: self.vm_path.as_deref(),
            vhd_path: None,
        })?;

        // powershell::run_set_vm_firmware

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
        let agent_disk_path = PathBuf::from("E:\\test\\cidata.vhd");
        {
            let _agent_disk = build_agent_image(
                MachineArch::X86_64,
                OsFlavor::Linux,
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
        let client = wait_for_agent(driver, &self.name, &pipette_output_dir).await?;

        Ok((PetriVmHyperV { config: self }, client))
    }
}

impl PetriVmHyperV {
    /// Wait for VM to stop
    pub fn wait_for_teardown(self) -> anyhow::Result<HaltReason> {
        hvc::hvc_wait_for_power_off(&self.config.name)?;
        powershell::run_remove_vm(&self.config.name)?;

        Ok(HaltReason::PowerOff)
    }
}

async fn wait_for_agent(
    driver: &DefaultDriver,
    name: &str,
    output_dir: &Path,
) -> anyhow::Result<PipetteClient> {
    let vm_id = diag_client::hyperv::vm_id_from_name(name)?;
    let stream =
        diag_client::hyperv::connect_vsock(driver, vm_id, pipette_client::PIPETTE_VSOCK_PORT)
            .await?;
    let mut vsock = PolledSocket::new(driver, socket2::Socket::from(stream))?;

    // Wait for the pipette connection.
    tracing::info!("listening for pipette connection");
    let (conn, _) = vsock
        .accept()
        .await
        .context("failed to accept pipette connection")?;

    tracing::info!("handshaking with pipette");
    let client = PipetteClient::new(driver, PolledSocket::new(driver, conn)?, output_dir)
        .await
        .context("failed to connect to pipette");

    tracing::info!("completed pipette handshake");
    client
}
