#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dbg!(autoeq_api::entries().await?);
    dbg!(autoeq_api::targets().await?);
    Ok(())
}
