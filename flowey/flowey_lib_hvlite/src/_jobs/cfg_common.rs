// Copyright (C) Microsoft Corporation. All rights reserved.

//! An amalgamated configuration node that streamlines the process of resolving
//! the most common subset of shared configuration requests required by OpenVMM
//! pipelines.

use flowey::node::prelude::*;

#[derive(Clone, Serialize, Deserialize)]
pub struct LocalOnlyParams {
    /// Prompt the user before certain interesting operations (e.g:
    /// installing packages from apt)
    pub interactive: bool,
    /// Automatically install any necessary system dependencies / tools.
    pub auto_install: bool,
    /// (WSL2 only) Use `mono` to run `nuget.exe`, instead of using native
    /// WSL2 interop.
    pub force_nuget_mono: bool,
    /// Claim that nuget is using an external auth mechanism, and Azure
    /// Credential Provider doesn't need to be present to pull down required
    /// packages.
    pub external_nuget_auth: bool,
    /// Ignore the Rust version requirement, and use whatever toolchain the user
    /// currently has installed.
    pub ignore_rust_version: bool,
}

flowey_request! {
    #[derive(Clone)]
    pub struct Params {
        pub local_only: Option<LocalOnlyParams>,
        pub verbose: ReadVar<bool>,
        pub locked: bool,
        pub deny_warnings: bool,
    }
}

new_simple_flow_node!(struct Node);

impl SimpleFlowNode for Node {
    type Request = Params;

    fn imports(ctx: &mut ImportCtx<'_>) {
        ctx.import::<crate::download_lxutil::Node>();
        ctx.import::<crate::download_openhcl_kernel_package::Node>();
        ctx.import::<crate::download_openvmm_deps::Node>();
        ctx.import::<crate::download_uefi_mu_msvm::Node>();
        ctx.import::<crate::git_checkout_openvmm_repo::Node>();
        ctx.import::<crate::init_openvmm_cargo_config_deny_warnings::Node>();
        ctx.import::<crate::install_git_credential_manager::Node>();
        ctx.import::<crate::install_openvmm_rust_build_essential::Node>();
        ctx.import::<flowey_lib_common::ado_task_azure_key_vault::Node>();
        ctx.import::<flowey_lib_common::ado_task_nuget_authenticate::Node>();
        ctx.import::<flowey_lib_common::cfg_cargo_common_flags::Node>();
        ctx.import::<flowey_lib_common::download_azcopy::Node>();
        ctx.import::<flowey_lib_common::download_cargo_nextest::Node>();
        ctx.import::<flowey_lib_common::download_nuget_exe::Node>();
        ctx.import::<flowey_lib_common::download_protoc::Node>();
        ctx.import::<flowey_lib_common::git_checkout::Node>();
        ctx.import::<flowey_lib_common::install_apt_pkg::Node>();
        ctx.import::<flowey_lib_common::install_azure_cli::Node>();
        ctx.import::<flowey_lib_common::install_git::Node>();
        ctx.import::<flowey_lib_common::install_nodejs::Node>();
        ctx.import::<flowey_lib_common::install_nuget_azure_credential_provider::Node>();
        ctx.import::<flowey_lib_common::install_rust::Node>();
        ctx.import::<flowey_lib_common::nuget_install_package::Node>();
        ctx.import::<flowey_lib_common::run_cargo_nextest_run::Node>();
        ctx.import::<flowey_lib_common::use_gh_cli::Node>();
        ctx.import::<flowey_lib_common::gh_download_azure_key_vault_secret::Node>();
    }

    fn process_request(request: Self::Request, ctx: &mut NodeCtx<'_>) -> anyhow::Result<()> {
        let Params {
            local_only,
            verbose,
            locked,
            deny_warnings,
        } = request;

        if matches!(ctx.backend(), FlowBackend::Github) {
            if local_only.is_some() {
                anyhow::bail!("can only set `local_only` params when using Local backend");
            }

            ctx.req(flowey_lib_common::install_azure_cli::Request::AutoInstall(
                true,
            ));
            ctx.req(flowey_lib_common::install_rust::Request::AutoInstall(true));
            ctx.req(flowey_lib_common::install_rust::Request::IgnoreVersion(
                false,
            ));

            // {
            //     let (gh_token, write_gh_token) = ctx.new_secret_var();
            //     ctx.req(
            //         flowey_lib_common::gh_download_azure_key_vault_secret::GetSecret {
            //             key_vault_name: "HvLite-PATs".to_string(),
            //             secret: "GitHub-CLI-PAT".into(),
            //             resolved_secret: write_gh_token,
            //         },
            //     );

            //     ctx.req(flowey_lib_common::use_gh_cli::Request::WithAuth(
            //         flowey_lib_common::use_gh_cli::GhCliAuth::AuthToken(gh_token),
            //     ));
            // }
        } else if matches!(ctx.backend(), FlowBackend::Ado) {
            if local_only.is_some() {
                anyhow::bail!("can only set `local_only` params when using Local backend");
            }

            ctx.req(
                flowey_lib_common::ado_task_nuget_authenticate::Request::ServiceConnection(
                    "AzureDevFeed".into(),
                ),
            );

            ctx.req(flowey_lib_common::install_azure_cli::Request::AutoInstall(
                true,
            ));

            {
                let (read_token, write_token) = ctx.new_secret_var();

                ctx.req(flowey_lib_common::ado_task_azure_key_vault::Request {
                    subscription: "HvLite Azure".into(),
                    key_vault_name: "HvLite-PATs".into(),
                    secret: "GitHub-CLI-PAT".into(),
                    resolved_secret: write_token,
                });

                ctx.req(flowey_lib_common::use_gh_cli::Request::WithAuth(
                    flowey_lib_common::use_gh_cli::GhCliAuth::AuthToken(read_token),
                ));
            }
        } else if matches!(ctx.backend(), FlowBackend::Local) {
            let local_only =
                local_only.ok_or(anyhow::anyhow!("missing essential request: local_only"))?;

            let LocalOnlyParams {
                interactive,
                auto_install,
                force_nuget_mono,
                external_nuget_auth,
                ignore_rust_version,
            } = local_only;

            // wire up `interactive`
            {
                ctx.req(
                    flowey_lib_common::install_apt_pkg::Request::LocalOnlyInteractive(interactive),
                );
                ctx.req(
                    flowey_lib_common::nuget_install_package::Request::LocalOnlyInteractive(
                        interactive,
                    ),
                );
                ctx.req(
                    flowey_lib_common::nuget_install_package::Request::LocalOnlyInteractive(
                        interactive,
                    ),
                );
                ctx.req(flowey_lib_common::use_gh_cli::Request::WithAuth(
                    flowey_lib_common::use_gh_cli::GhCliAuth::LocalOnlyInteractive,
                ));
                ctx.req(flowey_lib_common::install_rust::Request::IgnoreVersion(
                    ignore_rust_version,
                ));
            }

            // wire up auto_install
            {
                ctx.req(flowey_lib_common::install_rust::Request::AutoInstall(
                    auto_install,
                ));
                ctx.req(
                    flowey_lib_common::install_nuget_azure_credential_provider::Request::LocalOnlyAutoInstall(
                        auto_install,
                    ),
                );
                ctx.req(
                    flowey_lib_common::install_apt_pkg::Request::LocalOnlySkipUpdate(!auto_install),
                );
                ctx.req(flowey_lib_common::install_nodejs::Request::AutoInstall(
                    auto_install,
                ));
                ctx.req(flowey_lib_common::install_azure_cli::Request::AutoInstall(
                    auto_install,
                ));
                ctx.req(
                    flowey_lib_common::install_git::Request::LocalOnlyAutoInstall(auto_install),
                );
            }

            //
            // wire up misc.
            //
            ctx.req(
                flowey_lib_common::install_nuget_azure_credential_provider::Request::LocalOnlySkipAuthCheck(
                    external_nuget_auth,
                ),
            );

            ctx.req(
                flowey_lib_common::download_nuget_exe::Request::LocalOnlyForceWsl2MonoNugetExe(
                    force_nuget_mono,
                ),
            );

            // FUTURE: if we ever spin up a openvmm setup utility - it might be
            // interesting to distribute a flowey-based tool that also clones
            // the repo.
            ctx.req(flowey_lib_common::git_checkout::Request::LocalOnlyRequireExistingClones(true));
        } else {
            anyhow::bail!("unsupported backend")
        }

        ctx.requests::<flowey_lib_common::cfg_cargo_common_flags::Node>([
            flowey_lib_common::cfg_cargo_common_flags::Request::SetVerbose(verbose),
            flowey_lib_common::cfg_cargo_common_flags::Request::SetLocked(locked),
        ]);

        ctx.req(
            crate::init_openvmm_cargo_config_deny_warnings::Request::DenyWarnings(deny_warnings),
        );

        Ok(())
    }
}
