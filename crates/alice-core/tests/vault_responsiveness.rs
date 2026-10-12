use alice_core::{daemon, store::Store};
use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
use std::{
    any::Any,
    io::{Read, Write},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

struct Gate {
    started: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    value: Mutex<Vec<u8>>,
}
struct Builder(Arc<Gate>);
struct Entry(Arc<Gate>);
impl CredentialBuilderApi for Builder {
    fn build(&self, _: Option<&str>, _: &str, _: &str) -> keyring::Result<Box<Credential>> {
        Ok(Box::new(Entry(self.0.clone())))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
impl CredentialApi for Entry {
    fn set_secret(&self, value: &[u8]) -> keyring::Result<()> {
        self.0.started.send(()).unwrap();
        self.0
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        *self.0.value.lock().unwrap() = value.to_vec();
        Ok(())
    }
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        self.0.started.send(()).unwrap();
        self.0
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        Ok(self.0.value.lock().unwrap().clone())
    }
    fn delete_credential(&self) -> keyring::Result<()> {
        Ok(())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[tokio::test]
async fn credential_prompt_does_not_stall_the_local_api() -> anyhow::Result<()> {
    let (started, waiting) = mpsc::channel();
    let (release, released) = mpsc::channel();
    keyring::set_default_credential_builder(Box::new(Builder(Arc::new(Gate {
        started,
        release: Mutex::new(released),
        value: Mutex::default(),
    }))));
    let directory = tempfile::tempdir()?;
    let store = Arc::new(Store::open(&directory.path().join("mail.db"))?);
    let app = daemon::router(store, "fixture-token".into());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let waiting = Arc::new(Mutex::new(waiting));
    for api_key in [Some("fixture-key"), None] {
        let waiting = waiting.clone();
        let release = release.clone();
        // The probe and release run outside Tokio, so a blocked runtime cannot hide the failure.
        let probe = std::thread::spawn(move || {
            waiting
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            let outcome = (|| -> std::io::Result<bool> {
                let mut socket = std::net::TcpStream::connect(address)?;
                socket.set_read_timeout(Some(Duration::from_millis(500)))?;
                socket.write_all(b"GET /v1/health HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer fixture-token\r\nConnection: close\r\n\r\n")?;
                let mut response = String::new();
                socket.read_to_string(&mut response)?;
                Ok(response.starts_with("HTTP/1.1 200"))
            })();
            release.send(()).unwrap();
            outcome.unwrap_or(false)
        });
        let response = reqwest::Client::new()
            .post(format!("http://{address}/v1/ai/settings"))
            .bearer_auth("fixture-token")
            .json(
                &serde_json::json!({ "provider": "compatible", "model": "fixture",
            "base_url": "http://127.0.0.1:9/v1", "api_key": api_key }),
            )
            .send()
            .await?;
        assert!(response.status().is_success());
        assert!(
            probe.join().unwrap(),
            "Credential access stalled the authenticated health API"
        );
    }
    server.abort();
    Ok(())
}
