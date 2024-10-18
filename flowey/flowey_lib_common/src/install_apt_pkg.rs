// Copyright (C) Microsoft Corporation. All rights reserved.

//! Globally install a package via `apt` on debian-based linux systems

use flowey::node::prelude::*;
use std::collections::BTreeSet;

flowey_request! {
    pub enum Request {
        /// Whether to prompt the user before installing packages
        LocalOnlyInteractive(bool),
        /// Whether to skip the `apt-update` step, and allow stale
        /// packages
        LocalOnlySkipUpdate(bool),
        /// Install the specified package(s)
        Install {
            package_names: Vec<String>,
            done: WriteVar<SideEffect>,
        },
    }
}

new_flow_node!(struct Node);

impl FlowNode for Node {
    type Request = Request;

    fn imports(_ctx: &mut ImportCtx<'_>) {}

    fn emit(requests: Vec<Self::Request>, ctx: &mut NodeCtx<'_>) -> anyhow::Result<()> {
        let mut skip_update = None;
        let mut interactive = None;
        let mut packages = BTreeSet::new();
        let mut did_install = Vec::new();

        for req in requests {
            match req {
                Request::Install {
                    package_names,
                    done,
                } => {
                    packages.extend(package_names);
                    did_install.push(done);
                }
                Request::LocalOnlyInteractive(v) => {
                    same_across_all_reqs("LocalOnlyInteractive", &mut interactive, v)?
                }
                Request::LocalOnlySkipUpdate(v) => {
                    same_across_all_reqs("LocalOnlySkipUpdate", &mut skip_update, v)?
                }
            }
        }

        let packages = packages;
        let (skip_update, interactive) =
            if matches!(ctx.backend(), FlowBackend::Ado | FlowBackend::Github) {
                if interactive.is_some() {
                    anyhow::bail!(
                        "can only use `LocalOnlyInteractive` when using the Local backend"
                    );
                }

                if skip_update.is_some() {
                    anyhow::bail!(
                        "can only use `LocalOnlySkipUpdate` when using the Local backend"
                    );
                }

                (false, false)
            } else if matches!(ctx.backend(), FlowBackend::Local) {
                (
                    skip_update.ok_or(anyhow::anyhow!(
                        "Missing essential request: LocalOnlySkipUpdate",
                    ))?,
                    interactive.ok_or(anyhow::anyhow!(
                        "Missing essential request: LocalOnlyInteractive",
                    ))?,
                )
            } else {
                anyhow::bail!("unsupported backend")
            };

        // -- end of req processing -- //

        if did_install.is_empty() {
            return Ok(());
        }

        // maybe a questionable design choice... but we'll allow non-linux
        // platforms from taking a dep on this, and simply report that it was
        // installed.
        if !matches!(ctx.platform(), FlowPlatform::Linux) {
            ctx.emit_side_effect_step([], did_install);
            return Ok(());
        }

        let need_install =
            ctx.emit_rust_stepv("checking if apt packages need to be installed", |_ctx| {
                let packages = packages.clone();
                move |_| {
                    let sh = xshell::Shell::new()?;

                    let mut installed_packages = BTreeSet::new();
                    let packages_to_check = &packages;

                    let fmt = "${binary:Package}\n";
                    let output = xshell::cmd!(sh, "dpkg-query -W -f={fmt} {packages_to_check...}")
                        .ignore_status()
                        .output()?;
                    let output = String::from_utf8(output.stdout)?;
                    for ln in output.trim().lines() {
                        let package = match ln.split_once(':') {
                            Some((package, _arch)) => package,
                            None => ln,
                        };
                        let no_existing = installed_packages.insert(package.to_owned());
                        assert!(no_existing);
                    }

                    // apt won't re-install packages that are already
                    // up-to-date, so this sort of coarse-grained signal should
                    // be plenty sufficient.
                    Ok(installed_packages != packages)
                }
            });

        ctx.emit_rust_step("installing `apt` packages", move |ctx| {
            let packages = packages.clone();
            let need_install = need_install.claim(ctx);
            did_install.claim(ctx);
            move |rt| {
                let need_install = rt.read(need_install);

                if !need_install {
                    return Ok(());
                }

                let sh = xshell::Shell::new()?;

                if !skip_update {
                    xshell::cmd!(sh, "i=0; while [ $i -lt 60 ] && sudo fuser /var/lib/dpkg/lock-frontend >/dev/null 2>&1 ; do ((i++)); sleep 1; done; sudo apt-get update").run()?;
                }
                let auto_accept = (!interactive).then_some("-y");
                xshell::cmd!(
                    sh,
                    "sudo apt-get -o DPkg::Lock::Timeout=60 install {auto_accept...} {packages...}"
                )
                .run()?;

                Ok(())
            }
        });

        Ok(())
    }
}
