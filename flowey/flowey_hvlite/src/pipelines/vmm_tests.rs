// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use flowey::node::prelude::ReadVar;
use flowey::pipeline::prelude::*;
use flowey_lib_hvlite::run_cargo_build::common::CommonArch;
use flowey_lib_hvlite::run_cargo_build::common::CommonTriple;
use std::path::PathBuf;
use vmm_test_images::KnownTestArtifacts;

#[derive(clap::ValueEnum, Copy, Clone)]
pub enum VmmTestTargetCli {
    /// Windows Aarch64
    WindowsAarch64,
    /// Windows X64
    WindowsX64,
    /// Linux X64
    LinuxX64,
}

/// Build everything needed and run the VMM tests
#[derive(clap::Args)]
pub struct VmmTestsCli {
    /// Specify what target to build the VMM tests for
    ///
    /// If not specified, defaults to the current host target.
    #[clap(long)]
    target: Option<VmmTestTargetCli>,

    /// Directory for the output artifacts
    #[clap(long)]
    dir: Option<PathBuf>,

    /// Custom test filter (overrides selections based on other flags)
    #[clap(long)]
    filter: Option<String>,
    /// Custom artifact list (overrides selections based on other flags)
    #[clap(long)]
    artifacts: Vec<KnownTestArtifacts>,
    /// pass `--verbose` to cargo
    #[clap(long)]
    verbose: bool,
    /// Automatically install any missing required dependencies.
    #[clap(long)]
    install_missing_deps: bool,

    /// Enable Hyper-V TDX tests
    #[clap(long)]
    tdx: bool,
    /// Enable Hyper-V VBS tests
    #[clap(long)]
    hyperv_vbs: bool,
    /// Use unstable WHP interfaces
    #[clap(long)]
    unstable_whp: bool,
    /// Release build instead of debug build
    #[clap(long)]
    release: bool,
    /// Skip Windows guest tests
    #[clap(long)]
    no_windows: bool,
    /// Skip Ubuntu guest tests
    #[clap(long)]
    no_ubuntu: bool,
    /// Skip FreeBSD guest tests
    #[clap(long)]
    no_freebsd: bool,
    /// Skip OpenHCL tests
    #[clap(long)]
    no_openhcl: bool,
    /// Skip OpenVMM tests
    #[clap(long)]
    no_openvmm: bool,
    /// Skip Hyper-V tests
    #[clap(long)]
    no_hyperv: bool,
    /// Build only, do not run
    #[clap(long)]
    build_only: bool,
    /// Copy extras to output dir (symbols, etc)
    #[clap(long)]
    pub copy_extras: bool,
}

impl IntoPipeline for VmmTestsCli {
    fn into_pipeline(self, backend_hint: PipelineBackendHint) -> anyhow::Result<Pipeline> {
        if !matches!(backend_hint, PipelineBackendHint::Local) {
            anyhow::bail!("vmm-tests is for local use only")
        }

        let Self {
            target,
            dir,
            filter,
            artifacts,
            verbose,
            install_missing_deps,
            tdx,
            hyperv_vbs,
            unstable_whp,
            release,
            no_windows,
            mut no_ubuntu,
            no_freebsd,
            mut no_openhcl,
            no_openvmm,
            no_hyperv,
            build_only,
            copy_extras,
        } = self;

        let openvmm_repo = flowey_lib_common::git_checkout::RepoSource::ExistingClone(
            ReadVar::from_static(crate::repo_root()),
        );

        let mut pipeline = Pipeline::new();

        let host_target = match (
            FlowArch::host(backend_hint),
            FlowPlatform::host(backend_hint),
        ) {
            (FlowArch::Aarch64, FlowPlatform::Windows) => VmmTestTargetCli::WindowsAarch64,
            (FlowArch::X86_64, FlowPlatform::Windows) => VmmTestTargetCli::WindowsX64,
            (FlowArch::X86_64, FlowPlatform::Linux(_)) => VmmTestTargetCli::LinuxX64,
            _ => anyhow::bail!("unsupported host"),
        };

        if !matches!(host_target, VmmTestTargetCli::LinuxX64) {
            log::warn!(
                "Cannot build for linux on windows. Skipping all tests that rely on linux artifacts."
            );
            no_ubuntu = true;
            no_openhcl = true;
        }

        let target = match target.unwrap_or(host_target) {
            VmmTestTargetCli::WindowsAarch64 => CommonTriple::AARCH64_WINDOWS_MSVC,
            VmmTestTargetCli::WindowsX64 => CommonTriple::X86_64_WINDOWS_MSVC,
            VmmTestTargetCli::LinuxX64 => CommonTriple::X86_64_LINUX_GNU,
        };
        let arch = target.common_arch().unwrap();

        let nextest_filter_expr = if let Some(filter) = filter {
            filter
        } else {
            let mut filter = "all()".to_string();
            if !tdx {
                filter.push_str(" & !test(tdx)");
            }
            if !hyperv_vbs {
                filter.push_str(" & !(test(vbs) & test(hyperv))");
            }
            if no_ubuntu {
                filter.push_str(" & !test(ubuntu)");
            }
            if no_windows {
                filter.push_str(" & !test(windows)");
            }
            if no_freebsd {
                filter.push_str(" & !test(freebsd)");
            }
            if no_openhcl {
                filter.push_str(" & !test(openhcl)");
            }
            if no_openvmm {
                filter.push_str(" & !test(openvmm)");
            }
            if no_hyperv {
                filter.push_str(" & !test(hyperv)");
            }
            filter
        };

        let test_artifacts = if artifacts.is_empty() {
            match arch {
                CommonArch::X86_64 => {
                    let mut artifacts = Vec::new();

                    if !no_windows && (tdx || hyperv_vbs) {
                        artifacts.push(KnownTestArtifacts::Gen2WindowsDataCenterCore2025X64Vhd);
                    }
                    if !no_ubuntu {
                        artifacts.push(KnownTestArtifacts::Ubuntu2204ServerX64Vhd);
                    }
                    if !no_windows {
                        artifacts.extend_from_slice(&[
                            KnownTestArtifacts::Gen1WindowsDataCenterCore2022X64Vhd,
                            KnownTestArtifacts::Gen2WindowsDataCenterCore2022X64Vhd,
                        ]);
                    }
                    if !no_freebsd {
                        artifacts.extend_from_slice(&[
                            KnownTestArtifacts::FreeBsd13_2X64Vhd,
                            KnownTestArtifacts::FreeBsd13_2X64Iso,
                        ]);
                    }
                    if !(no_windows && no_ubuntu) {
                        artifacts.push(KnownTestArtifacts::VmgsWithBootEntry);
                    }

                    artifacts
                }
                CommonArch::Aarch64 => {
                    let mut artifacts = Vec::new();

                    if !no_ubuntu {
                        artifacts.push(KnownTestArtifacts::Ubuntu2404ServerAarch64Vhd);
                    }
                    if !no_windows {
                        artifacts.push(KnownTestArtifacts::Windows11EnterpriseAarch64Vhdx);
                    }
                    if !(no_windows && no_ubuntu) {
                        artifacts.push(KnownTestArtifacts::VmgsWithBootEntry);
                    }

                    artifacts
                }
            }
        } else {
            artifacts
        };

        pipeline
            .new_job(
                FlowPlatform::host(backend_hint),
                FlowArch::host(backend_hint),
                "build vmm test dependencies",
            )
            .dep_on(|_| flowey_lib_hvlite::_jobs::cfg_versions::Request {})
            .dep_on(
                |_| flowey_lib_hvlite::_jobs::cfg_hvlite_reposource::Params {
                    hvlite_repo_source: openvmm_repo.clone(),
                },
            )
            .dep_on(|_| flowey_lib_hvlite::_jobs::cfg_common::Params {
                local_only: Some(flowey_lib_hvlite::_jobs::cfg_common::LocalOnlyParams {
                    interactive: true,
                    auto_install: install_missing_deps,
                    force_nuget_mono: false,
                    external_nuget_auth: false,
                    ignore_rust_version: true,
                }),
                verbose: ReadVar::from_static(verbose),
                locked: false,
                deny_warnings: false,
            })
            .dep_on(
                |ctx| flowey_lib_hvlite::_jobs::local_build_and_run_nextest_vmm_tests::Params {
                    target,
                    test_content_dir: dir,
                    nextest_filter_expr: Some(nextest_filter_expr),
                    test_artifacts,
                    unstable_whp,
                    release,
                    build_only,
                    copy_extras,
                    done: ctx.new_done_handle(),
                },
            )
            .finish();

        Ok(pipeline)
    }
}
