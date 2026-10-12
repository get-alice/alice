//! What sync records about the server, kept beside the cached messages.
//!
//! `remote_messages` lists every server copy of cached mail: one row per mailbox, UIDVALIDITY and
//! UID, pointing at the cached message it belongs to. Providers keep one message in several
//! mailboxes (Gmail's All Mail holds every Inbox and Sent message), so several copies can point
//! at one message. The message's own `remote_mailbox`/`uid_validity`/`remote_uid` name its
//! primary copy, which decides its folder and flags: Inbox, then Sent, Archive and Trash.
//! When the primary copy leaves the server another copy takes over (archiving in Gmail's web
//! app moves the message to Archive here); the message is deleted with its last copy.
//!
//! `mailbox_sync` keeps each mailbox's last known status, so a sync can skip the UID search
//! when nothing arrived or left, and ask a CONDSTORE server only for changed flags.

use crate::{model::*, remote::RemoteFlags, store::Store};
use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::collections::HashSet;

/// Creates sync bookkeeping and retires the old conversation index. Run on every open.
pub(crate) fn migrate(db: &mut Connection) -> Result<()> {
    let tx = db.transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_parts(name TEXT PRIMARY KEY, version INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS remote_messages(account_id TEXT NOT NULL, mailbox TEXT NOT NULL, uid_validity INTEGER NOT NULL, uid INTEGER NOT NULL, message TEXT NOT NULL, is_read INTEGER, is_starred INTEGER, PRIMARY KEY(account_id, mailbox, uid_validity, uid)) WITHOUT ROWID;
        CREATE INDEX IF NOT EXISTS remote_messages_message ON remote_messages(message);
        CREATE TABLE IF NOT EXISTS mailbox_sync(account_id TEXT NOT NULL, mailbox TEXT NOT NULL, folder TEXT NOT NULL, uid_validity INTEGER, uid_next INTEGER, message_count INTEGER, highest_modseq INTEGER, window_since TEXT, server_total INTEGER NOT NULL DEFAULT 0, flags_checked TEXT, PRIMARY KEY(account_id, mailbox)) WITHOUT ROWID;
        DROP TRIGGER IF EXISTS conversation_index_insert;
        DROP TRIGGER IF EXISTS conversation_index_update;
        DROP TRIGGER IF EXISTS conversation_index_delete;
        DROP TABLE IF EXISTS conversation_index;
        DELETE FROM schema_parts WHERE name='conversation_index';",
    )?;
    // Each copy's own flags, so a copy that becomes the shown one brings its real state.
    let has_flags: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('remote_messages') WHERE name='is_read')",
        [],
        |r| r.get(0),
    )?;
    if !has_flags {
        tx.execute_batch("ALTER TABLE remote_messages ADD COLUMN is_read INTEGER; ALTER TABLE remote_messages ADD COLUMN is_starred INTEGER;")?;
    }
    let version = |tx: &Transaction<'_>, name: &str| -> Result<Option<i64>> {
        Ok(tx
            .query_row(
                "SELECT version FROM schema_parts WHERE name=?1",
                [name],
                |r| r.get(0),
            )
            .optional()?)
    };
    if version(&tx, "remote_messages")?.is_none() {
        // Earlier builds kept one server copy per message on the message itself. Mail cached
        // before HTML bodies were stored is left out, so the next sync downloads it again.
        tx.execute_batch(
            "INSERT OR IGNORE INTO remote_messages(account_id,mailbox,uid_validity,uid,message,is_read,is_starred) SELECT json_extract(data,'$.account_id'),json_extract(data,'$.remote_mailbox'),json_extract(data,'$.uid_validity'),json_extract(data,'$.remote_uid'),id,json_extract(data,'$.is_read'),json_extract(data,'$.is_starred') FROM messages
                WHERE json_extract(data,'$.remote_mailbox') IS NOT NULL AND json_extract(data,'$.uid_validity') IS NOT NULL AND json_extract(data,'$.remote_uid') IS NOT NULL AND json_type(data,'$.html_body') IS NOT NULL;
            -- Gmail's All Mail copy of a sent message was cached as a separate Archive message.
            CREATE TEMP TABLE merged AS SELECT archived.id AS duplicate, (SELECT sent.id FROM messages sent WHERE json_extract(sent.data,'$.account_id')=json_extract(archived.data,'$.account_id') AND json_extract(sent.data,'$.message_id')=json_extract(archived.data,'$.message_id') AND json_extract(sent.data,'$.folder')='sent' LIMIT 1) AS keep
                FROM messages archived WHERE json_extract(archived.data,'$.folder')='archive' AND json_extract(archived.data,'$.message_id') IS NOT NULL;
            DELETE FROM temp.merged WHERE keep IS NULL;
            UPDATE remote_messages SET message=(SELECT keep FROM temp.merged WHERE duplicate=message) WHERE message IN (SELECT duplicate FROM temp.merged);
            DELETE FROM messages WHERE id IN (SELECT duplicate FROM temp.merged);
            DROP TABLE temp.merged;
            INSERT INTO schema_parts VALUES('remote_messages', 1);",
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// A mailbox's status as of its last sync.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MailboxState {
    pub folder: String,
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub message_count: Option<u32>,
    pub highest_modseq: Option<u64>,
    /// First day of the sync window the UIDs were searched for (`all` for no limit).
    pub window_since: Option<String>,
    /// Server messages in the window at the last search.
    pub server_total: usize,
    /// When every flag was last read, for servers without CONDSTORE (RFC 3339).
    pub flags_checked: Option<String>,
}

/// A server copy found by sync, with its message when that had to be downloaded.
#[derive(Clone, Debug)]
pub struct Incoming {
    pub uid: u32,
    pub message_id: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub date: Option<String>,
    pub message: Option<Message>,
}

/// Where a server copy belongs.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    /// A message cached before copies were tracked, whose content is downloaded again.
    Refresh(String),
    /// Another copy of an already cached message.
    Existing(String),
    New,
}

/// Which folder's copy a message shows when it has several.
fn rank(folder: &str) -> u8 {
    match folder {
        "spam" => 0,
        "inbox" => 1,
        "sent" => 2,
        "archive" => 3,
        "trash" => 4,
        _ => 5,
    }
}

fn account_exists(tx: &Transaction<'_>, account_id: &str) -> Result<()> {
    if tx
        .query_row("SELECT 1 FROM accounts WHERE id=?1", [account_id], |_| {
            Ok(())
        })
        .optional()?
        .is_none()
    {
        bail!(crate::text!("server.error.account_not_found"));
    }
    Ok(())
}

/// The cached message a copy belongs to. Sent copies join Sent messages (including mail Alice
/// delivered itself) or a message only known from Archive; Inbox copies never join Sent messages,
/// because mail you send yourself is both a Sent and an Inbox message; other copies join any.
fn target(
    tx: &Transaction<'_>,
    account_id: &str,
    folder: &str,
    mailbox: &str,
    validity: u32,
    uid: u32,
    message_id: Option<&str>,
) -> Result<Target> {
    let legacy: Option<String> = tx
        .prepare_cached("SELECT id FROM messages WHERE json_extract(data,'$.account_id')=?1 AND json_extract(data,'$.remote_mailbox')=?2 AND json_extract(data,'$.remote_uid')=?3 AND json_extract(data,'$.uid_validity')=?4 LIMIT 1")?
        .query_row(params![account_id, mailbox, uid, validity], |r| r.get(0))
        .optional()?;
    if let Some(id) = legacy {
        return Ok(Target::Refresh(id));
    }
    let Some(message_id) = message_id else {
        return Ok(Target::New);
    };
    // A message that already has another UID in this mailbox is a different delivery that reused
    // the Message-ID, not another copy: Message-IDs are chosen by the sender.
    let mut stmt = tx.prepare_cached(
        "SELECT id,json_extract(data,'$.folder') FROM messages WHERE json_extract(data,'$.account_id')=?1 AND json_extract(data,'$.message_id')=?2 AND json_extract(data,'$.folder')!='drafts'
            AND NOT EXISTS(SELECT 1 FROM remote_messages r WHERE r.message=messages.id AND r.account_id=?1 AND r.mailbox=?3 AND r.uid_validity=?4 AND r.uid!=?5)
            AND NOT (json_extract(data,'$.remote_mailbox') IS ?3 AND json_extract(data,'$.uid_validity') IS ?4 AND json_extract(data,'$.remote_uid') IS NOT NULL AND json_extract(data,'$.remote_uid')!=?5)",
    )?;
    let rows: Vec<(String, String)> = stmt
        .query_map(
            params![account_id, message_id, mailbox, validity, uid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .collect::<rusqlite::Result<_>>()?;
    let pick = |allowed: &[&str]| {
        allowed.iter().find_map(|wanted| {
            rows.iter()
                .find(|(_, f)| f == wanted)
                .map(|(id, _)| id.clone())
        })
    };
    let found = match folder {
        "sent" => pick(&["spam", "sent", "archive"]),
        "inbox" => pick(&["spam", "inbox", "archive", "trash"]),
        _ => pick(&["spam", "inbox", "archive", "trash", "sent"]),
    };
    Ok(found.map_or(Target::New, Target::Existing))
}

/// A server copy: which folder's mailbox holds it, and its UID there.
struct Location<'a> {
    folder: &'a str,
    mailbox: &'a str,
    validity: u32,
    uid: u32,
}

/// Points `id` at the copy `at`, taking that copy's flags and date when given.
fn make_primary(
    tx: &Transaction<'_>,
    id: &str,
    at: Location<'_>,
    flags: Option<(bool, bool)>,
    date: Option<&str>,
) -> Result<()> {
    tx.prepare_cached("UPDATE messages SET data=json_set(data,'$.folder',?2,'$.remote_mailbox',?3,'$.uid_validity',?4,'$.remote_uid',?5) WHERE id=?1")?
        .execute(params![id, at.folder, at.mailbox, at.validity, at.uid])?;
    if let Some((read, starred)) = flags {
        tx.prepare_cached("UPDATE messages SET data=json_set(data,'$.is_read',json(?2),'$.is_starred',json(?3)) WHERE id=?1 AND (json_extract(data,'$.is_read')!=?4 OR json_extract(data,'$.is_starred')!=?5)")?
            .execute(params![id, if read { "true" } else { "false" }, if starred { "true" } else { "false" }, read, starred])?;
    }
    if let Some(date) = date {
        tx.prepare_cached("UPDATE messages SET data=json_set(data,'$.date',?2) WHERE id=?1 AND json_extract(data,'$.date') IS NOT ?2")?.execute(params![id, date])?;
    }
    Ok(())
}

/// A message's folder and primary copy: mailbox, UIDVALIDITY and UID.
type Primary = (String, Option<String>, Option<u32>, Option<u32>);

/// After copies of `ids` were removed: messages whose primary copy is gone switch to their best
/// remaining copy, and messages without copies are deleted.
fn settle(tx: &Transaction<'_>, account_id: &str, ids: HashSet<String>) -> Result<usize> {
    let mut deleted = 0;
    for id in ids {
        let primary: Option<Primary> = tx
            .query_row("SELECT json_extract(data,'$.folder'),json_extract(data,'$.remote_mailbox'),json_extract(data,'$.uid_validity'),json_extract(data,'$.remote_uid') FROM messages WHERE id=?1", [&id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .optional()?;
        let Some((folder, mailbox, validity, uid)) = primary else {
            continue;
        };
        // A successful MOVE can be known before its destination UID is. Keep Spam isolated until
        // its mailbox sync resolves that UID, even if stale normal copies disappear meanwhile.
        if folder == "spam" && uid.is_none() {
            continue;
        }
        let still_there: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM remote_messages WHERE account_id=?1 AND mailbox=?2 AND uid_validity=?3 AND uid=?4 AND message=?5)",
            params![account_id, mailbox, validity, uid, id],
            |r| r.get(0),
        )?;
        if still_there {
            continue;
        }
        let mut copies = tx.prepare_cached("SELECT r.mailbox,r.uid_validity,r.uid,COALESCE(s.folder,'archive'),r.is_read,r.is_starred FROM remote_messages r LEFT JOIN mailbox_sync s ON s.account_id=r.account_id AND s.mailbox=r.mailbox WHERE r.account_id=?1 AND r.message=?2")?;
        let best = copies
            .query_map(params![account_id, id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, u32>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<bool>>(4)?
                        .zip(r.get::<_, Option<bool>>(5)?),
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .min_by_key(|(_, _, _, folder, _)| rank(folder));
        match best {
            Some((mailbox, validity, uid, folder, flags)) => make_primary(
                tx,
                &id,
                Location {
                    folder: &folder,
                    mailbox: &mailbox,
                    validity,
                    uid,
                },
                flags,
                None,
            )?,
            None => {
                tx.execute("DELETE FROM messages WHERE id=?1", [&id])?;
                deleted += 1;
            }
        }
    }
    Ok(deleted)
}

impl Store {
    pub fn mailbox_state(&self, account_id: &str, mailbox: &str) -> Result<Option<MailboxState>> {
        Ok(self
            .db
            .lock()
            .query_row(
                "SELECT folder,uid_validity,uid_next,message_count,highest_modseq,window_since,server_total,flags_checked FROM mailbox_sync WHERE account_id=?1 AND mailbox=?2",
                [account_id, mailbox],
                |r| Ok(MailboxState { folder: r.get(0)?, uid_validity: r.get(1)?, uid_next: r.get(2)?, message_count: r.get(3)?, highest_modseq: r.get::<_, Option<i64>>(4)?.map(|n| n as u64), window_since: r.get(5)?, server_total: r.get::<_, i64>(6)? as usize, flags_checked: r.get(7)? }),
            )
            .optional()?)
    }
    /// Records a mailbox's status. Does nothing once the account is removed.
    pub fn save_mailbox_state(
        &self,
        account_id: &str,
        mailbox: &str,
        state: &MailboxState,
    ) -> Result<()> {
        self.db.lock().execute(
            "INSERT INTO mailbox_sync(account_id,mailbox,folder,uid_validity,uid_next,message_count,highest_modseq,window_since,server_total,flags_checked) SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10 WHERE EXISTS(SELECT 1 FROM accounts WHERE id=?1)
                ON CONFLICT(account_id,mailbox) DO UPDATE SET folder=excluded.folder,uid_validity=excluded.uid_validity,uid_next=excluded.uid_next,message_count=excluded.message_count,highest_modseq=excluded.highest_modseq,window_since=excluded.window_since,server_total=excluded.server_total,flags_checked=excluded.flags_checked",
            params![account_id, mailbox, state.folder, state.uid_validity, state.uid_next, state.message_count, state.highest_modseq.map(|n| n as i64), state.window_since, state.server_total as i64, state.flags_checked],
        )?;
        Ok(())
    }
    /// Mailboxes no longer synced (renamed, or no longer a special-use folder) and their copies.
    pub fn forget_other_mailboxes(&self, account_id: &str, keep: &[String]) -> Result<()> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        let mailboxes: Vec<String> = tx.prepare("SELECT DISTINCT mailbox FROM remote_messages WHERE account_id=?1 UNION SELECT mailbox FROM mailbox_sync WHERE account_id=?1")?.query_map([account_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        for mailbox in mailboxes.iter().filter(|m| !keep.contains(m)) {
            forget(&tx, account_id, mailbox)?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Drops every copy in `mailbox` and its status, after its UIDVALIDITY changed.
    pub fn forget_mailbox(&self, account_id: &str, mailbox: &str) -> Result<()> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        forget(&tx, account_id, mailbox)?;
        tx.commit()?;
        Ok(())
    }
    /// UIDs of `mailbox` that are cached, in ascending order. A copy whose message was deleted
    /// some other way counts as missing, so the next sync downloads it again.
    pub fn known_uids(&self, account_id: &str, mailbox: &str, validity: u32) -> Result<Vec<u32>> {
        let db = self.db.lock();
        let mut stmt = db.prepare_cached("SELECT r.uid FROM remote_messages r WHERE r.account_id=?1 AND r.mailbox=?2 AND r.uid_validity=?3 AND EXISTS(SELECT 1 FROM messages m WHERE m.id=r.message) ORDER BY r.uid")?;
        Ok(stmt
            .query_map(params![account_id, mailbox, validity], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Removes copies the server no longer has in the window. Returns how many messages were deleted.
    pub fn remove_copies(
        &self,
        account_id: &str,
        mailbox: &str,
        validity: u32,
        uids: &[u32],
    ) -> Result<usize> {
        if uids.is_empty() {
            return Ok(0);
        }
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        account_exists(&tx, account_id)?;
        let mut affected = HashSet::new();
        {
            let mut find = tx.prepare_cached("DELETE FROM remote_messages WHERE account_id=?1 AND mailbox=?2 AND uid_validity=?3 AND uid=?4 RETURNING message")?;
            for uid in uids {
                if let Some(id) = find
                    .query_row(params![account_id, mailbox, validity, uid], |r| {
                        r.get::<_, String>(0)
                    })
                    .optional()?
                {
                    affected.insert(id);
                }
            }
        }
        let deleted = settle(&tx, account_id, affected)?;
        tx.commit()?;
        Ok(deleted)
    }
    /// Applies server flags to messages whose primary copy is in `mailbox`. Returns how many changed.
    pub fn apply_flags(
        &self,
        account_id: &str,
        mailbox: &str,
        validity: u32,
        flags: &[RemoteFlags],
    ) -> Result<usize> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        account_exists(&tx, account_id)?;
        let mut changed = 0;
        {
            let mut copy = tx.prepare_cached("UPDATE remote_messages SET is_read=?5,is_starred=?6 WHERE account_id=?1 AND mailbox=?2 AND uid_validity=?3 AND uid=?4")?;
            let mut update = tx.prepare_cached("UPDATE messages SET data=json_set(data,'$.is_read',json(?5),'$.is_starred',json(?6)) WHERE json_extract(data,'$.account_id')=?1 AND json_extract(data,'$.remote_mailbox')=?2 AND json_extract(data,'$.uid_validity')=?3 AND json_extract(data,'$.remote_uid')=?4 AND (json_extract(data,'$.is_read')!=?7 OR json_extract(data,'$.is_starred')!=?8)")?;
            for flag in flags {
                copy.execute(params![
                    account_id,
                    mailbox,
                    validity,
                    flag.uid,
                    flag.is_read,
                    flag.is_starred
                ])?;
                changed += update.execute(params![
                    account_id,
                    mailbox,
                    validity,
                    flag.uid,
                    if flag.is_read { "true" } else { "false" },
                    if flag.is_starred { "true" } else { "false" },
                    flag.is_read,
                    flag.is_starred
                ])?;
            }
        }
        tx.commit()?;
        Ok(changed)
    }
    /// The UIDs among `copies` whose content must be downloaded: new messages and messages cached
    /// before copies were tracked. Other copies join a cached message without downloading it.
    pub fn needs_content(
        &self,
        account_id: &str,
        folder: &str,
        mailbox: &str,
        validity: u32,
        copies: &[(u32, Option<String>)],
    ) -> Result<HashSet<u32>> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        let mut needed = HashSet::new();
        for (uid, message_id) in copies {
            if !matches!(
                target(
                    &tx,
                    account_id,
                    folder,
                    mailbox,
                    validity,
                    *uid,
                    message_id.as_deref()
                )?,
                Target::Existing(_)
            ) {
                needed.insert(*uid);
            }
        }
        Ok(needed)
    }
    /// Records server copies found in `mailbox` (a `folder` mailbox), caching new messages.
    /// Returns the messages added to the cache, and the UIDs whose content turned out to be
    /// needed (the message they would join changed since [`Self::needs_content`]) but was not
    /// supplied; those copies are not recorded.
    /// `new_since` is the previous mailbox UIDNEXT, absent on first sync/reset. Only Inbox
    /// UIDs at or above that floor may enqueue OTPs, in this same import transaction.
    pub fn import_copies(
        &self,
        account_id: &str,
        folder: &str,
        mailbox: &str,
        validity: u32,
        new_since: Option<u32>,
        incoming: Vec<Incoming>,
    ) -> Result<(Vec<Message>, Vec<u32>)> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        account_exists(&tx, account_id)?;
        let otp_enabled = crate::otp::enabled(&tx)?;
        let previous: Option<String> = tx.query_row(
            "SELECT json_extract(data,'$.last_synced') FROM accounts WHERE id=?1",
            [account_id],
            |r| r.get(0),
        )?;
        let previous = previous
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|t| t.timestamp());
        let now = chrono::Utc::now().timestamp();
        let mut added = Vec::new();
        let mut skipped = Vec::new();
        for copy in incoming {
            let flags = Some((copy.is_read, copy.is_starred));
            let id = match target(
                &tx,
                account_id,
                folder,
                mailbox,
                validity,
                copy.uid,
                copy.message_id.as_deref(),
            )? {
                Target::Refresh(id) => {
                    let Some(mut message) = copy.message else {
                        skipped.push(copy.uid);
                        continue;
                    };
                    let current: Message = serde_json::from_str(&tx.query_row(
                        "SELECT data FROM messages WHERE id=?1",
                        [&id],
                        |r| r.get::<_, String>(0),
                    )?)?;
                    (message.id, message.folder, message.category) =
                        (id.clone(), current.folder, current.category);
                    (message.is_read, message.is_starred) = (copy.is_read, copy.is_starred);
                    if let Some(date) = &copy.date {
                        message.date = date.clone();
                    }
                    // The invitation still says "needs action"; keep the answer Alice sent.
                    if let (Some(fresh), Some(cached)) = (&mut message.invite, &current.invite)
                        && fresh.response.is_none()
                        && (&fresh.uid, fresh.sequence) == (&cached.uid, cached.sequence)
                    {
                        fresh.response = cached.response;
                    }
                    tx.prepare_cached("UPDATE messages SET data=?2 WHERE id=?1")?
                        .execute(params![id, serde_json::to_string(&message)?])?;
                    id
                }
                Target::Existing(id) => {
                    let current: (String, Option<String>, Option<u32>, Option<u32>) = tx.query_row(
                        "SELECT json_extract(data,'$.folder'),json_extract(data,'$.remote_mailbox'),json_extract(data,'$.uid_validity'),json_extract(data,'$.remote_uid') FROM messages WHERE id=?1",
                        [&id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                    )?;
                    let has_primary = match (&current.1, current.2, current.3) {
                        (Some(m), Some(v), Some(u)) => tx.query_row("SELECT EXISTS(SELECT 1 FROM remote_messages WHERE account_id=?1 AND mailbox=?2 AND uid_validity=?3 AND uid=?4)", params![account_id, m, v, u], |r| r.get(0))?,
                        _ => false,
                    };
                    // A sent delivery without a server copy yet adopts this one; Gmail's All Mail
                    // copy of it keeps it in Sent, any other copy shows where it is.
                    // A move without COPYUID may leave Spam's UID unresolved. Until Junk sync finds
                    // it, a stale normal copy must not pull the message out of quarantine.
                    if (!has_primary && current.0 != "spam")
                        || rank(folder) < rank(&current.0)
                        || (folder == "spam" && !has_primary)
                    {
                        let folder = if current.0 == "sent" && folder == "archive" {
                            "sent"
                        } else {
                            folder
                        };
                        make_primary(
                            &tx,
                            &id,
                            Location {
                                folder,
                                mailbox,
                                validity,
                                uid: copy.uid,
                            },
                            flags,
                            copy.date.as_deref(),
                        )?;
                    }
                    id
                }
                Target::New => {
                    let Some(mut message) = copy.message else {
                        skipped.push(copy.uid);
                        continue;
                    };
                    (message.is_read, message.is_starred) = (copy.is_read, copy.is_starred);
                    if let Some(date) = &copy.date {
                        message.date = date.clone();
                    }
                    let taken: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM messages WHERE id=?1)",
                        [&message.id],
                        |r| r.get(0),
                    )?;
                    if taken {
                        // The id belongs to another location of this message; keep both.
                        message.id = uuid::Uuid::new_v5(
                            &uuid::Uuid::NAMESPACE_URL,
                            format!("{account_id}:{mailbox}:{validity}:{}", copy.uid).as_bytes(),
                        )
                        .to_string();
                    }
                    tx.prepare_cached("INSERT INTO messages VALUES(?1,?2)")?
                        .execute(params![message.id, serde_json::to_string(&message)?])?;
                    if otp_enabled && new_since.is_some_and(|floor| copy.uid >= floor) {
                        crate::otp::enqueue(&tx, &message, previous, copy.date.as_deref(), now)?;
                    }
                    let id = message.id.clone();
                    added.push(message);
                    id
                }
            };
            tx.prepare_cached(
                "INSERT OR REPLACE INTO remote_messages VALUES(?1,?2,?3,?4,?5,?6,?7)",
            )?
            .execute(params![
                account_id,
                mailbox,
                validity,
                copy.uid,
                id,
                copy.is_read,
                copy.is_starred
            ])?;
        }
        tx.commit()?;
        Ok((added, skipped))
    }
    /// Records that a message moved on the server from `from` to its current primary copy.
    pub fn record_move(&self, message: &Message, from: (&str, u32, u32)) -> Result<()> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        tx.execute("DELETE FROM remote_messages WHERE account_id=?1 AND mailbox=?2 AND uid_validity=?3 AND uid=?4 AND message=?5", params![message.account_id, from.0, from.1, from.2, message.id])?;
        if let (Some(mailbox), Some(validity), Some(uid)) = (
            &message.remote_mailbox,
            message.uid_validity,
            message.remote_uid,
        ) {
            tx.execute(
                "INSERT OR REPLACE INTO remote_messages VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    message.account_id,
                    mailbox,
                    validity,
                    uid,
                    message.id,
                    message.is_read,
                    message.is_starred
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Server copies of an account's sync window that are cached, and how many the server has.
    pub fn sync_totals(&self, account_id: &str) -> Result<(usize, usize)> {
        Ok(self.db.lock().query_row(
            "SELECT (SELECT count(*) FROM remote_messages r JOIN mailbox_sync s ON s.account_id=r.account_id AND s.mailbox=r.mailbox AND s.uid_validity=r.uid_validity WHERE r.account_id=?1),(SELECT COALESCE(sum(server_total),0) FROM mailbox_sync WHERE account_id=?1)",
            [account_id],
            |r| Ok((r.get::<_, i64>(0)? as usize, r.get::<_, i64>(1)? as usize)),
        )?)
    }
    /// The database file size and each account's cached mail by folder.
    pub fn storage(&self) -> Result<(u64, Vec<(String, FolderStorage)>)> {
        let db = self.db.lock();
        let pages: i64 = db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let size: i64 = db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        let mut stmt = db.prepare("SELECT json_extract(data,'$.account_id'),json_extract(data,'$.folder'),count(*),sum(length(CAST(data AS BLOB))) FROM messages GROUP BY 1,2")?;
        let folders = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    FolderStorage {
                        folder: r.get(1)?,
                        messages: r.get::<_, i64>(2)? as usize,
                        bytes: r.get::<_, i64>(3)? as u64,
                    },
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(((pages * size) as u64, folders))
    }
    /// Saves a sync's outcome on the account without touching its other settings; returns false
    /// when the account was removed meanwhile.
    pub fn record_sync(
        &self,
        account_id: &str,
        folders: Option<&Mailboxes>,
        error: Option<&str>,
    ) -> Result<bool> {
        let db = self.db.lock();
        let Some(data) = db
            .query_row("SELECT data FROM accounts WHERE id=?1", [account_id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
        else {
            return Ok(false);
        };
        let mut account: Account = serde_json::from_str(&data)?;
        if let Some(folders) = folders {
            account.folders = folders.clone();
        }
        match error {
            None => {
                account.last_synced = Some(chrono::Utc::now().to_rfc3339());
                account.sync_error = None;
            }
            Some(error) => account.sync_error = Some(error.to_string()),
        }
        db.execute(
            "UPDATE accounts SET data=?2 WHERE id=?1",
            params![account_id, serde_json::to_string(&account)?],
        )?;
        Ok(true)
    }
}

fn forget(tx: &Transaction<'_>, account_id: &str, mailbox: &str) -> Result<()> {
    let ids: HashSet<String> = tx
        .prepare(
            "DELETE FROM remote_messages WHERE account_id=?1 AND mailbox=?2 RETURNING message",
        )?
        .query_map([account_id, mailbox], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    tx.execute(
        "DELETE FROM mailbox_sync WHERE account_id=?1 AND mailbox=?2",
        [account_id, mailbox],
    )?;
    settle(tx, account_id, ids)?;
    Ok(())
}

/// Removes an account's sync records; part of account removal.
pub(crate) fn delete_account(tx: &Transaction<'_>, account_id: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM remote_messages WHERE account_id=?1",
        [account_id],
    )?;
    tx.execute("DELETE FROM mailbox_sync WHERE account_id=?1", [account_id])?;
    tx.execute(
        "DELETE FROM otp_deliveries WHERE account_id=?1",
        [account_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{providers, store::DEMO_ACCOUNT_ID};

    struct Fixture {
        store: Store,
        account: Account,
        _dir: tempfile::TempDir,
    }
    fn fixture() -> Result<Fixture> {
        let dir = tempfile::tempdir()?;
        let store = Store::open(&dir.path().join("mail.db"))?;
        let account = store.add_demo()?;
        for (folder, mailbox) in [
            ("inbox", "INBOX"),
            ("sent", "Sent"),
            ("archive", "All Mail"),
            ("trash", "Trash"),
            ("spam", "Junk"),
        ] {
            store.save_mailbox_state(
                DEMO_ACCOUNT_ID,
                mailbox,
                &MailboxState {
                    folder: folder.into(),
                    uid_validity: Some(1),
                    ..Default::default()
                },
            )?;
        }
        Ok(Fixture {
            store,
            account,
            _dir: dir,
        })
    }
    fn copies(store: &Store, id: &str) -> Result<Vec<(String, u32)>> {
        let db = store.db.lock();
        let mut stmt = db.prepare(
            "SELECT mailbox,uid FROM remote_messages WHERE message=?1 ORDER BY mailbox,uid",
        )?;
        Ok(stmt
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
    impl Fixture {
        /// Imports server copy `uid` of the message `message_id` found in `mailbox`, downloading
        /// its content only when the store asks for it, like a sync.
        fn arrive(
            &self,
            folder: &str,
            mailbox: &str,
            uid: u32,
            message_id: &str,
            read: bool,
        ) -> Result<(bool, Vec<Message>)> {
            let raw = format!(
                "From: Ana <ana@example.test>\r\nTo: alex@form.studio\r\nSubject: Plans\r\nMessage-ID: {message_id}\r\n\r\nSee you there.\r\n"
            );
            let needed = self
                .store
                .needs_content(
                    DEMO_ACCOUNT_ID,
                    folder,
                    mailbox,
                    1,
                    &[(uid, Some(message_id.into()))],
                )?
                .contains(&uid);
            let message = needed
                .then(|| {
                    providers::parse_message(&self.account, folder, mailbox, 1, uid, raw.as_bytes())
                })
                .transpose()?;
            let incoming = Incoming {
                uid,
                message_id: Some(message_id.into()),
                is_read: read,
                is_starred: false,
                date: Some("2026-10-01T10:00:00+00:00".into()),
                message,
            };
            Ok((
                needed,
                self.store
                    .import_copies(DEMO_ACCOUNT_ID, folder, mailbox, 1, None, vec![incoming])?
                    .0,
            ))
        }
    }

    #[test]
    fn copies_in_several_mailboxes_are_one_message_that_follows_its_best_copy() -> Result<()> {
        let f = fixture()?;
        let (needed, added) = f.arrive("inbox", "INBOX", 7, "<plans@x>", false)?;
        assert!(needed && added.len() == 1);
        let id = added[0].id.clone();
        // Gmail's All Mail copy joins the Inbox message without downloading it again.
        let (needed, added) = f.arrive("archive", "All Mail", 70, "<plans@x>", true)?;
        assert!(!needed && added.is_empty());
        assert_eq!(
            copies(&f.store, &id)?,
            [("All Mail".into(), 70), ("INBOX".into(), 7)]
        );
        let message = f.store.message(&id)?;
        assert_eq!(
            (message.folder.as_str(), message.remote_uid, message.is_read),
            ("inbox", Some(7), false)
        );
        // Flags come from the copy the message shows, not from its other copies.
        assert_eq!(
            f.store.apply_flags(
                DEMO_ACCOUNT_ID,
                "All Mail",
                1,
                &[RemoteFlags {
                    uid: 70,
                    is_read: true,
                    is_starred: true
                }]
            )?,
            0
        );
        assert_eq!(
            f.store.apply_flags(
                DEMO_ACCOUNT_ID,
                "INBOX",
                1,
                &[RemoteFlags {
                    uid: 7,
                    is_read: true,
                    is_starred: false
                }]
            )?,
            1
        );
        assert!(f.store.message(&id)?.is_read);
        // Archived elsewhere (the Inbox copy left the server): the message moves to Archive.
        let before = f.store.revision();
        assert_eq!(f.store.remove_copies(DEMO_ACCOUNT_ID, "INBOX", 1, &[7])?, 0);
        let archived = f.store.message(&id)?;
        assert_eq!(
            (
                archived.folder.as_str(),
                archived.remote_mailbox.as_deref(),
                archived.remote_uid
            ),
            ("archive", Some("All Mail"), Some(70))
        );
        assert!(
            archived.is_read && archived.is_starred,
            "the copy now shown brings its own flags"
        );
        assert_ne!(f.store.revision(), before);
        // With its last copy gone, the message is gone.
        assert_eq!(
            f.store
                .remove_copies(DEMO_ACCOUNT_ID, "All Mail", 1, &[70])?,
            1
        );
        assert!(f.store.message(&id).is_err());
        Ok(())
    }

    #[test]
    fn junk_copies_quarantine_normal_copies_regardless_of_sync_order() -> Result<()> {
        for junk_first in [false, true] {
            let f = fixture()?;
            let locations = if junk_first {
                [("spam", "Junk", 5, true), ("inbox", "INBOX", 7, false)]
            } else {
                [("inbox", "INBOX", 7, false), ("spam", "Junk", 5, true)]
            };
            let (_, added) = f.arrive(
                locations[0].0,
                locations[0].1,
                locations[0].2,
                "<junk@x>",
                locations[0].3,
            )?;
            let id = added[0].id.clone();
            for (folder, mailbox, uid, read) in [
                locations[1],
                ("archive", "All Mail", 70, false),
                ("sent", "Sent", 4, false),
                ("trash", "Trash", 6, false),
            ] {
                let (needed, added) = f.arrive(folder, mailbox, uid, "<junk@x>", read)?;
                assert!(!needed && added.is_empty());
            }
            let spam = f.store.message(&id)?;
            assert_eq!(
                (spam.folder.as_str(), spam.remote_uid, spam.is_read),
                ("spam", Some(5), true)
            );
            assert_eq!(
                f.store.apply_flags(
                    DEMO_ACCOUNT_ID,
                    "INBOX",
                    1,
                    &[RemoteFlags {
                        uid: 7,
                        is_read: false,
                        is_starred: true,
                    }]
                )?,
                0
            );
            f.store.apply_flags(
                DEMO_ACCOUNT_ID,
                "Junk",
                1,
                &[RemoteFlags {
                    uid: 5,
                    is_read: false,
                    is_starred: true,
                }],
            )?;
            for folder in [
                None,
                Some("inbox"),
                Some("sent"),
                Some("archive"),
                Some("starred"),
            ] {
                let query = MessageQuery {
                    folder: folder.map(str::to_string),
                    q: Some("Plans".into()),
                    ..Default::default()
                };
                assert_eq!(f.store.list(&query)?.total, 0);
                assert_eq!(f.store.threads(&query)?.unread_total, 0);
            }
            let spam_page = f.store.threads(&MessageQuery {
                folder: Some("spam".into()),
                ..Default::default()
            })?;
            assert_eq!(spam_page.total, 1);
            assert_eq!(spam_page.unread_total, 1);
            assert_eq!(spam_page.threads[0].count, 1);
            assert_eq!(
                f.store
                    .remove_copies(DEMO_ACCOUNT_ID, "All Mail", 1, &[70])?,
                0
            );
            assert_eq!(f.store.message(&id)?.folder, "spam");
            // Removing Junk on the provider restores the best remaining normal copy and its flags.
            assert_eq!(f.store.remove_copies(DEMO_ACCOUNT_ID, "Junk", 1, &[5])?, 0);
            let restored = f.store.message(&id)?;
            assert_eq!(
                (restored.folder.as_str(), restored.remote_uid),
                ("inbox", Some(7))
            );
            assert!(!restored.is_read && restored.is_starred);
        }
        Ok(())
    }

    #[test]
    fn spam_move_without_destination_uid_stays_quarantined_until_sync_and_can_restore() -> Result<()>
    {
        let f = fixture()?;
        let (_, added) = f.arrive("inbox", "INBOX", 7, "<moved-junk@x>", false)?;
        let id = added[0].id.clone();
        f.arrive("archive", "All Mail", 70, "<moved-junk@x>", false)?;
        // A successful MOVE without COPYUID, whose Message-ID lookup did not find its UID yet.
        let mut moved = f.store.message(&id)?;
        moved.folder = "spam".into();
        moved.remote_mailbox = Some("Junk".into());
        moved.remote_uid = None;
        f.store.save_message(&moved)?;
        f.store.record_move(&moved, ("INBOX", 1, 7))?;
        assert!(f.store.known_uids(DEMO_ACCOUNT_ID, "INBOX", 1)?.is_empty());
        assert!(
            !f.arrive("archive", "All Mail", 70, "<moved-junk@x>", true)?
                .0
        );
        assert_eq!(f.store.message(&id)?.folder, "spam");
        f.store
            .remove_copies(DEMO_ACCOUNT_ID, "All Mail", 1, &[70])?;
        assert_eq!(f.store.message(&id)?.folder, "spam");
        let (needed, added) = f.arrive("spam", "Junk", 5, "<moved-junk@x>", false)?;
        assert!(
            !needed && added.is_empty(),
            "Junk sync resolves the moved message, not a duplicate"
        );
        assert_eq!(f.store.message(&id)?.remote_uid, Some(5));
        let mut restored = f.store.message(&id)?;
        restored.folder = "inbox".into();
        restored.remote_mailbox = Some("INBOX".into());
        restored.remote_uid = Some(8);
        f.store.save_message(&restored)?;
        f.store.record_move(&restored, ("Junk", 1, 5))?;
        assert!(f.store.known_uids(DEMO_ACCOUNT_ID, "Junk", 1)?.is_empty());
        assert!(
            !f.arrive("archive", "All Mail", 71, "<moved-junk@x>", true)?
                .0
        );
        assert_eq!(f.store.message(&id)?.folder, "inbox");
        assert_eq!(f.store.known_uids(DEMO_ACCOUNT_ID, "INBOX", 1)?, [8]);
        assert_eq!(f.store.conversation(&id)?.len(), 1);
        Ok(())
    }

    #[test]
    fn deliveries_reusing_a_message_id_stay_apart_and_copies_needing_content_are_reported()
    -> Result<()> {
        let f = fixture()?;
        let (_, first) = f.arrive("inbox", "INBOX", 1, "<reused@x>", false)?;
        let (needed, second) = f.arrive("inbox", "INBOX", 2, "<reused@x>", false)?;
        assert!(
            needed && second.len() == 1 && second[0].id != first[0].id,
            "a second UID in the same mailbox is another message"
        );
        // A copy whose message vanished between asking and storing is reported, not dropped.
        let lone = Incoming {
            uid: 9,
            message_id: Some("<new@x>".into()),
            is_read: false,
            is_starred: false,
            date: None,
            message: None,
        };
        let (added, skipped) =
            f.store
                .import_copies(DEMO_ACCOUNT_ID, "inbox", "INBOX", 1, None, vec![lone])?;
        assert!(added.is_empty());
        assert_eq!(skipped, [9]);
        assert!(
            !f.store
                .known_uids(DEMO_ACCOUNT_ID, "INBOX", 1)?
                .contains(&9)
        );
        Ok(())
    }

    #[test]
    fn sent_copies_join_deliveries_but_mail_to_yourself_is_also_in_the_inbox() -> Result<()> {
        let f = fixture()?;
        let draft = Compose {
            id: uuid::Uuid::new_v4().to_string(),
            account_id: DEMO_ACCOUNT_ID.into(),
            to: vec!["alex@form.studio".into()],
            subject: "Plans".into(),
            body: "Sent by Alice".into(),
            in_reply_to: None,
        };
        f.store.save_draft(&draft)?;
        f.store.claim_delivery(&draft.id)?;
        f.store.finish_delivery(&draft.id, true)?;
        let identity = f.store.message(&draft.id)?.message_id.unwrap();
        let (needed, _) = f.arrive("sent", "Sent", 3, &identity, true)?;
        assert!(
            !needed,
            "the provider's Sent copy of a delivery is not downloaded"
        );
        let sent = f.store.message(&draft.id)?;
        assert_eq!(
            (sent.folder.as_str(), sent.remote_uid, sent.body.as_str()),
            ("sent", Some(3), "Sent by Alice")
        );
        let (needed, added) = f.arrive("inbox", "INBOX", 9, &identity, false)?;
        assert!(
            needed && added.len() == 1 && added[0].id != draft.id,
            "received copy of mail to yourself is its own Inbox message"
        );
        // All Mail's copy joins a cached message instead of adding an Archive duplicate.
        assert!(!f.arrive("archive", "All Mail", 90, &identity, true)?.0);
        assert_eq!(
            f.store
                .list(&MessageQuery {
                    folder: Some("archive".into()),
                    ..Default::default()
                })?
                .total,
            0
        );
        // A Sent copy seen before its All Mail copy's message turns that message into Sent mail.
        let (_, added) = f.arrive("archive", "All Mail", 91, "<older@x>", true)?;
        assert_eq!(added[0].folder, "archive");
        assert!(!f.arrive("sent", "Sent", 4, "<older@x>", true)?.0);
        assert_eq!(f.store.message(&added[0].id)?.folder, "sent");
        // A delivery whose first server copy is in Trash (trashed before it synced) shows in Trash.
        let trashed = Compose {
            id: uuid::Uuid::new_v4().to_string(),
            subject: "Oops".into(),
            ..draft
        };
        f.store.save_draft(&trashed)?;
        f.store.claim_delivery(&trashed.id)?;
        f.store.finish_delivery(&trashed.id, true)?;
        let identity = f.store.message(&trashed.id)?.message_id.unwrap();
        assert!(!f.arrive("trash", "Trash", 6, &identity, true)?.0);
        assert_eq!(f.store.message(&trashed.id)?.folder, "trash");
        Ok(())
    }

    #[test]
    fn forgetting_a_mailbox_or_account_drops_its_copies_and_lone_messages() -> Result<()> {
        let f = fixture()?;
        let (_, inbox) = f.arrive("inbox", "INBOX", 1, "<a@x>", false)?;
        f.arrive("archive", "All Mail", 10, "<a@x>", false)?;
        let (_, trash) = f.arrive("trash", "Trash", 5, "<b@x>", false)?;
        f.store.forget_mailbox(DEMO_ACCOUNT_ID, "INBOX")?;
        assert_eq!(f.store.message(&inbox[0].id)?.folder, "archive");
        assert_eq!(
            f.store.known_uids(DEMO_ACCOUNT_ID, "INBOX", 1)?,
            Vec::<u32>::new()
        );
        assert!(f.store.mailbox_state(DEMO_ACCOUNT_ID, "INBOX")?.is_none());
        f.store
            .forget_other_mailboxes(DEMO_ACCOUNT_ID, &["All Mail".into()])?;
        assert!(f.store.message(&trash[0].id).is_err());
        assert_eq!(f.store.sync_totals(DEMO_ACCOUNT_ID)?, (1, 0));
        // The demo inbox can only be removed once a real account exists.
        f.store.save_account(&Account {
            id: "work".into(),
            is_demo: false,
            ..f.account.clone()
        })?;
        f.store.remove_account(DEMO_ACCOUNT_ID)?;
        assert_eq!(
            f.store.known_uids(DEMO_ACCOUNT_ID, "All Mail", 1)?,
            Vec::<u32>::new()
        );
        assert!(
            f.arrive("inbox", "INBOX", 2, "<c@x>", false).is_err(),
            "a removed account takes no mail"
        );
        Ok(())
    }

    #[test]
    fn earlier_caches_are_adopted_on_open_and_their_missing_html_downloaded_again() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("mail.db");
        let (account, legacy, current_id, sent_id) = {
            let store = Store::open(&path)?;
            let account = store.add_demo()?;
            let raw = |id: &str| {
                format!(
                    "From: Ana <ana@example.test>\r\nSubject: Plans\r\nMessage-ID: {id}\r\nDate: Thu, 01 Oct 2026 10:00:00 +0000\r\n\r\nBody\r\n"
                )
            };
            let mut current = providers::parse_message(
                &account,
                "inbox",
                "INBOX",
                1,
                1,
                raw("<current@x>").as_bytes(),
            )?;
            current.id = "old-current".into();
            current.is_read = false;
            current.date = "2026-10-01T10:00:00+00:00".into();
            let mut legacy = providers::parse_message(
                &account,
                "inbox",
                "INBOX",
                1,
                2,
                raw("<legacy@x>").as_bytes(),
            )?;
            legacy.id = "old-legacy".into();
            legacy.date = "2026-10-02T10:00:00+00:00".into();
            legacy.is_read = false;
            let mut sent = providers::parse_message(
                &account,
                "sent",
                "Sent",
                1,
                3,
                raw("<mine@x>").as_bytes(),
            )?;
            sent.id = "old-sent".into();
            sent.date = "2026-10-03T10:00:00+00:00".into();
            sent.is_read = true;
            let mut duplicate = providers::parse_message(
                &account,
                "archive",
                "All Mail",
                1,
                30,
                raw("<mine@x>").as_bytes(),
            )?;
            duplicate.id = "all-mail-duplicate".into();
            store.save_messages([current.clone(), legacy.clone(), sent.clone(), duplicate])?;
            assert!(store.claim_delivery(&sent.id)?);
            store.finish_delivery(&sent.id, true)?;
            store.save_mailbox_state(
                &account.id,
                "INBOX",
                &MailboxState {
                    folder: "inbox".into(),
                    uid_validity: Some(1),
                    uid_next: Some(3),
                    message_count: Some(2),
                    highest_modseq: Some(7),
                    window_since: Some("all".into()),
                    server_total: 2,
                    flags_checked: Some("2026-10-03T10:00:00+00:00".into()),
                },
            )?;
            drop(store);
            let db = Connection::open(&path)?;
            db.execute_batch("DELETE FROM remote_messages; DELETE FROM schema_parts WHERE name='remote_messages';")?;
            // Recreate the historical derived layouts, not another production migration path.
            db.execute_batch(
                "CREATE TABLE conversation_index(message INTEGER PRIMARY KEY, id TEXT NOT NULL, account_id TEXT, subject TEXT, folder TEXT, from_name TEXT, from_email TEXT, message_id TEXT, is_read INTEGER, is_starred INTEGER, julian REAL);
                INSERT INTO conversation_index SELECT message,id,account_id,subject,folder,from_name,from_email,message_id,is_read,is_starred,julian FROM message_index;
                CREATE INDEX conversation_index_account ON conversation_index(account_id,julian);
                CREATE TRIGGER conversation_index_insert AFTER INSERT ON messages BEGIN
                    INSERT INTO conversation_index SELECT new.rowid,new.id,json_extract(new.data,'$.account_id'),json_extract(new.data,'$.subject'),json_extract(new.data,'$.folder'),json_extract(new.data,'$.from_name'),json_extract(new.data,'$.from_email'),json_extract(new.data,'$.message_id'),json_extract(new.data,'$.is_read'),json_extract(new.data,'$.is_starred'),julianday(json_extract(new.data,'$.date'));
                END;
                CREATE TRIGGER conversation_index_update AFTER UPDATE ON messages BEGIN
                    DELETE FROM conversation_index WHERE message=old.rowid;
                    INSERT INTO conversation_index SELECT new.rowid,new.id,json_extract(new.data,'$.account_id'),json_extract(new.data,'$.subject'),json_extract(new.data,'$.folder'),json_extract(new.data,'$.from_name'),json_extract(new.data,'$.from_email'),json_extract(new.data,'$.message_id'),json_extract(new.data,'$.is_read'),json_extract(new.data,'$.is_starred'),julianday(json_extract(new.data,'$.date'));
                END;
                CREATE TRIGGER conversation_index_delete AFTER DELETE ON messages BEGIN DELETE FROM conversation_index WHERE message=old.rowid; END;
                INSERT INTO schema_parts VALUES('conversation_index',2);
                DROP TRIGGER message_index_insert;
                DROP TRIGGER message_index_update;
                ALTER TABLE message_index DROP COLUMN message_id;
                PRAGMA user_version=1;
                ALTER TABLE mailbox_sync ADD COLUMN checked TEXT;
                UPDATE mailbox_sync SET checked='2026-10-03T10:00:00+00:00';"
            )?;
            db.execute(
                "UPDATE messages SET data=json_remove(data,'$.html_body') WHERE id=?1",
                [&legacy.id],
            )?;
            (account, legacy, current.id, sent.id)
        };
        let store = Store::open(&path)?;
        assert_eq!(
            store.known_uids(&account.id, "INBOX", 1)?,
            [1],
            "a cache without HTML bodies is not adopted"
        );
        assert_eq!(store.known_uids(&account.id, "Sent", 1)?, [3]);
        assert_eq!(store.known_uids(&account.id, "All Mail", 1)?, [30]);
        assert!(
            store.message("all-mail-duplicate").is_err(),
            "All Mail's copy of sent mail merges into the Sent message"
        );
        let plans = |store: &Store, q: &str| -> Result<Vec<String>> {
            Ok(store
                .list(&MessageQuery {
                    account_id: Some(account.id.clone()),
                    q: Some(q.into()),
                    ..Default::default()
                })?
                .messages
                .into_iter()
                .map(|m| m.id)
                .collect())
        };
        let thread = |store: &Store| -> Result<Vec<String>> {
            Ok(store
                .conversation(&legacy.id)?
                .into_iter()
                .map(|m| m.id)
                .collect())
        };
        assert_eq!(
            thread(&store)?,
            [current_id.clone(), legacy.id.clone(), sent_id.clone()]
        );
        assert_eq!(
            plans(&store, "plans")?,
            [sent_id.clone(), legacy.id.clone(), current_id.clone()]
        );
        assert_eq!(
            plans(&store, "plans is:unread")?,
            [legacy.id.clone(), current_id.clone()]
        );
        assert!(!store.claim_delivery(&sent_id)?);
        let mut state = store.mailbox_state(&account.id, "INBOX")?.unwrap();
        assert_eq!(
            (
                state.uid_validity,
                state.uid_next,
                state.message_count,
                state.highest_modseq,
                state.server_total
            ),
            (Some(1), Some(3), Some(2), Some(7), 2)
        );
        assert_eq!(state.window_since.as_deref(), Some("all"));
        assert_eq!(
            state.flags_checked.as_deref(),
            Some("2026-10-03T10:00:00+00:00")
        );
        state.uid_next = Some(4);
        store.save_mailbox_state(&account.id, "INBOX", &state)?;
        store.save_mailbox_state(
            &account.id,
            "New",
            &MailboxState {
                folder: "archive".into(),
                ..Default::default()
            },
        )?;
        assert!(
            store
                .needs_content(
                    &account.id,
                    "inbox",
                    "INBOX",
                    1,
                    &[(2, legacy.message_id.clone())]
                )?
                .contains(&2)
        );
        let mut refreshed = providers::parse_message(&account, "inbox", "INBOX", 1, 2, b"From: Ana <ana@example.test>\r\nSubject: Plans\r\nMessage-ID: <legacy@x>\r\nContent-Type: text/html\r\n\r\n<p>Body</p>\r\n")?;
        refreshed.id = "ignored".into();
        let added = store.import_copies(
            &account.id,
            "inbox",
            "INBOX",
            1,
            None,
            vec![Incoming {
                uid: 2,
                message_id: legacy.message_id.clone(),
                is_read: true,
                is_starred: false,
                date: Some(legacy.date.clone()),
                message: Some(refreshed),
            }],
        )?;
        assert!(added.0.is_empty() && added.1.is_empty());
        let message = store.message(&legacy.id)?;
        assert_eq!(
            (message.html_body.as_deref().map(str::trim), message.is_read),
            (Some("<p>Body</p>"), true)
        );
        assert_eq!(
            plans(&store, "plans is:unread")?,
            std::slice::from_ref(&current_id)
        );
        store.patch(
            &legacy.id,
            &MessagePatch {
                is_starred: Some(true),
                ..Default::default()
            },
        )?;
        assert_eq!(
            plans(&store, "plans is:starred")?,
            std::slice::from_ref(&legacy.id)
        );
        let mut reply = store.save_draft(&Compose {
            id: uuid::Uuid::new_v4().to_string(),
            account_id: account.id.clone(),
            to: vec!["ana@example.test".into()],
            subject: "Re: Plans".into(),
            body: "A new reply".into(),
            in_reply_to: legacy.message_id.clone(),
        })?;
        reply.date = "2026-10-04T10:00:00+00:00".into();
        store.save_message(&reply)?;
        assert!(store.claim_delivery(&reply.id)?);
        store.finish_delivery(&reply.id, true)?;
        assert_eq!(
            thread(&store)?,
            [
                current_id.clone(),
                legacy.id.clone(),
                sent_id.clone(),
                reply.id.clone()
            ]
        );
        assert!(plans(&store, "plans")?.contains(&reply.id));
        assert_eq!(store.remove_copies(&account.id, "INBOX", 1, &[1])?, 1);
        assert_eq!(
            thread(&store)?,
            [legacy.id.clone(), sent_id.clone(), reply.id.clone()]
        );
        assert!(!plans(&store, "plans")?.contains(&current_id));
        let expected_search = plans(&store, "plans")?;
        drop(store);
        let store = Store::open(&path)?;
        assert_eq!(
            thread(&store)?,
            [legacy.id.clone(), sent_id.clone(), reply.id.clone()]
        );
        assert_eq!(plans(&store, "plans")?, expected_search);
        assert_eq!(
            plans(&store, "plans is:starred")?,
            std::slice::from_ref(&legacy.id)
        );
        assert!(!store.claim_delivery(&sent_id)?);
        assert!(!store.claim_delivery(&reply.id)?);
        assert_eq!(store.mailbox_state(&account.id, "INBOX")?, Some(state));
        assert_eq!(
            store.mailbox_state(&account.id, "New")?.unwrap().folder,
            "archive"
        );
        Ok(())
    }

    #[test]
    fn storage_counts_each_accounts_folders() -> Result<()> {
        let f = fixture()?;
        f.arrive("inbox", "INBOX", 1, "<a@x>", false)?;
        let (database, folders) = f.store.storage()?;
        let (_, inbox) = folders
            .iter()
            .find(|(account, usage)| account == DEMO_ACCOUNT_ID && usage.folder == "inbox")
            .unwrap();
        assert_eq!(inbox.messages, 14, "thirteen demo messages and the new one");
        assert!(inbox.bytes > 0 && database >= inbox.bytes);
        Ok(())
    }
}
