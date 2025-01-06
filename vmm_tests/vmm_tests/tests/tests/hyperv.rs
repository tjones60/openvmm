// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Integration tests that run on hyper-v

use petri::hyperv::PetriVmConfigHyperV;
use petri_artifacts_common::tags::MachineArch;

#[test]
fn hyperv_test_linux() {
    async fn hyperv_test(driver: ::pal_async::DefaultDriver) -> anyhow::Result<()> {
        let resolver = petri::TestArtifactResolver::new(Box::new(
            petri_artifact_resolver_openvmm_known_paths::OpenvmmKnownPathsTestArtifactResolver,
        ))
        .require(::petri_artifacts_common::artifacts::PIPETTE_LINUX_X64)
        .require(petri_artifacts_vmm_test::artifacts::test_vhd::UBUNTU_2204_SERVER_X64)
        .finalize();
        let config = PetriVmConfigHyperV::new(
            petri::Firmware::Uefi {
                guest: petri::UefiGuest::Vhd(petri::BootImageConfig::from_vhd(
                    petri_artifacts_vmm_test::artifacts::test_vhd::UBUNTU_2204_SERVER_X64,
                )),
            },
            MachineArch::X86_64,
            resolver,
            &driver,
        )?;
        let (vm, agent) = config.run().await?;
        agent.power_off().await?;
        vm.wait_for_teardown()?;

        Ok(())
    }

    ::pal_async::DefaultPool::run_with(|driver| async move { hyperv_test(driver).await }).unwrap()
}

#[test]
fn hyperv_test_windows() {
    async fn hyperv_test(driver: ::pal_async::DefaultDriver) -> anyhow::Result<()> {
        let resolver = petri::TestArtifactResolver::new(Box::new(
            petri_artifact_resolver_openvmm_known_paths::OpenvmmKnownPathsTestArtifactResolver,
        ))
        .require(::petri_artifacts_common::artifacts::PIPETTE_WINDOWS_X64)
        .require(
            petri_artifacts_vmm_test::artifacts::test_vhd::GEN2_WINDOWS_DATA_CENTER_CORE2022_X64,
        )
        .finalize();
        let config = PetriVmConfigHyperV::new(
            petri::Firmware::Uefi {
                guest: petri::UefiGuest::Vhd(petri::BootImageConfig::from_vhd(
                    petri_artifacts_vmm_test::artifacts::test_vhd::GEN2_WINDOWS_DATA_CENTER_CORE2022_X64,
                )),
            },
            MachineArch::X86_64,
            resolver,
            &driver,
        )?;
        let (vm, agent) = config.run().await?;
        agent.power_off().await?;
        vm.wait_for_teardown()?;

        Ok(())
    }

    ::pal_async::DefaultPool::run_with(|driver| async move { hyperv_test(driver).await }).unwrap()
}

#[test]
fn hyperv_test_windows_openhcl() {
    async fn hyperv_test(driver: ::pal_async::DefaultDriver) -> anyhow::Result<()> {
        let resolver = petri::TestArtifactResolver::new(Box::new(
            petri_artifact_resolver_openvmm_known_paths::OpenvmmKnownPathsTestArtifactResolver,
        ))
        .require(::petri_artifacts_common::artifacts::PIPETTE_WINDOWS_X64)
        .require(
            petri_artifacts_vmm_test::artifacts::test_vhd::GEN2_WINDOWS_DATA_CENTER_CORE2022_X64,
        )
        .require(petri_artifacts_vmm_test::artifacts::openhcl_igvm::LATEST_STANDARD_X64)
        .finalize();
        let config = PetriVmConfigHyperV::new(
            petri::Firmware::OpenhclUefi { 
                guest: petri::UefiGuest::Vhd(petri::BootImageConfig::from_vhd(
                    petri_artifacts_vmm_test::artifacts::test_vhd::GEN2_WINDOWS_DATA_CENTER_CORE2022_X64,
                )),
                isolation: None,
                vtl2_nvme_boot: false
            },
            MachineArch::X86_64,
            resolver,
            &driver,
        )?;
        let (vm, agent) = config.run().await?;
        agent.power_off().await?;
        vm.wait_for_teardown()?;

        Ok(())
    }

    ::pal_async::DefaultPool::run_with(|driver| async move { hyperv_test(driver).await }).unwrap()
}
