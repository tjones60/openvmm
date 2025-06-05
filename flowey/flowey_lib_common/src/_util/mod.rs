// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

pub use flowey::util::copy_dir_all;

use flowey::node::prelude::FlowPlatformKind;
use flowey::node::prelude::RustRuntimeServices;
use std::path::Path;
use std::time::SystemTime;

pub mod cargo_output;
pub mod extract;
pub mod wslpath;

// include a "dummy" _rt argument to enforce that this helper should only be
// used in runtime contexts, and not during flow compile-time.
pub fn running_in_wsl(_rt: &mut RustRuntimeServices<'_>) -> bool {
    let Ok(output) = std::process::Command::new("wslpath")
        .args(["-aw", "/"])
        .output()
    else {
        return false;
    };
    String::from_utf8_lossy(&output.stdout).starts_with(r"\\wsl.localhost")
}

/// Returns the name of the bsdtar binary to use. On Windows, this is just the
/// inbox tar.exe. Elsewhere, use bsdtar. This will require installing the
/// libarchive-tools package on Debian-based Linux.
pub fn bsdtar_name(rt: &mut RustRuntimeServices<'_>) -> &'static str {
    match rt.platform().kind() {
        FlowPlatformKind::Windows => "tar.exe",
        FlowPlatformKind::Unix => "bsdtar",
    }
}

/// determine whether the newest file in the inputs is newer than the oldest
/// file in the outputs. useful to avoid repeating operations like copying.
pub fn needs_update(
    _rt: &mut RustRuntimeServices<'_>,
    inputs: impl IntoIterator<Item = impl AsRef<Path>>,
    outputs: impl IntoIterator<Item = impl AsRef<Path>>,
) -> std::io::Result<bool> {
    let mut oldest_output = SystemTime::now();
    for output in outputs {
        if !output.as_ref().try_exists()? {
            return Ok(true);
        }
        let modified = fs_err::metadata(output)?.modified()?;
        if modified < oldest_output {
            oldest_output = modified;
        }
    }
    let mut newest_input = SystemTime::UNIX_EPOCH;
    for input in inputs {
        let modified = fs_err::metadata(input)?.modified()?;
        if modified > newest_input {
            newest_input = modified;
        }
    }
    Ok(newest_input > oldest_output)
}
