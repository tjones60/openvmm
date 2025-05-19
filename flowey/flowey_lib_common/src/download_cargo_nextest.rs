// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Download (and optionally, install) a copy of `cargo-nextest`.

use crate::cache::CacheHit;
use flowey::node::prelude::*;

flowey_request! {
    pub enum Request {
        /// Version of `cargo nextest` to install (e.g: "0.9.57")
        Version(String),
        /// Install `cargo-nextest` as a standalone binary, without requiring Rust
        /// to be installed.
        ///
        /// Useful when running archived nextest tests in a separate job.
        InstallStandalone(WriteVar<PathBuf>),
    }
}

new_flow_node!(struct Node);

impl FlowNode for Node {
    type Request = Request;

    fn imports(ctx: &mut ImportCtx<'_>) {
        ctx.import::<crate::cache::Node>();
    }

    fn emit(requests: Vec<Self::Request>, ctx: &mut NodeCtx<'_>) -> anyhow::Result<()> {
        let mut version = None;
        let mut install_standalone = Vec::new();

        for req in requests {
            match req {
                Request::Version(v) => same_across_all_reqs("Version", &mut version, v)?,
                Request::InstallStandalone(v) => install_standalone.push(v),
            }
        }

        let version = version.ok_or(anyhow::anyhow!("Missing essential request: Version"))?;
        let install_standalone = install_standalone;

        // -- end of req processing -- //

        if install_standalone.is_empty() {
            return Ok(());
        }

        let cargo_nextest_bin = ctx.platform().binary("cargo-nextest");

        let cache_dir = ctx.emit_rust_stepv("create cargo-nextest cache dir", |_| {
            |_| Ok(std::env::current_dir()?.absolute()?)
        });

        let cache_key = ReadVar::from_static(format!("cargo-nextest-{version}"));
        let hitvar = ctx.reqv(|v| {
            crate::cache::Request {
                label: "cargo-nextest".into(),
                dir: cache_dir.clone(),
                key: cache_key,
                restore_keys: None, // we want an exact hit
                hitvar: v,
            }
        });

        ctx.emit_rust_step("downloading cargo-nextest", |ctx| {
            let install_standalone = install_standalone.claim(ctx);
            let cache_dir = cache_dir.claim(ctx);
            let hitvar = hitvar.claim(ctx);

            move |rt| {
                let cache_dir = rt.read(cache_dir);

                let cached_bin_path = cache_dir.join(&cargo_nextest_bin);

                if !matches!(rt.read(hitvar), CacheHit::Hit) {
                    let sh = xshell::Shell::new()?;

                    xshell::cmd!(sh, "curl --fail -L https://get.nexte.st/{version}/{target}.tar.gz -o nextest.tar.gz").run()?;
                    xshell::cmd!(sh, "tar -xf gh.tar.gz").run()?;

                    // move the downloaded bin into the cache dir
                    fs_err::rename(out_bin, &cached_bin_path)?;
                    let final_bin = cached_bin_path.absolute()?;
                }

                assert!(cached_bin_path.exists());
                for var in install_standalone {
                    rt.write(var, &cached_bin_path)
                }

                Ok(())
            }
        });

        Ok(())
    }
}
