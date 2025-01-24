use anyhow::Result;
mod fanos;

#[async_std::main]
async fn main() -> Result<()> {

    let mut client = fanos::FanosClient::new().await?;
    client.send(b"EVAL echo $PWD", None).await?;
    client.recv().await?;

    Ok(())
}
