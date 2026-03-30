use crate::infra::qemu::QemuInstance;

#[tokio::test]
async fn sendfile() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("sendfile-test").await?;
    assert_eq!(output, "sendfile: OK\n");
    Ok(())
}

#[tokio::test]
async fn cat_proc_version() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("cat /proc/version").await?;
    assert_eq!(output.trim(), "Solaya 0.1.0");
    Ok(())
}

#[tokio::test]
async fn touch_and_cat() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    solaya.run_prog("touch /tmp/test").await?;
    let output = solaya.run_prog("cat /tmp/test").await?;
    assert_eq!(output, "");
    Ok(())
}

#[tokio::test]
async fn rm_file() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    solaya.run_prog("touch /tmp/x").await?;
    solaya.run_prog("rm /tmp/x").await?;
    Ok(())
}

#[tokio::test]
async fn ls_root() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("ls-test /").await?;
    assert!(output.contains("bin"), "ls / should list bin");
    assert!(output.contains("lib"), "ls / should list lib");
    assert!(output.contains("tmp"), "ls / should list tmp");
    assert!(output.contains("proc"), "ls / should list proc");
    assert!(output.contains("dev"), "ls / should list dev");
    Ok(())
}

#[tokio::test]
async fn ls_proc() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("ls-test /proc").await?;
    assert!(output.contains("version"), "ls /proc should list version");
    Ok(())
}

#[tokio::test]
async fn rm_nonexistent() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    // Should not panic the kernel - rm should exit with an error
    solaya.run_prog("rm /tmp/nonexistent-file").await?;
    // Verify kernel is still alive
    let output = solaya.run_prog("cat /proc/version").await?;
    assert_eq!(output.trim(), "Solaya 0.1.0");
    Ok(())
}

#[tokio::test]
async fn vfs_roundtrip() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("vfs-test").await?;
    assert!(output.contains("OK create_and_write"));
    assert!(output.contains("OK read_back"));
    assert!(output.contains("OK proc_version"));
    assert!(output.contains("OK remove"));
    assert!(output.contains("OK gone"));
    Ok(())
}

#[tokio::test]
async fn devfs_null_and_zero() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("devfs_test").await?;
    assert!(output.contains("OK null_write"));
    assert!(output.contains("OK null_read"));
    assert!(output.contains("OK zero_read"));
    assert!(output.contains("OK zero_write"));
    Ok(())
}

#[tokio::test]
async fn cat_dev_null() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("cat /dev/null").await?;
    assert_eq!(output, "");
    Ok(())
}

#[tokio::test]
async fn ls_dev() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("ls-test /dev").await?;
    assert!(output.contains("null"), "ls /dev should list null");
    assert!(output.contains("zero"), "ls /dev should list zero");
    assert!(output.contains("random"), "ls /dev should list random");
    assert!(
        output.contains("vda"),
        "/dev/vda should appear (block device always attached)"
    );
    Ok(())
}

#[tokio::test]
async fn pread_pwrite() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("pread-test").await?;
    assert_eq!(output, "pread_pwrite: OK\n");
    Ok(())
}

#[tokio::test]
async fn vfs_metadata() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("metadata-test").await?;
    assert_eq!(output, "metadata: OK\n");
    Ok(())
}

#[tokio::test]
async fn file_metadata_ops() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("fmeta-test").await?;
    assert!(output.contains("OK ftruncate_grow"));
    assert!(output.contains("OK ftruncate_shrink"));
    assert!(output.contains("OK fchmod"));
    assert!(output.contains("OK fchown"));
    assert!(output.contains("OK fchown_partial"));
    Ok(())
}

#[tokio::test]
async fn ls_long_format() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("ls -alh /tmp").await?;
    assert!(
        !output.contains("No such file"),
        "ls -alh should not fail: {output}"
    );
    Ok(())
}

#[tokio::test]
async fn symlinks_and_links() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("symlink-test").await?;
    assert_eq!(output, "symlink_test: OK\n");
    Ok(())
}
