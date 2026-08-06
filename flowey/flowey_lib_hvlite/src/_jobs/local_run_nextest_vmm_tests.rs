// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! A local-only job that builds everything needed and runs the VMM tests

use crate::_jobs::local_build_and_run_nextest_vmm_tests::build_test_label;
use crate::_jobs::local_build_and_run_nextest_vmm_tests::init_artifacts_dir;
use crate::common::CommonTriple;
use crate::install_vmm_tests_deps::VmmTestsDepSelections;
use flowey::node::prelude::*;
use vmm_test_images::KnownTestArtifacts;

flowey_request! {
    pub struct Params {
        pub target: CommonTriple,
        /// Test content dir with all artifacts and repo root
        pub test_content_dir: PathBuf,
        /// Skip the interactive VHD download prompt
        pub skip_vhd_prompt: bool,
        pub nextest_profile: crate::run_cargo_nextest_run::NextestProfile,
        pub reuse_prepped_vhds: bool,
        /// Optional: incubator profile path. When set, tests run inside
        /// an emulated VM instead of on the host.
        pub incubator_profile: Option<PathBuf>,

        pub filter: String,
        pub artifacts: Vec<KnownTestArtifacts>,
        pub deps: VmmTestsDepSelections,
        pub prep_steps_variants: Vec<String>,

        pub done: WriteVar<SideEffect>,
    }
}

new_simple_flow_node!(struct Node);

impl SimpleFlowNode for Node {
    type Request = Params;

    fn imports(ctx: &mut ImportCtx<'_>) {
        ctx.import::<crate::_jobs::consume_and_test_nextest_vmm_tests_archive::Node>();
        ctx.import::<crate::download_openvmm_vmm_tests_artifacts::Node>();
    }

    fn process_request(request: Self::Request, ctx: &mut NodeCtx<'_>) -> anyhow::Result<()> {
        let Params {
            target,
            test_content_dir,
            skip_vhd_prompt,
            nextest_profile,
            reuse_prepped_vhds,
            incubator_profile,
            filter,
            artifacts,
            deps,
            prep_steps_variants,
            done,
        } = request;

        let test_content_dir = test_content_dir.absolute()?;

        let target_triple = target.as_triple();
        let test_label = build_test_label(&target_triple);

        init_artifacts_dir(ctx, &test_content_dir, skip_vhd_prompt)?;

        ctx.req(
            crate::_jobs::consume_and_test_nextest_vmm_tests_archive::Params {
                junit_test_label: test_label,
                nextest_vmm_tests_archive: None,
                target: target_triple,
                nextest_profile,
                nextest_filter_expr: Some(filter),
                vmm_tests_dep_artifacts: None,
                test_artifacts: artifacts,
                prep_steps_variants,
                hugetlb_2mb_overcommit_pages: None,
                incubator_profile,
                fail_job_on_test_fail: true,
                artifact_dir: None,
                test_content_dir: Some(ReadVar::from_static(test_content_dir)),
                reuse_prepped_vhds,
                disable_remote_artifacts: false,
                test_content_dir_as_repo_root: true,
                needs_release_igvm: false, // TODO: this is ignored
                deps: Some(deps),
                done,
            },
        );

        Ok(())
    }
}
