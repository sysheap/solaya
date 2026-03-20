use std::{
    net::TcpListener,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};

use anyhow::anyhow;
use tokio::{
    io::AsyncWriteExt,
    process::{Child, ChildStdin, ChildStdout, Command},
};

use crate::{PROMPT, qmp::QmpClient, read_asserter::ReadAsserter};

fn find_available_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

pub fn project_root() -> anyhow::Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        if dir.join("qemu_wrapper.sh").exists() {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err(anyhow!(
                "Could not find project root (no qemu_wrapper.sh in any parent directory)"
            ));
        }
    }
}

pub struct QemuOptions {
    add_network_card: bool,
    use_smp: bool,
    enable_gdb: bool,
    block_device: Option<PathBuf>,
    framebuffer: bool,
    headless: bool,
    qmp_socket: Option<PathBuf>,
}

impl Default for QemuOptions {
    fn default() -> Self {
        let enable_gdb = std::env::var("SOLAYA_ENABLE_GDB").is_ok();
        Self {
            add_network_card: false,
            use_smp: true,
            enable_gdb,
            block_device: None,
            framebuffer: false,
            headless: true,
            qmp_socket: None,
        }
    }
}

impl QemuOptions {
    pub fn add_network_card(mut self, value: bool) -> Self {
        self.add_network_card = value;
        self
    }
    pub fn use_smp(mut self, value: bool) -> Self {
        self.use_smp = value;
        self
    }
    pub fn enable_gdb(mut self, value: bool) -> Self {
        self.enable_gdb = value;
        self
    }
    pub fn block_device(mut self, path: PathBuf) -> Self {
        self.block_device = Some(path);
        self
    }
    pub fn framebuffer(mut self, value: bool) -> Self {
        self.framebuffer = value;
        self
    }
    pub fn headless(mut self, value: bool) -> Self {
        self.headless = value;
        self
    }
    pub fn qmp_socket(mut self, path: PathBuf) -> Self {
        self.qmp_socket = Some(path);
        self
    }

    fn apply(self, command: &mut Command) -> Option<u16> {
        let mut network_port = None;
        if self.add_network_card {
            let port = find_available_port().expect("Failed to allocate network port");
            command.args(["--net", &port.to_string()]);
            network_port = Some(port);
        }
        if let Some(block_path) = &self.block_device {
            command.args(["--block", &block_path.to_string_lossy()]);
        }
        if self.framebuffer {
            command.arg("--fb");
        }
        if self.headless {
            command.arg("--headless");
        }
        if let Some(qmp_path) = &self.qmp_socket {
            command.args(["--qmp", &qmp_path.to_string_lossy()]);
        }
        if self.use_smp {
            command.arg("--smp");
        }
        if self.enable_gdb {
            command.arg("--gdb");
        }
        network_port
    }
}

pub struct QemuInstance {
    instance: Child,
    stdin: ChildStdin,
    stdout: ReadAsserter<ChildStdout>,
    network_port: Option<u16>,
    gdb_port: Option<u16>,
    qmp_socket: Option<PathBuf>,
}

impl QemuInstance {
    pub async fn start() -> anyhow::Result<Self> {
        Self::start_with(QemuOptions::default()).await
    }

    pub async fn start_with(mut options: QemuOptions) -> anyhow::Result<Self> {
        let root = project_root()?;
        let wrapper = root.join("qemu_wrapper.sh");
        let mut command = Command::new(&wrapper);

        command
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        if options.framebuffer && options.qmp_socket.is_none() {
            let pid = std::process::id();
            options.qmp_socket = Some(std::env::temp_dir().join(format!("solaya-qmp-{pid}.sock")));
        }

        let gdb_enabled = options.enable_gdb;
        let qmp_socket = options.qmp_socket.clone();
        let network_port = options.apply(&mut command);

        command.arg("target/riscv64gc-unknown-none-elf/release/boot");

        let mut instance = command.spawn()?;

        let stdin = instance
            .stdin
            .take()
            .ok_or(anyhow!("Could not get stdin"))?;

        let stdout = instance
            .stdout
            .take()
            .ok_or(anyhow!("Could not get stdout"))?;

        let mut stdout = ReadAsserter::new(stdout);
        if gdb_enabled {
            stdout = stdout.with_timeout(Duration::from_secs(3600));
        }

        stdout.assert_read_until("Hello World from Solaya!").await?;
        stdout.assert_read_until("kernel_init done!").await?;
        stdout.assert_read_until("init process started").await?;
        if network_port.is_some() {
            stdout.assert_read_until("dhcpd: configured ip").await?;
        }
        stdout.assert_read_until("starting shell").await?;
        stdout.assert_read_until(PROMPT).await?;

        let gdb_port = if gdb_enabled {
            std::fs::read_to_string(root.join(".gdb-port"))
                .ok()
                .and_then(|s| s.trim().parse().ok())
        } else {
            None
        };

        Ok(Self {
            instance,
            stdin,
            stdout,
            network_port,
            gdb_port,
            qmp_socket,
        })
    }

    pub fn stdout(&mut self) -> &mut ReadAsserter<ChildStdout> {
        &mut self.stdout
    }

    pub fn stdin(&mut self) -> &mut ChildStdin {
        &mut self.stdin
    }

    pub fn network_port(&self) -> Option<u16> {
        self.network_port
    }

    pub fn gdb_port(&self) -> Option<u16> {
        self.gdb_port
    }

    pub fn qmp_socket(&self) -> Option<&Path> {
        self.qmp_socket.as_deref()
    }

    pub async fn screendump(&self) -> anyhow::Result<crate::ppm::PpmImage> {
        let socket = self
            .qmp_socket
            .as_ref()
            .ok_or_else(|| anyhow!("QMP not enabled (no framebuffer?)"))?;
        let tmp =
            std::env::temp_dir().join(format!("solaya-screendump-{}.ppm", std::process::id()));
        let mut qmp = QmpClient::connect(socket).await?;
        qmp.screendump(&tmp).await?;
        let image = crate::ppm::PpmImage::from_file(&tmp)?;
        let _ = std::fs::remove_file(&tmp);
        Ok(image)
    }

    pub async fn ctrl_c_and_assert_prompt(&mut self) -> anyhow::Result<String> {
        self.stdin().write_all(&[0x03]).await?;
        self.stdin().flush().await?;
        self.stdout().assert_read_until(PROMPT).await?;
        Ok(String::new())
    }

    pub async fn wait_for_qemu_to_exit(mut self) -> anyhow::Result<ExitStatus> {
        // Ensure stdin is closed so the child isn't stuck waiting on
        // input while the parent is waiting for it to exit.
        drop(self.stdin);
        drop(self.stdout);

        Ok(self.instance.wait().await?)
    }

    pub async fn run_prog(&mut self, prog_name: &str) -> anyhow::Result<String> {
        self.run_prog_waiting_for(prog_name, PROMPT).await
    }

    pub async fn run_prog_waiting_for(
        &mut self,
        prog_name: &str,
        wait_for: &str,
    ) -> anyhow::Result<String> {
        let command = format!("{}\n", prog_name);

        self.stdin.write_all(command.as_bytes()).await?;
        self.stdin.flush().await?;

        let result = self.stdout.assert_read_until(wait_for).await?;
        let trimmed_result = &result[command.len()..result.len() - wait_for.len()];

        Ok(String::from_utf8_lossy(trimmed_result).into_owned())
    }

    pub async fn write_and_wait_for(&mut self, text: &str, wait: &str) -> anyhow::Result<()> {
        self.stdin().write_all(text.as_bytes()).await?;
        self.stdin().flush().await?;
        self.stdout().assert_read_until(wait).await?;
        Ok(())
    }
}
