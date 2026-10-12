# Performance, diagnostics and support

Research behind the matching items in [`todo.md`](../../todo.md). Each entry covers why it matters, how Mailspring and other clients do it, the relevant specs, what Alice has today, a build plan, pitfalls, acceptance criteria and effort. Code citations point at the Alice working tree as of 2026-10-13; Mailspring paths are relative to [Foundry376/Mailspring](https://github.com/Foundry376/Mailspring) (`mailspring/`) and [Foundry376/Mailspring-Sync](https://github.com/Foundry376/Mailspring-Sync) (`mailsync/`).

## Section findings

**One storage decision drives most of the performance problems.** Alice keeps each message as one JSON document in `messages(id, data)`, and that document includes `body` and `html_body` (`crates/alice-core/src/model.rs:89-117`, `crates/alice-core/src/store.rs:20`). Three costs follow:

- **Every flag change rewrites the bodies.** Marking a message read, starring it or moving its primary copy is a `json_set` on the whole blob (`sync_store.rs:199-206`, `sync_store.rs:375`). The rewrite fires the `message_index` trigger, and the body trigger parses both the old and new JSON to compare bodies (`store.rs:1168-1174`). A 100 KB newsletter costs about 100 KB of WAL writes per flag flip.
- **Every list response ships bodies.** Each row of `/v1/threads` is a `ThreadSummary` whose `message` is a full `Message`, including `body` and `html_body` (`model.rs:154-167`, `store.rs:246-248`). One page is 50 rows, so a single page can carry megabytes of HTML across Tauri IPC.
- **Every list call scans the whole mailbox.** `threads()` reads every message id in the view (`store.rs:149`), then loads a `ThreadRow` for every message of the account and computes the subject key in Rust (`store.rs:162`, `store.rs:1307-1324`). `/v1/state` calls `threads()` too, to get the unread count (`daemon.rs:374-379`). The desktop calls `/v1/state` every 2 s (`apps/desktop/src/App.tsx:139`) and the TUI does the same (`crates/alice/src/tui.rs:247`).

**One mutex serializes all database access.** All reads and writes share one `parking_lot::Mutex<Connection>` (`store.rs:10`). Axum handlers call the store directly on Tokio worker threads (`daemon.rs:384-409`). So a long sync transaction stalls the UI and also blocks async workers. The worst case is a full flag refresh on a server without CONDSTORE: `apply_flags` writes every known UID in one transaction (`sync.rs:391-393`, `sync_store.rs:362-399`).

**Recommended build order:**

1. Commit the benchmark fixture first, so every later change is measured. Today's 10,000-message probe only lived in `/tmp` (`docs/reviews/2026-10-11/fixes/evidence/performance.md`).
2. Split the writer from the readers, and move the revision into a counter the writer updates. That counter also makes push possible.
3. Run one schema migration:
   - hot columns move out of the JSON blob;
   - bodies move to their own table;
   - messages get an `INTEGER PRIMARY KEY`, so that VACUUM can no longer renumber the rowids the FTS index points at;
   - `auto_vacuum` switches to INCREMENTAL.
4. Page threads in SQL and stop shipping bodies in list rows.
5. Push changes by long-polling.
6. Virtualize the list.
7. Diagnostics, in order: logging, then copying the sync log, then the setup protocol log, then crash reports. Maintenance and count repair come after the schema work.

**No new crates are needed for the core work:**

- `rusqlite_migration` stores its version in `PRAGMA user_version`, which Alice already uses for `SEARCH_INDEX_VERSION` (`store.rs:1139`). Its current release also needs rusqlite 0.40, while Alice is on 0.38 (`crates/alice-core/Cargo.toml:21`). Alice already has a per-part version table, `schema_parts` (`sync_store.rs:23`); reuse it.
- `r2d2_sqlite` 0.35 and `deadpool-sqlite` 0.14 also need rusqlite 0.40. The 0.38-compatible releases are `r2d2_sqlite` 0.32 and `deadpool-sqlite` 0.13. A pool of four read-only connections is about 30 lines without either crate.

**Coordinate the schema with the Sync correctness section.** The new `thread` column should first hold today's subject key. The References/X-GM-THRID threading item then only changes how the column is filled. If "Multi-folder message model" lands first, the phase-2 `thread_view` table should be keyed by folder/label id built from `remote_messages`, as Mailspring 1.26 does with `MessageFolder` + `ThreadCategory`.

**Diagnostics today:**

- **Logs are thrown away.** The daemon is spawned with stdout and stderr sent to null (`crates/alice-core/src/client.rs:127-132`). The only record of a failure is the last `account.sync_error` string.
- **Crash backtraces would have no symbols.** Release builds strip them (`Cargo.toml:50`, `strip = true`).

**Privacy rules for every diagnostic feature.** Log account ids, mailbox names, counts and UIDs. Never log subjects, addresses or bodies. Redact IMAP `LOGIN`/`AUTHENTICATE`, SMTP `AUTH` and `334` continuations. Write crash reports locally first; send nothing without an explicit action. Mailspring runs Sentry always-on, identifies devices by a MAC-address hash (`repos/mailspring/app/src/error-logger-extensions/sentry-error-reporter.js:2,176-180`), and auto-submits native crashes (`error-logger.js:162-170`). Alice's "your emails stay on this device" promise rules that out.

---

## P0 → Performance foundations

<a id="move-hot-fields-date-account-mailbox-unread-starred-thread-id-out-of-the-json-bl"></a>

### Move hot fields (date, account, mailbox, unread, starred, thread id) out of the JSON blob into real indexed columns

*Roadmap: P0 · Performance foundations*

- **Why:**
  - Lists, counts and sync lookups go through `json_extract` expression indexes over a blob that includes the bodies.
  - Every flag change rewrites the whole message, HTML included, and re-runs the search triggers.
  - At 100k messages this makes sync and the UI fight over one lock, with large WAL churn.
- **Competitors:**
  - **Mailspring's message and thread tables** (`mailspring-schema.sql`):
    - `Message` has real columns: `accountId, headerMessageId, gMsgId, gThrId, subject, date, draft, unread, starred, threadId`.
    - The JSON `data` column holds only cold fields.
    - Bodies live in a separate `MessageBody(id, value, fetchedAt)`.
    - `Thread` is a real table with denormalized `unread, starred, lastMessageReceivedTimestamp, lastMessageSentTimestamp, inAllMail, participants, hasAttachments`.
  - **Mailspring's list and count tables:**
    - `ThreadCategory(id, value, inAllMail, unread, lastMessageReceivedTimestamp, lastMessageSentTimestamp)` has one row per thread × folder/label. Lists are an index range scan on `ThreadListCategoryIndex (lastMessageReceivedTimestamp DESC, value, inAllMail, unread, id)`.
    - Partial indexes cover hot views: `ThreadUnreadIndex … WHERE unread = 1 AND inAllMail = 1`, `ThreadStarredIndex`, `MessageListDraftIndex … WHERE draft = 1`.
    - `ThreadCounts(categoryId, unread, total)` feeds sidebar badges (`repos/mailspring/app/src/flux/stores/thread-counts-store.ts:31`).
  - **History and lessons:**
    - Changelog 1.0.10: "SQLite schema changes significantly improve Mailspring's sync performance on very large mailboxes."
    - 1.26 added `MessageFolder` placements (`rowid INTEGER PRIMARY KEY, accountId, messageId, folderId, remoteUID, unread, starred, …`), the equivalent of Alice's `remote_messages`.
    - Mailspring's counters are maintained by `+=`/`-=` arithmetic (`repos/mailsync/MailSync/Models/Thread.cpp:406`, `UPDATE ThreadCounts SET unread = unread + ?`). They drifted, and needed the V11 recount migration (`MailStore.cpp:370-430`, changelog 1.26 "Folder and unread counts left wrong by previous versions are repaired automatically").
    - Lesson for Alice: recompute aggregates for the touched threads; don't increment them.
- **Spec:**
  - SQLite expression indexes: https://www.sqlite.org/expridx.html
  - Partial indexes: https://www.sqlite.org/partialindex.html
  - ROWID and INTEGER PRIMARY KEY: https://www.sqlite.org/lang_createtable.html#rowid
  - VACUUM "may change the ROWIDs of entries in any tables that do not have an explicit INTEGER PRIMARY KEY": https://www.sqlite.org/lang_vacuum.html
- **Alice today:**
  - **Schema:**
    - `messages(id TEXT PRIMARY KEY, data TEXT)` with four `json_extract` expression indexes (`store.rs:20,27-30`).
    - A trigger-maintained projection, `message_index`, already holds most hot fields (folder, account, category, from, recipients, subject, is_read, is_starred, has_attachments, julian date, message_id; `store.rs:1157-1178`). It is effectively an interim hot-column table.
    - FTS5 `message_bodies` is contentless and keyed by the `messages` implicit rowid (`store.rs:1163`).
  - **Open-time cost:** every open runs an O(n) "in step" check that rebuilds the index if rowids moved (`store.rs:1180-1193`).
  - **Where `json_extract` is used:** 39 call sites in `store.rs` (22) and `sync_store.rs` (17), and nowhere else. Examples:
    - upsert identity lookups (`store.rs:1374,1384`);
    - account deletion (`store.rs:1403-1406`);
    - flags (`sync_store.rs:375`);
    - `make_primary` runs up to three full-row `json_set` updates per message (`sync_store.rs:199-206`);
    - `settle` (`sync_store.rs:220`);
    - storage stats sum `length(data)` over every row (`sync_store.rs:624`).
  - **Dates** are RFC 3339 strings sorted with `julianday(json_extract(...))` (`store.rs:87`).
  - **Query building is already abstracted:** `scope_filter` takes a column-mapping function (`store.rs:1108-1137`), and `search()` already passes `str::to_string` to address plain columns (`store.rs:1217`). Switching the mapper is a one-line change per caller.
- **Plan:**
  1. **Migration in Rust, one transaction.** Model it on mailsync's `_migrateToV10` (`repos/mailsync/MailSync/MailStore.cpp:298-360`), recorded in `schema_parts` as `('messages', 2)`:
     - check free disk ≥ 1.5× `page_count*page_size` first;
     - build the new table with a single `INSERT … SELECT json_extract(...)` (no Rust deserialization);
     - copy bodies to `message_body`;
     - drop the `message_index` table and its triggers;
     - rename the new table to `messages`;
     - re-create `message_bodies` and refill it keyed by `n`.

     Target DDL:
     ```sql
     CREATE TABLE messages_v2(
       n INTEGER PRIMARY KEY,               -- stable rowid alias: FTS key, survives VACUUM
       id TEXT NOT NULL UNIQUE,
       account_id TEXT NOT NULL,
       thread TEXT NOT NULL,                -- today: account + thread_key(subject), or 'message:<id>' for drafts/empty subjects
       folder TEXT NOT NULL, category TEXT NOT NULL,
       date INTEGER NOT NULL,               -- Unix ms UTC; the RFC 3339 string stays in data
       is_read INTEGER NOT NULL, is_starred INTEGER NOT NULL, has_attachments INTEGER NOT NULL,
       from_name TEXT NOT NULL, from_email TEXT NOT NULL, recipients TEXT NOT NULL,
       subject TEXT NOT NULL, preview TEXT NOT NULL, message_id TEXT,
       remote_mailbox TEXT, uid_validity INTEGER, remote_uid INTEGER,
       size INTEGER NOT NULL,               -- bytes of text+html, for /v1/storage
       data TEXT NOT NULL                   -- cold JSON: to, reply_to, attachments, invite, in_reply_to, date
     );
     CREATE TABLE message_body(n INTEGER PRIMARY KEY REFERENCES messages_v2(n) ON DELETE CASCADE, text TEXT NOT NULL, html TEXT);
     CREATE INDEX messages_view     ON messages_v2(folder, date DESC);
     CREATE INDEX messages_account  ON messages_v2(account_id, folder, date DESC);
     CREATE INDEX messages_thread   ON messages_v2(account_id, thread, date);
     CREATE INDEX messages_unread   ON messages_v2(folder, account_id, thread) WHERE is_read = 0;
     CREATE INDEX messages_starred  ON messages_v2(date DESC) WHERE is_starred = 1;
     CREATE INDEX messages_identity ON messages_v2(account_id, message_id);
     CREATE INDEX messages_remote   ON messages_v2(account_id, remote_mailbox, uid_validity, remote_uid);
     ```
     - **Date conversion:** `CAST((julianday(json_extract(data,'$.date')) - 2440587.5) * 86400000 AS INTEGER)`; an unparsable date becomes 0.
     - **Thread column:** `thread_key` is Rust-only (`store.rs:1262-1293`), so the migration first copies with `thread = ''`. A second pass, in chunks of 1,000, fills it from Rust. Alternatively, register a rusqlite scalar function (`Connection::create_scalar_function`, which needs the `functions` feature) and do it in SQL.
     - **FTS:** keep it contentless, with triggers on `message_body` instead of `messages`.
     - **Finish:** set `PRAGMA auto_vacuum=INCREMENTAL` and run one `VACUUM`. Changing auto_vacuum is allowed in WAL mode (https://www.sqlite.org/lang_vacuum.html). Then run `PRAGMA optimize`.
  2. **Remove the open-time scan.** With rowids stable, `ensure_search_index` (`store.rs:1151`) keeps only the version check and drops the O(n) in-step check.
  3. **Data model.** `Message` keeps its JSON shape for the API. Add `fn read_message(row)` and `fn write_message(tx, &Message)` in `store.rs`; they are the only code that maps columns.
     - `upsert` (`store.rs:1368`) writes columns, and writes `message_body` only when the content changed.
     - `make_primary`, `apply_flags` and `settle` become plain column `UPDATE`s, with `IS NOT` guards so no-op writes are skipped.
     - `delete_account` uses `account_id = ?`.
     - `storage()` uses `sum(size)`.
  4. **Queries.** Replace every `json_extract(data,'$.x')` with the column name. `scope_filter` callers pass `str::to_string`; date ordering becomes `ORDER BY date DESC, id`.
  5. **Tests.** Old-schema fixtures are built by executing the old DDL plus raw `INSERT INTO messages VALUES(id, json)`. The existing store tests (`store.rs:1455+`) must pass unchanged.
  6. **Phase 2, after threading lands, or earlier if the benchmark says list-open > 50 ms at 100k.** Add `thread_view` (Mailspring's `ThreadCategory`):
     ```sql
     CREATE TABLE thread_view(account_id TEXT NOT NULL, thread TEXT NOT NULL, folder TEXT NOT NULL,
       newest INTEGER NOT NULL, unread INTEGER NOT NULL, starred INTEGER NOT NULL, messages INTEGER NOT NULL,
       PRIMARY KEY(account_id, thread, folder)) WITHOUT ROWID;
     CREATE INDEX thread_view_list    ON thread_view(folder, newest DESC);
     CREATE INDEX thread_view_account ON thread_view(account_id, folder, newest DESC);
     CREATE INDEX thread_view_unread  ON thread_view(folder, newest DESC) WHERE unread > 0;
     ```
     Maintain it at the end of each write transaction by recomputing only the touched `(account_id, thread)` groups: `DELETE … WHERE account_id=? AND thread IN (…)`, then `INSERT … SELECT … GROUP BY folder`. Never apply deltas.
- **Pitfalls:**
  - **VACUUM:** don't run it before this migration (see the maintenance item). Today, VACUUM may renumber `messages` rowids and so force a full FTS rebuild on the next open (`store.rs:1147-1150`).
  - **Migration time:** budget 1–3 min for a 1 GB database, and show progress. Mailspring prints "Running Migration", and the client opens a status window when it sees that line (`repos/mailspring/app/src/mailsync-process.ts:505-508`).
  - **Disk space:** the old table's pages are only reclaimed by the VACUUM. Mailspring measured +385 MB file growth and a similar WAL peak on 1.15 GB (`MailStore.cpp:305-310`).
  - **Gmail duplicates:** Gmail stores one message in several mailboxes. Keep `remote_messages` as the placement table and keep its "primary copy" semantics (`sync_store.rs:1-12`).
  - **Uniqueness:** don't put a UNIQUE constraint on `(account_id, remote_mailbox, uid_validity, remote_uid)`. Legacy rows may violate it and abort the migration.
  - **Draft keys:** drafts and empty subjects need per-message `thread` values (`store.rs:182-186`), or they collapse into one conversation.
  - **Migration failure:** keep the migration idempotent; on failure, roll back and leave `schema_parts` unchanged so the next launch retries (mailsync V10 does this).
- **Done when:**
  - A new test, `legacy_json_messages_migrate_to_columns`, builds an old-schema database with 200 messages (drafts, spam, Gmail multi-copy, an unparsable date). After `Store::open`, every message round-trips equal and search returns the same ids.
  - `EXPLAIN QUERY PLAN` for the inbox page shows `USING INDEX messages_account` and no `USE TEMP B-TREE FOR ORDER BY`; assert it in a test.
  - The benchmark shows `apply_flags(1,000)` under 50 ms, and WAL growth for that call under 1 MB at 100k messages.
  - No `json_extract(data` remains for hot fields (grep).
- **Effort:** L. Depends on: "Add a benchmark fixture" (to measure). Enables: "Page the thread list in SQL", "Periodic SQLite maintenance", "Automatic repair of folder and unread counts". Coordinate with "Thread by `Message-ID` / `References`…" and "Multi-folder message model".

<a id="page-the-thread-list-in-sql-stop-loading-every-message-id-and-every-thread-row-o"></a>

### Page the thread list in SQL; stop loading every message id and every thread row on each `threads()` call

*Roadmap: P0 · Performance foundations*

- **Why:**
  - List-open, every paging step and every 2 s `/v1/state` poll cost O(all messages of the account) in CPU and memory.
  - The thread reader does the same scan: `conversation()` loads `thread_rows` for the whole account (`store.rs:281`).
  - The 2026-10-11 probe measured 9 ms for three one-row inbox queries at 10k messages. The work is linear, so 100k would be roughly 10× slower; that is an estimate, and the benchmark will measure it.
- **Competitors:**
  - **Mailspring's paging:**
    - It queries `Thread INNER JOIN ThreadCategory … ORDER BY lastMessageReceivedTimestamp DESC LIMIT/OFFSET` for only the rows on screen. The SQL shape is in `repos/mailspring/app/src/flux/attributes/attribute-collection.ts:24`.
    - The visible range is retained by `ListTabular.getRange()`: viewport rows ±2 (`app/src/components/list-tabular.tsx:306-322`).
    - That range drives `QuerySubscription` range queries (`flux/models/query-subscription.ts:230`).
    - Counts come from `ThreadCounts`, not from scanning.
  - **Mailspring's diagnostics:**
    - Queries over 100 ms are logged with `EXPLAIN QUERY PLAN` (`app/src/flux/stores/database-store.ts:307-321`).
    - Slow queries run in a forked background agent (`database-store.ts:346-380`).
  - **Others:** Thunderbird's message index (msf/Gloda) is also windowed. Apple Mail and Outlook page from local indexes.
- **Alice today:**
  - `threads()` (`store.rs:139-271`):
    - collects all matching ids (`:149`);
    - loads every `ThreadRow` of the account or of all accounts (`:162`);
    - groups them in HashMaps;
    - sorts and computes totals;
    - then deserializes the full JSON of the 50 shown newest messages (`:206,246`).
  - `/v1/state` runs `threads(limit 1)` just for `unread_total` (`daemon.rs:374-381`).
  - Per-row fields `count`, `unread_ids` and `participants` come from the Rust `conversation()` dedupe rules (`store.rs:1328-1365`), which must be preserved.
  - The thread list rows include full bodies (`model.rs:157`). The desktop uses the row's message as a stand-in reader until the conversation loads (`App.tsx:83-88`), and opens drafts from the row (`App.tsx:210`).
- **Plan:**
  1. **Page query.** After the column migration, the view query becomes one statement with window functions; SQLite has supported them since 3.25:
     ```sql
     SELECT account_id, thread, newest, unread, count(*) OVER () AS total, sum(unread) OVER () AS unread_total
     FROM (SELECT account_id, thread, max(date) AS newest, max(is_read = 0) AS unread
           FROM messages WHERE <scope_filter> GROUP BY account_id, thread)
     [WHERE unread]  ORDER BY newest DESC, thread LIMIT ?l OFFSET ?o;
     ```
     This preserves today's semantics:
     - list membership comes from in-view messages;
     - `unread` means "any listed message unread" (`store.rs:199-202`);
     - spam is excluded when no folder is given;
     - `starred` means `is_starred = 1 AND folder NOT IN ('trash','spam')`.
  2. **Row details for the page only.** For the ≤200 threads on the page, run `SELECT … FROM messages WHERE account_id=? AND thread=?` (indexed by `messages_thread`). Apply the existing `conversation()` filter to compute `count`, `unread_ids`, `participants` and `is_starred`. The only change is a smaller input.
  3. **Search.** Keep `search()` ranking. Group the hit list by its `thread` column, not by `thread_rows`. Look up only the page's threads, as in step 2.
  4. **Thread reader.** `conversation(id)` uses `WHERE account_id=? AND thread=?`, not the whole account.
  5. **Snapshot unread count.** `/v1/state` uses `SELECT count(*) FROM (SELECT 1 FROM messages WHERE folder='inbox' AND is_read=0 [AND account_id=?] GROUP BY account_id, thread)`, served by the partial index `messages_unread`. Drop the `threads()` call.
  6. **(NEW, folded in) Slim list payload.** `ThreadSummary.message` becomes a `MessageSummary`: everything except `body`, `html_body` and `invite.ics`. Update `apps/desktop/src/api.ts` (`Thread`).
     - The reader shows the row header plus a body skeleton until `/v1/threads/{id}` returns.
     - `createCompose` for a draft fetches `/v1/messages/{id}` first.
     - The CLI, TUI and MCP use `/v1/messages`, not `/v1/threads` (`crates/alice/src/tui.rs:253`, `mcp.rs:174`), so they are unaffected.
  7. **Off the async workers.** Run `threads`, `conversation` and search in `tokio::task::spawn_blocking` in `daemon.rs`.
  8. **Phase 2.** If the benchmark shows GROUP BY over 100k inbox rows exceeds budget, switch step 1 to `thread_view` (previous item). It is then an index range scan with no grouping, like `ThreadListCategoryIndex`.
- **Pitfalls:**
  - **OFFSET at large pages** is O(offset), but over a covering index that is fine at 100k. Keyset paging breaks random access from the scrollbar; Mailspring fixed scrollbar dragging in 1.21.1 (#2716).
  - **Clamp:** keep the `limit` clamp at 200 (`store.rs:205`).
  - **Conversation rules:** Message-ID dedupe and the trash/spam rules (`store.rs:1325-1365`) are subtle. Keep the Rust function as the single implementation and feed it a smaller input.
  - **Stand-in reader:** without bodies, the reader shows an empty body for one round trip. Keep the row's `preview` visible so it doesn't flash.
- **Done when:**
  - Existing tests pass unchanged: `conversations_group_by_subject_once_and_follow_the_view`, `spam_conversations_are_isolated_and_restoring_inbox_rejoins_normal_mail`, `category_counts_and_pages_use_the_same_account_scope`.
  - A new randomized test, `sql_paging_matches_reference_grouping`, keeps the old in-Rust grouping in the test module as an oracle. Over 2,000 generated messages it compares page contents and totals for every folder × unread × account × page combination.
  - Benchmark at 100k / 5 accounts:
    - unified inbox page 0 under 30 ms;
    - `/v1/state` under 5 ms;
    - thread open under 10 ms;
    - peak RSS independent of mailbox size, within 10%.
  - The `/v1/threads?limit=50` response is under 100 KB for a fixture with 100 KB HTML bodies.
- **Effort:** M. Depends on: "Move hot fields … into real indexed columns".

<a id="separate-read-connections-pool-so-sync-writes-dont-block-ui-reads-keep-the-singl"></a>

### Separate read connections (pool) so sync writes don't block UI reads; keep the single writer

*Roadmap: P0 · Performance foundations*

- **Why:** While a sync batch or a large flag refresh holds the only connection, every list, open and search waits. The handler also blocks a Tokio worker thread while it waits.
- **Competitors:**
  - **Mailspring's reader:** the Electron client opens the database read-only in `better-sqlite3` (`{ readonly: true, timeout: 10000 }`, `journal_mode = WAL`, `cache_size = 20000`, `synchronous = NORMAL`; `repos/mailspring/app/src/flux/stores/database-store.ts:51-67`). It retries "database is locked" with exponential backoff (`:268-330`) and forks a background agent process for slow reads.
  - **Mailspring's writers:**
    - Each account's `mailsync` process is a writer using `BEGIN IMMEDIATE`, `setBusyTimeout(60*1000)` and `synchronous=NORMAL` (`repos/mailsync/MailSync/MailStore.cpp:89-114`).
    - Every MailStore call must come from its owning thread (`assertCorrectThread`, `:432-450`).
    - Several writer processes caused the busy-timeout bugs fixed in 1.9.1 ("connections to its database no longer time out … syncing more than ~4 accounts") and 1.21.1 ("sync engine crash during many-account sync by increasing SQLite busy timeout").
    - Alice has one in-process writer, so it avoids that whole class.
- **Spec:**
  - WAL concurrency and checkpoint starvation: https://www.sqlite.org/wal.html
  - `PRAGMA data_version` is per-connection and "comparing values from different connections is not meaningful": https://www.sqlite.org/pragma.html#pragma_data_version
- **Alice today:**
  - `Store { db: Mutex<Connection>, epoch }` (`store.rs:9-13`).
  - The open sets only `journal_mode=WAL; foreign_keys=ON` (`store.rs:18`): no `busy_timeout`, `synchronous` or `cache_size`.
  - `revision()` is `epoch.total_changes()` of that single connection (`store.rs:1103-1105`). That only works because there is one connection.
  - There are 45 `db.lock()` call sites (store.rs 27, sync_store.rs 13, otp.rs 3, ai.rs 1, daemon.rs test 1).
  - Sync takes the per-account Tokio lock around each store write (`sync.rs:271-274`), but the database mutex is global.
  - The existing test `tests/vault_responsiveness.rs` already shows the pattern for asserting that the runtime stays responsive while a resource is blocked.
- **Plan:**
  1. **Connections.** Keep `db` as the writer: `PRAGMA synchronous=NORMAL; busy_timeout` via `Connection::busy_timeout(5s); cache_size=-16384; journal_size_limit=67108864; temp_store=MEMORY; optimize=0x10002`. After migrations, open `readers: Vec<parking_lot::Mutex<Connection>>`, four of them, with `Connection::open_with_flags(path, SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX | SQLITE_OPEN_URI)`, `busy_timeout(5s)` and `cache_size=-4096`.
  2. **The `read` helper.**
     ```rust
     fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
         let start = self.next.fetch_add(1, Ordering::Relaxed);
         for i in 0..self.readers.len() {
             if let Some(c) = self.readers[(start + i) % self.readers.len()].try_lock() { return f(&c); }
         }
         f(&self.readers[start % self.readers.len()].lock())
     }
     ```
     Read-only methods switch from `self.db.lock()` to `self.read(|db| …)`. These include `accounts`, `account`, `list`, `threads`, `conversation`, `message`, `preferences`, `ai_settings`, `dav_sources`, `address_book`, `each_correspondence`, `mailbox_state`, `known_uids`, `sync_totals`, `storage` and `pending_otp`. Multi-statement reads (count plus page) run inside `db.unchecked_transaction()`, so they see one snapshot.
  3. **The write path, revision and change signal.** Writes use `fn write<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T>`.
     - It compares `total_changes()` before and after `f`.
     - Once `f` has returned and the transaction has committed, it stores the new value in `changes: tokio::sync::watch::Sender<u64>` with `send_replace`.
     - `revision()` becomes `format!("{epoch}.{}", *changes.borrow())`.
     - Bump only after commit. A bump from `commit_hook` fires before the data is visible to readers, so a client could cache stale rows under the new revision.
  4. **Off the async workers.** Wrap heavy daemon handlers in `spawn_blocking`: `threads`, `messages` with `q`, `thread`, and `storage` (`daemon.rs:384-409,661`).
  5. **Short write transactions.** Chunk `apply_flags` and `remove_copies` to ≤500 UIDs per transaction, so a user's star never waits behind a 10k-flag refresh. Mailsync chunks by 100–500 (`MailProcessor.cpp:618`, `SyncWorker.cpp:1340`, `MailStore.cpp:408`).
- **Pitfalls:**
  - **Revision source:** `total_changes()` and `data_version` are per connection. The revision must come from the writer.
  - **Checkpoint starvation:** a reader that holds a read transaction while sync writes continuously keeps the WAL from resetting. Keep read transactions per request, never long-lived. `journal_size_limit` trims the WAL after the next successful checkpoint.
  - **Read-only opens:** they need the `-wal`/`-shm` files to exist. Open readers only after the writer has opened and set WAL.
  - **Tests:** tests use temp-file databases (`store.rs:1458`), which work. `:memory:` databases cannot be shared across connections.
  - **Lock order:** sync holds the per-account Tokio lock and then takes the writer. Keep that order everywhere to avoid deadlocks.
- **Done when:**
  - A new test, `reads_proceed_while_writer_is_busy`, holds the writer inside a transaction on another thread for 2 s while a probe thread outside Tokio (as in `vault_responsiveness.rs`) gets `/v1/threads` in under 200 ms.
  - A test `revision_changes_only_after_commit` asserts that a reader never sees the new revision with old rows.
  - The benchmark records list-open p95 under concurrent import load.
- **Effort:** M. No dependencies; do it before or with the schema migration. It enables "Push store changes to the UI".

<a id="add-a-benchmark-fixture-eg-100k-messages-5-accounts-and-track-cold-start-list-op"></a>

### Add a benchmark fixture (e.g. 100k messages, 5 accounts) and track cold start, list-open and search latency in CI

*Roadmap: P0 · Performance foundations*

- **Why:** Nothing guards large-mailbox performance today. The only measurement was a one-off 10,000-message probe whose binary lived in `/tmp` (`docs/reviews/2026-10-11/fixes/evidence/performance.md`, "188 ms to 9 ms", 107 MiB → 10.8 MiB peak RSS).
- **Competitors:** Mailspring has no performance CI. Its 1.26 investment was functional sync tests against Dovecot, Cyrus and a fake IMAP server. It logs slow queries at runtime (`database-store.ts:307-321`). Its database numbers (258k messages / 1.15 GB, `MailStore.cpp:307`; ~2 min VACUUM for 3 GB, changelog 1.1.3) are useful targets for fixture size.
- **Alice today:**
  - `Store::add_fixture` is a small demo dataset (`store.rs:816`).
  - `scripts/mail-fixture.sh` and `scripts/test-mail.py` drive a Docker Dovecot/Postfix server.
  - CI runs tests, smoke, reader and TUI on three OSes (`.github/workflows/ci.yml:20-74`).
  - No bench target exists.
- **Plan:**
  1. **Bench target.** Add `crates/alice-core/benches/store.rs` with `harness = false` in `crates/alice-core/Cargo.toml` (`[[bench]] name = "store"`). It needs no criterion or divan: macro timings need `Instant` and medians, not statistics. Only the public API is used: `Store::open`, `save_account`, `import_copies`, `threads`, `list`, `conversation`, `apply_flags`, plus `daemon::router` with `tower::ServiceExt::oneshot`, the way the daemon tests call handlers.
  2. **Generator.** Deterministic (splitmix64, no `rand`). Shape:
     - 5 accounts × N/5 messages;
     - thread sizes geometric with mean 3;
     - 2,000 senders;
     - folders 30% inbox, 50% archive, 10% sent, 5% trash, 5% spam;
     - 85% read, 2% starred;
     - 2 KB text bodies from a 5,000-word vocabulary, and 20% with 10 KB HTML;
     - 1% with attachment metadata.

     Insert through `import_copies` in batches of 50, so `remote_messages`, FTS and triggers are realistic. Report import throughput (msgs/s) as the first metric.
  3. **Metrics.** Median of 5 after one warm-up, plus max:
     - cold `Store::open` of an existing file;
     - first `/v1/state`;
     - `/v1/threads` unified inbox page 0, per-account page 0, unread-only and page 20;
     - thread open for a 20-message thread;
     - search for a sender word, a subject word with a typo, a body word (FTS), and `is:unread after:30d`;
     - `apply_flags` on 1,000 UIDs;
     - import of one batch of 50;
     - peak RSS (`VmHWM` from `/proc/self/status` on Linux).

     Output JSON on stdout and a Markdown table for `$GITHUB_STEP_SUMMARY`.
  4. **Budgets.** Add `crates/alice-core/benches/budget.json` with absolute budgets per metric at about 3× the first measured value on `ubuntu-24.04`; CI runners are noisy, and this catches order-of-magnitude regressions. The bench exits non-zero when a metric is over budget. Upload the JSON as an artifact for trends. Add `benchmark-action/github-action-benchmark` only if trend charts are wanted later.
  5. **CI.** A new `bench` job on `ubuntu-24.04`, after `test`:
     - on PRs, `cargo bench -p alice-core --bench store -- --messages 20000`;
     - on `main` and nightly (`schedule: cron`), `--messages 100000`.

     Cache nothing; generation should take under 3 min.
- **Pitfalls:**
  - **Cold timings:** "cold start" in CI is warm OS cache. Label it honestly, or drop caches only on self-hosted runners.
  - **Bodies:** keep them synthetic. Never commit or download real mail.
  - **Mixed workloads:** measure list latency under concurrent import too. Single-threaded timings hide lock contention.
  - **Build profile:** the release profile uses `lto = "thin"` and `codegen-units = 1` (`Cargo.toml:48-51`), and benches inherit it. Expect a longer compile, and rely on `Swatinem/rust-cache`.
- **Done when:**
  - `cargo bench -p alice-core --bench store -- --messages 100000` prints all metrics.
  - The CI job fails when a budget is exceeded. Verify by temporarily adding a sleep in `threads()` on a branch.
  - `docs/` records the first 100k baseline.
- **Effort:** M. No dependencies. Build it first.

---

## P2 → Diagnostics and support

<a id="structured-logging-to-rotating-files"></a>

### Structured logging to rotating files

*Roadmap: P2 · Diagnostics and support*

- **Why:** When sync fails, the user and the maintainers see one sentence (`account.sync_error`). There is no history, timing or context to diagnose stalls, provider quirks or crashes.
- **Competitors:**
  - **Mailsync's logs:** one spdlog rotating file per account, `mailsync-<accountId>.log`, 5 MB × 3 files (`repos/mailsync/MailSync/main.cpp:999-1038`, `rotating_file_sink_mt(logPath, 1048576 * 5, 3)`). Thread names are in the format, and critical errors are also written to stderr.
  - **Mailspring client:** it logs DB queries over 100 ms (`database-store.ts:307-321`). "Verbose logging" (`VERBOSE_UNTIL_KEY`) records all provider traffic for 30 minutes and needs a restart (`repos/mailspring/app/src/flux/mailsync-bridge.ts:150-174`).
  - **Others:** Thunderbird has `MOZ_LOG=IMAP:5` environment logging. Apple Mail has a Connection Doctor with a "Show Detail" log.
- **Spec:**
  - `tracing-appender` rolling `Builder` (time-based only: MINUTELY, HOURLY, DAILY, WEEKLY, NEVER; `max_log_files`): https://docs.rs/tracing-appender/latest/tracing_appender/rolling/struct.Builder.html
- **Alice today:**
  - There are no logs: the daemon is spawned with `stdin`, `stdout` and `stderr` set to `Stdio::null()` (`crates/alice-core/src/client.rs:127-132`).
  - `daemon::run` prints one `println!` (`daemon.rs:1106`); the desktop prints `eprintln!` on a daemon error (`apps/desktop/src-tauri/src/main.rs`, `fn main`).
  - `tracing` 0.1.44 is already in `Cargo.lock` transitively, but `tracing-subscriber` and `tracing-appender` are not.
  - Both entry points, CLI `daemon run` (`crates/alice/src/main.rs:382`) and desktop `--daemon`, call `alice_core::daemon::run()` (`daemon.rs:1073`). Initialization therefore belongs in one place.
- **Plan:**
  1. **Dependencies.** Add to `crates/alice-core/Cargo.toml`: `tracing = "0.1"`, `tracing-subscriber = { version = "0.3", default-features = false, features = ["fmt", "env-filter", "std", "ansi"] }` and `tracing-appender = "0.2"`.
  2. **Initialization.** Add `pub fn init_logging(name: &str) -> WorkerGuard` in a new `crates/alice-core/src/logging.rs`. It uses `RollingFileAppender::builder().rotation(Rotation::DAILY).filename_prefix(name).filename_suffix("log").max_log_files(7).build(data_dir()?.join("logs"))`, wrapped in `tracing_appender::non_blocking`.
     - The filter comes from `ALICE_LOG`, else `info,lettre=warn,imap=warn,hyper=warn,reqwest=warn`.
     - Wrap it in `tracing_subscriber::reload` so verbose mode can be switched without a restart; Mailspring needs one.
     - Call it at the top of `daemon::run`, holding the guard for the process lifetime. The desktop's non-daemon `main` calls `init_logging("desktop")`.
  3. **What to log.** Add a `sync` span with `account = %id` around `Run::run` (`sync.rs:264`), and a `mailbox` span in `refresh`. Events:
     - start and finish with `changed`, `remaining` and `ms`;
     - UIDVALIDITY resets (`sync.rs:360-365`);
     - errors with `{error:#}` at `warn` the first time a given error text appears, and at `debug` while it repeats;
     - non-2xx responses in the `authenticate` middleware (`daemon.rs:320-333`);
     - store calls slower than 100 ms, timed inside the new `read`/`write` helpers, as Mailspring does.
  4. **Verbose toggle.** Add `POST /v1/diagnostics/verbose {minutes: 30}`, which swaps the reload filter to `debug` until a deadline. Settings gets a toggle.
  5. **Privacy.** A short comment in `logging.rs` states the policy: ids, mailbox names, counts and UIDs only; never subjects, addresses or bodies.
- **Pitfalls:**
  - **No size cap:** `tracing-appender` rotates by time only. Size is bounded by staying at `info`. If size caps matter, a 25-line size-capped writer matching mailsync's 5 MB × 3 is the fallback.
  - **Credentials in debug logs:** lettre's `tracing` feature logs every SMTP write verbatim at debug, AUTH lines included (`lettre/src/transport/smtp/client/connection.rs`, `tracing::debug!("Wrote: {}")`). Keep `lettre=warn` unless the redacting capture from the setup-log item is in place.
  - **Leaks into error text:** Mailspring 1.21.1 fixed "secrets potentially appearing in the connection error detail modal".
  - **Windows:** the daemon is spawned with no console (`client.rs:38`). A file appender works there; stderr does not.
- **Done when:**
  - A sync run writes `logs/daemon.YYYY-MM-DD.log` with start and finish lines carrying `account=` and counts.
  - Files older than 7 days are pruned.
  - A unit test, `log_lines_never_contain_secrets`, runs a fixture sync with a fake token and asserts the token and the fixture subject never appear in the file.
  - The verbose toggle switches levels without a restart.
- **Effort:** S–M. No dependencies.

<a id="copy-last-n-sync-log-lines-from-account-settings"></a>

### Copy last N sync log lines from account settings

*Roadmap: P2 · Diagnostics and support*

- **Why:** Users can't attach useful details to a bug report. One click should copy what a maintainer needs.
- **Competitors:**
  - Mailspring 1.7.6 added "a new button in Preferences > Accounts [to] retrieve + copy the last ~50 lines of your sync logs".
  - It has since become **Error Details...**, shown next to **Reconnect** for auth-failed, error and offline states (`repos/mailspring/app/internal_packages/preferences/lib/tabs/preferences-account-details.tsx:266-310`).
  - `_onShowErrorDetails` (`:203-244`) builds a header with Mailspring version, platform, account state, provider, IMAP server and SMTP server.
  - It appends `tailClientLog(id)`, the last 3,000 bytes of `mailsync-<id>.log` (`app/src/flux/mailsync-bridge.ts:207-225`).
  - The result is written to a temp HTML file shown in a JavaScript-disabled window.
  - Separately, **Window > Open mailsync logs** (`app/src/window-event-handler.ts:102`) opens the config directory.
- **Alice today:** There are no logs. Settings shows "Needs attention" when `account.sync_error` is set (`apps/desktop/src/Settings.tsx:149`), with no detail action.
- **Plan:**
  1. **Endpoint.** Add `GET /v1/accounts/{id}/diagnostics?lines=50` in `daemon.rs`. It returns plain text:
     - a header: Alice version and build (`client::BUILD`), OS and arch, provider, IMAP host:port, SMTP host:port, CONDSTORE, `last_synced`, `sync_error`, and `sync_totals` (`sync_store.rs:612`);
     - then the last N lines containing `account=<id>`, read from the newest two log files by seeking to `len - 256 KB`.
  2. **Redaction pass** (`logging::redact`): drop lines containing `AUTHENTICATE`, `LOGIN `, `auth=Bearer` or `AUTH `; mask anything that looks like an email address other than the account's own.
  3. **UI.** Each account row in Settings (`Settings.tsx:149`) gets a **Copy diagnostics** button, prominent when `sync_error` is set and otherwise in the row. It calls `navigator.clipboard.writeText` and shows the toast "Copied 50 log lines". **Open logs folder** uses a new Tauri command `reveal_logs`, modelled on `downloads::reveal_download` but restricted to `data_dir()/logs`.
  4. **i18n.** Add the keys to `apps/desktop/src/locales/en-US/settings.json` and `de/settings.json`.
- **Pitfalls:**
  - **Tail reads:** don't read whole log files. Seek and cut at the first newline, as `tailClientLog` does.
  - **Day rollover:** right after midnight, the useful lines are in yesterday's file.
  - **What the user sees:** show the copied text, or a short preview, so they know what they share.
- **Done when:**
  - Test `account_diagnostics_return_only_that_accounts_redacted_lines`: given log lines from two accounts plus a planted `auth=Bearer xyz`, the endpoint returns ≤N lines, only for the requested account, newest last, with no token.
  - The button copies text in the desktop app.
- **Effort:** S. Depends on: "Structured logging to rotating files (`tracing`)".

<a id="verbose-imapsmtp-protocol-log-during-account-setup"></a>

### Verbose IMAP/SMTP protocol log during account setup

*Roadmap: P2 · Diagnostics and support*

- **Why:** Most setup failures (TLS versions, auth mechanisms, wrong port or security, server quirks) are obvious in a transcript and invisible in one error sentence.
- **Competitors:**
  - **Changelog:** 1.0.2 "view a 'Raw Log' of the IMAP and SMTP communication". 1.7.7 "mailcore and libetpan debug logs are included in the 'Detailed Log' output".
  - **Capture:** mailsync `--mode test` attaches an `AccumulatorLogger` to the IMAP and SMTP sessions. It skips `ConnectionLogTypeSentPrivate` ("AUTH commands and credential payloads", `repos/mailsync/MailSync/main.cpp:77-91,325,366`), adds libetpan error codes and locations (`:388-394`), and returns them as `"log"` in the JSON response (`:404,455,734`).
  - **Display:** onboarding's `FormErrorMessage` shows **View Log**, which writes the log to a temp file and opens it (`repos/mailspring/app/internal_packages/onboarding/lib/form-error-message.tsx:20-30`).
  - **Others:** Thunderbird uses `MOZ_LOG`; Apple Mail has Connection Doctor.
- **Spec:**
  - IMAP `LOGIN` and `AUTHENTICATE`: RFC 9051 §6.2.2–6.2.3 (and RFC 3501).
  - SMTP AUTH and the `334` continuation: RFC 4954 §4.
  - SASL PLAIN is base64 of the password: RFC 4616.
- **Alice today:**
  - Setup verifies IMAP only: `Remote::connect` plus `folders()` (`daemon.rs:792-801`). SMTP is first exercised at the first send (`providers.rs:213-236`), so a wrong SMTP host shows up only then.
  - The IMAP stream is `imap::Session<TlsStream<TcpStream>>` (`providers.rs:16`), built in `providers::session` (`:31-47`). `imap::Client::new` accepts any `Read + Write` stream, and the repo already uses an in-memory stream in tests (`remote.rs:590-660`).
  - Errors reach the UI as `{status, message}` only (`apps/desktop/src-tauri/src/main.rs`, `CommandError`), so an extra `log` field in the error body would be dropped by the bridge.
- **Plan:**
  1. **IMAP capture.** Add `struct Tap<S> { inner: S, log: Option<Arc<Mutex<Vec<u8>>>>, secret: bool }` in `providers.rs`, implementing `Read` (appends `S: …`) and `Write` (appends `C: …`). After a client line containing ` LOGIN ` or ` AUTHENTICATE `, client writes are replaced with `<redacted>` until the next tagged server response. Cap the log at 256 KB. Change the `Session` alias to `imap::Session<Tap<TlsStream<TcpStream>>>`; sync passes `log: None`.
  2. **SMTP capture.**
     - Add `pub fn verify_smtp(account, secret, log)` in `providers.rs` that runs `SmtpTransport::test_connection()`; it should connect, upgrade TLS and authenticate, which needs verifying against the lettre 0.11 source.
     - Run it inside `tracing::subscriber::with_default(capture, …)`, where `capture` is a `fmt` subscriber writing to the same buffer, filtered to `lettre=debug`.
     - Enable lettre's `tracing` feature.
     - Redact `Wrote: AUTH …` lines and every client line after a `<< 334` line.
  3. **Connect flow.** In `connect()` (`daemon.rs:786-801`), verify SMTP after IMAP. On failure, store `(Instant, transcript)` in a new `AppState.setup_log: Mutex<Option<…>>`. The transcript is prefixed with the resolved addresses tried (`providers.rs:36-42`) and the full error chain; TLS certificate errors happen before any IMAP bytes. Expose it at `GET /v1/connections/log` for 10 minutes, and clear it on success.
  4. **UI.** `AddAccount.tsx:51` shows **Show connection log** under `.form-error`. It opens a `<details><pre>` with a Copy button inline; no temp file.
- **Pitfalls:**
  - **AUTHENTICATE PLAIN** continuations are bare base64 lines after `+`. XOAUTH2 tokens are in the same place. Redact by protocol state, not by pattern.
  - **Error payloads:** some servers echo the username or a token in `NO` responses. Redact `user=`/`auth=Bearer` substrings too.
  - **Gmail:** the XOAUTH2 failure JSON (base64 in a `+` continuation) is useful; decode it, since it contains no secret.
  - **TLS:** native-tls versions differ by OS, so include `std::env::consts::OS`. Mailspring 1.7.7 found its TLS failures through exactly this log.
- **Done when:**
  - A test drives a scripted `Tap` over an in-memory stream (`LOGIN user pass` → `a1 NO`). The transcript contains `C: a1 LOGIN <redacted>` and `S: a1 NO`, and never `pass`.
  - A redaction unit test covers `Wrote: AUTH PLAIN AGFsaWNl…` and a `334` exchange.
  - Adding an account with a wrong SMTP port fails at setup, with a viewable log.
- **Effort:** M (S for IMAP only). Depends on: "Structured logging to rotating files (`tracing`)" for the shared redaction helper.

<a id="opt-in-crash-reporting"></a>

### Opt-in crash reporting

*Roadmap: P2 · Diagnostics and support*

- **Why:** Today a daemon panic is invisible.
  - A panic inside sync's `spawn_blocking` becomes an `account.sync_error` like "task … panicked".
  - A panic in the background loop (`daemon.rs:1104`, `tokio::spawn(run_background)`) silently stops all syncing until the next restart.
  - A process-level crash just makes the desktop restart the daemon.
- **Competitors:**
  - **Always on.** Mailspring runs `crashReporter.start({ submitURL: 'https://id.getmailspring.com/report-crash…', uploadToServer: true, autoSubmit: true })` (`repos/mailspring/app/src/error-logger.js:162-170`) and reports JS errors to Sentry with a hard-coded DSN. Device identity is a SHA hash of the MAC address (`error-logger-extensions/sentry-error-reporter.js:2,7,172-180`). There is no opt-out in the UI.
  - **User-facing text** says "Crash reports have been sent to the Mailspring team" (`preferences-account-details.tsx:284`).
  - **Symbols:** on Windows, mailsync PDBs ship so stack traces have names (changelog 1.7.7).
  - **Others:** Thunderbird uses Mozilla Crash Reporter, asking per crash with "Tell Mozilla about this crash". Apple Mail relies on the OS crash reporter.
- **Spec:**
  - `std::panic::set_hook`, `std::backtrace::Backtrace::force_capture`.
  - sentry-rust 0.49 features: default `backtrace, contexts, debug-images, logs, metrics, panic, transport, release-health`. `transport = reqwest + native-tls`, and its reqwest is ^0.13, the same as Alice's.
- **Alice today:** Nothing.
  - No panic hook.
  - `[profile.release] strip = true` (`Cargo.toml:50`), so backtraces from release builds have no function names.
- **Plan:**
  1. **Local crash reports, always on.** No network is involved. Add `logging::install_panic_hook(process: &str)`, called in `daemon::run` and in the desktop `main` (`apps/desktop/src-tauri/src/main.rs`, `fn main`).
     - It writes `data_dir()/crashes/<utc>-<process>.txt` containing version, build, OS/arch, thread name, panic message and location, and `Backtrace::force_capture()`.
     - It also emits `tracing::error!`.
     - Keep the newest 10 reports.
     - The hook does no allocation-heavy work and takes no store locks.
  2. **Release symbols.** Change to `strip = "debuginfo"` in `Cargo.toml`, which keeps symbol names so backtraces are readable. Measure the binary size delta in the bundle job.
  3. **Surfacing and sending.** `GET /v1/diagnostics/crashes` lists unseen reports and `POST …/{name}/seen` dismisses one. The desktop shows a banner: "Alice hit a problem and recovered." with **View report**, **Copy**, **Report on GitHub** and **Dismiss**. **Report on GitHub** opens a prefilled issue URL, truncated to about 6 KB, via `open_help_url`-style allowlisting. The user sees exactly what leaves the device.
  4. **Restart the sync loop.** Run the background loop in a supervisor (`loop { let r = tokio::spawn(run_background()).await; if r.is_err() { tracing::error!; sleep(5s) } }`) so a panic does not stop sync.
  5. **Optional, later: Sentry.** Only if a backend exists to triage.
     - Add `sentry = { version = "0.49", default-features = false, features = ["backtrace", "contexts", "panic", "reqwest", "rustls"] }`.
     - Initialize it only when `Preferences.crash_reports == "send"` (default `"off"`) and a DSN was compiled in (`option_env!("ALICE_SENTRY_DSN")`).
     - Set `send_default_pii: false` and `server_name: None`, with no breadcrumbs from logs and a `before_send` that drops `user`/`server_name` and masks `@` strings.
     - Add the setting to `api.ts` `Preferences` and Settings > Privacy.
- **Pitfalls:**
  - **Native crashes:** SIGSEGV in SQLite, WebKit or native-tls is not a Rust panic and needs out-of-process minidumps (e.g. Embark's `crash-handler` + `minidumper`). That is XL; rely on the OS reports (macOS `~/Library/Logs/DiagnosticReports`, Windows WER) until then.
  - **Panic messages can contain user data**, for example an `expect` on a parse error that includes the input. That is one more reason to show reports before sending.
  - **Tokio tasks:** task panics are caught by Tokio, but the hook still runs, which is good.
  - **Don't** add `panic = "abort"`: tasks would no longer survive panics.
- **Done when:**
  - In a debug-only test endpoint or test, a forced panic in a daemon task writes a crash file with a symbolized frame. The desktop shows the banner once, and dismissing it persists.
  - With `crash_reports` off, no network request is made; assert in a test that the Sentry client is not initialized.
  - Background sync resumes after an injected panic.
- **Effort:** M for local reports and UI; +M for opt-in Sentry. Depends on: "Structured logging to rotating files (`tracing`)".

<a id="reset-cache-for-one-account"></a>

### Reset cache for one account

*Roadmap: P2 · Diagnostics and support*

- **Why:** This is the standard fix for a corrupted or confused local state (stuck sync, phantom messages) without removing and re-adding the account, which loses its settings.
- **Competitors:**
  - **UI:** Mailspring Preferences > Accounts > Local Data > **Rebuild Cache...** (`preferences-account-details.tsx:405-408`) shows the dialog "Cleanup Started … An alert will appear when cleanup is complete."
  - **Process:** `resetCacheForAccount` (`repos/mailspring/app/src/flux/mailsync-bridge.ts:262-300`) kills the account's sync process. It runs `mailsync --mode reset`, executing `ACCOUNT_RESET_QUERIES` (`repos/mailsync/MailSync/constants.h:55-77`), then `VACUUM` (`MailStore.cpp:452-464`), then relaunches sync.
  - **Scope:** the reset deletes every per-account table, local drafts, tasks and metadata included. That works for Mailspring because drafts live on the server.
- **Alice today:**
  - The primitive already exists. When an account's server changes, `connect()` calls `forget_other_mailboxes(id, &[])` under `sync.pause` + `hold` (`daemon.rs:805-818`). This drops every server copy and mailbox state and deletes messages left without copies (`sync_store.rs:302-311,675-688,216-273`).
  - Drafts and Alice-delivered sent mail have no `remote_messages` rows, so they survive.
  - Messages cached before copies were tracked (no `remote_messages` row) would also survive. A reset should remove them.
- **Plan:**
  1. **Store method.** Add `Store::reset_account(id)` in `sync_store.rs`, one write transaction:
     - `forget` every mailbox;
     - `DELETE FROM messages WHERE account_id=? AND folder<>'drafts' AND id NOT IN (SELECT id FROM deliveries)`;
     - `DELETE FROM otp_deliveries WHERE account_id=?`;
     - keep the account, settings, contacts and drafts.
  2. **Endpoint.** `POST /v1/accounts/{id}/reset` runs `sync.pause(id)` (clears the in-memory `pending` plans, `sync.rs:129-136`), then `hold(id)`, `reset_account`, `contacts.invalidate()`, `PRAGMA incremental_vacuum`, then `sync_account(CatchUp)` and `wake()`. Mailspring's full VACUUM blocks too long.
  3. **UI.** The account row in Settings gets **Rebuild local cache…** with a confirm sentence: "Downloads this account's mail again. Drafts stay." While it runs, progress shows through the existing storage sync totals (`MailStorage.tsx`).
  4. **i18n** for en-US and de.
- **Pitfalls:**
  - **Local-only data:** don't delete drafts or `deliveries`. Alice does not save drafts to the server yet (P0 "Save drafts to the server Drafts folder").
  - **Pending sends:** a pending delivery (`claim_delivery`, `store.rs:399-412`) must block the reset, or wait for it.
  - **Gmail cost:** a 1-year window can mean tens of thousands of messages. Warn with the message count from `/v1/storage`.
- **Done when:**
  - Test `reset_keeps_drafts_and_deliveries_and_refetches`: after reset, the account has zero `remote_messages` and `mailbox_sync` rows, drafts and sent deliveries remain, and the next fixture sync re-imports.
  - The UI flow works with a confirmation.
- **Effort:** S. No dependencies.

<a id="automatic-repair-of-folder-and-unread-counts"></a>

### Automatic repair of folder and unread counts

*Roadmap: P2 · Diagnostics and support*

- **Why:** Badges and folder counts that disagree with the list erode trust.
- **Competitors:**
  - Mailspring 1.26 says "Folder and unread counts left wrong by previous versions are repaired automatically on upgrade."
  - The V11 migration (`repos/mailsync/MailSync/MailStore.cpp:370-430`) uses one SQL predicate to find threads whose `unread < 0`, whose per-label `_u` is a JSON boolean or out of range, or whose per-folder `_refs`/`_u` differ from a recount over their messages.
  - It recounts them in chunks of 500, one transaction per chunk, so an interrupted run resumes. Deltas are dropped.
  - The drift came from increment-style counters and double application by two workers.
- **Alice today:** Not applicable yet.
  - Counts are computed live from rows: `list()` uses `COUNT`/`SUM` (`store.rs:92-94`) and `threads()` counts in Rust (`store.rs:198-203`). There are no stored counters to drift.
  - One redundancy can disagree today: a message's `is_read`/`is_starred` versus its primary copy's flags in `remote_messages`. `apply_flags` updates both (`sync_store.rs:374-395`), but interrupted or older paths may not.
- **Note:**
  - This item becomes real only once `thread_view` (phase 2 of the hot-columns item) or a `ThreadCounts`-style table exists.
  - Design those tables to recompute touched groups instead of incrementing, and the repair reduces to a consistency check.
  - Consider re-scoping this item to "consistency check on open", and keep it P2.
- **Plan:**
  1. **Check and recount.** In `maintenance()` (next item) and once after each schema migration, run one mismatch query, `thread_view` versus `GROUP BY` over `messages` for the same `(account_id, thread, folder)`. Recount mismatched groups in chunks of 500 per transaction and log the count.
  2. **Flag reconciliation.** Fix messages whose flags differ from their primary copy's `remote_messages` flags: `UPDATE messages SET is_read=r.is_read … FROM remote_messages r WHERE …`; UPDATE-FROM needs SQLite 3.33+, which the bundled build has.
- **Done when:** Test `corrupted_thread_view_rows_are_recounted_on_open`: tamper with `thread_view.unread`, reopen, and check the counts match.
- **Effort:** S once the schema exists. Depends on: "Move hot fields … into real indexed columns" (phase 2).

<a id="periodic-sqlite-maintenance-vacuum-incremental-vacuum-analyze"></a>

### Periodic SQLite maintenance (VACUUM / incremental vacuum, ANALYZE)

*Roadmap: P2 · Diagnostics and support*

- **Why:** Deleting and re-downloading mail fragments the file and leaves free pages. Stale or missing statistics give bad query plans as mailboxes grow. The database also never shrinks after a sync-window reduction.
- **Competitors:**
  - Mailspring 1.1.3 added "VACUUM's the SQLite database every two weeks … ~2 minutes for a 3GB database … performed at launch with a new progress screen".
  - Current mailsync uses a 30-day interval (`VACUUM_INTERVAL`, `MailStore.cpp:117-118,276-296`). It sets the timer first, so a failed VACUUM is not retried at each launch, and ignores failures such as disk full.
  - It also VACUUMs after every account reset (`:463`).
  - Neither the client nor mailsync runs `ANALYZE` or `PRAGMA optimize`; grep finds none.
- **Spec:**
  - `PRAGMA optimize` (https://www.sqlite.org/pragma.html#pragma_optimize): long-lived connections "should run `PRAGMA optimize=0x10002;` when the connection is first opened, and then also run `PRAGMA optimize;` periodically, perhaps once per day or once per hour", and after schema changes.
  - `auto_vacuum=INCREMENTAL` with `incremental_vacuum(N)`. Changing the mode needs one VACUUM, which is allowed in WAL mode.
  - `wal_checkpoint(TRUNCATE)`.
  - VACUUM needs up to 2× the file size free and "may change the ROWIDs" of tables without an INTEGER PRIMARY KEY (https://www.sqlite.org/lang_vacuum.html).
  - FTS5 `'merge'` and `'optimize'` commands: https://www.sqlite.org/fts5.html#the_merge_command
- **Alice today:**
  - No maintenance at all.
  - The storage UI shows the database size (`apps/desktop/src/MailStorage.tsx:50`; `page_count*page_size`, `sync_store.rs:622-623`), which excludes the WAL and the freelist.
  - Running VACUUM today could renumber `messages` rowids (no INTEGER PRIMARY KEY). The next open would then detect the mismatch and rebuild `message_index` and all of FTS (`store.rs:1147-1150,1180-1193`).
- **Plan:**
  1. **Order of work.** Land the hot-columns migration first; it adds `n INTEGER PRIMARY KEY` and switches to `auto_vacuum=INCREMENTAL` with its one-time VACUUM.
  2. **Daily routine.** Add `Store::maintenance()`. It runs when the last run (stored in `settings` id `maintenance`) is over 24 h old, triggered after a background sync pass in `run_background` (`sync.rs:222-241`), never at launch. Steps:
     - `PRAGMA optimize`;
     - `INSERT INTO message_bodies(message_bodies, rank) VALUES('merge', 500)`;
     - if `freelist_count > page_count/10`, run `PRAGMA incremental_vacuum(2000)` in a loop, one writer acquisition per chunk, so user writes interleave;
     - `PRAGMA wal_checkpoint(TRUNCATE)`; a busy result is fine, and the next run retries.
  3. **At open.** Run `PRAGMA optimize=0x10002` on the writer, inside the readers/writer work.
  4. **Manual compaction.** **Compact database** in Settings > Mail storage (`MailStorage.tsx`) checks for 2× free disk, pauses sync for all accounts, then runs a full `VACUUM` followed by `wal_checkpoint(TRUNCATE)`, with a progress state. Never run a full VACUUM automatically at launch: Mailspring's ~2 min launch block for 3 GB is the anti-pattern.
  5. **Accurate size.** Report the database size as file + WAL + freelist so users see what compaction would reclaim.
- **Pitfalls:**
  - **Lock time:** `incremental_vacuum` and the FTS merge hold the write lock, so chunk them.
  - **Checkpoint TRUNCATE** cannot finish while a reader holds an old snapshot. Treat busy as "try tomorrow".
  - **Free space:** VACUUM on a nearly full disk fails. Check `statvfs`, or `fs2`-style free space; Mailspring catches the error and moves on.
  - **No-op without the migration:** incremental vacuum does nothing unless `auto_vacuum` is INCREMENTAL, which requires the migration's VACUUM.
- **Done when:**
  - Test `maintenance_reclaims_free_pages_and_keeps_search`: insert 5k messages, delete 4k, run maintenance; `freelist_count` drops, the file shrinks, and search still finds the remaining messages.
  - Maintenance doesn't run twice in 24 h.
  - The Compact button works with sync paused.
- **Effort:** S–M. Depends on: "Move hot fields … into real indexed columns".

<a id="database-corruption-recovery"></a>

### Database corruption recovery

*Roadmap: P2 · Diagnostics and support · new*

- **Why:** A malformed `mail.sqlite3` currently fails `Store::open`, and the daemon cannot start. That database also holds accounts, settings, DAV sources and local drafts, not just cached mail.
- **Competitors:** Mailspring's client detects `database disk image is malformed` (`database-store.ts:325`), shows "We encountered a problem with your local email database. We will now attempt to rebuild it.", deletes `edgehill.db`, `-wal` and `-shm`, and relaunches (`app/src/browser/application.ts:339-365`). It loses all local-only data.
- **Plan:**
  1. On `SQLITE_CORRUPT`/`SQLITE_NOTADB` at open, or during a read, rename the file and its `-wal`/`-shm` to `mail.sqlite3.corrupt-<ts>`.
  2. Create a fresh database, `ATTACH` the corrupt file, and copy `accounts`, `settings`, `dav_sources`, `contacts`, `deliveries` and drafts with `INSERT … SELECT`, ignoring tables that fail to read.
  3. Resync from servers.
  4. Tell the user once, and offer the corrupt file location for bug reports.
- **Done when:** Test: truncate a database file mid-page, and open recovers accounts and drafts and logs the event.
- **Effort:** M. Depends on: "Structured logging to rotating files (`tracing`)".

---

## P2 → Performance

<a id="virtualized-message-list-with-infinite-scroll"></a>

### Virtualized message list with infinite scroll

*Roadmap: P2 · Performance*

- **Why:** Today the list shows 50 conversations per page with Previous/Next buttons. Scrolling back through months of mail means clicking dozens of pages. Rendering thousands of rows instead would freeze the WebView.
- **Competitors:**
  - **Mailspring:**
    - `ListTabular` (`repos/mailspring/app/src/components/list-tabular.tsx`) renders only the viewport rows ±2 with a fixed `itemHeight` (`:306-322`).
    - It tells the data source its `setRetainedRange` (`:260,337`); `ObservableListDataSource` and `QuerySubscription` fetch only that range by `LIMIT/OFFSET` (`flux/models/query-subscription.ts:230`).
    - The scrollbar shows date "ticks" (`components/scrollbar-ticks.tsx`). 1.21.1 fixed the indicator not sliding through time while dragging (#2716).
  - **Other clients:** Superhuman, Spark and Apple Mail all use continuous virtualized lists. Thunderbird's tree view is virtualized natively.
- **Spec:**
  - `@tanstack/react-virtual` 3.14: headless; `useVirtualizer({count, getScrollElement, estimateSize, overscan, measureElement})`, `scrollToIndex`. 27.6 KB minified / 8.1 KB gzip (bundlephobia).
  - `react-window` 2.3: components; fixed or dynamic row height; 15.8 KB / 5.4 KB gzip.
  - `react-virtuoso`: 19.3 KB gzip.
  - The current app JS is 274 KB / 85 KB gzip (`docs/reviews/2026-10-11/fixes/evidence/b-technical-report.md:156`).
- **Alice today:**
  - `refresh()` requests `limit=50&offset=page*50` (`apps/desktop/src/App.tsx:106`).
  - The footer pager shows "1–50 of N" (`App.tsx:385`).
  - Rows are `motion.div` inside `AnimatePresence`, with staggered entry and an exit height animation (`App.tsx:376`).
  - Keyboard navigation and focus work on `.email-list` (`App.tsx:302,375`).
  - Row CSS is fixed padding with two densities (`styles.css:192`, `html[data-density]`).
- **Plan:**
  1. **Library.** Add `@tanstack/react-virtual`. It is headless, so the existing `.email-list` element, context menus, `motion` rows and focus handling stay. Make row height fixed per density: CSS line clamps on subject and preview, with `estimateSize` from the density. That allows plain absolute positioning.
  2. **Data.** Replace `page` state with a window cache: `Map<chunkIndex, Thread[]>` of 100-row chunks.
     - Fetch the chunks that intersect the visible range ±1 chunk, at most 2 in flight, with `offset=chunk*100&limit=100`.
     - Get `count` from `ThreadPage.total`.
     - On a new revision (push item), refetch only the visible chunks and drop the others.
  3. **Scroll anchoring.** When new mail shifts offsets, find the first visible thread id in the fresh data and adjust `scrollTop` by the shift times the row height, unless the list is at the top.
  4. **Behaviour to keep:**
     - j/k and arrow navigation call `virtualizer.scrollToIndex`;
     - `aria-setsize`/`aria-posinset` on rows, since the DOM no longer holds the whole list;
     - the staggered entry animation applies only to the first screen;
     - exit animations apply only to rows being removed by the user's own action (`leaving` set).
  5. **Footer.** Remove the pager, and keep a count ("2,341 conversations"). Placeholder rows (skeleton) for chunks still loading.
- **Pitfalls:**
  - **Find in page:** browser Ctrl/Cmd+F can't find off-screen rows. That is acceptable, because search is the feature.
  - **Height animations** conflict with absolute positioning; use opacity/translate exits.
  - **WebKitGTK** (Linux) scroll performance with large translated containers: test on Ubuntu.
  - **Offset paging is O(offset) in SQL.** It is fine at 100k with the covering index from the P0 items; without them every scroll step costs a full scan.
  - **Selection:** keep it by id, not index.
- **Done when:**
  - With the 100k fixture, scrolling from top to bottom keeps p95 frame time under 16 ms in the Tauri WebView (Playwright trace on the web build as a proxy).
  - There are never more than about 60 rows in the DOM.
  - Keyboard navigation reaches row 5,000.
  - A new message arriving while scrolled down doesn't move the visible rows.
- **Effort:** M. Depends on: "Page the thread list in SQL…", and preferably "Push store changes to the UI".

<a id="push-store-changes-to-the-ui-sse-websocket-instead-of-2s-polling"></a>

### Push store changes to the UI (SSE / websocket) instead of 2s polling

*Roadmap: P2 · Performance*

- **Why:**
  - Polling costs a full `/v1/state` every 2 s per client, even when idle; today that includes a whole-mailbox `threads()` scan.
  - It also adds up to 2 s of latency to changes made elsewhere: sync, CLI, MCP, the other window.
- **Competitors:**
  - **Mailspring's DeltaStream:** mailsync emits JSON deltas on stdout (`repos/mailsync/MailSync/DeltaStream.cpp`).
    - Deltas buffer per model class, and repeated saves of one object collapse into one (`upsertModelJSON`, `:79-99`).
    - The buffer flushes within a max delay: 500 ms for sync workers (`SyncWorker.cpp:79`), 5 ms for the foreground task loop (`main.cpp:744`).
    - Deltas are emitted only after `COMMIT` and are discarded on rollback (`MailStore.cpp:611-665`, `unsafeEraseTransactionDeltas` at `:640`).
  - **Mailspring client:** turns deltas into `DatabaseChangeRecord`s, so `QuerySubscription`s patch results or re-query their range.
  - **Others:** IMAP-backed clients poll their own local store less often. Electron apps typically use IPC events.
- **Spec:**
  - Server-sent events: https://html.spec.whatwg.org/multipage/server-sent-events.html
  - axum `response::sse::{Sse, Event, KeepAlive}`
  - Tauri v2 events `AppHandle::emit` / `listen`
- **Alice today:**
  - **Desktop:** polls `/v1/state` every 2 s and skips the list request when `revision` is unchanged (`apps/desktop/src/App.tsx:102-140`).
  - **TUI:** refreshes every 2 s (`crates/alice/src/tui.rs:247`).
  - **Tauri side:** polls `/v1/preferences` and `/v1/arrivals` every 5 s (`apps/desktop/src-tauri/src/desktop.rs:24,78-122`).
  - **Transport:** all desktop calls go through the Tauri command `daemon_request` (`api.ts:66-71`, `src-tauri/src/main.rs`), using a reqwest client with a 90 s timeout (`client.rs:71-76`). Web dev mode goes through the Vite `/api` proxy (`vite.config.ts`).
- **Plan:** Use long-polling first. It needs no new dependencies, no Tauri event bridge and no SSE parser, and works the same for the desktop, the web dev build, the TUI and the CLI.
  1. **Endpoint.** Add `GET /v1/changes?after=<revision>` in `daemon.rs`:
     - if `after` differs from `store.revision()`, return at once;
     - otherwise `tokio::time::timeout(25s, rx.changed())`, using the `watch` receiver from the read-pool item;
     - also wake on a `SyncEngine` notify when `active` flips, because sync start writes nothing yet the UI shows "syncing" (`sync.rs:164-191`);
     - return `{revision, syncing}`.
  2. **Desktop loop.** Replace `setInterval(update, 2000)` with a loop: `await get('/v1/changes?after=' + rev)`, then `refresh()`, with a minimum 150 ms gap that coalesces sync batches like DeltaStream's 500 ms. On error, back off from 1 s up to 30 s. Pause while `document.hidden`; on `visibilitychange`, refresh and resume. Keep a 60 s safety refresh.
  3. **TUI.** Run the same loop on a background task in `tui.rs`, posting to the UI channel.
  4. **Tauri watch loop.** `desktop.rs` waits on `/v1/changes` instead of a 5 s tick for arrivals. OTP delivery keeps its 5 s tick, or moves to the same wait.
  5. **Docs.** Update `docs/api.md` and the README "UI and TUI poll the shared store every two seconds" sentence (`README.md:233`).
  6. **Later, only if per-entity deltas are needed:**
     - add `GET /v1/events` as SSE (`axum::response::sse` plus `tokio-stream`'s `WatchStream`);
     - the Tauri side reads it with reqwest `Response::chunk()`, which needs no new feature, and calls `app.emit("store-changed", …)`;
     - the WebView `listen`s, as with the existing `listen('open-message')` (`api.ts:168`).
- **Pitfalls:**
  - **Revision timing:** the revision must change only after commit (read-pool item). Otherwise a client can miss a change forever.
  - **Connection count:** each client holds one connection for up to 25 s. reqwest's pool and axum handle that fine. Keep it under the 90 s client timeout.
  - **Refresh rate:** during sync, revisions change every batch. The client gap prevents a refresh storm; do not refresh faster than the previous refresh completes.
  - **Daemon restarts:** the revision epoch changes, so the first long-poll returns at once (`store.rs:11-12`). That is correct.
- **Done when:**
  - A star set via the CLI shows in the desktop within 300 ms.
  - An idle desktop makes at most about 3 requests per minute (count requests in a daemon log at debug).
  - Router test `changes_waits_until_a_write_and_times_out`, with `tokio::time::pause`, checks the endpoint returns within 50 ms of a commit and after 25 s with no change.
- **Effort:** S–M. Depends on: "Separate read connections (pool)…" for the post-commit revision signal.

<a id="chunk-sync-writes-and-skip-no-op-updates-batching-mostly-done"></a>

### Chunk sync writes and skip no-op updates (batching mostly done)

*Roadmap: P2 · Performance*

- **Why:** Write cost and lock hold time directly limit how long the UI waits during sync. Mailspring 1.21.1 says "Improved sync engine performance by reducing unnecessary SQLite writes and moving parsing outside transactions."
- **Competitors:**
  - Mailsync wraps work in `MailStoreTransaction` (`BEGIN IMMEDIATE`, `MailStoreTransaction.cpp`) and chunks bulk work: deletions by 200 (`SyncWorker.cpp:1340`), body/metadata updates by 100 (`MailProcessor.cpp:618`), recounts by 500.
  - `unsafeEraseTransactionDeltas` skips client notifications for internal-only writes (`MailStore.cpp:640-642`).
- **Alice today:** Mostly done already.
  - Parsing happens before the lock: `providers::message_from` builds `Incoming` in `import()` (`sync.rs:535-561`) before `self.locked(|| import_copies(…))` (`sync.rs:562-571`).
  - Each 50-message batch is one transaction (`BATCH`, `sync.rs:35`; `sync_store.rs:446-583`).
  - Remaining issues:
    - `apply_flags` and `remove_copies` are unbounded per transaction. A non-CONDSTORE refresh sends every known UID (`sync.rs:391-393`).
    - `apply_flags` updates `remote_messages` even when nothing changed (`sync_store.rs:374`).
    - `make_primary` issues up to three full-row JSON rewrites (`sync_store.rs:199-206`).
    - `serde_json::to_string(&message)` runs inside the transaction (`sync_store.rs:560`).
    - `save_mailbox_state` and `record_sync` write on every pass even when unchanged (`sync.rs:475`, `sync_store.rs:641-672`). Each write bumps the revision and triggers a client refresh.
- **Note:** Mostly done. Re-scope to "chunk and skip no-op writes".
- **Plan:**
  1. Chunk `apply_flags` and `remove_copies` callers to 500 UIDs per `locked(...)` call in `sync.rs`.
  2. Add `WHERE is_read IS NOT ?5 OR is_starred IS NOT ?6` to the copy UPDATE, and skip `save_mailbox_state` when the state equals the stored one. `MailboxState` is already `PartialEq`, `sync_store.rs:72-73`.
  3. Serialize message JSON outside the lock. `Incoming` carries pre-serialized cold JSON after the schema change.
  4. After the hot-column migration, collapse `make_primary` to one column UPDATE.
  5. Let `record_sync` write `last_synced` without bumping the client-visible revision. Alternatively, have the UI ignore `last_synced`-only changes. Decide with the push item.
- **Pitfalls:** Chunking must keep the invariants of `settle` (`sync_store.rs:216-273`); each chunk is self-contained, because `settle` runs per chunk on the affected ids.
- **Done when:**
  - The benchmark shows `apply_flags(10,000)` never holds the writer for more than 100 ms continuously, measured by the slow-write log.
  - An idle sync pass with no server changes produces zero revision bumps; test with the fixture IMAP mock.
- **Effort:** S. Depends on: "Separate read connections (pool)…" for measurement; step 4 depends on the hot-columns item.

<a id="cap-and-measure-daemon-memory-under-large-mailboxes-add-a-long-running-soak-test"></a>

### Cap and measure daemon memory under large mailboxes; add a long-running soak test

*Roadmap: P2 · Performance*

- **Why:** The daemon runs all day in the background. Growth or spikes show up as "Alice uses 1 GB" reports and OS memory pressure.
- **Competitors:**
  - Mailspring's memory fixes are client-side: 1.0.10 "major memory leak on Windows", 1.1.3 composer undo history and task-queue purging.
  - Mailsync uses `cache_size = 10000` pages (`MailStore.cpp:112`); the client uses 20,000 pages of 8 KB (`database-store.ts:65`), about 160 MB of potential page cache.
  - No soak test exists in either repo.
- **Alice today:**
  - **Per-call allocations:**
    - `threads()` and `conversation()` materialize a `ThreadRow` with owned strings for every message of the account on every call (`store.rs:1307-1324`);
    - `search()` folds sender, recipients and subject of every row in scope on every query (`store.rs:1218-1239`).
  - **IMAP batch size:** `fill_texts` fetches text parts for a whole 50-message batch in one response (`remote.rs:248-284`). Batches are bounded by count, not bytes. `whole()` caps a single message at 30 MB (`remote.rs:21,285-303`).
  - **SQLite cache:** default per connection, about 2 MB.
  - **Measured:** 10.8 MiB peak RSS for store queries at 10k (`docs/reviews/2026-10-11/fixes/evidence/performance.md`). Nothing has been measured under sync or over time.
- **Plan:**
  1. **Measure.** Add peak and steady RSS to the benchmark (`VmHWM`/`VmRSS` on Linux). Add a `--soak <minutes>` mode to the same bench binary. Each loop imports 50, flips 200 flags, and runs list, thread open and search across views. It samples RSS every 10 s and asserts that after a 5 min warm-up growth stays under 10% and the peak stays under a budget (e.g. 150 MB at 100k). Run it 30 min nightly in CI.
  2. **Real-IMAP soak, weekly or manual.** Use the Dovecot fixture (`scripts/mail-fixture.sh`). A small `imaplib` generator APPENDs 100k synthetic messages. Run the daemon for 1 h with sync window `all` and sample `ps -o rss` every 30 s.
  3. **Caps.**
     - SQL paging (P0) removes the O(n) per-request allocations.
     - Explicit `cache_size`: writer 16 MB, readers 4 MB × 4.
     - Bound fetch batches by bytes as well as count: stop adding UIDs when the sum of `Header.size` (`remote.rs:47`) exceeds 25 MB.
     - Fold search fields at write time into columns. Coordinate with the search owner.
  4. **Allocator, only if the soak shows it.** If Linux RSS creeps from glibc arena fragmentation, set `M_ARENA_MAX` via `mallopt` or switch to `mimalloc`.
- **Pitfalls:**
  - **RSS includes SQLite's page cache and mmap.** Compare like with like.
  - **macOS lacks `/proc`.** Run the soak on Linux CI only.
  - **Batch sizing:** bounding by bytes changes `FIRST_SYNC`/`BATCH` behaviour for huge messages. Keep at least one message per batch.
- **Done when:**
  - The nightly soak passes with recorded numbers.
  - The 100k benchmark peak RSS is within budget.
  - Test `fetch_batches_respect_byte_budget` uses headers with large `size`.
- **Effort:** M. Depends on: "Add a benchmark fixture…", and "Page the thread list in SQL…" for the main cap.

<a id="composer-stays-responsive-with-very-long-quoted-text-and-many-recipients"></a>

### Composer stays responsive with very long quoted text and many recipients

*Roadmap: P2 · Performance*

- **Why:** Typing lag in a reply to a long thread, or in a message to a large list, is very noticeable and makes the app feel broken.
- **Competitors:**
  - Mailspring 1.1.3: "Creating a draft is now ~16x faster in scenarios where lots of quoted text is present", and "The undo history of the composer is cleared when you navigate away from a draft".
  - 1.7.4: "The performance of the composer has been greatly improved when there are a large number of recipients" (its token field rendered a component and a contact lookup per recipient).
  - Gmail and Mailspring keep quoted text collapsed behind "…".
- **Alice today:**
  - Replies don't quote the original yet; the body starts empty or with an AI draft (`apps/desktop/src/App.tsx:207-213`). The "long quoted text" case arrives only with paste or the P0 Forward and Reply-all items.
  - Each keystroke already scales with body size:
    - `update()` sets React state;
    - it synchronously `JSON.stringify`s the whole draft into `localStorage` (`Compose.tsx:54-61`, `drafts.ts:5-7`);
    - a 400 ms debounce sends the whole body to the daemon.
  - On the daemon, `save_draft` rewrites the row (`store.rs:342-398`), and the body trigger deletes and re-inserts the draft in FTS on every autosave (`store.rs:1171-1174`).
  - Recipients are one comma-separated string (`Compose.tsx:16,38`).
- **Note:** Low urgency until quoting lands. Do the cheap parts now (steps 1 and 3) and fold step 2 into the Forward and Reply-all items.
- **Plan:**
  1. **Backup throttling.** Move `backupDraft` into the same 400 ms timer, and flush on blur, close and `beforeunload`.
  2. **Quoted text.** Keep it out of the editable textarea. Hold it in `Compose` as a separate `quoted` field, shown collapsed as "…" and expandable, appended at send in `providers::mail` (or wherever MIME is built). That is the Gmail/Mailspring model, and it also makes the P0 `References`/reply-all work cleaner.
  3. **FTS.** Don't index drafts in FTS on autosave: skip `folder='drafts'` in the body trigger, or index at send.
  4. **Recipients.** When recipient chips arrive (P0 "Cc and Bcc fields"), memoize chips, keep at most one suggestion request in flight, and do no per-chip lookups.
- **Pitfalls:**
  - **Throttled backup:** it can lose up to 400 ms of typing on a crash; flush on `visibilitychange` too.
  - **Signature:** collapsed quotes must still be sent, and must not break the signature-placement logic (`App.tsx:212`).
- **Done when:**
  - A jsdom or Playwright test types 200 characters into a draft with a 1 MB body. The handler's p95 is under 8 ms, and `localStorage.setItem` runs at most 3 times.
  - A 500-recipient draft keeps keystroke handling under 8 ms.
- **Effort:** S (steps 1, 3), M (step 2). Depends on: "Forward, with original attachments carried over", "Reply-all…", "Cc and Bcc fields".
