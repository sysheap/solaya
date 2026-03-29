use crate::infra::qemu::QemuInstance;

#[tokio::test]
async fn ext2_read_file_from_bin() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    // The init binary is at /init on the ext2 root; verify we can list /bin
    let output = solaya.run_prog("ls-test /bin").await?;
    assert!(
        output.contains("cat"),
        "Expected cat in /bin, got: {}",
        output
    );
    assert!(
        output.contains("dash"),
        "Expected dash in /bin, got: {}",
        output
    );
    Ok(())
}

#[tokio::test]
async fn ext2_readdir_root() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("ls-test /").await?;
    assert!(output.contains("bin"), "Expected bin in /, got: {}", output);
    assert!(output.contains("lib"), "Expected lib in /, got: {}", output);
    assert!(output.contains("tmp"), "Expected tmp in /, got: {}", output);
    assert!(
        output.contains("proc"),
        "Expected proc in /, got: {}",
        output
    );
    assert!(output.contains("dev"), "Expected dev in /, got: {}", output);
    Ok(())
}
