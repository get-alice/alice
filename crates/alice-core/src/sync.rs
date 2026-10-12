//! Keeps each account's sync window on this device.
//!
//! A sync run first catches up: per mailbox it reads the server status, applies changed flags
//! (with CONDSTORE only those changed since the last run), drops copies the server no longer has
//! in the window and downloads new mail. On a mailbox's first sync it downloads the newest
//! [`FIRST_SYNC`] messages, so the inbox is usable at once. Older mail in the window is then
//! backfilled newest first in the background, in short slices that release the account between
//! batches, so reading, sending and moving mail never wait on a long download.
//!
//! Only headers, structure and text parts are downloaded; attachments stay on the server until
//! requested.

use crate::{
    model::*,
    oauth, providers,
    remote::{Header, Remote},
    store::Store,
    sync_store::{Incoming, MailboxState},
    vault::Secret,
};
use anyhow::{Result, bail};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Messages per mailbox downloaded right away on its first sync.
const FIRST_SYNC: usize = 50;
/// Messages fetched and stored together.
const BATCH: usize = 50;
/// How long a background run backfills before every account is checked for new mail again.
const BACKFILL_SLICE: Duration = Duration::from_secs(20);
/// How often accounts are checked for new mail when nothing else is pending.
const INTERVAL: Duration = Duration::from_secs(60);
/// How often every flag is re-read on servers without CONDSTORE.
const FLAG_REFRESH: chrono::TimeDelta = chrono::TimeDelta::minutes(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// New mail, flags and removals only; for an explicit sync or a newly connected account.
    CatchUp,
    /// Catch up, then download older mail in the window for up to this long.
    Backfill(Duration),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Messages added, removed or changed by this run.
    pub changed: usize,
    /// Server messages in the window still to download.
    pub remaining: usize,
}

/// Called with an account's previous `last_synced` and each batch of messages a sync added.
pub type AddedHook = Arc<dyn Fn(Option<&str>, &[Message]) + Send + Sync>;

/// UIDs still to download from a mailbox, valid while its status is unchanged.
struct Pending {
    validity: u32,
    uid_next: Option<u32>,
    exists: u32,
    since: String,
    uids: Vec<u32>,
}

/// Per-account coordination.
pub struct AccountSync {
    /// Held while this account's cached mail or server state changes: sync storing a batch,
    /// a user's edit, move or send, and account removal.
    pub lock: Arc<tokio::sync::Mutex<()>>,
    run: Arc<tokio::sync::Mutex<()>>,
    /// An explicit sync is waiting for the running backfill to stop.
    yield_requested: AtomicBool,
    active: AtomicBool,
    pending: Mutex<HashMap<String, Pending>>,
}

pub struct SyncEngine {
    store: Arc<Store>,
    added: AddedHook,
    wake: tokio::sync::Notify,
    accounts: Mutex<HashMap<String, Arc<AccountSync>>>,
}

/// Clears an account's `active` flag when its run ends, however it ends.
struct Active<'a>(&'a AtomicBool);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl SyncEngine {
    pub fn new(store: Arc<Store>, added: AddedHook) -> Arc<Self> {
        Arc::new(Self {
            store,
            added,
            wake: tokio::sync::Notify::new(),
            accounts: Mutex::new(HashMap::new()),
        })
    }
    pub fn account(&self, account_id: &str) -> Arc<AccountSync> {
        self.accounts
            .lock()
            .entry(account_id.to_string())
            .or_insert_with(|| {
                Arc::new(AccountSync {
                    lock: Arc::new(tokio::sync::Mutex::new(())),
                    run: Arc::new(tokio::sync::Mutex::new(())),
                    yield_requested: AtomicBool::new(false),
                    active: AtomicBool::new(false),
                    pending: Mutex::new(HashMap::new()),
                })
            })
            .clone()
    }
    /// Drops a removed account's coordination state.
    pub fn forget(&self, account_id: &str) {
        self.accounts.lock().remove(account_id);
    }
    /// Stops the account's running sync at its next batch and keeps others from starting until the
    /// returned guard is dropped, e.g. while the account moves to another server. What the
    /// stopped run planned to download is forgotten.
    pub async fn pause(&self, account_id: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let entry = self.account(account_id);
        entry.yield_requested.store(true, Ordering::Release);
        let paused = entry.run.clone().lock_owned().await;
        entry.yield_requested.store(false, Ordering::Release);
        entry.pending.lock().clear();
        paused
    }
    /// Starts a background pass now, e.g. after the sync window changed.
    pub fn wake(&self) {
        self.wake.notify_one();
    }
    pub fn is_active(&self, account_id: &str) -> bool {
        self.accounts
            .lock()
            .get(account_id)
            .is_some_and(|a| a.active.load(Ordering::Acquire))
    }
    pub fn any_active(&self) -> bool {
        self.accounts
            .lock()
            .values()
            .any(|a| a.active.load(Ordering::Acquire))
    }
    /// Syncs one account. An explicit catch-up asks a running backfill to stop at its next batch.
    pub async fn sync_account(&self, account_id: &str, mode: Mode) -> Result<Outcome> {
        let entry = self.account(account_id);
        if mode == Mode::CatchUp {
            entry.yield_requested.store(true, Ordering::Release);
        }
        let _run = entry.run.lock().await;
        if mode == Mode::CatchUp {
            entry.yield_requested.store(false, Ordering::Release);
        }
        let account = self.store.account(account_id)?;
        entry.active.store(true, Ordering::Release);
        let active = Active(&entry.active);
        let result = async {
            let secret = oauth::credentials(&account).await?;
            let window = self.store.preferences()?.sync_window;
            let (store, entry, account, hook) = (
                self.store.clone(),
                entry.clone(),
                account.clone(),
                self.added.clone(),
            );
            tokio::task::spawn_blocking(move || {
                let previous = account.last_synced.clone();
                let added = move |messages: &[Message]| hook(previous.as_deref(), messages);
                let run = Run {
                    store: &store,
                    entry: &entry,
                    account: &account,
                    added: &added,
                };
                run.run(&secret, window, mode)
            })
            .await?
        }
        .await;
        // Recording the outcome changes the store revision, so clients polling for changes read
        // the account as idle from here on.
        drop(active);
        match result {
            Ok((outcome, folders)) => {
                self.store.record_sync(account_id, Some(&folders), None)?;
                Ok(outcome)
            }
            Err(error) => {
                if self
                    .store
                    .record_sync(account_id, None, Some(&error.to_string()))?
                {
                    return Err(error);
                }
                bail!(crate::text!("server.error.account_not_found"))
            }
        }
    }
    /// Catches up every account now; older mail keeps downloading in the background.
    pub async fn sync_all(&self) -> Result<Vec<(String, Result<Outcome>)>> {
        let mut results = Vec::new();
        for account in self.store.accounts()? {
            results.push((
                account.id.clone(),
                self.sync_account(&account.id, Mode::CatchUp).await,
            ));
        }
        self.wake();
        Ok(results)
    }
    /// Runs for the daemon's lifetime: checks accounts every minute (15 seconds with OTP
    /// copying enabled), or right after [`Self::wake`], and backfills while older mail remains.
    pub async fn run_background(self: Arc<Self>) {
        loop {
            let mut more = false;
            for account in self.store.accounts().unwrap_or_default() {
                if let Ok(outcome) = self
                    .sync_account(&account.id, Mode::Backfill(BACKFILL_SLICE))
                    .await
                {
                    more |= outcome.remaining > 0;
                }
            }
            if !more {
                tokio::select! {
                    // Opt-in code delivery polls sooner without changing manual sync/backfill.
                    _ = tokio::time::sleep(if self.store.preferences().is_ok_and(|p| p.otp_autocopy) { Duration::from_secs(15) } else { INTERVAL }) => {}
                    _ = self.wake.notified() => {}
                }
            }
        }
    }
}

/// One mailbox's work for this run.
struct Plan {
    folder: String,
    mailbox: String,
    validity: u32,
    /// UIDs to download, newest first.
    missing: Vec<u32>,
    /// UIDs from here on arrived since the previous run; `None` on a mailbox's first sync.
    floor: Option<u32>,
}

/// One sync run of one account, on a blocking thread.
struct Run<'a> {
    store: &'a Store,
    entry: &'a AccountSync,
    account: &'a Account,
    added: &'a dyn Fn(&[Message]),
}

impl Run<'_> {
    fn run(&self, secret: &Secret, window: SyncWindow, mode: Mode) -> Result<(Outcome, Mailboxes)> {
        let mut remote = Remote::connect(self.account, secret)?;
        let result = self.run_with(&mut remote, window, mode);
        remote.logout();
        result
    }
    /// Runs `change` while holding the account, so it never interleaves with a user's edit.
    fn locked<T>(&self, change: impl FnOnce() -> Result<T>) -> Result<T> {
        let _lock = self.entry.lock.blocking_lock();
        change()
    }
    fn run_with(
        &self,
        remote: &mut Remote,
        window: SyncWindow,
        mode: Mode,
    ) -> Result<(Outcome, Mailboxes)> {
        let (folders, mailboxes) = remote.folders()?;
        let synced: Vec<String> = mailboxes
            .iter()
            .map(|(_, mailbox)| mailbox.clone())
            .collect();
        self.locked(|| self.store.forget_other_mailboxes(&self.account.id, &synced))?;
        self.entry
            .pending
            .lock()
            .retain(|mailbox, _| synced.contains(mailbox));
        let since = window.since(chrono::Local::now().date_naive());
        let since_key = since.map_or_else(|| "all".to_string(), |day| day.to_string());
        let mut changed = 0;
        let mut plans = Vec::new();
        for (folder, mailbox) in &mailboxes {
            let (plan, refreshed) =
                self.refresh(remote, folder, mailbox, since, &since_key, mode)?;
            changed += refreshed;
            plans.push(plan);
        }
        // New mail first, in every mailbox; older mail only when backfilling.
        for plan in &mut plans {
            let fresh: Vec<u32> = match plan.floor {
                Some(floor) => plan
                    .missing
                    .iter()
                    .copied()
                    .filter(|uid| *uid >= floor)
                    .collect(),
                None => plan.missing.iter().copied().take(FIRST_SYNC).collect(),
            };
            let taken: HashSet<u32> = fresh.iter().copied().collect();
            plan.missing.retain(|uid| !taken.contains(uid));
            for batch in fresh.chunks(BATCH) {
                changed += self.fetch(remote, plan, batch)?;
            }
        }
        if let Mode::Backfill(budget) = mode {
            let deadline = Instant::now() + budget;
            'backfill: while plans.iter().any(|p| !p.missing.is_empty()) {
                for plan in &mut plans {
                    if Instant::now() >= deadline
                        || self.entry.yield_requested.load(Ordering::Acquire)
                    {
                        break 'backfill;
                    }
                    let batch: Vec<u32> = plan
                        .missing
                        .drain(..plan.missing.len().min(BATCH))
                        .collect();
                    if !batch.is_empty() {
                        changed += self.fetch(remote, plan, &batch)?;
                    }
                }
            }
        }
        let mut pending = self.entry.pending.lock();
        for plan in &plans {
            if let Some(p) = pending.get_mut(&plan.mailbox) {
                p.uids.clone_from(&plan.missing);
            }
        }
        let remaining = plans.iter().map(|p| p.missing.len()).sum();
        Ok((Outcome { changed, remaining }, folders))
    }
    /// Brings one mailbox's flags and membership up to date and plans what is left to download.
    fn refresh(
        &self,
        remote: &mut Remote,
        folder: &str,
        mailbox: &str,
        since: Option<chrono::NaiveDate>,
        since_key: &str,
        mode: Mode,
    ) -> Result<(Plan, usize)> {
        let account_id = &self.account.id;
        let status = remote.examine(mailbox)?;
        let mut state = match self.store.mailbox_state(account_id, mailbox)? {
            Some(state) if state.uid_validity == Some(status.validity) => state,
            Some(_) => {
                // UIDVALIDITY changed: every cached UID of this mailbox now means nothing.
                self.locked(|| self.store.forget_mailbox(account_id, mailbox))?;
                self.entry.pending.lock().remove(mailbox);
                MailboxState::default()
            }
            None => MailboxState::default(),
        };
        let known_before = state.uid_validity.is_some();
        let mut known: HashSet<u32> = self
            .store
            .known_uids(account_id, mailbox, status.validity)?
            .into_iter()
            .collect();
        let mut changed = 0;
        // Flags of cached mail: with CONDSTORE only what changed since the last run; otherwise
        // all of them, on an explicit sync and every few minutes.
        if !known.is_empty() {
            let now = chrono::Utc::now();
            let flags = match (state.highest_modseq, status.highest_modseq) {
                (Some(old), Some(new)) if new != old => Some(remote.changed_flags(old)?),
                (Some(_), Some(_)) => None,
                _ => {
                    let checked = state
                        .flags_checked
                        .as_deref()
                        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok());
                    if mode == Mode::CatchUp
                        || checked.is_none_or(|t| now.signed_duration_since(t) >= FLAG_REFRESH)
                    {
                        state.flags_checked = Some(now.to_rfc3339());
                        let mut uids: Vec<u32> = known.iter().copied().collect();
                        uids.sort_unstable();
                        Some(remote.flags(&uids)?)
                    } else {
                        None
                    }
                }
            };
            if let Some(flags) = flags.filter(|f| !f.is_empty()) {
                changed += self.locked(|| {
                    self.store
                        .apply_flags(account_id, mailbox, status.validity, &flags)
                })?;
            }
        }
        // Membership: skip the UID search when nothing arrived, left or aged out since last time.
        let unchanged = known_before
            && state.uid_next == status.uid_next
            && state.message_count == Some(status.exists)
            && state.window_since.as_deref() == Some(since_key);
        let pending = self
            .entry
            .pending
            .lock()
            .get(mailbox)
            .filter(|p| {
                p.validity == status.validity
                    && p.uid_next == status.uid_next
                    && p.exists == status.exists
                    && p.since == since_key
            })
            .map(|p| p.uids.clone());
        let missing: Vec<u32> = match pending {
            Some(uids) if unchanged => uids
                .into_iter()
                .filter(|uid| !known.contains(uid))
                .collect(),
            _ if unchanged && known.len() >= state.server_total => Vec::new(),
            _ => {
                let window = remote.window(since)?;
                let present: HashSet<u32> = window.iter().copied().collect();
                let gone: Vec<u32> = known
                    .iter()
                    .copied()
                    .filter(|uid| !present.contains(uid))
                    .collect();
                if !gone.is_empty() {
                    changed += gone.len();
                    self.locked(|| {
                        self.store
                            .remove_copies(account_id, mailbox, status.validity, &gone)
                    })?;
                    known.retain(|uid| present.contains(uid));
                }
                state.server_total = window.len();
                window
                    .into_iter()
                    .rev()
                    .filter(|uid| !known.contains(uid))
                    .collect()
            }
        };
        // What arrived since the previous run is new mail; the rest of `missing` is backfill.
        let floor = known_before.then(|| {
            state
                .uid_next
                .unwrap_or_else(|| known.iter().max().map_or(1, |uid| uid + 1))
        });
        self.entry.pending.lock().insert(
            mailbox.to_string(),
            Pending {
                validity: status.validity,
                uid_next: status.uid_next,
                exists: status.exists,
                since: since_key.to_string(),
                uids: missing.clone(),
            },
        );
        state.folder = folder.to_string();
        state.uid_validity = Some(status.validity);
        state.uid_next = status.uid_next;
        state.message_count = Some(status.exists);
        state.highest_modseq = status.highest_modseq;
        state.window_since = Some(since_key.to_string());
        self.store.save_mailbox_state(account_id, mailbox, &state)?;
        Ok((
            Plan {
                folder: folder.to_string(),
                mailbox: mailbox.to_string(),
                validity: status.validity,
                missing,
                floor,
            },
            changed,
        ))
    }
    /// Downloads and stores one batch of a mailbox's missing messages. Copies of messages already
    /// cached from another mailbox are recorded without downloading their content.
    fn fetch(&self, remote: &mut Remote, plan: &Plan, uids: &[u32]) -> Result<usize> {
        let account_id = &self.account.id;
        let headers = remote.headers(&plan.mailbox, uids)?;
        let copies: Vec<(u32, Option<String>)> = headers
            .iter()
            .map(|h| (h.uid, h.message_id.clone()))
            .collect();
        let needed = self.store.needs_content(
            account_id,
            &plan.folder,
            &plan.mailbox,
            plan.validity,
            &copies,
        )?;
        let count = headers.len();
        let skipped = self.import(remote, plan, headers.clone(), &needed)?;
        if !skipped.is_empty() {
            // The message these copies would have joined changed meanwhile (a user's move, say);
            // download them after all, so they are not left behind.
            let retry: Vec<Header> = headers
                .into_iter()
                .filter(|h| skipped.contains(&h.uid))
                .collect();
            let all: HashSet<u32> = skipped.into_iter().collect();
            self.import(remote, plan, retry, &all)?;
        }
        Ok(count)
    }
    /// Downloads the content of the `needed` copies among `headers` and stores them all. Returns
    /// the UIDs the store still wanted content for.
    fn import(
        &self,
        remote: &mut Remote,
        plan: &Plan,
        headers: Vec<Header>,
        needed: &HashSet<u32>,
    ) -> Result<Vec<u32>> {
        let wanted: Vec<Header> = headers
            .iter()
            .filter(|h| needed.contains(&h.uid))
            .cloned()
            .collect();
        let mut contents: HashMap<u32, _> = remote
            .contents(&plan.mailbox, &wanted)?
            .into_iter()
            .collect();
        let incoming: Vec<Incoming> = headers
            .into_iter()
            .map(|header| {
                let message = contents.remove(&header.uid).map(|part| {
                    let fields = mailparse::parse_headers(&header.raw)
                        .map(|(fields, _)| fields)
                        .unwrap_or_default();
                    providers::message_from(
                        self.account,
                        &plan.folder,
                        &plan.mailbox,
                        plan.validity,
                        header.uid,
                        &fields,
                        &part,
                    )
                });
                Incoming {
                    uid: header.uid,
                    message_id: header.message_id,
                    is_read: header.is_read,
                    is_starred: header.is_starred,
                    date: header.date,
                    message,
                }
            })
            .collect();
        let (added, skipped) = self.locked(|| {
            self.store.import_copies(
                &self.account.id,
                &plan.folder,
                &plan.mailbox,
                plan.validity,
                plan.floor,
                incoming,
            )
        })?;
        if !added.is_empty() {
            (self.added)(&added);
        }
        Ok(skipped)
    }
}
