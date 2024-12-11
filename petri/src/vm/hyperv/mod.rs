// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use vmm_core_defs::HaltReason;

mod construct;
mod modify;
mod runtime;

pub use construct::HyperVConfig;
pub use construct::HyperVGeneration;

pub struct PetriVmConfigHyperV(HyperVConfig);

pub struct PetriVmHyperV {
    config: HyperVConfig,
}

impl PetriVmConfigHyperV {
    /// Build and boot the requested VM
    pub fn run(self) -> anyhow::Result<PetriVmHyperV> {
        construct::run_new_vm(&self.0)?;
        runtime::hvc_start(&self.0.name)?;

        Ok(PetriVmHyperV { config: self.0 })
    }
}

impl PetriVmHyperV {
    /// Wait for VM to stop
    pub fn wait_for_teardown(self) -> anyhow::Result<HaltReason> {
        runtime::hvc_wait_for_power_off(&self.config.name)?;
        construct::run_remove_vm(&self.config.name)?;

        Ok(HaltReason::PowerOff)
    }
}

#[cfg(test)]
mod test {
    use super::HyperVConfig;
    use super::HyperVGeneration;
    use super::PetriVmConfigHyperV;
    use std::path::PathBuf;

    #[test]
    fn hyperv_test() {
        let config = PetriVmConfigHyperV(HyperVConfig {
            name: "petritestvm".to_string(),
            generation: Some(HyperVGeneration::Two),
            memory_startup_bytes: Some(0x1_0000_0000),
            path: None,
            vhd_path: Some(PathBuf::from("E:\\cross\\disk.vhdx")),
        });
        let vm = config.run().unwrap();
        vm.wait_for_teardown().unwrap();
    }
}
