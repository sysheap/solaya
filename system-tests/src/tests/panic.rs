use crate::infra::qemu::QemuInstance;

#[tokio::test]
async fn panic() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya
        .run_prog_waiting_for("panic", "Time to attach gdb ;) use 'just attach'")
        .await?;

    assert!(output.contains("Hello from Panic! Triggering kernel panic"));
    assert!(output.contains("KERNEL Panic"));
    assert!(output.contains("<solaya::panic::panic_handler"));

    Ok(())
}
