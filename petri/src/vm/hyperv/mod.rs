// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod hvc;
pub mod powershell;
use vmsocket::VmAddress;
use vmsocket::VmSocket;

use crate::disk_image::build_agent_image;
use crate::openhcl_diag::OpenHclDiagHandler;
use crate::Firmware;
use crate::IsolationType;
use crate::PetriVm;
use crate::PetriVmConfig;
use anyhow::Context;
use async_trait::async_trait;
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

#[async_trait]
impl PetriVmConfig for PetriVmConfigHyperV {
    async fn run_without_agent(self: Box<Self>) -> anyhow::Result<Box<dyn PetriVm>> {
        Ok(Box::new(Self::run_without_agent(*self)?))
    }

    async fn run_with_lazy_pipette(mut self: Box<Self>) -> anyhow::Result<Box<dyn PetriVm>> {
        Ok(Box::new(Self::run_with_lazy_pipette(*self)?))
    }

    async fn run(self: Box<Self>) -> anyhow::Result<(Box<dyn PetriVm>, PipetteClient)> {
        let (vm, client) = Self::run(*self).await?;
        Ok((Box::new(vm), client))
    }
}

/// A running VM that tests can interact with.
pub struct PetriVmHyperV {
    config: PetriVmConfigHyperV,
    openhcl_diag_handler: Option<OpenHclDiagHandler>,
    destroyed: bool,
}

#[async_trait]
impl PetriVm for PetriVmHyperV {
    async fn wait_for_halt(&mut self) -> anyhow::Result<HaltReason> {
        Self::wait_for_halt(self)
    }

    async fn wait_for_teardown(self: Box<Self>) -> anyhow::Result<HaltReason> {
        Self::wait_for_teardown(*self)
    }

    async fn test_inspect_openhcl(&mut self) -> anyhow::Result<()> {
        Self::test_inspect_openhcl(self).await
    }

    async fn wait_for_agent(&mut self) -> anyhow::Result<PipetteClient> {
        Self::wait_for_agent(self).await
    }

    async fn wait_for_vtl2_ready(&mut self) -> anyhow::Result<()> {
        Self::wait_for_vtl2_ready(self).await
    }
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

        let (guest_state_isolation_type, generation, guest_artifact, igvm_artifact) = match &firmware {
            Firmware::LinuxDirect | Firmware::OpenhclLinuxDirect => {
                todo!("linux direct not supported on hyper-v")
            }
            Firmware::Pcat { guest } => (
                powershell::HyperVGuestStateIsolationType::Disabled,
                powershell::HyperVGeneration::One,
                guest.artifact(),
                None,
            ),
            Firmware::Uefi { guest } => (
                powershell::HyperVGuestStateIsolationType::Disabled,
                powershell::HyperVGeneration::Two,
                guest.artifact(),
                None,
            ),
            Firmware::OpenhclUefi {
                guest,
                isolation,
                vtl2_nvme_boot: _, // TODO
            } => (
                match isolation {
                    Some(IsolationType::Vbs) => powershell::HyperVGuestStateIsolationType::Vbs,
                    Some(IsolationType::Snp) => powershell::HyperVGuestStateIsolationType::Snp,
                    Some(IsolationType::Tdx) => powershell::HyperVGuestStateIsolationType::Tdx,
                    None => powershell::HyperVGuestStateIsolationType::TrustedLaunch,
                },
                powershell::HyperVGeneration::Two,
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

        let reference_disk_path = resolver.resolve(guest_artifact);
        let openhcl_igvm = igvm_artifact.map(|a| resolver.resolve(a));

        Ok(PetriVmConfigHyperV {
            name: test_name,
            generation,
            guest_state_isolation_type,
            memory: 0x1_0000_0000,
            vm_path: None,
            vhd_paths: vec![vec![reference_disk_path]],
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

    /// Build and boot the requested VM. Does not configure and start pipette.
    /// Should only be used for testing platforms that pipette does not support.
    pub fn run_without_agent(self) -> anyhow::Result<PetriVmHyperV> {
        self.run_core(false)
    }

    /// Run the VM, configuring pipette to automatically start, but do not wait
    /// for it to connect. This is useful for tests where the first boot attempt
    /// is expected to not succeed, but pipette functionality is still desired.
    pub fn run_with_lazy_pipette(self) -> anyhow::Result<PetriVmHyperV> {
        self.run_core(true)
    }

    /// Run the VM, launching pipette and returning a client to it.
    pub async fn run(self) -> anyhow::Result<(PetriVmHyperV, PipetteClient)> {
        let mut vm = self.run_core(true)?;
        let client = vm.wait_for_agent().await?;
        Ok((vm, client))
    }

    /// Build and boot the requested VM
    fn run_core(self, with_agent: bool) -> anyhow::Result<PetriVmHyperV> {
        let ps_mod = self.temp_dir.path().join("hyperv.psm1");
        {
            let mut ps_mod_file = fs::File::create_new(&ps_mod)?;
            ps_mod_file
                .write_all(include_bytes!("hyperv.psm1"))
                .context("failed to write imc powershell module")?;
        }

        powershell::run_new_vm(powershell::HyperVNewVMArgs {
            name: &self.name,
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
                let diff_disk_path = self.temp_dir.path().join(format!(
                    "{}_{}_{}",
                    controller_number,
                    controller_location,
                    vhd.file_name()
                        .context("path has no filename")?
                        .to_string_lossy()
                ));

                powershell::create_child_vhd(&diff_disk_path, vhd)?;
                powershell::run_add_vm_hard_disk_drive(powershell::HyperVAddVMHardDiskDriveArgs {
                    name: &self.name,
                    controller_location: Some(controller_location as u32),
                    controller_number: Some(controller_number as u32),
                    path: Some(&diff_disk_path),
                })?;
            }
        }

        if with_agent {
            // Construct the agent disk.
            let agent_disk_path = self.temp_dir.path().join("cidata.vhd");
            {
                let agent_disk = build_agent_image(
                    self.arch,
                    self.os_flavor,
                    &self.resolver,
                    Some(&agent_disk_path),
                )
                .context("failed to build agent image")?;
                disk_vhd1::Vhd1Disk::make_fixed(&agent_disk)
                    .context("failed to make vhd for agent image")?;
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
                path: Some(&agent_disk_path),
            })?;
        }

        let openhcl_diag_handler = if self.openhcl_igvm.is_some() {
            Some(OpenHclDiagHandler {
                client: diag_client::DiagClient::from_hyperv_name(self.driver.clone(), &self.name)?,
                vtl2_vsock_path: PathBuf::from("TODO get rid of this"),
            })
        } else {
            None
        };

        hvc::hvc_start(&self.name)?;

        Ok(PetriVmHyperV {
            config: self,
            openhcl_diag_handler,
            destroyed: false,
        })
    }
}

impl PetriVmHyperV {
    /// Wait for the VM to halt, returning the reason for the halt.
    pub fn wait_for_halt(&mut self) -> anyhow::Result<HaltReason> {
        hvc::hvc_wait_for_power_off(&self.config.name)?;
        Ok(HaltReason::PowerOff) // TODO: Get actual halt reason
    }

    /// Wait for the VM to halt, returning the reason for the halt,
    /// and cleanly tear down the VM.
    pub fn wait_for_teardown(mut self) -> anyhow::Result<HaltReason> {
        let halt_reason = self.wait_for_halt()?;
        self.teardown()?;
        Ok(halt_reason)
    }

    /// Test that we are able to inspect OpenHCL.
    pub async fn test_inspect_openhcl(&mut self) -> anyhow::Result<()> {
        self.openhcl_diag()?.test_inspect().await
    }

    /// Wait for a connection from a pipette agent running in the guest.
    /// Useful if you've rebooted the vm or are otherwise expecting a fresh connection.
    pub async fn wait_for_vtl2_ready(&mut self) -> anyhow::Result<()> {
        self.openhcl_diag()?.wait_for_vtl2().await
    }

    /// Wait for VTL 2 to report that it is ready to respond to commands.
    /// Will fail if the VM is not running OpenHCL.
    ///
    /// This should only be necessary if you're doing something manual. All
    /// Petri-provided methods will wait for VTL 2 to be ready automatically.
    pub async fn wait_for_agent(&mut self) -> anyhow::Result<PipetteClient> {
        Self::wait_for_agent_core(
            &self.config.driver,
            &self.config.name,
            self.config.temp_dir.path(),
            false,
        )
        .await
    }

    async fn wait_for_agent_core(
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

    fn teardown(&mut self) -> anyhow::Result<()> {
        if !self.destroyed {
            powershell::run_remove_vm(&self.config.name)?;
            self.destroyed = true;
        }

        Ok(())
    }

    fn openhcl_diag(&self) -> anyhow::Result<&OpenHclDiagHandler> {
        if let Some(ohd) = &self.openhcl_diag_handler {
            Ok(ohd)
        } else {
            anyhow::bail!("VM is not configured with OpenHCL")
        }
    }
}

impl Drop for PetriVmHyperV {
    fn drop(&mut self) {
        // Try to remove the VM on test failure
        let _ = self.teardown();
    }
}