// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::Disk;
use crate::Drive;
use crate::Firmware;
use crate::IsolationType;
use crate::ModifyFn;
use crate::NoPetriVmFramebufferAcces;
use crate::NoPetriVmInspector;
use crate::OpenHclServicingFlags;
use crate::OpenvmmLogConfig;
use crate::PetriHaltReason;
use crate::PetriHaltReasonDetail;
use crate::PetriVmConfig;
use crate::PetriVmResources;
use crate::PetriVmRuntime;
use crate::PetriVmRuntimeConfig;
use crate::PetriVmmBackend;
use crate::ShutdownKind;
use crate::UefiConfig;
use crate::VmbusStorageController;
use crate::VmmQuirks;
use crate::kmsg_log_task;
use crate::openhcl_diag::OpenHclDiagHandler;
use crate::vm::PetriVmProperties;
use crate::vm::append_cmdline;
use anyhow::Context;
use async_trait::async_trait;
use futures_concurrency::future::Race;
use futures_concurrency::future::RaceOk;
use get_resources::ged::FirmwareEvent;
use guid::Guid;
use pal_async::DefaultDriver;
use pal_async::pipe::PolledPipe;
use pal_async::process::PolledChild;
use pal_async::socket::PolledSocket;
use pal_async::task::Spawn;
use pal_async::task::Task;
use petri_artifacts_common::tags::GuestQuirksInner;
use petri_artifacts_common::tags::MachineArch;
use petri_artifacts_core::ArtifactResolver;
use petri_artifacts_core::ResolvedArtifact;
use pipette_client::PipetteClient;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempPath;
use vtl2_settings_proto::Vtl2Settings;

/// The QEMU Petri backend
#[derive(Debug)]
pub struct QemuPetriBackend {
    qemu_path: ResolvedArtifact,
}

/// Resources needed at runtime for a QEMU Petri VM
pub struct QemuPetriRuntime {
    driver: DefaultDriver,
    qemu_process: PolledChild<std::process::Child>,
    host_pipette_port: u16,
    log_stream_tasks: Vec<Task<anyhow::Result<()>>>,
    output_dir: PathBuf,
}

#[async_trait]
impl PetriVmmBackend for QemuPetriBackend {
    type VmmConfig = ();
    type VmRuntime = QemuPetriRuntime;

    fn check_compat(_firmware: &Firmware, _arch: MachineArch) -> bool {
        true
    }

    fn quirks(_firmware: &Firmware) -> (GuestQuirksInner, VmmQuirks) {
        (GuestQuirksInner::default(), VmmQuirks::default())
    }

    fn default_servicing_flags() -> OpenHclServicingFlags {
        OpenHclServicingFlags {
            enable_nvme_keepalive: false,
            enable_mana_keepalive: false,
            override_version_checks: true,
            stop_timeout_hint_secs: None,
        }
    }

    fn create_guest_dump_disk() -> anyhow::Result<
        Option<(
            Arc<TempPath>,
            Box<dyn FnOnce() -> anyhow::Result<Box<dyn fatfs::ReadWriteSeek>>>,
        )>,
    > {
        Ok(None)
    }

    fn new(resolver: &ArtifactResolver<'_>) -> Self {
        QemuPetriBackend {
            qemu_path: resolver
                .require(petri_artifacts_vmm_test::artifacts::OPENVMM_NATIVE) // TODO
                .erase(),
        }
    }

    async fn run(
        self,
        config: PetriVmConfig,
        _modify_vmm_config: Option<ModifyFn<Self::VmmConfig>>,
        resources: &PetriVmResources,
        properties: PetriVmProperties,
    ) -> anyhow::Result<(Self::VmRuntime, PetriVmRuntimeConfig)> {
        let PetriVmResources { driver, log_source } = resources;

        let host_pipette_port = pick_free_port().context("failed to find a free port")?;
        let machine = match config.arch {
            MachineArch::Aarch64 => "virt,virtualization=on,iommu=smmuv3,gic-version=3",
            _ => todo!(),
        };
        let mut cmd =
            build_qemu_command(self.qemu_path.get(), machine, &config, host_pipette_port)?;
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut qemu_process = cmd.spawn().context("failed to launch QEMU")?;
        let qemu_stdout = qemu_process.stdout.take().expect("stdout should be piped");
        let qemu_stderr = qemu_process.stderr.take().expect("stderr should be piped");

        let qemu_process = PolledChild::<std::process::Child>::new(driver, qemu_process)
            .context("failed to create PolledChild")?;

        let mut log_stream_tasks = Vec::new();

        let qemu_stdout_pipe = PolledPipe::new(driver, child_pipe_to_file(qemu_stdout))
            .context("failed to create polled pipe for qemu stdout")?;
        let qemu_stdout_log_file = log_source.log_file("qemu_stdout")?;
        let qemu_stdout_task = driver.spawn(
            "qemu_stdout",
            crate::log_task(qemu_stdout_log_file, qemu_stdout_pipe, "qemu_stdout"),
        );
        log_stream_tasks.push(qemu_stdout_task);

        let qemu_stderr_pipe = PolledPipe::new(driver, child_pipe_to_file(qemu_stderr))
            .context("failed to create polled pipe for qemu stdout")?;
        let qemu_stderr_log_file = log_source.log_file("qemu_stderr")?;
        let qemu_stderr_task = driver.spawn(
            "qemu_stderr",
            crate::log_task(qemu_stderr_log_file, qemu_stderr_pipe, "qemu_stderr"),
        );
        log_stream_tasks.push(qemu_stderr_task);

        Ok((
            QemuPetriRuntime {
                driver: driver.clone(),
                qemu_process,
                host_pipette_port,
                log_stream_tasks,
                output_dir: log_source.output_dir().to_owned(),
            },
            config
                .firmware
                .into_runtime_config(config.vmbus_storage_controllers),
        ))
    }
}

#[async_trait]
impl PetriVmRuntime for QemuPetriRuntime {
    type VmInspector = NoPetriVmInspector;
    type VmFramebufferAccess = NoPetriVmFramebufferAcces;

    async fn teardown(mut self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn wait_for_halt(&mut self, _allow_reset: bool) -> anyhow::Result<PetriHaltReasonDetail> {
        todo!()
    }

    async fn wait_for_agent(&mut self, _set_high_vtl: bool) -> anyhow::Result<PipetteClient> {
        todo!()
    }

    fn openhcl_diag(&self) -> Option<OpenHclDiagHandler> {
        None
    }

    async fn wait_for_boot_event(
        &mut self,
        _timeout: Option<Duration>,
    ) -> anyhow::Result<Option<FirmwareEvent>> {
        todo!()
    }

    async fn wait_for_enlightened_shutdown_ready(&mut self) -> anyhow::Result<()> {
        todo!()
    }

    async fn send_enlightened_shutdown(&mut self, kind: ShutdownKind) -> anyhow::Result<()> {
        todo!()
    }

    async fn restart_openhcl(
        &mut self,
        new_openhcl: &ResolvedArtifact,
        flags: OpenHclServicingFlags,
    ) -> anyhow::Result<()> {
        todo!()
    }

    async fn save_openhcl(
        &mut self,
        _new_openhcl: &ResolvedArtifact,
        _flags: OpenHclServicingFlags,
    ) -> anyhow::Result<()> {
        todo!()
    }

    async fn restore_openhcl(&mut self) -> anyhow::Result<()> {
        todo!()
    }

    async fn update_command_line(&mut self, _command_line: &str) -> anyhow::Result<()> {
        todo!()
    }

    fn take_framebuffer_access(&mut self) -> Option<NoPetriVmFramebufferAcces> {
        todo!()
    }

    async fn reset(&mut self) -> anyhow::Result<()> {
        todo!()
    }

    async fn get_guest_state_file(&self) -> anyhow::Result<Option<PathBuf>> {
        todo!()
    }

    async fn set_vtl2_settings(&mut self, _settings: &Vtl2Settings) -> anyhow::Result<()> {
        todo!()
    }

    async fn set_vmbus_drive(
        &mut self,
        _drive: &Drive,
        _controller_id: &Guid,
        _controller_location: u32,
    ) -> anyhow::Result<()> {
        todo!()
    }
}

impl QemuPetriRuntime {
    async fn client_core(&self) -> anyhow::Result<PipetteClient> {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], self.host_pipette_port));
        let conn = pal_async::socket::PolledSocket::connect_tcp(&self.driver, addr)
            .await
            .context("failed to connect to pipette")?;
        PipetteClient::new(&self.driver, conn, &self.output_dir)
            .await
            .context("failed to create pipette client")
    }

    /// Poll `f` until it produces a value, failing if the QEMU process exits
    /// first and giving up once `timeout` has elapsed.
    async fn poll_until_exit_or_timeout<T>(
        &mut self,
        timeout: Option<Duration>,
        f: impl AsyncFn(&Self) -> anyhow::Result<Option<T>>,
    ) -> anyhow::Result<Option<T>> {
        (
            f(self),
            async {
                let status = self.qemu_process.wait().await?;
                Err(anyhow::anyhow!(
                    "QEMU exited unexpectedly (status: {status})"
                ))
            },
            async {
                pal_async::timer::PolledTimer::new(&self.driver)
                    .sleep(timeout.unwrap_or(Duration::MAX))
                    .await;
                Err(anyhow::anyhow!("Timed out waiting for operation"))
            },
        )
            .race()
            .await
    }
}

/// Convert a child process's stdout/stderr pipe into a [`std::fs::File`] so it
/// can be wrapped in a [`PolledPipe`]. The owned-handle type differs by
/// platform, but the conversion is otherwise identical.
#[cfg(unix)]
pub fn child_pipe_to_file(pipe: impl Into<std::os::unix::io::OwnedFd>) -> std::fs::File {
    std::fs::File::from(pipe.into())
}

#[cfg(windows)]
pub fn child_pipe_to_file(pipe: impl Into<std::os::windows::io::OwnedHandle>) -> std::fs::File {
    std::fs::File::from(pipe.into())
}

/// Find a free TCP port by binding to port 0 and reading the assigned port.
fn pick_free_port() -> anyhow::Result<u16> {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").context("failed to bind ephemeral port")?;
    let port = listener
        .local_addr()
        .context("failed to get local addr")?
        .port();
    Ok(port)
}

/// Build the QEMU command line for a TCG launch.
pub fn build_qemu_command(
    binary: &Path,
    machine: &str,
    config: &PetriVmConfig,
    host_pipette_port: u16,
) -> anyhow::Result<Command> {
    let mut cmd = Command::new(binary);

    cmd.arg("-machine").arg(&machine);
    cmd.arg("-cpu")
        .arg(config.proc_topology.vp_count.to_string());
    cmd.arg("-m").arg(config.memory.startup_bytes.to_string());
    cmd.arg("-smp")
        .arg(if config.proc_topology.enable_smt.is_some_and(|x| x) {
            "2"
        } else {
            "1"
        });
    cmd.arg("-nographic");

    match &config.firmware {
        Firmware::LinuxDirect { kernel, initrd } => {
            cmd.arg("-kernel").arg(kernel);
            cmd.arg("-initrd").arg(initrd);
        }
        _ => anyhow::bail!("qemu backend only supports linux direct"),
    };

    // cmd.arg("-append")
    //     .arg(format!("{} rdinit=/{INIT_SCRIPT_NAME}", config.cmdline));
    cmd.arg("-no-reboot");

    // 9p: share the host directory into the guest
    /* TODO
    cmd.arg("-fsdev").arg(format!(
        "local,id=fsdev0,path={},security_model=none",
        share_dir.display()
    ));
    cmd.arg("-device")
        .arg("virtio-9p-pci,fsdev=fsdev0,mount_tag=hostshare");
    */

    // User-mode networking with port forwarding for pipette TCP
    cmd.arg("-netdev").arg(format!(
        "user,id=net0,hostfwd=tcp::{host_pipette_port}-:{guest_port}",
        guest_port = pipette_client::PIPETTE_PORT,
    ));
    cmd.arg("-device")
        .arg("virtio-net-pci,netdev=net0,romfile=");

    // Console on serial (diagnostic only)
    cmd.arg("-serial").arg("mon:stdio");

    // Extra devices from the profile.
    // Each device gets its own PCIe root port at a known PCI device number
    // (`addr=`), so the VFIO setup code can find the bridge by its devfn
    // in sysfs and enumerate the child behind it.
    /* TODO
    for (i, device) in devices.iter().enumerate() {
        let rp_id = format!("hosting_rp{i}");
        let addr = EXTRA_DEVICE_ADDR_BASE + i;
        // Each root port needs a unique `slot` within its chassis. QEMU
        // defaults every pcie-root-port to `chassis=0,slot=0`, so a second
        // port collides with "Can't add chassis slot, error -16" (EBUSY).
        // Number the slots from 1.
        let slot = i + 1;
        cmd.arg("-device").arg(format!(
            "pcie-root-port,id={rp_id},addr={addr:#x},slot={slot}"
        ));

        match device {
            DeviceConfig::VirtioBlk(cfg) => {
                let node_name = format!("disk{i}");
                let size_bytes = parse_size(&cfg.size)
                    .with_context(|| format!("invalid size for device '{}'", cfg.name))?;
                cmd.arg("-blockdev")
                    .arg(format!("null-co,node-name={node_name},size={size_bytes}"));
                cmd.arg("-device")
                    .arg(format!("virtio-blk-pci,drive={node_name},bus={rp_id},iommu_platform=on,disable-legacy=on,romfile="));
            }
            DeviceConfig::Edu(cfg) => {
                // A conventional PCI endpoint with a register-programmed DMA
                // engine. Widen `dma_mask` when provided so the engine can
                // address high aarch64 guest physical addresses (its 28-bit
                // default would clamp them).
                let mut dev = format!("edu,bus={rp_id}");
                if let Some(mask) = &cfg.dma_mask {
                    dev.push_str(&format!(",dma_mask={mask}"));
                }
                cmd.arg("-device").arg(dev);
            }
            DeviceConfig::IvshmemPlain(cfg) => {
                // BAR2 is a prefetchable, RAM-backed memory window served by a
                // host memory backend — a valid P2P DMA target BAR.
                let mem_id = format!("ivshmem_mem{i}");
                let size_bytes = parse_size(&cfg.size)
                    .with_context(|| format!("invalid size for device '{}'", cfg.name))?;
                cmd.arg("-object")
                    .arg(format!("memory-backend-ram,id={mem_id},size={size_bytes}"));
                cmd.arg("-device")
                    .arg(format!("ivshmem-plain,memdev={mem_id},bus={rp_id}"));
            }
        }
    }
    */

    Ok(cmd)
}

/// Wait for pipette to signal readiness via the serial console relay
/// task. Races against QEMU exit and a timeout.
pub async fn wait_for_pipette_ready(
    driver: &impl pal_async::driver::Driver,
    timeout: Duration,
    qemu_child: &mut PolledChild<std::process::Child>,
    ready_rx: mesh::OneshotReceiver<()>,
) -> anyhow::Result<()> {
    enum Event {
        Ready,
        QemuExited(std::process::ExitStatus),
        Timeout,
    }

    let event = (
        async {
            match ready_rx.await {
                Ok(()) => Event::Ready,
                // Sender dropped without sending — relay task exited
                // without seeing the marker (QEMU likely crashed).
                Err(_) => Event::QemuExited(std::process::ExitStatus::default()),
            }
        },
        async {
            match qemu_child.wait().await {
                Ok(status) => Event::QemuExited(status),
                Err(_) => Event::QemuExited(std::process::ExitStatus::default()),
            }
        },
        async {
            pal_async::timer::PolledTimer::new(driver)
                .sleep(timeout)
                .await;
            Event::Timeout
        },
    )
        .race()
        .await;

    match event {
        Event::Ready => Ok(()),
        Event::QemuExited(status) => {
            anyhow::bail!("QEMU exited before pipette was ready (status: {status})");
        }
        Event::Timeout => {
            anyhow::bail!("timed out waiting for pipette ready signal");
        }
    }
}
