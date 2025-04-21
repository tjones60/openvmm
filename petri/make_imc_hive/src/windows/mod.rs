// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod offreg;

use self::offreg::Hive;
use anyhow::Context;

pub(crate) fn main() -> anyhow::Result<()> {
    let path = std::env::args_os().nth(1).context("missing path")?;
    let hive = Hive::create()?;

    let key_system = hive.create_key("SYSTEM")?;
    {
        let key_current_control_set = key_system.create_key("CurrentControlSet")?;
        {
            let key_control = key_current_control_set.create_key("Control")?;
            {
                let key_computer_name = key_control.create_key("ComputerName")?;
                {
                    let key_computer_name_inner = key_computer_name.create_key("ComputerName")?;

                    key_computer_name_inner.set_sz("ComputerName", "ImcVM")?;
                }
            }
        }
        {
            let key_services = key_current_control_set.create_key("Services")?;
            {
                let key_tcpip = key_services.create_key("Tcpip")?;
                {
                    let key_parameters = key_tcpip.create_key("Parameters")?;

                    key_parameters.set_sz("Hostname", "ImcVM")?;
                    key_parameters.set_sz("NV Hostname", "ImcVM")?;
                }
            }
            {
                let key_pipette = key_services.create_key("pipette")?;

                key_pipette.set_dword("Type", 0x10)?; // win32 service
                key_pipette.set_dword("Start", 2)?; // auto start
                key_pipette.set_dword("ErrorControl", 1)?; // normal
                key_pipette.set_sz("ImagePath", "D:\\pipette.exe --service")?;
                key_pipette.set_sz("DisplayName", "Petri pipette agent")?;
                key_pipette.set_sz("ObjectName", "LocalSystem")?;
                key_pipette.set_multi_sz("DependOnService", ["RpcSs"])?;
            }
        }
    }

    // Windows defaults to 1, so we need to set it to 2 to cause Windows to
    // apply the IMC changes on first boot.
    hive.set_dword("Sequence", 0xf)?;

    let _ = std::fs::remove_file(&path);
    hive.save(path.as_ref())?;
    Ok(())
}
