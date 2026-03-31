use crate::infra::qemu::QemuInstance;

#[tokio::test]
async fn connect4() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;

    solaya
        .run_prog_waiting_for("connect4", "search depth:")
        .await?;

    solaya.write_and_wait_for("10\n", "(h)uman").await?;

    solaya
        .write_and_wait_for("c\n", "Calculating moves")
        .await?;

    solaya.ctrl_c_and_assert_prompt().await?;

    Ok(())
}
