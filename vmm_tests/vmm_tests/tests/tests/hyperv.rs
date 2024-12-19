// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Integration tests that run on hyper-v

use petri::hyperv::HyperVGeneration;
use petri::hyperv::PetriVmConfigHyperV;
use std::path::PathBuf;

#[test]
fn hyperv_test() {
    async fn hyperv_test(driver: ::pal_async::DefaultDriver) -> anyhow::Result<()> {
        let resolver = petri::TestArtifactResolver::new(Box::new(
            petri_artifact_resolver_openvmm_known_paths::OpenvmmKnownPathsTestArtifactResolver,
        ))
        .require(::petri_artifacts_common::artifacts::PIPETTE_LINUX_X64)
        .finalize();
        let config = PetriVmConfigHyperV {
            name: "petritestvm".to_string(),
            generation: Some(HyperVGeneration::Two),
            guest_state_isolation_type: None,
            memory: Some(0x1_0000_0000),
            vm_path: None,
            vhd_paths: vec![vec![PathBuf::from(
                "E:\\test\\ubuntu-22.04-server-cloudimg-amd64.vhd",
            )]],
            resolver,
        };
        let (vm, agent) = config.run(&driver).await?;
        agent.power_off().await?;
        vm.wait_for_teardown()?;

        Ok(())
    }

    ::pal_async::DefaultPool::run_with(|driver| async move { hyperv_test(driver).await }).unwrap()
}
