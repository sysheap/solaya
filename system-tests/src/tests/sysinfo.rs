use crate::infra::qemu::QemuInstance;

#[tokio::test]
async fn test_sysinfo_syscalls() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("sysinfo_test").await?;
    assert!(output.contains("Linux"), "expected Linux in uname output");
    assert!(output.contains("solaya"), "expected solaya in uname output");
    assert!(
        output.contains("riscv64"),
        "expected riscv64 in uname output"
    );
    assert!(output.contains("OK uname"), "uname test failed");
    assert!(output.contains("OK sysinfo"), "sysinfo test failed");
    assert!(output.contains("OK getrandom"), "getrandom test failed");
    assert!(output.contains("OK getrusage"), "getrusage test failed");
    Ok(())
}
