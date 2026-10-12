# How Mailspring's sync engine works

Back to [`todo.md`](../../todo.md).

## Appendix D: How Mailspring's sync engine works

This is a deep dive into Mailspring's native sync engine (Foundry376/Mailspring-Sync, "mailsync") at `f7b0239` (2026-10-09), the engine that ships in Mailspring 1.26.0. It was read from the source, the 2024-06 to 2026-10 git history, the engine's own `CLAUDE.md`, its test suite, Mailspring's SQLite schema and the Electron client code that drives it.

How to read the citations:

- Bare paths (`MailSync/SyncWorker.cpp:351`) are relative to the mailsync repo root.
- `client:` paths are in the Mailspring client repo, and `alice:` paths are in this repo.
- Seven-character hex strings (`a83cc9a`) are mailsync commits.

Most 2026 commits have long, precise messages written with AI assistance. Many describe bugs measured on real accounts, so they are quoted here as evidence of what broke.

### D.0 The short version

- **One process per account, several threads, one shared SQLite file.** The client spawns `mailsync --mode sync` once per account. Inside it are a *background* worker that walks folders, a *foreground* worker that IDLEs on the Inbox and runs tasks, a calendar/contacts thread and two metadata threads. Every thread has its own SQLite connection to the same `edgehill.db`. Every account's process uses that file too, and the client reads it directly through a read-only connection.
- **The engine is the only writer.** Each write emits a JSON "delta" on stdout so the reactive UI can update. User actions arrive on stdin as **tasks**, which have a *local* phase (an optimistic database change, done at once) and a *remote* phase (the IMAP/SMTP work, run later by the foreground worker).
- **Message identity is a hash of headers, not of the location.** In 1.26 a message can have several **placements** (one `MessageFolder` row per physical copy: folder, UID, flags). This replaced eight months of "which folder wins" heuristics that made mail flicker between folders.
- **The sync loop depends on server capabilities.** With CONDSTORE and QRESYNC it uses `CHANGEDSINCE` plus `VANISHED`, with a daily full-folder safety scan. Without them it does a 2-minute "shallow" scan of the newest 400 UIDs and a 10-minute "deep" attributes-only scan of the whole folder. Headers are synced for *every* message in *every* folder. Full bodies (whole RFC 822, attachments written to disk) are kept only for the last three months.
- **Most 2026 fixes are of one kind:** a cursor recorded past work that was never done, or a server notification dropped. The examples are `syncedMinUID` past a truncated fetch (`2076b05`), `HIGHESTMODSEQ` past dropped `VANISHED` (`eba3adc`) and `uidnext` past a truncated fetch (`a83cc9a`).
- **The big 2026 investment was a black-box test harness.** It runs the real binary against Dovecot and Cyrus in Docker and against a scriptable fake IMAP server whose quirks are named "personalities" (`test/README.md`). It found five engine bugs in two days, and Mailspring credits it with making the placements refactor shippable (`ms.md` 1.26.0 notes).

---

### D.1 Process model

#### Processes and threads

```
Electron client ──stdin: JSON lines──▶ main thread ──performLocal──▶ edgehill.db (SQLite, WAL)
      ▲                                     │ wakes foreground after 300 ms        ▲ ▲ ▲ ▲
      │                                     ▼                                       │ │ │ │
      └──stdout: JSON deltas◀──DeltaStream◀─┬─ foreground SyncWorker (IDLE, tasks, bodies)
                                            ├─ background SyncWorker (folder passes)
                                            ├─ calContacts (CalDAV/CardDAV, Google People)
                                            ├─ metadata (identity-server stream)
                                            └─ metadataExpiration (snooze/reminder timers)
client's DatabaseStore reads edgehill.db through its own read-only connection
```

- **One process per account.** The README says so ("Mailspring runs one mailsync process for each connected email account… auth failures, connection errors, etc. can simply terminate the process"). The client launches one `MailsyncProcess` per account (`client:app/src/flux/mailsync-bridge.ts:333-389`). The process gets the account and identity JSON as two lines on stdin after it prints its first line (`client:app/src/mailsync-process.ts:222-229`, `MailSync/main.cpp:940-991`).
- **Modes.** `sync`, `test` (validates IMAP and SMTP for account setup: it requires an Inbox, or All Mail on Gmail; `main.cpp:301-419`), `reset`, `migrate` and `install-check` (`main.cpp:924-1086`; the usage string also lists `calendar`, which has no handler). The client runs `--mode migrate` before launching sync and watches stdout for "Running Migration" or "Running Vacuum" to show a progress window (`client:app/src/mailsync-process.ts:501-517`).
- **Threads in `sync` mode** (`main.cpp:1053-1086`):

| Thread | What it does | Entry |
|---|---|---|
| main | Blocks on stdin and runs each task's local phase as it arrives | `main.cpp:740-868` |
| background | Lists folders, then loops `syncNow()` (one pass over all folders) until nothing is left, then sleeps up to 120 s | `main.cpp:168-227`, `SyncWorker.cpp:351-781` |
| foreground | Started after the first folder-list sync. Each cycle: fetch requested bodies, run queued remote phases, sync the Inbox, IDLE | `main.cpp:145-166, 186-202`, `SyncWorker.cpp:108-319` |
| calContacts | Calendars every 15 min, contacts every 75 min; never kills the process | `main.cpp:229-298` |
| metadata | Long-lived HTTP stream from id.getmailspring.com, with backoff 3 s → 300 s | `MetadataWorker.cpp:25, 43-87, 111-153` |
| metadataExpiration | Sleeps until the next plugin-metadata expiry, then emits `metadata-expiration` deltas | `MetadataExpirationWorker.cpp:55-139` |

- **Each thread owns its SQLite connection.** `SyncWorker` makes its own `MailStore` (`SyncWorker.cpp:72-80`). `MailStore::assertCorrectThread` throws if a store is used from the wrong thread, because prepared statements are cached per connection (`MailStore.cpp:432-450`).
- **Startup is staggered.** Background workers wait 0-4 s, derived from the first character of the account id (`Models/Account.cpp:32-38`, `main.cpp:173`). Contacts wait a further 15 s (`main.cpp:233`). This avoids "database locking caused by many sync workers all trying to open several sqlite references at once" (`main.cpp:171-172`).

#### Commands in (stdin)

`runListenOnMainThread` reads newline-delimited JSON (`main.cpp:740-868`):

| `type` | Effect |
|---|---|
| `queue-task` | Runs `TaskProcessor::performLocal` on the main thread, then schedules **one** foreground `idleInterrupt()` 300 ms later, de-duplicated by an atomic flag, so a burst of tasks causes one wake (`main.cpp:782-802`) |
| `cancel-task` | Sets `should_cancel` on the task row (`TaskProcessor.cpp:1024-1032`) |
| `wake-workers` | Marks every folder busy again, wakes sleeping workers, interrupts IDLE (`main.cpp:810-824`, `MailUtils.cpp:955-965`) |
| `need-bodies` | Queues message ids for the foreground worker and interrupts IDLE (`main.cpp:826-834`) |
| `sync-calendar` | One-off calendar refresh on a detached thread (`main.cpp:836-855`) |

The client sends `wake-workers` on the "Sync now" command, when it comes back online, and on OS resume or unlock. It does this because "the sync engine's retry wait only counts awake time" (`client:app/src/browser/application.ts:708-717`, `client:app/src/flux/stores/online-status-store.ts:79-81`).

#### Deltas out (stdout)

- **Every `MailStore::save` and `remove` produces a delta.** `save()` bumps the model's version `v`, runs `beforeSave`, does an UPDATE or INSERT, runs `afterSave` and emits `{"type":"persist","modelClass":…,"modelJSONs":[…]}`. `remove()` emits `unpersist` (`MailStore.cpp:664-772`).
- **Deltas wait for the transaction.** Inside a transaction they accumulate and are emitted on commit. A rollback discards them, so the UI never sees state that was rolled back (`MailStore.cpp:611-662, 1322-1328`).
- **`DeltaStream` coalesces.** It buffers per model class, merges repeated saves of one object into the last one while *merging keys*, so a conditionally included `body` or `rulesReady` survives, and flushes on a timer (`DeltaStream.cpp:79-99, 149-204`). The main thread uses a 5 ms window so task results show at once (`main.cpp:744`). Sync workers use 500 ms (`SyncWorker.cpp:79`).
- **Process-state pseudo-models ride the same stream.** `ProcessState` carries `connectionError` (online/offline), `ProcessAccountSecretsUpdated` carries refreshed OAuth tokens and `ProcessIdentityRefreshNeeded` asks the client to refresh the identity (`DeltaStream.cpp:206-230`, `client:app/src/flux/mailsync-bridge.ts:465-479`).
- **The client re-broadcasts each delta.** It sends it to other windows, inflates models and triggers `DatabaseStore` observers. When a `Task` delta arrives with `status: complete`, it calls `task.onError` or `task.onSuccess` (`client:app/src/flux/mailsync-bridge.ts:439-518`).

#### Crash and restart

- **Fatal means `abort()`.** A non-retryable `SyncException`, or any non-`SyncException`, in a sync worker calls `abort()` (`main.cpp:151-165, 212-224`). Retryable ones set `connectionError` and sleep up to 120 s, and `wake-workers` cuts the sleep short (`MailUtils.cpp:948-953`).
- **Orphaned processes exit.** If stdin has been broken for 30 s, the engine exits with code 141 (`main.cpp:762-777`).
- **On quit the client simply kills every process** (`client:app/src/flux/mailsync-bridge.ts:564-569`). The README calls this safe because "all modifications are done within sqlite transactions".
- **The client relaunches a crashed process.** If it crashes 5 times in 5 minutes, the client stops and marks the account `SYNC_STATE_AUTH_FAILED` or `SYNC_STATE_ERROR`. It decides which by string-matching the error text for `401`, `403`, `invalid_grant` or `ErrorAuthentication` (`client:app/src/flux/mailsync-bridge.ts:66-112, 354-380`).
- **Tasks are cleaned up at launch.** Tasks left in `local` status (a crash during the local phase) are deleted. Completed tasks are trimmed to the last 100 (`TaskProcessor.cpp:725-767`).

---

### D.2 Data model

#### Storage style

Every model table is "fat": a `data` JSON column holds the whole object, and a few copied columns exist only so they can be indexed. Adding a field needs no migration. The README admits the cost: larger rows and slower scans ("In retrospect we may have chosen a different approach"). `MailModel` carries `id`, `aid` (account id), `v` (version, bumped on every save; version 1 means INSERT), `__cls` and an optional `metadata` array of plugin values (`Models/MailModel.cpp:22-137`). Schema DDL is a series of `Vn_SETUP_QUERIES` vectors in `constants.h`, applied by `MailStore::migrate()` against `PRAGMA user_version`, which is currently 12 (`MailStore.cpp:116, 211-296`).

#### Tables that matter for mail

| Table | Role | Schema |
|---|---|---|
| `Message` | One logical message. JSON holds headers, participants, `folders` snapshot, `labels`, flags, `threadId`, `files`, `_sa` and `_suc` (task lock), `rr` (rules state) | `constants.h:164-183`, rebuilt in V10 `constants.h:297-332` |
| `MessageBody` | `id`, `value` (HTML or text), `fetchedAt`. A row with NULL `value` is a "body fetch in progress" reservation | `constants.h:191-192, 206-207` |
| `MessageFolder` | **Placements**: one row per physical copy (`messageId`, `folderId`, `remoteUID`, `unread`, `starred`, `draft`, `remoteXGMLabels`, `pendingFolderId`) | `constants.h:250-261, 337-340` |
| `MessageOrphan` | Messages with no placement and since when; the end-of-pass sweep reads it | `constants.h:262-265` |
| `Thread` | Per-thread counters and arrays: `folders` and `labels` with `_refs` and `_u`, `unread`, `starred`, `lmrt`/`lmst`/`lmt`/`fmt` timestamps, participants, `searchRowId` | `constants.h:110-136`, `Models/Thread.cpp:23-49` |
| `ThreadReference` | `(threadId, accountId, headerMessageId)`, the threading index | `constants.h:138-142` |
| `ThreadCategory` | One row per (thread, folder or label) with `inAllMail`, `unread` and the timestamps; covering indexes serve folder thread lists | `constants.h:144-156` |
| `ThreadCounts` | `(categoryId, unread, total)`, the folder badges | `constants.h:158`, `Models/Folder.cpp:71-91` |
| `ThreadSearch` | FTS5 per thread (`subject`, `to_`, `from_`, `categories`, `body`) | `constants.h:160` |
| `Folder` / `Label` | `path`, `role`, and `localStatus` (sync cursors, see D.3) inside the JSON | `constants.h:90-108` |
| `Task` | The task queue: JSON plus a `status` column (`local`/`remote`/`complete`/`cancelled`) | `constants.h:202, 211-212` |
| `ModelPluginMetadata`, `DetatchedPluginMetadata` | Plugin-metadata join table with an `expiration` index, plus a holding table for metadata that arrived before its message | `constants.h:185-189`, `Models/MailModel.cpp:139-211` |

#### Identity

- **Messages: a hash of their headers.** `MailUtils::idForMessage` is base58(SHA-256(`accountId-` + date + subject + `-` + sorted To/Cc/Bcc mailboxes + `-` + Message-ID)), truncated to 30 bytes (`MailUtils.cpp:620-694`). The folder and UID are deliberately left out, so a message keeps its id when it moves. That id is the key for plugin metadata (snooze, reminders, read receipts) synced between devices (`CLAUDE.md` "Message Identity and Placements").
- **Quirks of that hash:**
  - Server-generated Message-IDs count as empty.
  - A message with no Date header falls back to `folderPath:UID`, accepting "delete + create" when it moves (`MailUtils.cpp:670-679`).
  - There are two schemes, switched by the Mailspring ID's creation date, because an older formula used a localized date string and could not be changed without losing metadata (`MailUtils.cpp:580-591, 622-633`).
  - The README admits collisions: "In rare cases, two messages in an account can have the same ID and Mailspring will only show one."
- **Other ids:**
  - Folders: hash of `accountId:path`, so a rename is a delete plus a create (`MailUtils.cpp:524-529`).
  - Threads: `"t:" + <first message id>` (`Models/Thread.cpp:23-24`).
  - Local drafts: hash of `accountId:headerMessageId` (`MailUtils.cpp:572-578`, `TaskProcessor.cpp:1059`).
  - Random ids now come from the OS entropy source, after two processes seeded by `time(0)` collided (`dc4a446`, `MailUtils.cpp:555-570`).

#### Placements (1.26, schema V10)

What a placement is: one physical copy of a message on the server. The `MessageFolder` table is canonical. Everything else is derived from it:

- **`Message.folders` is a snapshot** `{folderId: bits}` with bit 1 = unread, 2 = starred, 4 = draft (`Placement.hpp:22-26`). It carries ids only, never paths or roles.
- **Message-level `unread`/`starred`/`draft`** are the OR over placements. `draft` is also true for any copy in the Drafts folder. `labels` is the union of X-GM-LABELS (`MailStore.cpp:876-922`).
- **A copy being moved is reported under its pending destination:** `reportedFolderId()` returns `pendingFolderId` when set (`Placement.cpp:44-46`). That is how optimistic moves show at once.
- **UID 0 means "not on the server"**: a local draft, or a copy waiting to be relinked after a UIDVALIDITY change. A partial unique index enforces `(accountId, folderId, remoteUID)` only `WHERE remoteUID > 0` (`constants.h:338`).
- **All writes go through one set of helpers.** These are `upsertPlacement`, `setPlacementUnread/Starred/Labels`, `beginPlacementMove`/`commitPlacementMove`/`abandonPlacementMove`, `removePlacement` and the bulk SQL helpers (`MailStore.hpp:132-164`, `MailStore.cpp:939-1320`). A helper sets an in-memory `_placementsChanged` flag. `Message::beforeSave` then rebuilds the snapshot once, however many helpers ran (`Models/Message.cpp:517-523`).
- **Orphans.** A copy that disappears is deleted at once. A message left with **no** rows keeps its flags and gets a `MessageOrphan(messageId, since)` record in the same transaction (`MailStore.cpp:900-911, 1156-1170`). `INSERT OR IGNORE` keeps the earliest `since` on purpose, so an orphan refreshed every pass still expires (`MailStore.cpp:901-903`). Any later upsert of a copy clears the record, which is how a moved message keeps its id, body and metadata when the destination folder is scanned after the source.
- **Gmail keeps exactly one placement**, in All Mail, Spam or Trash. Upserting into one of those deletes the others, because Gmail has no QRESYNC to report the old copy as gone (`MailProcessor.cpp:299-304`, `MailStore.cpp:1010-1019`).

How it got here (worth knowing because Alice is at the earlier stage, see D.11):

1. Until September 2026 a `Message` had one `remoteFolderId`/`remoteUID`. A message in two folders at once collapsed onto one row, and the folders "took turns" owning it on every pass. Examples: self-addressed mail in Inbox and Sent, Exchange's duplicate Sent copies, ProtonMail's `\All`, iCloud. Inbox and Sent looked empty and mail flickered (`b53e0db` message: "15 folder reassignments in 7 min").
2. Patches tried a **folder priority** (inbox > sent > archive > trash), first only for iCloud (`4653004`, `b7d0a7e`) and then more widely (`0df7864`). On normal servers a priority makes correctness depend on whether a move's source or destination is scanned first: "the source folder must detect the removal BEFORE the destination folder detects the addition. This ordering cannot be reliably guaranteed" (`b7d0a7e`).
3. `a83cc9a` (#144, 2026-09-26) replaced all of it with placements and orphans.

**The V10 migration** (`MailStore.cpp:298-368`, `constants.h:244-344`):

- It runs in one explicit `BEGIN IMMEDIATE` transaction.
- It refuses to start without 1.5× the database size free, and says so in plain words.
- It backfills one placement per message. A move still in flight becomes a row with `pendingFolderId` set.
- It rebuilds `Message` without its location columns, and builds the indexes after the bulk insert.
- Measured time: 12.5 s on a 1.15 GB / 258k-message database.

**V11 repairs thread counters** that older engines left wrong (`314fe40`, `d04cfa8`, `MailStore.cpp:370-430`). Two causes:

- an operator-precedence bug that stored a label's unread count `_u` as a boolean;
- both workers applying one change's thread delta twice.

On a 55k-thread database it recounted 3,741 threads.

#### Thread state, maintained by diffing

Thread counters are never recomputed from scratch at runtime. When a `Message` is loaded, a `MessageSnapshot` is captured: unread, starred, file count, labels and the folders map (`Models/Message.hpp:33-50`, `Models/Message.cpp:137-178`). `Message::afterSave` calls `Thread::applyMessageAttributeChanges(oldSnapshot, newMessage)`, which:

- subtracts the old state and adds the new;
- keeps per-folder `_refs` (distinct messages) and `_u` (unread, counted by **that copy's** bit, so a message unread in Inbox and read in Sent makes the thread bold only in Inbox);
- keeps per-label `_refs` and `_u` (counted only when the message is in All Mail);
- keeps the timestamps and participants (`Models/Thread.cpp:168-335`).

`Thread::afterSave` then rewrites `ThreadCategory` rows if categories or timestamps changed, and applies ± deltas to `ThreadCounts` (`Models/Thread.cpp:359-427`). `Message::afterRemove` subtracts and deletes the thread when its folder set is empty (`Models/Message.cpp:580-603`). The cost of this design shows in V11 and in the harness's `invariants.py` (D.10): incremental counters drift, so something must recompute them to check.

---

### D.3 The sync algorithm

#### Per-folder sync state (`Folder.localStatus`)

| Key | Meaning |
|---|---|
| `uidvalidity` | Last seen UIDVALIDITY |
| `uidnext` | UIDNEXT up to which new mail is known |
| `syncedMinUID` | Lowest UID the initial walk has reached (1 = done) |
| `highestmodseq` | CONDSTORE cursor |
| `lastShallow`, `lastDeep`, `lastCleanup` | When each scan last ran |
| `busy` | Drives the client's progress spinner |
| `bodiesPresent`, `bodiesWanted` | Body-cache progress |
| `messageCount`, `unseenCount`, `recentCount` | Last STATUS counts |
| `serverSavesSent` | The SMTP server has been seen filing its own Sent copy |

(`constants.h:34-54`). These keys are saved with `saveFolderStatus`, which merges **only the keys this pass changed** into a freshly loaded row. That stops one worker's stale snapshot from overwriting the other worker's progress (`MailStore.cpp:727-747`; the foreground copies the value rather than brace-initializing for exactly this reason, `SyncWorker.cpp:232-235`).

#### Intervals and limits

| Constant | Value | Where |
|---|---|---|
| Background sleep between passes | 120 s, wakeable | `main.cpp:225` |
| `SHALLOW_SCAN_INTERVAL` (newest 400 local UIDs, flags only) | 2 min | `SyncWorker.cpp:30, 687-701` |
| `DEEP_SCAN_INTERVAL` (whole folder, flags only) | 10 min | `SyncWorker.cpp:31, 703-720` |
| `CONDSTORE_GAP_SCAN_INTERVAL` (QRESYNC safety net) | 24 h | `SyncWorker.cpp:32-35, 618-642` |
| `CACHE_CLEANUP_INTERVAL` | 1 h | `SyncWorker.cpp:29, 744-752` |
| `MAX_FULL_HEADERS_REQUEST_SIZE` | 1024 messages per heavy fetch | `SyncWorker.cpp:50` |
| `MAX_SINGLE_RANGE_SWEEP_SIZE` | 5000 messages | `SyncWorker.cpp:51-56` |
| `MODSEQ_TRUNCATION_THRESHOLD` / `_UID_COUNT` | 4000 modseqs / newest 12,000 UIDs | `SyncWorker.cpp:57-58, 1402-1413` |
| `ORPHAN_SWEEP_MAX_WAIT` | 24 h (overridable from the environment) | `SyncWorker.cpp:36-48` |
| Bodies per folder per pass | 30, newest first | `SyncWorker.cpp:1567` |
| Body cache window | 3 months, plus drafts of any age | `SyncWorker.cpp:1496-1506, 1527-1536` |

#### 1. The folder list (every pass)

`syncFoldersAndLabels` runs at the start of every `syncNow()` pass (`SyncWorker.cpp:392, 836-1121`):

- **LIST "" "\*".** A `NO` to LIST is reported as a *connection* error, not "missing folder", because O365 answers `NO Server Unavailable. 15` during outages, and the old mapping aborted the process in a loop (`SyncWorker.cpp:859-864`, `dfdb083`).
- **Clean the list.** Drop `\Noselect` mailboxes, then de-duplicate repeated LIST entries by folder id, merging their flags. DavMail, Proton Bridge and Zoho list mailboxes twice, and the duplicate INSERT used to crash every launch (`SyncWorker.cpp:811-834`, `95829f2`).
- **Helper folder.** Create `Mailspring/Snoozed` if the account allows helper folders (`SyncWorker.cpp:872-920`; turned off for O365 shared mailboxes in `bf90a04`).
- **Gmail.** Only mailboxes flagged `\All`, `\Spam`/`\Junk` or `\Trash` become `Folder`s. Every other mailbox becomes a `Label`, which is never synced directly (`SyncWorker.cpp:960-964`).
- **Roles,** for `all, sent, drafts, spam, important, starred, archive, inbox, trash, snoozed` (`MailUtils.cpp:313-315`): first by SPECIAL-USE/XLIST flags (`MailUtils.cpp:325-359`), then by name. Name matching strips the namespace prefix, decodes modified UTF-7 and looks the result up in a localized table (`MailUtils.cpp:361-417`, `constants.h:346-413`). Before `dfdb083`, non-ASCII names never matched, and the table had Spanish Drafts mapped to trash and Czech Trash to sent.
- **Folders that disappeared** are detached first in bounded transactions: messages whose only copy was there are removed, the others lose that folder. Then the folder rows are deleted in one short write transaction. All diffing happens *outside* the transaction (`SyncWorker.cpp:925-1118`, `379ed85`).

#### 2. Pass structure (`syncNow`, background thread)

1. **Refresh the selected mailbox.** If the connection already has a mailbox selected, send `NOOP` first. Dovecot answers STATUS and FETCH on the *selected* mailbox from that connection's stale view until a NOOP, IDLE or SELECT (`SyncWorker.cpp:407-414, 1348-1361`; test README "Things learned from Dovecot").
2. **Order folders by role:** inbox, sent, drafts, all, archive, trash, spam, then the rest (`SyncWorker.cpp:419-424`). Then, for each folder:
   - **STATUS.** If it fails, skip the folder, clear `busy` and mark the folder "not covered" for the orphan sweep (`SyncWorker.cpp:429-438`).
   - **Missing UIDNEXT.** If STATUS reports UIDNEXT 0 or omits it (Coremail, hMailServer, home.pl), a change in MESSAGES, UNSEEN or RECENT triggers a deep scan (`SyncWorker.cpp:447-460`, `dfdb083`).
   - **First visit:** record UIDVALIDITY, set `highestmodseq` to the server's value *now*, and set `syncedMinUID = uidnext` (`SyncWorker.cpp:462-475`).
   - **Skip duplicate `\All` views.** On non-Gmail accounts an `\All` mailbox (ProtonMail Bridge's All Mail) is marked synced without being scanned. Gmail is excluded, because there All Mail is the primary store (`SyncWorker.cpp:477-507`, `b53e0db`).
   - **UIDVALIDITY changed:** see below.
   - **Initial-sync walk:** see below.
   - **Incremental sync:** see below.
   - **Bodies** for this folder, cache cleanup, `busy`, then `saveFolderStatus` (`SyncWorker.cpp:723-762`).
3. **Delete late duplicate Sent copies** (D.5) (`SyncWorker.cpp:765-769`).
4. **Orphan sweep** (`SyncWorker.cpp:771-775`).
5. **Return "more to do"** if any folder still has bodies to fetch, an unfinished walk or an unfinished deep scan. The background loop then runs another pass with no sleep (`main.cpp:206-209`).

#### 3. Initial sync: newest first, in chunks

- **Walk downwards from UIDNEXT.** The first chunk is 750 UIDs and later chunks are 1024. A folder with fewer than 5,000 messages is swept in **one** range down to UID 1, because a sparse UID space (UIDNEXT 200,000 with 2,000 messages) would otherwise cost about 200 empty round trips (`SyncWorker.cpp:556-598`).
- **Each chunk is a heavy fetch:** ENVELOPE, `BODY.PEEK[HEADER.FIELDS (References)]`, INTERNALDATE and FLAGS, plus X-GM-LABELS, X-GM-THRID and X-GM-MSGID on Gmail (`MailUtils.cpp:925-939`; mailcore `Vendor/mailcore2/src/core/imap/MCIMAPSession.cpp:2657-2735`). Light fetches ask only for FLAGS, plus X-GM-LABELS on Gmail.
- **Only what was ingested is recorded.** `syncFolderUIDRange` returns `truncated` and `syncedMinUID`, and `syncNow` advances the walk only to the lowest UID actually ingested (`SyncWorker.hpp:98-108`, `SyncWorker.cpp:579-587`). This is the fix for **`2076b05`**. Before it, chunks were 5,000 UIDs but full headers were capped at 1,024, so about 80% of each chunk was skipped. On CONDSTORE+QRESYNC servers nothing ever revisits old unchanged messages, so they stayed missing until UIDVALIDITY changed. Measured on Dovecot: 1,774 of 2,500 synced, and 60+ later passes never filled the gap.
- **Folders interleave.** Each pass does one chunk per folder, so all folders fill in together and each folder's new mail is handled in the same pass.
- **When the walk reaches UID 1**, the folder counts as fully covered: `lastDeep` is set and leftover UID-0 placements are deleted (`SyncWorker.cpp:589-597`).

#### 4. Incremental sync with CONDSTORE and QRESYNC

The background uses this branch only when **both** are present. iCloud's QRESYNC is switched off, because it returns malformed VANISHED responses and makes mail disappear (`SyncWorker.cpp:398-405`, `MailUtils.cpp:877-884`, `2ddaf57`).

`syncFolderChangesViaCondstore` (`SyncWorker.cpp:1363-1452`):

1. **Apply notifications already received.** First drain any VANISHED the connection has accumulated, *before* the early return and before `highestmodseq` moves (`SyncWorker.cpp:1379-1380`).
2. **Return early** if neither modseq nor UIDNEXT changed.
3. **`UID FETCH 1:<UIDNEXT-1> (…) (CHANGEDSINCE m VANISHED)`.** The upper bound is explicit rather than `*`, because Cyrus never reports expunges above the current highest UID (cyrus-imapd #6071) (`SyncWorker.cpp:1394-1400`). The foreground (`mustSyncAll=false`) limits the request to the newest 12,000 UIDs when the modseq gap is larger than 4,000, so it never blocks for minutes (`SyncWorker.cpp:1402-1413`).
4. **Apply the results.** Upsert every changed or added message. Delete VANISHED placements from the result *and* from the connection's accumulator, because libetpan passes on only the first VANISHED line (`SyncWorker.cpp:1432-1443`).
5. **Record cursors carefully.** Record UIDNEXT. Record the server's HIGHESTMODSEQ **only if the request was not limited** (`SyncWorker.cpp:1445-1451`). Otherwise the background pass would see "nothing to do" and the changes below the window would wait a day.
6. **Daily safety net.** Once the walk is complete, an attributes-only scan of the whole folder runs every 24 h. Messages never ingested have old modseqs, so `CHANGEDSINCE` will never mention them (`SyncWorker.cpp:614-642`, `2076b05`).

The VANISHED handling was rebuilt in **`eba3adc`**. A `* VANISHED 20:120` used to run `DELETE … WHERE remoteUID < 120`, because the `Query` builder keys clauses by column name, so `.gte().lt()` on one column kept only the second clause (`Query.cpp:25-71`). Archiving 56 messages in a 200-message inbox deleted 178 locally. The fix uses one `BETWEEN` clause with inclusive bounds (`MailUtils.cpp:490-497`). It also makes mailcore accumulate every VANISHED on every command (body fetch, STORE, MOVE), not only the first one after IDLE (`5a31251` began this).

#### 5. Incremental sync without QRESYNC

This includes CONDSTORE-only servers in the background (`SyncWorker.cpp:643-721`):

- **New mail:** if UIDNEXT grew, heavy-fetch `[localUidnext, remoteUidnext)`. For inbox or All Mail, also fetch about 31 bodies newest-first right away. `uidnext` advances only if the fetch was not truncated (`SyncWorker.cpp:658-685`).
- **Shallow scan** every 2 min: attributes-only over `[400th-newest local UID, UIDNEXT)` (`SyncWorker.cpp:687-701`).
- **Deep scan** every 10 min (or when counts change on a server without UIDNEXT): attributes-only over `[syncedMinUID, *]` (`SyncWorker.cpp:703-720`). If the deep scan truncated and the backlog is **still shrinking**, the folder stays busy and the next pass continues. If the count of messages needing headers stops falling (a message that can never be ingested while a task holds it), the engine backs off instead of looping hot (`SyncWorker.cpp:1123-1150`).

#### 6. One range scan (`syncFolderUIDRange`, `SyncWorker.cpp:1152-1346`)

1. **Fix up the range.** `location 0` becomes 1. A range wider than 1,024 UIDs is downgraded to a light fetch (`SyncWorker.cpp:1156-1165`).
2. **NOOP first** without QRESYNC when the folder is already selected, so moved-out copies are not resurrected from Dovecot's stale view (`SyncWorker.cpp:1184-1194`).
3. **Read local attributes before the network call:** UID → (unread, starred, draft, labels, messageId) from `MessageFolder` (`SyncWorker.cpp:1196-1202`, `MailStore.cpp:471-524`). The comment explains why: comparing old local state with old server data avoids treating freshly arrived rows as deletions.
4. **FETCH.** Honour RFC 9738 `MESSAGELIMIT`: Yahoo may answer only the highest UIDs, so UIDs below the lowest one returned are not treated as gone (`SyncWorker.cpp:1212-1224`).
5. **Compare per UID.** On a heavy fetch, also check that the row names the same message id. That catches a mis-paired COPYUID (`SyncWorker.cpp:1241-1254`). Every 250 ms of CPU, sleep 50 ms so other threads get the database (`SyncWorker.cpp:1230-1236`).
6. **Heavy-fetch what differs.** Light fetches collect the differing UIDs, keep the **highest** 1,024 (capped by MESSAGELIMIT) and report the rest as truncated. The remainder is therefore a contiguous block at the bottom of the range that a cursor can describe (`SyncWorker.cpp:1281-1328`).
7. **Delete the rest.** Local UIDs not returned (and at or above the MESSAGELIMIT floor) are deleted as vanished placements in chunks of 200 (`SyncWorker.cpp:1330-1343`).

**Ingest** (`MailProcessor.cpp:122-321`): `insertFallbackToUpdateMessage` hashes the id and, if the row exists and the placement matches, does nothing without opening a transaction. Otherwise it calls `updateMessage`, which reloads the message and placement **inside** `BEGIN IMMEDIATE`. This was a fix: both workers once applied one change's thread delta twice from stale loads (`a83cc9a`). Inside the transaction it:

- honours the task lock (`_sa` in the future means "ignore server data for already recorded copies", but a *new* copy is still recorded);
- upserts the placement and applies Gmail's one-placement rule;
- saves only if the client-visible JSON changed.

If there is no row, `insertMessage` threads the message and inserts it. A unique-constraint race with the other worker (`SQLITE_CONSTRAINT`, code 19) falls back to update.

#### 7. UIDVALIDITY changes

`SyncWorker.cpp:509-554`:

1. Set every placement in the folder to UID 0, silently, with no deltas (`MailStore.cpp:1213-1221`).
2. Run a light scan of `1:*` and heavy-fetch what it finds, which re-hashes headers to the same message ids and **relinks** the rows in place (`MailStore.cpp:956-1004`). Nothing is re-downloaded, and ids and metadata survive.
3. Only when the rebuild covered the whole folder, delete rows still at UID 0, except drafts (`MailStore.cpp:1223-1235`).
4. If the rebuild truncated, set `syncedMinUID` to the remainder so the initial walk finishes it, and keep the folder busy.

#### 8. How deletions are detected

| Path | Mechanism |
|---|---|
| QRESYNC servers | VANISHED from `CHANGEDSINCE`, from IDLE, and riding along on any other command (`SyncWorker.cpp:264, 1380, 1439-1443`) |
| Plain servers | Diff of the range scans (shallow: newest 400; deep: whole folder) (`SyncWorker.cpp:1330-1343`) |
| Safety net on QRESYNC servers | Daily full attributes scan (`SyncWorker.cpp:614-642`) |
| A message's last copy | Becomes an orphan, swept at the end of a later pass (below) |

**The orphan sweep** (`SyncWorker.cpp:356-390, 775`; `MailProcessor.cpp:695-723`) removes orphans recorded before `sweepBefore`:

- `sweepBefore` is the start of the current pass, *lowered* to the last full-coverage time of any folder this pass did not cover in full. A folder is not covered when its STATUS failed, its scan truncated or it is still in its initial walk.
- A folder never stalls the sweep for more than 24 h, unless its initial walk moved down this pass.
- Removal goes through `store->remove`, so `afterRemove` fixes the thread, body, metadata and orphan record.

Each of those rules is a fix for a real incident in `a83cc9a`'s history:

- A 4,500-message move by another client was seen 1,024 at a time at the destination. The sweep deleted the remaining 1,428 source orphans and re-inserted them without metadata.
- A folder unreadable for lack of the `r` ACL right disabled the sweep forever.

#### 9. Bodies and attachments

- **What gets a body:**
  - Messages in non-Spam/Trash folders dated within 3 months, plus drafts of any age.
  - Up to 30 per folder per pass, newest first (`SyncWorker.cpp:1500-1506, 1557-1635`).
  - New Inbox mail on plain servers, about 31 bodies immediately (`SyncWorker.cpp:671-684`).
  - The foreground also does the Inbox each cycle (`SyncWorker.cpp:291`).
  - Bodies the user opens arrive as `need-bodies` and are fetched first in the next foreground cycle (`SyncWorker.cpp:113-152`).
- **Reservation:** before fetching, a `MessageBody` row with NULL value is inserted inside a transaction, so the other worker skips the message and a message that always fails is tried only once (`SyncWorker.cpp:1590-1618`).
- **Query shape matters:** body candidates come from `MessageListDateIndex(accountId, date DESC)` with a correlated "has a copy in this folder" check: 30-70 ms instead of 0.8-1.3 s on a 180k-message folder (`SyncWorker.cpp:1519-1536`).
- **Which copy to fetch from:** the preferred (selected) folder, then normal folders, then Spam/Trash. A copy refused with `ErrorFetch` moves on to the next one (`MailStore.cpp:829-861`, `SyncWorker.cpp:1637-1686`).
- **What is fetched:** the **whole message** (`BODY.PEEK[]`; mailcore `MCIMAPSession.cpp:2966-2974`), parsed with mailcore's `MessageParser`. **Every attachment is written to disk** under `CONFIG_DIR/files/xx/yy/<fileId>/<name>`, and `File` rows are created. CID references without a disposition are promoted to inline (`MailProcessor.cpp:384-563`). `List-Unsubscribe` and importance headers are extracted from the full header at this point (`MailProcessor.cpp:510-545`, `0e7daff`).
- **Reload after the fetch:** the message is reloaded inside the save transaction, because a stale object would overwrite placement changes made during the fetch (`MailProcessor.cpp:464-476`, `a83cc9a`).
- **Cleanup:** hourly per folder, bodies older than 3 months that were fetched more than 14 days ago are deleted. Their FTS text and attachment files are **not** removed (a TODO, `SyncWorker.cpp:1471-1492`).
- **Mail rules:** a message becomes `rulesReady` once it has a stored body *and* an "incoming" copy (outside Sent/Drafts/Spam/Trash; on Gmail `\Inbox` or neither `\Sent` nor `\Draft`). The delta that flips it carries the body (`Models/Message.cpp:367-398, 525-557`, `eb246b6`). The client runs mail rules only on that delta.

#### 10. Gmail specifics

- **Only All Mail, Spam and Trash are folders;** every other mailbox, including INBOX and Sent, is a `Label` (`SyncWorker.cpp:960-964`; CLAUDE.md "Gmail-Specific Behavior"). Message ids still come from headers, X-GM-MSGID is stored as `gMsgId`, and X-GM-THRID decides the thread (`MailProcessor.cpp:173-175`).
- **Labels:** X-GM-LABELS are stored per placement, sorted, with `\Trash` and `\Spam` dropped because those are folders (`MailStore.cpp:50-80`). A label name like `\Inbox` is mapped to the `INBOX` label row (`MailUtils.cpp:435-467`). A Gmail "Inbox" view is the set of threads whose `ThreadCategory` includes the INBOX label.
- **Detection** is by the `X-GM-EXT-1` capability, not the provider setting, so Gmail added as generic IMAP still works (`MailProcessor.cpp:90-94`).
- **Task paths** use `X-GM-LABELS` STOREs for labels (`TaskProcessor.cpp:662-692`), and the send path handles All Mail (D.5).

---

### D.4 IDLE and the foreground worker

`idleCycleIteration` runs in a loop forever (`SyncWorker.cpp:108-319`):

1. **Fetch bodies the client asked for.** Reconnect first if needed, and prefer the Inbox copy because it is the folder already selected (`SyncWorker.cpp:113-152`, `4eb3533`).
2. **Connect and log in**, then run every task with `status = "remote"`, in rowid order. It loops so that tasks created by tasks also run (a send queues metadata tasks) (`SyncWorker.cpp:159-214`).
3. **Pick the folder:** the Inbox, or All Mail if there is no Inbox (`SyncWorker.cpp:221-231`).
4. **Sync it.** If the last IDLE ended in an error, re-SELECT first: a dropped connection's VANISHED are lost, and the re-select plus `CHANGEDSINCE` re-reports them as `VANISHED (EARLIER)`. A dropped IDLE used to segfault the next cycle (`SyncWorker.cpp:251-259`, `a83cc9a`). Drain the accumulated VANISHED (`SyncWorker.cpp:264`). Then run CONDSTORE (`mustSyncAll=false`) or a range scan of the newest 100 UIDs, then bodies (`SyncWorker.cpp:266-293`).
5. **IDLE.** mailcore's IDLE ends after 28 minutes (`MAX_IDLE_DELAY`, mailcore `MCIMAPSession.cpp:115, 3833`), on any untagged response, or on `interruptIdle()`. IDLE errors are not thrown, because "Yandex (maybe others) abruptly and randomly close IDLE connections - and that's ok". A failed *reconnect* on the next cycle is what reports the problem (`SyncWorker.cpp:302-313`).
6. **No IDLE support?** Then block on a condition variable until a task or wake arrives (`SyncWorker.cpp:314-318`).

**How it interacts with the background worker:**

- The two workers use separate IMAP connections and separate SQLite connections. The only coordination is the database: `BEGIN IMMEDIATE` and reloads inside transactions, the message-level task lock, body reservations and key-wise `saveFolderStatus`.
- Without CONDSTORE, the foreground's Inbox update never advances `uidnext` past a truncated fetch, because the background reads that value to find new mail (`SyncWorker.cpp:284-288`).
- Both workers' orphans share one clock: the background pass start time (`SyncWorker.cpp:356-360`).

**Waking from sleep.** The client sends `wake-workers` on OS resume or unlock and when it comes back online. The engine marks folders busy, wakes the 120 s sleeps and interrupts IDLE, because "our network connection may have been reset and it'll sit for a while otherwise" (`main.cpp:810-824`).

---

### D.5 Tasks: local-first mutations

#### Lifecycle

1. **The client sends `queue-task`** with `status: "local"` (`client:app/src/flux/mailsync-bridge.ts:391-416`).
2. **`performLocal`** (main thread, `TaskProcessor.cpp:771-894`):
   - It saves the task first, so a crash leaves a `local` row, which is deleted at the next launch.
   - It dispatches on `__cls` and sets `status = "remote"`.
   - Failures and their outcome:

     | Failure | Outcome |
     |---|---|
     | `SyncException` | Status `complete` with `error` (`{key, debuginfo, retryable, offline}`) |
     | `std::exception` (e.g. `json::type_error`) | Same, with the message scrubbed to printable ASCII so serializing the error cannot itself throw |
     | `SQLite::Exception` | **Rethrown, leaving the task as it was.** Database errors are usually transient and say nothing about whether the task can run |

   - The second row is a fix: a task with a `null` field used to abort the engine on every launch (`203637b`, `TaskProcessor.cpp:694-716`).
   - The third row only keeps the task in the remote phase. In the local phase, the rethrown error reaches the main thread's `catch (...) { abort(); }` (`main.cpp:863-866`). The row is still `local`, so the relaunch deletes it (`TaskProcessor.cpp:725-735`): a database error during the local phase loses the task.
3. **`performRemote`** (foreground, `TaskProcessor.cpp:898-1022`):
   - It checks `should_cancel`, dispatches, and ends at `complete` or `cancelled`.
   - There is **no automatic retry**. A network error mid-command completes the task with `error.retryable = true`, and the client runs `task.onError`.
   - What *does* wait is a task queued while offline: the foreground only reaches the task loop after `connectIfNeeded`/`loginIfNeeded` succeed (`SyncWorker.cpp:159-214`). So an offline queue exists, without a retry policy.
   - A database error escapes, aborts the process (`main.cpp:161-164`) and leaves the row `remote`, so the task runs again after the client relaunches the engine.
4. **The client learns the result from the Task delta** (`client:app/src/flux/mailsync-bridge.ts:503-517`).

#### The `syncedAt` lock

The local phase of a message task sets `_sa = now + 24h` and increments `_suc` on every message it touches (`TaskProcessor.cpp:1135-1138`). While `_sa` is in the future, `updateMessage` ignores server data for **copies already recorded**, so a scan cannot undo a flag or bring back a copy that is being moved (`MailProcessor.cpp:271-276`). A copy at a (folder, UID) the message has no row for is still recorded. Another client may have moved the message's only copy, and skipping it would hand the message to the orphan sweep (`remote-move-while-task-in-flight` scenario, `a83cc9a`). Each remote phase decrements `_suc` whether it succeeded or failed, and resets `_sa` to now when it reaches 0 (`TaskProcessor.cpp:1260-1264`). Before `a83cc9a`, a failed remote phase leaked the lock, and the message ignored the server for 24 h.

#### Task types

| Task | Local phase | Remote phase |
|---|---|---|
| `ChangeUnreadTask` / `ChangeStarredTask` | Sets the flag on **every** placement (`MailStore.cpp:1021-1034`) | `UID STORE ±FLAGS` once per folder that holds a copy. On failure, `lastDeep = 0` on those folders forces a full flag rescan, because on CONDSTORE servers an unchanged modseq would never be reported again (`TaskProcessor.cpp:101-121, 407-437, 1281-1285`) |
| `ChangeLabelsTask` (Gmail) | Edits the placement's X-GM-LABELS, **sorted** to match the scan, which would otherwise see a fake change on every pass (`TaskProcessor.cpp:637-660`) | `STORE ±X-GM-LABELS` (`TaskProcessor.cpp:662-692`) |
| `ChangeFolderTask` | Chooses copies (`_movesForMessage`), sets `pendingFolderId`, and records `undoPlacements {messageId: [{folderId, bits}]}` on the task (`TaskProcessor.cpp:518-594`) | Per source folder: `UID MOVE`, or COPY + `\Deleted` + UID EXPUNGE; resolves the new UIDs; commits each row (`TaskProcessor.cpp:249-302, 607-623, 1190-1318`) |
| `SyncbackDraftTask` | Upserts the draft; a new draft gets a Drafts placement at UID 0 (`TaskProcessor.cpp:1320-1369`) | **None.** Drafts never go to the server ("right now we don't syncback drafts", `TaskProcessor.cpp:922-923`) |
| `DestroyDraftTask` | Removes the draft and creates a hidden `deleted-*` placeholder that takes over its server UID and shows under Trash, so the Drafts scan does not bring it back (`Models/Message.cpp:30-60`, `TaskProcessor.cpp:1371-1427`) | Deletes the server copies, then the placeholder (`TaskProcessor.cpp:1429-1444`) |
| `SendDraftTask` | — | See below |
| `SyncbackCategoryTask` | — | `CREATE` or `RENAME` with namespace and delimiter fix-ups; saves a `Folder` (or a `Label` on Gmail) (`TaskProcessor.cpp:1887-1941`) |
| `DestroyCategoryTask` | — | `DELETE` the mailbox; the next folder-list sync detaches its messages (`TaskProcessor.cpp:1993-2006`) |
| `ExpungeAllInFolderTask` | — | `STORE 1:* +\Deleted`, `EXPUNGE`, then detach placements in chunks with a 300 ms pause so the UI keeps up (`TaskProcessor.cpp:2568-2591`) |
| `GetMessageRFC2822Task` / `GetManyRFC2822Task` | — | Exports `.eml` files. The bulk export pages by UID cursor, 50 at a time, persists its progress, and checks cancel inside one write transaction so a cancel is never lost (`TaskProcessor.cpp:2593-2889`, `5558bed`) |
| `EventRSVPTask` | — | Validates the ICS, builds an iTIP REPLY or COUNTER with wrapped base64, sends it over SMTP (`TaskProcessor.cpp:2911-…`, `7516f24`, `d0c7085`) |
| `SyncbackMetadataTask` | Upserts versioned plugin metadata on the model (`TaskProcessor.cpp:1944-1966`) | POSTs to the identity server (`TaskProcessor.cpp:1969-1991`) |
| `ChangeRoleMappingTask` | Moves a role to another folder | — |
| Contact, ContactGroup and Event tasks | Local save or hide | CardDAV, CalDAV or Google People calls |

#### Move details

- **Which copies move** (`TaskProcessor.cpp:518-578`). Trash and Spam take every copy. Otherwise the copies in `sourceFolderIds` (the folder the user was looking at). Without a source list, every copy outside Sent and Drafts. A copy is moved even if the destination already holds one, so two copies in one folder are simply two placements.
- **The remote phase re-derives the selection from the current rows.** It therefore finds the same copies after the local phase, and still works after an earlier task moved them.
- **Without MOVE:** COPY, `STORE +\Deleted`, then `UID EXPUNGE` of exactly those UIDs when UIDPLUS is available. Plain `EXPUNGE` is used only if UIDPLUS is missing or UID EXPUNGE is refused. After a dropped connection, nothing is expunged at all, leaving the copies `\Deleted` (`TaskProcessor.cpp:218-247`, `dfdb083`). The old plain EXPUNGE erased messages that another client (Outlook's "mark for deletion" mode) had only flagged. mailcore never even recorded the UIDPLUS capability until that fix.
- **New UIDs.** COPYUID is trusted only for a single message. Yahoo answers a multi-message UID MOVE with ascending ranges but assigns a *permutation*. So the destination headers (or, without COPYUID, the newest `2 × n` UIDs) are fetched and matched by message id (`TaskProcessor.cpp:123-216`).
- **Commit.** `confirmPlacementChange` rewrites the row to (destination, new UID), handling three cases: a row the destination scan already recorded, a stale row of another message at that UID, and a "move" onto the copy's own folder (`MailStore.cpp:1055-1127`). A pending marker added later by an undo is carried over (`TaskProcessor.cpp:1289-1318`).
- **Partial failure.** If one folder fails, copies already moved are committed, markers on the rest are abandoned so they show where the server has them, the lock is released, and then the error is rethrown (`TaskProcessor.cpp:1216-1287`).
- **Undo is just another `ChangeFolderTask`.** It carries `restorePlacements` (the recorded `undoPlacements`) and moves copies back one per recorded folder. It prefers copies whose flag bits match, then copies still in flight, then the highest UIDs (`TaskProcessor.cpp:439-516`; client side in `client:docs/undo-redo-task-pattern.md`).

#### `SendDraftTask` (`TaskProcessor.cpp:2085-2517`)

1. **Run at most once.** `_performRemoteRan` is written before anything else, and retries are always new tasks (`TaskProcessor.cpp:2089-2095`).
2. **Build and send.** Build MIME with mailcore. `References` and `In-Reply-To` hold only the replied-to Message-ID (`TaskProcessor.cpp:2148-2156`). Send over SMTP. "Multisend" (per-recipient tracked bodies) is one SMTP submission per recipient, and a partial failure reports who already got it (`TaskProcessor.cpp:2230-2272`).
3. **Delete the draft's server copies** (`TaskProcessor.cpp:2278`).
4. **Find the server's own Sent copy.** Re-SELECT Sent and search the last 15 messages by Message-ID, retrying after 0/1/1/2 s, or 0/1/1/2/2/3/3 s once the folder is known to get server copies (`serverSavesSent`). Exchange Online filed its copy 0.5-3.5 s after the SMTP 250, sometimes 6-8 s, and once about 3 minutes (`TaskProcessor.cpp:2284-2312`, `21ab5fa`). The search re-SELECTs because mailcore's cached message count was stale on an already selected folder, so every retry missed the copy (`Vendor/mailcore2/src/core/imap/MCIMAPSession.cpp:2129-2158`, `a83cc9a`).
5. **Multisend cleanup:** delete every per-recipient copy from Sent, and on Gmail again from All Mail (`TaskProcessor.cpp:2314-2330`).
6. **No server copy found? APPEND one** with `\Seen`. On Gmail, copy the thread's user labels onto it so the thread does not leave its labels (`TaskProcessor.cpp:2346-2381`).
7. **Record the sent message locally.** Fetch it back (on Gmail from All Mail if it already shows there), insert it through the normal ingest path, attach the body from the MIME just built, and queue metadata tasks (`TaskProcessor.cpp:2390-2516`).
8. **Clean up late duplicates.** If the engine APPENDed, it records `_asc = {folder, uid, expires}`. For 30 minutes the background pass removes any *other* copy of that message from Sent, the late server copy, and keeps its own (`TaskProcessor.cpp:2023-2083, 2471-2482`, `SyncWorker.cpp:765-769`). It never deletes a copy it did not append beside.

---

### D.6 Threading

Threads are assigned once, when a message is first inserted (`MailProcessor.cpp:158-237`):

1. **Gmail:** find the `Thread` with the same `gThrId` (X-GM-THRID) (`MailProcessor.cpp:173-175`).
2. **Otherwise,** unless the Message-ID was auto-generated: `SELECT Thread … JOIN ThreadReference WHERE headerMessageId IN (own Message-ID, first 50 References) LIMIT 1` (`MailProcessor.cpp:177-197`).
3. **No match:** create a new thread `t:<messageId>` with the message's subject (`MailProcessor.cpp:199-202`).
4. **Index the new message.** Insert `ThreadReference` rows for the message's own Message-ID, the **first** reference (the root) and the **last 99** references, so long threads stay connected through both the root and recent replies (`MailProcessor.cpp:786-821`, `bf12deb`).

What it does **not** do:

- It never compares subjects.
- In-Reply-To is stored (`rthMsgId`, `Models/Message.cpp:82-87`) but not used for matching, so a reply with In-Reply-To and no References starts a new thread.
- Threads are never merged or split later. A message that references two existing threads joins whichever the query returns first.
- `ThreadReference` rows are not deleted when a thread is (`Models/Thread.cpp:429-443`). They are harmless, because the lookup joins `Thread`, but they accumulate.

Because the walk goes newest-first, children usually arrive before their parent. The parent still joins, because the children indexed their References, which contain the parent's Message-ID.

**When messages move or are deleted**, only counters change, through the snapshot diff in D.2:

- A move moves the thread's folder `_refs` and `_u`.
- An orphaned message drops out of every folder view but keeps its thread membership.
- When the last message is removed, the thread is removed with its `ThreadCategory`, `ThreadCounts` and `ThreadSearch` rows (`Models/Message.cpp:580-603`, `Models/Thread.cpp:429-443`).
- A message's thread never changes after insert.

---

### D.7 Search

- **The FTS5 tables:**
  - `ThreadSearch`: one row per **thread**, with columns `subject`, `to_`, `from_`, `categories`, `body`.
  - `ContactSearch` and `EventSearch`.
  - All three use `tokenize = 'porter unicode61'` (`constants.h:88, 160, 198`).
- **When a thread is indexed:**
  - When a message is inserted: the subject (set once at thread creation), plus To/Cc/Bcc and From names and emails appended without duplicates, plus categories (`MailProcessor.cpp:220-222, 725-784`).
  - When a body is fetched: the first 5,000 characters of its plain text are appended to the thread's `body` (`MailProcessor.cpp:497-501, 761-763`).
  - When the thread's folders or labels change: `categories` is rewritten (`Models/Thread.cpp:419-425`).
  - When the thread is removed: its row is deleted.
- **Gaps:**
  - Body text is never removed when a message leaves the thread or its body is purged (`SyncWorker.cpp:1487`).
  - A body fetched twice is appended twice.
  - Only messages whose bodies are cached (the 3-month window) have searchable bodies.
  - The `Thread.isSearchIndexed` column and its two indexes (`constants.h:125, 135-136`) are no longer read or written by the engine.
- **How the client queries it:** read-only, `Thread.id IN (SELECT content_id FROM ThreadSearch WHERE ThreadSearch MATCH …)` (`client:app/src/services/search/search-query-backend-local.ts:269-274`, `client:app/src/flux/attributes/matcher.ts:362-363`). Search is local only; there is no IMAP SEARCH task.

---

### D.8 Storage and performance

- **Connection settings** (`MailStore.cpp:89-114`):
  - One connection per thread per process, all on `edgehill.db`.
  - `PRAGMA journal_mode = WAL`, `synchronous = NORMAL`, `page_size = 4096`, `cache_size = 10000`.
  - **`busy_timeout = 60 s`**, raised from 10 s "to wait rather than crash+retry during many account sync" (`63ab613`).
  - Transactions are `BEGIN IMMEDIATE` (`MailStore.cpp:91`), so the write lock is taken up front and there are no deadlocks from upgrading a read lock.
- **Transactions report slowness.** The `MailStoreTransaction` RAII wrapper rolls back in its destructor and logs `[BUSY] Transaction=X waited Nms` (more than 1 s to get the lock) and `[SLOW] … held write lock for Nms` (more than 100 ms) (`MailStoreTransaction.cpp:17-64`, `60c8098`). That greppable log is how the team found the next item.
- **Lock time is kept short.**
  - Parse and diff outside transactions, then write in a short one (`379ed85`, `b2b1cef`; `SyncWorker.cpp:925-1071`).
  - Open no transaction for an unchanged copy (`MailProcessor.cpp:131-143`).
  - Don't write the thread twice on insert (`de3cab0`, `MailProcessor.cpp:212-218`).
  - Chunk sizes: 100 messages per refresh transaction, 200 UIDs per delete, 500 ids per `IN` list, 900 per `findLargeSet` (`CLAUDE.md` "Database access"; `MailProcessor.cpp:610-635`).
  - Yield 50 ms every 250 ms of CPU in scan loops (`SyncWorker.cpp:1230-1236`).
- **Statements are compiled once.** Insert/update/remove/placement statements are cached per `MailStore`, and the caches are dropped wholesale on rollback (`MailStore.cpp:611-633, 777-797`). The README explains why: "think of [a statement] as building a dynamic function. Build them infrequently and re-use them".
- **Deltas are kept down.**
  - `unsafeEraseTransactionDeltas()` drops deltas for engine-internal writes: contact refcounts, `_asc`, remote-phase confirms that changed nothing visible (`MailStore.cpp:635-642`, `TaskProcessor.cpp:1275-1277`).
  - `saveFolderStatus` skips a save when nothing changed, "because this creates a lot of noise in the client" (`SyncWorker.cpp:759-761`).
- **VACUUM** runs every 30 days in `--mode migrate` and after an account reset (`MailStore.cpp:273-295, 452-464`).
- **Indexes that matter:**
  - `MessageFolderUIDIndex` (partial unique, `remoteUID > 0`). Every range read adds `AND remoteUID > 0` so the planner can use it (`MailStore.cpp:471-473, 1215`).
  - `MessageFolderMessageIndex(messageId)`.
  - `MessageOrphanSinceIndex(accountId, since)`.
  - `MessageListDateIndex(accountId, date DESC)` and the partial `MessageListDraftIndex`, which the draft body query names with `INDEXED BY` because the planner otherwise picks the date index (`SyncWorker.cpp:1527-1536`).
  - The covering `ThreadListCategoryIndex(lmrt DESC, value, inAllMail, unread, id)` for folder lists.
  - Partial `ThreadUnreadIndex` and `ThreadStarredIndex`.
  - `TaskByStatus(accountId, status)` (`constants.h:129-212, 335-344`).
- **The cost of one file for every account and the client:** startup staggering, the 60 s busy timeout and the effort to minimize lock time exist because N engine processes and the UI all hit one SQLite file.

---

### D.9 Error handling and resilience

- **One exception type.** `SyncException{key, debuginfo, retryable, offline}`. mailcore's `ErrorConnection` is retryable and offline. `ErrorParse` and `ErrorFetch` are retryable ("caused by abrupt connection termination"). Every other mailcore error, including authentication, certificate and TLS errors, is **fatal** (`SyncException.cpp:45-63`). curl network errors are retryable and offline (`SyncException.cpp:21-43`). HTTP 401, 403 and `invalid_grant` are fatal; other HTTP errors are retryable (`NetworkRequestUtils.cpp:177-203`).
- **Fatal errors** abort the process. The client relaunches it and, after 5 crashes in 5 minutes, parks the account as auth-failed or errored (D.1). Retryable errors set `connectionError`, sleep up to 120 s, and retry.
- **OAuth:** access tokens are cached per account with a 60 s safety margin, under a mutex shared by the threads. A refusal from the provider (an `error` field) is fatal. Anything else that is not a token, such as a captive portal's HTML, is retryable and offline (`XOAuth2TokenManager.cpp:90-144`, `0f1255b`). A rotated refresh token is sent to the client as `ProcessAccountSecretsUpdated`.
- **Isolation between folders is partial.**
  - A folder whose STATUS fails is skipped (`SyncWorker.cpp:429-438`), and a failing gap scan is caught (`SyncWorker.cpp:622-641`).
  - But any other `SyncException` in one folder's scan escapes `syncNow`. The rest of that pass is skipped and the pass is retried after the 120 s sleep. A folder that fails every pass therefore blocks the folders after it in role order.
- **Calendar and contacts never kill the process.** A 403 or 401 stops that thread, a quota error sleeps 4 h, other non-retryable errors sleep 90 min (`main.cpp:265-295`).
- **Server quirks are handled where they occur**, each with a citation in a comment: Yandex XLIST (`9969b97`), Coremail `ID` before SELECT (`MailUtils.cpp:849-856`), iCloud QRESYNC, O365 LIST `NO`, duplicate LIST entries, UIDNEXT 0, MESSAGELIMIT, Yahoo permuted COPYUID, Cyrus VANISHED clipped at `*`, and an obsolete-TLS fallback with advice text (`3ca1cfa`, `MailUtils.cpp:789-834`). Certificate checks stay on even with "allow insecure SSL" (`f7b0239`).
- **Logging:**
  - A rotating file `mailsync-<accountId>.log`, 5 MB × 3, with thread names. Only `critical` goes to stderr, which the client reports when the process dies (`main.cpp:999-1038`).
  - IMAP and SMTP traffic is logged only with `--verbose` (a 30-minute opt-in in the client UI, `client:app/src/flux/mailsync-bridge.ts:152-174`).
  - Lines mailcore tags `ConnectionLogTypeSentPrivate` (AUTH commands and tokens) are never written, even in test mode (`MailUtils.cpp:750-753`, `main.cpp:85-90`, `3ba5381`).
  - vCard and ICS payloads are reduced to byte counts unless verbose (`1ab7668`).
  - OAuth error bodies are redacted and truncated (`XOAuth2TokenManager.cpp:58-77`).
  - The client strips known secrets again before showing logs (`client:app/src/mailsync-process.ts:232-264`).

---

### D.10 Testing: the fakeimap, Cyrus and Dovecot suite

#### How it is structured (`test/README.md`)

- **Black box.** Each test runs the **real `mailsync` binary** exactly as the client does: account JSON on stdin, `--mode migrate` before each launch, tasks and `wake-workers` on stdin. No C++ is compiled for the tests.
- **`test/harness/`** drives the process (`mailsync.py`) and runs YAML scenarios (`scenario.py`). It also holds the **oracle** (`db.py`), which turns the engine's schema into `{folder: {uid: placement}}` while `Server.truth()` reads the same shape back over IMAP. `db.py` is the only code that knows the schema, so the scenarios survived the placements refactor unchanged.
- **Quiescence is inferred**, because the engine has no "done" signal. The engine is idle when every folder is `busy=false` with `syncedMinUID<=1`, the background's last log line is "Sync loop complete.", the foreground's last command is IDLE, and no delta has arrived within a settle window.
- **`test/scenarios/*.yaml`** has about 65 scenarios. Each declares servers, initial mailboxes (generated messages are unique unless a scenario asks for duplicates, because identical generated Message-IDs once produced fake bugs), steps and expectations.
  - **Steps** include server changes made "by another client" (`expunge`, `move`, `flags`, `set_uidvalidity`, `drop_connections`, `pause`). They can be pinned to a protocol moment, one connection (`session: foreground|background`) or one command regex. Other steps: `server.reject` (answer a command `NO [OVERQUOTA]`), `client.task` and `client.undo_task`, `sync: pass`, `force_scans`, `restart` onto another binary (for migrations), and `db.sql`.
  - **Expectations** include `db_matches_server`, `counts`, `shown` (what the UI shows, pending moves included), `stable: {passes: N}` (a flapping detector: N more passes must not move a single placement), `deltas`, `log_present`/`log_count`, `rules_ready`, `folder_roles` and `connection_error`.
- **Invariants run after every scenario** (`harness/invariants.py`). They recompute each derived layer from the stored layer below it: message snapshot and flags from `MessageFolder`, thread `_refs`/`_u` from the snapshots, `ThreadCategory` from the threads, `ThreadCounts` from `ThreadCategory`, and `MessageOrphan` from messages with no row. They found the "both workers double-apply a thread delta" bug on their first day.
- **`test/fakeimap/`** is a stateful IMAP4rev1 server in Python. It supports CONDSTORE, QRESYNC, IDLE, UIDPLUS, MOVE, ID, SPECIAL-USE and the X-GM-* extensions, and includes a small SMTP server that can file Sent copies late. Notifications are per session; an expunge is told once.
  - **Personalities.** The default `dovecot` personality is **proved against real Dovecot 2.3.21** by `test/conformance/` (9 probe groups, 56 response comparisons; a difference fails unless it is allow-listed with a reason). Every deviation is a named quirk with a citation: `proton-bridge`, `gateway-duplicate-list`, `netease-coremail`, `courier`, `outlook`, `icloud`, `gmail` (All Mail views with per-view UIDs), `yahoo` (permuted COPYUID), `yahoo-messagelimit`, `cyrus-vanished`, `hmailserver` (UIDNEXT 0), `inbox-only`, `plain`. Reconstructed wire formats are marked `NEEDS-RECORDING`, and `tools/record_personality.py` captures real ones.
- **`test/servers/`** has Docker images for Dovecot 2.3.21 (Alpine) and **Cyrus 3.6** (Debian; the server Fastmail runs, with Fastmail's namespace and delayed expunge). Profiles strip capabilities (`plain`, `no-move`) or reshape folders (`proton-like`, `default-ns`).
- **Tools.** `tools/ab.py` records the outcomes of two binaries and diffs them; this was the refactor's regression gate ("0 regressions and the 9 placements cases fixed"). `tools/cyrus_server.py` runs a persistent Cyrus to point the real client at.
- **Scale and results.** The full suite takes about 6 minutes with 16 workers, and the slowest case about 140 s. 171 cases passed at `21ab5fa`. The suite found five engine bugs on 2026-09-19 and 20, all fixed within two days, plus a sixth from the invariants on 2026-09-22.

#### What it covers

Use the "Scenarios" table in `test/README.md` as a checklist. The areas it covers:

- initial sync over large and sparse UID spaces;
- every VANISHED path (wide ranges, two lines, during IDLE, during a body fetch);
- deep-scan deletion;
- remote moves in both scan orders;
- placements: self-addressed mail, duplicate Sent copies, flags on a second copy, moving into a folder that already has a copy;
- UIDVALIDITY changes (including a 2,500-message rebuild);
- tasks: move, trash, expunge, undo before the remote phase, a move rejected by the server, a remote move while a task is in flight;
- connections dropped in IDLE and mid-command;
- send and Sent-copy timing (late, multisend, a refused STORE);
- Gmail labels and send;
- migrations from older binaries;
- mail-rules readiness;
- every provider quirk listed above.

#### A proposal for Alice

Alice already has the base layer:

- a disposable Dovecot + Postfix container with localized SPECIAL-USE names (`alice:scripts/mail-fixture.sh`);
- an end-to-end script that drives the real daemon over HTTP and checks server state with `imaplib` (`alice:scripts/test-mail.py`);
- a CI job that runs both (`alice:.github/workflows/ci.yml:196-210`);
- mock-stream unit tests (`alice:crates/alice-core/src/remote.rs:590-675`).

Grow it in this order:

1. **Turn `test-mail.py` into a scenario runner.** One function or YAML file per scenario: seed the server, start the daemon, mutate the server as "another client" with `imaplib`, call `POST /v1/sync`, assert. Add the three checks that give Mailspring's suite its leverage:
   - **An oracle:** every (mailbox, UIDVALIDITY, UID) on the server ↔ a `remote_messages` row with the same Message-ID and flags, and nothing extra (`alice:crates/alice-core/src/sync_store.rs:24`).
   - **`stable`:** two more syncs leave `remote_messages` and `messages` byte-identical.
   - **Invariants recomputed from canonical rows:** each message's `folder` equals its best-ranked copy (`sync_store.rs:109-118`) and its flags come from that copy; every non-draft, non-local-Sent message has at least one copy; `message_index` and `message_bodies` match `messages` (`alice:crates/alice-core/src/store.rs:1147-1196`).
2. **Add Cyrus** next to Dovecot, as a second real server with different QRESYNC, namespace and expunge behaviour. Mailspring's `test/servers/cyrus/` (Debian bookworm `cyrus-imapd`, about 3 s per container) is a working recipe. Add Dovecot profiles that strip `MOVE`, or `CONDSTORE QRESYNC`, through `imap_capability`.
3. **Write a small fake server only for what real servers can't be made to do:** X-GM-* views, a permuted COPYUID, MESSAGELIMIT, UIDNEXT 0, `NO` to LIST, a connection dropped or a reply held at a chosen command. Python asyncio fits next to the existing Python tests. Port the *ideas* (personalities with citations, conformance probes against Dovecot so the fake cannot drift into "must support" bugs) rather than Mailspring's GPLv3 Python, which avoids any licensing question for an AGPL-3.0-only repo. GreenMail (Java, Docker) is convenient for SMTP+IMAP smoke tests, but its extension set is smaller. Confirm CONDSTORE and QRESYNC support before relying on it for sync tests; Dovecot and Cyrus are what users actually connect to.
4. **First scenarios to port**, each matching an Alice code path: `remote-move-to-archive` and `remote-move-destination-scanned-first` (D.11 item 3), `uidvalidity-change` (item 6), `self-addressed-inbox-and-sent` and `o365-duplicate-sent-copies` (`sync_store.rs:136-181`), `two-folders-identical-messages`, `status-uidnext-zero-new-mail` (item 9), `connection-dropped-during-command` (`remote.rs:52-98`), `move-without-move-keeps-other-deleted` (item 12), `sent-copy-saved-late` (item 5), `gmail-labels`, and `stable` over a 5,000-message mailbox.

---

### D.11 Lessons for Alice

Alice today, for comparison:

- **One daemon process** holds all accounts and **one SQLite connection** behind a mutex (`alice:crates/alice-core/src/store.rs:9-31`).
- **Sync runs accounts one after another**, every 60 s, or 15 s with OTP copy enabled. There is no IDLE (`alice:crates/alice-core/src/sync.rs:32-41, 222-241`).
- **Each run opens a fresh connection** and, for each of five special-use mailboxes (`remote.rs:503-583`), does EXAMINE, then CONDSTORE `CHANGEDSINCE` flags or a 10-minute full flag read, then `UID SEARCH SINCE <window>` for membership. It downloads headers and text parts (not attachments) in batches of 50 (`sync.rs:347-576`, `remote.rs:114-236`).
- **Copies live in `remote_messages`**, one row per (mailbox, UIDVALIDITY, UID) with per-copy flags. The message's single `folder` and flags come from its **best-ranked copy** (spam > inbox > sent > archive > trash) (`sync_store.rs:3-12, 108-118, 192-273`).
- **Message ids** are UUIDv5 of the account and Message-ID, with Sent copies in their own namespace (`providers.rs:424-441`).
- **User actions run online and synchronously:** a new IMAP session per change, STORE on the primary copy only, and MOVE required (`alice:crates/alice-core/src/daemon.rs:410-458`, `providers.rs:78-172`; "an offline mutation queue is not implemented", `alice:README.md:296`).
- **Conversations are grouped by normalized subject** at query time (`store.rs:139-292, 1262`).

Many items below also appear in this report's sync section (`research/sync.md`). Several depend on its recommended move from `imap` 2.4 to `async-imap`, which is needed for VANISHED, X-GM-* and an IDLE that can be interrupted.

#### Adopt, in priority order

1. **A persisted task queue with local and remote phases.**
   - *Mailspring:* `Task` rows with a `local → remote → complete` status (`TaskProcessor.cpp:771-1022`); a 24 h `syncedAt` lock so scans can't undo in-flight changes, while still recording copies the engine has never seen (`MailProcessor.cpp:254-276`).
   - *Alice:* replace `daemon.rs:410-458` `change()` and `providers::modify` with "write locally in one transaction, return, run remotely on a per-account worker that reuses one connection".
   - *Copy these failure rules:*
     - Release the lock and drop pending markers on failure (`TaskProcessor.cpp:1216-1287`).
     - Force a full flag rescan after a failed STORE on CONDSTORE servers (`TaskProcessor.cpp:101-121`).
     - Never let a malformed task abort or wedge the worker (`203637b`).
     - Keep sends at-most-once. Alice's `claim_delivery` already does this (`daemon.rs:494`).
   - *Do better than Mailspring:* retry retryable network errors with backoff instead of completing the task with an error.
2. **Build the scenario harness before changing sync semantics** (D.10 proposal). Mailspring could not ship placements until the harness existed, and the harness found six bugs in four days. Alice's Dovecot CI job is the starting point.
3. **Don't delete a message the moment its last copy disappears.** `settle` deletes it at once (`sync_store.rs:266-269`). Because Alice refreshes INBOX before Archive in every run (`remote.rs:570-576`, `sync.rs:295-300`), a move made in another client is always seen source-first. So it is always a delete, followed by a fresh download of the message (`needs_content` → `Target::New`, `sync_store.rs:402-430`) and loss of local-only fields such as `category`. *Cheapest fix:* keep row-less messages during the run and delete only those still row-less when the run ends. That fixes the common case and skips the re-download. *Mailspring's full version* (`MessageOrphan`, a sweep bounded by per-folder coverage, a 24 h cap, `SyncWorker.cpp:356-390`) is only needed once some mailboxes go unscanned in a run. The incidents that shaped it are in `a83cc9a`.
4. **Never record a cursor before the work behind it is committed.** This is Mailspring's most repeated bug (`2076b05`, `eba3adc`, and three cursor fixes inside `a83cc9a`). Alice has a likely instance:
   - `refresh()` saves the new `uid_next` (`sync.rs:469-475`) *before* `run_with` downloads the new UIDs (`sync.rs:302-317`).
   - If that download fails, the next run's `floor` is the new `uid_next` (`sync.rs:454-458`). Mail that arrived in the failed run is then treated as backfill: an explicit catch-up skips it, and it never reaches the OTP queue, which only considers UIDs at or above `new_since` (`sync_store.rs:561`).
   - *Fix:* save `uid_next` (and `highest_modseq`) after the fresh batches are stored.
5. **Put sent mail on the server when the provider doesn't.** Alice never APPENDs (`README.md:295`), so on Fastmail, iCloud, Dovecot and most plain IMAP, mail sent from Alice is invisible to every other device. *Copy* Mailspring's `21ab5fa` design:
   - look for the server's copy with a short backoff, remembering per Sent folder whether the server ever files one;
   - APPEND if none was found;
   - for 30 minutes, delete a late server copy only next to a copy Alice itself appended (`TaskProcessor.cpp:2023-2083, 2284-2381`).
6. **Relink after a UIDVALIDITY change instead of forgetting the mailbox.** `forget_mailbox` drops every copy, deletes messages whose only copy was there, and re-downloads them (`sync.rs:358-367`, `sync_store.rs:313-319, 675-688`). Alice's ids already depend only on the Message-ID. So mark the copies "UID unknown", re-read headers for the window, relink by Message-ID, and delete leftovers only after a full window search (Mailspring: `SyncWorker.cpp:509-554`, `MailStore.cpp:1213-1235`). Dovecot changes UIDVALIDITY on index rebuilds, and Cyrus on `reconstruct`.
7. **Model a message in several folders as a set, and update flags on every copy.** Alice's ranked "primary copy" is the stage Mailspring spent January to September 2026 on (`4653004`, `b7d0a7e`, `0df7864`) before replacing it (`a83cc9a`). Alice's ranking is deterministic, so it does not flicker the way "latest folder wins" did, but it has the same blind spots:
   - Flags read and written only on the primary copy (`providers.rs:97-116`, `sync_store.rs:375`) let other copies drift on the server. The message's unread state can then flip when the primary copy leaves.
   - Folder views cannot show a message in two folders.
   - *Cheap first step:* STORE on every copy, like Mailspring's flag fan-out (`MailStore.cpp:1021-1041`, `TaskProcessor.cpp:1202-1206`).
   - *Then:* per-folder views that join `remote_messages`, with a per-copy unread bit for per-folder counts (`Models/Thread.cpp:168-177`).
8. **Push with IDLE on the Inbox, on its own long-lived connection.**
   - Re-IDLE every 28 minutes or less, and interrupt IDLE for user actions and for wake or network events (`SyncWorker.cpp:302-318`, `main.cpp:810-824`; Mailspring's client sends wake on OS resume, `client:app/src/browser/application.ts:708-717`).
   - Don't treat an IDLE error as fatal (Yandex drops IDLE), and re-SELECT after a dropped IDLE (`SyncWorker.cpp:251-259, 302-313`).
   - *Watch out:* Alice's `drain()` throws away untagged responses (`remote.rs:413-416`). That is fine with an EXAMINE every run, but on an IDLE connection the EXISTS, EXPUNGE and VANISHED are the data, and they arrive only once (`eba3adc`, `5a31251`).
9. **Don't skip the membership SEARCH on servers without UIDNEXT.** `unchanged` compares `uid_next` and `exists` (`sync.rs:407-410`). When STATUS or EXAMINE has no UIDNEXT (Coremail), or reports 0 (hMailServer, home.pl; `dfdb083`), one arrival plus one removal between runs leaves both values equal, so the run skips the UID SEARCH and misses both changes. Treat a missing or zero `uid_next` as "always search", or search at least every few minutes.
10. **Thread by headers, not subjects.** Subject grouping merges unrelated mail (`README.md:294`) and recomputes every conversation on every list request (`store.rs:139-271`). Copy Mailspring's keys: X-GM-THRID on Gmail; otherwise a reference table holding the message's own Message-ID, the first reference and the last N references (`MailProcessor.cpp:173-197, 786-821`). Also use In-Reply-To, which Mailspring ignores. Fall back to subject only when a message has no References or In-Reply-To. If threads and counts are materialized (Mailspring's `Thread`/`ThreadCategory`/`ThreadCounts`), ship the invariant check with them: Mailspring's incremental counters drifted badly enough to need a repair migration (`314fe40`, `d04cfa8`).
11. **On Gmail, sync All Mail, Spam and Trash, and read membership from X-GM-LABELS.** Alice syncs INBOX, Sent, Trash, Spam and All Mail as separate mailboxes and joins them by Message-ID (`remote.rs:555-583`, `sync_store.rs:136-181`). It fetches headers of most messages twice and cannot show user labels. Mailspring's model: one placement per message in All Mail, Spam or Trash, X-GM-MSGID and X-GM-THRID, and labels as data (`SyncWorker.cpp:960-964`, `MailProcessor.cpp:299-304`). This needs the IMAP crate migration.
12. **A MOVE fallback, done the safe way.** Alice refuses to move without the `MOVE` capability (`providers.rs:135-137`). If a fallback is added, it must be COPY, `STORE +\Deleted` on exactly those UIDs, `UID EXPUNGE` with UIDPLUS, and **never** a plain EXPUNGE after a reconnect (`TaskProcessor.cpp:218-247`, `dfdb083`). Trust a multi-message COPYUID only after matching headers, because Yahoo permutes it (`TaskProcessor.cpp:123-216`). Alice moves one UID per command today, so the COPYUID it already trusts (`providers.rs:183-191`) is safe.
13. **Cheap database instrumentation.** Wrap transactions to log lock-wait over 1 s and hold time over 100 ms, as Mailspring does (`MailStoreTransaction.cpp:36-64`). Alice already fetches content before taking `entry.lock` (`sync.rs:519-571`), which is the main point of `379ed85`. If more connections or processes ever share the file, set `busy_timeout` (Mailspring needed 60 s, `63ab613`) and give HTTP reads their own WAL read connection so the UI doesn't wait on the sync mutex (`store.rs:9-13`).
14. **Decide deliberately about syncing every folder.** Alice syncs five special-use mailboxes (`remote.rs:570-582`), so mail that server-side filters put in user folders never appears. Mailspring syncs every selectable folder in role order (`SyncWorker.cpp:419-424, 955-994`) and detects roles by localized names after decoding modified UTF-7 (`MailUtils.cpp:361-417`). If Alice adds folders, take the name table (`constants.h:346-413`) and the duplicate-LIST de-duplication (`SyncWorker.cpp:811-834`) with it.

#### Avoid: bugs Mailspring hit, and where Alice stands

| Mailspring bug (commit) | What happened | Alice |
|---|---|---|
| Walk cursor advanced past a truncated fetch (`2076b05`) | About 80% of each initial-sync chunk was never ingested on QRESYNC servers | Not exposed: `missing` is recomputed from a full window search (`sync.rs:429-452`). Item 4 is the same bug class |
| `Query` builder kept one clause per column (`eba3adc`) | `VANISHED 20:120` deleted everything below 120; 178 of 200 messages lost | Alice writes SQL by hand. Never build "range" predicates through a map keyed by column |
| VANISHED only harvested after IDLE, only the first line (`eba3adc`, `5a31251`) | Expunges seen on other commands were lost for good once HIGHESTMODSEQ advanced | Relevant once IDLE or QRESYNC arrives (item 8): collect untagged responses on every command |
| Stale view of the selected mailbox (Dovecot; `a83cc9a`) | STATUS and FETCH on the selected mailbox returned moved-out messages and old UIDNEXT | Not exposed: Alice EXAMINEs each mailbox again every run (`sync.rs:357`). Keep it that way, or NOOP before trusting the selected mailbox |
| One folder per message, "latest wins" or priority (`b53e0db`, `b7d0a7e`, `0df7864`) | Flicker; Inbox and Sent looked empty | Ranked primary copy (item 7): deterministic, but the same blind spots |
| Duplicate LIST entries → UNIQUE violation (`95829f2`) | Crash loop on every launch | `discover_folders` takes the first match, so it is safe. Keep folder inserts idempotent if folders become rows |
| Task with a `null` field (`203637b`) | Engine aborted on every launch until the user rebuilt the cache | Applies to the item 1 queue: catch everything per task and mark it failed |
| Two workers applying one change from stale loads (`a83cc9a`) | Thread counters double-counted | Alice saves under one mutex and reloads inside the transaction (`sync_store.rs:446-584`). Keep "load inside the write transaction" |
| Saving an object loaded before network I/O (`a83cc9a`, `MailProcessor.cpp:464-476`) | A stale `folders` snapshot overwrote fresh state | `change()` saves `updated`, built from the message read before the IMAP call (`daemon.rs:417-432`). Today the per-account lock stops sync from writing in between; a task queue must reload after the remote phase |
| Plain EXPUNGE in the no-MOVE fallback (`dfdb083`) | Erased other clients' `\Deleted` mail | Item 12 |
| Duplicate Sent copies (`21ab5fa`) | Exchange files its copy up to about 3 minutes after SMTP | Item 5; don't APPEND without the late-copy cleanup |
| A failed remote phase leaked the 24 h lock (`a83cc9a`) | The message ignored the server for a day | Applies to item 1 |
| Thread counters drifted (`314fe40`, `d04cfa8`) | Needed a repair migration over 3,741 threads | Item 10: materialize only with an invariant check |
| Time-seeded random ids (`dc4a446`) | Two processes generated the same ids | Alice uses `uuid` v4/v5, so it is safe |
| A busy timeout too short for many accounts (`63ab613`) | Crash-and-retry loops | One process and one connection today; see item 13 if that changes |

#### Where Alice is already better, or simpler on purpose

- **Membership can't silently drift.** A window `UID SEARCH` whenever the mailbox status changed recomputes exactly what is missing (`sync.rs:406-452`). Mailspring's walk and scan cursors needed three fixes and a daily safety scan to get there.
- **A fresh view every run.** One EXAMINE per mailbox per run, on read-only connections (`remote.rs:1-7, 114-130`), avoids the stale-view and NOOP issues entirely.
- **Lean downloads.** Text parts only (4 MB cap), attachments on demand (`remote.rs:205-361`), and a user-chosen window (`README.md:292`). Mailspring fetches the whole RFC 822 for three months and writes every attachment to disk (`MailProcessor.cpp:420-461`), and its cleanup never removes those files or the FTS text.
- **Simpler identity.** A Message-ID-based id, plus "another UID with this Message-ID in the same mailbox is a different delivery" (`sync_store.rs:155-161, 551-558`). It avoids Mailspring's documented collisions and its two id schemes (`MailUtils.cpp:580-633`).
- **One process and one writer.** No cross-process lock fights, no start staggering, no abort-and-relaunch loop. Sync errors are recorded per account (`sync_store.rs:641-672`) instead of crashing.
- **Revision polling instead of a delta stream.** `epoch.total_changes` (`store.rs:72-74, 1103-1105`) avoids Mailspring's delta coalescing, key merging and "unsafe erase" bookkeeping. Revisit only if whole-view refetches show up in profiles.
- **Better search.** Field-aware queries, diacritic folding, prefix indexing and typo tolerance (`search.rs:1-12`, `store.rs:1163`) over per-message rows maintained by triggers. Mailspring uses thread-level FTS whose body text only ever grows.
- **At-most-once sending already exists.** `claim_delivery`/`finish_delivery` (`daemon.rs:494-504`) does the job of Mailspring's `_performRemoteRan`.
