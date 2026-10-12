use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Secret {
    pub password: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    /// Microsoft Graph access token for contacts and calendar; Outlook mail uses `access_token`.
    /// Microsoft issues one token per API, both from the same `refresh_token`.
    #[serde(default)]
    pub graph_token: Option<String>,
    #[serde(default)]
    pub graph_expires_at: Option<i64>,
}

fn entry(id: &str) -> Result<keyring::Entry> {
    let namespace = crate::client::data_dir()?;
    keyring::Entry::new(
        "email.getalice.alice",
        &format!("{}:{id}", namespace.display()),
    )
    .context(crate::text!("server.error.credentials_unavailable"))
}
pub async fn save(id: &str, secret: &Secret) -> Result<()> {
    let id = id.to_owned();
    let value = serde_json::to_string(secret)?;
    tokio::task::spawn_blocking(move || {
        entry(&id)?
            .set_password(&value)
            .context(crate::text!("server.error.credentials_save"))
    })
    .await?
}
pub async fn load(id: &str) -> Result<Secret> {
    let id = id.to_owned();
    let value = tokio::task::spawn_blocking(move || match entry(&id)?.get_password() {
        Ok(value) => Ok(value),
        Err(keyring::Error::NoEntry) => Err(anyhow::anyhow!(crate::text!(
            "server.error.credentials_missing"
        ))),
        // Shown verbatim as the account's sync error, so the OS reason goes in the message itself.
        Err(error) => Err(anyhow::anyhow!(crate::text!(
            "server.error.credentials_read",
            error = error.to_string()
        ))),
    })
    .await??;
    serde_json::from_str(&value).context(crate::text!("server.error.credentials_invalid"))
}
pub async fn delete(id: &str) -> Result<()> {
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || match entry(&id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error).context(crate::text!("server.error.credentials_delete")),
    })
    .await?
}
