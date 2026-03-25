use crate::infra::qemu::{QemuInstance, QemuOptions};

#[tokio::test]
async fn boot_smp() -> anyhow::Result<()> {
    QemuInstance::start().await?;
    Ok(())
}

#[tokio::test]
async fn boot_single_core() -> anyhow::Result<()> {
    QemuInstance::start_with(QemuOptions::default().use_smp(false)).await?;
    Ok(())
}

#[tokio::test]
async fn boot_with_network() -> anyhow::Result<()> {
    QemuInstance::start_with(QemuOptions::default().add_network_card(true)).await?;
    Ok(())
}

#[tokio::test]
async fn shutdown() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;

    solaya
        .run_prog_waiting_for("exit", "shutting down system")
        .await?;

    assert!(solaya.wait_for_qemu_to_exit().await?.success());

    Ok(())
}

#[tokio::test]
async fn execute_program() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;

    let output = solaya.run_prog("prog1").await?;

    assert_eq!(output, "Hello from Prog1\n");

    Ok(())
}

#[tokio::test]
async fn execute_same_program_twice() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;

    let expected = "Hello from Prog1\n";

    let output = solaya.run_prog("prog1").await?;
    assert_eq!(output, expected);

    let output = solaya.run_prog("prog1").await?;
    assert_eq!(output, expected);

    Ok(())
}

#[tokio::test]
async fn execute_different_programs() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;

    let output = solaya.run_prog("prog1").await?;
    assert_eq!(output, "Hello from Prog1\n");

    let output = solaya.run_prog("prog2").await?;
    assert!(output.contains("Hello from Prog2\n"));

    Ok(())
}

#[tokio::test]
async fn credential_syscalls() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("cred-test").await?;
    assert!(
        output.contains("child-ok"),
        "child should inherit creds: {output}"
    );
    assert!(
        output.contains("cred_test: OK"),
        "credential test failed: {output}"
    );
    Ok(())
}

#[tokio::test]
async fn unimplemented_syscall_kills_process_not_kernel() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;

    let output = solaya.run_prog("bad-syscall").await?;
    assert!(
        output.contains("[UNIMPLEMENTED SYSCALL]"),
        "Expected unimplemented syscall diagnostic, got: {output}"
    );
    assert!(
        output.contains("[BACKTRACE]"),
        "Expected backtrace, got: {output}"
    );
    assert!(
        !output.contains("BUG"),
        "Process should have been killed, got: {output}"
    );

    // Kernel should still be alive — run another program
    let output = solaya.run_prog("prog1").await?;
    assert_eq!(output, "Hello from Prog1\n");

    Ok(())
}
