// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Download a github release artifact

use flowey::node::prelude::*;

#[derive(Serialize, Deserialize)]
pub struct GhReleaseParams<C = VarNotClaimed> {
    /// The github repo path
    pub repo: ReadVar<String, C>,
    /// Release tag
    pub tag: ReadVar<String, C>,
    /// Release title
    pub title: ReadVar<String, C>,
    /// Target branch or full commit SHA
    pub target: ReadVar<String, C>,

    /// Files to upload
    pub files: Vec<ReadVar<PathBuf, C>>,

    pub done: WriteVar<SideEffect, C>,
}

flowey_request! {
    pub struct Request(pub GhReleaseParams);
}

new_flow_node!(struct Node);

impl FlowNode for Node {
    type Request = Request;

    fn imports(ctx: &mut ImportCtx<'_>) {
        ctx.import::<crate::use_gh_cli::Node>();
    }

    fn emit(requests: Vec<Self::Request>, ctx: &mut NodeCtx<'_>) -> anyhow::Result<()> {
        if requests.is_empty() {
            return Ok(());
        }

        let gh_cli = ctx.reqv(crate::use_gh_cli::Request::Get);

        ctx.emit_rust_step("publish github releases", |ctx| {
            let gh_cli = gh_cli.claim(ctx);
            let requests = requests
                .into_iter()
                .map(|r| r.0.claim(ctx))
                .collect::<Vec<_>>();

            move |rt| {
                let gh_cli = rt.read(gh_cli);

                let sh = xshell::Shell::new()?;

                for req in requests {
                    let repo = rt.read(req.repo);
                    let tag = rt.read(req.tag);
                    let title = rt.read(req.title);
                    let target = rt.read(req.target);
                    let files = rt.read(req.files);

                    xshell::cmd!(
                        sh,
                        "{gh_cli} release create --repo {repo} --title {title} --target {target} {tag} {files...}"
                    )
                    .run()?;
                }

                Ok(())
            }
        });

        Ok(())
    }
}

impl GhReleaseParams {
    pub fn claim(self, ctx: &mut StepCtx<'_>) -> GhReleaseParams<VarClaimed> {
        let GhReleaseParams {
            repo,
            tag,
            title,
            target,
            files,
            done,
        } = self;

        GhReleaseParams {
            repo: repo.claim(ctx),
            tag: tag.claim(ctx),
            title: title.claim(ctx),
            target: target.claim(ctx),
            files: files.claim(ctx),
            done: done.claim(ctx),
        }
    }
}
