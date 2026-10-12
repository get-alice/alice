use alice_core::{ai, store::Store, AiRequest, SaveAiSettings};
use axum::{Router, Json, routing::post, http::HeaderMap};
use keyring::credential::{CredentialApi, CredentialBuilderApi, Credential};
use std::{any::Any, collections::HashMap, sync::{Arc, Mutex}};

type Secrets = Arc<Mutex<HashMap<String, Vec<u8>>>>;
struct Builder(Secrets);
struct Entry { key: String, values: Secrets }
impl CredentialBuilderApi for Builder {
    fn build(&self, _: Option<&str>, service: &str, user: &str) -> keyring::Result<Box<Credential>> {
        Ok(Box::new(Entry { key: format!("{service}:{user}"), values: self.0.clone() }))
    }
    fn as_any(&self) -> &dyn Any { self }
}
impl CredentialApi for Entry {
    fn set_secret(&self, value: &[u8]) -> keyring::Result<()> {
        self.values.lock().unwrap().insert(self.key.clone(), value.to_vec()); Ok(())
    }
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        self.values.lock().unwrap().get(&self.key).cloned().ok_or(keyring::Error::NoEntry)
    }
    fn delete_credential(&self) -> keyring::Result<()> {
        self.values.lock().unwrap().remove(&self.key).ok_or(keyring::Error::NoEntry)?; Ok(())
    }
    fn as_any(&self) -> &dyn Any { self }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    keyring::set_default_credential_builder(Box::new(Builder(Arc::default())));
    let received = Arc::new(Mutex::new(false));
    let spy = received.clone();
    let app = Router::new().route("/v1/chat/completions", post(move |headers: HeaderMap| {
        let spy = spy.clone(); async move {
            *spy.lock().unwrap() = headers.get("authorization").is_some_and(|v| v == "Bearer fixture-key-A");
            Json(serde_json::json!({"choices":[{"message":{"content":"{\"text\":\"Fixture answer\",\"actions\":[]}"}}]}))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let data = std::env::args().nth(1).unwrap();
    let store = Store::open(std::path::Path::new(&data))?;
    ai::save_settings(&store, SaveAiSettings { provider: "compatible".into(), model: "fixture".into(), base_url: "http://127.0.0.1:9/v1".into(), api_key: Some("fixture-key-A".into()) })?;
    let settings = ai::save_settings(&store, SaveAiSettings { provider: "compatible".into(), model: "fixture".into(), base_url: format!("http://127.0.0.1:{port}/v1"), api_key: None })?;
    ai::run(&store, AiRequest { prompt: "Summarize this fixture".into(), message_id: Some("demo-1".into()) }).await?;
    println!("endpoint_change_without_new_key_accepted={}", settings.configured);
    println!("second_origin_received_first_origin_key={}", *received.lock().unwrap());
    server.abort();
    Ok(())
}
