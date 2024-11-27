// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Functions for creating Hyper-V VMs.

use anyhow::Context as _;

/// Runs New-VM with the given arguments.
pub fn run_new_vm(vm: &str) -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new("New-VM");
    cmd.arg("-Name").arg(vm);
    let status = cmd.status().context("failed run New-VM")?;
    if !status.success() {
        anyhow::bail!("New-VM failed with exit code: {}", status);
    }
    Ok(())
}

/// Runs New-VM with the given arguments.
pub fn run_remove_vm(vm: &str) -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new("Remove-VM");
    cmd.arg("-Name").arg(vm);
    let status = cmd.status().context("failed run Remove-VM")?;
    if !status.success() {
        anyhow::bail!("Remove-VM failed with exit code: {}", status);
    }
    Ok(())
}
