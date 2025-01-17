// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! This is the petri utility

use anyhow::Context;
use pal_async::socket::PolledSocket;
use pal_async::timer::PolledTimer;
use pal_async::DefaultDriver;
use pipette_client::PipetteClient;
use std::path::Path;
use std::time::Duration;
use vmsocket::VmAddress;
use vmsocket::VmSocket;

#[cfg(not(target_os = "windows"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("unsupported")
}

#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    use std::path::PathBuf;

    ::pal_async::DefaultPool::run_with(|driver| async move {
        let agent = wait_for_agent(
            &driver,
            "WindowsServer2019",
            &PathBuf::from("C:\\temp"),
            false,
        )
        .await?;
        agent.power_off().await?;
        Ok(())
    })
}

async fn wait_for_agent(
    driver: &DefaultDriver,
    name: &str,
    output_dir: &Path,
    set_high_vtl: bool,
) -> anyhow::Result<PipetteClient> {
    let vm_id = diag_client::hyperv::vm_id_from_name(name)?;

    #[cfg_attr(not(feature = "pipette_host_server"), allow(unused_mut))]
    let mut socket = VmSocket::new().context("failed to create AF_HYPERV socket")?;

    socket
        .set_high_vtl(set_high_vtl)
        .context("failed to set socket for VTL0")?;

    #[cfg(feature = "pipette_host_server")]
    let socket = {
        socket
            .set_connect_timeout(std::time::Duration::from_secs(300))
            .context("failed to set connect timeout")?;
        socket.bind(VmAddress::hyperv_vsock(
            vm_id,
            pipette_client::PIPETTE_VSOCK_PORT,
        ))?;
        let listener = socket.listen(1)?;
        let socket = listener
            .accept()
            .context("failed to accept pipette connection")?
            .0;
        PolledSocket::new(driver, socket)?
    };

    #[cfg(not(feature = "pipette_host_server"))]
    let socket = {
        socket
            .set_connect_timeout(Duration::from_secs(10))
            .context("failed to set connect timeout")?;
        let mut socket = PolledSocket::new(driver, socket)?.convert();
        loop {
            match socket
                .connect(&VmAddress::hyperv_vsock(vm_id, pipette_client::PIPETTE_VSOCK_PORT).into())
                .await
            {
                Ok(_) => break,
                Err(_) => {
                    PolledTimer::new(driver).sleep(Duration::from_secs(1)).await;
                    continue;
                }
            }
        }
        socket
    };

    PipetteClient::new(driver, socket, output_dir)
        .await
        .context("failed to connect to pipette")
}
