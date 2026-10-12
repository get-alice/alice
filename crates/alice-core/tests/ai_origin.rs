use alice_core::{AiRequest, SaveAiSettings, ai, i18n::Locale, store::Store};
use axum::{Json, Router, http::HeaderMap, routing::post};
use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
use std::{
    any::Any,
    collections::HashMap,
    sync::{Arc, Mutex},
};

type Secrets = Arc<Mutex<HashMap<String, Vec<u8>>>>;
struct Builder(Secrets);
struct Entry {
    key: String,
    values: Secrets,
}
impl CredentialBuilderApi for Builder {
    fn build(
        &self,
        _: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<Credential>> {
        Ok(Box::new(Entry {
            key: format!("{service}:{user}"),
            values: self.0.clone(),
        }))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
impl CredentialApi for Entry {
    fn set_secret(&self, value: &[u8]) -> keyring::Result<()> {
        self.values
            .lock()
            .unwrap()
            .insert(self.key.clone(), value.to_vec());
        Ok(())
    }
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        self.values
            .lock()
            .unwrap()
            .get(&self.key)
            .cloned()
            .ok_or(keyring::Error::NoEntry)
    }
    fn delete_credential(&self) -> keyring::Result<()> {
        self.values
            .lock()
            .unwrap()
            .remove(&self.key)
            .ok_or(keyring::Error::NoEntry)?;
        Ok(())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[tokio::test]
async fn existing_keys_never_cross_endpoint_origins() -> anyhow::Result<()> {
    let secrets: Secrets = Arc::default();
    keyring::set_default_credential_builder(Box::new(Builder(secrets.clone())));
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let spy = received.clone();
    let prompts = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
    let prompt_spy = prompts.clone();
    let app = Router::new().route("/v1/chat/completions", post(move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
        let spy = spy.clone();
        let prompt_spy = prompt_spy.clone();
        async move {
            spy.lock().unwrap().push(headers.get("authorization").unwrap().to_str().unwrap().to_owned());
            prompt_spy.lock().push(body["messages"][0]["content"].as_str().unwrap().to_owned());
            Json(serde_json::json!({"choices":[{"message":{"content":"{\"text\":\"Fixture answer\",\"actions\":[]}"}}]}))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let directory = tempfile::tempdir()?;
    let store = Store::open(&directory.path().join("mail.db"))?;
    store.add_demo()?;
    let original = "http://127.0.0.1:9/v1";
    let next = format!("http://127.0.0.1:{port}/v1");
    let config = |url: &str, key: Option<&str>| SaveAiSettings {
        provider: "compatible".into(),
        model: "fixture".into(),
        base_url: url.into(),
        api_key: key.map(str::to_owned),
    };
    ai::save_settings(&store, config(original, Some("fixture-key-A"))).await?;
    ai::save_settings(&store, config(original, None)).await?;
    assert!(
        ai::save_settings(&store, config(&next, None))
            .await
            .is_err()
    );
    assert_eq!(store.ai_settings()?.base_url, original);
    assert!(received.lock().unwrap().is_empty());
    ai::save_settings(&store, config(&next, Some("fixture-key-B"))).await?;
    ai::run(
        &store,
        AiRequest {
            prompt: "Summarize this fixture".into(),
            message_id: Some("demo-1".into()),
        },
        Locale::De,
    )
    .await?;
    assert_eq!(*received.lock().unwrap(), ["Bearer fixture-key-B"]);
    let other = Store::open(&directory.path().join("other.db"))?;
    ai::save_settings(&other, config(original, Some("fixture-key-A"))).await?;
    ai::run(
        &store,
        AiRequest {
            prompt: "Summarize again".into(),
            message_id: Some("demo-1".into()),
        },
        Locale::EnUs,
    )
    .await?;
    assert_eq!(
        *received.lock().unwrap(),
        ["Bearer fixture-key-B", "Bearer fixture-key-B"]
    );
    assert!(prompts.lock()[0].contains("German (de)"));
    assert!(prompts.lock()[1].contains("English (en-US)"));
    secrets.lock().unwrap().clear();
    alice_core::vault::save(
        "ai-compatible",
        &alice_core::vault::Secret {
            password: Some("legacy-key-A".into()),
            ..Default::default()
        },
    )
    .await?;
    assert!(
        ai::run(
            &store,
            AiRequest {
                prompt: "A missing scoped key must not use the legacy key".into(),
                message_id: Some("demo-1".into()),
            },
            Locale::EnUs,
        )
        .await
        .is_err()
    );
    assert_eq!(
        *received.lock().unwrap(),
        ["Bearer fixture-key-B", "Bearer fixture-key-B"]
    );
    server.abort();
    Ok(())
}
