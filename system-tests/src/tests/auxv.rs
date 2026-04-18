use crate::infra::qemu::QemuInstance;

#[tokio::test]
async fn auxv_roundtrip() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("auxv-test").await?;
    assert_eq!(output, "auxv: OK\n");
    Ok(())
}
