use crate::i18n::Locale;
use anyhow::{Context, Result, bail};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, value::RawValue};
#[cfg(not(windows))]
use std::process::{Command, Stdio};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[cfg(windows)]
fn spawn_daemon(
    exe: &std::path::Path,
    daemon_args: &[&str],
) -> Result<winsafe::guard::CloseHandlePiGuard> {
    let exe = exe
        .to_str()
        .context(crate::text!("server.error.daemon_start"))?;
    let mut command_line = format!("\"{exe}\"");
    for arg in daemon_args {
        command_line.push_str(" \"");
        let mut backslashes = 0;
        for ch in arg.chars() {
            if ch == '\\' {
                backslashes += 1;
                continue;
            }
            let count = if ch == '"' {
                backslashes * 2 + 1
            } else {
                backslashes
            };
            command_line.extend(std::iter::repeat_n('\\', count));
            command_line.push(ch);
            backslashes = 0;
        }
        command_line.extend(std::iter::repeat_n('\\', backslashes * 2));
        command_line.push('"');
    }
    // Stdio::null() still leaks inheritable parent pipes through Rust's Windows spawn.
    Ok(winsafe::CreateProcess(
        Some(exe),
        Some(&command_line),
        None,
        None,
        false,
        winsafe::co::CREATE::NO_WINDOW,
        &[],
        None,
        &mut winsafe::STARTUPINFO::default(),
    )?)
}

pub fn data_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("ALICE_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    Ok(dirs::data_local_dir()
        .context(crate::text!("server.error.data_directory"))?
        .join("Alice"))
}
pub fn port() -> Result<u16> {
    std::env::var("ALICE_PORT")
        .unwrap_or_else(|_| "4815".into())
        .parse()
        .context(crate::text!("server.error.invalid_port"))
}
/// This binary's daemon API identity, reported by `/v1/health`. A daemon of another build is
/// replaced on connect, even at the same version: its endpoints may differ.
pub const BUILD: &str = env!("ALICE_BUILD");
/// The version with its build, as `alice --version` prints it.
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("ALICE_BUILD"), ")");
pub fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    token: Arc<str>,
    base: Arc<str>,
    locale: Locale,
}
impl Client {
    pub fn existing() -> Result<Self> {
        let token = std::fs::read_to_string(data_dir()?.join("daemon.token"))
            .context(crate::text!("server.error.daemon_not_running"))?;
        Ok(Self {
            http: http_client()?,
            token: token.trim().into(),
            base: format!("http://127.0.0.1:{}", port()?).into(),
            locale: Locale::EnUs,
        })
    }
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }
    pub async fn connect(daemon_args: &[&str]) -> Result<Self> {
        if let Ok(client) = Self::existing()
            && let Ok(health) = client.request_value("GET", "/v1/health", None).await
        {
            if health["build"] == BUILD {
                return Ok(client);
            }
            // A daemon left running by another build serves another API: endpoints this build
            // calls may be missing. Stop it, and start this build's own once it releases its lock.
            client.request_value("POST", "/v1/shutdown", None).await?;
            let lock = std::fs::File::open(data_dir()?.join("daemon.lock"))?;
            let mut stopped = false;
            for _ in 0..100 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if lock.try_lock().is_ok() {
                    stopped = true;
                    break;
                }
            }
            drop(lock);
            if !stopped {
                bail!(crate::text!("server.error.daemon_outdated"));
            }
        }
        let exe = std::env::current_exe()?;
        #[cfg(not(windows))]
        let mut child = {
            let mut command = Command::new(exe);
            command
                .args(daemon_args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                // Keep terminal hangups and foreground signals from reaching the daemon.
                command.process_group(0);
            }
            command
                .spawn()
                .context(crate::text!("server.error.daemon_start"))?
        };
        #[cfg(windows)]
        let child =
            spawn_daemon(&exe, daemon_args).context(crate::text!("server.error.daemon_start"))?;
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if let Ok(client) = Self::existing()
                && client
                    .request_value("GET", "/v1/health", None)
                    .await
                    .is_ok()
            {
                return Ok(client);
            }
            #[cfg(not(windows))]
            let status = child.try_wait()?;
            #[cfg(windows)]
            let status = {
                use std::os::windows::process::ExitStatusExt;
                if child.hProcess.WaitForSingleObject(Some(0))? == winsafe::co::WAIT::OBJECT_0 {
                    Some(std::process::ExitStatus::from_raw(
                        child.hProcess.GetExitCodeProcess()?,
                    ))
                } else {
                    None
                }
            };
            if let Some(status) = status {
                bail!(crate::text!(
                    "server.error.daemon_exited",
                    status = status.to_string()
                ));
            }
        }
        bail!(crate::text!("server.error.daemon_not_ready"))
    }
    /// Sends a request and returns the successful response body, or an [`ApiFailure`].
    async fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<impl AsRef<[u8]>> {
        Ok(self.exchange(method, path, body).await?.1)
    }
    /// Downloads an attachment: the file name the daemon suggests and the file's bytes.
    pub async fn download(&self, path: &str) -> Result<(String, Vec<u8>)> {
        let (headers, bytes) = self.exchange("GET", path, None).await?;
        let name = headers
            .get("x-alice-filename")
            .and_then(|v| v.to_str().ok())
            .map(|value| String::from_utf8_lossy(&crate::mime::percent_decode(value)).into_owned())
            .unwrap_or_default();
        Ok((name, bytes.as_ref().to_vec()))
    }
    pub async fn download_in(&self, locale: Locale, path: &str) -> Result<(String, Vec<u8>)> {
        self.clone().with_locale(locale).download(path).await
    }
    async fn exchange(
        &self,
        method: &str,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<(reqwest::header::HeaderMap, impl AsRef<[u8]>)> {
        if !path.starts_with("/v1/") || path.contains("..") || path.contains('#') {
            bail!(crate::text!("server.error.api_path"));
        }
        let method = reqwest::Method::from_bytes(method.as_bytes())?;
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base, path))
            .bearer_auth(self.token.as_ref())
            .header(reqwest::header::ACCEPT_LANGUAGE, self.locale.tag());
        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }
        let response = request
            .send()
            .await
            .context(crate::text!("server.error.daemon_unreachable"))?;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .bytes()
            .await
            .context(crate::text!("server.error.daemon_response"))?;
        if !status.is_success() {
            let message = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|v| v.pointer("/error/message")?.as_str().map(str::to_owned))
                .unwrap_or_else(|| crate::t!(self.locale, "server.error.request_failed"));
            return Err(ApiFailure {
                status: status.as_u16(),
                message,
            }
            .into());
        }
        Ok((headers, bytes))
    }
}
impl Client {
    pub async fn request_value(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value> {
        let body = body.map(serde_json::to_vec).transpose()?;
        decode(self.send(method, path, body).await?)
    }
    /// Like [`Self::request_value`], but keeps the response as validated JSON text for forwarding.
    pub async fn request_raw(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Box<RawValue>> {
        let body = body.map(serde_json::to_vec).transpose()?;
        decode(self.send(method, path, body).await?)
    }
    pub async fn request_raw_in(
        &self,
        locale: Locale,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Box<RawValue>> {
        self.clone()
            .with_locale(locale)
            .request_raw(method, path, body)
            .await
    }
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        decode(self.send("GET", path, None).await?)
    }
    pub async fn post<T: DeserializeOwned>(&self, path: &str, body: &impl Serialize) -> Result<T> {
        decode(
            self.send("POST", path, Some(serde_json::to_vec(body)?))
                .await?,
        )
    }
    pub async fn patch<T: DeserializeOwned>(&self, path: &str, body: &impl Serialize) -> Result<T> {
        decode(
            self.send("PATCH", path, Some(serde_json::to_vec(body)?))
                .await?,
        )
    }
}
fn decode<T: DeserializeOwned>(bytes: impl AsRef<[u8]>) -> Result<T> {
    serde_json::from_slice(bytes.as_ref()).context(crate::text!("server.error.daemon_response"))
}

/// An error response from the daemon. Displays as the daemon's message.
#[derive(Debug)]
pub struct ApiFailure {
    pub status: u16,
    pub message: String,
}
impl std::fmt::Display for ApiFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ApiFailure {}

/// Whether a request failed before the daemon handled it, because the daemon is not running or was
/// restarted with a new token. Such a request is safe to retry after [`Client::connect`].
pub fn needs_reconnect(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_connect)
            || cause
                .downcast_ref::<ApiFailure>()
                .is_some_and(|failure| failure.status == 401)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn requests_carry_the_selected_language_without_changing_the_client() -> Result<()> {
        async fn language(headers: axum::http::HeaderMap) -> axum::Json<Value> {
            axum::Json(
                serde_json::json!({"language": headers["Accept-Language"].to_str().unwrap()}),
            )
        }
        async fn attachment(headers: axum::http::HeaderMap) -> String {
            headers["Accept-Language"].to_str().unwrap().to_string()
        }
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let client = Client {
            http: http_client()?,
            token: "test-token".into(),
            base: format!("http://{}", listener.local_addr()?).into(),
            locale: Locale::EnUs,
        };
        let app = axum::Router::new()
            .route("/v1/locale", axum::routing::get(language))
            .route("/v1/attachment", axum::routing::get(attachment));
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let outcome: Result<()> = async {
            let value = client.request_value("GET", "/v1/locale", None).await?;
            assert_eq!(value["language"], "en-US");
            let raw = client
                .request_raw_in(Locale::De, "GET", "/v1/locale", None)
                .await?;
            let value: Value = serde_json::from_str(raw.get())?;
            assert_eq!(value["language"], "de");
            let value = client.request_value("GET", "/v1/locale", None).await?;
            assert_eq!(value["language"], "en-US");
            let german = client.with_locale(Locale::De);
            let value = german.request_value("GET", "/v1/locale", None).await?;
            assert_eq!(value["language"], "de");
            let (_, bytes) = german.download_in(Locale::EnUs, "/v1/attachment").await?;
            assert_eq!(bytes, b"en-US");
            Ok(())
        }
        .await;
        server.abort();
        outcome
    }
}
