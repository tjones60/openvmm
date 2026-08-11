// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Setup directory structure that the VMM tests require to run.

use crate::_jobs::consume_and_test_nextest_vmm_tests_archive::VmmTestsDepArtifacts;
use crate::build_incubator::incubator_profile_dir;
use crate::common::CommonArch;
use crate::download_release_igvm_files_from_gh::OpenhclReleaseVersion;
use flowey::node::prelude::*;

flowey_request! {
    pub struct Request {
        /// Directory to symlink / copy test contents into. Does not need to be
        /// empty.
        pub test_content_dir: ReadVar<PathBuf>,
        /// What triple VMM tests are built for.
        ///
        /// Used to detect cases of running Windows VMM tests via WSL2, and adjusting
        /// reported paths appropriately.
        pub vmm_tests_target: target_lexicon::Triple,
        /// Artifacts corresponding to required test dependencies
        pub vmm_tests_dep_artifacts: VmmTestsDepArtifacts,
        /// Copy files necessary to use the test content dir as a minimal repo root.
        ///
        /// This is useful for running tests on machines without a local clone.
        pub is_repo_root: bool,
        pub needs_release_igvm: bool,

        pub done: WriteVar<SideEffect>
    }
}

new_simple_flow_node!(struct Node);

impl SimpleFlowNode for Node {
    type Request = Request;

    fn imports(ctx: &mut ImportCtx<'_>) {
        ctx.import::<crate::resolve_openvmm_test_initrd::Node>();
        ctx.import::<crate::resolve_openvmm_test_linux_kernel::Node>();
        ctx.import::<crate::resolve_openvmm_test_virtio_win::Node>();
        ctx.import::<crate::git_checkout_openvmm_repo::Node>();
        ctx.import::<crate::download_uefi_mu_msvm::Node>();
        ctx.import::<crate::download_release_igvm_files_from_gh::resolve::Node>();
    }

    fn process_request(request: Self::Request, ctx: &mut NodeCtx<'_>) -> anyhow::Result<()> {
        let Request {
            test_content_dir,
            vmm_tests_target,
            vmm_tests_dep_artifacts,
            is_repo_root,
            needs_release_igvm,
            done,
        } = request;

        let openvmm_repo_path =
            is_repo_root.then(|| ctx.reqv(crate::git_checkout_openvmm_repo::req::GetRepoDir));

        let arch = CommonArch::from_architecture(vmm_tests_target.architecture)?;

        let test_linux_initrd =
            ctx.reqv(|v| crate::resolve_openvmm_test_initrd::Request::Get(arch, v));
        let test_linux_kernel = ctx.reqv(|v| {
            crate::resolve_openvmm_test_linux_kernel::Request::Get(
                crate::resolve_openvmm_test_linux_kernel::OpenvmmTestKernelFile::Kernel,
                arch,
                crate::resolve_openvmm_test_linux_kernel::DEFAULT_LINUX_TEST_KERNEL_VERSION,
                v,
            )
        });
        let test_linux_bzimage =
            crate::resolve_openvmm_test_linux_kernel::OpenvmmTestKernelFile::BzImage
                .is_available_for(arch)
                .then(|| {
                    ctx.reqv(|v| {
                        crate::resolve_openvmm_test_linux_kernel::Request::Get(
                            crate::resolve_openvmm_test_linux_kernel::OpenvmmTestKernelFile::BzImage,
                            arch,
                            crate::resolve_openvmm_test_linux_kernel::DEFAULT_LINUX_TEST_KERNEL_VERSION,
                            v,
                        )
                    })
                });

        let uefi =
            ctx.reqv(|v| crate::download_uefi_mu_msvm::Request::GetMsvmFd { arch, msvm_fd: v });

        let virtio_win_dir = ctx.reqv(crate::resolve_openvmm_test_virtio_win::Request::Get);

        let release_igvm_files = if needs_release_igvm {
            Some(ctx.reqv(
                |v| crate::download_release_igvm_files_from_gh::resolve::Request {
                    arch,
                    release_igvm_files: v,
                    release_version: OpenhclReleaseVersion::latest(),
                },
            ))
        } else {
            None
        };

        let VmmTestsDepArtifacts {
            incubator: register_incubator,
            openvmm: register_openvmm,
            openvmm_vhost: register_openvmm_vhost,
            pipette_windows: register_pipette_windows,
            pipette_linux_musl: register_pipette_linux_musl,
            guest_test_uefi: register_guest_test_uefi,
            prep_steps: register_prep_steps,
            openhcl_standard,
            openhcl_standard_dev,
            openhcl_cvm,
            openhcl_linux_direct,
            tmks: register_tmks,
            tmk_vmm: register_tmk_vmm,
            tmk_vmm_linux_musl: register_tmk_vmm_linux_musl,
            vmgstool: register_vmgstool,
            vmgstool_dev: register_vmgstool_dev,
            tpm_guest_tests_windows: register_tpm_guest_tests_windows,
            tpm_guest_tests_linux: register_tpm_guest_tests_linux,
            test_igvm_agent_rpc_server: register_test_igvm_agent_rpc_server,
        } = vmm_tests_dep_artifacts;

        let register_openhcl_igvm_files = [
            openhcl_standard,
            openhcl_standard_dev,
            openhcl_cvm,
            openhcl_linux_direct,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

        ctx.emit_rust_step("setting up vmm_tests content dir", |ctx| {
            let test_content_dir = test_content_dir.claim(ctx);
            let openvmm_repo_path = openvmm_repo_path.claim(ctx);

            let incubator = register_incubator.claim(ctx);
            let openvmm = register_openvmm.claim(ctx);
            let openvmm_vhost = register_openvmm_vhost.claim(ctx);
            let pipette_win = register_pipette_windows.claim(ctx);
            let pipette_linux = register_pipette_linux_musl.claim(ctx);
            let guest_test_uefi = register_guest_test_uefi.claim(ctx);
            let prep_steps = register_prep_steps.claim(ctx);
            let openhcl_igvm_files = register_openhcl_igvm_files.claim(ctx);
            let tmks = register_tmks.claim(ctx);
            let tmk_vmm = register_tmk_vmm.claim(ctx);
            let tmk_vmm_linux_musl = register_tmk_vmm_linux_musl.claim(ctx);
            let vmgstool = register_vmgstool.claim(ctx);
            let vmgstool_dev = register_vmgstool_dev.claim(ctx);
            let tpm_guest_tests_windows = register_tpm_guest_tests_windows.claim(ctx);
            let tpm_guest_tests_linux = register_tpm_guest_tests_linux.claim(ctx);
            let test_igvm_agent_rpc_server = register_test_igvm_agent_rpc_server.claim(ctx);

            let test_linux_initrd = test_linux_initrd.claim(ctx);
            let test_linux_kernel = test_linux_kernel.claim(ctx);
            let test_linux_bzimage = test_linux_bzimage.claim(ctx);
            let uefi = uefi.claim(ctx);
            let virtio_win_dir = virtio_win_dir.claim(ctx);
            let release_igvm_files_dir = release_igvm_files.claim(ctx);

            done.claim(ctx);

            move |rt| {
                let test_linux_initrd = rt.read(test_linux_initrd);
                let test_linux_kernel = rt.read(test_linux_kernel);
                let test_linux_bzimage = test_linux_bzimage.map(|v| rt.read(v));
                let uefi = rt.read(uefi);
                let release_igvm_files_dir = rt.read(release_igvm_files_dir);
                let test_content_dir = rt.read(test_content_dir);

                if !test_content_dir.exists() {
                    fs_err::create_dir_all(&test_content_dir)?
                };

                if let Some(openvmm_repo_path) = openvmm_repo_path {
                    let openvmm_repo_path = rt.read(openvmm_repo_path);

                    let nextest_config_file = PathBuf::new().join(".config").join("nextest.toml");
                    fs_err::create_dir_all(
                        test_content_dir
                            .join(&nextest_config_file)
                            .parent()
                            .context("no parent")?,
                    )?;
                    fs_err::copy(
                        openvmm_repo_path.join(&nextest_config_file),
                        test_content_dir.join(&nextest_config_file),
                    )?;

                    let repo_cargo_toml_file = Path::new("Cargo.toml");
                    fs_err::copy(
                        openvmm_repo_path.join(repo_cargo_toml_file),
                        test_content_dir.join(repo_cargo_toml_file),
                    )?;

                    let crate_cargo_toml_file = PathBuf::new()
                        .join("vmm_tests")
                        .join("vmm_tests")
                        .join("Cargo.toml");
                    fs_err::create_dir_all(
                        test_content_dir
                            .join(&crate_cargo_toml_file)
                            .parent()
                            .context("no parent")?,
                    )?;
                    fs_err::copy(
                        openvmm_repo_path.join(&crate_cargo_toml_file),
                        test_content_dir.join(&crate_cargo_toml_file),
                    )?;

                    if incubator.is_some() {
                        let incubator_profile_dir = incubator_profile_dir();
                        fs_err::create_dir_all(test_content_dir.join(&incubator_profile_dir))?;
                        for entry in
                            fs_err::read_dir(openvmm_repo_path.join(&incubator_profile_dir))?
                        {
                            let profile = entry?.path();
                            if profile.is_file()
                                && profile.extension().is_some_and(|ext| ext == "toml")
                            {
                                let dst = test_content_dir
                                    .join(&incubator_profile_dir)
                                    .join(profile.file_name().context("no file name")?);
                                fs_err::copy(profile, dst)?;
                            }
                        }
                    }
                }

                if let Some(openvmm) = openvmm {
                    match rt.read(openvmm) {
                        crate::build_openvmm::OpenvmmOutput::WindowsBin { exe, pdb: _ } => {
                            fs_err::copy(exe, test_content_dir.join("openvmm.exe"))?;
                        }
                        crate::build_openvmm::OpenvmmOutput::LinuxBin { bin, dbg: _ } => {
                            let dst = test_content_dir.join("openvmm");
                            fs_err::copy(bin, dst.clone())?;
                            dst.make_executable()?;
                        }
                    }
                }

                if let Some(openvmm_vhost) = openvmm_vhost {
                    let crate::build_openvmm_vhost::OpenvmmVhostOutput { bin, dbg: _ } =
                        rt.read(openvmm_vhost);
                    let dst = test_content_dir.join("openvmm_vhost");
                    fs_err::copy(bin, &dst)?;
                    dst.make_executable()?;
                }

                if let Some(pipette_win) = pipette_win {
                    match rt.read(pipette_win) {
                        crate::build_pipette::PipetteOutput::WindowsBin { exe, pdb: _ } => {
                            fs_err::copy(exe, test_content_dir.join("pipette.exe"))?;
                        }
                        _ => anyhow::bail!("did not find `pipette.exe` in RegisterPipetteWindows"),
                    }
                }

                if let Some(pipette_linux) = pipette_linux {
                    match rt.read(pipette_linux) {
                        crate::build_pipette::PipetteOutput::LinuxBin { bin, dbg: _ } => {
                            let dst = test_content_dir.join("pipette");
                            fs_err::copy(bin, &dst)?;
                            dst.make_executable()?;
                        }
                        _ => {
                            anyhow::bail!("did not find `pipette` in RegisterPipetteLinuxMusl")
                        }
                    }
                }

                if let Some(guest_test_uefi) = guest_test_uefi {
                    let crate::build_guest_test_uefi::GuestTestUefiOutput {
                        efi: _,
                        pdb: _,
                        img,
                    } = rt.read(guest_test_uefi);
                    fs_err::copy(img, test_content_dir.join("guest_test_uefi.img"))?;
                }

                if let Some(tmks) = tmks {
                    let crate::build_tmks::TmksOutput { bin, dbg: _ } = rt.read(tmks);
                    fs_err::copy(bin, test_content_dir.join("simple_tmk"))?;
                }

                if let Some(tmk_vmm) = tmk_vmm {
                    match rt.read(tmk_vmm) {
                        crate::build_tmk_vmm::TmkVmmOutput::WindowsBin { exe, .. } => {
                            fs_err::copy(exe, test_content_dir.join("tmk_vmm.exe"))?;
                        }
                        crate::build_tmk_vmm::TmkVmmOutput::LinuxBin { bin, .. } => {
                            let dst = test_content_dir.join("tmk_vmm");
                            fs_err::copy(bin, &dst)?;
                            dst.make_executable()?;
                        }
                    }
                }

                if let Some(tmk_vmm_linux_musl) = tmk_vmm_linux_musl {
                    let crate::build_tmk_vmm::TmkVmmOutput::LinuxBin { bin, dbg: _ } =
                        rt.read(tmk_vmm_linux_musl)
                    else {
                        anyhow::bail!("invalid tmk_vmm output")
                    };
                    // Note that this overwrites the previous tmk_vmm. That's
                    // OK, they should be the same. Fix this when the resolver
                    // can handle multiple different outputs with the same name.
                    fs_err::copy(bin, test_content_dir.join("tmk_vmm"))?;
                }

                if let Some(vmgstool) = vmgstool {
                    match rt.read(vmgstool) {
                        crate::build_vmgstool::VmgstoolOutput::WindowsBin { exe, .. } => {
                            fs_err::copy(exe, test_content_dir.join("vmgstool.exe"))?;
                        }
                        crate::build_vmgstool::VmgstoolOutput::LinuxBin { bin, .. } => {
                            let dst = test_content_dir.join("vmgstool");
                            fs_err::copy(bin, &dst)?;
                            dst.make_executable()?;
                        }
                    }
                }

                if let Some(vmgstool_dev) = vmgstool_dev {
                    match rt.read(vmgstool_dev) {
                        crate::build_vmgstool::VmgstoolOutput::WindowsBin { exe, .. } => {
                            fs_err::copy(exe, test_content_dir.join("vmgstool-dev.exe"))?;
                        }
                        crate::build_vmgstool::VmgstoolOutput::LinuxBin { bin, .. } => {
                            let dst = test_content_dir.join("vmgstool-dev");
                            fs_err::copy(bin, &dst)?;
                            dst.make_executable()?;
                        }
                    }
                }

                if let Some(tpm_guest_tests_windows) = tpm_guest_tests_windows {
                    let crate::build_tpm_guest_tests::TpmGuestTestsOutput::WindowsBin {
                        exe, ..
                    } = rt.read(tpm_guest_tests_windows)
                    else {
                        anyhow::bail!("expected Windows tpm_guest_tests artifact")
                    };
                    fs_err::copy(exe, test_content_dir.join("tpm_guest_tests.exe"))?;
                }

                if let Some(tpm_guest_tests_linux) = tpm_guest_tests_linux {
                    let crate::build_tpm_guest_tests::TpmGuestTestsOutput::LinuxBin { bin, .. } =
                        rt.read(tpm_guest_tests_linux)
                    else {
                        anyhow::bail!("expected Linux tpm_guest_tests artifact")
                    };
                    let dst = test_content_dir.join("tpm_guest_tests");
                    fs_err::copy(bin, &dst)?;
                    dst.make_executable()?;
                }

                if let Some(test_igvm_agent_rpc_server) = test_igvm_agent_rpc_server {
                    let crate::build_test_igvm_agent_rpc_server::TestIgvmAgentRpcServerOutput {
                        exe,
                        ..
                    } = rt.read(test_igvm_agent_rpc_server);
                    fs_err::copy(exe, test_content_dir.join("test_igvm_agent_rpc_server.exe"))?;
                }

                if let Some(prep_steps) = prep_steps {
                    match rt.read(prep_steps) {
                        crate::build_prep_steps::PrepStepsOutput::WindowsBin { exe, .. } => {
                            fs_err::copy(exe, test_content_dir.join("prep_steps.exe"))?;
                        }
                        crate::build_prep_steps::PrepStepsOutput::LinuxBin { bin, .. } => {
                            let dst = test_content_dir.join("prep_steps");
                            fs_err::copy(bin, &dst)?;
                            dst.make_executable()?;
                        }
                    }
                }

                if let Some(incubator) = incubator {
                    let crate::build_incubator::IncubatorOutput { bin, .. } = rt.read(incubator);
                    fs_err::copy(bin, test_content_dir.join("incubator"))?;
                }

                for openhcl_igvm in rt.read(openhcl_igvm_files) {
                    let igvm_bin = openhcl_igvm.igvm_bin();
                    if let Some(recipe) = openhcl_igvm.recipe() {
                        fs_err::copy(
                            igvm_bin,
                            test_content_dir.join(format!("{}.bin", recipe.non_production_name())),
                        )?;
                    } else {
                        log::warn!("petri doesn't support custom OpenHCL files");
                    };
                }

                if let Some(release_igvm_files) = release_igvm_files_dir {
                    let latest_release_version = OpenhclReleaseVersion::latest();

                    if let Some(src) = &release_igvm_files.openhcl {
                        let new_name = format!("{latest_release_version}-x64-openhcl.bin");
                        fs_err::copy(src, test_content_dir.join(new_name))?;
                    }

                    if let Some(src) = &release_igvm_files.openhcl_aarch64 {
                        let new_name = format!("{latest_release_version}-aarch64-openhcl.bin");
                        fs_err::copy(src, test_content_dir.join(new_name))?;
                    }

                    if let Some(src) = &release_igvm_files.openhcl_direct {
                        let new_name = format!("{latest_release_version}-x64-direct-openhcl.bin");
                        fs_err::copy(src, test_content_dir.join(new_name))?;
                    }
                }

                let (arch_dir, kernel_file_name) = match arch {
                    CommonArch::X86_64 => ("x64", "vmlinux"),
                    CommonArch::Aarch64 => ("aarch64", "Image"),
                };
                fs_err::create_dir_all(test_content_dir.join(arch_dir))?;
                fs_err::copy(
                    test_linux_initrd,
                    test_content_dir.join(arch_dir).join("initrd"),
                )?;
                fs_err::copy(
                    test_linux_kernel,
                    test_content_dir.join(arch_dir).join(kernel_file_name),
                )?;
                if let Some(bzimage_path) = test_linux_bzimage {
                    fs_err::copy(
                        bzimage_path,
                        test_content_dir.join(arch_dir).join("bzImage"),
                    )?;
                }

                let uefi_dir = test_content_dir.join(match arch {
                    CommonArch::Aarch64 => {
                        "hyperv.uefi.mscoreuefi.AARCH64.RELEASE/MsvmAARCH64/RELEASE_CLANGPDB/FV"
                    }
                    CommonArch::X86_64 => {
                        "hyperv.uefi.mscoreuefi.x64.RELEASE/MsvmX64/RELEASE_VS2022/FV"
                    }
                });
                fs_err::create_dir_all(&uefi_dir)?;
                fs_err::copy(uefi, uefi_dir.join("MSVM.fd"))?;

                {
                    let src = rt.read(virtio_win_dir);
                    let dst = test_content_dir.join("virtio-win");
                    let _ = fs_err::remove_dir_all(&dst);
                    flowey_lib_common::_util::copy_dir_all(&src, &dst)?;
                }

                // debug log the current contents of the dir
                log::debug!("final folder content: {}", test_content_dir.display());
                for entry in test_content_dir.read_dir()? {
                    let entry = entry?;
                    log::debug!("contains: {:?}", entry.file_name());
                }

                Ok(())
            }
        });

        Ok(())
    }
}
