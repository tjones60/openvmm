// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Functions for interacting with Hyper-V VMs.

use anyhow::Context as _;
use anyhow::Ok;

pub fn hvc_start(vm: &str) -> anyhow::Result<()> {
    run_hvc(|cmd| cmd.arg("start").arg(vm))
}

// pub fn hvc_kill(vm: &str) -> anyhow::Result<()> {
//     run_hvc(|cmd| cmd.arg("kill").arg(vm))
// }

pub fn hvc_wait_for_power_off(vm: &str) -> anyhow::Result<()> {
    while hvc_output(|cmd| cmd.arg("state").arg(vm))? != "off" {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    Ok(())
}

/// Runs hvc with the given arguments.
fn run_hvc(
    f: impl FnOnce(&mut std::process::Command) -> &mut std::process::Command,
) -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new("hvc.exe");
    f(&mut cmd);
    let status = cmd.status().context("failed to launch hvc")?;
    if !status.success() {
        anyhow::bail!("hvc failed with exit code: {}", status);
    }
    Ok(())
}

/// Runs hvc with the given arguments and returns the output.
fn hvc_output(
    f: impl FnOnce(&mut std::process::Command) -> &mut std::process::Command,
) -> anyhow::Result<String> {
    let mut cmd = std::process::Command::new("hvc.exe");
    f(&mut cmd);
    let output = cmd.output().expect("failed to launch hvc");
    if !output.status.success() {
        anyhow::bail!("hvc failed with exit code: {}", output.status);
    }
    String::from_utf8(output.stdout).context("output is not utf-8")
}
