mod theme;
mod view;

use alice_core::{
    Account, Client, Compose, Contact, Message, MessagePage, MessagePatch, Preferences, Rsvp,
    RsvpRequest, i18n,
};
use anyhow::{Result, bail};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde_json::{Value, json};
use std::{
    io::IsTerminal,
    time::{Duration, Instant},
};
use theme::Theme;
use uuid::Uuid;
use view::Activity;

type InboxLoad = tokio::task::JoinHandle<Result<(MessagePage, Vec<Account>, Preferences)>>;
type ContactLookup = tokio::task::JoinHandle<Result<Vec<Contact>>>;

/// Recipient suggestions shown under the To field.
const SUGGESTIONS: usize = 5;

const FOLDERS: [&str; 7] = [
    "inbox", "starred", "drafts", "sent", "archive", "trash", "spam",
];
const PAGE: usize = 50;
/// Poll interval while a spinner or status fade is on screen.
const ANIMATING_POLL: Duration = Duration::from_millis(50);
/// Poll interval when nothing moves; keystrokes still wake the loop at once.
const IDLE_POLL: Duration = Duration::from_millis(250);

pub async fn run(client: Client) -> Result<()> {
    if !std::io::stdin().is_terminal() {
        bail!(alice_core::text!("tui.error.terminal"));
    }
    let mut terminal = ratatui::init();
    let outcome = run_loop(&client, &mut terminal).await;
    ratatui::restore();
    outcome
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Info,
    Error,
}

/// The footer message. `since` drives the arrival fade.
struct Status {
    text: String,
    tone: Tone,
    since: Instant,
    reconnecting: bool,
}

impl Status {
    fn set(&mut self, tone: Tone, text: impl Into<String>) {
        let text = text.into();
        self.reconnecting = false;
        // Re-setting the same message (e.g. a repeating reconnect error)
        // must not replay the arrival fade.
        if self.text != text || self.tone != tone {
            self.text = text;
            self.tone = tone;
            self.since = Instant::now();
        }
    }
    fn info(&mut self, text: impl Into<String>) {
        self.set(Tone::Info, text);
    }
    fn error(&mut self, text: impl Into<String>) {
        self.set(Tone::Error, text);
    }
    fn clear(&mut self) {
        self.text.clear();
        self.reconnecting = false;
    }
    fn fading(&self) -> bool {
        !self.text.is_empty() && self.since.elapsed() < theme::STATUS_FADE
    }
}

/// Everything the screen shows. The background jobs stay in `run_loop`.
struct App {
    folder_index: usize,
    offset: usize,
    total: usize,
    unread: usize,
    loaded: bool,
    accounts: Vec<Account>,
    preferences: Preferences,
    messages: Vec<Message>,
    selected: Option<usize>,
    /// First visible row of the message list; kept in view by the renderer.
    list_top: usize,
    query: String,
    searching: bool,
    editing: Option<Compose>,
    editor_to: String,
    editor_locked: bool,
    /// "New message", "Reply" or "Edit draft"; shown in the top bar and form.
    editor_title: String,
    field: usize,
    cursors: [usize; 3],
    scroll: u16,
    confirm: bool,
    /// Waiting for y/m/n to answer the selected invitation.
    rsvp: bool,
    /// Contacts matching the recipient being typed in the To field, best first.
    suggestions: Vec<Contact>,
    suggestion: usize,
    /// The text `suggestions` were requested for.
    suggest_query: String,
    help: bool,
    help_scroll: u16,
    status: Status,
    started: Instant,
}

impl App {
    fn new() -> Self {
        Self {
            folder_index: 0,
            offset: 0,
            total: 0,
            unread: 0,
            loaded: false,
            accounts: Vec::new(),
            preferences: Preferences::default(),
            messages: Vec::new(),
            selected: Some(0),
            list_top: 0,
            query: String::new(),
            searching: false,
            editing: None,
            editor_to: String::new(),
            editor_locked: false,
            editor_title: alice_core::t!(crate::locale(), "tui.editor.new"),
            field: 0,
            cursors: [0; 3],
            scroll: 0,
            confirm: false,
            rsvp: false,
            suggestions: Vec::new(),
            suggestion: 0,
            suggest_query: String::new(),
            help: false,
            help_scroll: 0,
            status: Status {
                text: String::new(),
                tone: Tone::Info,
                since: Instant::now(),
                reconnecting: false,
            },
            started: Instant::now(),
        }
    }
}

async fn run_loop(client: &Client, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
    let theme = Theme::from_env();
    let mut app = App::new();
    let mut loading: Option<InboxLoad> = None;
    let mut load_scope = None;
    let mut load_started = Instant::now();
    let mut mutation: Option<tokio::task::JoinHandle<Result<Update>>> = None;
    let mut lookup: Option<ContactLookup> = None;
    let mut quit_after_save = false;
    let mut last = Instant::now().checked_sub(Duration::from_secs(10)).unwrap();
    loop {
        let scope = (app.folder_index, app.query.clone(), app.offset);
        if mutation.is_some() || load_scope.as_ref().is_some_and(|old| old != &scope) {
            if let Some(job) = loading.take() {
                job.abort();
            }
            load_scope = None;
            last = Instant::now().checked_sub(Duration::from_secs(10)).unwrap();
        }
        if mutation.as_ref().is_some_and(|job| job.is_finished()) {
            match mutation.take().unwrap().await? {
                Ok(Update::Saved) => {
                    app.editing = None;
                    app.status
                        .info(alice_core::t!(crate::locale(), "tui.status.draft_saved"));
                    if quit_after_save {
                        break;
                    }
                }
                Ok(Update::Status(text)) => app.status.info(text),
                Ok(Update::Answered(message, text)) => {
                    if let Some(old) = app.messages.iter_mut().find(|m| m.id == message.id) {
                        *old = *message;
                    }
                    app.status.info(text);
                }
                Ok(Update::Failed(text)) => app.status.error(text),
                Err(error) => {
                    app.status.error(i18n::error_text(&error, crate::locale()));
                    quit_after_save = false;
                }
            }
            last = Instant::now().checked_sub(Duration::from_secs(10)).unwrap();
        }
        if loading.as_ref().is_some_and(|job| job.is_finished()) {
            match loading.take().unwrap().await? {
                Ok((page, accounts, preferences)) => {
                    app.accounts = accounts;
                    app.preferences = preferences;
                    let id = app
                        .selected
                        .and_then(|i| app.messages.get(i))
                        .map(|m| m.id.clone());
                    app.total = page.total;
                    app.unread = page.unread_total;
                    app.messages = page.messages;
                    app.loaded = true;
                    if app.offset > 0 && app.offset >= app.total {
                        app.offset = app.total.saturating_sub(1) / PAGE * PAGE;
                    }
                    app.selected = id
                        .and_then(|id| app.messages.iter().position(|m| m.id == id))
                        .or(if app.messages.is_empty() {
                            None
                        } else {
                            Some(0)
                        });
                    if app.status.reconnecting {
                        app.status.clear();
                    }
                }
                Err(error) => {
                    app.status.error(alice_core::t!(
                        crate::locale(),
                        "tui.error.reconnecting",
                        error = i18n::error_text(&error, crate::locale())
                    ));
                    app.status.reconnecting = true;
                }
            }
            load_scope = None;
            last = Instant::now();
        }
        if loading.is_none()
            && mutation.is_none()
            && last.elapsed() >= Duration::from_secs(2)
            && app.editing.is_none()
            && !app.confirm
        {
            let client = client.clone();
            let path = format!(
                "/v1/messages?folder={}&q={}&limit={PAGE}&offset={}",
                FOLDERS[app.folder_index],
                crate::encode(&app.query),
                app.offset
            );
            loading = Some(tokio::spawn(async move {
                Ok((
                    client.get(&path).await?,
                    client.get("/v1/accounts").await?,
                    client.get("/v1/preferences").await?,
                ))
            }));
            load_scope = Some(scope);
            load_started = Instant::now();
        }
        let (wanted, account_id) = match &app.editing {
            Some(draft) if app.field == 0 => (
                recipient_query(&app.editor_to, app.cursors[0]).to_string(),
                draft.account_id.as_str(),
            ),
            _ => (String::new(), ""),
        };
        if wanted != app.suggest_query {
            if let Some(job) = lookup.take() {
                job.abort();
            }
            app.suggestions.clear();
            app.suggestion = 0;
            if !wanted.is_empty() {
                let client = client.clone();
                let path = format!(
                    "/v1/contacts?q={}&account_id={}&limit={SUGGESTIONS}",
                    crate::encode(&wanted),
                    crate::encode(account_id)
                );
                lookup = Some(tokio::spawn(async move { client.get(&path).await }));
            }
            app.suggest_query = wanted;
        }
        if lookup.as_ref().is_some_and(|job| job.is_finished()) {
            // Suggestions are a convenience: a failed lookup just shows none.
            if let Ok(Ok(contacts)) = lookup.take().unwrap().await {
                let typed = recipients(&app.editor_to);
                app.suggestions = contacts
                    .into_iter()
                    .filter(|c| !typed.iter().any(|t| t.eq_ignore_ascii_case(&c.email)))
                    .take(SUGGESTIONS)
                    .collect();
                app.suggestion = 0;
            }
        }
        let activity = Activity {
            loading: loading.as_ref().map(|_| load_started.elapsed()),
            busy: mutation.is_some(),
            clock: app.started.elapsed(),
        };
        terminal.draw(|frame| view::draw(frame, &mut app, &theme, activity))?;
        let animating =
            activity.loading.is_some() || activity.busy || lookup.is_some() || app.status.fading();
        if !event::poll(if animating { ANIMATING_POLL } else { IDLE_POLL })? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if app.editing.is_none() {
                break;
            }
            quit_after_save = true;
        }
        if app.help {
            match key.code {
                KeyCode::Char('?') | KeyCode::Esc => app.help = false,
                KeyCode::PageDown => app.help_scroll = app.help_scroll.saturating_add(8),
                KeyCode::PageUp => app.help_scroll = app.help_scroll.saturating_sub(8),
                _ => {}
            }
            continue;
        }
        let active = app.selected.and_then(|i| app.messages.get(i));
        if app.confirm {
            app.confirm = false;
            if key.code == KeyCode::Char('y')
                && mutation.is_none()
                && let Some(m) = active
            {
                let client = client.clone();
                let path = format!("/v1/messages/{}/send", crate::encode(&m.id));
                mutation = Some(tokio::spawn(async move {
                    let _: Message = client.post(&path, &json!({})).await?;
                    Ok(Update::Status(alice_core::t!(
                        crate::locale(),
                        "tui.status.draft_sent"
                    )))
                }));
                app.status
                    .info(alice_core::t!(crate::locale(), "tui.status.sending"));
            } else if key.code != KeyCode::Char('y') {
                app.status
                    .info(alice_core::t!(crate::locale(), "tui.status.send_cancelled"));
            }
            continue;
        }
        if app.rsvp {
            app.rsvp = false;
            let response = match key.code {
                KeyCode::Char('y') => Some(Rsvp::Accepted),
                KeyCode::Char('m') => Some(Rsvp::Tentative),
                KeyCode::Char('n') => Some(Rsvp::Declined),
                _ => None,
            };
            match (response, active) {
                (Some(response), Some(m)) if mutation.is_none() => {
                    let client = client.clone();
                    let path = format!("/v1/messages/{}/rsvp", crate::encode(&m.id));
                    let demo = app
                        .accounts
                        .iter()
                        .any(|a| a.id == m.account_id && a.is_demo);
                    mutation = Some(tokio::spawn(async move {
                        let message: Message = client
                            .post(
                                &path,
                                &RsvpRequest {
                                    response,
                                    comment: None,
                                },
                            )
                            .await?;
                        let text = crate::rsvp_done(&message, demo);
                        Ok(Update::Answered(Box::new(message), text))
                    }));
                    app.status
                        .info(alice_core::t!(crate::locale(), "tui.status.answering"));
                }
                _ => app
                    .status
                    .info(alice_core::t!(crate::locale(), "tui.status.not_answered")),
            }
            continue;
        }
        if let Some(draft) = &mut app.editing {
            if mutation.is_some() {
                continue;
            }
            if key.code == KeyCode::Esc
                || quit_after_save
                || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s'))
            {
                draft.to = recipients(&app.editor_to);
                let client = client.clone();
                let draft = draft.clone();
                mutation = Some(tokio::spawn(async move {
                    let _: Message = client.post("/v1/messages", &draft).await?;
                    Ok(Update::Saved)
                }));
                app.status
                    .info(alice_core::t!(crate::locale(), "tui.status.saving"));
                continue;
            }
            if key.code == KeyCode::F(2) && !app.editor_locked {
                if !app.accounts.is_empty() {
                    let index = app
                        .accounts
                        .iter()
                        .position(|a| a.id == draft.account_id)
                        .unwrap_or(0);
                    draft.account_id = app.accounts[(index + 1) % app.accounts.len()].id.clone();
                }
                continue;
            }
            if app.field == 0 && !app.suggestions.is_empty() {
                match key.code {
                    KeyCode::Down => {
                        app.suggestion = (app.suggestion + 1).min(app.suggestions.len() - 1);
                        continue;
                    }
                    KeyCode::Up => {
                        app.suggestion = app.suggestion.saturating_sub(1);
                        continue;
                    }
                    KeyCode::Enter => {
                        let email = crate::terminal_text(&app.suggestions[app.suggestion].email);
                        app.cursors[0] =
                            accept_recipient(&mut app.editor_to, app.cursors[0], &email);
                        continue;
                    }
                    _ => {}
                }
            }
            if key.code == KeyCode::Tab {
                app.field = (app.field + 1) % 3;
                continue;
            }
            let value = match app.field {
                0 => &mut app.editor_to,
                1 => &mut draft.subject,
                _ => &mut draft.body,
            };
            edit_text(value, &mut app.cursors[app.field], key, app.field == 2);
            continue;
        }
        if app.searching {
            match key.code {
                KeyCode::Enter | KeyCode::Esc => app.searching = false,
                KeyCode::Char(c) => app.query.push(c),
                KeyCode::Backspace => {
                    app.query.pop();
                }
                _ => {}
            }
            app.offset = 0;
            last = Instant::now().checked_sub(Duration::from_secs(10)).unwrap();
            continue;
        }
        match key.code {
            KeyCode::Char('q') => break,
            KeyCode::Char('?') => {
                app.help = true;
                app.help_scroll = 0;
            }
            KeyCode::Esc if !app.query.is_empty() => {
                app.query.clear();
                app.offset = 0;
                app.scroll = 0;
                last = Instant::now().checked_sub(Duration::from_secs(10)).unwrap();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !app.messages.is_empty() {
                    app.selected =
                        Some((app.selected.unwrap_or(0) + 1).min(app.messages.len() - 1));
                    app.scroll = 0;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.selected = app.selected.map(|i| i.saturating_sub(1));
                app.scroll = 0;
            }
            KeyCode::PageDown => app.scroll = app.scroll.saturating_add(8),
            KeyCode::PageUp => app.scroll = app.scroll.saturating_sub(8),
            KeyCode::Char('/') => app.searching = true,
            KeyCode::Char('g') => {
                app.folder_index = (app.folder_index + 1) % FOLDERS.len();
                app.offset = 0;
                last = Instant::now().checked_sub(Duration::from_secs(10)).unwrap();
                app.scroll = 0;
            }
            KeyCode::Char('[') if app.offset > 0 => {
                app.offset = app.offset.saturating_sub(PAGE);
                app.scroll = 0;
                last = Instant::now().checked_sub(Duration::from_secs(10)).unwrap();
            }
            KeyCode::Char(']') if app.offset + PAGE < app.total => {
                app.offset += PAGE;
                app.scroll = 0;
                last = Instant::now().checked_sub(Duration::from_secs(10)).unwrap();
            }
            KeyCode::Char('S') if mutation.is_none() => {
                let client = client.clone();
                mutation = Some(tokio::spawn(async move {
                    let value: Value = client.post("/v1/sync", &json!({})).await?;
                    Ok(sync_summary(&value, crate::locale()))
                }));
                app.status
                    .info(alice_core::t!(crate::locale(), "tui.status.syncing"));
            }
            KeyCode::Char('r') | KeyCode::Char('c') if mutation.is_none() => {
                let reply = key.code == KeyCode::Char('r');
                let accounts = &app.accounts;
                let account = active
                    .filter(|_| reply)
                    .map(|m| m.account_id.clone())
                    .or_else(|| {
                        accounts
                            .iter()
                            .find(|a| {
                                a.id == app.preferences.default_account_id
                                    && (!a.is_demo || accounts.iter().all(|a| a.is_demo))
                            })
                            .or_else(|| accounts.iter().find(|a| !a.is_demo))
                            .or(accounts.first())
                            .map(|a| a.id.clone())
                    });
                if let Some(account_id) = account {
                    let edit_draft = reply && active.is_some_and(|m| m.folder == "drafts");
                    let draft = if edit_draft {
                        let m = active.unwrap();
                        Compose {
                            id: m.id.clone(),
                            account_id: m.account_id.clone(),
                            to: m.to.clone(),
                            subject: m.subject.clone(),
                            body: m.body.clone(),
                            in_reply_to: m.in_reply_to.clone(),
                        }
                    } else {
                        Compose {
                            id: Uuid::new_v4().to_string(),
                            account_id,
                            to: if reply {
                                active.map(Message::reply_recipients).unwrap_or_default()
                            } else {
                                vec![]
                            },
                            subject: if reply {
                                active
                                    .map(|m| {
                                        if m.subject.to_lowercase().starts_with("re:") {
                                            m.subject.clone()
                                        } else {
                                            format!("Re: {}", m.subject)
                                        }
                                    })
                                    .unwrap_or_default()
                            } else {
                                String::new()
                            },
                            body: app.preferences.draft_body(""),
                            in_reply_to: if reply {
                                active.and_then(|m| m.message_id.clone())
                            } else {
                                None
                            },
                        }
                    };
                    app.editor_to = draft.to.join(", ");
                    app.cursors = [
                        app.editor_to.len(),
                        draft.subject.len(),
                        if active.is_some_and(|m| m.id == draft.id && m.folder == "drafts") {
                            draft.body.len()
                        } else {
                            0
                        },
                    ];
                    app.editor_locked = reply && active.is_some();
                    app.editor_title = if edit_draft {
                        alice_core::t!(crate::locale(), "tui.editor.edit")
                    } else if app.editor_locked {
                        alice_core::t!(crate::locale(), "tui.editor.reply")
                    } else {
                        alice_core::t!(crate::locale(), "tui.editor.new")
                    };
                    app.field = if reply { 2 } else { 0 };
                    app.editing = Some(draft);
                    app.status.clear();
                } else {
                    app.status
                        .info(alice_core::t!(crate::locale(), "tui.status.no_accounts"));
                }
            }
            KeyCode::Char('i') if mutation.is_none() => {
                match active.and_then(|m| m.invite.as_ref()) {
                    Some(invite) if crate::answerable(invite) => app.rsvp = true,
                    Some(invite) if invite.cancelled => app.status.info(alice_core::t!(
                        crate::locale(),
                        "tui.status.event_cancelled"
                    )),
                    Some(_) => app.status.info(alice_core::t!(
                        crate::locale(),
                        "tui.status.no_answer_needed"
                    )),
                    None => app
                        .status
                        .info(alice_core::t!(crate::locale(), "tui.status.not_invitation")),
                }
            }
            KeyCode::Char('!') => {
                if active.is_some_and(|m| m.folder == "drafts") {
                    app.confirm = true;
                } else {
                    app.status
                        .info(alice_core::t!(crate::locale(), "tui.status.only_draft"));
                }
            }
            key => {
                if let Some(m) = active {
                    let patch = match key {
                        KeyCode::Enter => Some((
                            MessagePatch {
                                is_read: Some(true),
                                ..Default::default()
                            },
                            "tui.status.marked_read",
                        )),
                        KeyCode::Char('a') => Some((
                            MessagePatch {
                                folder: Some("archive".into()),
                                ..Default::default()
                            },
                            "tui.status.archived",
                        )),
                        KeyCode::Char('d') => Some((
                            MessagePatch {
                                folder: Some("trash".into()),
                                ..Default::default()
                            },
                            "tui.status.trashed",
                        )),
                        KeyCode::Char('x') if !matches!(m.folder.as_str(), "drafts" | "sent") => {
                            Some((
                                MessagePatch {
                                    folder: Some(
                                        if m.folder == "spam" { "inbox" } else { "spam" }.into(),
                                    ),
                                    ..Default::default()
                                },
                                if m.folder == "spam" {
                                    "tui.status.not_spam"
                                } else {
                                    "tui.status.spam"
                                },
                            ))
                        }
                        KeyCode::Char('s') => Some((
                            MessagePatch {
                                is_starred: Some(!m.is_starred),
                                ..Default::default()
                            },
                            if m.is_starred {
                                "tui.status.unstarred"
                            } else {
                                "tui.status.starred"
                            },
                        )),
                        KeyCode::Char('u') => Some((
                            MessagePatch {
                                is_read: Some(false),
                                ..Default::default()
                            },
                            "tui.status.marked_unread",
                        )),
                        _ => None,
                    };
                    if let Some((patch, done)) = patch
                        && mutation.is_none()
                    {
                        let client = client.clone();
                        let path = format!("/v1/messages/{}", crate::encode(&m.id));
                        mutation = Some(tokio::spawn(async move {
                            let _: Message = client.patch(&path, &patch).await?;
                            Ok(Update::Status(i18n::tr(crate::locale(), done, &[])))
                        }));
                        app.status
                            .info(alice_core::t!(crate::locale(), "tui.status.updating"));
                    }
                }
            }
        }
    }
    Ok(())
}

enum Update {
    Saved,
    Status(String),
    Failed(String),
    /// An invitation was answered: the updated message and what happened.
    Answered(Box<Message>, String),
}

/// The addresses in the To field, split on `,` or `;`.
fn recipients(to: &str) -> Vec<String> {
    to.split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Byte range of the recipient under `cursor`: between the separators around it.
fn recipient_range(to: &str, cursor: usize) -> std::ops::Range<usize> {
    let start = to[..cursor].rfind([',', ';']).map_or(0, |i| i + 1);
    let end = to[cursor..]
        .find([',', ';'])
        .map_or(to.len(), |i| cursor + i);
    start..end
}

/// The recipient being typed at `cursor`, trimmed; what suggestions are looked up for.
fn recipient_query(to: &str, cursor: usize) -> &str {
    to[recipient_range(to, cursor)].trim()
}

/// Replaces the recipient under `cursor` with `address`, keeps the others, and leaves `, ` after it
/// so the next one can be typed. Returns the new cursor.
fn accept_recipient(to: &mut String, cursor: usize, address: &str) -> usize {
    let range = recipient_range(to, cursor);
    let before = to[..range.start].trim_end();
    let after = to[range.end..].trim_start_matches([',', ';']).trim_start();
    let mut next = String::from(before);
    if !next.is_empty() {
        next.push(' ');
    }
    next.push_str(address);
    next.push_str(", ");
    let cursor = next.len();
    next.push_str(after);
    *to = next;
    cursor
}

/// Turns the `/v1/sync` response (`{ synced, errors: [{ message }] }`) into a
/// sentence for the footer.
fn sync_summary(value: &Value, locale: i18n::Locale) -> Update {
    let errors: Vec<&str> = value["errors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|error| error["message"].as_str())
        .collect();
    if !errors.is_empty() {
        return Update::Failed(alice_core::t!(
            locale,
            "tui.error.sync",
            count = errors.len(),
            errors = errors.join(" · ")
        ));
    }
    match value["synced"].as_u64().unwrap_or(0) {
        0 => Update::Status(alice_core::t!(locale, "tui.status.up_to_date")),
        count => Update::Status(alice_core::t!(locale, "tui.status.synced", count = count)),
    }
}

fn edit_text(value: &mut String, cursor: &mut usize, key: KeyEvent, multiline: bool) {
    let previous = value[..*cursor]
        .char_indices()
        .next_back()
        .map(|(i, _)| i)
        .unwrap_or(0);
    let next = value[*cursor..]
        .chars()
        .next()
        .map(|c| *cursor + c.len_utf8())
        .unwrap_or(*cursor);
    let line_start = value[..*cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = value[*cursor..]
        .find('\n')
        .map(|i| *cursor + i)
        .unwrap_or(value.len());
    match key.code {
        KeyCode::Left => *cursor = previous,
        KeyCode::Right => *cursor = next,
        KeyCode::Home => *cursor = line_start,
        KeyCode::End => *cursor = line_end,
        KeyCode::Up | KeyCode::Down if multiline => {
            let column = value[line_start..*cursor].chars().count();
            let target = if key.code == KeyCode::Up && line_start > 0 {
                Some(
                    value[..line_start - 1]
                        .rfind('\n')
                        .map(|i| i + 1)
                        .unwrap_or(0),
                )
            } else if key.code == KeyCode::Down && line_end < value.len() {
                Some(line_end + 1)
            } else {
                None
            };
            if let Some(start) = target {
                let end = value[start..]
                    .find('\n')
                    .map(|i| start + i)
                    .unwrap_or(value.len());
                *cursor = start
                    + value[start..end]
                        .char_indices()
                        .nth(column)
                        .map(|(i, _)| i)
                        .unwrap_or(end - start);
            }
        }
        KeyCode::Backspace if *cursor > 0 => {
            value.replace_range(previous..*cursor, "");
            *cursor = previous;
        }
        KeyCode::Delete => {
            value.replace_range(*cursor..next, "");
        }
        KeyCode::Enter if multiline => {
            value.insert(*cursor, '\n');
            *cursor += 1;
        }
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !c.is_control() =>
        {
            value.insert(*cursor, c);
            *cursor += c.len_utf8();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_at_the_cursor_without_splitting_unicode() {
        let mut text = "Hello你好\nSecond".to_string();
        let mut cursor = "Hello你好".len();
        for key in [
            KeyCode::Left,
            KeyCode::Left,
            KeyCode::Left,
            KeyCode::Char('X'),
        ] {
            edit_text(
                &mut text,
                &mut cursor,
                KeyEvent::new(key, KeyModifiers::NONE),
                true,
            );
        }
        assert_eq!(text, "HellXo你好\nSecond");
        edit_text(
            &mut text,
            &mut cursor,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            true,
        );
        edit_text(
            &mut text,
            &mut cursor,
            KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE),
            true,
        );
        assert_eq!(text, "HellXo你好\nSecon");
        edit_text(
            &mut text,
            &mut cursor,
            KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
            true,
        );
        edit_text(
            &mut text,
            &mut cursor,
            KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE),
            true,
        );
        assert_eq!(text, "HellXo你好\n!Secon");
    }

    #[test]
    fn accepting_a_suggestion_replaces_only_the_recipient_being_typed() {
        let mut to = "ana@x.com, mi".to_string();
        assert_eq!(recipient_query(&to, to.len()), "mi");
        let cursor = accept_recipient(&mut to, 13, "maya@form.studio");
        assert_eq!(to, "ana@x.com, maya@form.studio, ");
        assert_eq!(cursor, to.len());
        assert_eq!(recipient_query(&to, cursor), "");

        // Editing a middle recipient keeps the ones after it.
        let mut to = "ana@x.com; jo ,bo@y.com".to_string();
        let cursor = "ana@x.com; jo".len();
        assert_eq!(recipient_query(&to, cursor), "jo");
        let cursor = accept_recipient(&mut to, cursor, "jo@z.com");
        assert_eq!(to, "ana@x.com; jo@z.com, bo@y.com");
        assert_eq!(&to[..cursor], "ana@x.com; jo@z.com, ");

        let mut to = "  sö".to_string();
        assert_eq!(
            accept_recipient(&mut to, 5, "söz@x.com"),
            "söz@x.com, ".len()
        );
        assert_eq!(to, "söz@x.com, ");
        assert_eq!(recipients("a@x.com, ;b@y.com;"), ["a@x.com", "b@y.com"]);
    }

    #[test]
    fn sync_results_read_as_sentences() {
        let Update::Status(text) =
            sync_summary(&json!({"synced": 3, "errors": []}), i18n::Locale::EnUs)
        else {
            panic!("expected success");
        };
        assert_eq!(text, "Synced 3 messages · your inbox is up to date");
        let Update::Failed(text) = sync_summary(
            &json!({"synced": 0, "errors": [{"account_id": "a", "message": "bad password"}]}),
            i18n::Locale::EnUs,
        ) else {
            panic!("expected failure");
        };
        assert_eq!(text, "Sync failed for 1 account: bad password");
    }
}
