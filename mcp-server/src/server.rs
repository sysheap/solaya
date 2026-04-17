use std::{sync::Arc, time::Duration};

use qemu_infra::qemu::{QemuInstance, QemuOptions, project_root};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::ToolCallContext, wrapper::Parameters},
    model::*,
    schemars,
    service::RequestContext,
    tool, tool_router,
};
use tokio::{io::AsyncWriteExt, process::Command, sync::Mutex};

#[derive(Clone)]
pub struct QemuMcpServer {
    qemu: Arc<Mutex<Option<QemuInstance>>>,
    tool_router: ToolRouter<Self>,
}

impl QemuMcpServer {
    pub fn new() -> Self {
        Self {
            qemu: Arc::new(Mutex::new(None)),
            tool_router: Self::tool_router(),
        }
    }
}

fn mcp_err(msg: impl Into<String>) -> McpError {
    McpError::internal_error(msg.into(), None)
}

fn text_result(text: impl Into<String>) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(text.into())]))
}

fn require_running(guard: &mut Option<QemuInstance>) -> Result<&mut QemuInstance, McpError> {
    guard
        .as_mut()
        .ok_or_else(|| mcp_err("QEMU is not running. Call boot_qemu first."))
}

fn format_command_output(label: &str, success_word: &str, output: &std::process::Output) -> String {
    format!(
        "{} {}\n\n--- stdout ---\n{}\n--- stderr ---\n{}",
        label,
        if output.status.success() {
            success_word
        } else {
            "FAILED"
        },
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

async fn lock_qemu(
    qemu: &Mutex<Option<QemuInstance>>,
) -> Result<tokio::sync::MutexGuard<'_, Option<QemuInstance>>, McpError> {
    tokio::time::timeout(Duration::from_secs(10), qemu.lock())
        .await
        .map_err(|_| mcp_err("Another QEMU operation is in progress, try again later."))
}

async fn run_command(mut cmd: Command) -> Result<std::process::Output, std::io::Error> {
    cmd.kill_on_drop(true).spawn()?.wait_with_output().await
}

// --- Parameter types ---

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BootParams {
    #[schemars(description = "Enable network card")]
    pub network: Option<bool>,
    #[schemars(description = "Enable SMP (multi-core). Defaults to true")]
    pub smp: Option<bool>,
    #[schemars(description = "Force restart if already running")]
    pub force: Option<bool>,
    #[schemars(description = "Enable GDB debugging. Defaults to true")]
    pub gdb: Option<bool>,
    #[schemars(description = "Path to a raw disk image to attach as virtio-blk device")]
    pub block: Option<String>,
    #[schemars(description = "Enable bochs-display framebuffer device")]
    pub framebuffer: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SendCommandParams {
    #[schemars(description = "Shell command to send")]
    pub command: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SendInputParams {
    #[schemars(description = "Raw text to send to QEMU stdin")]
    pub input: String,
    #[schemars(description = "String to wait for in output before returning")]
    pub wait_for: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BuildKernelParams {
    #[schemars(description = "Also run clippy after building")]
    pub clippy: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ScreenshotParams {
    #[schemars(
        description = "File path to save the PNG screenshot (default: /tmp/solaya-screenshot.png)"
    )]
    pub output_path: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RunSystemTestsParams {
    #[schemars(description = "Specific test name to run (runs all if omitted)")]
    pub test_name: Option<String>,
}

// --- Tool implementations ---

#[tool_router]
impl QemuMcpServer {
    #[tool(
        description = "Boot QEMU with the Solaya kernel. Returns boot log. Errors if already running unless force=true."
    )]
    async fn boot_qemu(
        &self,
        Parameters(params): Parameters<BootParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut guard = lock_qemu(&self.qemu).await?;

        if guard.is_some() {
            if params.force.unwrap_or(false) {
                let old = guard.take().unwrap();
                if tokio::time::timeout(Duration::from_secs(5), old.wait_for_qemu_to_exit())
                    .await
                    .is_err()
                {
                    // Timeout: old instance dropped, kill_on_drop cleans up
                }
            } else {
                return Err(mcp_err(
                    "QEMU is already running. Use force=true to restart.",
                ));
            }
        }

        let mut options = QemuOptions::default()
            .add_network_card(params.network.unwrap_or(false))
            .use_smp(params.smp.unwrap_or(true))
            .enable_gdb(params.gdb.unwrap_or(true))
            .framebuffer(params.framebuffer.unwrap_or(false));
        if let Some(block_path) = params.block {
            options = options.block_device(std::path::PathBuf::from(block_path));
        }

        // Release the lock during the long boot so other tools can check status
        drop(guard);

        let instance = tokio::time::timeout(Duration::from_secs(15), async {
            QemuInstance::start_with(options).await
        })
        .await
        .map_err(|_| mcp_err("Timed out waiting for QEMU to boot (15s)"))?
        .map_err(|e| mcp_err(format!("Failed to boot QEMU: {e}")))?;

        let net_info = instance
            .network_port()
            .map(|p| format!(" Network port: {p}."))
            .unwrap_or_default();
        let gdb_info = instance
            .gdb_port()
            .map(|p| format!(" GDB port: {p}."))
            .unwrap_or_default();

        let mut guard = lock_qemu(&self.qemu).await?;
        *guard = Some(instance);

        text_result(format!("QEMU booted successfully.{net_info}{gdb_info}"))
    }

    #[tool(
        description = "Shutdown the running QEMU instance. Sends 'exit' to the shell and waits for QEMU to exit."
    )]
    async fn shutdown_qemu(&self) -> Result<CallToolResult, McpError> {
        let mut guard = lock_qemu(&self.qemu).await?;
        let mut instance = guard
            .take()
            .ok_or_else(|| mcp_err("QEMU is not running."))?;

        instance
            .stdin()
            .write_all(b"exit\n")
            .await
            .map_err(|e| mcp_err(format!("Failed to send exit: {e}")))?;
        instance
            .stdin()
            .flush()
            .await
            .map_err(|e| mcp_err(format!("Failed to flush exit: {e}")))?;

        let status = tokio::time::timeout(Duration::from_secs(5), instance.wait_for_qemu_to_exit())
            .await
            .map_err(|_| mcp_err("Timed out waiting for QEMU to exit (5s)"))?
            .map_err(|e| mcp_err(format!("Error waiting for QEMU exit: {e}")))?;

        text_result(format!("QEMU shut down. Exit status: {status}"))
    }

    #[tool(description = "Check if QEMU is running and return status info.")]
    async fn get_status(&self) -> Result<CallToolResult, McpError> {
        let guard = lock_qemu(&self.qemu).await?;
        match guard.as_ref() {
            Some(instance) => {
                let net_info = instance
                    .network_port()
                    .map(|p| format!(", network port: {p}"))
                    .unwrap_or_default();
                let gdb_info = instance
                    .gdb_port()
                    .map(|p| format!(", GDB port: {p}"))
                    .unwrap_or_default();
                text_result(format!("QEMU is running{net_info}{gdb_info}"))
            }
            None => text_result("QEMU is not running"),
        }
    }

    #[tool(
        description = "Send a shell command to Solaya. Waits for shell prompt and returns output."
    )]
    async fn send_command(
        &self,
        Parameters(params): Parameters<SendCommandParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut guard = lock_qemu(&self.qemu).await?;
        let instance = require_running(&mut guard)?;
        let output =
            tokio::time::timeout(Duration::from_secs(20), instance.run_prog(&params.command))
                .await
                .map_err(|_| mcp_err("Timed out waiting for output (20s)"))?
                .map_err(|e| mcp_err(format!("Failed to run command: {e}")))?;
        text_result(output)
    }

    #[tool(
        description = "Send raw input to QEMU stdin and wait for a custom marker string in output. For interactive programs."
    )]
    async fn send_input(
        &self,
        Parameters(params): Parameters<SendInputParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut guard = lock_qemu(&self.qemu).await?;
        let instance = require_running(&mut guard)?;

        tokio::time::timeout(Duration::from_secs(10), async {
            instance
                .write_and_wait_for(&params.input, &params.wait_for)
                .await
        })
        .await
        .map_err(|_| mcp_err("Timed out waiting for marker (10s)"))?
        .map_err(|e| mcp_err(format!("Failed to send input: {e}")))?;

        text_result("Input sent and marker found.")
    }

    #[tool(description = "Send Ctrl+C to the running program and wait for shell prompt.")]
    async fn send_ctrl_c(&self) -> Result<CallToolResult, McpError> {
        let mut guard = lock_qemu(&self.qemu).await?;
        let instance = require_running(&mut guard)?;

        tokio::time::timeout(Duration::from_secs(5), instance.ctrl_c_and_assert_prompt())
            .await
            .map_err(|_| mcp_err("Timed out waiting for prompt after Ctrl+C (5s)"))?
            .map_err(|e| mcp_err(format!("Failed to send Ctrl+C: {e}")))?;

        text_result("Ctrl+C sent, shell prompt received.")
    }

    #[tool(
        description = "Take a screenshot of the QEMU framebuffer. Requires framebuffer to be enabled. Saves a PNG image to the given path (or a temp path) and returns image dimensions and path."
    )]
    async fn screenshot(
        &self,
        Parameters(params): Parameters<ScreenshotParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut guard = lock_qemu(&self.qemu).await?;
        let instance = require_running(&mut guard)?;
        let image = instance
            .screendump()
            .await
            .map_err(|e| mcp_err(format!("Screenshot failed: {e}")))?;

        let output_path = params
            .output_path
            .unwrap_or_else(|| "/tmp/solaya-screenshot.png".to_string());
        let png_data = image
            .to_png_bytes()
            .map_err(|e| mcp_err(format!("PNG encoding failed: {e}")))?;
        std::fs::write(&output_path, &png_data)
            .map_err(|e| mcp_err(format!("Failed to write screenshot: {e}")))?;

        text_result(format!(
            "Screenshot saved to {output_path} ({}x{}, {} bytes)",
            image.width,
            image.height,
            png_data.len()
        ))
    }

    #[tool(description = "Non-blocking read of any available console output from QEMU.")]
    async fn read_output(&self) -> Result<CallToolResult, McpError> {
        let mut guard = lock_qemu(&self.qemu).await?;
        let instance = require_running(&mut guard)?;

        let data = instance.stdout().read_available().await;
        let output = String::from_utf8_lossy(&data);
        if output.is_empty() {
            text_result("(no output available)")
        } else {
            text_result(output.into_owned())
        }
    }

    #[tool(
        description = "Build the kernel (runs 'cmake --build build'). Optionally run clippy too."
    )]
    async fn build_kernel(
        &self,
        Parameters(params): Parameters<BuildKernelParams>,
    ) -> Result<CallToolResult, McpError> {
        let root = project_root().map_err(|e| mcp_err(format!("{e}")))?;

        let output = tokio::time::timeout(Duration::from_secs(30), {
            let mut cmd = Command::new("cmake");
            cmd.args(["--build", "build"]).current_dir(&root);
            run_command(cmd)
        })
        .await
        .map_err(|_| mcp_err("Build timed out (30s)"))?
        .map_err(|e| mcp_err(format!("Failed to run build: {e}")))?;

        let mut result = format_command_output("Build", "succeeded", &output);

        if params.clippy.unwrap_or(false) {
            let clippy_output = tokio::time::timeout(Duration::from_secs(30), {
                let mut cmd = Command::new("cmake");
                cmd.args(["--build", "build", "--target", "clippy"])
                    .current_dir(&root);
                run_command(cmd)
            })
            .await
            .map_err(|_| mcp_err("Clippy timed out (30s)"))?
            .map_err(|e| mcp_err(format!("Failed to run clippy: {e}")))?;

            result.push_str(&format!(
                "\n\n{}",
                format_command_output("Clippy", "succeeded", &clippy_output)
            ));
        }

        text_result(result)
    }

    #[tool(
        description = "Run system tests (cmake --build build --target test-system). Optionally run a specific test by name."
    )]
    async fn run_system_tests(
        &self,
        Parameters(params): Parameters<RunSystemTestsParams>,
    ) -> Result<CallToolResult, McpError> {
        let root = project_root().map_err(|e| mcp_err(format!("{e}")))?;

        let output = tokio::time::timeout(Duration::from_secs(60), {
            let cmd = match &params.test_name {
                Some(name) => {
                    let mut c = Command::new("cargo");
                    c.args([
                        "nextest",
                        "run",
                        "--release",
                        "--manifest-path",
                        "system-tests/Cargo.toml",
                        "--target",
                        "x86_64-unknown-linux-gnu",
                        name,
                    ])
                    .current_dir(&root);
                    c
                }
                None => {
                    let mut c = Command::new("cmake");
                    c.args(["--build", "build", "--target", "test-system"])
                        .current_dir(&root);
                    c
                }
            };
            run_command(cmd)
        })
        .await
        .map_err(|_| mcp_err("System tests timed out (60s)"))?
        .map_err(|e| mcp_err(format!("Failed to run system tests: {e}")))?;

        text_result(format_command_output("Tests", "PASSED", &output))
    }
}

impl ServerHandler for QemuMcpServer {
    fn get_info(&self) -> ServerInfo {
        // ServerInfo (== InitializeResult) is #[non_exhaustive] in rmcp 1.x,
        // so we fill it through the constructor + field assignment.
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        info.protocol_version = ProtocolVersion::V_2024_11_05;
        info.instructions = Some(
            "MCP server for interacting with Solaya kernel running in QEMU. \
             Use boot_qemu to start, then send_command to interact, \
             and shutdown_qemu when done. build_kernel and run_system_tests \
             handle the build/test cycle."
                .into(),
        );
        info
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let tool_name = request.name.clone();
        let tcc = ToolCallContext::new(self, request, context);
        tokio::time::timeout(Duration::from_secs(90), self.tool_router.call(tcc))
            .await
            .map_err(|_| mcp_err(format!("Tool '{}' timed out (90s safety limit)", tool_name)))?
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            meta: None,
            next_cursor: None,
        })
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tool_router.get(name).cloned()
    }
}
