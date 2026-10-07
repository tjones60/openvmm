// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Nested tests

use petri::PetriVmBuilder;
use petri::PetriVmmBackend;

// use petri::ResolvedArtifact;
// use petri::openvmm::OpenVmmPetriBackend;
// use petri_artifacts_common::artifacts::PIPETTE_LINUX_X64_MUSL;
// use petri_artifacts_vmm_test::artifacts::OPENVMM_LINUX_X64_MUSL;
// use petri_artifacts_vmm_test::artifacts::host_tools::NEXTEST_VMM_TESTS_ARCHIVE_LINUX_X64_MUSL;
// use petri_artifacts_vmm_test::artifacts::loadable::LINUX_DIRECT_TEST_INITRD_X64;
// use petri_artifacts_vmm_test::artifacts::loadable::LINUX_DIRECT_TEST_KERNEL_X64;
// use vmm_test_macros::openvmm_test;

// #[openvmm_test(openvmm_linux_direct_x64[
//     NEXTEST_VMM_TESTS_ARCHIVE_LINUX_X64_MUSL,
//     LINUX_DIRECT_TEST_KERNEL_X64,
//     LINUX_DIRECT_TEST_INITRD_X64,
//     PIPETTE_LINUX_X64_MUSL,
//     OPENVMM_LINUX_X64_MUSL
// ])]
// async fn nested_petri(
//     config: PetriVmBuilder<OpenVmmPetriBackend>,
//     (archive, kernel, initrd, pipette, openvmm): (
//         ResolvedArtifact<NEXTEST_VMM_TESTS_ARCHIVE_LINUX_X64_MUSL>,
//         ResolvedArtifact<LINUX_DIRECT_TEST_KERNEL_X64>,
//         ResolvedArtifact<LINUX_DIRECT_TEST_INITRD_X64>,
//         ResolvedArtifact<PIPETTE_LINUX_X64_MUSL>,
//         ResolvedArtifact<OPENVMM_LINUX_X64_MUSL>,
//     ),
// ) -> anyhow::Result<()> {
//     let (vm, agent) = config
//         .with_nested_test(archive, "tests", "multiarch::openvmm_linux_x64_boot")
//         .with_nested_test_artifact(kernel)
//         .with_nested_test_artifact(initrd)
//         .with_nested_test_artifact(pipette)
//         .with_nested_test_artifact(openvmm)
//         .with_nested_virt()
//         .run()
//         .await?;

//     vm.run_nested_test(&agent).await?;
//     agent.power_off().await?;
//     vm.wait_for_clean_teardown().await?;
//     Ok(())
// }

pub(crate) async fn nested_vm_host<T: PetriVmmBackend>(
    config: PetriVmBuilder<T>,
) -> anyhow::Result<()> {
    let (vm, agent) = config.with_nested_virt().run().await?;

    vm.run_nested_test(&agent, Default::default()).await?;
    agent.power_off().await?;
    vm.wait_for_clean_teardown().await?;
    Ok(())
}
