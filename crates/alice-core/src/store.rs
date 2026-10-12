use crate::{model::*, search};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{Connection, Transaction, params};
use std::{collections::HashSet, path::Path};
use uuid::Uuid;

pub struct Store {
    pub(crate) db: Mutex<Connection>,
    /// Separates this process's change counter from an earlier daemon's; see `revision`.
    epoch: i64,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let mut db = Connection::open(path)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS accounts(id TEXT PRIMARY KEY, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS messages(id TEXT PRIMARY KEY, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS settings(id TEXT PRIMARY KEY, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS deliveries(id TEXT PRIMARY KEY, status TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS otp_deliveries(id TEXT PRIMARY KEY, account_id TEXT NOT NULL, received_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, data TEXT, status TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS otp_pending ON otp_deliveries(status,expires_at);
            CREATE TABLE IF NOT EXISTS dav_sources(id TEXT PRIMARY KEY, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS contacts(owner TEXT NOT NULL, source TEXT NOT NULL, email TEXT NOT NULL, name TEXT NOT NULL, PRIMARY KEY(owner, source, email));
            CREATE INDEX IF NOT EXISTS message_scope ON messages(json_extract(data,'$.folder'),json_extract(data,'$.account_id'),json_extract(data,'$.category'),json_extract(data,'$.is_read'),julianday(json_extract(data,'$.date')));
            CREATE INDEX IF NOT EXISTS message_identity ON messages(json_extract(data,'$.account_id'),json_extract(data,'$.message_id'));
            CREATE INDEX IF NOT EXISTS message_starred ON messages(json_extract(data,'$.is_starred'),json_extract(data,'$.folder'));
            CREATE INDEX IF NOT EXISTS message_remote ON messages(json_extract(data,'$.account_id'),json_extract(data,'$.remote_mailbox'),json_extract(data,'$.remote_uid'),json_extract(data,'$.uid_validity'));",
        )?;
        ensure_search_index(&mut db)?;
        // Sync's legacy-copy merge deletes messages through the shared index/FTS triggers.
        crate::sync_store::migrate(&mut db)?;
        // A legacy enabled toggle is not consent to the new sending capability.
        db.execute(
            "UPDATE settings SET data=json_set(data,'$.mcp',CASE json_type(data,'$.mcp') WHEN 'true' THEN 'read_only' ELSE 'off' END)
             WHERE id='preferences' AND json_type(data,'$.mcp') IN ('true','false')",
            [],
        )?;
        let store = Self {
            db: Mutex::new(db),
            epoch: Utc::now().timestamp_micros(),
        };
        // Earlier builds could add sample accounts that no longer exist.
        {
            let mut db = store.db.lock();
            let tx = db.transaction()?;
            for legacy in ["demo", "demo-work", "demo-personal"] {
                delete_account(&tx, legacy)?;
            }
            tx.commit()?;
        }
        Ok(store)
    }
    pub fn accounts(&self) -> Result<Vec<Account>> {
        let db = self.db.lock();
        let mut stmt = db.prepare("SELECT data FROM accounts ORDER BY rowid")?;
        stmt.query_map([], |row| row.get::<_, String>(0))?
            .map(|r| Ok(serde_json::from_str(&r?)?))
            .collect()
    }
    pub fn account(&self, id: &str) -> Result<Account> {
        let data: String = self
            .db
            .lock()
            .query_row("SELECT data FROM accounts WHERE id=?1", [id], |r| r.get(0))
            .context(crate::text!("server.error.account_not_found"))?;
        Ok(serde_json::from_str(&data)?)
    }
    /// Changes whenever stored data may have changed, so clients can skip refetching an unchanged view.
    pub fn revision(&self) -> String {
        revision(self.epoch, &self.db.lock())
    }
    pub fn save_account(&self, account: &Account) -> Result<()> {
        self.db.lock().execute(
            "INSERT INTO accounts VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            params![account.id, serde_json::to_string(account)?],
        )?;
        Ok(())
    }
    pub fn list(&self, query: &MessageQuery) -> Result<MessagePage> {
        let db = self.db.lock();
        let revision = revision(self.epoch, &db);
        let (scope, values) =
            scope_filter(query, |field| format!("json_extract(data,'$.{field}')"));
        let order = "ORDER BY julianday(json_extract(data,'$.date')) DESC,id ASC";
        let text = query.q.as_deref().unwrap_or("").trim();
        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(100).clamp(1, 200);
        let (messages, all_total, unread_total) = if text.is_empty() {
            let (all, unread): (usize, usize) = db.query_row(
                &format!("SELECT COUNT(*),COALESCE(SUM(json_extract(data,'$.is_read')=0),0) FROM messages WHERE {scope}"),
                rusqlite::params_from_iter(&values), |r| Ok((r.get::<_,i64>(0)? as usize,r.get::<_,i64>(1)? as usize)))?;
            let read_filter = if query.unread == Some(true) {
                " AND json_extract(data,'$.is_read')=0"
            } else {
                ""
            };
            let mut stmt = db.prepare(&format!("SELECT data FROM messages WHERE id IN (SELECT id FROM messages WHERE {scope}{read_filter} {order} LIMIT {limit} OFFSET {offset}) {order}"))?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(&values), |r| {
                    r.get::<_, String>(0)
                })?
                .map(|r| Ok(serde_json::from_str(&r?)?))
                .collect::<Result<Vec<Message>>>()?;
            (rows, all, unread)
        } else {
            let hits = search(&db, query, text)?;
            let unread = hits.iter().filter(|hit| !hit.is_read).count();
            let mut body = db.prepare_cached("SELECT data FROM messages WHERE id=?1")?;
            let rows = hits
                .iter()
                .filter(|hit| query.unread != Some(true) || !hit.is_read)
                .skip(offset)
                .take(limit)
                .map(|hit| {
                    Ok(serde_json::from_str(
                        &body.query_row([&hit.id], |r| r.get::<_, String>(0))?,
                    )?)
                })
                .collect::<Result<Vec<Message>>>()?;
            (rows, hits.len(), unread)
        };
        Ok(MessagePage {
            messages,
            total: if query.unread == Some(true) {
                unread_total
            } else {
                all_total
            },
            all_total,
            unread_total,
            revision,
        })
    }
    /// Conversations in the queried view, newest first. Messages of an account belong to one conversation
    /// when their subjects match after reply and forward prefixes are removed; drafts stand alone.
    pub fn threads(&self, query: &MessageQuery) -> Result<ThreadPage> {
        let accounts = self.accounts()?;
        let db = self.db.lock();
        let revision = revision(self.epoch, &db);
        let (scope, values) =
            scope_filter(query, |field| format!("json_extract(data,'$.{field}')"));
        let text = query.q.as_deref().unwrap_or("").trim();
        // Search ranks conversations by their best-matching message; grouping still walks newest first.
        let mut scores = std::collections::HashMap::new();
        let matched: Vec<String> = if text.is_empty() {
            let mut ids = db.prepare(&format!("SELECT id FROM messages WHERE {scope} ORDER BY julianday(json_extract(data,'$.date')) DESC,id ASC"))?;
            ids.query_map(rusqlite::params_from_iter(&values), |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        } else {
            let mut hits = search(&db, query, text)?;
            hits.sort_by(|a, b| b.julian.total_cmp(&a.julian).then_with(|| a.id.cmp(&b.id)));
            hits.into_iter()
                .map(|hit| {
                    scores.insert(hit.id.clone(), hit.score);
                    hit.id
                })
                .collect()
        };
        let rows = thread_rows(&db, query.account_id.as_deref())?;
        let by_id: std::collections::HashMap<&str, &ThreadRow> =
            rows.iter().map(|r| (r.id.as_str(), r)).collect();
        let mut members: std::collections::HashMap<(&str, &str), Vec<&ThreadRow>> =
            std::collections::HashMap::new();
        for row in &rows {
            if !row.key.is_empty() {
                members
                    .entry((&row.account_id, &row.key))
                    .or_default()
                    .push(row);
            }
        }
        // Group the view's messages, keeping the newest-first order of each conversation's newest message.
        let mut order: Vec<(String, Vec<&ThreadRow>)> = Vec::new();
        let mut position = std::collections::HashMap::new();
        for id in &matched {
            let Some(row) = by_id.get(id.as_str()) else {
                continue;
            };
            let thread = if row.folder == "drafts" || row.key.is_empty() {
                format!("message:{}", row.id)
            } else {
                format!("{}:{}", row.account_id, row.key)
            };
            let index = *position.entry(thread.clone()).or_insert_with(|| {
                order.push((thread, Vec::new()));
                order.len() - 1
            });
            order[index].1.push(*row);
        }
        if !scores.is_empty() {
            let best =
                |rows: &[&ThreadRow]| rows.iter().map(|r| scores[&r.id]).fold(f64::MIN, f64::max);
            order.sort_by(|a, b| best(&b.1).total_cmp(&best(&a.1)));
        }
        let all_total = order.len();
        let unread: Vec<bool> = order
            .iter()
            .map(|(_, rows)| rows.iter().any(|r| !r.is_read))
            .collect();
        let unread_total = unread.iter().filter(|u| **u).count();
        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(100).clamp(1, 200);
        let mut body = db.prepare("SELECT data FROM messages WHERE id=?1")?;
        let mut threads = Vec::new();
        for ((id, listed), _) in order
            .into_iter()
            .zip(unread)
            .filter(|(_, unread)| query.unread != Some(true) || *unread)
            .skip(offset)
            .take(limit)
        {
            let newest = listed[0];
            let own: HashSet<&str> = listed.iter().map(|r| r.id.as_str()).collect();
            let conversation = if id.starts_with("message:") {
                vec![newest]
            } else {
                conversation(
                    &members[&(newest.account_id.as_str(), newest.key.as_str())],
                    query.folder.as_deref(),
                    |r| own.contains(r.id.as_str()),
                )
            };
            let email = accounts
                .iter()
                .find(|a| a.id == newest.account_id)
                .map(|a| a.email.to_lowercase())
                .unwrap_or_default();
            let mut participants: Vec<String> = Vec::new();
            for row in &conversation {
                let name = if row.folder == "sent" || row.from_email.eq_ignore_ascii_case(&email) {
                    "me".to_string()
                } else if row.from_name.trim().is_empty() {
                    row.from_email.clone()
                } else {
                    row.from_name.clone()
                };
                if !participants.contains(&name) {
                    participants.push(name);
                }
            }
            threads.push(ThreadSummary {
                id,
                message: serde_json::from_str(
                    &body.query_row([&newest.id], |r| r.get::<_, String>(0))?,
                )?,
                ids: listed.iter().map(|r| r.id.clone()).collect(),
                unread_ids: conversation
                    .iter()
                    .filter(|r| !r.is_read)
                    .map(|r| r.id.clone())
                    .collect(),
                count: conversation.len(),
                participants,
                is_starred: conversation.iter().any(|r| r.is_starred),
            });
        }
        Ok(ThreadPage {
            threads,
            total: if query.unread == Some(true) {
                unread_total
            } else {
                all_total
            },
            all_total,
            unread_total,
            revision,
        })
    }
    /// Every message in the conversation of message `id`, oldest first. Spam is isolated from normal
    /// mail; Trash is included only for a Trash message. Provider copies appear once.
    pub fn conversation(&self, id: &str) -> Result<Vec<Message>> {
        let message = self.message(id)?;
        let key = thread_key(&message.subject);
        if message.folder == "drafts" || key.is_empty() {
            return Ok(vec![message]);
        }
        let db = self.db.lock();
        let rows = thread_rows(&db, Some(&message.account_id))?;
        let members: Vec<&ThreadRow> = rows.iter().filter(|r| r.key == key).collect();
        let mut body = db.prepare("SELECT data FROM messages WHERE id=?1")?;
        conversation(&members, Some(&message.folder), |r| r.id == id)
            .into_iter()
            .map(|r| {
                Ok(serde_json::from_str(
                    &body.query_row([&r.id], |row| row.get::<_, String>(0))?,
                )?)
            })
            .collect()
    }
    pub fn message(&self, id: &str) -> Result<Message> {
        let data: String = self
            .db
            .lock()
            .query_row("SELECT data FROM messages WHERE id=?1", [id], |r| r.get(0))
            .context(crate::text!("server.error.message_not_found"))?;
        Ok(serde_json::from_str(&data)?)
    }
    pub fn save_message(&self, message: &Message) -> Result<()> {
        self.save_messages([message.clone()])
    }
    /// Saves messages in one transaction.
    pub fn save_messages(&self, messages: impl IntoIterator<Item = Message>) -> Result<()> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        for message in messages {
            upsert(&tx, message)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn patch(&self, id: &str, patch: &MessagePatch) -> Result<Message> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        let data: String = tx
            .query_row("SELECT data FROM messages WHERE id=?1", [id], |r| r.get(0))
            .context(crate::text!("server.error.message_not_found"))?;
        let mut message: Message = serde_json::from_str(&data)?;
        if let Some(read) = patch.is_read {
            message.is_read = read;
        }
        if let Some(starred) = patch.is_starred {
            message.is_starred = starred;
        }
        if let Some(folder) = &patch.folder {
            if !["inbox", "archive", "trash", "spam"].contains(&folder.as_str())
                || ["drafts", "sent"].contains(&message.folder.as_str())
            {
                bail!(crate::text!("server.error.move_folder"));
            }
            message.folder = folder.clone();
        }
        tx.execute(
            "UPDATE messages SET data=?2 WHERE id=?1",
            params![id, serde_json::to_string(&message)?],
        )?;
        tx.commit()?;
        Ok(message)
    }
    pub fn save_draft(&self, draft: &Compose) -> Result<Message> {
        Uuid::parse_str(&draft.id).context(crate::text!("server.error.draft_uuid"))?;
        let account = self.sending_account(&draft.account_id)?;
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        let existing: Option<String> = tx
            .query_row("SELECT data FROM messages WHERE id=?1", [&draft.id], |r| {
                r.get(0)
            })
            .ok();
        if let Some(existing) = existing {
            let m: Message = serde_json::from_str(&existing)?;
            if m.folder != "drafts" || m.account_id != account.id {
                bail!(crate::text!("server.error.overwrite_message"));
            }
        }
        if tx
            .query_row(
                "SELECT status FROM deliveries WHERE id=?1",
                [&draft.id],
                |r| r.get::<_, String>(0),
            )
            .is_ok()
        {
            bail!(crate::text!("server.error.draft_submitted"));
        }
        let message = Message {
            id: draft.id.clone(),
            account_id: account.id,
            from_name: account.name,
            from_email: account.email,
            to: draft.to.clone(),
            reply_to: vec![],
            subject: draft.subject.clone(),
            preview: preview(&draft.body),
            body: draft.body.clone(),
            html_body: None,
            date: Utc::now().to_rfc3339(),
            folder: "drafts".into(),
            is_read: true,
            is_starred: false,
            category: "personal".into(),
            attachments: vec![],
            message_id: Some(format!("<{}@alice.local>", draft.id)),
            in_reply_to: draft.in_reply_to.clone(),
            remote_mailbox: None,
            remote_uid: None,
            uid_validity: None,
            invite: None,
        };
        tx.execute(
            "INSERT INTO messages VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            params![message.id, serde_json::to_string(&message)?],
        )?;
        tx.commit()?;
        Ok(message)
    }
    pub fn claim_delivery(&self, id: &str) -> Result<bool> {
        let db = self.db.lock();
        let previous = db.query_row("SELECT status FROM deliveries WHERE id=?1", [id], |r| {
            r.get::<_, String>(0)
        });
        if let Ok(status) = previous {
            if status == "sent" {
                return Ok(false);
            }
            bail!(crate::text!("server.error.delivery_pending_create"));
        }
        db.execute("INSERT INTO deliveries VALUES(?1,'pending')", [id])?;
        Ok(true)
    }
    pub fn discard_draft(&self, id: &str) -> Result<()> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        let data: String = tx
            .query_row("SELECT data FROM messages WHERE id=?1", [id], |r| r.get(0))
            .context(crate::text!("server.error.message_not_found"))?;
        let message: Message = serde_json::from_str(&data)?;
        if message.folder != "drafts" {
            bail!(crate::text!("server.error.discard_unsent"));
        }
        if tx
            .query_row("SELECT status FROM deliveries WHERE id=?1", [id], |r| {
                r.get::<_, String>(0)
            })
            .is_ok()
        {
            bail!(crate::text!("server.error.delivery_pending_discard"));
        }
        tx.execute("DELETE FROM messages WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
    /// Deletes an account and everything cached for it on this device. Provider mail is untouched.
    pub fn remove_account(&self, id: &str) -> Result<Account> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        let data: String = tx
            .query_row("SELECT data FROM accounts WHERE id=?1", [id], |r| r.get(0))
            .context(crate::text!("server.error.account_not_found"))?;
        let account: Account = serde_json::from_str(&data)?;
        delete_account(&tx, id)?;
        tx.commit()?;
        Ok(account)
    }
    pub fn finish_delivery(&self, id: &str, succeeded: bool) -> Result<()> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        tx.execute(
            "UPDATE deliveries SET status=?2 WHERE id=?1",
            params![id, if succeeded { "sent" } else { "unknown" }],
        )?;
        if succeeded {
            let data: String =
                tx.query_row("SELECT data FROM messages WHERE id=?1", [id], |r| r.get(0))?;
            let mut message: Message = serde_json::from_str(&data)?;
            message.folder = "sent".into();
            tx.execute(
                "UPDATE messages SET data=?2 WHERE id=?1",
                params![id, serde_json::to_string(&message)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn ai_settings(&self) -> Result<AiSettings> {
        let data: Option<String> = self
            .db
            .lock()
            .query_row("SELECT data FROM settings WHERE id='ai'", [], |r| r.get(0))
            .ok();
        Ok(data
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or(AiSettings {
                provider: "openai".into(),
                model: "".into(),
                base_url: "https://api.openai.com/v1".into(),
                configured: false,
            }))
    }
    pub fn save_ai_settings(&self, settings: &AiSettings) -> Result<()> {
        self.db.lock().execute(
            "INSERT INTO settings VALUES('ai',?1) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            [serde_json::to_string(settings)?],
        )?;
        Ok(())
    }
    /// CardDAV and CalDAV servers, oldest first.
    pub fn dav_sources(&self) -> Result<Vec<DavSource>> {
        let db = self.db.lock();
        let mut stmt = db.prepare("SELECT data FROM dav_sources ORDER BY rowid")?;
        stmt.query_map([], |row| row.get::<_, String>(0))?
            .map(|r| Ok(serde_json::from_str(&r?)?))
            .collect()
    }
    pub fn dav_source(&self, id: &str) -> Result<DavSource> {
        let data: String = self
            .db
            .lock()
            .query_row("SELECT data FROM dav_sources WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .context(crate::text!("server.error.server_not_found"))?;
        Ok(serde_json::from_str(&data)?)
    }
    pub fn save_dav_source(&self, source: &DavSource) -> Result<()> {
        self.db.lock().execute(
            "INSERT INTO dav_sources VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            params![source.id, serde_json::to_string(source)?],
        )?;
        Ok(())
    }
    /// Forgets a DAV server and clears the default calendar when it is one of the server's. Its
    /// password stays in the vault; see `daemon` for removing it.
    pub fn remove_dav_source(&self, id: &str) -> Result<DavSource> {
        let source = self.dav_source(id)?;
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        tx.execute("DELETE FROM dav_sources WHERE id=?1", [id])?;
        forget_defaults(&tx, id)?;
        tx.commit()?;
        Ok(source)
    }
    /// Saves a DAV server's sync outcome unless it was removed meanwhile.
    pub fn update_dav_source(&self, source: &DavSource) -> Result<()> {
        self.db.lock().execute(
            "UPDATE dav_sources SET data=?2 WHERE id=?1",
            params![source.id, serde_json::to_string(source)?],
        )?;
        Ok(())
    }
    /// Replaces the contacts `owner` (an account ID, a DAV source ID or `system`) has from `source`
    /// with `contacts`, returning how many are stored. Nothing is stored when the owner was removed
    /// or stopped sharing contacts while they were read, so a slow import cannot bring them back.
    pub fn replace_contacts(
        &self,
        source: ContactSource,
        owner: &str,
        contacts: &[ImportedContact],
    ) -> Result<usize> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        let current: bool = match source {
            ContactSource::Google | ContactSource::Microsoft => tx.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1 AND json_extract(data,'$.integrations.contacts'))", [owner], |r| r.get(0))?,
            ContactSource::Carddav => tx.query_row("SELECT EXISTS(SELECT 1 FROM dav_sources WHERE id=?1)", [owner], |r| r.get(0))?,
            ContactSource::System => owner == SYSTEM_CONTACTS,
            ContactSource::Mail => bail!(crate::text!("server.error.mail_contacts")),
        };
        if !current {
            return Ok(0);
        }
        let source = contact_source_key(source);
        tx.execute(
            "DELETE FROM contacts WHERE owner=?1 AND source=?2",
            params![owner, source],
        )?;
        {
            // The first entry with a name wins when an address book lists an address twice.
            let mut insert = tx.prepare("INSERT INTO contacts VALUES(?1,?2,?3,?4) ON CONFLICT DO UPDATE SET name=excluded.name WHERE name=''")?;
            for contact in contacts {
                insert.execute(params![
                    owner,
                    source,
                    contact.email.to_lowercase(),
                    contact.name
                ])?;
            }
        }
        let stored: i64 = tx.query_row(
            "SELECT count(*) FROM contacts WHERE owner=?1 AND source=?2",
            params![owner, source],
            |r| r.get(0),
        )?;
        tx.commit()?;
        Ok(stored as usize)
    }
    /// Forgets every contact `owner` (an account ID, a DAV source ID or `system`) brought.
    pub fn remove_contacts(&self, owner: &str) -> Result<usize> {
        Ok(self
            .db
            .lock()
            .execute("DELETE FROM contacts WHERE owner=?1", [owner])?)
    }
    /// Every stored address book contact, in the order they were stored.
    pub fn address_book(&self) -> Result<Vec<StoredContact>> {
        let db = self.db.lock();
        let mut stmt = db.prepare("SELECT owner,source,email,name FROM contacts ORDER BY rowid")?;
        stmt.query_map([], |r| {
            let source: String = r.get(1)?;
            Ok((r.get(0)?, source, r.get(2)?, r.get(3)?))
        })?
        .filter_map(|row| match row {
            Ok((owner, source, email, name)) => parse_contact_source(&source).map(|source| {
                Ok(StoredContact {
                    owner,
                    source,
                    email,
                    name,
                })
            }),
            Err(e) => Some(Err(e.into())),
        })
        .collect()
    }
    /// How many contacts are stored from provider and CardDAV address books.
    pub fn synced_contact_count(&self) -> Result<usize> {
        let count: i64 = self.db.lock().query_row(
            "SELECT count(*) FROM contacts WHERE owner<>?1",
            [SYSTEM_CONTACTS],
            |r| r.get(0),
        )?;
        Ok(count as usize)
    }
    /// Walks the headers of every stored message but drafts and spam, from the search index so no message
    /// JSON is parsed: `visit(account_id, folder, from_name, from_email, recipients, julian_date)`,
    /// recipients separated by spaces. Returns the revision the walk saw.
    pub fn each_correspondence(
        &self,
        mut visit: impl FnMut(&str, &str, &str, &str, &str, f64),
    ) -> Result<String> {
        let db = self.db.lock();
        let revision = revision(self.epoch, &db);
        let mut stmt = db.prepare("SELECT account_id,folder,from_name,from_email,recipients,julian FROM message_index WHERE folder IS NOT 'drafts' AND folder IS NOT 'spam'")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let text = |index| -> rusqlite::Result<&str> {
                Ok(row.get_ref(index)?.as_str_or_null()?.unwrap_or(""))
            };
            visit(
                text(0)?,
                text(1)?,
                text(2)?,
                text(3)?,
                text(4)?,
                row.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
            );
        }
        Ok(revision)
    }
    pub fn preferences(&self) -> Result<Preferences> {
        let db = self.db.lock();
        let data: Option<String> = db
            .query_row(
                "SELECT data FROM settings WHERE id='preferences'",
                [],
                |r| r.get(0),
            )
            .ok();
        Ok(data
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_default())
    }
    pub fn save_preferences(&self, patch: PreferencesPatch) -> Result<Preferences> {
        if patch
            .signature
            .as_ref()
            .is_some_and(|s| s.len() > 10_000 || s.contains('\0'))
        {
            bail!(crate::text!("server.error.signature_limits"));
        }
        if patch
            .default_calendar
            .as_ref()
            .is_some_and(|s| s.len() > 2_000)
        {
            bail!(crate::text!("server.error.choose_calendar"));
        }
        if let Some(id) = &patch.default_account_id
            && !id.is_empty()
        {
            self.account(id)?;
        }
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        let data: Option<String> = tx
            .query_row(
                "SELECT data FROM settings WHERE id='preferences'",
                [],
                |r| r.get(0),
            )
            .ok();
        let mut saved: Preferences = data
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_default();
        if let Some(value) = patch.language {
            saved.language = value;
        }
        if let Some(value) = patch.theme {
            saved.theme = value;
        }
        if let Some(value) = patch.mail_layout {
            saved.mail_layout = value;
        }
        if let Some(value) = patch.show_avatars {
            saved.show_avatars = value;
        }
        if let Some(value) = patch.density {
            saved.density = value;
        }
        if let Some(value) = patch.message_format {
            saved.message_format = value;
        }
        if let Some(value) = patch.remote_images {
            saved.remote_images = value;
        }
        if let Some(value) = patch.thread_order {
            saved.thread_order = value;
        }
        if let Some(value) = patch.sync_window {
            saved.sync_window = value;
        }
        if let Some(value) = patch.signature {
            saved.signature = value;
        }
        if let Some(value) = patch.default_account_id {
            saved.default_account_id = value;
        }
        if let Some(value) = patch.notifications {
            saved.notifications = value;
        }
        if let Some(value) = patch.otp_autocopy {
            saved.otp_autocopy = value;
            if !value {
                tx.execute(
                    "UPDATE otp_deliveries SET status='retired',data=NULL WHERE status='pending'",
                    [],
                )?;
            }
        }
        if let Some(value) = patch.menu_bar {
            saved.menu_bar = value;
        }
        if let Some(value) = patch.default_calendar {
            saved.default_calendar = value;
        }
        if let Some(value) = patch.system_contacts {
            saved.system_contacts = value;
        }
        if let Some(value) = patch.mcp {
            saved.mcp = value;
        }
        tx.execute("INSERT INTO settings VALUES('preferences',?1) ON CONFLICT(id) DO UPDATE SET data=excluded.data", [serde_json::to_string(&saved)?])?;
        tx.commit()?;
        Ok(saved)
    }
    pub fn sending_account(&self, requested: &str) -> Result<Account> {
        if !requested.is_empty() {
            return self.account(requested);
        }
        let accounts = self.accounts()?;
        let preferred = self.preferences()?.default_account_id;
        accounts
            .iter()
            .find(|a| a.id == preferred)
            .or(accounts.first())
            .cloned()
            .context(crate::text!("server.error.connect_before_composing"))
    }
    /// Records the account's answer to the invitation in message `id`, on every copy of that
    /// message the account has (Gmail keeps one in several mailboxes).
    pub fn record_rsvp(&self, id: &str, response: Rsvp) -> Result<Message> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        let data: String = tx
            .query_row("SELECT data FROM messages WHERE id=?1", [id], |r| r.get(0))
            .context("Message not found")?;
        let message: Message = serde_json::from_str(&data)?;
        let uid = message
            .invite
            .as_ref()
            .map(|i| i.uid.clone())
            .context("This message has no calendar invitation")?;
        let mut ids = vec![id.to_string()];
        if let Some(message_id) = &message.message_id {
            let mut copies = tx.prepare("SELECT id FROM messages WHERE json_extract(data,'$.account_id')=?1 AND json_extract(data,'$.message_id')=?2 AND id<>?3")?;
            ids.extend(
                copies
                    .query_map(params![message.account_id, message_id, id], |r| {
                        r.get::<_, String>(0)
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }
        let mut updated = None;
        for copy_id in ids {
            let data: String =
                tx.query_row("SELECT data FROM messages WHERE id=?1", [&copy_id], |r| {
                    r.get(0)
                })?;
            let mut copy: Message = serde_json::from_str(&data)?;
            let Some(invite) = copy.invite.as_mut().filter(|i| i.uid == uid) else {
                continue;
            };
            invite.response = Some(response);
            tx.execute(
                "UPDATE messages SET data=?2 WHERE id=?1",
                params![copy_id, serde_json::to_string(&copy)?],
            )?;
            if copy_id == id {
                updated = Some(copy);
            }
        }
        tx.commit()?;
        updated.context("Message not found")
    }
}
#[cfg(test)]
pub const FIXTURE_ACCOUNT_ID: &str = "fixture";
#[cfg(test)]
impl Store {
    /// Adds an account with sample messages for tests. Returns the existing one if already added.
    pub fn add_fixture(&self) -> Result<Account> {
        if let Ok(account) = self.account(FIXTURE_ACCOUNT_ID) {
            return Ok(account);
        }
        let account = Account {
            id: FIXTURE_ACCOUNT_ID.into(),
            name: "Fixture".into(),
            email: "alex@form.studio".into(),
            provider: Provider::Imap,
            imap_host: "".into(),
            imap_port: 993,
            smtp_host: "".into(),
            smtp_port: 587,
            folders: Mailboxes::default(),
            last_synced: None,
            sync_error: None,
            integrations: Integrations::default(),
        };
        self.save_account(&account)?;
        let samples = [
            (
                "Sophie Chen",
                "sophie@form.studio",
                "A little room for big ideas",
                "Hey Alex,\n\nI've been thinking about our conversation yesterday. What if we gave the new collection a little more room to breathe? Less noise, more intention.\n\nI put together a few directions for the studio identity. The first explores softer shapes and a warmer palette; the second is a little more unexpected.\n\nWould love your thoughts before we share these with the team on Thursday. No rush — good ideas need a little space.\n\nTalk soon,\nSophie",
                "work",
                false,
                true,
            ),
            (
                "Oliver Park",
                "oliver@atelier.design",
                "The website is ready for a first look",
                "Hi Alex,\n\nThe first version of the website is ready. All the little details we talked about are in: the quiet transitions, generous spacing, and that unexpected shade of green.\n\nHave a look when you have a moment. I'd especially love your thoughts on the project pages.\n\nOliver",
                "work",
                false,
                false,
            ),
            (
                "Maya Williams",
                "maya@example.com",
                "Sunday, somewhere outside?",
                "Hey you,\n\nThe forecast looks lovely for Sunday. Coffee, a long walk, and absolutely no agenda?\n\nI'm thinking the little place by the canal. Around 10?\n\nMaya",
                "personal",
                false,
                true,
            ),
            (
                "Linear",
                "notifications@linear.app",
                "Your weekly workspace, in perspective",
                "Your team completed 12 issues this week.\n\nThe studio website is on track for Friday, and the identity refresh is ready for review.\n\nTake a moment to appreciate the progress.",
                "updates",
                true,
                false,
            ),
            (
                "Daniel Foster",
                "daniel@form.studio",
                "Re: Studio notes — October",
                "Thanks Alex,\n\nThis feels right. Let's go with the quieter direction and give the typography some space.\n\nI'll pull together the production estimates ahead of our call.\n\nDaniel",
                "work",
                true,
                false,
            ),
            (
                "Are.na",
                "newsletter@are.na",
                "A few things worth keeping",
                "This week's collection: small rituals, independent publishing, and the surprising beauty of everyday objects.\n\nA good place to begin: make a little room for something new.",
                "newsletters",
                true,
                false,
            ),
            (
                "Emma Laurent",
                "emma@another.studio",
                "An invitation to collaborate",
                "Hello Alex,\n\nWe've been following your studio's work and would love to explore a collaboration for our winter exhibition.\n\nWould you be open to a short call next week?\n\nWarmly,\nEmma",
                "work",
                false,
                false,
            ),
            (
                "Noah Kim",
                "noah@example.com",
                "That book I was telling you about",
                "Found it! The one about noticing the things we usually walk past.\n\nI've left a copy for you at the studio. Page 42 is a good place to start.\n\nNoah",
                "personal",
                true,
                false,
            ),
            (
                "Figma",
                "updates@figma.com",
                "The details make the difference",
                "A handful of new tools for your next idea.\n\nExplore the latest updates to components and prototyping in your workspace.",
                "updates",
                true,
                false,
            ),
            (
                "The Modern House",
                "journal@themodernhouse.com",
                "Spaces that make you feel something",
                "This weekend's journal explores a home built around a courtyard, a painter's quiet studio, and the art of living with less.",
                "newsletters",
                true,
                false,
            ),
            (
                "Sophie Chen",
                "sophie@form.studio",
                "Thursday's studio session",
                "Hi everyone,\n\nA quick reminder: we'll meet at the studio on Thursday at 2 pm to review the new identity directions.\n\nBring your questions and something you found inspiring this week.\n\nSophie",
                "work",
                true,
                false,
            ),
            (
                "Goodhood",
                "hello@goodhoodstore.com",
                "A slower kind of weekend",
                "New arrivals for slower mornings and longer walks.\n\nVisit the store this weekend to explore the autumn selection.",
                "newsletters",
                true,
                false,
            ),
        ];
        self.save_messages(samples.into_iter().enumerate().map(
            |(index, (name, email, subject, body, category, read, starred))| Message {
                id: format!("sample-{}", index + 1),
                account_id: account.id.clone(),
                from_name: name.into(),
                from_email: email.into(),
                reply_to: vec![],
                to: vec![account.email.clone()],
                subject: subject.into(),
                preview: preview(body),
                body: body.into(),
                html_body: None,
                date:
                    (Utc::now() - chrono::Duration::minutes((index as i64) * 43 + 8)).to_rfc3339(),
                folder: "inbox".into(),
                is_read: read,
                is_starred: starred,
                category: category.into(),
                attachments: vec![],
                message_id: Some(format!("<sample-{}@alice.local>", index + 1)),
                in_reply_to: None,
                remote_mailbox: None,
                remote_uid: None,
                uid_validity: None,
                invite: None,
            },
        ))?;
        self.save_message(&fixture_invitation(&account)?)?;
        Ok(account)
    }
}
#[cfg(test)]
/// The fixture's calendar invitation, written as Google Calendar writes them: a review a few
/// weekdays ahead at 10:00 in Los Angeles, with the zone's `VTIMEZONE`.
fn fixture_invitation(account: &Account) -> Result<Message> {
    use chrono::{Datelike, TimeZone, Weekday};
    use chrono_tz::America::Los_Angeles;
    let now = Utc::now();
    let mut day = now.with_timezone(&Los_Angeles).date_naive() + chrono::Duration::days(3);
    while matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
        day += chrono::Duration::days(1);
    }
    let start = Los_Angeles
        .from_local_datetime(&day.and_hms_opt(10, 0, 0).context("Invalid fixture time")?)
        .earliest()
        .context("Invalid fixture time")?;
    let end = start + chrono::Duration::minutes(45);
    let local = |time: chrono::DateTime<chrono_tz::Tz>| time.format("%Y%m%dT%H%M%S").to_string();
    let ics = [
        "BEGIN:VCALENDAR".to_string(),
        "PRODID:-//Google Inc//Google Calendar 70.9054//EN".into(),
        "VERSION:2.0".into(),
        "CALSCALE:GREGORIAN".into(),
        "METHOD:REQUEST".into(),
        "BEGIN:VTIMEZONE".into(),
        "TZID:America/Los_Angeles".into(),
        "X-LIC-LOCATION:America/Los_Angeles".into(),
        "BEGIN:DAYLIGHT".into(),
        "TZOFFSETFROM:-0800".into(),
        "TZOFFSETTO:-0700".into(),
        "TZNAME:PDT".into(),
        "DTSTART:19700308T020000".into(),
        "RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU".into(),
        "END:DAYLIGHT".into(),
        "BEGIN:STANDARD".into(),
        "TZOFFSETFROM:-0700".into(),
        "TZOFFSETTO:-0800".into(),
        "TZNAME:PST".into(),
        "DTSTART:19701101T020000".into(),
        "RRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU".into(),
        "END:STANDARD".into(),
        "END:VTIMEZONE".into(),
        "BEGIN:VEVENT".into(),
        format!("DTSTART;TZID=America/Los_Angeles:{}", local(start)),
        format!("DTEND;TZID=America/Los_Angeles:{}", local(end)),
        format!("DTSTAMP:{}", now.format("%Y%m%dT%H%M%SZ")),
        "ORGANIZER;CN=Maya Patel:mailto:maya@studio-north.co".into(),
        "UID:sample-13-identity-review@studio-north.co".into(),
        "ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;CN=Maya Pat".into(),
        " el;X-NUM-GUESTS=0:mailto:maya@studio-north.co".into(),
        "ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=".into(),
        format!(" TRUE;CN={0};X-NUM-GUESTS=0:mailto:{0}", account.email),
        "DESCRIPTION:A walkthrough of the identity directions before they go to the".into(),
        "  wider team.\\n\\nJoin by video: https://meet.example.com/studio-north-review".into(),
        "LOCATION:Studio North\\, 2150 Mission St\\, San Francisco".into(),
        "SEQUENCE:0".into(),
        "STATUS:CONFIRMED".into(),
        "SUMMARY:Identity review — Studio North × Form".into(),
        "TRANSP:OPAQUE".into(),
        "END:VEVENT".into(),
        "END:VCALENDAR".into(),
    ]
    .join("\r\n")
        + "\r\n";
    let invite = crate::ical::invite(&ics, &account.email)?;
    let when = start.format("%a %b %-d, %Y");
    let body = format!(
        "Hi Alex,\n\nBefore we share the identity directions with the wider team, I'd love to walk you through them. I've booked 45 minutes at the studio; there's a video link too if you'd rather join remotely.\n\nIf the time doesn't work, let me know and I'll find another.\n\nMaya\n\nIdentity review — Studio North × Form\n{when} · 10am – 10:45am (Pacific Time)\nStudio North, 2150 Mission St, San Francisco\nJoin by video: https://meet.example.com/studio-north-review"
    );
    Ok(Message {
        id: "sample-13".into(),
        account_id: account.id.clone(),
        from_name: "Maya Patel".into(),
        from_email: "maya@studio-north.co".into(),
        to: vec![account.email.clone()],
        reply_to: vec![],
        subject: format!(
            "Invitation: Identity review — Studio North × Form @ {when} 10am – 10:45am ({})",
            start.format("%Z")
        ),
        preview: preview(&body),
        body,
        html_body: None,
        date: (now - chrono::Duration::minutes(30)).to_rfc3339(),
        folder: "inbox".into(),
        is_read: false,
        is_starred: false,
        category: "work".into(),
        attachments: vec![],
        message_id: Some("<sample-13@alice.local>".into()),
        in_reply_to: None,
        remote_mailbox: None,
        remote_uid: None,
        uid_validity: None,
        invite: Some(invite),
    })
}
/// The owner of contacts imported from the operating system's address book.
pub const SYSTEM_CONTACTS: &str = "system";
/// An address book contact as stored: who brought it (`owner`: an account ID, a DAV source ID or
/// [`SYSTEM_CONTACTS`]) and from which kind of address book.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredContact {
    pub owner: String,
    pub source: ContactSource,
    /// Lowercased.
    pub email: String,
    pub name: String,
}
fn contact_source_key(source: ContactSource) -> &'static str {
    match source {
        ContactSource::Mail => "mail",
        ContactSource::Google => "google",
        ContactSource::Microsoft => "microsoft",
        ContactSource::Carddav => "carddav",
        ContactSource::System => "system",
    }
}
fn parse_contact_source(key: &str) -> Option<ContactSource> {
    [
        ContactSource::Google,
        ContactSource::Microsoft,
        ContactSource::Carddav,
        ContactSource::System,
    ]
    .into_iter()
    .find(|source| contact_source_key(*source) == key)
}
fn revision(epoch: i64, db: &Connection) -> String {
    format!("{epoch}.{}", db.total_changes())
}
/// SQL conditions and values selecting the queried account, folder and category. `column` gives the
/// SQL for a message field: the stored JSON, or a column of the search index.
fn scope_filter(
    query: &MessageQuery,
    column: fn(&str) -> String,
) -> (String, Vec<rusqlite::types::Value>) {
    let mut conditions = vec!["1=1".to_string()];
    let mut values: Vec<rusqlite::types::Value> = vec![];
    if let Some(id) = &query.account_id {
        conditions.push(format!("{}=?", column("account_id")));
        values.push(id.clone().into());
    }
    if let Some(folder) = &query.folder {
        if folder == "starred" {
            conditions.push(format!(
                "{}=1 AND {} NOT IN ('trash','spam')",
                column("is_starred"),
                column("folder")
            ));
        } else {
            conditions.push(format!("{}=?", column("folder")));
            values.push(folder.clone().into());
        }
    } else {
        conditions.push(format!("{}!='spam'", column("folder")));
    }
    if let Some(category) = &query.category {
        conditions.push(format!("{}=?", column("category")));
        values.push(category.clone().into());
    }
    (conditions.join(" AND "), values)
}
/// Bump when the search index's layout or tokenizer changes; the index is then rebuilt on open.
const SEARCH_INDEX_VERSION: i64 = 2;
/// The shared index row for a message stored in `data` with id `id`: raw headers and flags
/// used by search and conversation grouping, without the bodies.
fn search_projection(id: &str, data: &str) -> String {
    format!(
        "{id},json_extract({data},'$.account_id'),json_extract({data},'$.folder'),json_extract({data},'$.category'),json_extract({data},'$.from_name'),json_extract({data},'$.from_email'),(SELECT group_concat(value,' ') FROM json_each({data},'$.to')),json_extract({data},'$.subject'),json_extract({data},'$.is_read'),json_extract({data},'$.is_starred'),COALESCE(json_array_length({data},'$.attachments'),0)>0,julianday(json_extract({data},'$.date')),json_extract({data},'$.message_id')"
    )
}
/// Creates the search index, kept current by triggers on `messages`: `message_index`, a compact
/// copy of each message's headers and flags, and `message_bodies`, a full-text index of the plain
/// text bodies. Both are keyed by the message's rowid, so the index is rebuilt when its version
/// changed or it no longer lines up with `messages` (e.g. after a `VACUUM` renumbered rows).
fn ensure_search_index(db: &mut Connection) -> Result<()> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let tx = db.transaction()?;
    if version != SEARCH_INDEX_VERSION {
        tx.execute_batch("DROP TRIGGER IF EXISTS message_index_insert; DROP TRIGGER IF EXISTS message_index_update; DROP TRIGGER IF EXISTS message_bodies_update; DROP TRIGGER IF EXISTS message_index_delete; DROP TABLE IF EXISTS message_index; DROP TABLE IF EXISTS message_bodies;")?;
    }
    let new = search_projection("new.rowid,new.id", "new.data");
    let changed = search_projection("new.id", "new.data");
    tx.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS message_index(message INTEGER PRIMARY KEY, id TEXT NOT NULL, account_id TEXT, folder TEXT, category TEXT, from_name TEXT, from_email TEXT, recipients TEXT, subject TEXT, is_read INTEGER, is_starred INTEGER, has_attachments INTEGER, julian REAL, message_id TEXT);
        CREATE INDEX IF NOT EXISTS message_index_scope ON message_index(folder, account_id, category);
        CREATE INDEX IF NOT EXISTS message_index_account ON message_index(account_id, julian);
        CREATE VIRTUAL TABLE IF NOT EXISTS message_bodies USING fts5(body, content='', contentless_delete=1, tokenize='unicode61 remove_diacritics 2', prefix='2 3');
        CREATE TRIGGER IF NOT EXISTS message_index_insert AFTER INSERT ON messages BEGIN
            INSERT INTO message_index VALUES({new});
            INSERT INTO message_bodies(rowid, body) VALUES(new.rowid, json_extract(new.data,'$.body'));
        END;
        CREATE TRIGGER IF NOT EXISTS message_index_update AFTER UPDATE ON messages BEGIN
            UPDATE message_index SET (id,account_id,folder,category,from_name,from_email,recipients,subject,is_read,is_starred,has_attachments,julian,message_id)=({changed}) WHERE message=new.rowid;
        END;
        CREATE TRIGGER IF NOT EXISTS message_bodies_update AFTER UPDATE ON messages WHEN json_extract(old.data,'$.body') IS NOT json_extract(new.data,'$.body') BEGIN
            DELETE FROM message_bodies WHERE rowid=old.rowid;
            INSERT INTO message_bodies(rowid, body) VALUES(new.rowid, json_extract(new.data,'$.body'));
        END;
        CREATE TRIGGER IF NOT EXISTS message_index_delete AFTER DELETE ON messages BEGIN
            DELETE FROM message_index WHERE message=old.rowid;
            DELETE FROM message_bodies WHERE rowid=old.rowid;
        END;"
    ))?;
    let in_step: bool = tx.query_row(
        "SELECT (SELECT count(*) FROM messages)=(SELECT count(*) FROM message_index) AND NOT EXISTS(SELECT 1 FROM message_index i LEFT JOIN messages m ON m.rowid=i.message WHERE m.id IS NOT i.id)",
        [],
        |r| r.get(0),
    )?;
    if !in_step {
        let stored = search_projection("rowid,id", "data");
        tx.execute_batch(&format!(
            "DELETE FROM message_index;
            INSERT INTO message_bodies(message_bodies) VALUES('delete-all');
            INSERT INTO message_index SELECT {stored} FROM messages;
            INSERT INTO message_bodies(rowid, body) SELECT rowid, json_extract(data,'$.body') FROM messages;"
        ))?;
    }
    tx.execute_batch(&format!("PRAGMA user_version={SEARCH_INDEX_VERSION}"))?;
    tx.commit()?;
    Ok(())
}
/// A message matched by [`search`].
struct Hit {
    id: String,
    score: f64,
    julian: f64,
    is_read: bool,
}
/// Messages in the query's account, folder and category that match the search `text`, best match
/// first and newest first among equals. See [`crate::search`] for the query language.
fn search(db: &Connection, query: &MessageQuery, text: &str) -> Result<Vec<Hit>> {
    // Text with nothing to match, like `()`, filters nothing.
    let expr = search::parse(text, &chrono::Local::now()).unwrap_or(search::Expr::And(vec![]));
    let mut fulltext =
        db.prepare_cached("SELECT rowid FROM message_bodies WHERE message_bodies MATCH ?1")?;
    let mut matcher = search::Matcher::new(expr, |fts| {
        fulltext
            .query_map([fts], |r| r.get(0))?
            .collect::<rusqlite::Result<HashSet<i64>>>()
    })?;
    let (scope, values) = scope_filter(query, str::to_string);
    let mut stmt = db.prepare(&format!("SELECT message,id,from_name,from_email,recipients,subject,is_read,is_starred,has_attachments,julian FROM message_index WHERE {scope}"))?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(&values), |r| {
            let folded = |index| -> rusqlite::Result<String> {
                Ok(search::fold(
                    r.get_ref(index)?.as_str_or_null()?.unwrap_or(""),
                ))
            };
            Ok(search::Row {
                rowid: r.get(0)?,
                id: r.get(1)?,
                from_name: folded(2)?,
                from_email: folded(3)?,
                recipients: folded(4)?,
                subject: folded(5)?,
                is_read: r.get::<_, Option<bool>>(6)?.unwrap_or(false),
                is_starred: r.get::<_, Option<bool>>(7)?.unwrap_or(false),
                has_attachments: r.get::<_, Option<bool>>(8)?.unwrap_or(false),
                julian: r.get::<_, Option<f64>>(9)?.unwrap_or(f64::MIN),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    matcher.allow_typos(&rows);
    let mut hits: Vec<Hit> = rows
        .into_iter()
        .filter_map(|row| {
            Some(Hit {
                score: matcher.score(&row)?,
                julian: row.julian,
                is_read: row.is_read,
                id: row.id,
            })
        })
        .collect();
    hits.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(b.julian.total_cmp(&a.julian))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(hits)
}
/// The subject that identifies a conversation: reply and forward prefixes (`Re:`, `Fwd: [2]`, `AW:`…)
/// removed, whitespace collapsed, lowercased. Empty when nothing is left to group by.
fn thread_key(subject: &str) -> String {
    const PREFIXES: [&str; 10] = [
        "re", "fwd", "fw", "aw", "wg", "sv", "vs", "antw", "tr", "rif",
    ];
    let mut rest = subject.trim();
    'strip: loop {
        // ASCII lowercasing keeps byte offsets, so lengths measured on `lower` apply to `rest`.
        let lower = rest.to_ascii_lowercase();
        for prefix in PREFIXES {
            let Some(after) = lower.strip_prefix(prefix) else {
                continue;
            };
            let mut after = after.trim_start();
            if let Some(counted) = after
                .strip_prefix('[')
                .and_then(|s| s.split_once(']'))
                .filter(|(n, _)| n.chars().all(|c| c.is_ascii_digit()))
            {
                after = counted.1.trim_start();
            }
            if let Some(after) = after.strip_prefix(':').or_else(|| after.strip_prefix('：')) {
                rest = rest[rest.len() - after.len()..].trim_start();
                continue 'strip;
            }
        }
        break;
    }
    rest.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
/// What grouping needs about a message; bodies stay in SQLite.
struct ThreadRow {
    id: String,
    account_id: String,
    key: String,
    folder: String,
    from_name: String,
    from_email: String,
    message_id: Option<String>,
    is_read: bool,
    is_starred: bool,
}
/// Every message of one account (or all accounts), oldest first, from the shared message index.
fn thread_rows(db: &Connection, account_id: Option<&str>) -> Result<Vec<ThreadRow>> {
    let mut stmt = db.prepare_cached("SELECT id,account_id,subject,folder,from_name,from_email,message_id,is_read,is_starred FROM message_index WHERE ?1 IS NULL OR account_id=?1 ORDER BY julian ASC,id ASC")?;
    stmt.query_map([account_id], |r| {
        Ok(ThreadRow {
            id: r.get(0)?,
            account_id: r.get(1)?,
            key: thread_key(r.get_ref(2)?.as_str_or_null()?.unwrap_or("")),
            folder: r.get(3)?,
            from_name: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
            from_email: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
            message_id: r.get(6)?,
            is_read: r.get::<_, Option<bool>>(7)?.unwrap_or(true),
            is_starred: r.get::<_, Option<bool>>(8)?.unwrap_or(false),
        })
    })?
    .map(|r| Ok(r?))
    .collect()
}
/// A conversation's messages, oldest first, without drafts. Spam contains only Spam messages;
/// other views exclude Spam, and include Trash only in Trash. Each Message-ID appears once,
/// preferring `preferred` copies, then Inbox, Sent and other folders, then Archive.
fn conversation<'a>(
    members: &[&'a ThreadRow],
    folder: Option<&str>,
    preferred: impl Fn(&ThreadRow) -> bool,
) -> Vec<&'a ThreadRow> {
    let rank = |r: &ThreadRow| {
        if preferred(r) {
            0
        } else {
            match r.folder.as_str() {
                "inbox" => 1,
                "sent" => 2,
                "archive" => 4,
                _ => 3,
            }
        }
    };
    let mut chosen: Vec<&ThreadRow> = Vec::new();
    for row in members.iter().copied().filter(|r| {
        r.folder != "drafts"
            && if folder == Some("spam") {
                r.folder == "spam"
            } else {
                r.folder != "spam" && (folder == Some("trash") || r.folder != "trash")
            }
    }) {
        match row.message_id.as_ref().and_then(|id| {
            chosen
                .iter()
                .position(|c| c.message_id.as_ref() == Some(id))
        }) {
            Some(index) if rank(row) < rank(chosen[index]) => chosen[index] = row,
            Some(_) => {}
            None => chosen.push(row),
        }
    }
    chosen
}
/// Inserts or replaces a message, keeping the row of an already cached provider copy and the local
/// identity of mail Alice delivered itself.
fn upsert(tx: &Transaction<'_>, mut message: Message) -> Result<()> {
    if let (Some(mailbox), Some(uid), Some(validity)) = (
        &message.remote_mailbox,
        message.remote_uid,
        message.uid_validity,
    ) {
        let existing: Option<String> = tx.prepare_cached("SELECT id FROM messages WHERE json_extract(data,'$.account_id')=?1 AND json_extract(data,'$.remote_mailbox')=?2 AND json_extract(data,'$.remote_uid')=?3 AND json_extract(data,'$.uid_validity')=?4 LIMIT 1")?
            .query_row(params![message.account_id, mailbox, uid, validity], |r| r.get(0))
            .ok();
        if let Some(id) = existing {
            message.id = id;
        }
    }
    if message.folder == "sent"
        && let Some(identity) = &message.message_id
    {
        let local_id: Option<String> = tx.prepare_cached("SELECT id FROM messages WHERE json_extract(data,'$.account_id')=?1 AND json_extract(data,'$.message_id')=?2 AND json_extract(data,'$.folder')='sent' AND id IN (SELECT id FROM deliveries WHERE status='sent') LIMIT 1")?
            .query_row(params![message.account_id, identity], |r| r.get(0))
            .ok();
        if let Some(id) = local_id {
            if id != message.id {
                tx.execute("DELETE FROM messages WHERE id=?1", [&message.id])?;
            }
            message.id = id;
        }
    }
    tx.prepare_cached(
        "INSERT INTO messages VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
    )?
    .execute(params![message.id, serde_json::to_string(&message)?])?;
    Ok(())
}
/// Removes an account's messages, delivery records, address book contacts, default sender and
/// default calendar preferences, then the account.
fn delete_account(tx: &Transaction<'_>, id: &str) -> Result<()> {
    tx.execute("DELETE FROM deliveries WHERE id IN (SELECT id FROM messages WHERE json_extract(data,'$.account_id')=?1)", [id])?;
    tx.execute(
        "DELETE FROM messages WHERE json_extract(data,'$.account_id')=?1",
        [id],
    )?;
    tx.execute("DELETE FROM accounts WHERE id=?1", [id])?;
    tx.execute("DELETE FROM contacts WHERE owner=?1", [id])?;
    crate::sync_store::delete_account(tx, id)?;
    forget_defaults(tx, id)
}
/// Clears the default sender and the default calendar when they belong to `owner`, an account or a
/// DAV source.
fn forget_defaults(tx: &Transaction<'_>, owner: &str) -> Result<()> {
    let preferences: Option<String> = tx
        .query_row(
            "SELECT data FROM settings WHERE id='preferences'",
            [],
            |r| r.get(0),
        )
        .ok();
    let Some(mut preferences) = preferences
        .map(|s| serde_json::from_str::<Preferences>(&s))
        .transpose()?
    else {
        return Ok(());
    };
    let sender = preferences.default_account_id == owner;
    let calendar =
        crate::calendar::calendar_owner(&preferences.default_calendar).as_deref() == Some(owner);
    if sender {
        preferences.default_account_id.clear();
    }
    if calendar {
        preferences.default_calendar.clear();
    }
    if sender || calendar {
        tx.execute(
            "UPDATE settings SET data=?1 WHERE id='preferences'",
            [serde_json::to_string(&preferences)?],
        )?;
    }
    Ok(())
}
pub fn preview(body: &str) -> String {
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store_with_fixture(path: &Path) -> Result<Store> {
        let store = Store::open(path)?;
        store.add_fixture()?;
        Ok(store)
    }
    #[test]
    fn legacy_mcp_preferences_survive_startup_without_granting_send_access() -> Result<()> {
        for (value, expected) in [
            (serde_json::json!(true), McpAccess::ReadOnly),
            (serde_json::json!(false), McpAccess::Off),
            (serde_json::json!("off"), McpAccess::Off),
            (serde_json::json!("read_only"), McpAccess::ReadOnly),
            (serde_json::json!("full"), McpAccess::Full),
        ] {
            let dir = tempfile::tempdir()?;
            let path = dir.path().join("mail.db");
            let saved = serde_json::json!({"mcp": value, "signature": "Keep my signature", "theme": "dark"});
            let db = Connection::open(&path)?;
            db.execute_batch("CREATE TABLE settings(id TEXT PRIMARY KEY, data TEXT NOT NULL)")?;
            db.execute(
                "INSERT INTO settings VALUES('preferences',?1)",
                [saved.to_string()],
            )?;
            drop(db);
            for _ in 0..2 {
                let store = Store::open(&path)?;
                let preferences = store.preferences()?;
                assert_eq!(preferences.mcp, expected);
                assert_eq!(preferences.signature, "Keep my signature");
                assert_eq!(preferences.theme, Theme::Dark);
                let stored: String = store.db.lock().query_row(
                    "SELECT data FROM settings WHERE id='preferences'",
                    [],
                    |r| r.get(0),
                )?;
                let mut expected_saved = saved.clone();
                expected_saved["mcp"] = serde_json::to_value(expected)?;
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&stored)?,
                    expected_saved
                );
            }
        }
        Ok(())
    }
    #[test]
    fn sample_accounts_from_earlier_builds_are_cleared_on_open() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("mail.db");
        let store = Store::open(&path)?;
        assert!(store.accounts()?.is_empty());
        let mut real = store.add_fixture()?;
        let mut mail = store.message("sample-1")?;
        real.id = "real".into();
        store.save_account(&real)?;
        store.remove_account(FIXTURE_ACCOUNT_ID)?;
        drop(store);
        // Sample accounts seeded by earlier builds are cleared on open; real accounts stay.
        let legacy = Connection::open(&path)?;
        let mut sample = real.clone();
        sample.id = "demo".into();
        legacy.execute(
            "INSERT INTO accounts VALUES(?1,?2)",
            params![sample.id, serde_json::to_string(&sample)?],
        )?;
        mail.id = "legacy-1".into();
        mail.account_id = sample.id.clone();
        legacy.execute(
            "INSERT INTO messages VALUES(?1,?2)",
            params![mail.id, serde_json::to_string(&mail)?],
        )?;
        drop(legacy);
        let reopened = Store::open(&path)?;
        assert_eq!(
            reopened
                .accounts()?
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            ["real"]
        );
        assert!(reopened.message("legacy-1").is_err());
        Ok(())
    }
    #[test]
    fn category_counts_and_pages_use_the_same_account_scope() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = store_with_fixture(&dir.path().join("mail.db"))?;
        for m in store.list(&MessageQuery::default())?.messages {
            store.patch(
                &m.id,
                &MessagePatch {
                    folder: Some("trash".into()),
                    ..Default::default()
                },
            )?;
        }
        let mut mail = store.message("sample-1")?;
        mail.folder = "inbox".into();
        for i in 0..65 {
            mail.id = format!("scope-{i:02}");
            mail.account_id = if i < 60 {
                FIXTURE_ACCOUNT_ID
            } else {
                "other-account"
            }
            .into();
            mail.category = if i >= 55 { "work" } else { "personal" }.into();
            mail.is_read = i % 2 == 1;
            mail.body = "HÉLLO, user".into();
            mail.date = (Utc::now() - chrono::Duration::minutes(i)).to_rfc3339();
            store.save_message(&mail)?;
        }
        let query = MessageQuery {
            folder: Some("inbox".into()),
            account_id: Some(FIXTURE_ACCOUNT_ID.into()),
            category: Some("work".into()),
            limit: Some(2),
            offset: Some(2),
            ..Default::default()
        };
        let page = store.list(&query)?;
        assert_eq!((page.total, page.all_total, page.unread_total), (5, 5, 2));
        assert_eq!(
            page.messages
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            ["scope-57", "scope-58"]
        );
        let search = store.list(&MessageQuery {
            q: Some("héllo".into()),
            ..query.clone()
        })?;
        assert_eq!(
            (search.total, search.messages.len(), search.unread_total),
            (5, 2, 2)
        );
        let page = store.list(&MessageQuery {
            unread: Some(true),
            offset: Some(0),
            ..query
        })?;
        assert_eq!((page.total, page.all_total, page.unread_total), (2, 5, 2));
        assert!(page.messages.iter().all(|m| !m.is_read));
        Ok(())
    }
    #[test]
    fn importing_a_remote_message_with_a_new_id_preserves_its_existing_cache_row() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = store_with_fixture(&dir.path().join("mail.db"))?;
        let mut sent = store.message("sample-1")?;
        sent.folder = "sent".into();
        sent.remote_mailbox = Some("Sent".into());
        sent.remote_uid = Some(42);
        sent.uid_validity = Some(3);
        store.save_message(&sent)?;
        sent.id = "new-provider-identity".into();
        store.save_message(&sent)?;
        let page = store.list(&MessageQuery {
            folder: Some("sent".into()),
            ..Default::default()
        })?;
        assert_eq!(page.total, 1);
        assert_eq!(page.messages[0].id, "sample-1");
        Ok(())
    }
    #[test]
    fn conversations_group_by_subject_once_and_follow_the_view() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = store_with_fixture(&dir.path().join("mail.db"))?;
        let original = store.message("sample-1")?;
        let fixture = |id: &str,
                       subject: &str,
                       folder: &str,
                       date: &str,
                       message_id: &str,
                       from: (&str, &str),
                       read: bool| {
            let mut m = original.clone();
            (m.id, m.subject, m.folder, m.date, m.message_id, m.is_read) = (
                id.into(),
                subject.into(),
                folder.into(),
                date.into(),
                Some(message_id.into()),
                read,
            );
            (m.from_name, m.from_email) = (from.0.into(), from.1.into());
            m
        };
        let sophie = ("Sophie Chen", "sophie@form.studio");
        let me = ("Alex", "ALEX@form.studio");
        store.save_messages([
            fixture(
                "sample-1",
                "A little room for big ideas",
                "inbox",
                "2030-01-01T10:00:00Z",
                "<sample-1@alice.local>",
                sophie,
                false,
            ),
            fixture(
                "my-reply",
                "Re: A little room for big ideas",
                "sent",
                "2030-01-01T11:00:00Z",
                "<reply@x>",
                me,
                true,
            ),
            // Gmail also lists the reply in All Mail; the conversation shows it once.
            fixture(
                "my-reply-copy",
                "Re: A little room for big ideas",
                "archive",
                "2030-01-01T11:00:00Z",
                "<reply@x>",
                me,
                true,
            ),
            fixture(
                "answer",
                "RE: Fwd [2]: a little  room for BIG ideas",
                "inbox",
                "2030-01-01T12:00:00Z",
                "<answer@x>",
                sophie,
                false,
            ),
            fixture(
                "thrown-away",
                "Re: A little room for big ideas",
                "trash",
                "2030-01-01T13:00:00Z",
                "<trash@x>",
                sophie,
                true,
            ),
            fixture(
                "blank-1",
                "Re:",
                "inbox",
                "2029-01-01T10:00:00Z",
                "<b1@x>",
                sophie,
                true,
            ),
            fixture(
                "blank-2",
                "Re:",
                "inbox",
                "2029-01-01T11:00:00Z",
                "<b2@x>",
                sophie,
                true,
            ),
        ])?;
        let inbox = MessageQuery {
            folder: Some("inbox".into()),
            ..Default::default()
        };
        let page = store.threads(&inbox)?;
        // Thirteen fixture conversations (one now with three messages) and two subjects with nothing to group by.
        assert_eq!(page.all_total, 15);
        let thread = &page.threads[0];
        assert_eq!(thread.message.id, "answer");
        assert_eq!(thread.ids, ["answer", "sample-1"]);
        assert_eq!(thread.count, 3);
        assert_eq!(thread.participants, ["Sophie Chen", "me"]);
        assert_eq!(thread.unread_ids, ["sample-1", "answer"]);
        assert_eq!(
            page.threads
                .iter()
                .filter(|t| t.message.subject == "Re:")
                .count(),
            2
        );
        let unread = store.threads(&MessageQuery {
            unread: Some(true),
            ..inbox.clone()
        })?;
        assert_eq!(unread.total, unread.unread_total);
        assert!(unread.threads.iter().all(|t| !t.unread_ids.is_empty()));

        let ids = |messages: Vec<Message>| messages.into_iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(
            ids(store.conversation("answer")?),
            ["sample-1", "my-reply", "answer"]
        );
        assert_eq!(
            ids(store.conversation("my-reply-copy")?),
            ["sample-1", "my-reply-copy", "answer"]
        );
        assert_eq!(
            ids(store.conversation("thrown-away")?),
            ["sample-1", "my-reply", "answer", "thrown-away"]
        );
        assert_eq!(ids(store.conversation("blank-1")?), ["blank-1"]);
        let sent = store.threads(&MessageQuery {
            folder: Some("sent".into()),
            ..Default::default()
        })?;
        assert_eq!(sent.threads[0].ids, ["my-reply"]);
        assert_eq!(sent.threads[0].count, 3);
        Ok(())
    }
    #[test]
    fn dates_sort_by_instant_and_synced_sent_mail_keeps_local_identity() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = store_with_fixture(&dir.path().join("mail.db"))?;
        let mut a = store.message("sample-1")?;
        a.id = "offset-a".into();
        a.date = "2026-10-11T09:00:00+09:00".into();
        a.subject = "timezone fixture".into();
        store.save_message(&a)?;
        a.id = "offset-b".into();
        a.date = "2026-10-11T01:00:00+00:00".into();
        store.save_message(&a)?;
        let page = store.list(&MessageQuery {
            q: Some("timezone fixture".into()),
            ..Default::default()
        })?;
        assert_eq!(page.messages[0].id, "offset-b");

        let draft = Compose {
            id: Uuid::new_v4().to_string(),
            account_id: FIXTURE_ACCOUNT_ID.into(),
            to: vec!["support@example.com".into()],
            subject: "sent fixture".into(),
            body: "hello".into(),
            in_reply_to: None,
        };
        let mut sent = store.save_draft(&draft)?;
        store.claim_delivery(&draft.id)?;
        store.finish_delivery(&draft.id, true)?;
        sent.id = "provider-copy".into();
        sent.folder = "sent".into();
        sent.remote_mailbox = Some("Sent".into());
        sent.remote_uid = Some(42);
        sent.uid_validity = Some(3);
        store.save_message(&sent)?;
        let page = store.list(&MessageQuery {
            q: Some("sent fixture".into()),
            ..Default::default()
        })?;
        assert_eq!(page.total, 1);
        assert_eq!(page.messages[0].id, draft.id);
        assert_eq!(page.messages[0].remote_uid, Some(42));
        assert!(!store.claim_delivery(&draft.id)?);
        sent.id = "incoming-copy".into();
        sent.folder = "inbox".into();
        sent.remote_mailbox = Some("INBOX".into());
        sent.remote_uid = Some(43);
        sent.body = "Incoming mail cannot replace a delivery receipt".into();
        store.save_message(&sent)?;
        assert_eq!(store.message(&draft.id)?.body, "hello");
        let account = store.account(FIXTURE_ACCOUNT_ID)?;
        let bytes = crate::providers::build_mail(&account, &store.message(&draft.id)?)?.formatted();
        use mailparse::MailHeaderMap;
        let parsed = mailparse::parse_mail(&bytes)?;
        assert_eq!(
            parsed.headers.get_first_value("Message-ID"),
            store.message(&draft.id)?.message_id
        );
        Ok(())
    }
    #[test]
    fn inbox_changes_survive_restart_and_drafts_are_not_resent() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("mail.db");
        let store = store_with_fixture(&path)?;
        store.patch(
            "sample-1",
            &MessagePatch {
                folder: Some("archive".into()),
                ..Default::default()
            },
        )?;
        let draft = Compose {
            id: Uuid::new_v4().to_string(),
            account_id: FIXTURE_ACCOUNT_ID.into(),
            to: vec!["sophie@form.studio".into()],
            subject: "Re: Ideas".into(),
            body: "Let's explore this.".into(),
            in_reply_to: None,
        };
        store.save_draft(&draft)?;
        assert!(store.claim_delivery(&draft.id)?);
        assert!(store.claim_delivery(&draft.id).is_err());
        assert!(store.save_draft(&draft).is_err());
        store.finish_delivery(&draft.id, true)?;
        drop(store);
        let reopened = Store::open(&path)?;
        assert_eq!(reopened.message("sample-1")?.folder, "archive");
        assert!(!reopened.claim_delivery(&draft.id)?);
        assert_eq!(reopened.message(&draft.id)?.folder, "sent");
        assert_eq!(
            reopened
                .list(&MessageQuery {
                    q: Some("\"EXPLORE THIS\"".into()),
                    ..Default::default()
                })?
                .total,
            1
        );
        Ok(())
    }
    #[test]
    fn removing_an_account_deletes_only_its_local_data() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = store_with_fixture(&dir.path().join("mail.db"))?;
        let mut connected = store.account(FIXTURE_ACCOUNT_ID)?;
        connected.id = "connected".into();
        store.save_account(&connected)?;
        let mut other = connected.clone();
        other.id = "other".into();
        store.save_account(&other)?;
        let mut cached = store.message("sample-1")?;
        cached.id = "connected-1".into();
        cached.account_id = connected.id.clone();
        store.save_message(&cached)?;
        let mut kept = cached.clone();
        kept.id = "other-1".into();
        kept.account_id = other.id.clone();
        store.save_message(&kept)?;
        let draft = store.save_draft(&Compose {
            id: Uuid::new_v4().to_string(),
            account_id: connected.id.clone(),
            to: vec!["sophie@form.studio".into()],
            subject: "Unsent".into(),
            body: "Draft".into(),
            in_reply_to: None,
        })?;
        assert!(store.claim_delivery(&draft.id)?);
        store.finish_delivery(&draft.id, false)?;
        let calendar =
            crate::calendar::calendar_id(CalendarSource::Google, &connected.id, "primary");
        store.save_preferences(PreferencesPatch {
            default_account_id: Some(connected.id.clone()),
            default_calendar: Some(calendar),
            ..Default::default()
        })?;

        assert_eq!(store.remove_account(&connected.id)?.id, connected.id);
        assert!(store.account(&connected.id).is_err());
        assert!(store.message(&cached.id).is_err());
        assert!(store.message(&draft.id).is_err());
        // The uncertain delivery record goes with its draft; a reused draft id is not blocked.
        assert!(store.claim_delivery(&draft.id)?);
        assert_eq!(store.preferences()?.default_account_id, "");
        assert_eq!(store.preferences()?.default_calendar, "");
        // Another owner's default calendar stays.
        let kept_calendar = crate::calendar::calendar_id(
            CalendarSource::Caldav,
            "dav-1",
            "https://dav.example.test/home/",
        );
        store.save_preferences(PreferencesPatch {
            default_calendar: Some(kept_calendar.clone()),
            ..Default::default()
        })?;
        for id in ["dav-1", "dav-2"] {
            store.save_dav_source(&DavSource {
                id: id.into(),
                kind: DavKind::Caldav,
                name: id.into(),
                url: "https://dav.example.test/".into(),
                username: "alex".into(),
                last_synced: None,
                error: None,
            })?;
        }
        store.remove_dav_source("dav-2")?;
        assert_eq!(store.preferences()?.default_calendar, kept_calendar);
        store.remove_dav_source("dav-1")?;
        assert_eq!(store.preferences()?.default_calendar, "");
        assert_eq!(store.message(&kept.id)?.account_id, other.id);
        assert!(store.account(&other.id).is_ok());
        assert!(
            store
                .remove_account(&connected.id)
                .unwrap_err()
                .to_string()
                .contains("not found")
        );
        Ok(())
    }
    #[test]
    fn search_ranks_sender_above_body_and_its_index_follows_every_change() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("mail.db");
        let store = store_with_fixture(&path)?;
        let mut sender = store.message("sample-1")?;
        sender.id = "sender".into();
        sender.from_name = "Quarterly Reports".into();
        sender.subject = "Hello".into();
        sender.body = "Nothing to see".into();
        sender.date = (Utc::now() - chrono::Duration::days(3)).to_rfc3339();
        let mut body = sender.clone();
        body.id = "body".into();
        body.from_name = "Zed".into();
        body.body = "The QUARTERLY forecast".into();
        body.date = Utc::now().to_rfc3339();
        store.save_messages([sender.clone(), body.clone()])?;
        let ids = |store: &Store, q: &str| -> Result<Vec<String>> {
            Ok(store
                .list(&MessageQuery {
                    q: Some(q.into()),
                    ..Default::default()
                })?
                .messages
                .into_iter()
                .map(|m| m.id)
                .collect())
        };
        assert_eq!(
            ids(&store, "quarterly")?,
            ["sender", "body"],
            "A sender hit outranks a newer body hit"
        );
        assert_eq!(ids(&store, "qaurterly")?, ["sender"], "Typos match headers");
        store.patch(
            "sender",
            &MessagePatch {
                is_read: Some(true),
                ..Default::default()
            },
        )?;
        assert_eq!(ids(&store, "quarterly is:read")?, ["sender"]);
        body.body = "Rescheduled".into();
        store.save_message(&body)?;
        assert!(
            !ids(&store, "forecast")?.contains(&body.id),
            "An edited body is reindexed"
        );
        assert_eq!(ids(&store, "resched")?, ["body"]);
        let draft = store.save_draft(&Compose {
            id: Uuid::new_v4().to_string(),
            account_id: FIXTURE_ACCOUNT_ID.into(),
            to: vec![],
            subject: "Quarterly draft".into(),
            body: String::new(),
            in_reply_to: None,
        })?;
        assert_eq!(ids(&store, "draft quarterly")?, [draft.id.as_str()]);
        store.discard_draft(&draft.id)?;
        assert!(
            ids(&store, "draft quarterly")?.is_empty(),
            "A deleted message leaves the index"
        );
        drop(store);
        // An index that fell out of step is rebuilt on open.
        Connection::open(&path)?.execute_batch("DELETE FROM message_index; INSERT INTO message_bodies(message_bodies) VALUES('delete-all');")?;
        let store = Store::open(&path)?;
        assert_eq!(ids(&store, "quarterly")?, ["sender"]);
        assert_eq!(ids(&store, "from:zed resched")?, ["body"]);
        Ok(())
    }

    #[test]
    fn spam_conversations_are_isolated_and_restoring_inbox_rejoins_normal_mail() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = store_with_fixture(&dir.path().join("mail.db"))?;
        let mut normal = store.message("sample-1")?;
        normal.subject = "Quarantine fixture".into();
        normal.date = "2026-10-01T10:00:00Z".into();
        normal.is_read = true;
        normal.is_starred = false;
        let mut reply = normal.clone();
        reply.id = "normal-reply".into();
        reply.folder = "sent".into();
        reply.subject = "Re: Quarantine fixture".into();
        reply.date = "2026-10-01T11:00:00Z".into();
        reply.message_id = Some("<normal-reply@x>".into());
        let mut trash = reply.clone();
        trash.id = "normal-trash".into();
        trash.folder = "trash".into();
        trash.message_id = Some("<normal-trash@x>".into());
        let mut spam = reply.clone();
        spam.id = "spam-reply".into();
        spam.folder = "inbox".into();
        spam.from_name = "Unwanted sender".into();
        spam.from_email = "unwanted@example.test".into();
        spam.body = "Quarantined body".into();
        spam.date = "2026-10-01T12:00:00Z".into();
        spam.message_id = Some("<spam-reply@x>".into());
        spam.is_read = false;
        spam.is_starred = true;
        store.save_messages([normal, reply, trash, spam])?;
        store.patch(
            "spam-reply",
            &MessagePatch {
                folder: Some("spam".into()),
                ..Default::default()
            },
        )?;
        let query = |folder: Option<&str>| MessageQuery {
            folder: folder.map(str::to_string),
            q: Some("Quarantine fixture".into()),
            ..Default::default()
        };
        for folder in [None, Some("inbox"), Some("sent")] {
            let page = store.threads(&query(folder))?;
            assert_eq!(page.all_total, 1);
            assert_eq!(page.unread_total, 0);
            let thread = &page.threads[0];
            assert_eq!(thread.count, 2);
            assert!(thread.unread_ids.is_empty());
            assert!(!thread.is_starred);
            assert!(!thread.participants.iter().any(|p| p == "Unwanted sender"));
        }
        assert_eq!(store.threads(&query(Some("starred")))?.total, 0);
        assert_eq!(store.list(&query(Some("starred")))?.total, 0);
        assert_eq!(
            store
                .list(&MessageQuery {
                    q: Some("Quarantined body".into()),
                    ..Default::default()
                })?
                .total,
            0
        );
        let ids = |messages: Vec<Message>| messages.into_iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(
            ids(store.conversation("sample-1")?),
            ["sample-1", "normal-reply"]
        );
        assert_eq!(
            ids(store.conversation("normal-trash")?),
            ["sample-1", "normal-reply", "normal-trash"]
        );
        assert_eq!(ids(store.conversation("spam-reply")?), ["spam-reply"]);
        let spam_page = store.threads(&query(Some("spam")))?;
        assert_eq!(spam_page.total, 1);
        assert_eq!(spam_page.unread_total, 1);
        assert_eq!(spam_page.threads[0].count, 1);
        assert_eq!(spam_page.threads[0].ids, ["spam-reply"]);
        assert_eq!(spam_page.threads[0].unread_ids, ["spam-reply"]);
        assert_eq!(spam_page.threads[0].participants, ["Unwanted sender"]);

        store.patch(
            "spam-reply",
            &MessagePatch {
                folder: Some("inbox".into()),
                ..Default::default()
            },
        )?;
        assert_eq!(store.threads(&query(Some("spam")))?.total, 0);
        let inbox = store.threads(&query(Some("inbox")))?;
        assert_eq!(inbox.unread_total, 1);
        assert_eq!(inbox.threads[0].count, 3);
        assert_eq!(inbox.threads[0].unread_ids, ["spam-reply"]);
        assert!(inbox.threads[0].is_starred);
        assert_eq!(
            ids(store.conversation("spam-reply")?),
            ["sample-1", "normal-reply", "spam-reply"]
        );
        assert_eq!(
            store.list(&query(Some("starred")))?.messages[0].id,
            "spam-reply"
        );
        Ok(())
    }
}
