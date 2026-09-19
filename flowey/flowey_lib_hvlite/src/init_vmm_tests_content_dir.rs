// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Setup directory structure that the VMM tests require to run.

use crate::build_flowey_hvlite::FloweyHvliteOutput;
use crate::build_guest_test_uefi::GuestTestUefiOutput;
use crate::build_incubator::IncubatorOutput;
use crate::build_incubator::incubator_profile_dir;
use crate::build_nextest_vmm_tests::NextestVmmTestsArchive;
use crate::build_openhcl_igvm_from_recipe::OpenhclIgvmOutput;
use crate::build_openvmm::OpenvmmOutput;
use crate::build_openvmm_vhost::OpenvmmVhostOutput;
use crate::build_pipette::PipetteOutput;
use crate::build_prep_steps::PrepStepsOutput;
use crate::build_test_igvm_agent_rpc_server::TestIgvmAgentRpcServerOutput;
use crate::build_tmk_vmm::TmkVmmOutput;
use crate::build_tmks::TmksOutput;
use crate::build_tpm_guest_tests::TpmGuestTestsOutput;
use crate::build_vmgstool::VmgstoolOutput;
use crate::common::CommonArch;
use crate::download_release_igvm_files_from_gh::OpenhclReleaseVersion;
use flowey::node::prelude::*;
use petri_artifacts_common::artifacts::*;
use petri_artifacts_core::ArtifactId;
use petri_artifacts_vmm_test::artifacts::*;

macro_rules! define_vmm_tests_built_artifacts {
    (
        $($artifact:ident(
            $($variant:ident(
                $output_variant:ty,
                $member:ident,
                $artifact_ty:ty
            )),* $(,)?
        ) => $output:ty),* $(,)?
    ) => {
        ::paste::paste! {
            #[derive(Serialize, Deserialize)]
            pub struct VmmTestsBuiltArtifacts<C = VarNotClaimed> {$($(
                pub [<$artifact _ $variant>]: Option<::flowey::node::prelude::ReadVar<$output, C>>,
            )*)*}

            impl Default for VmmTestsBuiltArtifacts<VarNotClaimed> {
                fn default() -> Self {
                    Self {$($(
                        [<$artifact _ $variant>]: None,
                    )*)*}
                }
            }

            impl VmmTestsBuiltArtifacts<VarNotClaimed> {
                fn claim(self, ctx: &mut StepCtx<'_>) -> VmmTestsBuiltArtifacts<VarClaimed> {
                    let Self {$($(
                        [<$artifact _ $variant>],
                    )*)*} = self;
                    VmmTestsBuiltArtifacts {$($(
                        [<$artifact _ $variant>]: [<$artifact _ $variant>].claim(ctx),
                    )*)*}
                }

                $(pub fn $artifact(&mut self, target: Option<::target_lexicon::Triple>) -> ::anyhow::Result<&mut Option<::flowey::node::prelude::ReadVar<$output>>> {
                    #[allow(unreachable_patterns)]
                    match target {
                        $($artifact_ty::TARGET => Ok(&mut self.[<$artifact _ $variant>]),)*
                        _ => Err(::anyhow::anyhow!(concat!("target does not exist for ", stringify!($artifact)))),
                    }
                })*

                $($(pub fn [<$artifact _ $variant _target>]() -> ::target_lexicon::Triple {
                    // All built artifacts should have a target
                    $artifact_ty::TARGET.unwrap()
                })*)*
            }

            impl VmmTestsBuiltArtifacts<VarClaimed> {
                fn write(self, rt: &mut RustRuntimeServices<'_>, test_content_dir: impl AsRef<Path>) -> ::anyhow::Result<()> {
                    let Self {$($(
                        [<$artifact _ $variant>],
                    )*)*} = self;


                    $($(if let Some(artifact) = [<$artifact _ $variant>] {
                        let dst = test_content_dir
                            .as_ref()
                            .join(::petri_artifacts_core::artifact_subdir::<$artifact_ty>())
                            .join($artifact_ty::FILENAME);

                        #[allow(irrefutable_let_patterns)]
                        let $output_variant { $member, .. } = rt.read(artifact) else {
                            ::anyhow::bail!(concat!(
                                "unexpected variant of ",
                                stringify!($output),
                                " for ",
                                stringify!([<$artifact _ $variant>])
                            ));
                        };

                        fs_err::copy($member, &dst)?;
                        dst.make_executable()?;
                    })*)*

                    Ok(())
                }
            }

            #[derive(Serialize, Deserialize, Default)]
            pub struct VmmTestsBuiltArtifactsWrite {$($(
                pub [<$artifact _ $variant>]: Option<::flowey::node::prelude::WriteVar<$output>>,
            )*)*}

            #[derive(Serialize, Deserialize, Default, Debug)]
            pub struct VmmTestsBuiltArtifactsSelections {$($(
                pub [<$artifact _ $variant>]: bool,
            )*)*}

            impl VmmTestsBuiltArtifactsSelections {
                pub fn resolve_artifact(&mut self, id: &str) -> bool {
                    match id {
                        $($($artifact_ty::GLOBAL_UNIQUE_ID => {
                            self.[<$artifact _ $variant>] = true;
                            true
                        })*)*
                        _ => false
                    }
                }

                $(pub fn [<$artifact _native>](&self) -> ::anyhow::Result<bool>{
                    #[allow(unreachable_patterns)]
                    match Some(::target_lexicon::Triple::host()) {
                        $($artifact_ty::TARGET => {
                            Ok(self.[<$artifact _ $variant>])
                        })*
                        _ => Err(::anyhow::anyhow!(concat!("host target does not exist for ", stringify!($artifact)))),
                    }
                }

                pub fn [<require_ $artifact _native>](&mut self) -> ::anyhow::Result<()>{
                    #[allow(unreachable_patterns)]
                    match Some(::target_lexicon::Triple::host()) {
                        $($artifact_ty::TARGET => {
                            self.[<$artifact _ $variant>] = true;
                        })*
                        _ => ::anyhow::bail!(concat!("host target does not exist for ", stringify!($artifact))),
                    }
                    Ok(())
                })*
            }

        }
    };
}

define_vmm_tests_built_artifacts!(
    // Artifacts used at the pipeline level.
    flowey_hvlite(
        windows_x64(FloweyHvliteOutput::WindowsBin, exe, host_tools::FLOWEY_HVLITE_WIN_X64),
        windows_aarch64(
            FloweyHvliteOutput::WindowsBin,
            exe,
            host_tools::FLOWEY_HVLITE_WIN_AARCH64
        ),
        linux_x64(FloweyHvliteOutput::LinuxBin, bin, host_tools::FLOWEY_HVLITE_LINUX_X64),
    ) => FloweyHvliteOutput,
    nextest_vmm_tests_archive(
        windows_x64(
            NextestVmmTestsArchive,
            archive_file,
            host_tools::NEXTEST_VMM_TESTS_ARCHIVE_WINDOWS_X64
        ),
        windows_aarch64(
            NextestVmmTestsArchive,
            archive_file,
            host_tools::NEXTEST_VMM_TESTS_ARCHIVE_WINDOWS_AARCH64
        ),
        linux_x64(
            NextestVmmTestsArchive,
            archive_file,
            host_tools::NEXTEST_VMM_TESTS_ARCHIVE_LINUX_X64
        ),
        linux_musl_x64(
            NextestVmmTestsArchive,
            archive_file,
            host_tools::NEXTEST_VMM_TESTS_ARCHIVE_LINUX_X64_MUSL
        ),
        linux_musl_aarch64(
            NextestVmmTestsArchive,
            archive_file,
            host_tools::NEXTEST_VMM_TESTS_ARCHIVE_LINUX_AARCH64_MUSL
        ),
    ) => NextestVmmTestsArchive,
    incubator(
        linux_x64(IncubatorOutput, bin, host_tools::INCUBATOR_LINUX_X64),
    ) => IncubatorOutput,
    prep_steps(
        windows_x64(PrepStepsOutput::WindowsBin, exe, host_tools::PREP_STEPS_WINDOWS_X64),
        linux_musl_x64(PrepStepsOutput::LinuxBin, bin, host_tools::PREP_STEPS_LINUX_X64_MUSL),
    ) => PrepStepsOutput,
    test_igvm_agent_rpc_server(
        windows_x64(
            TestIgvmAgentRpcServerOutput,
            exe,
            host_tools::TEST_IGVM_AGENT_RPC_SERVER_WINDOWS_X64
        ),
    ) => TestIgvmAgentRpcServerOutput,

    // Artifacts used internally by petri.
    openvmm(
        windows_x64(OpenvmmOutput::WindowsBin, exe, OPENVMM_WIN_X64),
        windows_aarch64(OpenvmmOutput::WindowsBin, exe, OPENVMM_WIN_AARCH64),
        linux_x64(OpenvmmOutput::LinuxBin, bin, OPENVMM_LINUX_X64),
        linux_aarch64(OpenvmmOutput::LinuxBin, bin, OPENVMM_LINUX_AARCH64),
        linux_musl_x64(OpenvmmOutput::LinuxBin, bin, OPENVMM_LINUX_X64_MUSL),
        linux_musl_aarch64(OpenvmmOutput::LinuxBin, bin, OPENVMM_LINUX_AARCH64_MUSL),
    ) => OpenvmmOutput,
    openvmm_vhost(
        linux_x64(OpenvmmVhostOutput, bin, OPENVMM_VHOST_LINUX_X64),
        linux_aarch64(OpenvmmVhostOutput, bin, OPENVMM_VHOST_LINUX_AARCH64),
        linux_musl_x64(OpenvmmVhostOutput, bin, OPENVMM_VHOST_LINUX_X64_MUSL),
        linux_musl_aarch64(OpenvmmVhostOutput, bin, OPENVMM_VHOST_LINUX_AARCH64_MUSL),
    ) => OpenvmmVhostOutput,
    pipette(
        windows_x64(PipetteOutput::WindowsBin, exe, PIPETTE_WINDOWS_X64),
        windows_aarch64(PipetteOutput::WindowsBin, exe, PIPETTE_WINDOWS_AARCH64),
        linux_x64(PipetteOutput::LinuxBin, bin, PIPETTE_LINUX_X64),
        linux_musl_x64(PipetteOutput::LinuxBin, bin, PIPETTE_LINUX_X64_MUSL),
        linux_musl_aarch64(
            PipetteOutput::LinuxBin,
            bin,
            PIPETTE_LINUX_AARCH64_MUSL
        ),
    ) => PipetteOutput,
    guest_test_uefi(
        x64(GuestTestUefiOutput, img, test_vhd::GUEST_TEST_UEFI_X64),
        aarch64(
            GuestTestUefiOutput,
            img,
            test_vhd::GUEST_TEST_UEFI_AARCH64
        ),
    ) => GuestTestUefiOutput,
    openhcl_standard(
        x64(OpenhclIgvmOutput::X64, igvm_bin, openhcl_igvm::LATEST_STANDARD_X64),
        aarch64(
            OpenhclIgvmOutput::Aarch64,
            igvm_bin,
            openhcl_igvm::LATEST_STANDARD_AARCH64
        ),
    ) => OpenhclIgvmOutput,
    openhcl_standard_dev(
        x64(
            OpenhclIgvmOutput::X64Devkern,
            igvm_bin,
            openhcl_igvm::LATEST_STANDARD_DEV_KERNEL_X64
        ),
        aarch64(
            OpenhclIgvmOutput::Aarch64Devkern,
            igvm_bin,
            openhcl_igvm::LATEST_STANDARD_DEV_KERNEL_AARCH64
        ),
    ) => OpenhclIgvmOutput,
    openhcl_cvm(
        x64(
            OpenhclIgvmOutput::X64Cvm,
            igvm_bin,
            openhcl_igvm::LATEST_CVM_X64
        ),
    ) => OpenhclIgvmOutput,
    openhcl_linux_direct(
        x64(
            OpenhclIgvmOutput::X64TestLinuxDirect,
            igvm_bin,
            openhcl_igvm::LATEST_LINUX_DIRECT_TEST_X64
        ),
    ) => OpenhclIgvmOutput,
    tmks(
        x64(TmksOutput, bin, tmks::SIMPLE_TMK_X64),
        aarch64(TmksOutput, bin, tmks::SIMPLE_TMK_AARCH64),
    ) => TmksOutput,
    tmk_vmm(
        windows_x64(TmkVmmOutput::WindowsBin, exe, tmks::TMK_VMM_WIN_X64),
        windows_aarch64(
            TmkVmmOutput::WindowsBin,
            exe,
            tmks::TMK_VMM_WIN_AARCH64
        ),
        linux_musl_x64(
            TmkVmmOutput::LinuxBin,
            bin,
            tmks::TMK_VMM_LINUX_X64_MUSL
        ),
        linux_musl_aarch64(
            TmkVmmOutput::LinuxBin,
            bin,
            tmks::TMK_VMM_LINUX_AARCH64_MUSL
        ),
    ) => TmkVmmOutput,
    vmgstool(
        windows_x64(VmgstoolOutput::WindowsBin, exe, vmgstool::VMGSTOOL_WIN_X64),
        windows_aarch64(
            VmgstoolOutput::WindowsBin,
            exe,
            vmgstool::VMGSTOOL_WIN_AARCH64
        ),
        linux_x64(VmgstoolOutput::LinuxBin, bin, vmgstool::VMGSTOOL_LINUX_X64),
    ) => VmgstoolOutput,
    vmgstool_dev(
        windows_x64(VmgstoolOutput::WindowsBin, exe, vmgstool::VMGSTOOL_DEV_WIN_X64),
        windows_aarch64(
            VmgstoolOutput::WindowsBin,
            exe,
            vmgstool::VMGSTOOL_DEV_WIN_AARCH64
        ),
        linux_x64(
            VmgstoolOutput::LinuxBin,
            bin,
            vmgstool::VMGSTOOL_DEV_LINUX_X64
        ),
    ) => VmgstoolOutput,
    tpm_guest_tests(
        windows_x64(
            TpmGuestTestsOutput::WindowsBin,
            exe,
            guest_tools::TPM_GUEST_TESTS_WINDOWS_X64
        ),
        linux_x64(
            TpmGuestTestsOutput::LinuxBin,
            bin,
            guest_tools::TPM_GUEST_TESTS_LINUX_X64
        ),
    ) => TpmGuestTestsOutput,
);

pub type ResolveVmmTestsBuiltArtifacts =
    Box<dyn Fn(&mut flowey::pipeline::prelude::PipelineJobCtx<'_>) -> VmmTestsBuiltArtifacts>;

#[macro_export]
macro_rules! vmm_tests_built_artifacts_builder {
    (
        $name:ty,
        (
            $($artifact:ident => $output:ty),* $(,)?
        )
    ) => {
        ::paste::paste! {
            #[derive(Default, Clone)]
            pub struct $name {
                $(pub [<use_ $artifact>]: Option<::flowey::pipeline::prelude::UseTypedArtifact<$output>>,)*
            }

            impl $name {
                pub fn finish(self) -> Result<$crate::init_vmm_tests_content_dir::ResolveVmmTestsBuiltArtifacts, &'static str> {
                    let $name {
                        $([<use_ $artifact>],)*
                    } = self;

                    $(let [<use_ $artifact>] = [<use_ $artifact>].ok_or(stringify!($artifact))?;)*

                    Ok(Box::new(move |ctx| $crate::init_vmm_tests_content_dir::VmmTestsBuiltArtifacts {
                        $($artifact: Some(ctx.use_typed_artifact(&[<use_ $artifact>])),)*
                        .. Default::default()
                    }))
                }

                pub fn pair(ctx: &mut ::flowey::node::prelude::NodeCtx<'_>) -> (
                    $crate::init_vmm_tests_content_dir::VmmTestsBuiltArtifacts,
                    $crate::init_vmm_tests_content_dir::VmmTestsBuiltArtifactsWrite,
                ) {

                    $(let ([<$artifact _read>], [<$artifact _write>]) = ctx.new_var();)*

                    let built_artifacts_read = $crate::init_vmm_tests_content_dir::VmmTestsBuiltArtifacts {
                        $($artifact: Some([<$artifact _read>]),)*
                        .. Default::default()
                    };

                    let built_artifacts_write = $crate::init_vmm_tests_content_dir::VmmTestsBuiltArtifactsWrite {
                        $($artifact: Some([<$artifact _write>]),)*
                        .. Default::default()
                    };

                    (built_artifacts_read, built_artifacts_write)
                }
            }
        }
    };
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct VmmTestsPreBuiltArtifactsSelections {
    // Specify arch for initrd and kernel here as a hack to bring up qemu support
    // TODO: have a VmmTestsPreBuiltArtifacts for each target with corresponding
    // test content sub-dir.
    pub test_linux_initrd_x64: bool,
    pub test_linux_kernel_x64: bool,
    pub test_linux_initrd_aarch64: bool,
    pub test_linux_kernel_aarch64: bool,
    pub test_linux_bzimage_x64: bool,
    pub uefi: bool,
    pub virtio_win_drivers: bool,
    pub release_igvm: bool,
    pub qemu_system_aarch64: bool,
}

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
        /// Artifacts used by the tests that are built in the openvmm repo
        pub built_artifacts: VmmTestsBuiltArtifacts,
        /// Artifacts to download that are pre-built as part of OpenVMM deps
        pub prebuilt_artifacts: VmmTestsPreBuiltArtifactsSelections,
        /// Copy files necessary to use the test content dir as a minimal repo root.
        ///
        /// This is useful for running tests on machines without a local clone.
        pub is_repo_root: bool,
        /// Whether to copy incubator profiles into the test content directory.
        pub needs_incubator_profiles: bool,

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
        ctx.import::<crate::resolve_openvmm_qemu::Node>();
    }

    fn process_request(request: Self::Request, ctx: &mut NodeCtx<'_>) -> anyhow::Result<()> {
        let Request {
            test_content_dir,
            vmm_tests_target,
            built_artifacts,
            prebuilt_artifacts,
            is_repo_root,
            needs_incubator_profiles,
            done,
        } = request;

        let openvmm_repo_path =
            is_repo_root.then(|| ctx.reqv(crate::git_checkout_openvmm_repo::req::GetRepoDir));

        let arch = CommonArch::from_architecture(vmm_tests_target.architecture)?;

        let test_linux_initrd_x64 = prebuilt_artifacts.test_linux_initrd_x64.then(|| {
            ctx.reqv(|v| crate::resolve_openvmm_test_initrd::Request::Get(CommonArch::X86_64, v))
        });
        let test_linux_initrd_aarch64 = prebuilt_artifacts.test_linux_initrd_aarch64.then(|| {
            ctx.reqv(|v| crate::resolve_openvmm_test_initrd::Request::Get(CommonArch::Aarch64, v))
        });
        let test_linux_kernel_x64 = prebuilt_artifacts.test_linux_kernel_x64.then(|| {
            ctx.reqv(|v| {
                crate::resolve_openvmm_test_linux_kernel::Request::Get(
                    crate::resolve_openvmm_test_linux_kernel::OpenvmmTestKernelFile::Kernel,
                    CommonArch::X86_64,
                    crate::resolve_openvmm_test_linux_kernel::DEFAULT_LINUX_TEST_KERNEL_VERSION,
                    v,
                )
            })
        });
        let test_linux_kernel_aarch64 = prebuilt_artifacts.test_linux_kernel_aarch64.then(|| {
            ctx.reqv(|v| {
                crate::resolve_openvmm_test_linux_kernel::Request::Get(
                    crate::resolve_openvmm_test_linux_kernel::OpenvmmTestKernelFile::Kernel,
                    CommonArch::Aarch64,
                    crate::resolve_openvmm_test_linux_kernel::DEFAULT_LINUX_TEST_KERNEL_VERSION,
                    v,
                )
            })
        });
        let test_linux_bzimage_x64 = prebuilt_artifacts.test_linux_bzimage_x64.then(|| {
            ctx.reqv(|v| {
                crate::resolve_openvmm_test_linux_kernel::Request::Get(
                    crate::resolve_openvmm_test_linux_kernel::OpenvmmTestKernelFile::BzImage,
                    CommonArch::X86_64,
                    crate::resolve_openvmm_test_linux_kernel::DEFAULT_LINUX_TEST_KERNEL_VERSION,
                    v,
                )
            })
        });

        let uefi = prebuilt_artifacts.uefi.then(|| {
            ctx.reqv(|v| crate::download_uefi_mu_msvm::Request::GetMsvmFd { arch, msvm_fd: v })
        });

        let virtio_win_dir = prebuilt_artifacts
            .virtio_win_drivers
            .then(|| ctx.reqv(crate::resolve_openvmm_test_virtio_win::Request::Get));

        let release_igvm_files = prebuilt_artifacts.release_igvm.then(|| {
            ctx.reqv(
                |v| crate::download_release_igvm_files_from_gh::resolve::Request {
                    arch,
                    release_igvm_files: v,
                    release_version: OpenhclReleaseVersion::latest(),
                },
            )
        });

        let qemu_system_aarch64 = prebuilt_artifacts.qemu_system_aarch64.then(|| {
            ctx.reqv(|v| {
                crate::resolve_openvmm_qemu::Request::Get(
                    crate::resolve_openvmm_qemu::QemuFile::SystemAarch64,
                    arch,
                    v,
                )
            })
        });

        ctx.emit_rust_step("setting up vmm_tests content dir", |ctx| {
            claim_vars!(
                ctx,
                (
                    test_content_dir,
                    openvmm_repo_path,
                    // built artifacts
                    built_artifacts,
                    // downloaded artifacts
                    test_linux_initrd_x64,
                    test_linux_kernel_x64,
                    test_linux_initrd_aarch64,
                    test_linux_kernel_aarch64,
                    test_linux_bzimage_x64,
                    uefi,
                    virtio_win_dir,
                    release_igvm_files,
                    qemu_system_aarch64,
                )
            );

            done.claim(ctx);

            move |rt| {
                read_vars!(
                    rt,
                    (
                        test_linux_initrd_x64,
                        test_linux_kernel_x64,
                        test_linux_initrd_aarch64,
                        test_linux_kernel_aarch64,
                        test_linux_bzimage_x64,
                        uefi,
                        release_igvm_files,
                        qemu_system_aarch64,
                        test_content_dir
                    )
                );

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

                    if needs_incubator_profiles {
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

                built_artifacts.write(rt, &test_content_dir)?;

                if let Some(release_igvm_files) = release_igvm_files {
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

                if let Some(qemu_system_aarch64) = qemu_system_aarch64 {
                    fs_err::copy(
                        qemu_system_aarch64,
                        test_content_dir.join("qemu-system-aarch64"),
                    )?;
                }

                let arch_dir = |arch| match arch {
                    CommonArch::X86_64 => "x64",
                    CommonArch::Aarch64 => "aarch64",
                };
                if test_linux_initrd_x64.is_some()
                    || test_linux_kernel_x64.is_some()
                    || test_linux_bzimage_x64.is_some()
                {
                    fs_err::create_dir_all(test_content_dir.join(arch_dir(CommonArch::X86_64)))?;
                }
                if test_linux_initrd_aarch64.is_some() || test_linux_kernel_aarch64.is_some() {
                    fs_err::create_dir_all(test_content_dir.join(arch_dir(CommonArch::Aarch64)))?;
                }
                if let Some(test_linux_initrd_x64) = test_linux_initrd_x64 {
                    fs_err::copy(
                        test_linux_initrd_x64,
                        test_content_dir
                            .join(arch_dir(CommonArch::X86_64))
                            .join("initrd"),
                    )?;
                }
                if let Some(test_linux_initrd_aarch64) = test_linux_initrd_aarch64 {
                    fs_err::copy(
                        test_linux_initrd_aarch64,
                        test_content_dir
                            .join(arch_dir(CommonArch::Aarch64))
                            .join("initrd"),
                    )?;
                }
                if let Some(test_linux_kernel_x64) = test_linux_kernel_x64 {
                    fs_err::copy(
                        test_linux_kernel_x64,
                        test_content_dir
                            .join(arch_dir(CommonArch::X86_64))
                            .join("vmlinux"),
                    )?;
                }
                if let Some(test_linux_kernel_aarch64) = test_linux_kernel_aarch64 {
                    fs_err::copy(
                        test_linux_kernel_aarch64,
                        test_content_dir
                            .join(arch_dir(CommonArch::Aarch64))
                            .join("Image"),
                    )?;
                }
                if let Some(test_linux_bzimage_x64) = test_linux_bzimage_x64 {
                    fs_err::copy(
                        test_linux_bzimage_x64,
                        test_content_dir
                            .join(arch_dir(CommonArch::X86_64))
                            .join("bzImage"),
                    )?;
                }

                if let Some(uefi) = uefi {
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
                }

                if let Some(virtio_win_dir) = virtio_win_dir {
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

/// Utility builders which make it easy to "skim off" artifacts required by VMM
/// test execution from other pipeline jobs.
//
// DEVNOTE: this is pub so internal tests can reuse the same builders
pub mod vmm_tests_artifact_builders {
    use super::*;

    vmm_tests_built_artifacts_builder!(
        VmmTestsArtifactsBuilderLinuxX86,
        (
            // windows build machine
            pipette_windows_x64 => PipetteOutput,
            // linux build machine
            nextest_vmm_tests_archive_linux_x64 => NextestVmmTestsArchive,
            openvmm_linux_x64 => OpenvmmOutput,
            openvmm_vhost_linux_x64 => OpenvmmVhostOutput,
            pipette_linux_musl_x64 => PipetteOutput,
            pipette_linux_musl_aarch64 => PipetteOutput,
            prep_steps_linux_musl_x64 => PrepStepsOutput,
            tmk_vmm_linux_musl_x64 => TmkVmmOutput,
            // any machine
            guest_test_uefi_x64 => GuestTestUefiOutput,
            tmks_x64 => TmksOutput,
        )
    );

    vmm_tests_built_artifacts_builder!(
        VmmTestsArtifactsBuilderLinuxMuslX86,
        (
            // windows build machine
            pipette_windows_x64 => PipetteOutput,
            // linux build machine
            nextest_vmm_tests_archive_linux_musl_x64 => NextestVmmTestsArchive,
            openvmm_linux_musl_x64 => OpenvmmOutput,
            openvmm_vhost_linux_musl_x64 => OpenvmmVhostOutput,
            pipette_linux_musl_x64 => PipetteOutput,
            pipette_linux_musl_aarch64 => PipetteOutput,
            prep_steps_linux_musl_x64 => PrepStepsOutput,
            tmk_vmm_linux_musl_x64 => TmkVmmOutput,
            // any machine
            guest_test_uefi_x64 => GuestTestUefiOutput,
            tmks_x64 => TmksOutput,
        )
    );

    vmm_tests_built_artifacts_builder!(
        VmmTestsArtifactsBuilderWindowsX86,
        (
            // windows build machine
            nextest_vmm_tests_archive_windows_x64 => NextestVmmTestsArchive,
            openvmm_windows_x64 => OpenvmmOutput,
            pipette_windows_x64 => PipetteOutput,
            tmk_vmm_windows_x64 => TmkVmmOutput,
            prep_steps_windows_x64 => PrepStepsOutput,
            vmgstool_windows_x64 => VmgstoolOutput,
            vmgstool_dev_windows_x64 => VmgstoolOutput,
            tpm_guest_tests_windows_x64 => TpmGuestTestsOutput,
            test_igvm_agent_rpc_server_windows_x64 => TestIgvmAgentRpcServerOutput,
            // linux build machine
            openhcl_standard_x64 => OpenhclIgvmOutput,
            openhcl_cvm_x64 => OpenhclIgvmOutput,
            openhcl_linux_direct_x64 => OpenhclIgvmOutput,
            pipette_linux_musl_x64 => PipetteOutput,
            tmk_vmm_linux_musl_x64 => TmkVmmOutput,
            tpm_guest_tests_linux_x64 => TpmGuestTestsOutput,
            // any machine
            guest_test_uefi_x64 => GuestTestUefiOutput,
            tmks_x64 => TmksOutput,
        )
    );

    vmm_tests_built_artifacts_builder!(
        VmmTestsArtifactsBuilderWindowsAarch64,
        (
            // windows build machine
            nextest_vmm_tests_archive_windows_aarch64 => NextestVmmTestsArchive,
            openvmm_windows_aarch64 => OpenvmmOutput,
            pipette_windows_aarch64 => PipetteOutput,
            tmk_vmm_windows_aarch64 => TmkVmmOutput,
            vmgstool_windows_aarch64 => VmgstoolOutput,
            vmgstool_dev_windows_aarch64 => VmgstoolOutput,
            // linux build machine
            openhcl_standard_aarch64 => OpenhclIgvmOutput,
            pipette_linux_musl_aarch64 => PipetteOutput,
            tmk_vmm_linux_musl_aarch64 => TmkVmmOutput,
            // any machine
            guest_test_uefi_aarch64 => GuestTestUefiOutput,
            tmks_aarch64 => TmksOutput,
        )
    );

    // Artifact builder for aarch64 Linux VMM tests running via QEMU TCG.
    //
    // The test binaries are aarch64-linux-musl (run inside QEMU), but the
    // incubator binary is x86_64-linux-gnu (runs on the CI host).
    vmm_tests_built_artifacts_builder!(
        VmmTestsArtifactsBuilderLinuxAarch64Tcg,
        (
            // x86_64 CI host binary
            incubator_linux_x64 => IncubatorOutput,
            // aarch64 guest binaries
            nextest_vmm_tests_archive_linux_musl_aarch64 => NextestVmmTestsArchive,
            openvmm_linux_musl_aarch64 => OpenvmmOutput,
            pipette_linux_musl_aarch64 => PipetteOutput,
            guest_test_uefi_aarch64 => GuestTestUefiOutput,
            tmks_aarch64 => TmksOutput,
            tmk_vmm_linux_musl_aarch64 => TmkVmmOutput,
        )
    );
}
