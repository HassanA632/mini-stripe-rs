use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

use crate::signature;

pub async fn post_webhook(
    client: &Client,
    url: &str,
    secret: &str,
    body: &Value,
) -> Result<u16, String> {
    let bytes = serde_json::to_vec(body).map_err(|e| format!("json encode: {e}"))?;
    let sig = signature::sign(secret, &bytes);

    let res = client
        .post(url)
        .timeout(Duration::from_secs(5))
        .header("content-type", "application/json")
        .header("x-ministripe-signature", sig)
        .body(bytes)
        .send()
        .await
        .map_err(|e| format!("http error: {e}"))?;

    Ok(res.status().as_u16())
}
