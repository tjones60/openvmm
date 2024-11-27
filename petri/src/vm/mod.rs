// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use async_trait::async_trait;
use vmm_core_defs::HaltReason;

pub mod hyperv;
mod openvmm;

pub use openvmm::*;

/// Configuration state for a test VM.
#[async_trait]
pub trait PetriVmConfig<T: PetriVm> {
    /// Build and boot the requested VM. Does not configure and start pipette.
    /// Should only be used for testing platforms that pipette does not support.
    async fn run_without_agent(self) -> anyhow::Result<T>;
}

/// A running VM that tests can interact with.
#[async_trait]
pub trait PetriVm {
    /// Wait for the VM to halt, returning the reason for the halt,
    /// and cleanly tear down the VM.
    async fn wait_for_teardown(mut self) -> anyhow::Result<HaltReason>;
}
