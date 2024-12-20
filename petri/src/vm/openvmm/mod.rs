// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Code managing the lifetime of a `PetriVmOpenVMM`. All VMs live the same lifecycle:
//! * A `PetriVmConfigOpenVMM` is built for the given firmware and architecture in `construct`.
//! * The configuration is optionally modified from the defaults using the helpers in `modify`.
//! * The `PetriVmOpenVMM` is started by the code in `start`.
//! * The VM is interacted with through the methods in `runtime`.
//! * The VM is either shut down by the code in `runtime`, or gets dropped and cleaned up automatically.

mod construct;
mod modify;
mod runtime;
mod start;

pub use runtime::PetriVmOpenVMM;

use super::Firmware;
use crate::linux_direct_serial_agent::LinuxDirectSerialAgent;
use crate::openhcl_diag::OpenHclDiagHandler;
use framebuffer::FramebufferAccess;
use fs_err::File;
use get_resources::ged::FirmwareEvent;
use guid::Guid;
use hvlite_defs::config::Config;
use hyperv_ic_resources::shutdown::ShutdownRpc;
use mesh::MpscReceiver;
use mesh::Sender;
use pal_async::socket::PolledSocket;
use pal_async::task::Task;
use pal_async::DefaultDriver;
use petri_artifacts_common::tags::MachineArch;
use petri_artifacts_common::tags::OsFlavor;
use petri_artifacts_core::TestArtifacts;
use std::path::PathBuf;
use std::sync::Arc;
use unix_socket::UnixListener;
use vtl2_settings_proto::Vtl2Settings;

/// The instance guid used for all of our SCSI drives.
pub(crate) const SCSI_INSTANCE: Guid =
    Guid::from_static_str("27b553e8-8b39-411b-a55f-839971a7884f");

/// The instance guid for the NVMe controller automatically added for boot media.
pub(crate) const BOOT_NVME_INSTANCE: Guid =
    Guid::from_static_str("92bc8346-718b-449a-8751-edbf3dcd27e4");

/// The namespace ID for the NVMe controller automatically added for boot media.
pub(crate) const BOOT_NVME_NSID: u32 = 37;

/// The LUN ID for the NVMe controller automatically added for boot media.
pub(crate) const BOOT_NVME_LUN: u32 = 1;

/// Configuration state for a test VM.
pub struct PetriVmConfigOpenVMM {
    // Direct configuration related information.
    firmware: Firmware,
    arch: MachineArch,
    config: Config,

    // Runtime resources
    resources: PetriVmResourcesOpenVMM,

    // Logging
    hvlite_log_file: File,

    // Resources that are only used during startup.
    ged: Option<get_resources::ged::GuestEmulationDeviceHandle>,
    vtl2_settings: Option<Vtl2Settings>,
    framebuffer_access: Option<FramebufferAccess>,
}

/// Various channels and resources used to interact with the VM while it is running.
struct PetriVmResourcesOpenVMM {
    serial_tasks: Vec<Task<anyhow::Result<()>>>,
    firmware_event_recv: MpscReceiver<FirmwareEvent>,
    shutdown_ic_send: Sender<ShutdownRpc>,
    expected_boot_event: Option<FirmwareEvent>,
    ged_send: Option<Arc<Sender<get_resources::ged::GuestEmulationRequest>>>,
    pipette_listener: PolledSocket<UnixListener>,
    vtl2_pipette_listener: Option<PolledSocket<UnixListener>>,
    openhcl_diag_handler: Option<OpenHclDiagHandler>,
    linux_direct_serial_agent: Option<LinuxDirectSerialAgent>,

    // Externally injected management stuff also needed at runtime.
    driver: DefaultDriver,
    resolver: TestArtifacts,
    output_dir: PathBuf,
}

impl PetriVmConfigOpenVMM {
    /// Get the OS that the VM will boot into.
    pub fn os_flavor(&self) -> OsFlavor {
        self.firmware.os_flavor()
    }
}