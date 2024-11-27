// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use vmm_core_defs::HaltReason;

mod construct;
mod modify;
mod runtime;

pub struct PetriVmConfigHyperV {
    name: String,
}

pub struct PetriVmHyperV {
    name: String,
}

impl PetriVmConfigHyperV {
    /// Build and boot the requested VM
    pub fn run(self) -> anyhow::Result<PetriVmHyperV> {
        let PetriVmConfigHyperV { name } = self;

        construct::run_new_vm(&name)?;
        runtime::hvc_start(&name)?;

        Ok(PetriVmHyperV { name })
    }
}

impl PetriVmHyperV {
    /// Wait for VM to stop
    pub fn wait_for_teardown(self) -> anyhow::Result<HaltReason> {
        runtime::hvc_wait_for_power_off(&self.name)?;
        construct::run_remove_vm(&self.name)?;

        Ok(HaltReason::PowerOff)
    }
}

#[cfg(test)]
mod test {
    use super::PetriVmConfigHyperV;

    #[test]
    fn hyperv_test() {
        let config = PetriVmConfigHyperV {
            name: "testvm".to_string(),
        };
        let vm = config.run().unwrap();
        vm.wait_for_teardown().unwrap();
    }
}
