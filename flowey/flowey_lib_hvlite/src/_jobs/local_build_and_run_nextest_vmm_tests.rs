// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! A local-only job that builds everything needed and runs the VMM tests

use crate::_jobs::build_and_publish_openhcl_igvm_from_recipe::OpenhclIgvmBuildParams;
use crate::_jobs::consume_and_test_nextest_vmm_tests_archive::VmmTestsDepArtifacts;
use crate::build_openhcl_igvm_from_recipe::OpenhclIgvmRecipe;
use crate::build_openhcl_igvm_from_recipe::OpenhclIgvmRecipeDetailsLocalOnly;
use crate::build_openhcl_igvm_from_recipe::OpenhclIgvmRecipeType;
use crate::build_openvmm_hcl::OpenvmmHclBuildProfile;
use crate::build_tpm_guest_tests::TpmGuestTestsOutput;
use crate::common::CommonArch;
use crate::common::CommonPlatform;
use crate::common::CommonProfile;
use crate::common::CommonTriple;
use crate::install_vmm_tests_deps::VmmTestsDepSelections;
use flowey::node::prelude::*;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use vmm_test_images::KnownTestArtifacts;

#[derive(Serialize, Deserialize)]
pub struct VmmTestSelections {
    /// Test filter
    pub filter: String,
    /// List of artifacts to download
    pub artifacts: Vec<KnownTestArtifacts>,
    /// List of artifacts to build
    pub build: BuildSelections,
    /// Dependencies to install
    pub deps: VmmTestsDepSelections,
    /// Whether to download release IGVM files from GitHub
    pub needs_release_igvm: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BuildSelections {
    pub openhcl_standard: bool,
    pub openhcl_standard_dev: bool,
    pub openhcl_cvm: bool,
    pub openhcl_linux_direct: bool,
    pub openvmm: bool,
    pub openvmm_vhost: bool,
    pub pipette_windows: bool,
    pub pipette_linux: bool,
    pub prep_steps_standard: bool,
    pub prep_steps_no_vmbus: bool,
    pub guest_test_uefi: bool,
    pub tmks: bool,
    pub tmk_vmm_windows: bool,
    pub tmk_vmm_linux: bool,
    pub vmgstool: bool,
    pub vmgstool_dev: bool,
    pub tpm_guest_tests_windows: bool,
    pub tpm_guest_tests_linux: bool,
    pub test_igvm_agent_rpc_server: bool,
}

flowey_request! {
    pub struct Params {
        pub target: CommonTriple,

        /// Toolchain platform to use when cross-compiling Windows *guest*
        /// payloads (e.g. pipette). On a non-WSL Linux build host this is
        /// [`CommonPlatform::WindowsGnu`], since the MSVC toolchain is
        /// unavailable there; otherwise it is [`CommonPlatform::WindowsMsvc`].
        pub windows_guest_platform: CommonPlatform,

        pub test_content_dir: PathBuf,

        pub selections: VmmTestSelections,

        /// Release build instead of debug build
        pub release: bool,

        /// Whether to run the tests or just build and archive
        pub build_only: bool,
        /// Copy extras to output dir (symbols, etc)
        pub copy_extras: bool,

        /// Optional: provide a custom kernel modules cpio or directory for initrd layering
        pub custom_kernel_modules: Option<PathBuf>,
        /// Optional: provide a custom kernel image to embed in IGVM (forces UEFI)
        pub custom_kernel: Option<PathBuf>,

        /// Skip the interactive VHD download prompt
        pub skip_vhd_prompt: bool,

        pub nextest_profile: crate::run_cargo_nextest_run::NextestProfile,

        pub reuse_prepped_vhds: bool,

        pub disable_secure_avic: bool,

        pub repetitions: u64,

        /// Optional: incubator profile path. When set, tests run inside
        /// an emulated VM instead of on the host.
        pub incubator_profile: Option<PathBuf>,

        pub done: WriteVar<SideEffect>,
    }
}

new_simple_flow_node!(struct Node);

impl SimpleFlowNode for Node {
    type Request = Params;

    fn imports(ctx: &mut ImportCtx<'_>) {
        ctx.import::<crate::build_guest_test_uefi::Node>();
        ctx.import::<crate::build_incubator::Node>();
        ctx.import::<crate::build_nextest_vmm_tests::Node>();
        ctx.import::<crate::build_openhcl_igvm_from_recipe::Node>();
        ctx.import::<crate::build_openvmm::Node>();
        ctx.import::<crate::build_openvmm_vhost::Node>();
        ctx.import::<crate::build_pipette::Node>();
        ctx.import::<crate::build_prep_steps::Node>();
        ctx.import::<crate::build_tmks::Node>();
        ctx.import::<crate::build_tmk_vmm::Node>();
        ctx.import::<crate::build_tpm_guest_tests::Node>();
        ctx.import::<crate::build_test_igvm_agent_rpc_server::Node>();
        ctx.import::<crate::download_openvmm_vmm_tests_artifacts::Node>();
        ctx.import::<crate::init_vmm_tests_content_dir::Node>();
        ctx.import::<crate::test_nextest_vmm_tests_archive::Node>();
        ctx.import::<crate::build_vmgstool::Node>();
        ctx.import::<crate::_jobs::build_and_publish_openhcl_igvm_from_recipe::Node>();
        ctx.import::<crate::_jobs::consume_and_test_nextest_vmm_tests_archive::Node>();
    }

    fn process_request(request: Self::Request, ctx: &mut NodeCtx<'_>) -> anyhow::Result<()> {
        let Params {
            target,
            windows_guest_platform,
            test_content_dir,
            selections,
            release,
            build_only,
            copy_extras,
            custom_kernel_modules,
            custom_kernel,
            skip_vhd_prompt,
            nextest_profile,
            reuse_prepped_vhds,
            disable_secure_avic,
            repetitions,
            incubator_profile,
            done,
        } = request;

        let test_content_dir = test_content_dir.absolute()?;
        let custom_kernel_modules_abs = custom_kernel_modules.map(|p| p.absolute()).transpose()?;
        let custom_kernel_abs = custom_kernel.map(|p| p.absolute()).transpose()?;

        let target_triple = target.as_triple();
        let arch = target.common_arch().unwrap();
        let test_label = build_test_label(&target_triple);

        let mut copy_to_dir = Vec::new();
        let extras_dir = Path::new("extras");

        let VmmTestSelections {
            filter: nextest_filter_expr,
            artifacts: test_artifacts,
            build,
            deps,
            needs_release_igvm,
        } = selections;

        let build_openhcl = build.openhcl_standard
            || build.openhcl_standard_dev
            || build.openhcl_cvm
            || build.openhcl_linux_direct;

        // Some things can only be built on linux
        if !matches!(ctx.platform(), FlowPlatform::Linux(_))
            && (build_openhcl
                || build.pipette_linux
                || build.openvmm_vhost
                || build.tmk_vmm_linux
                || build.tpm_guest_tests_linux)
        {
            anyhow::bail!(
                "Selected tests require artifacts that can only be built on linux. Try building from WSL2."
            );
        }

        let register_openhcl_igvm_files = if build_openhcl {
            let openvmm_hcl_profile = if release {
                OpenvmmHclBuildProfile::OpenvmmHclShip
            } else {
                OpenvmmHclBuildProfile::Debug
            };
            let mut openhcl_recipes = Vec::new();
            match arch {
                CommonArch::X86_64 => {
                    if build.openhcl_standard {
                        openhcl_recipes.push(OpenhclIgvmRecipe::X64);
                    }
                    if build.openhcl_standard_dev {
                        openhcl_recipes.push(OpenhclIgvmRecipe::X64Devkern);
                    }
                    if build.openhcl_cvm {
                        openhcl_recipes.push(OpenhclIgvmRecipe::X64Cvm);
                    }
                    if build.openhcl_linux_direct {
                        openhcl_recipes.push(OpenhclIgvmRecipe::X64TestLinuxDirect);
                    }
                }
                CommonArch::Aarch64 => {
                    if build.openhcl_standard {
                        openhcl_recipes.push(OpenhclIgvmRecipe::Aarch64);
                    }
                    if build.openhcl_standard_dev {
                        openhcl_recipes.push(OpenhclIgvmRecipe::Aarch64Devkern);
                    }
                }
            };
            let (read_igvm_files, write_igvm_files): (BTreeMap<_, _>, _) = openhcl_recipes
                .into_iter()
                .map(|recipe| {
                    let custom_recipe =
                        if custom_kernel_modules_abs.is_some() || custom_kernel_abs.is_some() {
                            let mut details = recipe.recipe_details(release);
                            if custom_kernel_abs.is_some() {
                                details.with_uefi = true;
                            }
                            assert!(details.local_only.is_none());
                            details.local_only = Some(OpenhclIgvmRecipeDetailsLocalOnly {
                                openvmm_hcl_no_strip: false,
                                openhcl_initrd_extra_params: None,
                                custom_openvmm_hcl: None,
                                custom_openhcl_boot: None,
                                custom_kernel: custom_kernel_abs.clone(),
                                custom_sidecar: None,
                                custom_extra_rootfs: vec![],
                            });
                            OpenhclIgvmRecipeType::LocalOnlyCustom(details)
                        } else {
                            OpenhclIgvmRecipeType::WellKnown(recipe.clone())
                        };

                    let (read_igvm, write_igvm) = ctx.new_var();
                    let (read_igvm_extras, write_igvm_extras) = ctx.new_var();
                    (
                        (recipe.clone(), (read_igvm, read_igvm_extras)),
                        (
                            OpenhclIgvmBuildParams {
                                profile: openvmm_hcl_profile,
                                recipe: custom_recipe,
                                custom_target: None,
                                extra_features: BTreeSet::new(),
                                release_cfg: release,
                                confidential_debug: true,
                                disable_secure_avic,
                            },
                            write_igvm,
                            write_igvm_extras,
                        ),
                    )
                })
                .unzip();

            let (read_igvm_files, read_igvm_extras): (BTreeMap<_, _>, BTreeMap<_, _>) =
                read_igvm_files
                    .into_iter()
                    .map(|(recipe, (igvm_file, igvm_extras))| {
                        ((recipe.clone(), igvm_file), (recipe, igvm_extras))
                    })
                    .unzip();

            ctx.req(
                crate::_jobs::build_and_publish_openhcl_igvm_from_recipe::Params {
                    igvm_files: write_igvm_files,
                    artifact_openhcl_verify_size_baseline: None,
                },
            );

            let openhcl_extras_dir = extras_dir.join("openhcl");
            for (recipe, extras) in read_igvm_extras.into_iter() {
                if copy_extras {
                    let dir = openhcl_extras_dir.join(recipe.non_production_tag());
                    copy_to_dir.extend_from_slice(&[
                        (dir.clone(), extras.map(ctx, |x| Some(x.openvmm_hcl.bin))),
                        (dir.clone(), extras.map(ctx, |x| x.openvmm_hcl.dbg)),
                        (dir.clone(), extras.map(ctx, |x| Some(x.openhcl_boot.bin))),
                        (dir.clone(), extras.map(ctx, |x| Some(x.openhcl_boot.dbg))),
                        (dir.clone(), extras.map(ctx, |x| x.sidecar.map(|y| y.bin))),
                        (dir.clone(), extras.map(ctx, |x| x.sidecar.map(|y| y.dbg))),
                    ]);
                } else {
                    extras.claim_unused(ctx);
                }
            }

            read_igvm_files
        } else {
            BTreeMap::new()
        };

        let register_openvmm = build.openvmm.then(|| {
            let output = ctx.reqv(|v| crate::build_openvmm::Request {
                params: crate::build_openvmm::OpenvmmBuildParams {
                    target: target.clone(),
                    profile: CommonProfile::from_release(release),
                    // FIXME: this relies on openvmm default features
                    features: [].into(),
                },
                version: None,
                openvmm: v,
            });
            if copy_extras {
                copy_to_dir.push((
                    extras_dir.to_owned(),
                    output.map(ctx, |x| match x {
                        crate::build_openvmm::OpenvmmOutput::WindowsBin { exe: _, pdb } => pdb,
                        crate::build_openvmm::OpenvmmOutput::LinuxBin { bin: _, dbg } => Some(dbg),
                    }),
                ));
            }
            output
        });

        let register_openvmm_vhost = build.openvmm_vhost.then(|| {
            ctx.reqv(|v| crate::build_openvmm_vhost::Request {
                params: crate::build_openvmm_vhost::OpenvmmVhostBuildParams {
                    target: target.clone(),
                    profile: CommonProfile::from_release(release),
                },
                openvmm_vhost: v,
            })
        });

        let register_pipette_windows = build.pipette_windows.then(|| {
            let output = ctx.reqv(|v| crate::build_pipette::Request {
                target: CommonTriple::Common {
                    arch,
                    platform: windows_guest_platform,
                },
                profile: CommonProfile::from_release(release),
                pipette: v,
            });
            if copy_extras {
                copy_to_dir.push((
                    extras_dir.to_owned(),
                    output.map(ctx, |x| match x {
                        crate::build_pipette::PipetteOutput::WindowsBin { exe: _, pdb } => pdb,
                        _ => unreachable!(),
                    }),
                ));
            }
            output
        });

        let register_pipette_linux_musl = build.pipette_linux.then(|| {
            let output = ctx.reqv(|v| crate::build_pipette::Request {
                target: CommonTriple::Common {
                    arch,
                    platform: CommonPlatform::LinuxMusl,
                },
                profile: CommonProfile::from_release(release),
                pipette: v,
            });
            if copy_extras {
                copy_to_dir.push((
                    extras_dir.to_owned(),
                    output.map(ctx, |x| {
                        Some(match x {
                            crate::build_pipette::PipetteOutput::LinuxBin { bin: _, dbg } => dbg,
                            _ => unreachable!(),
                        })
                    }),
                ));
            }
            output
        });

        let register_guest_test_uefi = build.guest_test_uefi.then(|| {
            let output = ctx.reqv(|v| crate::build_guest_test_uefi::Request {
                arch,
                profile: CommonProfile::from_release(release),
                guest_test_uefi: v,
            });
            if copy_extras {
                copy_to_dir.push((extras_dir.to_owned(), output.map(ctx, |x| Some(x.efi))));
                copy_to_dir.push((extras_dir.to_owned(), output.map(ctx, |x| Some(x.pdb))));
            }
            output
        });

        let register_tmks = build.tmks.then(|| {
            let output = ctx.reqv(|v| crate::build_tmks::Request {
                arch,
                profile: CommonProfile::from_release(release),
                tmks: v,
            });
            if copy_extras {
                copy_to_dir.push((extras_dir.to_owned(), output.map(ctx, |x| Some(x.dbg))));
            }
            output
        });

        let register_tpm_guest_tests_windows = build.tpm_guest_tests_windows.then(|| {
            let output = ctx.reqv(|v| crate::build_tpm_guest_tests::Request {
                target: CommonTriple::Common {
                    arch,
                    platform: windows_guest_platform,
                },
                profile: CommonProfile::from_release(release),
                tpm_guest_tests: v,
            });

            if copy_extras {
                copy_to_dir.push((
                    extras_dir.to_owned(),
                    output.map(ctx, |x| match x {
                        TpmGuestTestsOutput::WindowsBin { pdb, .. } => pdb.clone(),
                        TpmGuestTestsOutput::LinuxBin { .. } => unreachable!(),
                    }),
                ));
            }
            output
        });

        let register_tpm_guest_tests_linux = build.tpm_guest_tests_linux.then(|| {
            let output = ctx.reqv(|v| crate::build_tpm_guest_tests::Request {
                target: CommonTriple::Common {
                    arch,
                    platform: CommonPlatform::LinuxGnu,
                },
                profile: CommonProfile::from_release(release),
                tpm_guest_tests: v,
            });

            if copy_extras {
                copy_to_dir.push((
                    extras_dir.to_owned(),
                    output.map(ctx, |x| {
                        Some(match x {
                            TpmGuestTestsOutput::LinuxBin { dbg, .. } => dbg.clone(),
                            TpmGuestTestsOutput::WindowsBin { .. } => unreachable!(),
                        })
                    }),
                ));
            }
            output
        });

        let register_test_igvm_agent_rpc_server = build.test_igvm_agent_rpc_server.then(|| {
            let output = ctx.reqv(|v| crate::build_test_igvm_agent_rpc_server::Request {
                target: CommonTriple::Common {
                    arch,
                    platform: CommonPlatform::WindowsMsvc,
                },
                profile: CommonProfile::from_release(release),
                test_igvm_agent_rpc_server: v,
            });

            if copy_extras {
                copy_to_dir.push((extras_dir.to_owned(), output.map(ctx, |x| x.pdb.clone())));
            }
            output
        });

        let register_tmk_vmm = build.tmk_vmm_windows.then(|| {
            let output = ctx.reqv(|v| crate::build_tmk_vmm::Request {
                target: CommonTriple::Common {
                    arch,
                    platform: CommonPlatform::WindowsMsvc,
                },
                profile: CommonProfile::from_release(release),
                tmk_vmm: v,
            });
            if copy_extras {
                copy_to_dir.push((
                    extras_dir.to_owned(),
                    output.map(ctx, |x| match x {
                        crate::build_tmk_vmm::TmkVmmOutput::WindowsBin { exe: _, pdb } => pdb,
                        _ => unreachable!(),
                    }),
                ));
            }
            output
        });

        let register_tmk_vmm_linux_musl = build.tmk_vmm_linux.then(|| {
            let output = ctx.reqv(|v| crate::build_tmk_vmm::Request {
                target: CommonTriple::Common {
                    arch,
                    platform: CommonPlatform::LinuxMusl,
                },
                profile: CommonProfile::from_release(release),
                tmk_vmm: v,
            });
            if copy_extras {
                copy_to_dir.push((
                    extras_dir.to_owned(),
                    output.map(ctx, |x| {
                        Some(match x {
                            crate::build_tmk_vmm::TmkVmmOutput::LinuxBin { bin: _, dbg } => dbg,
                            _ => unreachable!(),
                        })
                    }),
                ));
            }
            output
        });

        let needs_prep_steps = build.prep_steps_standard || build.prep_steps_no_vmbus;
        let mut prep_steps_variants: Vec<String> = Vec::new();
        if build.prep_steps_standard {
            prep_steps_variants.push("standard".into());
        }
        if build.prep_steps_no_vmbus {
            prep_steps_variants.push("no-vmbus".into());
        }

        let register_prep_steps = needs_prep_steps.then(|| {
            let output = ctx.reqv(|v| crate::build_prep_steps::Request {
                target: target.clone(),
                profile: CommonProfile::from_release(release),
                prep_steps: v,
            });

            if copy_extras {
                copy_to_dir.push((
                    extras_dir.to_owned(),
                    output.map(ctx, |x| match x {
                        crate::build_prep_steps::PrepStepsOutput::WindowsBin { exe: _, pdb } => pdb,
                        crate::build_prep_steps::PrepStepsOutput::LinuxBin { bin: _, dbg } => {
                            Some(dbg)
                        }
                    }),
                ));
            }
            output
        });

        let mut build_vmgstool = |with_test_helpers| {
            let output = ctx.reqv(|v| crate::build_vmgstool::Request {
                target: target.clone(),
                profile: CommonProfile::from_release(release),
                with_crypto: true,
                with_test_helpers,
                vmgstool: v,
            });
            if copy_extras {
                copy_to_dir.push((
                    extras_dir.to_owned(),
                    output.map(ctx, |x| match x {
                        crate::build_vmgstool::VmgstoolOutput::WindowsBin { exe: _, pdb } => pdb,
                        crate::build_vmgstool::VmgstoolOutput::LinuxBin { bin: _, dbg } => {
                            Some(dbg)
                        }
                    }),
                ));
            }
            output
        };

        let register_vmgstool = build.vmgstool.then(|| build_vmgstool(false));

        let register_vmgstool_dev = build.vmgstool_dev.then(|| build_vmgstool(true));

        let register_incubator = incubator_profile.is_some().then(|| {
            let host_arch = match ctx.arch() {
                FlowArch::X86_64 => CommonArch::X86_64,
                FlowArch::Aarch64 => CommonArch::Aarch64,
                other => {
                    panic!("unsupported host architecture for incubator: {other:?}")
                }
            };
            let incubator_target = CommonTriple::Common {
                arch: host_arch,
                platform: CommonPlatform::LinuxGnu,
            };
            let output = ctx.reqv(|v| crate::build_incubator::Request {
                target: incubator_target,
                profile: if release {
                    CommonProfile::Release
                } else {
                    CommonProfile::Debug
                },
                incubator: v,
            });
            if copy_extras {
                copy_to_dir.push((
                    extras_dir.to_owned(),
                    output.map(ctx, |x| {
                        let crate::build_incubator::IncubatorOutput { bin: _, dbg } = x;
                        Some(dbg)
                    }),
                ));
            }
            output
        });

        let nextest_archive = ctx.reqv(|v| crate::build_nextest_vmm_tests::Request {
            target: target.as_triple(),
            profile: CommonProfile::from_release(release),
            build_mode: crate::build_nextest_vmm_tests::BuildNextestVmmTestsMode::Archive(v),
        });
        let nextest_archive_file = Path::new("vmm_tests.tar.zst");
        copy_to_dir.push((
            nextest_archive_file.to_owned(),
            nextest_archive.map(ctx, |x| Some(x.archive_file)),
        ));

        let mut side_effects = Vec::new();

        side_effects.push(
            ctx.emit_rust_step("copy additional files to test content dir", |ctx| {
                let copy_to_dir = copy_to_dir
                    .into_iter()
                    .map(|(dst, src)| (dst, src.claim(ctx)))
                    .collect::<Vec<_>>();
                let test_content_dir = test_content_dir.clone();

                move |rt| {
                    for (dst, src) in copy_to_dir {
                        let src = rt.read(src);

                        if let Some(src) = src {
                            // TODO: specify files names for everything
                            let dst = if dst.starts_with("extras") {
                                test_content_dir
                                    .join(dst)
                                    .join(src.file_name().context("no file name")?)
                            } else {
                                test_content_dir.join(dst)
                            };

                            fs_err::create_dir_all(dst.parent().context("no parent")?)?;
                            fs_err::copy(src, dst)?;
                        }
                    }

                    Ok(())
                }
            }),
        );

        let vmm_tests_dep_artifacts = VmmTestsDepArtifacts {
            incubator: register_incubator,
            openvmm: register_openvmm,
            openvmm_vhost: register_openvmm_vhost,
            pipette_windows: register_pipette_windows,
            pipette_linux_musl: register_pipette_linux_musl,
            guest_test_uefi: register_guest_test_uefi,
            prep_steps: register_prep_steps,
            openhcl_standard: register_openhcl_igvm_files
                .get(match arch {
                    CommonArch::X86_64 => &OpenhclIgvmRecipe::X64,
                    CommonArch::Aarch64 => &OpenhclIgvmRecipe::Aarch64,
                })
                .cloned(),
            openhcl_standard_dev: register_openhcl_igvm_files
                .get(match arch {
                    CommonArch::X86_64 => &OpenhclIgvmRecipe::X64Devkern,
                    CommonArch::Aarch64 => &OpenhclIgvmRecipe::Aarch64Devkern,
                })
                .cloned(),
            openhcl_cvm: matches!(arch, CommonArch::X86_64)
                .then(|| {
                    register_openhcl_igvm_files
                        .get(&OpenhclIgvmRecipe::X64Cvm)
                        .cloned()
                })
                .flatten(),
            openhcl_linux_direct: matches!(arch, CommonArch::X86_64)
                .then(|| {
                    register_openhcl_igvm_files
                        .get(&OpenhclIgvmRecipe::X64TestLinuxDirect)
                        .cloned()
                })
                .flatten(),
            tmks: register_tmks,
            tmk_vmm: register_tmk_vmm,
            tmk_vmm_linux_musl: register_tmk_vmm_linux_musl,
            vmgstool: register_vmgstool,
            vmgstool_dev: register_vmgstool_dev,
            tpm_guest_tests_windows: register_tpm_guest_tests_windows,
            tpm_guest_tests_linux: register_tpm_guest_tests_linux,
            test_igvm_agent_rpc_server: register_test_igvm_agent_rpc_server,
        };

        if build_only {
            side_effects.push(ctx.reqv(|v| crate::init_vmm_tests_content_dir::Request {
                test_content_dir: ReadVar::from_static(test_content_dir),
                vmm_tests_target: target_triple,
                vmm_tests_dep_artifacts,
                is_repo_root: true,
                needs_release_igvm,
                done: v,
            }));
        } else {
            init_artifacts_dir(ctx, &test_content_dir, skip_vhd_prompt)?;

            side_effects.push(ctx.reqv(|v| {
                crate::_jobs::consume_and_test_nextest_vmm_tests_archive::Params {
                    junit_test_label: test_label,
                    nextest_vmm_tests_archive: Some(nextest_archive),
                    target: target_triple,
                    nextest_profile,
                    nextest_filter_expr: Some(nextest_filter_expr),
                    vmm_tests_dep_artifacts: Some(vmm_tests_dep_artifacts),
                    test_artifacts,
                    prep_steps_variants,
                    hugetlb_2mb_overcommit_pages: None,
                    incubator_profile,
                    fail_job_on_test_fail: true,
                    test_content_dir: Some(ReadVar::from_static(test_content_dir)),
                    reuse_prepped_vhds,
                    disable_remote_artifacts: false,
                    test_content_dir_as_repo_root: true,
                    needs_release_igvm,
                    deps: Some(deps),
                    repetitions,
                    done: v,
                }
            }));
        }

        ctx.emit_side_effect_step(side_effects, [done]);

        Ok(())
    }
}

pub(crate) fn build_test_label(target: &target_lexicon::Triple) -> String {
    let arch = CommonArch::from_triple(target).unwrap();
    let arch_tag = match arch {
        CommonArch::X86_64 => "x64",
        CommonArch::Aarch64 => "aarch64",
    };
    let platform_tag = match target.operating_system {
        target_lexicon::OperatingSystem::Windows => "windows",
        target_lexicon::OperatingSystem::Linux => "linux",
        _ => unreachable!(),
    };
    format!("{arch_tag}-{platform_tag}-vmm-tests")
}

pub(crate) fn init_artifacts_dir(
    ctx: &mut NodeCtx<'_>,
    test_content_dir: &Path,
    skip_vhd_prompt: bool,
) -> anyhow::Result<()> {
    let vmm_test_artifacts_dir = test_content_dir.join("images");
    fs_err::create_dir_all(&vmm_test_artifacts_dir)?;
    ctx.config(crate::download_openvmm_vmm_tests_artifacts::Config {
        custom_cache_dir: Some(vmm_test_artifacts_dir.clone()),
        skip_prompt: Some(skip_vhd_prompt),
        ..Default::default()
    });
    Ok(())
}
