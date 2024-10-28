// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Publish JUnit test results.
//!
//! On supported platforms (ADO), this will hook into the backend's native JUnit
//! handling. On Github, this will publish an artifacts with the raw XML files.
//! When running locally, this will optionally copy the XML files to the provided
//! artifact directory.

use crate::_util::copy_dir_all;
use flowey::node::prelude::*;
use std::collections::BTreeMap;

flowey_request! {
    pub enum Request {
        /// Register a XML file to be published
        Register {
            /// Path to a junit.xml file
            ///
            /// HACK: this is an optional since `flowey` doesn't (yet?) have any way
            /// to perform conditional-requests, and there are instances where nodes
            /// will only conditionally output JUnit XML.
            ///
            /// To keep making forward progress, I've tweaked this node to accept an
            /// optional... but this ain't great.
            junit_xml: ReadVar<Option<PathBuf>>,
            /// Brief string used when publishing the test.
            test_label: String,
            /// Additional attachments for platforms without JUnit integration (not used on ADO)
            attachments: Option<BTreeMap<String, ReadVar<PathBuf>>>,
            /// Side-effect confirming that the publish has succeeded
            done: WriteVar<SideEffect>,
        },
        /// (Optional) publish all registered JUnit XML files to the provided dir
        /// Only supported on local backend
        PublishToArtifact(ReadVar<PathBuf>, WriteVar<SideEffect>),
    }
}

new_flow_node!(struct Node);

impl FlowNode for Node {
    type Request = Request;

    fn imports(_ctx: &mut ImportCtx<'_>) {}

    fn emit(requests: Vec<Self::Request>, ctx: &mut NodeCtx<'_>) -> anyhow::Result<()> {
        struct TestResult {
            junit_xml: ReadVar<Option<PathBuf>>,
            label: String,
            attachments: Option<BTreeMap<String, ReadVar<PathBuf>>>,
            done: WriteVar<SideEffect>,
        }

        let mut xmls = Vec::new();
        let mut artifact_dir = None;

        for req in requests {
            match req {
                Request::Register {
                    junit_xml,
                    test_label,
                    attachments,
                    done,
                } => xmls.push(TestResult {
                    junit_xml,
                    label: test_label,
                    attachments,
                    done,
                }),
                Request::PublishToArtifact(a, b) => same_across_all_reqs_backing_var(
                    "PublishToArtifact",
                    &mut artifact_dir,
                    (a, b),
                )?,
            }
        }

        let xmls = xmls;
        let artifact_dir = artifact_dir;

        if artifact_dir.is_some() && !matches!(ctx.backend(), FlowBackend::Local) {
            anyhow::bail!("Copying to a custom artifact directory is only supported locally.")
        }

        match ctx.backend() {
            FlowBackend::Ado => {
                for TestResult {
                    junit_xml,
                    label,
                    attachments: _,
                    done,
                } in xmls
                {
                    let has_path = junit_xml.map(ctx, |p| p.is_some());
                    let path = junit_xml.map(ctx, |p| {
                        p.map(|p| p.absolute().expect("TEMP").display().to_string())
                            .unwrap_or_default()
                    });
                    ctx.emit_ado_step_with_condition(
                        format!("publish JUnit test results: {label}"),
                        has_path,
                        |ctx| {
                            done.claim(ctx);
                            let path = path.claim(ctx);
                            move |rt| {
                                let path = rt.get_var(path).as_raw_var_name();
                                format!(
                                    r#"
                                    - task: PublishTestResults@2
                                      inputs:
                                        testResultsFormat: 'JUnit'
                                        testResultsFiles: '$({path})'
                                        testRunTitle: '{label}'
                                "#
                                )
                            }
                        },
                    );
                }
            }
            FlowBackend::Github => {
                let mut use_side_effects = Vec::new();
                let mut resolve_side_effects = Vec::new();
                for (
                    idx,
                    TestResult {
                        junit_xml,
                        label,
                        attachments,
                        done,
                    },
                ) in xmls.into_iter().enumerate()
                {
                    let has_path = junit_xml.map(ctx, |p| p.is_some());
                    let path = junit_xml.map(ctx, |p| {
                        p.map(|p| p.absolute().expect("invalid path").display().to_string())
                            .unwrap_or_default()
                    });

                    resolve_side_effects.push(done);
                    use_side_effects.push(
                        ctx.emit_gh_step(
                            format!("publish test results: {label} (JUnit XML)"),
                            "actions/upload-artifact@v4",
                        )
                        .condition(has_path)
                        .with(
                            "name",
                            format!("{}_{idx}_junit_xml", label.replace(' ', "_")),
                        )
                        .with("path", path)
                        .finish(ctx),
                    );
                    if let Some(attachments) = attachments {
                        for (attachment_label, attachment_path) in attachments {
                            let attachment_exists = attachment_path.map(ctx, |p| {
                                p.exists()
                                    && (p.is_file()
                                        || p.read_dir()
                                            .expect("failed to read attachment dir")
                                            .next()
                                            .is_some())
                            });
                            let attachment_path = attachment_path.map(ctx, |p| {
                                p.absolute().expect("invalid path").display().to_string()
                            });
                            use_side_effects.push(
                                ctx.emit_gh_step(
                                    format!("publish test results: {label} ({attachment_label})"),
                                    "actions/upload-artifact@v4",
                                )
                                .condition(attachment_exists)
                                .with(
                                    "name",
                                    format!(
                                        "{}_{idx}_{}",
                                        label.replace(' ', "_"),
                                        attachment_label.replace(' ', "_")
                                    ),
                                )
                                .with("path", attachment_path)
                                .finish(ctx),
                            );
                        }
                    }
                }
                ctx.emit_side_effect_step(use_side_effects, resolve_side_effects);
            }
            FlowBackend::Local => {
                let did_copy = if let Some((artifact_dir, done)) = artifact_dir {
                    let se = ctx.emit_rust_step("copy JUnit test results to artifact dir", |ctx| {
                        done.claim(ctx);
                        let artifact_dir = artifact_dir.claim(ctx);
                        let xmls = xmls
                            .iter()
                            .map(
                                |TestResult {
                                     junit_xml,
                                     label,
                                     attachments,
                                     done: _,
                                 }| {
                                    (
                                        junit_xml.clone().claim(ctx),
                                        label.clone(),
                                        attachments.as_ref().map(|x| {
                                            x.iter()
                                                .map(|(y, z)| (y.clone(), z.clone().claim(ctx)))
                                                .collect::<BTreeMap<_, _>>()
                                        }),
                                    )
                                },
                            )
                            .collect::<Vec<_>>();
                        |rt| {
                            let artifact_dir = rt.read(artifact_dir);

                            for (idx, (path, label, attachments)) in xmls.into_iter().enumerate() {
                                let Some(path) = rt.read(path) else {
                                    continue;
                                };
                                fs_err::copy(
                                    path,
                                    artifact_dir.join(format!(
                                        "{}_{idx}_results.xml",
                                        label.replace(' ', "_")
                                    )),
                                )?;
                                if let Some(attachments) = attachments {
                                    for (attachment_label, attachment_path) in attachments {
                                        let attachment_path = rt.read(attachment_path);
                                        copy_dir_all(
                                            attachment_path,
                                            artifact_dir.join(format!(
                                                "{}_{idx}_{}",
                                                label.replace(' ', "_"),
                                                attachment_label.replace(' ', "_")
                                            )),
                                        )?;
                                    }
                                }
                            }

                            Ok(())
                        }
                    });
                    Some(se)
                } else {
                    None
                };

                let all_done = xmls.into_iter().map(
                    |TestResult {
                         junit_xml: _,
                         label: _,
                         attachments: _,
                         done,
                     }| done,
                );
                ctx.emit_side_effect_step(did_copy, all_done);
            }
        }

        Ok(())
    }
}
