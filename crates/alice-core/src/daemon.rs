use crate::{
    ai, calendar, client, contacts, dav,
    i18n::{self, Locale},
    model::*,
    oauth, providers,
    remote::Remote,
    store::{SYSTEM_CONTACTS, Store},
    sync::{Mode, SyncEngine},
    vault::{self, Secret},
};
use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, FromRequest, FromRequestParts, Path, Query, Request, State},
    http::{StatusCode, header, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    fs::OpenOptions,
    sync::Arc,
};
use subtle::ConstantTimeEq;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    token: Arc<String>,
    sync: Arc<SyncEngine>,
    flows: Arc<Mutex<HashMap<String, Value>>>,
    shutdown: Arc<tokio::sync::Notify>,
    arrivals: Arc<Mutex<ArrivalLog>>,
    contacts: Arc<contacts::Directory>,
}
impl AppState {
    fn new(store: Arc<Store>, token: String) -> Self {
        let arrivals: Arc<Mutex<ArrivalLog>> = Arc::default();
        let log = arrivals.clone();
        Self {
            sync: SyncEngine::new(
                store.clone(),
                Arc::new(move |previous_sync, added| {
                    log.lock().record(self::arrivals(previous_sync, added))
                }),
            ),
            store,
            token: Arc::new(token),
            flows: Arc::default(),
            shutdown: Arc::default(),
            arrivals,
            contacts: Arc::default(),
        }
    }
    /// Waits for `account_id`'s lock: its cached mail and server state change one operation at a time.
    async fn hold(&self, account_id: &str) -> tokio::sync::OwnedMutexGuard<()> {
        self.sync
            .account(account_id)
            .lock
            .clone()
            .lock_owned()
            .await
    }
}
/// Recent arrivals recorded by this daemon's syncs. Cursors are `{boot}.{sequence}`, so a cursor from
/// an earlier daemon process is recognized.
struct ArrivalLog {
    boot: String,
    sequence: u64,
    recent: VecDeque<(u64, Arrival)>,
}
impl Default for ArrivalLog {
    fn default() -> Self {
        Self {
            boot: Uuid::new_v4().simple().to_string(),
            sequence: 0,
            recent: VecDeque::new(),
        }
    }
}
impl ArrivalLog {
    const KEPT: usize = 200;
    fn record(&mut self, arrivals: Vec<Arrival>) {
        for arrival in arrivals {
            self.sequence += 1;
            self.recent.push_back((self.sequence, arrival));
            if self.recent.len() > Self::KEPT {
                self.recent.pop_front();
            }
        }
    }
    /// Arrivals after `after`. Without a cursor there are none: a client starts watching from now.
    /// A cursor from an earlier daemon gets everything this one recorded, which all came later.
    fn since(&self, after: Option<&str>) -> Arrivals {
        let from = match after.and_then(|cursor| cursor.split_once('.')) {
            Some((boot, sequence)) if boot == self.boot => {
                sequence.parse().unwrap_or(self.sequence)
            }
            Some(_) => 0,
            None => self.sequence,
        };
        Arrivals {
            cursor: format!("{}.{}", self.boot, self.sequence),
            messages: self
                .recent
                .iter()
                .filter(|(n, _)| *n > from)
                .map(|(_, a)| a.clone())
                .collect(),
        }
    }
}
/// How long before the previous sync the server may have received mail that sync did not see: mail
/// that arrived while it ran, and clock differences between the server and this device.
const ARRIVAL_GRACE_MINUTES: i64 = 10;
/// The unread Inbox mail among one sync's newly imported `messages` that the server received since
/// the account's previous sync. An account's first sync brings no arrivals, and neither does mail the
/// server received long before (re-imported after a mailbox reset, or moved back into the Inbox).
/// Imported mail's `date` is the server's receipt time.
fn arrivals(previous_sync: Option<&str>, messages: &[Message]) -> Vec<Arrival> {
    let Some(previous) = previous_sync.and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
    else {
        return Vec::new();
    };
    let since = previous - chrono::Duration::minutes(ARRIVAL_GRACE_MINUTES);
    messages
        .iter()
        .filter(|m| {
            m.folder == "inbox"
                && !m.is_read
                && chrono::DateTime::parse_from_rfc3339(&m.date).is_ok_and(|date| date >= since)
        })
        .map(|m| Arrival {
            id: m.id.clone(),
            account_id: m.account_id.clone(),
            from_name: m.from_name.clone(),
            from_email: m.from_email.clone(),
            subject: m.subject.clone(),
            preview: m.preview.clone(),
        })
        .collect()
}
#[derive(serde::Deserialize)]
struct ArrivalQuery {
    after: Option<String>,
}
async fn arrivals_since(
    State(state): State<AppState>,
    Query(query): Query<ArrivalQuery>,
) -> Json<Arrivals> {
    Json(state.arrivals.lock().since(query.after.as_deref()))
}
async fn pending_otp(State(state): State<AppState>) -> ApiResult<Vec<OtpArrival>> {
    Ok(Json(state.store.pending_otp()?))
}
async fn claim_otp(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<bool> {
    Ok(Json(state.store.claim_otp(&id)?))
}
pub struct ApiError(StatusCode, i18n::Message);
impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        let message = error.to_string();
        let status = if message.contains("not found") {
            StatusCode::NOT_FOUND
        } else if message.contains("already")
            || message.contains("uncertain")
            || message.contains("pending")
        {
            StatusCode::CONFLICT
        } else {
            StatusCode::BAD_REQUEST
        };
        Self(status, i18n::Message::of(&error))
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.0,
            Json(json!({"error":{"code":self.0.as_str(),"message":self.1.render(Locale::EnUs)}})),
        )
            .into_response();
        response.extensions_mut().insert(self.1);
        response
    }
}
fn request_locale(headers: &axum::http::HeaderMap) -> Locale {
    headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .and_then(Locale::from_accept_language)
        .unwrap_or(Locale::EnUs)
}
impl<S: Send + Sync> FromRequestParts<S> for Locale {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        Ok(request_locale(&parts.headers))
    }
}
async fn localize(request: Request, next: Next) -> Response {
    let locale = request_locale(request.headers());
    let mut response = next.run(request).await;
    if let Some(message @ i18n::Message::Text(_)) = response.extensions().get::<i18n::Message>() {
        let status = response.status();
        let body = Json(json!({"error":{"code":status.as_str(),"message":message.render(locale)}}));
        *response.body_mut() = body.into_response().into_body();
        response.headers_mut().remove(header::CONTENT_LENGTH);
    }
    response.headers_mut().insert(
        header::CONTENT_LANGUAGE,
        axum::http::HeaderValue::from_static(locale.tag()),
    );
    response
}
struct JsonBody<T>(T);
impl<S, T> FromRequest<S> for JsonBody<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;
    async fn from_request(
        request: Request,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(input)| Self(input))
            .map_err(|_| {
                ApiError(
                    StatusCode::BAD_REQUEST,
                    crate::text!("server.error.request_body").into(),
                )
            })
    }
}
type ApiResult<T> = std::result::Result<Json<T>, ApiError>;
pub fn router(store: Arc<Store>, token: String) -> Router {
    routes(AppState::new(store, token))
}
fn routes(state: AppState) -> Router {
    Router::new()
        .route(
            "/v1/health",
            get(|| async {
                Json(json!({"status":"ok","version":env!("CARGO_PKG_VERSION"),"build":client::BUILD}))
            }),
        )
        .route("/v1/state", get(snapshot))
        .route("/v1/preferences", get(preferences).patch(preferences_save))
        .route("/v1/updates", get(crate::updates::check))
        .route("/v1/shutdown", post(shutdown))
        .route("/v1/accounts", get(accounts).post(connect))
        .route("/v1/accounts/{id}", delete(remove_account))
        .route("/v1/accounts/{id}/integrations", post(integrations))
        .route("/v1/connections/{id}", get(connection))
        .route("/v1/contacts", get(contacts_suggest))
        .route("/v1/contacts/sync", post(contacts_sync))
        .route(
            "/v1/contacts/system",
            put(system_contacts_save).delete(system_contacts_remove),
        )
        .route("/v1/dav", get(dav_sources).post(dav_add))
        .route("/v1/dav/{id}", delete(dav_remove))
        .route("/v1/messages", get(messages).post(draft))
        .route("/v1/threads", get(threads))
        .route("/v1/threads/{id}", get(thread))
        .route(
            "/v1/messages/{id}",
            get(message).patch(change).delete(discard),
        )
        .route("/v1/messages/{id}/otp", get(message_otp))
        .route("/v1/messages/{id}/send", post(send))
        .route("/v1/messages/{id}/attachments/{index}", get(attachment))
        .route("/v1/messages/{id}/rsvp", post(rsvp))
        .route("/v1/calendar/ics", post(calendar_file))
        .route("/v1/calendars", get(calendar_list))
        .route("/v1/calendar/events", post(calendar_add))
        .route("/v1/calendar/busy", get(calendar_busy))
        .route("/v1/sync", post(sync))
        .route("/v1/storage", get(storage))
        .route("/v1/arrivals", get(arrivals_since))
        .route("/v1/otp", get(pending_otp))
        .route("/v1/otp/{id}/claim", post(claim_otp))
        .route("/v1/ai/settings", get(ai_settings).post(ai_settings_save))
        .route("/v1/ai", post(ask))
        .fallback(|| async {
            ApiError(
                StatusCode::NOT_FOUND,
                crate::text!("server.error.endpoint_not_found").into(),
            )
        })
        .layer(DefaultBodyLimit::max(2_000_000))
        .layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .layer(middleware::from_fn(localize))
        .with_state(state)
}
async fn authenticate(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    if !supplied.is_some_and(|s| bool::from(s.as_bytes().ct_eq(state.token.as_bytes()))) {
        return ApiError(
            StatusCode::UNAUTHORIZED,
            crate::text!("server.error.authentication").into(),
        )
        .into_response();
    }
    let response = next.run(request).await;
    if !response.status().is_success()
        && !response
            .headers()
            .get(header::CONTENT_TYPE)
            .is_some_and(|v| v.to_str().unwrap_or("").starts_with("application/json"))
    {
        return ApiError(
            response.status(),
            crate::text!("server.error.invalid_request").into(),
        )
        .into_response();
    }
    response
}
async fn accounts(State(state): State<AppState>) -> ApiResult<Vec<Account>> {
    Ok(Json(state.store.accounts()?))
}
async fn shutdown(State(state): State<AppState>) -> Json<Value> {
    state.shutdown.notify_one();
    Json(json!({"status":"stopping"}))
}
async fn discard(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let _lock = state.hold(&state.store.message(&id)?.account_id).await;
    state.store.discard_draft(&id)?;
    Ok(Json(json!({"discarded":id})))
}
async fn remove_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
    locale: Locale,
) -> ApiResult<Value> {
    // Holding the account keeps an in-flight sync batch or send from writing it back.
    let lock = state.hold(&id).await;
    let account = state.store.remove_account(&id)?;
    drop(lock);
    state.sync.forget(&id);
    // Local data is already gone. A credential store that refuses (or is waiting on a permission
    // prompt) must not block mail operations or bring the account back, so report it instead.
    let credentials = vault::delete(&account.id).await;
    Ok(Json(json!({
        "removed": account.id,
        "credentials_removed": credentials.is_ok(),
        "message": credentials.err().map(|e| i18n::error_text(&e, locale)),
    })))
}
async fn snapshot(
    State(state): State<AppState>,
    Query(query): Query<MessageQuery>,
) -> ApiResult<Value> {
    // Read first: data written meanwhile then shows up as a newer revision on the next poll.
    let revision = state.store.revision();
    let accounts = state.store.accounts()?;
    // Counted in conversations, like the desktop's Unread tab.
    let inbox = state.store.threads(&MessageQuery {
        folder: Some("inbox".into()),
        limit: Some(1),
        account_id: query.account_id,
        ..Default::default()
    })?;
    Ok(Json(
        json!({"accounts":accounts,"unread_count":inbox.unread_total,"ai":state.store.ai_settings()?,"preferences":state.store.preferences()?,"syncing":state.sync.any_active(),"revision":revision}),
    ))
}
async fn messages(
    State(state): State<AppState>,
    Query(query): Query<MessageQuery>,
) -> ApiResult<MessagePage> {
    Ok(Json(state.store.list(&query)?))
}
async fn message(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Message> {
    Ok(Json(state.store.message(&id)?))
}
/// Explicit reader lookup: does not depend on automatic delivery, read state or mail age.
async fn message_otp(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Option<String>> {
    let message = state.store.message(&id)?;
    Ok(Json(crate::otp::recognize(&message).map(|code| code.code)))
}
async fn threads(
    State(state): State<AppState>,
    Query(query): Query<MessageQuery>,
) -> ApiResult<ThreadPage> {
    Ok(Json(state.store.threads(&query)?))
}
async fn thread(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Vec<Message>> {
    Ok(Json(state.store.conversation(&id)?))
}
async fn change(
    State(state): State<AppState>,
    Path(id): Path<String>,
    JsonBody(input): JsonBody<MessagePatch>,
) -> ApiResult<Message> {
    validate_patch(&input)?;
    let _lock = state.hold(&state.store.message(&id)?.account_id).await;
    let message = state.store.message(&id)?;
    let account = state.store.account(&message.account_id)?;
    if input.folder.is_some() && ["drafts", "sent"].contains(&message.folder.as_str()) {
        return Err(anyhow::anyhow!(crate::text!("server.error.move_sent")).into());
    }
    if message.folder != "drafts" {
        let secret = oauth::credentials(&account).await?;
        let account = account.clone();
        let original = message.clone();
        let patch = input.clone();
        let updated = tokio::task::spawn_blocking(move || {
            providers::modify(&account, &secret, &original, &patch)
        })
        .await
        .map_err(anyhow::Error::from)??;
        state.store.save_message(&updated)?;
        if let (Some(mailbox), Some(validity), Some(uid)) = (
            &message.remote_mailbox,
            message.uid_validity,
            message.remote_uid,
        ) && (
            &updated.remote_mailbox,
            updated.uid_validity,
            updated.remote_uid,
        ) != (
            &message.remote_mailbox,
            message.uid_validity,
            message.remote_uid,
        ) {
            state
                .store
                .record_move(&updated, (mailbox, validity, uid))?;
        }
        if message.folder != updated.folder
            && (message.folder == "spam" || updated.folder == "spam")
        {
            state.contacts.invalidate();
        }
        return Ok(Json(updated));
    }
    Ok(Json(state.store.patch(&id, &input)?))
}
fn validate_patch(input: &MessagePatch) -> Result<()> {
    if input
        .folder
        .as_ref()
        .is_some_and(|f| !["inbox", "archive", "trash", "spam"].contains(&f.as_str()))
    {
        bail!(crate::text!("server.error.choose_folder"));
    }
    if input.folder.is_none() && input.is_read.is_none() && input.is_starred.is_none() {
        bail!(crate::text!("server.error.no_changes"));
    }
    Ok(())
}
async fn draft(
    State(state): State<AppState>,
    JsonBody(input): JsonBody<Compose>,
) -> ApiResult<Message> {
    let _lock = state.hold(&input.account_id).await;
    if input.subject.len() > 998 || input.body.len() > 1_000_000 || input.to.len() > 100 {
        return Err(anyhow::anyhow!(crate::text!("server.error.draft_limits")).into());
    }
    Ok(Json(state.store.save_draft(&input)?))
}
async fn send(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Message> {
    let _lock = state.hold(&state.store.message(&id)?.account_id).await;
    let message = state.store.message(&id)?;
    let account = state.store.account(&message.account_id)?;
    if message.folder == "sent" {
        return Ok(Json(message));
    }
    if message.folder != "drafts" {
        return Err(anyhow::anyhow!(crate::text!("server.error.send_draft")).into());
    }
    let mail = providers::build_mail(&account, &message)?;
    let secret = oauth::credentials(&account).await?;
    if !state.store.claim_delivery(&id)? {
        return Ok(Json(state.store.message(&id)?));
    }
    let outcome =
        tokio::task::spawn_blocking(move || providers::deliver(&account, &secret, &mail)).await;
    let outcome = outcome.map_err(anyhow::Error::from).and_then(|r| r);
    if let Err(error) = outcome {
        state.store.finish_delivery(&id, false)?;
        return Err(error.into());
    }
    state.store.finish_delivery(&id, true)?;
    Ok(Json(state.store.message(&id)?))
}
async fn sync(State(state): State<AppState>, locale: Locale) -> ApiResult<Value> {
    let mut count = 0;
    let mut errors = Vec::new();
    for (account_id, result) in state.sync.sync_all().await? {
        match result {
            Ok(outcome) => count += outcome.changed,
            Err(e) => {
                errors.push(json!({"account_id":account_id,"message":i18n::error_text(&e, locale)}))
            }
        }
    }
    Ok(Json(json!({"synced":count,"errors":errors})))
}
/// The decoded bytes of a message's attachment, fetched from the mail server on request.
async fn attachment(
    State(state): State<AppState>,
    Path((id, index)): Path<(String, usize)>,
) -> Result<Response, ApiError> {
    let message = state.store.message(&id)?;
    let attachment = message
        .attachments
        .get(index)
        .cloned()
        .context(crate::text!("server.error.attachment_not_found"))?;
    let account = state.store.account(&message.account_id)?;
    let (Some(mailbox), Some(validity), Some(uid)) = (
        message.remote_mailbox.clone(),
        message.uid_validity,
        message.remote_uid,
    ) else {
        return Err(anyhow::anyhow!(crate::text!("server.error.message_not_remote")).into());
    };
    let secret = oauth::credentials(&account).await?;
    let known = message.attachments;
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        let mut remote = Remote::connect(&account, &secret)?;
        let result = remote.attachment(&mailbox, validity, uid, index, &known);
        remote.logout();
        result
    })
    .await
    .map_err(anyhow::Error::from)??;
    let encoded = percent_encode(&attachment.name);
    let fallback: String = attachment
        .name
        .chars()
        .map(|c| {
            if (c.is_ascii_graphic() || c == ' ') && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}"),
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            (header::HeaderName::from_static("x-alice-filename"), encoded),
        ],
        bytes,
    )
        .into_response())
}
/// RFC 5987 encoding: everything but unreserved characters as UTF-8 percent escapes.
fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
/// Answers the invitation in message `id`: mails the organizer an iTIP `REPLY` through the account's
/// SMTP server, then records the answer on the message.
async fn rsvp(
    State(state): State<AppState>,
    Path(id): Path<String>,
    locale: Locale,
    JsonBody(input): JsonBody<RsvpRequest>,
) -> ApiResult<Message> {
    let _lock = state.hold(&state.store.message(&id)?.account_id).await;
    let message = state.store.message(&id)?;
    let account = state.store.account(&message.account_id)?;
    let invite = message
        .invite
        .as_ref()
        .context(crate::text!("server.error.no_invitation"))?;
    if invite.cancelled {
        return Err(anyhow::anyhow!(crate::text!("server.error.event_cancelled")).into());
    }
    if invite.method != "REQUEST" {
        return Err(anyhow::anyhow!(crate::text!("server.error.answer_invitation")).into());
    }
    if !invite.attendees.is_empty()
        && !invite
            .attendees
            .iter()
            .any(|a| a.email.eq_ignore_ascii_case(account.email.trim()))
    {
        return Err(anyhow::anyhow!(crate::text!(
            "server.error.invitation_recipient",
            email = &account.email
        ))
        .into());
    }
    if input.comment.as_ref().is_some_and(|c| c.len() > 2_000) {
        return Err(anyhow::anyhow!(crate::text!("server.error.note_limits")).into());
    }
    let mail = providers::rsvp_mail(
        &account,
        &message,
        invite,
        input.response,
        input.comment.as_deref(),
        locale,
    )?;
    let secret = oauth::credentials(&account).await?;
    tokio::task::spawn_blocking(move || providers::deliver(&account, &secret, &mail))
        .await
        .map_err(anyhow::Error::from)??;
    Ok(Json(state.store.record_rsvp(&id, input.response)?))
}
async fn calendar_file(
    State(state): State<AppState>,
    JsonBody(input): JsonBody<EventFileRequest>,
) -> ApiResult<EventFile> {
    Ok(Json(calendar::event_file(&state.store, input)?))
}
async fn calendar_list(State(state): State<AppState>, locale: Locale) -> ApiResult<CalendarList> {
    Ok(Json(calendar::calendars(&state.store, locale).await?))
}
async fn calendar_add(
    State(state): State<AppState>,
    JsonBody(input): JsonBody<AddEvent>,
) -> ApiResult<CreatedEvent> {
    Ok(Json(calendar::add_event(&state.store, input).await?))
}
async fn calendar_busy(
    State(state): State<AppState>,
    Query(query): Query<BusyQuery>,
    locale: Locale,
) -> ApiResult<BusyReport> {
    Ok(Json(calendar::busy(&state.store, &query, locale).await?))
}
/// How much space each account's mail takes on this device, and how much of its sync window is here.
async fn storage(State(state): State<AppState>) -> ApiResult<StorageReport> {
    let report = tokio::task::spawn_blocking(move || -> Result<StorageReport> {
        let (database_bytes, folders) = state.store.storage()?;
        let order = |folder: &str| {
            ["inbox", "sent", "drafts", "archive", "spam", "trash"]
                .iter()
                .position(|f| *f == folder)
                .unwrap_or(6)
        };
        let mut accounts = Vec::new();
        for account in state.store.accounts()? {
            let mut own: Vec<FolderStorage> = folders
                .iter()
                .filter(|(id, _)| *id == account.id)
                .map(|(_, usage)| usage.clone())
                .collect();
            own.sort_by_key(|f| order(&f.folder));
            let (downloaded, total) = state.store.sync_totals(&account.id)?;
            accounts.push(AccountStorage {
                messages: own.iter().map(|f| f.messages).sum(),
                bytes: own.iter().map(|f| f.bytes).sum(),
                folders: own,
                sync: SyncStatus {
                    active: state.sync.is_active(&account.id),
                    downloaded,
                    total,
                    last_synced: account.last_synced,
                    error: account.sync_error,
                },
                account_id: account.id,
            });
        }
        Ok(StorageReport {
            database_bytes,
            accounts,
        })
    })
    .await
    .map_err(anyhow::Error::from)??;
    Ok(Json(report))
}
async fn connect(
    State(state): State<AppState>,
    locale: Locale,
    JsonBody(input): JsonBody<ConnectAccount>,
) -> ApiResult<Value> {
    input
        .email
        .parse::<lettre::Address>()
        .context(crate::text!("server.error.account_email"))?;
    if input.name.trim().is_empty() || input.name.len() > 200 {
        return Err(anyhow::anyhow!(crate::text!("server.error.account_name")).into());
    }
    let existing = state
        .store
        .accounts()?
        .into_iter()
        .find(|a| a.email.eq_ignore_ascii_case(&input.email));
    if existing
        .as_ref()
        .is_some_and(|a| a.provider != input.provider)
    {
        return Err(anyhow::anyhow!(crate::text!("server.error.email_connected")).into());
    }
    let (imap, smtp) = match input.provider {
        Provider::Gmail => ("imap.gmail.com", "smtp.gmail.com"),
        Provider::Outlook => ("outlook.office365.com", "smtp.office365.com"),
        Provider::Imap => (
            input.imap_host.as_deref().unwrap_or(""),
            input.smtp_host.as_deref().unwrap_or(""),
        ),
    };
    for host in [imap, smtp] {
        if host.is_empty()
            || host.len() > 253
            || !host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            return Err(anyhow::anyhow!(crate::text!("server.error.mail_hosts")).into());
        }
    }
    let account = Account {
        id: existing
            .as_ref()
            .map(|a| a.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        name: input.name.trim().into(),
        email: input.email.trim().into(),
        provider: input.provider.clone(),
        imap_host: imap.into(),
        imap_port: input.imap_port.unwrap_or(993),
        smtp_host: smtp.into(),
        smtp_port: input.smtp_port.unwrap_or(587),
        folders: existing
            .as_ref()
            .map(|a| a.folders.clone())
            .unwrap_or_default(),
        last_synced: None,
        sync_error: None,
        // Reconnecting asks for the same access again; see `oauth::begin`.
        integrations: existing.map(|a| a.integrations).unwrap_or_default(),
    };
    if account.imap_port == 0 || account.smtp_port == 0 {
        return Err(anyhow::anyhow!(crate::text!("server.error.mail_ports")).into());
    }
    let secret = Secret {
        password: input.password,
        client_id: input.client_id.or_else(|| {
            std::env::var(if input.provider == Provider::Gmail {
                "ALICE_GOOGLE_CLIENT_ID"
            } else {
                "ALICE_MICROSOFT_CLIENT_ID"
            })
            .ok()
        }),
        client_secret: input.client_secret.or_else(|| {
            if input.provider == Provider::Gmail {
                std::env::var("ALICE_GOOGLE_CLIENT_SECRET").ok()
            } else {
                None
            }
        }),
        ..Default::default()
    };
    if input.provider == Provider::Imap
        || (input.provider == Provider::Gmail
            && secret.password.as_ref().is_some_and(|p| !p.is_empty()))
    {
        if secret.password.as_ref().is_none_or(|p| p.is_empty()) {
            return Err(anyhow::anyhow!(crate::text!("server.error.app_password")).into());
        }
        let copy = account.clone();
        let credential = secret.clone();
        // Signing in and finding the account's folders proves the settings before anything is saved.
        let (folders, _) = tokio::task::spawn_blocking(move || -> Result<_> {
            let mut remote = Remote::connect(&copy, &credential)?;
            let folders = remote.folders();
            remote.logout();
            folders
        })
        .await
        .map_err(anyhow::Error::from)??;
        {
            // No sync of this account runs while its sign-in, server and folders change, so none
            // can bring mail from the previous server into the new configuration.
            let _paused = state.sync.pause(&account.id).await;
            let _lock = state.hold(&account.id).await;
            vault::save(&account.id, &secret).await?;
            let mut account = account.clone();
            account.folders = folders;
            // Mail cached from a different server has UIDs that mean nothing to the new one.
            if let Ok(previous) = state.store.account(&account.id)
                && (previous.imap_host != account.imap_host
                    || previous.imap_port != account.imap_port
                    || !previous.email.eq_ignore_ascii_case(&account.email))
            {
                state.store.forget_other_mailboxes(&account.id, &[])?;
            }
            state.store.save_account(&account)?;
        }
        // The newest mail is here when connecting returns; older mail follows in the background.
        let _ = state.sync.sync_account(&account.id, Mode::CatchUp).await;
        state.sync.wake();
        return Ok(Json(json!({"account":state.store.account(&account.id)?})));
    }
    let pending = oauth::begin(account, secret, locale).await?;
    let url = pending.url.clone();
    let flow_state = state.clone();
    let flow_id = track_flow(&flow_state, async move {
        let (account, _) = pending.finish().await?;
        let id = account.id.clone();
        state.store.save_account(&account)?;
        state.sync.sync_account(&id, Mode::CatchUp).await?;
        state.sync.wake();
        Ok(json!({"account_id":id}))
    });
    Ok(Json(json!({"flow_id":flow_id,"url":url})))
}
/// Records a browser sign-in as pending and finishes it in the background. Returns the flow ID that
/// `/v1/connections/{id}` reports on: `{status: "connected", …finished}` or `{status: "error", message}`.
fn track_flow(
    state: &AppState,
    finish: impl Future<Output = Result<Value>> + Send + 'static,
) -> String {
    let id = Uuid::new_v4().to_string();
    {
        let mut flows = state.flows.lock();
        // Only the newest finished sign-ins are still being polled; drop older results.
        flows.retain(|_, flow| flow["status"] == "pending");
        flows.insert(id.clone(), json!({"status":"pending"}));
    }
    let flows = state.flows.clone();
    let flow_id = id.clone();
    tokio::spawn(async move {
        let outcome = match finish.await {
            Ok(mut done) => {
                done["status"] = json!("connected");
                done
            }
            Err(e) => json!({"status":"error","message":e.to_string()}),
        };
        flows.lock().insert(flow_id, outcome);
    });
    id
}
async fn connection(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    Ok(Json(state.flows.lock().get(&id).cloned().context(
        crate::text!("server.error.connection_not_found"),
    )?))
}
/// Allows or stops an OAuth account's contacts and calendar access. Allowing access the account
/// does not have yet opens a browser sign-in, as connecting does: `{flow_id, url}`. Otherwise the
/// change is immediate: `{account}`.
async fn integrations(
    State(state): State<AppState>,
    Path(id): Path<String>,
    locale: Locale,
    JsonBody(input): JsonBody<IntegrationsPatch>,
) -> ApiResult<Value> {
    let account = state.store.account(&id)?;
    let wanted = Integrations {
        contacts: input.contacts.unwrap_or(account.integrations.contacts),
        calendar: input.calendar.unwrap_or(account.integrations.calendar),
    };
    let adds = (wanted.contacts && !account.integrations.contacts)
        || (wanted.calendar && !account.integrations.calendar);
    if !adds {
        let account = save_integrations(&state, &id, wanted).await?;
        return Ok(Json(json!({"account":account})));
    }
    if account.provider == Provider::Imap {
        return Err(anyhow::anyhow!(crate::text!("server.error.integration_provider")).into());
    }
    let secret = vault::load(&id).await?;
    if secret.refresh_token.is_none() {
        return Err(anyhow::anyhow!(crate::text!("server.error.integration_password")).into());
    }
    let mut requested = account;
    requested.integrations = wanted.clone();
    let pending = oauth::begin(requested, secret, locale).await?;
    let url = pending.url.clone();
    let flow_state = state.clone();
    let flow_id = track_flow(&state, async move {
        let (granted, _) = pending.finish().await?;
        let account = save_integrations(&flow_state, &id, granted.integrations.clone()).await?;
        if (wanted.contacts && !granted.integrations.contacts)
            || (wanted.calendar && !granted.integrations.calendar)
        {
            bail!(crate::text!("server.error.integration_permissions"));
        }
        Ok(json!({"account_id":id,"account":account}))
    });
    Ok(Json(json!({"flow_id":flow_id,"url":url})))
}
/// Stores an account's contacts and calendar access, reading the account under its lock so a sync
/// finishing meanwhile is not overwritten. Stopping contacts access forgets the account's address
/// book; allowing it reads the address book in the background.
async fn save_integrations(state: &AppState, id: &str, access: Integrations) -> Result<Account> {
    let _lock = state.hold(id).await;
    let mut account = state.store.account(id)?;
    let shared = account.integrations.contacts;
    account.integrations = access;
    state.store.save_account(&account)?;
    if shared && !account.integrations.contacts {
        state.store.remove_contacts(id)?;
        state.contacts.invalidate();
    } else if !shared && account.integrations.contacts {
        let (store, directory, account) =
            (state.store.clone(), state.contacts.clone(), account.clone());
        // A failure shows in `POST /v1/contacts/sync`; the scheduled refresh tries again.
        tokio::spawn(async move {
            if contacts::refresh_account(&store, &account).await.is_ok() {
                directory.invalidate();
            }
        });
    }
    Ok(account)
}
/// Recipient suggestions, best first; see [`contacts::Directory::suggest`].
async fn contacts_suggest(
    State(state): State<AppState>,
    Query(query): Query<ContactQuery>,
) -> ApiResult<Vec<Contact>> {
    // Rebuilding the index walks every stored message.
    let found = tokio::task::spawn_blocking(move || state.contacts.suggest(&state.store, &query))
        .await
        .context(crate::text!("server.error.contact_suggestions"))??;
    Ok(Json(found))
}
/// Reads the Google, Microsoft and CardDAV address books again now.
async fn contacts_sync(State(state): State<AppState>, locale: Locale) -> ApiResult<ContactsSync> {
    let synced = contacts::refresh_all(&state.store, locale).await?;
    state.contacts.invalidate();
    Ok(Json(synced))
}
/// Replaces the operating system address book's contacts, which the desktop app reads and pushes.
async fn system_contacts_save(
    State(state): State<AppState>,
    JsonBody(input): JsonBody<ContactImport>,
) -> ApiResult<Value> {
    let stored = state.store.replace_contacts(
        ContactSource::System,
        SYSTEM_CONTACTS,
        &contacts::tidy(input.contacts),
    )?;
    state.contacts.invalidate();
    Ok(Json(json!({"contacts":stored})))
}
async fn system_contacts_remove(State(state): State<AppState>) -> ApiResult<Value> {
    let removed = state.store.remove_contacts(SYSTEM_CONTACTS)?;
    state.contacts.invalidate();
    Ok(Json(json!({"removed":removed})))
}
async fn dav_sources(State(state): State<AppState>) -> ApiResult<Vec<DavSource>> {
    Ok(Json(state.store.dav_sources()?))
}
/// Adds a CardDAV or CalDAV server once signing in found its address books or calendars. A CardDAV
/// server's contacts are imported right away; failing to read them is recorded on the source
/// rather than refusing it.
async fn dav_add(
    State(state): State<AppState>,
    JsonBody(input): JsonBody<AddDavSource>,
) -> ApiResult<DavSource> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() >= 200 {
        return Err(anyhow::anyhow!(crate::text!("server.error.server_name")).into());
    }
    let url = dav::server_url(&input.url)?;
    let username = input.username.trim();
    if username.is_empty() || input.password.is_empty() {
        return Err(anyhow::anyhow!(crate::text!("server.error.server_credentials")).into());
    }
    let session = dav::Session::new(username, &input.password)?;
    let collections = session.discover(input.kind, &url).await?;
    let source = DavSource {
        id: Uuid::new_v4().to_string(),
        kind: input.kind,
        name: name.into(),
        url: url.to_string(),
        username: username.into(),
        last_synced: None,
        error: None,
    };
    vault::save(
        &format!("dav:{}", source.id),
        &Secret {
            password: Some(input.password),
            ..Default::default()
        },
    )
    .await?;
    state.store.save_dav_source(&source)?;
    if source.kind == DavKind::Carddav {
        // The outcome, contacts or error, is recorded on the source returned below.
        let _ = contacts::store_carddav(
            &state.store,
            &source,
            contacts::read_carddav(&session, &collections).await,
        );
        state.contacts.invalidate();
    }
    Ok(Json(state.store.dav_source(&source.id)?))
}
/// Forgets a DAV server, its contacts and its stored password.
async fn dav_remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
    locale: Locale,
) -> ApiResult<Value> {
    let source = state.store.remove_dav_source(&id)?;
    state.store.remove_contacts(&source.id)?;
    state.contacts.invalidate();
    // As with accounts, a credential store that refuses must not bring the server back.
    let credentials = vault::delete(&format!("dav:{}", source.id)).await;
    Ok(Json(json!({
        "removed": source.id,
        "credentials_removed": credentials.is_ok(),
        "message": credentials.err().map(|e| i18n::error_text(&e, locale)),
    })))
}
async fn ai_settings(State(state): State<AppState>) -> ApiResult<AiSettings> {
    Ok(Json(state.store.ai_settings()?))
}
async fn preferences(State(state): State<AppState>) -> ApiResult<Preferences> {
    Ok(Json(state.store.preferences()?))
}
async fn preferences_save(
    State(state): State<AppState>,
    JsonBody(input): JsonBody<PreferencesPatch>,
) -> ApiResult<Preferences> {
    let before = state.store.preferences()?;
    let saved = state.store.save_preferences(input)?;
    if saved.sync_window != before.sync_window || saved.otp_autocopy != before.otp_autocopy {
        state.sync.wake();
    }
    Ok(Json(saved))
}
async fn ai_settings_save(
    State(state): State<AppState>,
    JsonBody(input): JsonBody<SaveAiSettings>,
) -> ApiResult<AiSettings> {
    Ok(Json(ai::save_settings(&state.store, input).await?))
}
async fn ask(
    State(state): State<AppState>,
    locale: Locale,
    JsonBody(input): JsonBody<AiRequest>,
) -> ApiResult<AiResult> {
    Ok(Json(ai::run(&state.store, input, locale).await?))
}

pub async fn run() -> Result<()> {
    let root = client::data_dir()?;
    std::fs::create_dir_all(&root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("daemon.lock"))?;
    if lock.try_lock().is_err() {
        bail!(crate::text!("server.error.daemon_running"));
    }
    let port = client::port()?;
    // A string, not a number: numbers render with digit grouping, and a port reads "4815", not "4,815".
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .with_context(|| crate::text!("server.error.port_in_use", port = port.to_string()))?;
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let token_path = root.join("daemon.token");
    std::fs::write(&token_path, &token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600))?;
    }
    let state = AppState::new(Arc::new(Store::open(&root.join("mail.sqlite3"))?), token);
    tokio::spawn(state.sync.clone().run_background());
    tokio::spawn(contacts::run_background(state.store.clone()));
    println!("Alice daemon listening on 127.0.0.1:{port}");
    let shutdown = state.shutdown.clone();
    axum::serve(listener, routes(state))
        .with_graceful_shutdown(async move {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = shutdown.notified() => {} }
        })
        .await?;
    drop(lock);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::FIXTURE_ACCOUNT_ID;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use std::time::Duration;
    use tower::ServiceExt;
    fn fixture_store(path: &std::path::Path) -> Result<Arc<Store>> {
        let store = Store::open(path)?;
        store.add_fixture()?;
        Ok(Arc::new(store))
    }
    #[tokio::test]
    async fn localized_errors_keep_their_status_and_protocol_codes() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = fixture_store(&dir.path().join("mail.db"))?;
        let draft = Compose {
            id: Uuid::new_v4().to_string(),
            account_id: FIXTURE_ACCOUNT_ID.into(),
            to: vec!["support@example.com".into()],
            subject: String::new(),
            body: String::new(),
            in_reply_to: None,
        };
        store.save_draft(&draft)?;
        store.claim_delivery(&draft.id)?;
        let app = router(store, "test-token".into());
        for (method, path, body, authenticated, status, english, german) in [
            (
                "GET",
                "/v1/accounts".to_string(),
                "",
                false,
                StatusCode::UNAUTHORIZED,
                "Authentication required",
                "Authentifizierung erforderlich",
            ),
            (
                "GET",
                "/v1/missing".to_string(),
                "",
                true,
                StatusCode::NOT_FOUND,
                "Endpoint not found",
                "Endpunkt nicht gefunden",
            ),
            (
                "GET",
                "/v1/messages/missing".to_string(),
                "",
                true,
                StatusCode::NOT_FOUND,
                "Message not found",
                "Nachricht nicht gefunden",
            ),
            (
                "POST",
                "/v1/messages".to_string(),
                "{",
                true,
                StatusCode::BAD_REQUEST,
                "Invalid request body",
                "Ungültiger Anfrageinhalt",
            ),
            (
                "PUT",
                "/v1/messages".to_string(),
                "",
                true,
                StatusCode::METHOD_NOT_ALLOWED,
                "Invalid request",
                "Ungültige Anfrage",
            ),
            (
                "DELETE",
                format!("/v1/messages/{}", draft.id),
                "",
                true,
                StatusCode::CONFLICT,
                "Delivery is pending or uncertain. Check your provider before discarding this draft.",
                "Die Zustellung steht noch aus oder ihr Ergebnis ist ungewiss. Prüfen Sie dies bei Ihrem Anbieter, bevor Sie diesen Entwurf verwerfen.",
            ),
        ] {
            for (language, tag, expected) in [
                (None, "en-US", english),
                (Some("fr-FR"), "en-US", english),
                (Some("en;q=0.4, de-DE;q=0.9"), "de", german),
            ] {
                let mut request = Request::builder()
                    .method(method)
                    .uri(&path)
                    .header("Content-Type", "application/json");
                if authenticated {
                    request = request.header("Authorization", "Bearer test-token");
                }
                if let Some(language) = language {
                    request = request.header("Accept-Language", language);
                }
                let response = app.clone().oneshot(request.body(Body::from(body))?).await?;
                assert_eq!(response.status(), status);
                assert_eq!(response.headers()["Content-Language"], tag);
                let value: Value =
                    serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await?)?;
                assert_eq!(value["error"]["code"], status.as_str());
                assert_eq!(value["error"]["message"], expected);
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn language_preference_is_backward_compatible_and_persists() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("mail.db");
        let store = fixture_store(&path)?;
        // Preferences written before language selection existed remain readable.
        let mut legacy = serde_json::to_value(Preferences::default())?;
        legacy.as_object_mut().unwrap().remove("language");
        store.db.lock().execute(
            "INSERT INTO settings VALUES('preferences',?1)",
            [serde_json::to_string(&legacy)?],
        )?;
        let app = router(store.clone(), "test-token".into());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/preferences")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())?,
            )
            .await?;
        let value: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await?)?;
        assert_eq!(value["language"], "system");
        for (language, status) in [("de", StatusCode::OK), ("fr", StatusCode::BAD_REQUEST)] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PATCH")
                        .uri("/v1/preferences")
                        .header("Authorization", "Bearer test-token")
                        .header("Content-Type", "application/json")
                        .body(Body::from(format!(r#"{{"language":"{language}"}}"#)))?,
                )
                .await?;
            assert_eq!(response.status(), status);
        }
        assert_eq!(
            Store::open(&path)?.preferences()?.language,
            i18n::Language(Some(Locale::De))
        );
        Ok(())
    }
    #[tokio::test]
    async fn delivery_prevents_concurrent_edit_and_discard() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = fixture_store(&dir.path().join("mail.db"))?;
        let draft = Compose {
            id: Uuid::new_v4().to_string(),
            account_id: FIXTURE_ACCOUNT_ID.into(),
            to: vec!["support@example.com".into()],
            subject: "Original".into(),
            body: "Original body".into(),
            in_reply_to: None,
        };
        store.save_draft(&draft)?;
        let state = AppState::new(store.clone(), "test-token".into());
        let app = routes(state.clone());
        let delivery = state.hold(FIXTURE_ACCOUNT_ID).await;
        let edit_request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("Authorization", "Bearer test-token")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&Compose {
                body: "Changed".into(),
                ..draft.clone()
            })?))?;
        let discard_request = Request::builder()
            .method("DELETE")
            .uri(format!("/v1/messages/{}", draft.id))
            .header("Authorization", "Bearer test-token")
            .body(Body::empty())?;
        let mut edit = tokio::spawn(app.clone().oneshot(edit_request));
        let mut discard = tokio::spawn(app.oneshot(discard_request));
        assert!(
            tokio::time::timeout(Duration::from_millis(30), &mut edit)
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(30), &mut discard)
                .await
                .is_err()
        );
        store.claim_delivery(&draft.id)?;
        store.finish_delivery(&draft.id, true)?;
        drop(delivery);
        assert!(!edit.await??.status().is_success());
        assert!(!discard.await??.status().is_success());
        let sent = store.message(&draft.id)?;
        assert_eq!(sent.body, "Original body");
        assert_eq!(sent.folder, "sent");
        Ok(())
    }
    #[tokio::test]
    async fn real_sent_flags_require_provider_credentials() -> Result<()> {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        let dir = tempfile::tempdir()?;
        let store = fixture_store(&dir.path().join("mail.db"))?;
        let mut account = store.account(FIXTURE_ACCOUNT_ID)?;
        account.id = Uuid::new_v4().to_string();
        store.save_account(&account)?;
        let mut mail = store.message("sample-1")?;
        mail.account_id = account.id;
        mail.folder = "sent".into();
        mail.is_starred = false;
        mail.remote_mailbox = Some("Sent".into());
        mail.remote_uid = Some(1);
        mail.uid_validity = Some(1);
        store.save_message(&mail)?;
        let response = router(store.clone(), "test-token".into())
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/v1/messages/sample-1")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"is_starred":true}"#))?,
            )
            .await?;
        assert!(!response.status().is_success());
        assert!(!store.message("sample-1")?.is_starred);
        Ok(())
    }
    #[tokio::test]
    async fn authenticated_clients_share_the_same_inbox() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = fixture_store(&dir.path().join("mail.db"))?;
        let app = router(store.clone(), "test-token".into());
        let unauth = app
            .clone()
            .oneshot(Request::builder().uri("/v1/messages").body(Body::empty())?)
            .await?;
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);
        let mut mail = store.message("sample-2")?;
        mail.folder = "archive".into();
        store.save_message(&mail)?;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages?folder=archive")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())?,
            )
            .await?;
        let page: MessagePage =
            serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await?)?;
        assert_eq!(page.messages.len(), 1);
        assert_eq!(page.messages[0].id, "sample-2");
        Ok(())
    }

    #[tokio::test]
    async fn unread_count_counts_inbox_conversations_beyond_the_first_page() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = fixture_store(&dir.path().join("mail.db"))?;
        let inbox = MessageQuery {
            folder: Some("inbox".into()),
            ..Default::default()
        };
        let before = store.threads(&inbox)?.unread_total;
        let mut mail = store.message("sample-1")?;
        mail.is_read = false;
        // 120 new conversations of two unread messages each: the badge counts conversations, as the
        // Unread tab does, not messages.
        for i in 0..120 {
            mail.subject = format!("Unread conversation {i}");
            mail.id = format!("unread-{i}");
            store.save_message(&mail)?;
            mail.subject = format!("Re: Unread conversation {i}");
            mail.id = format!("unread-{i}-reply");
            store.save_message(&mail)?;
        }
        let response = router(store, "test-token".into())
            .oneshot(
                Request::builder()
                    .uri("/v1/state")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())?,
            )
            .await?;
        let snapshot: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await?)?;
        assert_eq!(snapshot["unread_count"], before + 120);
        Ok(())
    }
    #[tokio::test]
    async fn otp_api_is_cursorless_ordered_and_claims_without_a_request_body() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = fixture_store(&dir.path().join("mail.db"))?;
        let now = chrono::Utc::now();
        let mut account = store.account(FIXTURE_ACCOUNT_ID)?;
        account.last_synced = Some((now - chrono::Duration::minutes(1)).to_rfc3339());
        store.save_account(&account)?;
        store.save_preferences(PreferencesPatch {
            otp_autocopy: Some(true),
            ..Default::default()
        })?;
        for (uid, code, seconds) in [(900, "001234", 10), (901, "009876", 5)] {
            let raw = format!(
                "From: Security <security@example.test>\r\nSubject: Sign-in code\r\nMessage-ID: <otp-{uid}@example.test>\r\n\r\nYour sign-in code is {code}"
            );
            let message =
                providers::parse_message(&account, "inbox", "INBOX", 1, uid, raw.as_bytes())?;
            store.import_copies(
                &account.id,
                "inbox",
                "INBOX",
                1,
                Some(900),
                vec![crate::sync_store::Incoming {
                    uid,
                    message_id: message.message_id.clone(),
                    is_read: true,
                    is_starred: false,
                    date: Some((now - chrono::Duration::seconds(seconds)).to_rfc3339()),
                    message: Some(message),
                }],
            )?;
        }
        let app = router(store.clone(), "test-token".into());
        let request = |method: &str, uri: &str| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("Authorization", "Bearer test-token")
                .body(Body::empty())
        };
        let denied = app
            .clone()
            .oneshot(Request::builder().uri("/v1/otp").body(Body::empty())?)
            .await?;
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        let response = app.clone().oneshot(request("GET", "/v1/otp")?).await?;
        assert_eq!(response.status(), StatusCode::OK);
        let pending: Vec<OtpArrival> =
            serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await?)?;
        assert_eq!(
            pending
                .iter()
                .map(|event| event.code.as_str())
                .collect::<Vec<_>>(),
            ["009876", "001234"]
        );
        let uri = format!("/v1/otp/{}/claim", pending[0].id);
        for expected in [true, false] {
            let response = app.clone().oneshot(request("POST", &uri)?).await?;
            assert_eq!(response.status(), StatusCode::OK);
            let claimed: bool =
                serde_json::from_slice(&to_bytes(response.into_body(), 100).await?)?;
            assert_eq!(claimed, expected);
        }
        let response = app.clone().oneshot(request("GET", "/v1/otp")?).await?;
        let remaining: Vec<OtpArrival> =
            serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await?)?;
        assert_eq!(remaining, [pending[1].clone()]);
        store.save_preferences(PreferencesPatch {
            otp_autocopy: Some(false),
            ..Default::default()
        })?;
        let uri = format!("/v1/messages/{}/otp", pending[0].message_id);
        let denied = app
            .clone()
            .oneshot(Request::builder().uri(&uri).body(Body::empty())?)
            .await?;
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        let response = app.clone().oneshot(request("GET", &uri)?).await?;
        let code: Option<String> =
            serde_json::from_slice(&to_bytes(response.into_body(), 100).await?)?;
        assert_eq!(code.as_deref(), Some("009876"));
        let response = app
            .oneshot(request("GET", "/v1/messages/sample-1/otp")?)
            .await?;
        let code: Option<String> =
            serde_json::from_slice(&to_bytes(response.into_body(), 100).await?)?;
        assert_eq!(code, None);
        Ok(())
    }
    #[test]
    fn arrivals_are_unread_inbox_mail_received_since_the_previous_sync() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let template = fixture_store(&dir.path().join("mail.db"))?.message("sample-1")?;
        let mail = |id: &str, folder: &str, is_read: bool, date: &str| Message {
            id: id.into(),
            folder: folder.into(),
            is_read,
            date: date.into(),
            ..template.clone()
        };
        let previous = "2026-10-11T12:00:00+00:00";
        let imported = [
            mail("new", "inbox", false, "2026-10-11T12:00:30+00:00"),
            // Received while the previous sync was running, or with the server's clock slightly behind.
            mail("during-sync", "inbox", false, "2026-10-11T11:55:00+00:00"),
            mail("read-elsewhere", "inbox", true, "2026-10-11T12:00:30+00:00"),
            mail("sent", "sent", false, "2026-10-11T12:00:30+00:00"),
            mail("archived", "archive", false, "2026-10-11T12:00:30+00:00"),
            // Old mail imported again, e.g. after the mailbox's UIDVALIDITY changed.
            mail("reimported", "inbox", false, "2026-10-10T08:00:00+00:00"),
        ];
        let ids: Vec<String> = arrivals(Some(previous), &imported)
            .into_iter()
            .map(|a| a.id)
            .collect();
        assert_eq!(ids, ["new", "during-sync"]);
        assert!(
            arrivals(None, &imported).is_empty(),
            "an account's first sync is not new mail"
        );
        Ok(())
    }
    #[test]
    fn arrival_cursors_resume_where_the_client_left_off() {
        let arrival = |id: &str| Arrival {
            id: id.into(),
            account_id: "a".into(),
            from_name: "Sophie".into(),
            from_email: "sophie@example.com".into(),
            subject: "Hello".into(),
            preview: "".into(),
        };
        let mut log = ArrivalLog::default();
        log.record(vec![arrival("before-watching")]);
        let start = log.since(None);
        assert!(start.messages.is_empty(), "a new client starts from now");
        log.record(vec![arrival("one"), arrival("two")]);
        let next = log.since(Some(&start.cursor));
        assert_eq!(next.messages, [arrival("one"), arrival("two")]);
        assert!(log.since(Some(&next.cursor)).messages.is_empty());
        // A cursor from an earlier daemon process: everything this daemon recorded came after it.
        assert_eq!(log.since(Some("earlier.7")).messages.len(), 3);
    }
    #[tokio::test]
    async fn suggests_fixture_correspondents_and_imported_system_contacts() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let app = router(
            fixture_store(&dir.path().join("mail.db"))?,
            "test-token".into(),
        );
        let call = |method: &str, uri: &str, body: &str| {
            let request = Request::builder()
                .method(method)
                .uri(uri)
                .header("Authorization", "Bearer test-token")
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()));
            let app = app.clone();
            async move {
                let response = app.oneshot(request?).await?;
                assert_eq!(response.status(), StatusCode::OK);
                Ok::<Value, anyhow::Error>(serde_json::from_slice(
                    &to_bytes(response.into_body(), 1_000_000).await?,
                )?)
            }
        };
        let found: Vec<Contact> =
            serde_json::from_value(call("GET", "/v1/contacts?q=so", "").await?)?;
        assert_eq!(
            (found[0].name.as_str(), found[0].email.as_str()),
            ("Sophie Chen", "sophie@form.studio")
        );
        assert_eq!(found[0].sources, [ContactSource::Mail]);
        let imported = call("PUT", "/v1/contacts/system", r#"{"contacts":[{"name":"Sol Rivera","email":"Sol@Example.org"},{"name":"x","email":"invalid"}]}"#).await?;
        assert_eq!(imported["contacts"], 1);
        let found: Vec<Contact> =
            serde_json::from_value(call("GET", "/v1/contacts?q=riv&limit=3", "").await?)?;
        assert_eq!(
            (found[0].name.as_str(), found[0].email.as_str()),
            ("Sol Rivera", "sol@example.org")
        );
        assert_eq!(
            call("DELETE", "/v1/contacts/system", "").await?["removed"],
            1
        );
        assert!(
            call("GET", "/v1/contacts?q=riv", "")
                .await?
                .as_array()
                .is_some_and(Vec::is_empty)
        );
        Ok(())
    }
    #[tokio::test]
    async fn invitations_parse_and_export_as_calendar_files() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = fixture_store(&dir.path().join("mail.db"))?;
        let invite = store
            .message("sample-13")?
            .invite
            .context("sample-13 carries an invitation")?;
        assert_eq!(
            (
                invite.method.as_str(),
                invite.response,
                invite.event.all_day
            ),
            ("REQUEST", None, false)
        );
        assert_eq!(
            invite.organizer.map(|o| o.email),
            Some("maya@studio-north.co".into())
        );
        let app = router(store.clone(), "test-token".into());
        let post = |uri: &str, body: &str| {
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("Authorization", "Bearer test-token")
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
        };
        // Mail without an invitation cannot be answered.
        let response = app
            .clone()
            .oneshot(post(
                "/v1/messages/sample-1/rsvp",
                r#"{"response":"accepted"}"#,
            )?)
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response = app
            .oneshot(post("/v1/calendar/ics", r#"{"message_id":"sample-13"}"#)?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let file: EventFile =
            serde_json::from_slice(&to_bytes(response.into_body(), 4_000_000).await?)?;
        assert!(
            std::path::Path::new(&file.filename)
                .extension()
                .is_some_and(|e| e == "ics")
                && !file.filename.contains('/')
        );
        assert!(
            file.ics
                .contains("UID:sample-13-identity-review@studio-north.co")
                && !file.ics.contains("METHOD:")
        );
        Ok(())
    }
}
