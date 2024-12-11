// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Functions for creating Hyper-V VMs.

use anyhow::Context as _;
use std::fmt::Display;
use std::path::PathBuf;
use std::process::Command;

pub enum HyperVGeneration {
    One,
    Two,
}

impl Display for HyperVGeneration {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HyperVGeneration::One => {
                write!(f, "1")
            }
            HyperVGeneration::Two => {
                write!(f, "2")
            }
        }
    }
}

pub struct HyperVConfig {
    /// Specifies the name of the new virtual machine.
    pub name: String,
    // /// Specifies the device to use as the boot device for the new virtual machine.
    // pub boot_device: HyperVBootDevice,
    /// Specifies the generation for the virtual machine.
    pub generation: Option<HyperVGeneration>,
    /// Specifies the amount of memory, in bytes, to assign to the virtual machine.
    pub memory_startup_bytes: Option<u64>,
    /// Specifies the directory to store the files for the new virtual machine.
    pub path: Option<PathBuf>,
    /// Specifies the path to a virtual hard disk file.
    pub vhd_path: Option<PathBuf>,
}

/// Runs New-VM with the given arguments.
pub fn run_new_vm(config: &HyperVConfig) -> anyhow::Result<()> {
    let mut cmd = Command::new("powershell.exe");
    cmd.arg("New-VM").arg("-Force");
    cmd.arg("-Name").arg(&config.name);
    if let Some(generation) = &config.generation {
        cmd.arg("-Generation").arg(generation.to_string());
    }
    if let Some(memory_startup_bytes) = config.memory_startup_bytes {
        cmd.arg("-MemoryStartupBytes")
            .arg(memory_startup_bytes.to_string());
    }
    if let Some(path) = &config.path {
        cmd.arg("-Path").arg(path);
    }
    if let Some(vhd_path) = &config.vhd_path {
        cmd.arg("-VHDPath").arg(vhd_path);
    }

    let status = cmd.status().context("failed run New-VM")?;
    if !status.success() {
        anyhow::bail!("New-VM failed with exit code: {}", status);
    }
    Ok(())
}

/// Runs New-VM with the given arguments.
pub fn run_remove_vm(vm: &str) -> anyhow::Result<()> {
    let mut cmd = Command::new("powershell.exe");
    cmd.arg("Remove-VM").arg("-Name").arg(vm).arg("-Force");
    let status = cmd.status().context("failed run Remove-VM")?;
    if !status.success() {
        anyhow::bail!("Remove-VM failed with exit code: {}", status);
    }
    Ok(())
}
