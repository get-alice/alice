use crate::i18n::{self, Locale};
use anyhow::{Context, Result, bail};
use axum::Json;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Deserialize)]
struct Release {
    version: String,
    url: String,
}
fn release_status(bytes: &[u8]) -> Result<Value> {
    let release: Release =
        serde_json::from_slice(bytes).context(crate::text!("server.error.release_json"))?;
    let latest = semver::Version::parse(release.version.trim_start_matches('v'))
        .context(crate::text!("server.error.release_version"))?;
    let installed = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
    validate_url(&release.url)?;
    Ok(
        json!({"status":if latest > installed {"available"} else {"current"}, "version":latest.to_string(), "url":release.url}),
    )
}
fn validate_url(value: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(value)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!(crate::text!("server.error.update_url"));
    }
    Ok(url)
}
async fn fetch_release(feed: &str) -> Result<Value> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut response = http
        .get(validate_url(feed)?)
        .send()
        .await?
        .error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 64_000 {
            bail!(crate::text!("server.error.release_size"));
        }
        bytes.extend_from_slice(&chunk);
    }
    release_status(&bytes)
}
pub async fn check(locale: Locale) -> Json<Value> {
    let Ok(feed) = std::env::var("ALICE_RELEASE_FEED") else {
        return Json(
            json!({"status":"unavailable","message":crate::t!(locale, "server.updates.unavailable")}),
        );
    };
    Json(match fetch_release(&feed).await {
        Ok(result) => result,
        Err(error) => {
            json!({"status":"error","message":crate::t!(locale, "server.updates.failed", error = i18n::error_text(&error, locale))})
        }
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn release_feed_compares_versions_and_rejects_unsafe_downloads() {
        for (version, status) in [
            ("0.1.0", "current"),
            ("v0.2.0", "available"),
            ("0.0.9", "current"),
        ] {
            let value = super::release_status(
                format!(r#"{{"version":"{version}","url":"https://example.org/release"}}"#)
                    .as_bytes(),
            )
            .unwrap();
            assert_eq!(value["status"], status);
        }
        for url in [
            "file:///tmp/release",
            "http://example.org/release",
            "https://user:secret@example.org/release",
            "javascript:alert(1)",
        ] {
            assert!(
                super::release_status(format!(r#"{{"version":"0.2.0","url":"{url}"}}"#).as_bytes())
                    .is_err()
            );
        }
        assert!(
            super::release_status(br#"{"version":"broken","url":"https://example.org/release"}"#)
                .is_err()
        );
    }
}
