use crate::infra::qemu::QemuInstance;

#[tokio::test]
async fn fork_basic() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;

    let output = solaya.run_prog("fork-test").await?;

    assert!(output.contains("child"), "expected child output: {output}");
    assert!(
        output.contains("parent waited"),
        "expected parent output: {output}"
    );

    Ok(())
}

#[tokio::test]
async fn fork_cow_isolation() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;

    let output = solaya.run_prog("cow-test").await?;

    assert!(
        output.contains("child: value=99"),
        "child should see modified value: {output}"
    );
    assert!(
        output.contains("parent: value=42"),
        "parent should see original value: {output}"
    );

    Ok(())
}
