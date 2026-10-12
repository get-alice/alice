# Sync, server-side state, push and offline

Research behind the matching items in [`todo.md`](../../todo.md). Each entry covers why it matters, how Mailspring and other clients do it, the relevant specs, what Alice has today, a build plan, pitfalls, acceptance criteria and effort. Code citations point at the Alice working tree as of 2026-10-13; Mailspring paths are relative to [Foundry376/Mailspring](https://github.com/Foundry376/Mailspring) (`mailspring/`) and [Foundry376/Mailspring-Sync](https://github.com/Foundry376/Mailspring-Sync) (`mailsync/`).

Sections covered: P0 → Server-side mail state, P0 → Sync correctness, P0 → Push and connections, P1 → Offline, plus P2 additions requested by the lead (end of file).

## Section findings

**The IMAP crate is the first blocker.** Alice uses `imap` 2.4.1 with a vendored `imap-proto` 0.10 (`Cargo.toml` `[patch.crates-io]`, `crates/alice-core/Cargo.toml`). That parser has no `X-GM-LABELS`/`X-GM-THRID`/`X-GM-MSGID` fetch attributes, no `VANISHED` response and no typed `COPYUID`/`APPENDUID` codes (`vendor/imap-proto/src/parser/rfc3501.rs:419-431`, `vendor/imap-proto/src/types.rs`). Worse, `imap`'s `read_response_onto` turns any line it cannot parse into a fake `BYE` error and `assert!`s on tag mismatch (`imap-2.4.1/src/client.rs:1355-1383`). So even raw `run_command_and_read_response` cannot carry Gmail extensions or QRESYNC. Its IDLE is blocking, cannot be interrupted from another thread, and resets the socket read timeout to `None` afterwards (`imap-2.4.1/src/extensions/idle.rs:175-183`). The recommended replacement is **`async-imap` 0.12.0** (2026-09-30, tokio). It depends on `imap-proto` ^0.17, which has `Response::Vanished`, `AttributeValue::GmailLabels/GmailMsgId/GmailThrId/ModSeq` and `ResponseCode::CopyUid/AppendUid/HighestModSeq`. It also offers interruptible IDLE (`wait_with_timeout` returns a future plus a `StopSource`), `uid_mv`, `uid_copy`, `uid_expunge`, `append` with flags and date, `id`, and an `unsolicited_responses` channel. `imap` 3.0.0-alpha.15 (Feb 2025) is still alpha and blocking. `imap-codec`/`imap-types` 1.0 lack UIDPLUS and mark CONDSTORE/QRESYNC "unfinished". Run `async-imap` over `tokio-rustls` 0.26 with `rustls-platform-verifier` 0.7.1. Both are already in `Cargo.lock` via reqwest, so the migration also fixes a live bug: IMAP and SMTP currently use `native-tls` (`providers.rs:9,45`; lettre default features), which on macOS is Secure Transport and cannot do TLS 1.3 (Mailspring #2507, still open).

**Three architectural pieces serve almost every item here.** Build them once:
1. **A per-account connection actor** (new `crates/alice-core/src/connection.rs`). It owns a long-lived *foreground* session (IDLE, user actions, attachment and body fetches) and a *background* session (sync and backfill), reconnects with backoff, and wakes on demand. This replaces `providers::session()` per action, `Remote::connect` per sync run and per attachment.
2. **A persisted, local-first operations queue** (`operations` table). It applies a change locally at once and replays it remotely in order. It is the mechanism behind the offline queue, undo, optimistic UI, the MOVE fallback's crash recovery, Drafts/Sent APPEND and cross-account moves. Mailspring's `Task` table with `performLocal`/`performRemote` and the 24-hour `syncedAt` guard is the reference (`repos/mailsync/MailSync/TaskProcessor.cpp:771-1022,1118-1135`).
3. **A mailbox and membership model.** Alice is already halfway to Mailspring 1.26's "placements": `remote_messages` holds one row per server copy (`sync_store.rs:3-12,24`), the same idea as Mailspring's `MessageFolder` (mailsync `a83cc9a`). But every view still reads the single `messages.folder` "primary copy" chosen by rank (`sync_store.rs:109-118`). Add a `mailboxes` table, membership-based views and a `threads` table. Do these schema changes together with the P0 "Move hot fields … into real indexed columns" item, so there is one migration and not three.

**Recommended build order:**
- (a) Ship now on the current crate: the MOVE fallback, wake/network re-sync, and Sent APPEND detection.
- (b) Crate migration, rustls and the connection actor, then IDLE.
- (c) Operations queue, then the offline queue and Drafts APPEND.
- (d) `mailboxes` table and sync of every folder, then multi-folder views, then threading, then Gmail labels (which need (b) and (d)).
- (e) Full offline mode and the P2 additions.

Test infrastructure already exists: the Dovecot/Postfix fixture (`scripts/mail-fixture.sh`, `scripts/test-mail.py`, `crates/alice-core/tests/mail_provider.rs`) and mock-stream unit tests (`remote.rs:590-675`). Mailspring's scenario list in `repos/mailsync/test/README.md` ("Scenarios" table) is a ready-made checklist of provider behaviours worth porting as named tests.

---

## P0 → Server-side mail state

<a id="save-drafts-to-the-server-drafts-folder-imap-append-so-they-roam-across-devices"></a>

### Save drafts to the server Drafts folder (IMAP APPEND) so they roam across devices

*Roadmap: P0 · Server-side mail state*

- **Why:** A draft started on the laptop is invisible on the phone or in webmail, and is lost if the device dies. Drafts made elsewhere never show up in Alice at all.
- **Competitors:**
  - **Mailspring does not do this.** `SyncbackDraftTask`'s remote phase is a no-op (`repos/mailsync/MailSync/TaskProcessor.cpp:923`, "right now we don't syncback drafts"), and GitHub #1417 "Sync Local Drafts to Remote Folder" (7 👍) was closed unaddressed.
  - What Mailspring does do: it syncs the server Drafts folder down, so drafts from other clients appear. A local draft gets a UID-0 placement in the Drafts folder (`ensureDraftPlacement`, `TaskProcessor.cpp:1362-1377`).
  - Deleting a synced draft leaves an invisible placeholder that takes over the server UID, so the next Drafts scan does not resurrect it (`performLocalDestroyDraft`, `TaskProcessor.cpp:1379-1430`; scenario `synced-draft-destroy`).
  - After a successful send it deletes the server draft copies (`_removeMessageCopiesResilient`, `TaskProcessor.cpp:2278`).
  - If no Drafts role exists it errors with "choose a Drafts folder" (`draftsFolder()`, `TaskProcessor.cpp:1086-1096`; changelog 1.1.3, "display an alert if you need to choose a Drafts folder").
  - **Thunderbird** saves to the identity's server Drafts folder on explicit save and autosave: `mail.compose.autosave=true`, `mail.compose.autosaveinterval=5` minutes (`comm-central/mailnews/mailnews.js`). Each save APPENDs a new version and deletes the old one.
  - **Apple Mail, Outlook and Gmail web** all keep drafts server-side.
- **Spec:**
  - RFC 9051 §6.3.12 (APPEND with flag list and date-time).
  - RFC 4315 §3 (`APPENDUID`) and §2.1 (`UID EXPUNGE`).
  - RFC 6154 §2 (`\Drafts` special-use).
  - RFC 3501 §2.3.2 (`\Draft` flag).
- **Alice today:**
  - Drafts exist only in SQLite: `save_draft` writes `folder: "drafts"` with `remote_mailbox: None` (`crates/alice-core/src/store.rs:342-398`).
  - The draft's Message-ID is already stable, `<{uuid}@alice.local>` (`store.rs:385`).
  - The Drafts mailbox is not discovered or synced, because `discover_folders` returns only inbox/sent/archive/trash/spam (`remote.rs:501-583`).
  - Deliveries freeze a draft (`store.rs:358-367,399-412`).
- **Plan:**
  1. Discover a `drafts` role: `\Drafts` special-use first, then names ("Drafts", "[Gmail]/Drafts", "Entwürfe", "Brouillons"…). Add `drafts` to `Mailboxes` (`model.rs:31-40`) and sync that mailbox (depends on "Sync every folder").
  2. Add a `draft_copies(draft_id TEXT PRIMARY KEY, account_id, mailbox, uid_validity, uid, saved_at)` table in `sync_store.rs`. When the Drafts mailbox syncs, a message whose Message-ID matches `<uuid@alice.local>` maps back to the local draft instead of becoming a new row. A foreign draft (made in webmail) is imported as an editable local draft holding its server copy.
  3. Add an operations-queue op `SaveDraftRemote{draft_id}`, enqueued by `POST /v1/messages` (`daemon.rs:472-481`) and debounced: at most one per draft per 30 to 60 s, plus on composer close and daemon shutdown. Local saves stay immediate. The runner builds MIME with the same builder as send but with lettre's `.keep_bcc()` so Bcc roams, then APPENDs `(\Draft \Seen)`. It records `APPENDUID`, or falls back to `UID SEARCH HEADER Message-ID` above the pre-append UIDNEXT. Only then does it delete the previous UID.
  4. Deleting the old version: use `UID STORE +FLAGS.SILENT (\Deleted)` then `UID EXPUNGE` (UIDPLUS). On Gmail, MOVE it to `[Gmail]/Trash` and expunge there, as in Mailspring's `_removeMessagesResilient` (`TaskProcessor.cpp:308-376`).
  5. On send success, after the Sent handling, delete the server draft copy. On discard (`store.rs:413-434`), enqueue the same deletion.
  6. Conflicts: if the Drafts sync finds a newer server copy of a draft (same Message-ID, different UID, later INTERNALDATE than the local `saved_at`), keep the newer one as the draft. Save the other as a separate local draft titled "conflicted copy". Simple and lossless.
  7. Add a per-account setting "Save drafts to the server", default on (Thunderbird has a per-identity equivalent).
- **Pitfalls:**
  - IMAP messages are immutable, so every save is a full upload. Gmail caps IMAP uploads at 500 MB/day and suspends uploads for 1 to 24 h when exceeded (Google Workspace "Gmail bandwidth limits"). Throttle, skip unchanged content, and with large attachments upload only on close or explicit save.
  - On Gmail, a plain `\Deleted`+EXPUNGE in `[Gmail]/Drafts` can leave the old version in All Mail, depending on the user's "auto-expunge" setting. Move it to Trash first.
  - Webmail or other clients may rewrite a draft (new UID, sometimes a new Message-ID). Fall back to matching on an `X-Alice-Draft-Id` header if Message-ID changes.
  - Never APPEND drafts that are frozen by a pending delivery (`deliveries` row).
  - Drafts with attachments depend on "Attachment upload".
- **Done when:**
  - A `scripts/test-mail.py` case saves a draft with to/cc/bcc, then with imaplib finds exactly one `\Draft` message in Drafts carrying the Bcc header.
  - Editing and saving again leaves exactly one copy.
  - A second Alice instance (separate `ALICE_DATA_DIR`) shows the draft after sync.
  - Sending removes the Drafts copy.
  - A draft saved while the server is unreachable is uploaded after reconnect.
  - Unit test: the Drafts-sync matcher maps `<uuid@alice.local>` back to the local draft id.
- **Effort:** M. Depends on "Sync every folder…" (Drafts discovery), "Offline action queue…" (the runner), "Cc and Bcc fields", and "Attachment upload…" (for drafts with files).

<a id="append-sent-mail-to-the-server-sent-folder-for-providers-that-dont-do-it-themsel"></a>

### Append sent mail to the server Sent folder for providers that don't do it themselves

*Roadmap: P0 · Server-side mail state*

- **Why:** On iCloud, Fastmail and most generic IMAP hosts, mail sent from Alice exists only in Alice's local "Sent". It is missing from webmail and other devices, and is lost with the cache.
- **Competitors:**
  - **Mailspring** `performRemoteSendDraft` (`TaskProcessor.cpp:2085-2511`):
    - After SMTP it polls Sent for the Message-ID among the last 15 messages (`findUIDsOfRecentHeaderMessageID`, `Vendor/mailcore2/src/core/imap/MCIMAPSession.cpp:2129-2158`). Delays are `{0,1,1,2}` s, or `{0,1,1,2,2,3,3}` s once the folder is known to file copies (`TaskProcessor.cpp:2293`).
    - It learns `serverSavesSent` per Sent folder (`constants.h:51`).
    - It APPENDs `\Seen` only when no copy was found (`TaskProcessor.cpp:2346-2356`). If it appended, it records the UID (`_asc`). For 30 minutes (`SENT_COPY_CLEANUP_WINDOW`, `constants.h:54`), later passes delete any late server copy and keep its own (`removeLateSentCopies`, `TaskProcessor.cpp:2031-2083`; called from `SyncWorker.cpp:767`).
    - On Gmail, after appending it copies the thread's labels onto the new message (`TaskProcessor.cpp:2358-2381`) and records the copy's placement in All Mail.
    - Commit `21ab5fa` (2026-09-30, "Stop leaving duplicate Sent copies on servers that file their own"): live on O365 across 76 sends, the server copy appeared 0.5 to 3.5 s after SMTP 250, twice after 6 to 8 s, and once after ~3 min. Exchange Online, Gmail, Yahoo and Zoho file their own copy; iCloud and Fastmail do not by default.
    - Changelog: 1.26 "Fixed duplicate Sent copies … Exchange Online, Gmail, Yahoo and Zoho"; 1.5.7 Gmail duplicates with tracking (#1279); 1.0.4 "Sending mail is considerably faster for accounts that do not place the message in the Sent folder automatically"; GitHub #937 "duplicate in sent folder – outlook.com".
  - **Thunderbird:** "Place a copy in" Sent is on by default (`mail.identity.default.fcc=true`). Gmail users get duplicates unless they turn it off or Gmail merges by Message-ID.
- **Spec:**
  - RFC 9051 §6.3.12 (APPEND).
  - RFC 4315 §3 (`APPENDUID`).
  - RFC 6154 §2 (`\Sent`) and §3 (`CREATE-SPECIAL-USE`).
  - Gmail adds an auto-Bcc of every SMTP-submitted message to Sent Mail (Gmail team comment in GNOME bug 599230).
  - Microsoft: SMTP AUTH client submission stores sent mail in the mailbox's Sent Items (learn.microsoft.com "SMTP submission improvements").
- **Alice today:**
  - `send` delivers over SMTP (`daemon.rs:482-506`, `providers.rs:213-236`) and only relabels the local row `folder="sent"` (`store.rs:447-466`).
  - If the provider files a copy, sync adopts it by Message-ID (`sync_store.rs:136-181` `target()` for `sent`; `store.rs:1381-1393` `upsert`).
  - README is explicit: "Alice does not append a second copy automatically" (`README.md:295`).
  - RSVP mails are sent without any Sent handling (`daemon.rs:633`).
- **Plan:**
  1. Add `server_saves_sent: Option<bool>` to `Account` (`model.rs:11-29`). Seed it by provider: Gmail and Outlook true; generic IMAP None (learned). Add a `sent_copy` column (`pending|found|appended|skipped`) to `deliveries` (`store.rs:22`).
  2. Before SMTP, read Sent's UIDNEXT (STATUS). After `finish_delivery(id, true)`, enqueue `SentCopy{delivery_id}` so a crash between SMTP and APPEND still finishes.
  3. The runner polls `UID FETCH <uidnext>:* (UID BODY.PEEK[HEADER.FIELDS (MESSAGE-ID)])`, which is cheaper than a header SEARCH. Delays: `0,1,1,2` s, or `0,1,1,2,2,3,3` s when `server_saves_sent == Some(true)`. On a hit, set `server_saves_sent=true` and mark `found`; sync adopts the copy.
  4. On a miss, APPEND `(\Seen)` with the exact bytes and INTERNALDATE of the send. Build the lettre `Message` once with an explicit `Date` and `.keep_bcc()` for the Sent copy; the SMTP rendering drops Bcc. Mark `appended` and remember `(mailbox, uidvalidity, uid)`. On Gmail, never APPEND: its auto-copy can lag, and Gmail would only merge by Message-ID while still counting the upload against the quota.
  5. Clean up late copies. Within 30 minutes of an append, when Sent sync finds a second UID with the same Message-ID, delete that server copy and keep ours (Mailspring's rule). Without this, `target()`'s "another UID in this mailbox is a different delivery" rule (`sync_store.rs:155-160`) creates a visible duplicate row.
  6. If no Sent mailbox exists, do not create one silently (Alice never creates folders). Show "Choose a Sent folder" in account settings, offering `CREATE … (USE (\Sent))` when `CREATE-SPECIAL-USE` is advertised.
  7. Apply the same op to RSVP replies, optionally (`daemon.rs:633`).
- **Pitfalls:**
  - Exchange sometimes files its copy minutes late (above), hence the cleanup window.
  - A per-recipient (tracked) send produces one server copy per recipient (Mailspring 1.5.7, `21ab5fa`). Not relevant until Alice does multisend, but the cleanup must not delete copies it did not append beside.
  - Localized Sent names ("Gesendete Elemente", "Sent Items"): rely on `\Sent` first (`remote.rs:557` already does).
  - APPEND of large messages counts against Gmail's 500 MB/day upload limit, which is another reason to skip on Gmail.
  - Mailspring 1.0.4: dates from 1969 when the SMTP gateway rewrote headers. Pass INTERNALDATE explicitly.
  - Never re-run the SMTP part. Keep Alice's at-most-once claim; only the Sent op retries.
- **Done when:**
  - Against the Dovecot/Postfix fixture (Postfix does not file Sent copies), sending one message leaves exactly one copy in `Gesendet`, with `\Seen` and the Bcc header. A second send is not delayed beyond about 4 s.
  - Unit tests with a mock remote cover the decision table: copy found on first poll means no APPEND; never found means APPEND; found late after an APPEND means the late copy is deleted and ours kept; Gmail means no APPEND.
  - Killing the daemon after SMTP but before APPEND still yields one Sent copy after restart.
- **Effort:** M. Depends on "Two long-lived IMAP connections per account…" (needs no new crate, but runs on the actor), "Offline action queue…" (crash-safe op), and "Cc and Bcc fields" (for the Bcc header).

<a id="fall-back-to-copy-deleted-uid-expunge-when-the-server-has-no-move"></a>

### Fall back to COPY + `\Deleted` + UID EXPUNGE when the server has no MOVE

*Roadmap: P0 · Server-side mail state*

- **Why:** Archive, trash and spam fail outright on servers without MOVE ("mail.imap.no_safe_move"), so mail cannot be organised at all there.
- **Competitors:**
  - **Mailspring** `_moveMessagesResilient` (`TaskProcessor.cpp:253-311`): `UID MOVE` when advertised; otherwise `COPY`, then `_expungeMovedCopies` (`TaskProcessor.cpp:224-251`).
  - `_expungeMovedCopies` sets `\Deleted`. If the STORE fails it leaves the copies alone. With UIDPLUS it runs `UID EXPUNGE` of exactly those UIDs, falling back to a plain `EXPUNGE` only without UIDPLUS or if UID EXPUNGE is refused.
  - After a dropped connection it never runs a plain EXPUNGE on the reconnected session (scenario `move-without-move-connection-drop`).
  - It re-applies `\Seen`/`\Flagged`/`\Draft` on the destination copy, and resolves new UIDs from COPYUID, or else by fetching `[UIDNEXT-2n, *]` in the destination and matching header hashes (`_resolveNewUIDs`, `TaskProcessor.cpp:134-222`; underflow fix `0a6b09f`).
  - Changelog: 1.0.11 introduced the fallback. Commit `dfdb083` (1.26) fixed it purging other clients' `\Deleted` mail (Outlook desktop's "mark for deletion" mode); before that fix mailcore never recorded UIDPLUS at all.
  - **Thunderbird** uses the same UID EXPUNGE approach.
- **Spec:**
  - RFC 6851 §3.3 (semantics; COPY/STORE/EXPUNGE "is not atomic … can also expunge unrelated messages") and §4.3 (COPYUID in an untagged OK before expunges).
  - RFC 4315 §2.1 (`UID EXPUNGE`) and §3 (`COPYUID`).
  - RFC 9051 §6.4.3 and §6.4.8.
- **Alice today:**
  - `providers::modify` bails when MOVE is absent (`crates/alice-core/src/providers.rs:135-137`). README states it (`README.md:127`).
  - COPYUID parsing already works on raw bytes (`moved_uid`, `providers.rs:183-191`), with a Message-ID search fallback (`providers.rs:147-168`).
  - `imap` 2.4.1 already has `uid_copy`, `uid_store` and `uid_expunge` (`imap-2.4.1/src/client.rs:787,884,918`), so this can ship before the crate migration.
- **Plan:**
  1. In `providers.rs` (later in the connection actor), add `move_copy(session, uid, dest, caps)`. Use `UID MOVE` if MOVE is advertised. Otherwise:
     - Read the destination's UIDNEXT via STATUS.
     - Run `UID COPY uid dest` through `run_command_and_read_response`; parse COPYUID with `moved_uid`.
     - Run `UID STORE uid +FLAGS.SILENT (\Deleted)`. On failure, stop and report "copied, original left in place".
  2. With `UIDPLUS`, run `UID EXPUNGE uid`. Without UIDPLUS, run `UID SEARCH DELETED`. If the result is a subset of our UIDs, plain `EXPUNGE` is safe. Otherwise leave our copy flagged `\Deleted`, hide it locally (the copy row is removed by `record_move`), and log it. Never purge other messages.
  3. If the connection drops after the EXPUNGE was sent, do not retry the EXPUNGE. A later sync sees the copy either gone or `\Deleted`.
  4. Resolve the destination UID from COPYUID. Otherwise search UIDs at or above the pre-copy UIDNEXT for the Message-ID (narrower than today's whole-mailbox search).
  5. Remove the "requires MOVE" sentence from `README.md:127` and the `mail.imap.no_safe_move` string from the locales.
- **Pitfalls:**
  - Without UIDPLUS, a plain EXPUNGE races with other clients. The `SEARCH DELETED` check narrows the window but does not close it; document that.
  - COPY copies flags; set `\Deleted` only after COPY, so the destination copy is not deleted.
  - Partial failure leaves a duplicate (in both mailboxes), never a loss (RFC 6851 §3.3 asks the same of servers).
  - Yahoo permutes COPYUID order (Mailspring fake "yahoo (permuted COPYUID)"): pair the source and destination sets by position, as RFC 4315 specifies, and verify by Message-ID when counts disagree.
  - Treating a COPY into a missing mailbox (`NO [TRYCREATE]`) as transient would loop forever; it is permanent.
- **Done when:**
  - With the Dovecot fixture's capabilities overridden to drop MOVE (Dovecot `imap_capability`, as Mailspring's `dovecot:no-move` profile does), archiving moves the message, preserves `\Seen`/`\Flagged` in the destination, and records the new UID.
  - A second message another client set `\Deleted` in INBOX survives (port scenario `move-without-move-keeps-other-deleted`).
  - Unit test: mock streams for "UIDPLUS present" and "absent with another \Deleted message", asserting that no plain `EXPUNGE` is sent in the second case.
- **Effort:** S. No dependencies; ship before the crate migration.

## P0 → Sync correctness

<a id="thread-by-message-id-references-in-reply-to-plus-x-gm-thrid-on-gmail-subject-onl"></a>

### Thread by `Message-ID` / `References` / `In-Reply-To`, plus `X-GM-THRID` on Gmail; subject-only grouping as a last resort

*Roadmap: P0 · Sync correctness*

- **Why:** Today any two mails with the same subject in an account merge ("Invoice", "Meeting", newsletters), while a reply whose subject was edited splits off. Gmail users see conversations that differ from the web UI.
- **Competitors:**
  - **Mailspring** `MailProcessor::insertMessage` (`repos/mailsync/MailSync/MailProcessor.cpp:157-235`):
    - On Gmail it uses the thread with the same `gThrId` (`X-GM-THRID`).
    - Otherwise, unless the Message-ID was auto-generated, it looks up `ThreadReference` rows for the message's own Message-ID plus up to 50 References. If nothing matches it creates a new thread. There is no subject fallback and no In-Reply-To lookup.
    - `upsertThreadReferences` indexes its own ID, the first reference and the last 99 (`MailProcessor.cpp:786-818`; commit `bf12deb`). Indexing a message's References means a parent that arrives later finds its children's thread.
    - Threads are never merged (`LIMIT 1`).
    - Schema: `Thread(gThrId …)` with `ThreadGmailLookup` and `ThreadReference(threadId, accountId, headerMessageId)` (`mailspring-schema.sql`).
    - Fixes: 1.0.6 "When you forward a message, it is now correctly associated with the existing thread"; 1.17.2 / `75d69a3` In-Reply-To garbage on iCloud spam; 1.1.3 messages with the same subject and no Date header thrashing.
    - Demand: Mailspring forum #291 "Make Threading/Conversation View Optional" is the top request (172 votes); forum #320 and GitHub #426 ask to move or delete a single message in a thread.
  - **Thunderbird** defaults to References only: `mail.strict_threading=true`, `mail.thread_without_re=false`, `mail.correct_threading=true` (re-parents when the parent arrives late).
  - **Gmail web and Mimestream** use Gmail's thread id.
  - **Outlook** uses ConversationId / `Thread-Index`.
- **Spec:**
  - RFC 5322 §3.6.4 (Message-ID, In-Reply-To, References).
  - JWZ threading algorithm (https://www.jwz.org/doc/threading.html).
  - Gmail IMAP extensions `X-GM-THRID` (developers.google.com/workspace/gmail/imap/imap-extensions).
  - Optional: RFC 8474 `THREADID` where advertised. RFC 5256 `THREAD=REFERENCES` is server-side threading; not needed.
- **Alice today:**
  - Grouping is by normalized subject, computed in Rust over every row on each `threads()` call (`crates/alice-core/src/store.rs:137-271`; `thread_key` at `store.rs:1262-1293`; `thread_rows` at `store.rs:1307-1324`).
  - `ThreadSummary.id` is `account:subject_key` (`model.rs:151-167`).
  - `Message` stores `message_id` and `in_reply_to` but not References (`model.rs:109-110`), although sync downloads the full header block (`remote.rs:175`).
  - README lists the limitation (`README.md:294`). The CLI/TUI list messages, not threads.
- **Plan:**
  1. In `providers::message_from` (`providers.rs:340-423`), parse `References` and `In-Reply-To` with `mailparse::msgidparse` (already a dependency) into `references: Vec<String>`; keep at most the first 1 and last 49. Add `references` to `Message` with `#[serde(default)]`. Drop IDs without `@`, over 250 chars, or equal to the message's own ID.
  2. Schema in `sync_store::migrate`:
     - `threads(id TEXT PRIMARY KEY, account_id, gm_thrid INTEGER, subject_key TEXT, last_date REAL)`, with index `(account_id, gm_thrid)`.
     - `thread_refs(account_id, header_id, thread_id, PRIMARY KEY(account_id, header_id)) WITHOUT ROWID`.
     - A `thread_id` column on messages (with the "hot fields" item).
  3. Assign threads inside `import_copies` (`sync_store.rs:437-584`), in the same transaction:
     - Gmail: thread by `X-GM-THRID`.
     - Otherwise collect {own ID, In-Reply-To, References} and look them up in `thread_refs`. No match: start a new thread, unless the subject has a reply/forward prefix and the message has no References or In-Reply-To. In that case join an existing thread with the same `subject_key` in the same account whose last message is within 30 days and shares a participant.
     - One thread found: join it. Two or more: merge the smaller into the older (update `messages.thread_id` and `thread_refs`).
     - Insert all IDs into `thread_refs`.
  4. Backfill: a one-time migration pass per mailbox, `UID FETCH 1:* (UID BODY.PEEK[HEADER.FIELDS (MESSAGE-ID IN-REPLY-TO REFERENCES)])`, rebuilds threads for cached mail. Until it finishes, existing subject grouping stays as the fallback.
  5. `threads()` and `conversation()` group by `thread_id` (with SQL paging from the performance item). `ThreadSummary.id` becomes the thread id. Keep the Spam isolation and Trash rules of `conversation()` (`store.rs:1325-1365`).
  6. Drafts carry the thread of their `in_reply_to`, so a reply in progress shows inside its conversation.
  7. Add a "Group by conversation" setting (on by default), answering forum #291. When off, list views call `list()` and not `threads()`.
- **Pitfalls:**
  - Out-of-order arrival: index References so a late parent finds its children (Mailspring's trick). Merge threads, which Mailspring never does.
  - Outlook replies often carry In-Reply-To with truncated References; use both.
  - iCloud spam has garbage In-Reply-To (`75d69a3`).
  - Mailing-list digests reuse subjects; keep subject fallback narrow.
  - Gmail splits a thread when the subject changes and caps thread size. Always follow `X-GM-THRID` on Gmail even when References disagree, so the result matches the web UI.
  - Self-addressed mail creates Inbox and Sent rows with the same Message-ID (`sync_store.rs:133-135`). They share a thread and `conversation()` already shows them once (`store.rs:1354-1362`).
  - Selection in the desktop keys on thread id, so the id change needs a one-time reselect.
  - `X-GM-THRID` cannot be read on the current crate (see Section findings).
- **Done when:** `store.rs` unit tests cover:
  - (a) A←B←C chain → one thread;
  - (b) C then B then A arriving out of order → one thread;
  - (c) two unrelated "Invoice" mails without references → two threads;
  - (d) "Re: Lunch" with no headers, within 30 days and sharing a participant → joins, while after 60 days it does not;
  - (e) Gmail THRID overrides References;
  - (f) a message referencing two threads merges them;
  - (g) a migration test builds threads for an existing DB.
  
  `README.md:294` is updated.
- **Effort:** L. Depends on "Move hot fields … into real indexed columns" (shared migration), "Two long-lived IMAP connections per account…" (crate migration, for X-GM-THRID), and "Correct MIME for replies: `References` header chain" (so Alice's own replies thread elsewhere).

<a id="multi-folder-message-model-one-message-can-belong-to-several-mailboxeslabels-at"></a>

### Multi-folder message model: one message can belong to several mailboxes/labels at once

*Roadmap: P0 · Sync correctness*

- **Why:** A message in Inbox and a user folder, or with two Gmail labels, shows only in one place, and moves or deletes can act on the wrong copy. Self-sent mail currently needs a special duplicate-row hack.
- **Competitors:**
  - **Mailspring 1.26** (commit `a83cc9a`, 2026-09-26, plus V11 counter repairs `314fe40` and `d04cfa8`):
    - `MessageFolder` (one row per server copy with per-copy unread/starred/draft bits, `remoteXGMLabels` and `pendingFolderId`) and `MessageOrphan` (`mailspring-schema.sql`).
    - `Message.folders` holds a `{folderId: bits}` snapshot rebuilt from the rows on save.
    - Thread counters are a diff over folder sets.
    - A copy disappearing never deletes the Message. Orphans are swept at the end of a pass only after every folder that could hold a moved copy has been covered, or after 24 h (`SyncWorker.cpp:356-391`; `repos/mailsync/CLAUDE.md` "Message Identity and Placements").
    - Flags fan out to every copy. ChangeFolderTask moves only the copies in `sourceFolderIds`, except that trash and spam take every copy, and empty source means every copy outside sent/drafts. Undo restores each copy to its own folder.
    - On Gmail exactly one placement exists (All Mail, Spam or Trash), with labels on it.
    - Before this, Mailspring flickered:
      - 1.25 `\All` mailbox flicker (`b53e0db`);
      - iCloud two-folder flicker (`4653004`, `0df7864`);
      - Proton All Mail thrash (1.10.0);
      - Gmail flicker when marking read and archiving quickly (1.5.7);
      - unread-archive flicker (1.0.8).
    - Forum #1848: "Don't move sent messages when a thread is moved".
  - **Thunderbird and Apple Mail** treat folders as independent copies. Gmail web, Mimestream and Superhuman are label-native.
- **Alice today:**
  - Copies are already tracked: `remote_messages(account_id, mailbox, uid_validity, uid, message, is_read, is_starred)` (`crates/alice-core/src/sync_store.rs:24`).
  - The message's own `folder`, `remote_mailbox`, `uid_validity` and `remote_uid` name a single "primary" copy chosen by `rank` (spam < inbox < sent < archive < trash, `sync_store.rs:109-118`; `make_primary`/`settle` at `sync_store.rs:192-273`).
  - Views filter on `messages.folder` (`scope_filter`, `store.rs:1108-1137`).
  - Actions act only on the primary copy (`providers.rs:84-93`).
  - Self-sent mail is two rows, because Inbox copies never join Sent messages (`sync_store.rs:133-135`; `providers.rs:424-441`).
- **Plan:**
  1. Rename the concept: `remote_messages` becomes the canonical membership table. Add `pending_mailbox TEXT` (optimistic move target) and `deleted INTEGER` (no-MOVE fallback leftovers). Add an index `(account_id, mailbox, message)`.
  2. Add local memberships: drafts and not-yet-synced sent receipts get a row with `uid=0` in the role mailbox, like Mailspring's UID-0 placements, so every listed message has at least one membership.
  3. List and count queries become membership joins: `… FROM messages m JOIN remote_messages r ON r.message=m.id WHERE r.account_id=? AND COALESCE(r.pending_mailbox, r.mailbox)=?`. Unread is per copy (`r.is_read`). Keep `messages.folder` only as a derived "best folder" for unified views and notifications.
  4. Spam stays exclusive: a message with any Spam copy appears only in Spam (current rule). On Gmail, Trash is exclusive too.
  5. Actions:
     - Mark read/star: STORE in every mailbox holding a copy.
     - Archive/move from a view: move only the copy in the current mailbox (pass the source mailbox in `MessagePatch`).
     - Trash/spam: move every copy.
     - Thread moves skip Sent copies (forum #1848).
  6. Self-sent mail: once membership drives the views, collapse the Inbox/Sent row split in `target()` (`sync_store.rs:175-179`) and `message_row_id` (`providers.rs:426-441`) into one message with two memberships. This needs a migration that merges the pairs.
  7. Orphans: when a copy vanishes and the message has no other copy, keep it hidden for one full pass before deleting. That is a `orphaned_at` column, swept after all mailboxes were covered; it avoids delete-then-re-download on a move seen source-first.
- **Pitfalls:**
  - The grace sweep must not wait forever on a mailbox whose STATUS always fails (Mailspring's 24 h cap, `ORPHAN_SWEEP_MAX_WAIT`).
  - Exchange keeps duplicate Sent copies at adjacent UIDs (scenario `o365-duplicate-sent-copies`), so two copies in one mailbox are two memberships.
  - iCloud and NetEase duplicate freely.
  - Proton Bridge's `All Mail` is a duplicate view; skip non-Gmail `\All` (`b53e0db`).
  - UIDVALIDITY changes should relink by Message-ID, not delete and re-download (Mailspring resets UIDs to 0 and relinks).
  - Per-copy flags differ: self-sent mail is unread in Inbox and read in Sent.
  - The guard against sync reverting pending local changes belongs to "Offline action queue".
- **Done when:**
  - `sync_store.rs` tests: a message with copies in INBOX and "Projects" appears in both views. Archiving from Projects leaves the INBOX membership. Marking read updates both copies. A move seen destination-first then source-gone keeps the same message id and never deletes it (port Mailspring scenarios `remote-move-destination-scanned-first`, `trash-message-with-two-placements` and `flag-change-on-second-placement`).
  - A migration test converts existing self-sent pairs.
- **Effort:** L. Depends on "Sync every folder…" (more mailboxes), "Move hot fields …" (same migration), and "Page the thread list in SQL…" (the membership joins).

<a id="gmail-labels-via-x-gm-labels-read-add-remove-show-labels-in-the-list-and-reader"></a>

### Gmail labels via `X-GM-LABELS`: read, add, remove; show labels in the list and reader

*Roadmap: P0 · Sync correctness*

- **Why:** Labels are how Gmail users organise mail. Today Alice hides them, and filing a message into a label is impossible.
- **Competitors:**
  - **Mailspring:**
    - Gmail accounts sync only All Mail, Spam and Trash. Every other LIST entry becomes a `Label` (`repos/mailsync/MailSync/SyncWorker.cpp:963`).
    - Labels come from `X-GM-LABELS` on the single placement (`MessageFolder.remoteXGMLabels`).
    - `ChangeLabelsTask` issues `UID STORE ±X-GM-LABELS`. `_xgmKeyForLabel` maps role inbox to `\Inbox`, role important to `\Important`, and anything else to the raw path (`TaskProcessor.cpp:625-690`).
    - Archive on Gmail removes `\Inbox` (`repos/mailspring/app/src/flux/tasks/task-factory.ts:76-85`).
    - IDLE falls back to All Mail because Gmail's INBOX is a label (`SyncWorker.cpp:221-232`).
    - Changelog: 1.6.3 "can no longer select a Label as your Gmail Trash"; 1.0.7 separate folder and label shortcuts; 1.22 crash archiving from search with label-based archive (#2743); 1.24.0 `ChangeLabelsTask` assertion (#2816).
    - Demand: forum #540 and GitHub #1006 ask for labels on non-Gmail IMAP via keywords (15 votes).
  - **Mimestream** uses the Gmail API for labels, colors and categories. **Thunderbird** shows Gmail labels as folders, which duplicates storage.
- **Spec:**
  - Gmail IMAP extensions: `X-GM-EXT-1` capability; `FETCH (X-GM-LABELS)`; `STORE +X-GM-LABELS (foo)`; `SEARCH X-GM-LABELS`. Labels are ASTRINGs, modified-UTF-7 where needed. System labels appear as `\Inbox`, `\Sent`, `\Drafts`, `Important`.
  - Google Workspace "Gmail bandwidth limits": ≤500 labels best practice; 2,500 MB/day download.
- **Alice today:**
  - Gmail syncs INBOX, `[Gmail]/Sent Mail`, `[Gmail]/All Mail` (as archive), Trash and Spam as separate mailboxes (`remote.rs:554-583`), deduplicated by Message-ID (`sync_store.rs:402-430`).
  - User labels are invisible.
  - The current parser cannot read `X-GM-LABELS` (Section findings). Archive is `UID MOVE INBOX → All Mail` (`providers.rs:124-141`).
- **Plan:**
  1. Prerequisite: the crate migration (`imap-proto` 0.17 `AttributeValue::GmailLabels/GmailThrId/GmailMsgId`).
  2. Add `labels(account_id, path TEXT raw mUTF-7, name TEXT decoded, role, PRIMARY KEY(account_id, path))`, filled from LIST entries on Gmail that are not `\All`, `\Junk`, `\Trash`, `\Noselect` or `[Gmail]/Important`/`Starred`. Add `message_labels(account_id, message, label)`, or a JSON `labels` column on the All Mail membership row.
  3. Gmail sync mode: sync only All Mail, Spam and Trash. Fetch `X-GM-LABELS X-GM-THRID X-GM-MSGID` with headers and flags. Derive the Inbox, Sent and Drafts views from `\Inbox`, `\Sent` and `\Drafts`, and Starred from `\Flagged`. This halves Gmail header traffic (no INBOX + All Mail double fetch) and removes the double-copy reconciliation.
  4. Change detection:
     - Gmail advertises CONDSTORE but not QRESYNC. Use `CHANGEDSINCE` for flags and labels if a live test shows Gmail bumps MODSEQ on label changes (unconfirmed; RFC 7162 does not require it for non-flag metadata).
     - Otherwise re-fetch `(UID FLAGS X-GM-LABELS)` for the newest ~400 UIDs every 2 min and all UIDs in the window every 10 min (Mailspring's shallow and deep intervals, `SyncWorker.cpp:29-31`).
  5. Actions:
     - Archive: `UID STORE -X-GM-LABELS (\Inbox)` on the All Mail UID.
     - Add/remove label: `±X-GM-LABELS ("Name")`.
     - Move to label: add the label and remove `\Inbox`.
     - Trash and spam: MOVE to `[Gmail]/Trash` or `[Gmail]/Spam` (Gmail drops other labels).
     - Extend `MessagePatch` (`model.rs:212-218`) with `labels_add`/`labels_remove`.
  6. UI:
     - `ThreadSummary` gets `labels`; list rows show chips.
     - The reader header shows labels with remove ×.
     - The sidebar gets a per-account "Labels" tree (nested by `/`).
     - Add a label picker with type-ahead (shared with "Move to any folder") and an `l` shortcut.
     - Colors are not exposed over IMAP: hash a color, or let the user pick and store it locally.
  7. Search: add a `label:` operator in `search.rs`; `X-GM-RAW` is a server fallback later.
- **Pitfalls:**
  - Quote and mUTF-7-encode label names (spaces, `"`, non-ASCII), and compare raw strings with LIST output.
  - `\Important` is noisy; hide it by default.
  - Labels hidden from IMAP in Gmail settings ("Show in IMAP" unchecked) can still appear in `X-GM-LABELS`; tolerate unknown labels.
  - A message in Trash or Spam has no other labels.
  - Gmail session BYE "Session expired, please login again" after hours: reconnect.
  - Sent-only messages (no `\Inbox`) must not appear in Inbox.
  - Gmail's per-account connection limit: ~15 simultaneous IMAP connections across all clients (third-party reports; Google publishes no number).
  - Note for later: model labels so IMAP keywords (`$label1`, custom keywords when `PERMANENTFLAGS` has `\*`) can reuse the same table (forum #540).
- **Done when:**
  - A parser unit test reads a recorded `* 1 FETCH (UID 5 X-GM-THRID 1278455344230334865 X-GM-LABELS (\Inbox "Muy Importante" foo/bar) FLAGS (\Seen))`.
  - With an opt-in live Gmail test (`#[ignore]`, like `tests/mail_provider.rs`):
    - a label added in Gmail web appears within one pass;
    - add and remove from Alice are visible in web;
    - archive removes only `\Inbox`;
    - a message with two labels shows in both label views;
    - INBOX is no longer selected during sync.
- **Effort:** L. Depends on "Two long-lived IMAP connections per account…" (crate migration), "Multi-folder message model…", "Sync every folder…" (label list and tree UI), and "Move to any folder with type-ahead picker" (picker).

<a id="sync-every-folder-not-only-the-five-special-use-roles-show-the-folder-tree-in-th"></a>

### Sync every folder, not only the five special-use roles; show the folder tree in the sidebar

*Roadmap: P0 · Sync correctness*

- **Why:** Users who file mail into folders (or whose server rules do) cannot see any of it in Alice, and cannot move mail to those folders.
- **Competitors:**
  - **Mailspring:**
    - `syncFoldersAndLabels` (`SyncWorker.cpp:836-1128`) takes every selectable LIST entry (dropping `\Noselect` and duplicate LIST lines, `95829f2`) and creates Folder rows.
    - Roles come from SPECIAL-USE flags, then from names in `COMMON_FOLDER_NAMES` (`constants.h:346`), decoded from modified UTF-7. That decoding was broken until `dfdb083`, 1.26, which also added `\Archive`.
    - `syncNow` runs STATUS on every folder each pass in role order inbox, sent, drafts, all, archive, trash, spam, then the rest (`SyncWorker.cpp:419-425`), skipping non-Gmail `\All` (`b53e0db`).
    - Users can override role mapping (1.0.4 "choose folder associations explicitly").
    - Changelog: 1.20 nested folder and label creation UX; 1.24.1 unread counts on every folder; 1.0.4 `INBOX.` namespace prefix; 1.0.6 emoji folder names and rename flicker.
  - **Thunderbird** lists subscribed folders by default and checks only Inbox for new mail unless `mail.server.default.check_all_folders_for_new=true` (default false). Per-folder "Check this folder for new messages".
- **Spec:**
  - RFC 9051 §6.3.9 (LIST), §6.3.11 (STATUS), §5.1 and Appendix A.1 (hierarchy, modified UTF-7).
  - RFC 3501 §5.1.3 (mUTF-7).
  - RFC 6154 §2 (special-use attributes, including `\Archive`).
  - RFC 2342 / RFC 9051 §6.3.10 (NAMESPACE).
  - RFC 5819 (LIST-STATUS, an optimisation).
  - RFC 6855 (UTF8=ACCEPT).
- **Alice today:**
  - `discover_folders` maps only inbox/sent/archive/trash/spam (`crates/alice-core/src/remote.rs:501-583`).
  - Everything else is dropped by `forget_other_mailboxes` (`sync.rs:281-290`; `sync_store.rs:301-311`).
  - The desktop sidebar is a fixed list of seven views (`apps/desktop/src/App.tsx:20-25`). `MessagePatch.folder` only accepts the four roles (`daemon.rs:459-471`, `store.rs:327-333`).
  - Folder names are never mUTF-7-decoded.
  - Each pass EXAMINEs every mailbox (`remote.rs:114-130`).
- **Plan:**
  1. Add a `mailboxes(account_id, path TEXT raw, name TEXT decoded, delimiter, parent, role, attributes TEXT, subscribed INTEGER, sync INTEGER DEFAULT 1, id TEXT uuid-v5(account,path), PRIMARY KEY(account_id, path))` table in `sync_store.rs`, refreshed from `LIST "" "*"` each pass. Fold the `Mailboxes` struct (`model.rs:31-54`) into role lookups on this table.
  2. Add a ~30-line mUTF-7 decoder (modified base64 with `,` for `/`, UTF-16BE) using the `base64` crate already in deps. Keep the raw path for every IMAP command, the decoded name for display only.
  3. Roles: SPECIAL-USE (`\Sent \Drafts \Trash \Junk \Archive \All`), then a localized name table (port Mailspring's `COMMON_FOLDER_NAMES`, fixed in `dfdb083`), then the user's overrides.
  4. Sync loop (`sync.rs:275-345`):
     - Iterate synced mailboxes in role order, then the rest alphabetically.
     - Use `STATUS (MESSAGES UIDNEXT UIDVALIDITY UNSEEN HIGHESTMODSEQ)` to skip unchanged mailboxes without EXAMINE. NOOP first if the mailbox is the selected one (Dovecot answers STATUS from a stale view).
     - Skip `\Noselect`/`\NonExistent`, non-Gmail `\All`, and Gmail labels once "Gmail labels…" lands.
     - When a path disappears, wait one pass before `forget` (rename = delete + create; otherwise the moved mail is deleted and re-downloaded).
  5. API: `GET /v1/folders?account_id=` returns the tree with total/unread counts. `MessageQuery.folder` accepts role names or mailbox ids. `MessagePatch.folder` accepts a mailbox id (shared with "Move to any folder").
  6. Desktop sidebar: under each account, role folders first, then a collapsible tree (`\HasChildren`, delimiter-split), with per-folder unread badges. The unified view keeps role-only entries. The CLI gets `alice folders`, and the TUI a folder pane.
  7. Settings: a per-folder "sync this folder" toggle (shared with the P2 "Per-account and per-folder sync settings").
- **Pitfalls:**
  - Hundreds of folders mean hundreds of commands per pass. Use STATUS, or LIST-STATUS where advertised; push the full scan to the background session.
  - Namespace prefixes (`INBOX.` with `.` delimiter on Courier and older Cyrus; Mailspring 1.0.4).
  - Exchange lists non-mail folders (Calendar, Contacts, Sync Issues…); offer hiding.
  - Gmail `[Gmail]` is `\Noselect`.
  - Proton Bridge `Labels/*` folders duplicate messages.
  - Gateways list the same mailbox twice (`95829f2`).
  - Shared and other-user namespaces (O365 shared mailboxes are a separate item).
  - Initial sync of many large folders can take days and load the machine (Mailspring GitHub #1007). Keep the existing slice budget (`sync.rs:37`) and the window.
  - Coremail/NetEase need `ID` before SELECT, and some servers report UIDNEXT 0 (`dfdb083`).
- **Done when:**
  - The Dovecot fixture gains nested folders `Projects/Alpha` and a non-ASCII `Koš` created via imaplib. A message appended to `Projects/Alpha` appears in Alice under the right tree node, with an unread count.
  - Moving a message from INBOX to `Projects/Alpha` works.
  - Renaming the folder server-side keeps the message without re-download (check the fetch log).
  - Unit tests: mUTF-7 round-trip (`Ko&AWE-` ↔ `Koš`) and role detection from localized names without SPECIAL-USE (port scenario `folder-roles-by-localized-name`).
- **Effort:** L. Depends on "Multi-folder message model…" (views by membership) and "Move to any folder with type-ahead picker" (UI overlap).

## P0 → Push and connections

<a id="imap-idle-on-inbox-and-notify-where-supported-instead-of-60s-polling"></a>

### IMAP IDLE on Inbox (and NOTIFY where supported) instead of 60s polling

*Roadmap: P0 · Push and connections*

- **Why:** New mail and verification codes arrive up to 60 s late, and changes made elsewhere lag just as long. The OTP feature works around this by polling every 15 s (`sync.rs:236`).
- **Competitors:**
  - **Mailspring's foreground worker** (`repos/mailsync/MailSync/SyncWorker.cpp:108-321`) loops:
    - fetch queued bodies;
    - connect and log in;
    - run queued remote tasks;
    - pick the IDLE folder: inbox role, else `all` (Gmail);
    - apply VANISHED the connection already received;
    - catch up the folder via CONDSTORE (or a 100-message UID range without it);
    - `IDLE` until a server event or an interrupt.
  - `idleInterrupt()` is called 300 ms after a task is queued, on `need-bodies`, and on `wake-workers` (`main.cpp:782-833`).
  - IDLE errors are logged but not thrown, because Yandex drops IDLE abruptly (`SyncWorker.cpp:309-314`). Throwing was tried and reverted: `a3978b2`, reverted `f5730dc`, changelog 1.17.4.
  - VANISHED during IDLE was ignored until `5a31251` (1.18.0, FastMail). QRESYNC is disabled for iCloud (`2ddaf57`, `SyncWorker.cpp:402-405`).
  - The background worker still scans all folders every 120 s.
  - NOTIFY is not implemented.
  - **Thunderbird** has `mail.server.default.use_idle=true` and checks every 10 min (`check_time=10`).
- **Spec:**
  - RFC 2177 ("re-issue it at least every 29 minutes"; no commands until `DONE`).
  - RFC 9051 §6.3.13.
  - RFC 7162 §3.1.4.1 (CHANGEDSINCE), §3.2.3 (QRESYNC needs `ENABLE QRESYNC`), §3.2.5 (SELECT QRESYNC), §3.2.10 (VANISHED).
  - RFC 5465 NOTIFY: Dovecot implements it; the big hosted providers do not advertise it, so check CAPABILITY.
- **Alice today:**
  - `run_background` polls every 60 s, or 15 s with OTP auto-copy (`crates/alice-core/src/sync.rs:220-241`).
  - Each run opens and logs out a session (`sync.rs:264-269`).
  - Unsolicited responses are discarded (`remote.rs:413-416`).
  - README: "Push (IMAP IDLE) is not implemented" (`README.md:292`).
- **Plan:**
  1. Prerequisite: "Two long-lived IMAP connections per account…" (async-imap foreground session).
  2. Foreground loop in `connection.rs`:
     - SELECT the IDLE mailbox (Inbox; All Mail once Gmail runs in label mode) with `(CONDSTORE)`, or `(QRESYNC (validity modseq))` after `ENABLE QRESYNC` when advertised and not iCloud.
     - Catch up that mailbox with the existing `Run::refresh` logic (`sync.rs:347-486`) refactored to take one mailbox.
     - `idle.init()`, then `select!` on `wait_with_timeout(10 min)`, the action channel, and the wake signal.
  3. On `NewData` (EXISTS, EXPUNGE, FETCH FLAGS or VANISHED), debounce 500 ms, send `DONE`, apply the data, run the mailbox catch-up, call the arrival hook (`daemon.rs:46-50`), and re-IDLE. On `Timeout`, `DONE` and re-IDLE. On an action, stop via `StopSource`, run the action, re-IDLE.
  4. Apply unsolicited EXPUNGE/VANISHED/FETCH from every command; do not discard them as `drain()` does.
  5. If IDLE is not advertised, fall back to today's 60 s poll. With IDLE active on Inbox, the background pass for other folders can relax to 2 to 5 min (Mailspring: 120 s).
  6. Remove the 15 s OTP poll once IDLE is on.
  7. NOTIFY: defer. If added later, `NOTIFY SET (selected (MessageNew MessageExpunge FlagChange)) (subtree INBOX (MessageNew MessageExpunge))` on Dovecot only, behind a capability check.
- **Pitfalls:**
  - The tokio timer uses monotonic time, which stops during sleep on macOS and Linux. A dead socket after wake can sit until keepalive fires; see "Re-sync immediately on wake…".
  - NAT and firewall idle timeouts are often far below RFC 5382's 2 h 4 min: re-IDLE every ~10 min and enable TCP keepalive (Thunderbird: `mail.imap.tcp_keepalive.idle_time=100`, retry 5 s).
  - Dovecot sends IDLE notifications about 0.5 s late and delivers part of a burst only after `DONE` (Mailspring test README).
  - Gmail ends sessions after hours with "BYE Session expired"; refresh the OAuth token before reconnect.
  - iCloud QRESYNC is broken (`2ddaf57`).
  - Cyrus clips VANISHED at `*`; bound the CHANGEDSINCE set at UIDNEXT-1 (`SyncWorker.cpp:1392-1400`, cyrus-imapd #6071).
  - Battery: stagger reconnects across accounts.
- **Done when:** An integration test with the Dovecot fixture shows:
  - with the daemon idle, a message APPENDed to INBOX via imaplib is stored and appears in `/v1/arrivals` within 3 s;
  - a `\Seen` set by another connection is applied within 3 s;
  - an expunge by another connection removes the message within 3 s;
  - an archive issued during IDLE completes within 1 s;
  - the re-IDLE interval (set via an env override to 5 s in the test) cycles without reconnecting.
  
  Port Mailspring scenarios `new-mail-during-idle`, `remote-flag-changes`, `qresync-two-expunges-during-idle` and `connection-dropped-during-idle`.
- **Effort:** M (L if counted with the crate migration). Depends on "Two long-lived IMAP connections per account…".

<a id="two-long-lived-imap-connections-per-account-one-for-idle-one-for-sync-and-action"></a>

### Two long-lived IMAP connections per account (one for IDLE, one for sync and actions)

*Roadmap: P0 · Push and connections*

- **Why:** Every star, archive and attachment open costs DNS, TCP, TLS and XOAUTH2 login, several round trips and often more than a second. Archiving a 10-message conversation opens 10 sessions in a row (`apps/desktop/src/App.tsx:171` loops `patch` per id). It also churns provider connection limits.
- **Competitors:**
  - **Mailspring** keeps two long-lived sessions per account process: background for folder scans, foreground for IDLE, tasks and body fetches (`repos/mailsync/MailSync/main.cpp:145-226`). Both retry every 120 s after a retryable error, and `wake-workers` cuts that short.
  - Tasks queued in a burst are coalesced by a 300 ms wake delay (`main.cpp:787-801`).
  - Online detection uses the real IMAP and SMTP servers rather than DNS (1.7.4, #1357).
  - **Thunderbird** caches up to 5 IMAP connections per account by default (users hitting Gmail limits lower it to 1).
  - Gmail allows ~15 simultaneous IMAP connections per account across all clients (third-party reports; no official number).
- **Spec:**
  - RFC 9051 §6.2.2 (AUTHENTICATE), §6.1.2 (NOOP), §7.1.5 (BYE).
  - RFC 2971 (ID, recommended by Gmail and required by Coremail).
  - RFC 7628 / Google XOAUTH2.
- **Alice today:**
  - `providers::session` dials and logs in afresh: 20 s connect timeout, 30 s read/write timeout, native-tls (`crates/alice-core/src/providers.rs:31-76`).
  - `modify` opens a session per action (`providers.rs:87`).
  - Sync opens one per run (`sync.rs:264-269`).
  - Attachments open one per download (`daemon.rs:541-546`).
  - `Remote` already has poison/reconnect logic for a confused session (`remote.rs:52-98`).
  - Per-account locks serialise edits and sync batches (`sync.rs:71-81`).
- **Plan:**
  1. Dependencies:
     - Replace `imap` and the vendored `imap-proto` with `async-imap = { version = "0.12", default-features = false, features = ["runtime-tokio"] }` (check the exact feature name on docs.rs);
     - add `tokio-rustls` and `rustls-platform-verifier` (both already in `Cargo.lock`);
     - drop `native-tls` for IMAP;
     - delete `vendor/imap-proto` and the `[patch]`.
  2. New `crates/alice-core/src/connection.rs`: `AccountConnection`, one tokio task per account, holding up to two sessions:
     - `fg`: IDLE, user actions, attachments;
     - `bg`: sync and backfill (the current `Run`).
     - An `mpsc` channel carries `Op` (Modify, Append, FetchAttachment, Wake, Shutdown); a `watch` publishes status (connected, offline, auth-failed).
     - The `SyncEngine` (`sync.rs`) drives the `bg` session instead of `Remote::connect`.
  3. Connect:
     - TCP via `tokio::net::TcpStream` plus `socket2` keepalive (idle 100 s, interval 5 s, Thunderbird's values);
     - rustls;
     - XOAUTH2 or LOGIN;
     - `CAPABILITY` (cached: MOVE, UIDPLUS, IDLE, CONDSTORE, QRESYNC, X-GM-EXT-1, SPECIAL-USE, ID, MESSAGELIMIT, LIST-STATUS);
     - `ID ("name" "Alice" "version" …)`.
  4. Health: NOOP before use if idle for more than 60 s. On BYE or IO error, mark the session dead and reconnect with backoff 2 s, 5 s, 15 s, 30 s, 60 s, capped at 5 min, with jitter. Reset on success and on wake. Re-fetch the OAuth token (`oauth::credentials`) before every reconnect. Treat auth failures as non-retryable until the user re-authenticates (ties to "Re-authenticate flow…").
  5. Batch actions: the daemon accepts multiple ids per PATCH (or the queue groups consecutive ops), and the actor issues one `UID STORE`/`UID MOVE` per mailbox with `uid_set` (`remote.rs:452-473`).
  6. Port `Remote` methods to async on the session. Keep the read-only EXAMINE discipline for `bg` and SELECT on `fg`.
  7. Lifecycle: close sessions on account removal (`SyncEngine::forget`, `sync.rs:123-125`) and on pause; LOGOUT on daemon shutdown.
- **Note:** The todo says "one" connection. Recommend **two** per account, as Mailspring does: one connection cannot IDLE while a long backfill FETCH runs, and user actions would queue behind it. Two per account stays well under provider limits.
- **Pitfalls:**
  - Session expiry: Gmail sends BYE "Session expired" after hours; Exchange can drop sessions when the access token expires. Always handle BYE as "reconnect".
  - O365 transiently answers `NO Server Unavailable. 15` to LIST, then BYE; treat it as offline, not "folder missing" (`dfdb083`).
  - The 30 s read timeout must not apply while IDLE.
  - Async migration means all of `remote.rs` (722 lines) and its callers move to async, so plan a full-day port plus tests.
  - Stale selected-mailbox views on Dovecot: NOOP before trusting STATUS or FETCH on the selected mailbox (`SyncWorker.cpp:1347-1361`).
  - Never run two commands concurrently on one session (async-imap requires `&mut`).
  - Yahoo `MESSAGELIMIT` truncates large FETCHes (RFC 9738; Mailspring handles `[MESSAGELIMIT]`).
- **Done when:**
  - Starring 20 messages in a row creates one IMAP login, checked in the protocol log or with a test-only connection counter.
  - Archiving a 10-message thread issues one `UID MOVE`.
  - Killing the server connection (`doveadm kick alice@example.test` in the fixture) reconnects within the first backoff step and the queued action completes.
  - Auth failure surfaces as an account error without a hot reconnect loop.
  - The existing `scripts/test-mail.py` passes unchanged.
- **Effort:** L. Prerequisite for "IMAP IDLE on Inbox…", "Gmail labels via `X-GM-LABELS`…", the X-GM-THRID part of threading, and "Offline action queue…".

<a id="re-sync-immediately-on-wake-from-sleep-and-when-the-network-comes-back"></a>

### Re-sync immediately on wake from sleep and when the network comes back

*Roadmap: P0 · Push and connections*

- **Why:** After opening the laptop, mail can stay stale for a minute or more. Worse, a sync can hang on a dead pre-sleep socket until its timeout.
- **Competitors:**
  - **Mailspring:**
    - Electron `powerMonitor` `resume` and `unlock-screen` send `system-resumed` to the main window (`repos/mailspring/app/src/browser/application.ts:708-717`).
    - `OnlineStatusStore` calls `sendSyncMailNow()` on that, on `window` `online`, when an account's connection error clears, and when a 5-minute interval fires more than 1 minute late, which means the machine slept. All of these are throttled to once per 3 s (`app/src/flux/stores/online-status-store.ts:5-81`).
    - `wake-workers` wakes sleeping workers and interrupts IDLE, "because our network connection may have been reset" (`repos/mailsync/MailSync/main.cpp:810-824`).
    - The comment at `application.ts:708` notes the engine's retry wait "only counts awake time".
    - Changelog: 1.0.12 "When waking from sleep, Mailspring checks for new mail immediately" (#468); 1.26 "coming back online re-syncs your accounts immediately"; 1.7.4 online checks hit the real IMAP/SMTP servers. GitHub #1407 "accounts having issues – and doesn't retry".
- **Alice today:**
  - `run_background` waits `tokio::time::sleep(60 s)` or `wake.notified()` (`crates/alice-core/src/sync.rs:233-239`).
  - `SyncEngine::wake` exists (`sync.rs:137-140`) and `POST /v1/sync` catches up (`daemon.rs:507-519`).
  - There is no sleep or network detection.
  - The desktop's `connectionError` refers to the daemon, not IMAP (`apps/desktop/src/App.tsx:64,136-143`).
- **Plan:**
  1. In `sync.rs`, add a clock-gap watcher task: tick every 15 s and compare `SystemTime` elapsed against the expected period. If the wall clock advanced 45 s or more beyond it, the process was suspended. Then call `SyncEngine::wake_all()`: interrupt IDLE, drop sessions older than the gap, reset backoff, run `sync_all`. Keep the decision in a pure function `slept(prev_wall, now_wall, period) -> bool` for testing. This needs only std and works for CLI/TUI-only users.
  2. Stagger the per-account catch-up by about 250 ms to avoid a burst.
  3. Desktop: listen for webview `online` events, and for `visibilitychange` when hidden for more than 5 min. Throttled to 3 s, call `POST /v1/sync` (the bridge already proxies daemon calls).
  4. Reconnect backoff in the connection actor; a wake resets it.
  5. Optional later, only if the gap watcher proves insufficient: OS hooks (macOS `NSWorkspaceDidWakeNotification`, Windows `WM_POWERBROADCAST`/`PBT_APMRESUMEAUTOMATIC`, logind `PrepareForSleep(false)` over D-Bus) and network-path monitors.
- **Pitfalls:**
  - Tokio timers do not advance during suspend on macOS and Linux, so a 60 s sleep can end long after wake. On Windows the monotonic clock may keep counting. The wall-delta-between-ticks check covers both cases.
  - NTP jumps cause a harmless extra sync.
  - Do not hammer: many accounts waking together against Gmail's connection limit.
  - DHCP and VPN changes kill sockets without sleep. Keepalive and the IDLE re-issue catch those.
- **Done when:**
  - Unit test: `slept()` returns true for a 10-minute wall gap at a 15 s period, and false for jitter under 30 s.
  - Manual test: sleep the laptop for 5 min, send mail to the account, wake; it appears within 10 s.
  - Fixture test: `doveadm kick` followed by `POST /v1/sync` recovers without waiting out 60 s.
- **Effort:** S. A first version (wake leads to `sync_all` plus `wake()`) ships on today's code; IDLE interruption follows "IMAP IDLE on Inbox…".

<a id="imap-and-smtp-over-rustls-so-tls-13-only-servers-connect-on-macos"></a>

### IMAP and SMTP over rustls so TLS 1.3-only servers connect on macOS

*Roadmap: P0 · Push and connections · new*

- **Why:** On macOS, `native-tls` uses Secure Transport, which cannot negotiate TLS 1.3. A TLS 1.3-only IMAP or SMTP server cannot be added at all. This is Mailspring's open GitHub #2507.
- **Competitors:** Mailspring ships OpenSSL in mailsync and still has #2507 open; it added a legacy-TLS fallback that still requires trusted certificates (`3ca1cfa`, `f7b0239`). Thunderbird uses NSS.
- **Spec:** RFC 8446 (TLS 1.3); RFC 8314 (implicit TLS for mail).
- **Alice today:**
  - IMAP: `native_tls::TlsConnector` (`crates/alice-core/src/providers.rs:9,16,45`).
  - SMTP: lettre's default `native-tls` feature (`Cargo.lock` lettre depends on `native-tls`).
  - HTTP already uses rustls (`Cargo.toml` reqwest `rustls`, `rustls-platform-verifier` 0.7.1 in the lock).
- **Plan:**
  1. IMAP: wrap `TcpStream` in `rustls::StreamOwned` with `rustls_platform_verifier::ConfigVerifierExt` (system trust stores, matching native-tls behaviour). `imap::Client::new` accepts any `Read + Write`. A newtype implements `imap`'s `SetReadTimeout`, needed only for IDLE. This becomes `tokio-rustls` after the async migration.
  2. SMTP: set `lettre = { version = "0.11", default-features = false, features = ["smtp-transport", "builder", "hostname", "pool", "rustls", "rustls-platform-verifier"] }`. Confirm the platform-verifier feature name in lettre 0.11.23's features. Use `Tls::Required` / `Tls::Wrapper` as today.
  3. Remove `native-tls` from `crates/alice-core/Cargo.toml` once unused.
- **Pitfalls:**
  - Corporate MITM roots are installed in the OS store; the platform verifier honours them, plain webpki-roots would not.
  - Some ancient servers still need TLS 1.0/1.1, which rustls refuses. Report a clear error rather than downgrade (Mailspring's legacy fallback still requires trusted certificates, `f7b0239`).
  - Keep certificate validation on (README promise).
- **Done when:** The fixture with Dovecot `ssl_min_protocol = TLSv1.3` and Postfix `smtpd_tls_protocols = >=TLSv1.3` connects, syncs and sends on macOS. The existing fixture still passes.
- **Effort:** S. No dependencies; do it first or inside the connection migration.

## P1 → Offline

<a id="offline-action-queue-archive-move-flag-and-send-work-offline-and-replay-in-order"></a>

### Offline action queue: archive, move, flag and send work offline and replay in order when online

*Roadmap: P1 · Offline*

- **Why:** On a plane or flaky Wi-Fi, every archive and star fails with an error, and sending is impossible. A failed send also leaves the draft's delivery "uncertain" and blocks retries.
- **Competitors:**
  - **Mailspring** queues every mutation as a `Task` row (`Task(id, version, data, accountId, status)` with index `TaskByStatus`, `mailspring-schema.sql`). Its lifecycle is `local` → `remote` → `complete` or `cancelled`:
    - `performLocal` applies the change in one transaction on the main thread, sets a 24 h `syncedAt` guard plus an in-flight counter so incoming sync cannot revert it, and for moves sets `pendingFolderId` so the UI shows the destination immediately (`repos/mailsync/MailSync/TaskProcessor.cpp:771-896,1118-1180`).
    - `performRemote` runs only on the foreground worker after connect and login succeed, so offline tasks simply wait (`SyncWorker.cpp:155-186`).
    - On failure, flags are rescanned from the server and moves abandoned so the UI shows server truth (`TaskProcessor.cpp:1190-1294`); the task completes with an error the client shows.
    - A send never runs twice (`_performRemoteRan`, `TaskProcessor.cpp:2093-2095`).
    - Tasks stuck in `local` after a crash are dropped at launch; only the last 100 completed are kept (`TaskProcessor.cpp:725-769`).
    - Queued sends show in an Outbox (`repos/mailspring/app/src/flux/stores/outbox-store.ts`). An offline notification offers "Try now".
    - Undo is an inverse task, with engine-written snapshots for moves (`repos/mailspring/docs/undo-redo-task-pattern.md`).
    - Changelog: 1.0.8 list flicker archiving unread mail; 1.5.7 Gmail flicker on quick read+archive; 1.0.6 undo for generic IMAP. GitHub #1086 duplicate sends.
  - **Thunderbird** "Work Offline" plays back an offline-operations log and an "Unsent Messages" outbox. **Apple Mail** has an Outbox.
- **Alice today:**
  - `change` is remote-first: it opens IMAP, applies the change, then saves locally, so offline it errors (`crates/alice-core/src/daemon.rs:410-458`).
  - Drafts change locally (`store.rs:314-341`).
  - Send claims the delivery, then on any failure marks it `unknown`, which blocks automatic retries (`daemon.rs:482-506`, `store.rs:399-412,447-466`; `README.md:235,296`).
- **Plan:**
  1. Add `operations(id TEXT PRIMARY KEY, account_id, seq INTEGER, kind TEXT, payload TEXT, status TEXT CHECK(status IN ('pending','running','done','failed')), attempts, next_attempt_at, error, created_at)` with index `(account_id, status, seq)` in `store.rs`. Keep the last ~200 done rows for diagnostics.
  2. `change` becomes local-first. In one transaction it validates, applies the local change (flags; `pending_mailbox` on the affected memberships, from "Multi-folder message model"), and inserts the op. It returns at once, bumps the revision and pokes the account's connection actor (debounced ~300 ms, like Mailspring, so bursts batch).
  3. The runner in the connection actor takes pending ops in `seq` order:
     - Resolve UIDs at run time from the membership rows, never at enqueue time, since UIDs change after MOVE and UIDVALIDITY changes. Group consecutive same-kind ops per mailbox.
     - On success, clear `pending_mailbox` and record new UIDs (`record_move`, `sync_store.rs:586-610`).
     - On transient errors (IO, BYE, timeout), back off and keep the op pending.
     - On permanent errors (`NO [TRYCREATE]`, `[NONEXISTENT]`, message gone), mark it failed, clear the pending markers, trigger a catch-up of the mailbox, and expose the error via `/v1/state`.
  4. Sync guard: `apply_flags`, `import_copies` and `remove_copies` (`sync_store.rs:330-584`) skip messages that have a pending or running op, the equivalent of Mailspring's `syncedAt`.
  5. Sends:
     - Queue `Send{draft_id}`. When run, use lettre's lower-level `SmtpConnection` so connect, EHLO, STARTTLS and AUTH failures (`Kind::Connection/Network/Tls` before `MAIL FROM`) mean "not sent, retry later".
     - Only failures after `DATA` mark the delivery `unknown`, keeping at-most-once.
     - Add an Outbox view for queued sends with "Send now" and "Cancel". Cancel is allowed only before the claim.
     - After success, the Sent-copy op follows.
  6. Desktop and TUI: pending indicator on rows, a per-account "Offline, N changes waiting" banner from the actor's status, and undo (Organization P1) as inverse ops. If the original is still pending, drop both.
  7. MCP access checks still happen at enqueue time (`crates/alice/src/mcp.rs`).
- **Pitfalls:**
  - Ordering (archive then unarchive) must hold per message.
  - A queued op on a message the server deleted meanwhile is dropped, not retried forever.
  - UIDVALIDITY changes while queued: re-resolve by Message-ID.
  - Sync must not resurrect a copy the user just moved (Mailspring `mid-pass-foreground-tombstone`, `remote-move-while-task-in-flight`).
  - The queue survives daemon restarts. Ops left `running` at crash become `pending` again, except sends: a send left `running` becomes `unknown`.
  - Conflicting remote edits: user intent wins on replay (last writer).
  - Gmail flicker came from sync overwriting local state between local and remote (1.5.7); the guard fixes this.
- **Done when:** Store-level tests with a fake remote:
  - with the server unreachable, archive puts the message in Archive locally at once with an op pending;
  - a sync before replay does not revert it;
  - after reconnect the server shows it archived and the op is `done`;
  - two ops on one message replay in order.
  
  Fixture tests:
  - with the account pointed at a closed port, send queues; after fixing the port, Postfix delivers exactly once;
  - an error injected after DATA (`smtpd` `-o smtpd_end_of_data_restrictions`) yields `unknown` with no retry.
- **Effort:** L. Depends on "Two long-lived IMAP connections per account…" (runner), "Multi-folder message model…" (pending markers), and "Undo for archive, trash, move, mark read" (shares the queue).

<a id="optional-full-offline-mode-download-all-bodies-and-attachments-within-the-window"></a>

### Optional full offline mode: download all bodies and attachments within the window

*Roadmap: P1 · Offline*

- **Why:** Travellers and people on poor connections cannot open attachments, inline images or very long messages offline.
- **Competitors:**
  - **Mailspring has no such mode.** Bodies are cached for 3 months, hard-coded (`maxAgeForBodySync`, "TODO pref!"). Bodies fetched over 14 days ago and older than that are purged (`cleanMessageCache`, `repos/mailsync/MailSync/SyncWorker.cpp:1471-1498`). Attachments download on demand only; there is no prefetch preference in `app/src/config-schema.ts`.
  - **Thunderbird** downloads full messages for offline use by default (`mail.server.default.offline_download=true`, `autosync_offline_stores=true`, `autosync_max_age_days=-1`). Account Settings → Synchronization & Storage offers per-folder offline selection, "Synchronize the most recent …" and "Don't download messages larger than … KB".
  - **eM Client and classic Outlook** ("Download email for the past …") offer per-account offline scopes.
- **Alice today:**
  - Sync already stores headers and text/HTML parts (up to 4 MB each) and invites for every message in the window (`crates/alice-core/src/sync.rs:10-11`; `README.md:292`), so plain reading works offline.
  - Missing: attachments (fetched per open, `remote.rs:308-361`, `daemon.rs:521-548`), inline `cid:` images (not implemented, `README.md:293`), and text parts over the limit (`providers.rs:379-381`).
  - `AccountStorage.bytes` says attachments are never stored (`model.rs:182`).
- **Plan:**
  1. Add a per-account setting `offline: { attachments: off | up_to_mb(u32) | all }`, default off, stored on `Account` (shares UI with the P2 "Per-account and per-folder sync settings").
  2. Add a cache under `<data>/cache/<account>/<sha256(mailbox,uidvalidity,uid,section)>` with 0600 permissions, plus a table `attachment_cache(message, idx, path, bytes, fetched_at)`. The attachment endpoint serves from cache first. The same cache backs `cid:` images and oversized text parts.
  3. A background job on the `bg` session, after backfill finishes (`Outcome.remaining == 0`), walks the window newest first and fetches sections with the existing section logic (`remote.rs:362-412`). Budget: about 1 GB/day per account by default, and stop on bandwidth errors.
  4. Eviction: when the window narrows, the account is removed, or the cap is exceeded (LRU by `fetched_at`). The storage report includes cache bytes and the Settings → Mail storage panel shows them (`apps/desktop/src/MailStorage.tsx`).
- **Pitfalls:**
  - Gmail caps IMAP downloads at 2,500 MB/day and suspends access for 1 to 24 h when exceeded (Google Workspace "Gmail bandwidth limits"), so the budget must stay well below that.
  - Disk usage on small SSDs.
  - Cached files are unencrypted, like bodies (README); keep 0600 and inside the data dir.
  - Executables stay "save and reveal, never open" (existing rule).
  - Sections can shift if the server reconstructs the message; key by section and verify the name, as `attachment_section` does (`remote.rs:362-386`).
- **Done when:**
  - With the setting at "up to 10 MB", after sync and with the network disabled, opening any in-window attachment under 10 MB works and `cid:` images render.
  - The storage page shows cache size; narrowing the window frees it.
  - A unit test covers eviction ordering.
- **Effort:** M. Depends on "Two long-lived IMAP connections per account…" (bg session) and the P2 "Per-account and per-folder sync settings" (UI). Inline `cid:` rendering is owned by the Reading/Composer items.

## P2 additions from user demand (NEW)

<a id="per-account-and-per-folder-sync-settings-sync-window-which-folders-sync-and-gmai"></a>

### Per-account and per-folder sync settings: sync window, which folders sync, and Gmail All Mail scope

*Roadmap: P2 · Sync · new*

- **Why:** One global window does not fit a 15-year archive account next to a small work account. Users with huge All Mail (5 GB in forum #626, "Don't Sync All of All Mail") want to cap it, and users with many folders want to skip some.
- **Competitors:**
  - **Mailspring has no per-account settings.** Bodies are 3 months, hard-coded (`SyncWorker.cpp` `maxAgeForBodySync`). Forum #170 (17 votes): "Right now, this is only for the last three months, except for Spam and Trash". Forum #474 (5 votes): per-account and per-folder sync, scan and cache settings. GitHub #473 (7 👍) was closed without the feature.
  - **Thunderbird** has per-account "Synchronization & Storage" (keep messages, most-recent age, size limit, per-folder offline selection) and per-folder "Check this folder for new messages".
  - **Classic Outlook** has a per-account "Download email for the past" slider.
- **Alice today:**
  - A single global `Preferences.sync_window` (`crates/alice-core/src/model.rs:286-310,322`) is read for every account (`sync.rs:168`).
  - It is set in Settings → Mail accounts (`apps/desktop/src/Settings.tsx:150` → `MailStorage`).
  - Changing it wakes sync (`daemon.rs:1054`).
- **Plan:**
  1. Add `sync_window: Option<SyncWindow>` to `Account` (`model.rs:11-29`), where `None` inherits the global value. `Run::run` uses `account.sync_window.unwrap_or(global)`.
  2. Per-folder: `mailboxes.sync` on/off plus an optional window, from "Sync every folder". Role folders cannot be disabled except Spam and Trash.
  3. Gmail All Mail scope: "All Mail: everything in window" or "only mail that is in Inbox, Sent or has a label". The latter filters the window search with `UID SEARCH SINCE … X-GM-RAW "in:inbox OR in:sent OR has:userlabels"` (verify on a live account).
  4. API: extend the account PATCH (new `PATCH /v1/accounts/{id}`, shared with "Edit an existing account's settings") with `sync_window`, `folders: {id: {sync, window}}`, and `offline` (from "Optional full offline mode"). Changes call `SyncEngine::wake()`.
  5. UI: an account detail sheet in Settings with a window select ("Use default" first), a folder checklist with sizes from `/v1/storage`, and the Gmail scope radio.
- **Pitfalls:**
  - Narrowing a window deletes local copies (existing behaviour, `README.md:292`); confirm when it frees more than ~1 GB.
  - Disabling a folder must keep messages that also live in a synced folder (membership model).
  - `X-GM-RAW` adds server CPU and returns UIDs only for the selected mailbox.
  - Search should say "searching N days of mail" when windows differ per account.
- **Done when:**
  - A store test: account A at 30 days and account B at all-time each keep the right UIDs after one pass.
  - Disabling a folder removes only messages without another membership.
  - The fixture test shows the per-account window honoured.
- **Effort:** M. Depends on "Sync every folder…" and "Edit an existing account's settings; reorder accounts; per-account color".

<a id="cross-account-move-and-copy-of-messages-and-folders"></a>

### Cross-account move and copy of messages and folders

*Roadmap: P2 · Sync · new*

- **Why:** Mail arrives at the wrong account (personal vs work), and users consolidating providers want to move whole folders. It is the fifth most-voted request on Mailspring's forum and was never built.
- **Competitors:**
  - **Mailspring** does not support it. GitHub #580 was labelled "product-direction" and closed; forum #244 has 37 votes and #1713 asks the same.
  - **Thunderbird** supports drag-and-drop and "Move To / Copy To" across accounts: it downloads the message and APPENDs it to the target, then deletes the source for a move.
  - **Apple Mail** supports dragging between accounts' mailboxes.
- **Spec:** RFC 9051 §6.3.12 (APPEND with flags and INTERNALDATE), RFC 4315 §3 (APPENDUID), RFC 6851 (source removal via MOVE to Trash) or COPY/UID EXPUNGE.
- **Alice today:** `MessagePatch` moves only within the account's role folders (`crates/alice-core/src/model.rs:212-218`, `daemon.rs:459-471`). `imap` 2.4.1 already has `append_with_flags_and_date` (`imap-2.4.1/src/client.rs:1187`).
- **Plan:**
  1. Add an op kind `CrossAccountTransfer{message, dest_account, dest_mailbox, mode: copy|move}` in the operations queue. The UI calls `POST /v1/messages/{id}/transfer`.
  2. Runner steps:
     - fetch `BODY.PEEK[]` from the source (bg session);
     - before APPEND, `UID SEARCH HEADER Message-ID` in the destination, so a re-run after a crash does not duplicate;
     - APPEND with the source flags (`\Seen`, `\Flagged`, `\Answered`) and the original INTERNALDATE;
     - get the UID from APPENDUID or a search;
     - for a move, delete the source by MOVE to the source Trash, then expunge on Gmail, or `\Deleted` + UID EXPUNGE elsewhere.
     
     Each step is idempotent and its progress recorded in the op payload.
  3. Folder transfer: create the destination tree (CREATE with the target delimiter and mUTF-7 encoding), then queue per-message transfers in UID order with progress in the UI. Cancel is allowed between messages.
  4. UI: "Move to…" and "Copy to…" pickers list every account's folders. Drag a thread onto another account's folder in the sidebar. Show a progress toast for bulk transfers.
- **Pitfalls:**
  - Never delete the source before the destination copy is confirmed. A duplicate is acceptable; a loss is not.
  - Gmail's 500 MB/day IMAP upload limit suspends uploads for 1 to 24 h, so warn above about 300 MB.
  - Provider size limits on APPEND (oversized messages get `NO`): mark the op failed and keep the source.
  - Gmail labels do not carry over to non-Gmail. Thread ids are per account.
  - The sending identity does not change; this is filing, not forwarding.
  - Sent-folder semantics: moving into another account's Sent is allowed but confusing.
- **Done when:**
  - Two fixture accounts (add a second user to `mail-fixture.sh`): copying a message leaves it in both with flags and date preserved; moving leaves it only in the destination.
  - Killing the daemon mid-move and restarting yields exactly one destination copy and no source copy.
  - A 50-message folder transfer completes with progress shown.
- **Effort:** M for messages; +M for folders. Depends on "Offline action queue…", "Sync every folder…" (pickers), and "Create, rename, delete and nest folders and labels" (folder transfer).

<a id="pause-an-account-stop-syncing-idle-and-sends-for-an-account-without-removing-it"></a>

### Pause an account: stop syncing, IDLE and sends for an account without removing it

*Roadmap: P2 · Sync · new*

- **Why:** When a company server is down for maintenance, or an account is temporarily unwanted, the client keeps retrying, logging errors and draining battery. Users want to pause it without losing cached mail and settings.
- **Competitors:**
  - **Mailspring** cannot do it; forum #442 "Temporary Disable/Enable Account" (3 votes): "Mailspring attempts repeatedly to connect to a mail server that will never be available".
  - **Apple Mail** has an "Enable this account" checkbox in Mail → Settings → Accounts.
  - **Thunderbird** has no single switch (uncheck "Check for new messages" per account). **Outlook** can exclude an account from Send/Receive groups.
- **Alice today:**
  - `SyncEngine::pause` already stops an account's sync and blocks new runs while its guard is held (`crates/alice-core/src/sync.rs:126-136`). It is used for server moves, not exposed.
  - `run_background` iterates every account (`sync.rs:222-241`).
- **Plan:**
  1. Add `paused: bool` (`#[serde(default)]`) to `Account` (`model.rs:11-29`).
  2. `run_background`, `sync_all` and the connection actor skip paused accounts. Pausing closes sessions (LOGOUT) and stops IDLE.
  3. Queued operations stay pending; sends to a paused account stay in the Outbox with "Account paused".
  4. Notifications and OTP auto-copy ignore paused accounts.
  5. API and UI: `PATCH /v1/accounts/{id} {paused}`. A toggle in Settings → Mail accounts and in the account's sidebar context menu. Paused accounts show dimmed with a "Paused" badge; the unified Inbox still lists their cached mail, greyed.
  6. CLI: `alice accounts pause|resume <id>`.
- **Pitfalls:**
  - OAuth refresh tokens can expire while paused for months (Google: 6 months unused); on resume, surface the re-auth flow.
  - Do not drop queued sends silently.
  - Resume should run a catch-up immediately.
- **Done when:**
  - A test: a paused account makes no IMAP connection across two background passes (connection counter).
  - An op queued while paused runs after resume.
  - The UI shows the badge and a CLI round-trip works.
- **Effort:** S. Better after "Two long-lived IMAP connections per account…" and "Offline action queue…", but the sync-skip part ships now.

### Recurring complaints to keep in mind (Pitfalls across this area)

- Mail disappearing from the Inbox (GitHub #304, "mailserver-incompatibility"). Root causes in Mailspring were the one-folder model (`b53e0db`, `4653004`, fixed by `a83cc9a`), iCloud QRESYNC (`2ddaf57`), and VANISHED handling (`eba3adc`: a range query without its lower bound unlinked whole folders). Alice guards: membership model, orphan grace, iCloud QRESYNC off, tested VANISHED range math.
- Duplicate Sent on Outlook.com (GitHub #937) and duplicate sends (#1086): see the Sent APPEND item; keep at-most-once SMTP.
- Multi-day initial sync that slows the machine (GitHub #1007). Mailspring also silently dropped messages beyond 1,024 per chunk on CONDSTORE+QRESYNC servers until `2076b05` (1.25). Alice's backfill slices (`sync.rs:32-39`) should stay. Every "come back for the rest" path needs a test that checks nothing is skipped (port `condstore-initial-sync-large`, `heavy-fetch-truncation-backlog`).
- TLS 1.3-only servers (GitHub #2507, open): Alice currently uses native-tls for IMAP and SMTP, so on macOS it has the same failure. See the (NEW) rustls item.
- "Accounts having issues – and doesn't retry" (GitHub #1407): reconnect backoff must reset on wake and on network return.
