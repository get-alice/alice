#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod clipboard;
mod contacts;
mod desktop;
mod downloads;
mod notification;

use alice_core::{
    Client, Preferences,
    client::{ApiFailure, needs_reconnect},
    i18n::{self, Locale},
};
use serde::de::DeserializeOwned;
use serde_json::{Value, value::RawValue};
use std::sync::Arc;

/// The app's daemon client, shared by every IPC call so its HTTP connection pool stays warm.
#[derive(Default)]
struct Daemon(tokio::sync::Mutex<Option<Arc<Client>>>);
impl Daemon {
    /// The current client, or a fresh one (starting the daemon if needed) when there is none or
    /// `stale` is the current one. The lock is held while connecting so callers share one attempt.
    async fn client(&self, stale: Option<&Arc<Client>>) -> anyhow::Result<Arc<Client>> {
        let mut current = self.0.lock().await;
        if let Some(client) = current.as_ref()
            && !stale.is_some_and(|stale| Arc::ptr_eq(stale, client))
        {
            return Ok(client.clone());
        }
        let client = Arc::new(Client::connect(&["--daemon"]).await?);
        *current = Some(client.clone());
        Ok(client)
    }
    /// Sends a daemon API request, reconnecting once if the daemon was restarted or is not running.
    /// The response JSON is kept as text.
    async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> anyhow::Result<Box<RawValue>> {
        self.request_in(Locale::EnUs, method, path, body).await
    }
    async fn request_in(
        &self,
        locale: Locale,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> anyhow::Result<Box<RawValue>> {
        let client = self.client(None).await?;
        match client.request_raw_in(locale, method, path, body).await {
            Err(error) if needs_reconnect(&error) => {
                self.client(Some(&client))
                    .await?
                    .request_raw_in(locale, method, path, body)
                    .await
            }
            response => response,
        }
    }
    async fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        Ok(serde_json::from_str(
            self.request("GET", path, None).await?.get(),
        )?)
    }
}

#[derive(serde::Serialize)]
struct CommandError {
    status: Option<u16>,
    message: String,
}

#[tauri::command]
fn system_locale() -> &'static str {
    Locale::system().tag()
}

/// Forwards a daemon API request. The response JSON passes through without being rebuilt.
#[tauri::command]
async fn daemon_request(
    app: tauri::AppHandle,
    daemon: tauri::State<'_, Daemon>,
    method: String,
    path: String,
    body: Option<Value>,
    locale: Option<String>,
) -> Result<Box<RawValue>, CommandError> {
    let locale = locale
        .as_deref()
        .and_then(Locale::from_tag)
        .unwrap_or(Locale::EnUs);
    let response = daemon
        .request_in(locale, &method, &path, body.as_ref())
        .await
        .map_err(|error| CommandError {
            status: error.downcast_ref::<ApiFailure>().map(|error| error.status),
            message: i18n::error_text(&error, locale),
        })?;
    // A settings change applies at once rather than at the next check.
    if method == "PATCH"
        && path == "/v1/preferences"
        && let Ok(preferences) = serde_json::from_str::<Preferences>(response.get())
    {
        desktop::apply(&app, &preferences);
    }
    Ok(response)
}
#[tauri::command]
fn open_auth_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let parsed = url.parse::<tauri::Url>().map_err(|e| e.to_string())?;
    if parsed.scheme() != "https"
        || ![
            Some("accounts.google.com"),
            Some("login.microsoftonline.com"),
        ]
        .contains(&parsed.host_str())
    {
        return Err(alice_core::t!(
            desktop::locale(&app),
            "desktop.error.sign_in_url"
        ));
    }
    open::that(url).map_err(|e| e.to_string())
}
#[tauri::command]
fn open_help_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let parsed = url.parse::<tauri::Url>().map_err(|e| e.to_string())?;
    if parsed.scheme() != "https"
        || ![
            "developers.google.com",
            "support.google.com",
            "learn.microsoft.com",
            "developers.openai.com",
            "platform.claude.com",
            "docs.ollama.com",
        ]
        .contains(&parsed.host_str().unwrap_or(""))
    {
        return Err(alice_core::t!(
            desktop::locale(&app),
            "desktop.error.help_url"
        ));
    }
    open::that(url).map_err(|e| e.to_string())
}
#[tauri::command]
fn open_email_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    open_email_url_in(desktop::locale(&app), url)
}
fn open_email_url_in(locale: Locale, url: String) -> Result<(), String> {
    let parsed = url.parse::<tauri::Url>().map_err(|e| e.to_string())?;
    if !["http", "https"].contains(&parsed.scheme())
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(alice_core::t!(locale, "desktop.error.email_url"));
    }
    open::that(url).map_err(|e| e.to_string())
}
fn main() {
    if std::env::args().any(|arg| arg == "--daemon") {
        let runtime = tokio::runtime::Runtime::new().expect(&alice_core::t!(
            Locale::system(),
            "desktop.error.daemon_runtime"
        ));
        if let Err(error) = runtime.block_on(alice_core::daemon::run()) {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return;
    }
    tauri::Builder::default()
        .manage(Daemon::default())
        .manage(desktop::Desktop::default())
        .setup(|app| {
            desktop::setup(app.handle());
            Ok(())
        })
        .on_window_event(desktop::on_window_event)
        .invoke_handler(tauri::generate_handler![
            daemon_request,
            system_locale,
            desktop::take_opened,
            clipboard::copy_message_otp,
            downloads::open_attachment,
            downloads::saved_attachments,
            downloads::read_attachment,
            downloads::reveal_download,
            downloads::open_calendar_file,
            contacts::read_system_contacts,
            open_auth_url,
            open_help_url,
            open_email_url
        ])
        .build(tauri::generate_context!())
        .expect(&alice_core::t!(Locale::system(), "desktop.error.start"))
        .run(desktop::on_run_event);
}

#[cfg(test)]
mod tests {
    #[test]
    fn email_links_cannot_open_local_files_scripts_or_embedded_credentials() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,test",
            "https://user:password@example.org",
            "/relative",
        ] {
            assert!(
                super::open_email_url_in(alice_core::i18n::Locale::EnUs, url.into()).is_err(),
                "Accepted unsafe URL: {url}"
            );
        }
    }
}
