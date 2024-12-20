// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Integration tests that run on hyper-v

use petri::hyperv::PetriVmConfigHyperV;
use petri_artifacts_common::tags::MachineArch;

#[test]
fn hyperv_test() {
    async fn hyperv_test(driver: ::pal_async::DefaultDriver) -> anyhow::Result<()> {
        let resolver = petri::TestArtifactResolver::new(Box::new(
            petri_artifact_resolver_openvmm_known_paths::OpenvmmKnownPathsTestArtifactResolver,
        ))
        .require(::petri_artifacts_common::artifacts::PIPETTE_WINDOWS_AARCH64)
        .finalize();
        let config = PetriVmConfigHyperV::new(
            petri::Firmware::Uefi {
                guest: petri::UefiGuest::None,
            },
            MachineArch::Aarch64,
            resolver,
            &driver,
        )?;
        let (vm, agent) = config.run(&driver).await?;
        agent.power_off().await?;
        vm.wait_for_teardown()?;

        Ok(())
    }

    ::pal_async::DefaultPool::run_with(|driver| async move { hyperv_test(driver).await }).unwrap()
}
