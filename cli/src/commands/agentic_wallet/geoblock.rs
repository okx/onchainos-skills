use anyhow::{bail, Result};

const PATH: &str = "/priapi/v5/wallet/agentic/geoblock/check";

pub(super) async fn cmd_check() -> Result<()> {
    let mut client = crate::wallet_api::WalletApiClient::new()?;
    let data = client.get_no_okheaders(PATH).await?;

    let blocked = data
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("blocked"))
        .and_then(|v| v.as_bool());

    match blocked {
        Some(b) => {
            println!("{{\"blocked\":{}}}", b);
            Ok(())
        }
        None => bail!("malformed response: missing data[0].blocked"),
    }
}
