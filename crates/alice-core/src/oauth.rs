use crate::{
    client::http_client,
    i18n::Locale,
    model::{Account, Integrations, Provider},
    vault::{self, Secret},
};
use anyhow::{Context, Result, bail};
use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::Html,
    routing::get,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use uuid::Uuid;

const REDIRECT: &str = "http://127.0.0.1:4816/callback";
const GOOGLE_MAIL: &str = "https://mail.google.com/";
const GOOGLE_CONTACTS: [&str; 2] = [
    "https://www.googleapis.com/auth/contacts.readonly",
    "https://www.googleapis.com/auth/contacts.other.readonly",
];
const GOOGLE_CALENDAR: [&str; 2] = [
    "https://www.googleapis.com/auth/calendar.readonly",
    "https://www.googleapis.com/auth/calendar.events",
];
const OUTLOOK_MAIL: &str = "offline_access https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send";
const GRAPH_CONTACTS: [&str; 1] = ["https://graph.microsoft.com/Contacts.Read"];
const GRAPH_CALENDAR: [&str; 1] = ["https://graph.microsoft.com/Calendars.ReadWrite"];

/// Provider APIs reached with an account's OAuth sign-in besides mail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    /// Google People and Google Calendar, for Gmail accounts.
    Google,
    /// Microsoft Graph contacts and calendars, for Outlook accounts.
    Graph,
}

/// Authorization and token endpoints.
fn endpoints(provider: &Provider) -> (&'static str, &'static str) {
    match provider {
        Provider::Gmail => (
            "https://accounts.google.com/o/oauth2/v2/auth",
            "https://oauth2.googleapis.com/token",
        ),
        Provider::Outlook => (
            "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            "https://login.microsoftonline.com/common/oauth2/v2.0/token",
        ),
        Provider::Imap => unreachable!("IMAP password accounts do not use OAuth"),
    }
}
/// The scopes for the contacts and calendar access in `access`.
fn feature_scopes(provider: &Provider, access: &Integrations) -> Vec<&'static str> {
    let (contacts, calendar): (&[&str], &[&str]) = match provider {
        Provider::Gmail => (&GOOGLE_CONTACTS, &GOOGLE_CALENDAR),
        Provider::Outlook => (&GRAPH_CONTACTS, &GRAPH_CALENDAR),
        Provider::Imap => (&[], &[]),
    };
    let mut scopes = Vec::new();
    if access.contacts {
        scopes.extend_from_slice(contacts);
    }
    if access.calendar {
        scopes.extend_from_slice(calendar);
    }
    scopes
}
/// Everything the sign-in asks for: mail, plus the account's contacts and calendar access. Microsoft
/// consents to several APIs at once but issues each token for one API.
fn authorize_scope(provider: &Provider, access: &Integrations) -> String {
    let mail = if *provider == Provider::Gmail {
        GOOGLE_MAIL
    } else {
        OUTLOOK_MAIL
    };
    std::iter::once(mail)
        .chain(feature_scopes(provider, access))
        .collect::<Vec<_>>()
        .join(" ")
}
/// The access in `requested` that the provider's `granted` scope list includes. People can untick
/// individual permissions on Google's consent screen.
fn granted(provider: &Provider, requested: &Integrations, granted: &str) -> Integrations {
    let has = |feature: Integrations| {
        feature_scopes(provider, &feature).iter().all(|scope| {
            granted
                .split_whitespace()
                .any(|g| g.eq_ignore_ascii_case(scope))
        })
    };
    Integrations {
        contacts: requested.contacts
            && has(Integrations {
                contacts: true,
                calendar: false,
            }),
        calendar: requested.calendar
            && has(Integrations {
                contacts: false,
                calendar: true,
            }),
    }
}
#[derive(Deserialize)]
struct Callback {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}
struct CallbackState {
    expected: String,
    locale: Locale,
    sender: Mutex<Option<oneshot::Sender<Result<String>>>>,
}
async fn callback(
    State(state): State<Arc<CallbackState>>,
    Query(query): Query<Callback>,
) -> (StatusCode, Html<String>) {
    if !query
        .state
        .as_deref()
        .is_some_and(|s| bool::from(s.as_bytes().ct_eq(state.expected.as_bytes())))
    {
        return (
            StatusCode::BAD_REQUEST,
            Html(format!(
                "<h1>{}</h1><p>{}</p>",
                crate::t!(state.locale, "mail.oauth.browser_unverified"),
                crate::t!(state.locale, "mail.oauth.browser_retry")
            )),
        );
    }
    let code = if query.error.is_some() {
        Err(anyhow::anyhow!(crate::text!("mail.oauth.declined")))
    } else {
        query
            .code
            .filter(|s| !s.is_empty())
            .context(crate::text!("mail.oauth.no_code"))
    };
    let ok = code.is_ok();
    if let Some(sender) = state.sender.lock().unwrap().take() {
        let _ = sender.send(code);
    }
    (
        StatusCode::OK,
        Html(if ok {
            format!(
                "<!doctype html><meta name='viewport' content='width=device-width'><style>body{{font:18px system-ui;background:#f7f7f9;color:#323039;padding:12vh 12vw}}h1{{font-size:40px}}</style><h1>{}</h1><p>{}</p>",
                crate::t!(state.locale, "mail.oauth.browser_success"),
                crate::t!(state.locale, "mail.oauth.browser_close")
            )
        } else {
            format!(
                "<h1>{}</h1><p>{}</p>",
                crate::t!(state.locale, "mail.oauth.browser_cancelled"),
                crate::t!(state.locale, "mail.oauth.browser_return")
            )
        }),
    )
}
pub struct Pending {
    pub url: String,
    pub account: Account,
    secret: Secret,
    verifier: String,
    receiver: oneshot::Receiver<Result<String>>,
    server: JoinHandle<()>,
}
/// Starts a browser sign-in for `account`, asking for mail and the contacts and calendar access in
/// `account.integrations`.
pub async fn begin(account: Account, mut secret: Secret, locale: Locale) -> Result<Pending> {
    let client_id = secret
        .client_id
        .as_ref()
        .filter(|s| !s.is_empty())
        .context(crate::text!("mail.oauth.client_required"))?;
    let listener = TcpListener::bind("127.0.0.1:4816")
        .await
        .context(crate::text!("mail.oauth.port_busy"))?;
    let expected = Uuid::new_v4().to_string();
    let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let (authorize, _) = endpoints(&account.provider);
    let scope = authorize_scope(&account.provider, &account.integrations);
    let mut url = reqwest::Url::parse(authorize)?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", client_id.as_str()),
        ("redirect_uri", REDIRECT),
        ("response_type", "code"),
        ("scope", scope.as_str()),
        ("state", &expected),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("login_hint", &account.email),
    ]);
    if account.provider == Provider::Gmail {
        url.query_pairs_mut().extend_pairs([
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("include_granted_scopes", "true"),
        ]);
    }
    let (sender, receiver) = oneshot::channel();
    let router = Router::new()
        .route("/callback", get(callback))
        .with_state(Arc::new(CallbackState {
            expected,
            locale,
            sender: Mutex::new(Some(sender)),
        }));
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    secret.password = None;
    Ok(Pending {
        url: url.into(),
        account,
        secret,
        verifier,
        receiver,
        server,
    })
}
#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    /// The scopes granted, space separated.
    scope: Option<String>,
}
impl Pending {
    /// Waits for the browser sign-in, then stores the credentials. The returned account's
    /// `integrations` are the requested ones the person actually granted.
    pub async fn finish(mut self) -> Result<(Account, Secret)> {
        let outcome = tokio::time::timeout(Duration::from_secs(300), &mut self.receiver).await;
        self.server.abort();
        let code = outcome.context(crate::text!("mail.oauth.timeout"))???;
        let provider = self.account.provider.clone();
        let (_, token_url) = endpoints(&provider);
        let mut params = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code),
            ("redirect_uri", REDIRECT.to_string()),
            ("client_id", self.secret.client_id.clone().unwrap()),
            ("code_verifier", self.verifier.clone()),
        ];
        if provider == Provider::Outlook {
            // The code covers every consented API, but a token is issued for one: mail.
            params.push(("scope", OUTLOOK_MAIL.to_string()));
        }
        if let Some(secret) = &self.secret.client_secret {
            params.push(("client_secret", secret.clone()));
        }
        let response = http_client()?.post(token_url).form(&params).send().await?;
        if !response.status().is_success() {
            bail!(crate::text!("mail.oauth.exchange_failed"));
        }
        let tokens: Tokens = response
            .json()
            .await
            .context(crate::text!("mail.oauth.invalid_credentials"))?;
        let mail_scope = tokens.scope.clone();
        update_secret(&mut self.secret, tokens)?;
        // A new sign-in replaces earlier Graph consent; its token is fetched again when needed.
        self.secret.graph_token = None;
        self.secret.graph_expires_at = None;
        let requested = self.account.integrations.clone();
        self.account.integrations = match provider {
            _ if requested == Integrations::default() => requested,
            Provider::Gmail => granted(&provider, &requested, mail_scope.as_deref().unwrap_or("")),
            // Asking Graph for a token both proves the consent and reports what it covers.
            _ => match refresh_graph(&mut self.secret, &requested).await {
                Ok(scope) => granted(&provider, &requested, &scope),
                Err(_) => Integrations::default(),
            },
        };
        vault::save(&self.account.id, &self.secret).await?;
        Ok((self.account, self.secret))
    }
}
fn update_secret(secret: &mut Secret, tokens: Tokens) -> Result<()> {
    if tokens.access_token.is_empty() {
        bail!(crate::text!("mail.oauth.empty_token"));
    }
    secret.access_token = Some(tokens.access_token);
    if tokens.refresh_token.is_some() {
        secret.refresh_token = tokens.refresh_token;
    }
    secret.expires_at = Some(expiry(tokens.expires_in));
    Ok(())
}
fn expiry(expires_in: Option<i64>) -> i64 {
    chrono::Utc::now().timestamp() + expires_in.unwrap_or(3600).clamp(60, 86400)
}
fn expiring(expires_at: Option<i64>) -> bool {
    expires_at.unwrap_or(0) <= chrono::Utc::now().timestamp() + 60
}
/// Serializes token refreshes: Microsoft may rotate the refresh token on each use, and the vault
/// entry is read, changed and written whole.
static REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// Redeems the refresh token for `scope` (the account's default when `None`).
async fn redeem(provider: &Provider, secret: &Secret, scope: Option<&str>) -> Result<Tokens> {
    let refresh = secret
        .refresh_token
        .clone()
        .context(crate::text!("mail.oauth.sign_in_again"))?;
    let (_, url) = endpoints(provider);
    let mut params = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh),
        (
            "client_id",
            secret
                .client_id
                .clone()
                .context(crate::text!("mail.oauth.client_missing"))?,
        ),
    ];
    if let Some(scope) = scope {
        params.push(("scope", scope.to_string()));
    }
    if let Some(client_secret) = &secret.client_secret {
        params.push(("client_secret", client_secret.clone()));
    }
    let response = http_client()?.post(url).form(&params).send().await?;
    if !response.status().is_success() {
        bail!(crate::text!("mail.oauth.authorization_expired"));
    }
    response
        .json()
        .await
        .context(crate::text!("mail.oauth.invalid_credentials"))
}
/// Fetches a Graph token for `access` into `secret`. Returns the granted scopes.
async fn refresh_graph(secret: &mut Secret, access: &Integrations) -> Result<String> {
    let scopes = feature_scopes(&Provider::Outlook, access);
    if scopes.is_empty() {
        bail!(crate::text!("mail.oauth.allow_integrations"));
    }
    let scope = format!("offline_access {}", scopes.join(" "));
    let tokens = redeem(&Provider::Outlook, secret, Some(&scope))
        .await
        .context(crate::text!("mail.oauth.microsoft_access"))?;
    if tokens.access_token.is_empty() {
        bail!(crate::text!("mail.oauth.empty_token"));
    }
    if tokens.refresh_token.is_some() {
        secret.refresh_token = tokens.refresh_token;
    }
    secret.graph_token = Some(tokens.access_token);
    secret.graph_expires_at = Some(expiry(tokens.expires_in));
    Ok(tokens.scope.unwrap_or(scope))
}
/// The account's stored sign-in, with a current mail access token for OAuth accounts.
pub async fn credentials(account: &Account) -> Result<Secret> {
    let secret = vault::load(&account.id).await?;
    if secret.access_token.is_none() || !expiring(secret.expires_at) {
        return Ok(secret);
    }
    let _refreshing = REFRESH.lock().await;
    // Another request may have refreshed while this one waited.
    let mut secret = vault::load(&account.id).await?;
    if expiring(secret.expires_at) {
        let scope = (account.provider == Provider::Outlook).then_some(OUTLOOK_MAIL);
        let tokens = redeem(&account.provider, &secret, scope).await?;
        update_secret(&mut secret, tokens)?;
        vault::save(&account.id, &secret).await?;
    }
    Ok(secret)
}
/// A bearer token for `api` on behalf of `account`, which must have granted the access it is used
/// for (see `Account::integrations`).
pub async fn api_token(account: &Account, api: Api) -> Result<String> {
    match (api, &account.provider) {
        (Api::Google, Provider::Gmail) => credentials(account)
            .await?
            .access_token
            .context(crate::text!("mail.oauth.google_sign_in")),
        (Api::Graph, Provider::Outlook) => {
            let secret = vault::load(&account.id).await?;
            if let Some(token) = secret
                .graph_token
                .filter(|_| !expiring(secret.graph_expires_at))
            {
                return Ok(token);
            }
            let _refreshing = REFRESH.lock().await;
            let mut secret = vault::load(&account.id).await?;
            if let Some(token) = secret
                .graph_token
                .clone()
                .filter(|_| !expiring(secret.graph_expires_at))
            {
                return Ok(token);
            }
            refresh_graph(&mut secret, &account.integrations).await?;
            vault::save(&account.id, &secret).await?;
            secret
                .graph_token
                .context(crate::text!("mail.oauth.no_graph_token"))
        }
        _ => bail!(crate::text!("mail.oauth.unsupported_service")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn consent_asks_for_mail_plus_each_allowed_feature_and_trusts_only_granted_scopes() {
        let both = Integrations {
            contacts: true,
            calendar: true,
        };
        let google = authorize_scope(&Provider::Gmail, &both);
        assert!(google.starts_with("https://mail.google.com/ "));
        assert!(google.contains("contacts.other.readonly") && google.contains("calendar.events"));
        assert_eq!(
            authorize_scope(&Provider::Gmail, &Integrations::default()),
            GOOGLE_MAIL
        );
        // Google's consent screen lets people untick calendar access.
        let partial = format!("{GOOGLE_MAIL} {}", GOOGLE_CONTACTS.join(" "));
        assert_eq!(
            granted(&Provider::Gmail, &both, &partial),
            Integrations {
                contacts: true,
                calendar: false
            }
        );
        assert_eq!(
            granted(
                &Provider::Outlook,
                &both,
                "https://graph.microsoft.com/Calendars.ReadWrite https://graph.microsoft.com/Contacts.Read"
            ),
            both
        );
        assert!(authorize_scope(&Provider::Outlook, &both).starts_with(OUTLOOK_MAIL));
    }
}
