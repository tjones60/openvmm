// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! See [`OpenvmmKnownPathsTestArtifactResolver`].

#![forbid(unsafe_code)]

use anyhow::Context;
use petri_artifacts_core::ArtifactSource;
use petri_artifacts_core::ErasedArtifactHandle;
use petri_artifacts_vmm_test::vmm_test_image_from_id;
use std::env::consts::EXE_EXTENSION;
use std::path::Path;
use std::path::PathBuf;

/// An implementation of [`petri_artifacts_core::ResolveTestArtifact`]
/// that resolves artifacts to various "known paths" within the context of
/// the OpenVMM repository.
pub struct OpenvmmKnownPathsTestArtifactResolver<'a>(&'a str);

impl<'a> OpenvmmKnownPathsTestArtifactResolver<'a> {
    /// Creates a new resolver for a test with the given name.
    pub fn new(test_name: &'a str) -> Self {
        Self(test_name)
    }
}

impl petri_artifacts_core::ResolveTestArtifact for OpenvmmKnownPathsTestArtifactResolver<'_> {
    fn resolve(&self, handle: ErasedArtifactHandle) -> anyhow::Result<PathBuf> {
        use petri_artifacts_common::artifacts::*;
        use petri_artifacts_core::ArtifactId;
        use petri_artifacts_vmm_test::artifacts::*;

        match handle.global_unique_id() {
            TEST_LOG_DIRECTORY::GLOBAL_UNIQUE_ID => test_log_directory_path(self.0),

            test_vhd::GEN2_WINDOWS_DATA_CENTER_CORE2025_X64_PREPPED::GLOBAL_UNIQUE_ID
            | test_vhd::GEN2_WINDOWS_DATA_CENTER_CORE2022_X64_NO_VMBUS_PREPPED::GLOBAL_UNIQUE_ID => {
                get_vmm_test_image_path(handle.filename(), "prepped vhd")
            }

            id if let Some(artifact) = vmm_test_image_from_id(id) => {
                get_vmm_test_image_path(artifact.filename(), artifact.name())
            }

            _ => artifact_path(handle),
        }
    }

    fn resolve_source(&self, handle: ErasedArtifactHandle) -> anyhow::Result<ArtifactSource> {
        // Try local resolution first.
        let local_err = match self.resolve(handle) {
            Ok(path) => return Ok(ArtifactSource::Local(path)),
            Err(e) => e,
        };

        // Fall back to remote URL for artifacts hosted on Azure Blob Storage,
        // but only for formats the blob disk backend supports (fixed VHD1 and flat).
        if let Some(url) = vmm_test_image_from_id(handle.global_unique_id()).and_then(|i| i.url()) {
            return Ok(ArtifactSource::Remote { url });
        }

        // No local path and no remote URL available — return the original error.
        Err(local_err)
    }
}

const VMM_TESTS_CONTENT_DIR_ENV_VAR: &str = "VMM_TESTS_CONTENT_DIR";
const TEST_OUTPUT_PATH_ENV_VAR: &str = "TEST_OUTPUT_PATH";
const VMM_TEST_IMAGES_ENV_VAR: &str = "VMM_TEST_IMAGES";

fn artifact_path(handle: ErasedArtifactHandle) -> anyhow::Result<PathBuf> {
    let test_content_dir_path = test_content_dir_artifact_path(handle);

    if test_content_dir_path.is_ok() {
        return test_content_dir_path;
    }

    let exe_artifact_path = get_executable_path_artifact(handle, None);

    if exe_artifact_path.is_ok() {
        return exe_artifact_path;
    }

    let relative_path = get_executable_path_relative(handle.filename());

    if relative_path.is_ok() {
        return relative_path;
    }

    Err(anyhow::anyhow!(
        "unable to locate {}:\n\t{}\n\t{}\n\t{}",
        handle.global_unique_id(),
        test_content_dir_path.unwrap_err(),
        exe_artifact_path.unwrap_err(),
        relative_path.unwrap_err()
    ))
}

fn test_content_dir_artifact_path(handle: ErasedArtifactHandle) -> anyhow::Result<PathBuf> {
    let test_content_dir =
        std::env::var(VMM_TESTS_CONTENT_DIR_ENV_VAR).context("test content dir env var not set")?;
    let path = PathBuf::from(test_content_dir).join(handle.relative_path());
    if !path.exists() {
        anyhow::bail!(
            "missing {} at {}",
            handle.global_unique_id(),
            path.display()
        )
    }
    Ok(path)
}

/// Path to the per-test test output directory.
fn test_log_directory_path(test_name: &str) -> anyhow::Result<PathBuf> {
    let root =
        std::env::var_os(TEST_OUTPUT_PATH_ENV_VAR).context("test output path env var not set")?;
    // Use a per-test subdirectory, replacing `::` with `__` to avoid issues
    // with filesystems that don't support `::` in filenames.
    let path = PathBuf::from(root).join(test_name.replace("::", "__"));
    fs_err::create_dir_all(&path)?;
    Ok(path)
}

/// Gets a path to the root of the repo.
pub fn get_repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Returns the Cargo build profile directory name for cross-compiled
/// artifacts (e.g., pipette).
///
/// Infers the profile from the currently running binary's path (looking
/// for a `release` component in the executable path). Defaults to `"debug"`.
// DEVNOTE: `pub` in order to re-use in perf_tests and other crates.
pub fn cargo_build_profile() -> &'static str {
    static PROFILE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PROFILE.get_or_init(|| {
        if let Ok(exe) = std::env::current_exe() {
            if exe.components().any(|c| c.as_os_str() == "release") {
                return "release".to_string();
            }
        }
        "debug".to_string()
    })
}

/// Attempts to find the path to a rust executable built by Cargo using the path
/// to the current executable to find the base path.
pub fn get_executable_path_relative(name: &str) -> anyhow::Result<PathBuf> {
    let mut current_exe = std::env::current_exe().context("unable to get current exe")?;
    // Sometimes we end up inside deps instead of the output dir, but if we
    // are we can just go up a level.
    if current_exe.parent().and_then(|x| x.file_name()).unwrap() == "deps" {
        current_exe.pop();
    }

    let exe_path = current_exe
        .parent()
        .context("current exe has no parent")?
        .join(Path::new(name).with_extension(EXE_EXTENSION));
    if !exe_path.exists() {
        anyhow::bail!("{} not found", exe_path.display());
    }
    Ok(exe_path)
}

/// Attempts to find the path to a rust executable built by Cargo using a path
/// constructed from the repo root and the default target directory. The build
/// profile is inferred from the current exe path if not specified.
pub fn get_executable_path_artifact(
    handle: ErasedArtifactHandle,
    build_profile: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let exe_path = get_repo_root()
        .join("target")
        .join(
            handle
                .target_triple()
                .context("no associated triple for artifact")?
                .to_string(),
        )
        .join(build_profile.unwrap_or_else(|| cargo_build_profile()))
        .join(handle.filename());
    if !exe_path.exists() {
        anyhow::bail!("{} not found", exe_path.display());
    }
    Ok(exe_path)
}

fn get_vmm_test_image_path(filename: &str, name: &str) -> Result<PathBuf, anyhow::Error> {
    let test_images_dir =
        std::env::var(VMM_TEST_IMAGES_ENV_VAR).context("test images dir env var not set")?;
    let path = PathBuf::from(test_images_dir).join(filename);
    if !path.exists() {
        anyhow::bail!("missing {} at {}", name, path.display())
    }
    Ok(path)
}
