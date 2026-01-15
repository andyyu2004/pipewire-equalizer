use autoeq_api::EqualizeRequest;

#[tokio::main]
async fn main() -> reqwest::Result<()> {
    let client = reqwest::Client::new();
    // dbg!(autoeq_api::entries(&client).await?);
    // dbg!(autoeq_api::targets(&client).await?);
    dbg!(
        autoeq_api::equalize(
            &client,
            &EqualizeRequest {
                target: "Harman over-ear 2018".to_string(),
                name: "Focal Clear".to_string(),
                source: "oratory1990".to_string(),
                rig: "GRAS 45BC-10".to_string(),
                sample_rate: 48000,
            }
        )
        .await?
    );
    Ok(())
}
