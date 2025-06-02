// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Generate a cargo-nextest run command.

use crate::run_cargo_build::CargoBuildProfile;
use flowey::node::prelude::*;
use std::collections::BTreeMap;
use std::ffi::OsString;

#[derive(Serialize, Deserialize)]
pub enum NextestInvocation {
    // when tests are already built and provided via archive
    Standalone { nextest_bin: PathBuf },
    // when tests need to be compiled first
    WithCargo { rust_toolchain: Option<String> },
}

flowey_request! {
pub struct Request {
    /// What kind of test run this is (inline build vs. from nextest archive).
    pub archive_file: ReadVar<PathBuf>,
    /// Target to run the tests on
    pub target: ReadVar<target_lexicon::Triple>,
    /// Use the provided nextest bin or cargo
    pub nextest_invocation: NextestInvocation,
    /// Working directory the test archive was created from.
    pub working_dir: ReadVar<PathBuf>,
    /// Path to `.config/nextest.toml`
    pub config_file: ReadVar<PathBuf>,
    /// Path to any tool-specific config files
    pub tool_config_files: Vec<(String, ReadVar<PathBuf>)>,
    /// Nextest profile to use when running the source code (as defined in the
    /// `.config.nextest.toml`).
    pub nextest_profile: String,
    /// Nextest test filter expression
    pub nextest_filter_expr: Option<String>,
    /// Whether to run ignored tests
    pub run_ignored: bool,
    /// Additional env vars set when executing the tests.
    pub extra_env: Option<ReadVar<BTreeMap<String, String>>>,
    /// Command for running the tests
    pub command: WriteVar<String>,
}
}

new_flow_node!(struct Node);

impl FlowNode for Node {
    type Request = Request;

    fn imports(ctx: &mut ImportCtx<'_>) {
        ctx.import::<crate::cfg_cargo_common_flags::Node>();
        ctx.import::<crate::download_cargo_nextest::Node>();
        ctx.import::<crate::install_cargo_nextest::Node>();
        ctx.import::<crate::install_rust::Node>();
    }

    fn emit(requests: Vec<Self::Request>, ctx: &mut NodeCtx<'_>) -> anyhow::Result<()> {
        for Request {
            archive_file,
            target,
            nextest_invocation,
            working_dir,
            config_file,
            tool_config_files,
            nextest_profile,
            extra_env,
            nextest_filter_expr,
            run_ignored,
            command,
        } in requests
        {
            ctx.emit_rust_step(format!("generate nextest command"), |ctx| {
                let working_dir = working_dir.claim(ctx);
                let config_file = config_file.claim(ctx);
                let tool_config_files = tool_config_files
                    .into_iter()
                    .map(|(a, b)| (a, b.claim(ctx)))
                    .collect::<Vec<_>>();
                let extra_env = extra_env.claim(ctx);
                let target = target.claim(ctx);

                move |rt| {
                    let working_dir = rt.read(working_dir);
                    let config_file = rt.read(config_file);
                    let mut with_env = rt.read(extra_env).unwrap_or_default();
                    let target = rt.read(target);

                    let windows_via_wsl2 = crate::_util::running_in_wsl(rt)
                        && matches!(
                            target.operating_system,
                            target_lexicon::OperatingSystem::Windows
                        );

                    let maybe_convert_path = |path: PathBuf| -> PathBuf {
                        if windows_via_wsl2 {
                            crate::_util::wslpath::linux_to_win(path)
                        } else {
                            path
                        }
                    };

                    // the invocation of `nextest run` is quite different
                    // depending on whether this is an archived run or not, as
                    // archives don't require passing build args (after all -
                    // those were passed when the archive was built), nor do
                    // they require having cargo installed.
                    let (nextest_invocation, build_args, build_env) = match nextest_invocation {
                        NextestInvocation::Standalone { nextest_bin } => {
                            let build_args = vec![
                                "--archive-file".into(),
                                maybe_convert_path(rt.read(archive_file))
                                    .display()
                                    .to_string(),
                            ];

                            let nextest_invocation = NextestInvocation::Standalone {
                                nextest_bin: rt.read(nextest_bin),
                            };

                            (nextest_invocation, build_args, BTreeMap::default())
                        }
                        NextestInvocation::WithCargo { rust_toolchain } => {
                            let (mut build_args, build_env) = cargo_nextest_build_args_and_env(
                                rt.read(cargo_flags),
                                profile,
                                target,
                                rt.read(packages),
                                features,
                                unstable_panic_abort_tests,
                                no_default_features,
                                rt.read(extra_env),
                            );

                            let nextest_invocation = NextestInvocation::WithCargo {
                                rust_toolchain: rt.read(rust_toolchain),
                            };

                            // nextest also requires explicitly specifying the
                            // path to a cargo-metadata.json file when running
                            // using --workspace-remap (which do we below).
                            let cargo_metadata_path = std::env::current_dir()?
                                .absolute()?
                                .join("cargo_metadata.json");

                            let sh = xshell::Shell::new()?;
                            sh.change_dir(&working_dir);
                            let output =
                                xshell::cmd!(sh, "cargo metadata --format-version 1").output()?;
                            let cargo_metadata = String::from_utf8(output.stdout)?;
                            fs_err::write(&cargo_metadata_path, cargo_metadata)?;

                            build_args.push("--cargo-metadata".into());
                            build_args.push(cargo_metadata_path.display().to_string());

                            (nextest_invocation, build_args, build_env)
                        }
                    };

                    let mut args: Vec<OsString> = Vec::new();

                    let argv0: OsString = match nextest_invocation {
                        NextestInvocation::Standalone { nextest_bin } => nextest_bin.into(),
                        NextestInvocation::WithCargo { rust_toolchain } => {
                            if let Some(rust_toolchain) = rust_toolchain {
                                args.extend(["run".into(), rust_toolchain.into(), "cargo".into()]);
                                "rustup".into()
                            } else {
                                "cargo".into()
                            }
                        }
                    };

                    args.extend([
                        "nextest".into(),
                        "run".into(),
                        "--profile".into(),
                        (&nextest_profile).into(),
                        "--config-file".into(),
                        maybe_convert_path(config_file).into(),
                        "--workspace-remap".into(),
                        maybe_convert_path(working_dir.clone()).into(),
                    ]);

                    for (tool, config_file) in tool_config_files {
                        args.extend([
                            "--tool-config-file".into(),
                            format!(
                                "{}:{}",
                                tool,
                                maybe_convert_path(rt.read(config_file)).display()
                            )
                            .into(),
                        ]);
                    }

                    args.extend(build_args.into_iter().map(Into::into));

                    if let Some(nextest_filter_expr) = nextest_filter_expr {
                        args.push("--filter-expr".into());
                        args.push(nextest_filter_expr.into());
                    }

                    if run_ignored {
                        args.push("--run-ignored".into());
                        args.push("all".into());
                    }

                    if let Some(fail_fast) = fail_fast {
                        if fail_fast {
                            args.push("--fail-fast".into());
                        } else {
                            args.push("--no-fail-fast".into());
                        }
                    }

                    // useful default to have
                    if !with_env.contains_key("RUST_BACKTRACE") {
                        with_env.insert("RUST_BACKTRACE".into(), "1".into());
                    }

                    // if running in CI, no need to waste time with incremental
                    // build artifacts
                    if !matches!(rt.backend(), FlowBackend::Local) {
                        with_env.insert("CARGO_INCREMENTAL".into(), "0".into());
                    }

                    // also update WSLENV in cases where we're running windows tests via WSL2
                    if crate::_util::running_in_wsl(rt) {
                        let old_wslenv = std::env::var("WSLENV");
                        let new_wslenv = with_env.keys().cloned().collect::<Vec<_>>().join(":");
                        with_env.insert(
                            "WSLENV".into(),
                            format!(
                                "{}{}",
                                old_wslenv.map(|s| s + ":").unwrap_or_default(),
                                new_wslenv
                            ),
                        );
                    }

                    // the build_env vars don't need to be mirrored to WSLENV,
                    // and so they are only injected after the WSLENV code has
                    // run.
                    with_env.extend(build_env);

                    let arg_string = || {
                        args.iter()
                            .map(|v| format!("'{}'", v.to_string_lossy()))
                            .collect::<Vec<_>>()
                            .join(" ")
                    };

                    let env_string = match target.operating_system {
                        target_lexicon::OperatingSystem::Windows => with_env
                            .iter()
                            .map(|(k, v)| format!("$env:{k}='{v}'"))
                            .collect::<Vec<_>>()
                            .join("; "),
                        _ => with_env
                            .iter()
                            .map(|(k, v)| format!("{k}='{v}'"))
                            .collect::<Vec<_>>()
                            .join(" "),
                    };

                    log::info!(
                        "{} {} {}",
                        env_string,
                        argv0.to_string_lossy(),
                        arg_string()
                    );

                    // nextest has meaningful exit codes that we want to parse.
                    // <https://github.com/nextest-rs/nextest/blob/main/nextest-metadata/src/exit_codes.rs#L12>
                    //
                    // unfortunately, xshell doesn't have a mode where it can
                    // both emit to stdout/stderr, _and_ report the specific
                    // exit code of the process.
                    //
                    // So we have to use the raw process API instead.
                    let mut command = std::process::Command::new(&argv0);
                    command.args(&args).envs(with_env).current_dir(&working_dir);

                    Ok(())
                }
            });
        }

        Ok(())
    }
}

// shared with `cargo_nextest_archive`
pub(crate) fn cargo_nextest_build_args_and_env(
    cargo_flags: crate::cfg_cargo_common_flags::Flags,
    cargo_profile: CargoBuildProfile,
    target: target_lexicon::Triple,
    packages: build_params::TestPackages,
    features: build_params::FeatureSet,
    unstable_panic_abort_tests: Option<build_params::PanicAbortTests>,
    no_default_features: bool,
    mut extra_env: BTreeMap<String, String>,
) -> (Vec<String>, BTreeMap<String, String>) {
    let locked = cargo_flags.locked.then_some("--locked");
    let verbose = cargo_flags.verbose.then_some("--verbose");
    let cargo_profile = match &cargo_profile {
        CargoBuildProfile::Debug => "dev",
        CargoBuildProfile::Release => "release",
        CargoBuildProfile::Custom(s) => s,
    };
    let target = target.to_string();

    let packages: Vec<String> = {
        // exclude benches
        let mut v = vec!["--tests".into(), "--bins".into()];

        match packages {
            build_params::TestPackages::Workspace { exclude } => {
                v.push("--workspace".into());
                for crate_name in exclude {
                    v.push("--exclude".into());
                    v.push(crate_name);
                }
            }
            build_params::TestPackages::Crates { crates } => {
                for crate_name in crates {
                    v.push("-p".into());
                    v.push(crate_name);
                }
            }
        }

        v
    };

    let features: Vec<String> = {
        let mut v = Vec::new();

        if no_default_features {
            v.push("--no-default-features".into())
        }

        match features {
            build_params::FeatureSet::All => v.push("--all-features".into()),
            build_params::FeatureSet::Specific(features) => {
                if !features.is_empty() {
                    v.push("--features".into());
                    v.push(features.join(","));
                }
            }
        }

        v
    };

    let (z_panic_abort_tests, use_rustc_bootstrap) = match unstable_panic_abort_tests {
        Some(kind) => (
            Some("-Zpanic-abort-tests"),
            match kind {
                build_params::PanicAbortTests::UsingNightly => false,
                build_params::PanicAbortTests::UsingRustcBootstrap => true,
            },
        ),
        None => (None, false),
    };

    let mut args = Vec::new();
    args.extend(locked.map(Into::into));
    args.extend(verbose.map(Into::into));
    args.push("--cargo-profile".into());
    args.push(cargo_profile.into());
    args.extend(z_panic_abort_tests.map(Into::into));
    args.push("--target".into());
    args.push(target);
    args.extend(packages);
    args.extend(features);

    let mut env = BTreeMap::new();
    if use_rustc_bootstrap {
        env.insert("RUSTC_BOOTSTRAP".into(), "1".into());
    }
    env.append(&mut extra_env);

    (args, env)
}

// FUTURE: this seems like something a proc-macro can help with...
impl build_params::NextestBuildParams {
    pub fn claim(self, ctx: &mut StepCtx<'_>) -> build_params::NextestBuildParams<VarClaimed> {
        let build_params::NextestBuildParams {
            packages,
            features,
            no_default_features,
            unstable_panic_abort_tests,
            target,
            profile,
            extra_env,
        } = self;

        build_params::NextestBuildParams {
            packages: packages.claim(ctx),
            features,
            no_default_features,
            unstable_panic_abort_tests,
            target,
            profile,
            extra_env: extra_env.claim(ctx),
        }
    }
}

// FUTURE: this seems like something a proc-macro can help with...
impl RunKindDeps {
    pub fn claim(self, ctx: &mut StepCtx<'_>) -> RunKindDeps<VarClaimed> {
        match self {
            RunKindDeps::BuildAndRun {
                params,
                nextest_installed,
                rust_toolchain,
                cargo_flags,
            } => RunKindDeps::BuildAndRun {
                params: params.claim(ctx),
                nextest_installed: nextest_installed.claim(ctx),
                rust_toolchain: rust_toolchain.claim(ctx),
                cargo_flags: cargo_flags.claim(ctx),
            },
            RunKindDeps::RunFromArchive {
                archive_file,
                nextest_bin,
                target,
            } => RunKindDeps::RunFromArchive {
                archive_file: archive_file.claim(ctx),
                nextest_bin: nextest_bin.claim(ctx),
                target: target.claim(ctx),
            },
        }
    }
}
