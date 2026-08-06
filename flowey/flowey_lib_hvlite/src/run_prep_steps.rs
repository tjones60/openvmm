// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Raw bindings to `prep_steps`, used to prepare test images before running tests.

use crate::build_prep_steps::PrepStepsOutput;
use flowey::node::prelude::*;
use std::collections::BTreeMap;
use target_lexicon::OperatingSystem;

#[derive(Serialize, Deserialize)]
pub enum PrepStepsSource {
    TestContentDir(target_lexicon::Triple),
    DirectOutput(ReadVar<PrepStepsOutput>),
}

flowey_request! {
    pub struct Request {
        /// Path to prep_steps bin to use. If not specified, try to find a
        /// binary in VMM_TESTS_CONTENT_DIR.
        pub prep_steps: PrepStepsSource,
        /// Arguments to pass to prep_steps (e.g. "standard" or "no-vmbus")
        pub args: Vec<String>,
        /// Environment variables to set when running prep_steps
        pub env: ReadVar<BTreeMap<String, String>>,
        /// Completion indicator
        pub done: WriteVar<SideEffect>,
    }
}

new_simple_flow_node!(struct Node);

impl SimpleFlowNode for Node {
    type Request = Request;

    fn imports(_ctx: &mut ImportCtx<'_>) {}

    fn process_request(request: Self::Request, ctx: &mut NodeCtx<'_>) -> anyhow::Result<()> {
        let Request {
            prep_steps,
            args,
            env,
            done,
        } = request;

        let prep_steps = match prep_steps {
            PrepStepsSource::DirectOutput(output) => output.map(ctx, |o| match o {
                PrepStepsOutput::LinuxBin { bin, .. } => bin,
                PrepStepsOutput::WindowsBin { exe, .. } => exe,
            }),
            PrepStepsSource::TestContentDir(triple) => {
                ctx.emit_rust_stepv("resolve prep_steps", |ctx| {
                    let env = env.clone().claim(ctx);
                    move |rt| {
                        let env = rt.read(env);
                        let test_content_dir = env
                            .get("VMM_TESTS_CONTENT_DIR")
                            .context("VMM_TESTS_CONTENT_DIR not set")?;

                        let test_content_dir = if flowey_lib_common::_util::running_in_wsl(rt) {
                            flowey_lib_common::_util::wslpath::win_to_linux(rt, test_content_dir)
                        } else {
                            PathBuf::from(test_content_dir)
                        };

                        let exe = test_content_dir.join(match triple.operating_system {
                            OperatingSystem::Windows => "prep_steps.exe",
                            _ => "prep_steps",
                        });
                        if !exe.exists() {
                            anyhow::bail!("prep_steps bin not found at {}", exe.display());
                        }
                        Ok(exe)
                    }
                })
            }
        };

        ctx.emit_rust_step("running vmm_test prep_steps", |ctx| {
            let prep_steps = prep_steps.claim(ctx);
            let env = env.claim(ctx);
            done.claim(ctx);
            move |rt| {
                let prep_steps = rt.read(prep_steps);
                let env = rt.read(env);

                #[cfg(windows)]
                if !matches!(rt.backend(), FlowBackend::Local) {
                    // Shutdown and remove any running VMs that might be using the disk
                    // generated during a previous test run. (CI only)
                    let vms = powershell_builder::PowerShellBuilder::new()
                        .cmdlet("Get-VM")
                        .finish()
                        .build()
                        .output()?;
                    log::info!(
                        "removing any existing VMs: {}",
                        String::from_utf8_lossy(&vms.stdout)
                    );

                    powershell_builder::PowerShellBuilder::new()
                        .cmdlet("Get-VM")
                        .pipeline()
                        .cmdlet("Stop-VM")
                        .flag("TurnOff")
                        .finish()
                        .build()
                        .output()?;

                    powershell_builder::PowerShellBuilder::new()
                        .cmdlet("Get-VM")
                        .pipeline()
                        .cmdlet("Remove-VM")
                        .flag("Force")
                        .finish()
                        .build()
                        .output()?;
                }

                // When running a Windows exe from WSL2, environment variables don't
                // automatically propagate. We need to set WSLENV to tell WSL which
                // env vars to share with Windows processes.
                let is_windows_exe_via_wsl = flowey_lib_common::_util::running_in_wsl(rt)
                    && prep_steps.extension().is_some_and(|ext| ext == "exe");

                let mut env = env;
                if is_windows_exe_via_wsl {
                    // Inherit the existing WSLENV value if any and append any
                    // new vars to add. No /p flag needed since paths are
                    // already converted to Windows format.
                    let old_wslenv = std::env::var("WSLENV");
                    let new_wslenv = env.keys().cloned().collect::<Vec<_>>().join(":");
                    env.insert(
                        "WSLENV".into(),
                        format!(
                            "{}{}",
                            old_wslenv.map(|s| s + ":").unwrap_or_default(),
                            new_wslenv
                        ),
                    );
                }

                flowey::shell_cmd!(rt, "{prep_steps}")
                    .args(&args)
                    .envs(env)
                    .run()?;

                Ok(())
            }
        });

        Ok(())
    }
}
