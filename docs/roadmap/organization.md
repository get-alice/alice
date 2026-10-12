# Organization, productivity and inbox triage

Research behind the matching items in [`todo.md`](../../todo.md). Each entry covers why it matters, how Mailspring and other clients do it, the relevant specs, what Alice has today, a build plan, pitfalls, acceptance criteria and effort. Code citations point at the Alice working tree as of 2026-10-13; Mailspring paths are relative to [Foundry376/Mailspring](https://github.com/Foundry376/Mailspring) (`mailspring/`) and [Foundry376/Mailspring-Sync](https://github.com/Foundry376/Mailspring-Sync) (`mailsync/`).

Sections covered: P1 → Organization, P2 → Productivity, P2 → Inbox triage, with (NEW) items at the end of each section.

Path prefixes: **MS-app/** = `scratchpad/repos/mailspring/app/`, **mailsync/** = `scratchpad/repos/mailsync/MailSync/`, **MS-cl** = `scratchpad/ms.md` (the Mailspring changelog). Alice paths are relative to the repo root.

## Section findings

1. **Every action is one HTTP PATCH per message, and each one opens a new TLS IMAP login.** The desktop loops `await patch(id)` (`apps/desktop/src/App.tsx:169-173`). `PATCH /v1/messages/{id}` (`crates/alice-core/src/daemon.rs:410-458`) calls `providers::modify`, which runs `session()` → `SELECT` → `STORE`/`MOVE` → `LOGOUT` for that single UID (`crates/alice-core/src/providers.rs:78-172`).
   - Archiving 50 conversations is therefore about 150 logins.
   - Multi-select, mark-all-read, undo, rules, snooze and block-sender all need a **batch action path**: group by account and mailbox, then send one `UID STORE`/`UID MOVE` over a UID set per mailbox.
   - Mailspring does exactly this. One `ChangeFolderTask` covers many threads, and mailsync groups copies by folder and sends one command per folder (`mailsync/TaskProcessor.cpp:1190-1287`).
   - Build the batch path together with the P0 "Two long-lived IMAP connections per account" item.
2. **Folders are a fixed whitelist.** Only `inbox`/`archive`/`trash`/`spam` are accepted as destinations (`daemon.rs:459-471`, `providers.rs:122-131`, `store.rs:330-336`), and the sidebar list is hardcoded (`App.tsx:20-22`).
   - Moving to any folder, folder CRUD, per-folder counts, a Snoozed folder and rules with "move to folder" all depend on three P0 items: **Sync every folder**, **Multi-folder message model** and **Gmail labels**.
   - Do those first.
3. **Use one query language for everything.** `search::Expr` and `Matcher` (`crates/alice-core/src/search.rs:25-44`, `:351-420`) already cover AND/OR/NOT, fields, flags and dates, with typo tolerance. Mailspring's search grammar is weaker: NOT is binary only, and it has a precedence bug (`MS-app/src/services/search/search-query-parser.ts:334-353`).
   - Make `Expr` the condition language for saved searches, split-inbox tabs, mail rules, the screener and Gmail-filter export.
   - Extend the grammar once (`in:`, `cc:`, `reply-to:`, `list:`, `is:muted`, `is:snoozed`, natural dates) and every feature gains it.
   - Mailspring has two separate systems: the search grammar, and the rule templates in `MS-app/src/mail-rules-templates.ts`. They drifted apart.
4. **Build one durable local scheduler.** Snooze, send later, follow-up reminders and undo send all need "at time T, do X, survive a restart, at most once".
   - Mailspring stores an `expiration` in `ModelPluginMetadata` (`mailspring-schema.sql:41-44`). `MetadataExpirationWorker` re-emits it every pass until the client clears it (`mailsync/MetadataExpirationWorker.cpp:61-125`). It needs no network.
   - Alice should have one `jobs` table plus one tokio timer in the daemon, woken like `SyncEngine::wake`, instead of four separate mechanisms.
   - The daemon is a detached process started on demand by clients (`crates/alice-core/src/client.rs:10-145`). Scheduled work fires only while it runs, so the P1 "Launch at login" item matters here. Mailspring has the same limitation: mailsync exits about 30 s after the app closes (`mailsync/main.cpp:762-775`).
5. **Persist on the server through IMAP itself, not a cloud.** Mailspring's snooze and reminders need a Mailspring ID and its metadata service at `id.getmailspring.com` (`mailsync/TaskProcessor.cpp:1969-1990`). Without one, snooze doesn't load at all (`MS-app/src/package-manager.ts:133-135`). Alice has no cloud. Use these instead:
   - **Mailboxes:** RFC 9979 (May 2026) registers the `Snoozed`, `Scheduled` and `Memos` mailbox attributes.
   - **Keywords:** `$muted`, `$followed`, `$new`, `$unsubscribed`, `$canunsubscribe` (RFC 9979), `$Forwarded`, `$SubmitPending` and `$Submitted` (RFC 5550), and `$Junk`/`$NotJunk` (IANA registry).
   - **Optional mailbox annotations:** RFC 5464 METADATA, on Dovecot and Cyrus. It works per server and per mailbox, never per message.
6. **Alice needs generic IMAP keyword support** before mute, tags, `$Forwarded` de-duplication of rule forwards, `$Junk` training and cross-device send-later locks.
   - Today `RemoteFlags` only knows `\Seen` and `\Flagged` (`crates/alice-core/src/remote.rs:33-37`).
   - Store a `keywords: Vec<String>` from `FETCH FLAGS`, and write it with `STORE ±FLAGS`.
   - Check that `PERMANENTFLAGS` contains `\*` (RFC 9051 §7.1) before relying on custom keywords.
   - Watch the limits: Dovecot's maildir backend allows only 26 keywords per mailbox, and Cyrus allows 128. Never encode data such as timestamps in keyword names.
7. **Make one command registry.** `apps/desktop/src/shortcuts.ts:20-49` is already the single source for keys, and conversation actions are `MenuItem` records (`App.tsx:253-272`, `ContextMenu.tsx:8`). Turn these into one registry `{id, label, keys, enabled, run}`. The command palette, shortcut presets, remapping and `g` sequences all read from it.
8. **Provider APIs need new opt-in scopes.**
   - Gmail filters and vacation need `gmail.settings.basic`, a **restricted** scope.
   - Outlook Focused Inbox and rules need Graph `Mail.Read` and `MailboxSettings.ReadWrite`.
   - Follow the existing per-feature `Integrations` consent pattern (`crates/alice-core/src/model.rs:56-64`, `crates/alice-core/src/oauth.rs:63-78`).

**Recommended build order:**
1. Batch action endpoint, plus keyword plumbing.
2. The P0 folder, label and multi-folder work.
3. Multi-select.
4. Undo (in-memory inverse actions; the offline journal comes later).
5. Move picker.
6. Folder CRUD.
7. Mark all read.
8. Per-folder counts.
9. Permanent delete.
10. Unsubscribe.
11. Block sender.
12. Search grammar, then saved searches, then rules, then the split inbox and screener.
13. The scheduler, then snooze, send later and reminders.
14. Command registry, then the palette, presets and `g`-navigation.

**Most-requested Mailspring features in this area:**
- Forum, by votes:
  - #291 conversation view optional (172)
  - #244 move between accounts (37)
  - #320 delete a single message in a thread (26)
  - #470 custom sorting (21)
  - #528 saved searches (20)
  - #677 remove Mailspring/Snoozed (20)
  - #540 IMAP keyword tags (15)
- GitHub, by 👍:
  - #159 mark folder read (41)
  - #426 delete a single message (27)
  - #580 move between accounts (25)
  - #526 label colors (18)

All of these are covered below.

## P1: Organization

<a id="multi-select-and-bulk-actions-shiftcmd-click-select-all"></a>

### Multi-select and bulk actions (shift/cmd-click, select all)

*Roadmap: P1 · Organization*

- **Why:** Cleaning up an inbox one conversation at a time is the main complaint of anyone coming from Gmail, Outlook or Apple Mail. Bulk archive, read and move are table stakes.
- **Competitors:**
  - **Mailspring click handling:** Cmd/Ctrl-click toggles and Shift-click extends (`MS-app/src/components/multiselect-list.tsx:274-282`).
    - In list mode a checkmark column is shown, and `x` (`core:select-item`) toggles the row under the keyboard cursor (`multiselect-list-interaction-handler.ts:29-31,67-73`).
    - In split mode the focused thread becomes the selection and `x` does nothing (`multiselect-split-interaction-handler.ts:42-52,67-69`).
  - **Mailspring keyboard selection:**
    - `shift+up/down` extends the selection; the Gmail preset adds `shift+j/k`.
    - Select all is `mod+a`; the Gmail preset adds `* a`, and `* n` deselects.
    - `* r` / `* u` / `* s` / `* t` select read / unread / starred / unstarred (`MS-app/keymaps/templates/Gmail.json:12-18`). The Edit menu repeats these as "Select All Read/Unread/Starred/Unstarred" (`MS-app/menus/darwin.js:72-75`).
    - Select all covers only the **loaded** result set, not the whole folder (`observable-list-data-source.ts:96-101`).
  - **Mailspring bulk toolbar:**
    - List mode shows "N threads selected" and "Clear Selection" (`multiselect-toolbar.tsx:20-43`).
    - Split mode shows a "N Selected Messages" card stack (`selected-items-stack.tsx:29-63`).
    - Buttons: Archive, Spam/Not Spam, Trash, Star, Read/Unread, Move to Folder, Apply Label (Gmail), Snooze and Reminder (`thread-list/lib/thread-toolbar-buttons.tsx:449-475`).
    - The folder picker is hidden when the selection spans accounts (`category-picker/lib/toolbar-category-picker.tsx:44-91`).
    - Each action is **one task covering all selected threads** (`task-factory.ts:12-38`).
  - **Mailspring changelog:**
    - 1.7.2: selection cleared on layout switch.
    - 1.9.0: select-all follows the OS convention.
    - GitHub #682: select all broken on Linux.
  - **Gmail:** checkbox column, plus a "Select all N conversations in Inbox" banner that applies to the whole folder.
  - **Apple Mail:** standard Cmd/Shift selection; a multi-selection shows a stack in the reader.
- **Alice today:**
  - Single selection only: `selectedId` (`App.tsx:58`).
  - Actions take one `Thread` (`App.tsx:253-272`).
  - `change()` patches ids sequentially (`App.tsx:169-173`), and the daemon has no batch endpoint (`daemon.rs:276-280`).
  - The page size is 50 (footer, `App.tsx:385`).
- **Plan:**
  1. **Daemon.** Add `POST /v1/messages/batch` taking `{ids: Vec<String>, patch: MessagePatch}`, or `{query: MessageQuery, patch}` for "all in folder".
     - Lock each account.
     - Group the messages by `(remote_mailbox, uid_validity)`.
     - Per group: `SELECT`, then `UID STORE <set> +FLAGS.SILENT (...)` or `UID MOVE <set> <dest>`, chunked with the existing `uid_set` (`remote.rs:73`) and `UID_CHUNK` (`remote.rs:23`).
     - Generalise `moved_uid` (`providers.rs:183-191`) to parse COPYUID *sets*, so every moved message keeps its new identity. The undo item needs these too.
     - Return `{updated, errors: [{id, message}]}`. Have the single-message `PATCH` call the same code.
  2. **Desktop selection state.** In `App.tsx`, add `selection: Set<string>` (thread ids) plus an `anchor`.
     - Mod-click toggles, Shift-click selects the range from the anchor, `x` toggles (Gmail), `Shift+J/K` extends, `Mod+A` selects the page, and Escape clears.
     - Prune ids that disappear on the 2-second refresh (`App.tsx:139`).
  3. **Bulk header.** When `selection.size > 1`, the reader pane shows "N selected", the action buttons from `threadActions` (archive, trash, spam, inbox, read/unread, star, Move…) and "Select all N in <folder>". The last option switches the request to `{query}`.
  4. **Shortcuts and menus act on the selection.** Keyboard shortcuts (`App.tsx:303-306`) and the row context menu (`App.tsx:381`) act on the selection when it is not empty. Thread actions take `Thread[]`.
  5. **Accessibility.** Make the list `role="listbox" aria-multiselectable="true"` with `aria-selected` on rows. Optionally, the avatar becomes a checkbox on hover, as in Gmail and Spark.
  6. **CLI and MCP.** Accept several ids in `alice archive ID...` and in MCP `update_message`, so both use the batch endpoint.
- **Pitfalls:**
  - A conversation's `ids` are only the copies in the listed view (`model.rs:158-159`). Keep that semantic for bulk moves, so moving out of Inbox doesn't drag Sent copies along. Mailspring's `sourceFolderIds` exists for the same reason (`change-folder-task.ts:90-92`), and GitHub #1848 is the complaint when it goes wrong.
  - Cross-account selections need per-account destination folders. Mailspring hides the picker in that case.
  - "Select all in folder" must say whether it includes mail outside the sync window. Mailspring only covers loaded rows.
  - Partial failure: report which items failed, and keep the rest applied.
  - Long command lines: some servers cap them at about 8 KB, so chunk the UID sets.
  - Optimistic UI: rows fold away before the server confirms (the existing `leaving` set). If the batch fails, they must come back.
- **Done when:**
  - A Rust test runs `batch` against a mock session and checks it sends one `UID MOVE 1:3,7` per mailbox.
  - A `scripts/test-mail.py` case archives 30 messages in at most 2 IMAP logins, and all of them land in Archive on Dovecot.
  - A UI test covers Shift-click range, Mod-click toggle, `Mod+A`, and the bulk header count.
- **Effort:** M. Depends on: "Two long-lived IMAP connections per account" (P0). It works without that, just slower.

<a id="undo-for-archive-trash-move-mark-read"></a>

### Undo for archive, trash, move, mark read

*Roadmap: P1 · Organization*

- **Why:** Single-key triage (`e`, `#`, `!`) without undo loses mail, and people trust keyboard triage only once undo exists.
- **Competitors:**
  - **Mailspring undo stack** (`MS-app/src/flux/stores/undo-redo-store.ts`):
    - One `queueTasks` call becomes one undo block, and only if **every** task has `canBeUndone` (:53). Descriptions are joined with ", " (:56).
    - Undo waits up to 10 s for the engine's copy of the task (:12-13, :114-130), then queues `createUndoTasks()` (:60-76).
    - Redo re-queues identical tasks (:78-82). The stack lives in memory only.
  - **Undo data per task:**
    - `ChangeFolderTask` has the engine write `undoPlacements` (each copy's original folder), and undo restores copy by copy (`mailsync/TaskProcessor.cpp:453-516, 583-594`). If the engine never reported, it approximates from source folders (`change-folder-task.ts:204-227`).
    - Unread and starred undo simply invert the flag on every thread (`change-unread-task.ts:48-52`), which loses a mixed earlier state.
  - **Toast:** shown 3000 ms; hover pauses it (`undo-redo/lib/undo-redo-toast.tsx:58-60, 205-211`).
  - **Shortcuts:** `mod+z` undo, `mod+shift+z` / `mod+y` redo (`MS-app/keymaps/base.json:2-3`). In text fields these do text undo instead (`window-event-handler.ts:144-147`).
  - **Not undoable:** folder create/rename/delete, Empty folder, mark-read-on-open, mail-rule actions and unsnooze.
  - **Changelog:** 1.0.5 undo toast fixes; 1.0.6 undo of a move fixed on generic IMAP; 1.0.8 toasts reappearing; 1.19.0 `canBeUndone` init fix.
  - **Gmail:** an "Undo" link in its toast for about 5 s.
  - **Superhuman:** `Z` undoes.
- **Alice today:**
  - A toast exists (`App.tsx:65, 157`, 4.2 s, held while hovered), but it has no action button.
  - Moves record the old location for sync (`store.record_move`, `daemon.rs:430-447`; `crates/alice-core/src/sync_store.rs:586`).
  - `UID MOVE` already captures the new UID from COPYUID (`providers.rs:136-142`).
  - Nothing exposes undo.
- **Plan:**
  1. **Undo records in the daemon** (so the CLI and TUI get undo too). In `AppState` (`daemon.rs:41`), keep an in-memory ring of the last 20 `UndoRecord{id, account_id, created_at, inverse: Vec<(message_id, MessagePatch, from_placement)>}`.
     - The batch endpoint builds the record from what it actually changed:
       - Read/star changes store the **previous value per message**, not "invert all".
       - Moves store `(dest mailbox, new uid) → source mailbox`.
     - It returns `undo_id`.
  2. **Undo endpoint.** `POST /v1/undo/{undo_id}` replays the inverse through the same batch code. Moves use `UID MOVE` from the recorded destination UID back to the source mailbox. Return 410 when expired (after 60 s) or when a UID has vanished.
  3. **Desktop toast.** Every action toast gets an "Undo" button and `Mod+Z` (new `shortcuts.undo`; ignored in text fields). Keep a redo stack of one level; skip redo initially.
  4. **Grouping.** One user gesture is one record, so a bulk archive of 40 conversations undoes as one.
  5. **What not to make undoable,** like Mailspring: rule actions, mark-read-on-open, permanent delete and folder deletion.
  6. **Later.** When the P1 offline queue lands, switch moves to "delayed commit": keep the server operation pending for the toast duration and cancel it locally. Undo then becomes free and works offline, and both features share one action journal table.
- **Pitfalls:**
  - **Gmail.** Archiving from INBOX is a MOVE to All Mail, which removes `\Inbox`. Undo must MOVE back to INBOX from All Mail using the All Mail UID, not COPY; COPY would duplicate the label state.
  - **Servers without MOVE** (fallback COPY + `\Deleted` + UID EXPUNGE, a P0 item): undo is the same fallback in reverse.
  - **Concurrent changes.** Another client may have moved the message meanwhile. Then the UID is gone; report "Couldn't undo: the message changed on the server."
  - **Mark-read-on-open** happens on every `choose` (`App.tsx:180`). Don't let it push the archive record out of the "last action" slot.
  - **Undo of a move must restore the conversation's selection** and scroll position. Mailspring users complained when undo didn't reselect.
- **Done when:**
  - A Rust test in `daemon.rs` tests: archive 3 messages → undo → all three are back in INBOX with the same Message-IDs. Also: mark read on a mixed read/unread set → undo restores the mixed state.
  - A UI check: `e` then `Mod+Z` brings the row back within one refresh.
- **Effort:** M. Depends on: Multi-select and bulk actions (batch endpoint). Pairs with the Offline action queue (P1).

<a id="create-rename-delete-and-nest-folders-and-labels"></a>

### Create, rename, delete and nest folders and labels

*Roadmap: P1 · Organization*

- **Why:** Users who file mail need to make folders where they work. Today they have to leave Alice to create one.
- **Competitors:**
  - **Mailspring menus:**
    - Folder management is in the sidebar context menu or "•••" button: "Mark All as Read" | "New Subfolder..."/"New Sublabel...", "Rename", "Delete" | Export .eml/.mbox (`MS-app/src/components/outline-view-item.tsx:271-319`). A "+" on the section header creates a top-level folder (`sidebar-section.ts:244-246`).
    - Delete confirms with "Deleting folders and labels cannot be undone…" (`sidebar-item.ts:63-71`, added 1.7.4).
  - **Mailspring engine:**
    - `SyncbackCategoryTask` (`syncback-category-task.ts:11-47`) has no local phase.
    - Its remote phase replaces `/` with the server delimiter, prepends the namespace prefix, then issues `RENAME` or `CREATE` (`mailsync/TaskProcessor.cpp:1887-1941`).
    - `DestroyCategoryTask` sends `DELETE` (:1993-2006), not recursively. The local row disappears on the next folder sync (`mailsync/SyncWorker.cpp:1062-1112`).
  - **Mailspring weak spots:**
    - Rename splits on `/[./\\]/`, so a `.` in a name is treated as a separator (`sidebar-item.ts:262-273`).
    - Folder ids are a hash of the path (`mailsync/MailUtils.cpp:524-528`), so a renamed folder is re-downloaded from scratch.
  - **Changelog:**
    - 1.0.4: namespace prefixes such as `INBOX.`.
    - 1.0.6: rename showed both names; emoji in names.
    - 1.20.0: easier nested create (#2655).
  - **Thunderbird and Apple Mail:** drag a folder to re-parent it. Thunderbird subscribes on create.
- **Spec:**
  - RFC 9051 §6.3.4 CREATE, §6.3.5 DELETE (a mailbox with inferiors may be left `\Noselect`), §6.3.6 RENAME (inferior names are renamed too), §6.3.7 SUBSCRIBE, §6.3.9 LIST (hierarchy delimiter).
  - RFC 9051 §5.1.3 / RFC 3501 §5.1.3 modified UTF-7 names, unless the server offers `UTF8=ACCEPT` (RFC 6855).
  - RFC 6154 special-use (never rename or delete `\Sent`, `\Trash` etc.).
  - Gmail: labels are mailboxes; CREATE/RENAME/DELETE manage them (developers.google.com/workspace/gmail/imap/imap-extensions).
- **Alice today:**
  - `Remote::folders` lists everything (`remote.rs:106-112`), but `discover_folders` keeps only the five roles (`remote.rs:503-583`).
  - Nothing creates, renames or deletes mailboxes.
  - Names are not decoded from modified UTF-7.
- **Plan:**
  1. **Prerequisite** (P0 "Sync every folder"): persist each account's LIST result in `mailboxes(account_id, name, delimiter, attributes, role, subscribed)` and decode modified UTF-7 for display. Add `utf7-imap` or hand-roll about 40 lines. Expose it as `GET /v1/accounts/{id}/mailboxes`.
  2. **Create.** `POST /v1/accounts/{id}/mailboxes {parent?, name}`:
     - Reject the delimiter character inside `name`.
     - Join `parent + delimiter + name` and encode to modified UTF-7.
     - Send `CREATE`, then `SUBSCRIBE`. Many clients, Apple Mail included, show only subscribed folders.
  3. **Rename.** `PATCH /v1/accounts/{id}/mailboxes/{name} {name?, parent?}` sends `RENAME`.
     - Afterwards, `SELECT` the new name. If UIDVALIDITY is unchanged, rewrite `remote_mailbox` in `messages` and `remote_messages`, and `mailbox_sync.mailbox` (`sync_store.rs:24-26`), in one transaction for the folder and its children. That avoids Mailspring's full re-download.
     - Otherwise, forget the old mailbox and let sync rebuild it.
  4. **Delete.** `DELETE /v1/accounts/{id}/mailboxes/{name}` refuses special-use and INBOX. It confirms with the message count from `STATUS (MESSAGES)`, deletes children first (deepest first), then sends `DELETE` and drops local rows.
  5. **Sidebar UI.** A folder tree under each account (`App.tsx` sidebar; `Settings` for the empty state). Context menu: New subfolder…, Rename…, Delete…, Mark all as read; "+" on the section header. Drag a folder onto another to re-parent it. Gmail accounts say "label" instead of "folder".
  6. **CLI.** `alice folders [list|create|rename|delete]`.
- **Pitfalls:**
  - **Namespace prefixes.** Some servers need everything under `INBOX.` (Courier, older Cyrus). Use the NAMESPACE response (RFC 2342), or infer from where INBOX children live (Mailspring 1.0.4).
  - **Delimiter differences:** `.` vs `/`, and Gmail's `[Gmail]` container is `\Noselect`.
  - **Proton Bridge and Outlook** reject some characters.
  - **Gmail.** Deleting a label does not delete its mail; it stays in All Mail. Say so in the dialog. On folder servers, DELETE destroys the mail.
  - **Never delete a mailbox an Alice feature depends on.** The Snoozed folder and the folders that rules use need a warning.
  - **Unicode NFC/NFD** normalisation on macOS input.
- **Done when:**
  - A `scripts/test-mail.py` Dovecot case creates `Projects/Alpha` (UTF-8 "Ärger" as a second name), moves a message into it, renames it to `Projects/Beta` without re-downloading (the message row id is unchanged), and deletes it after a confirmation count.
  - A unit test round-trips modified UTF-7 encode/decode against the RFC 3501 examples.
- **Effort:** M. Depends on: "Sync every folder, not only the five special-use roles" and "Gmail labels via X-GM-LABELS" (P0).

<a id="move-to-any-folder-with-type-ahead-picker"></a>

### Move to any folder with type-ahead picker

*Roadmap: P1 · Organization*

- **Why:** Filing into project folders is the core workflow for non-Gmail users. Alice can only reach the four role folders today.
- **Competitors:**
  - **Mailspring:**
    - "Move to..." and "Label as..." popovers (`MS-app/internal_packages/category-picker`).
    - Shortcuts `v` and `l` in the Gmail preset (`Gmail.json:44-45`); `l` only for label accounts.
    - Word-prefix regex filter (`MS-app/src/flux/models/utils.ts:119-121`).
    - The list hides drafts, sent, snoozed, the current folder, and All Mail except from Spam/Trash (`move-picker-popover.tsx:80-100`).
    - Tab or → completes the highlighted name (1.11.0; `:199-201`).
    - A `"text" (create)` row creates the folder, waits for the server, then moves (`:113-120, 151-165`).
    - The label picker shows full/partial/none state with a dash for partial (`label-picker-popover.tsx:142-193`).
    - There is no recently-used list.
    - Changelog: 1.17.2 search lock-up race; 1.21.1 regex compile optimisation.
  - **Superhuman:** `V` move and `L` label with fuzzy search.
  - **Apple Mail:** "Move to Predicted Folder" (Ctrl+Cmd+M) suggests the destination from past filing.
- **Alice today:**
  - The destination whitelist is in `validate_patch` (`daemon.rs:459-471`) and `providers::modify` (`providers.rs:122-131`).
  - `MessagePatch.folder` is a logical name (`model.rs:212-218`).
- **Plan:**
  1. **Accept real mailboxes.** Extend `MessagePatch` with `mailbox: Option<String>`, an exact server name from the account's `mailboxes` table. Validate that it exists and is selectable.
     - After a move, set `Message.folder` to the logical role if the mailbox has one, otherwise `"mailbox"`. The P0 multi-folder model replaces this mapping.
  2. **Provider.** `providers::modify` and the batch path `UID MOVE` to the given mailbox. The no-MOVE fallback comes from the P0 item.
  3. **Picker component.** `MovePicker.tsx` is a popover anchored to a new toolbar button "Move…" and opened by `v` (Gmail preset) or the command palette.
     - The input filters with accent folding, reusing the `fold` logic from `search.rs:47-55` (port it to TS) and word-prefix matching.
     - Results are indented by hierarchy; the 5 most recently used destinations appear first (localStorage).
     - Enter moves; Tab completes; the last row is "Create “text”", which calls the folder-create endpoint and then moves.
  4. **Gmail.** A separate "Label…" picker (`l`) with checkboxes showing partial state, using `X-GM-LABELS` add/remove from the P0 labels item. "Move to" on Gmail means add the label and remove `\Inbox`.
  5. **Context menu.** Add a "Move to ▸" item listing the recents.
  6. **Drag and drop.** Dragging rows onto sidebar folders uses the same endpoint (Mailspring GitHub #1533).
- **Pitfalls:**
  - **Cross-account selections:** show folders per account, or disable the picker, as Mailspring does.
  - **Moving a conversation** moves only the copies in the current view. A move from Inbox must not take Sent copies.
  - **Large trees:** Fastmail and Exchange accounts can have 1,000+ folders, so virtualise the list.
  - **Name collisions** with the delimiter.
  - **Moving into a `\Noselect` parent** must be disabled.
- **Done when:**
  - A Dovecot fixture test moves a conversation to `Clients/Acme` via `v`, types `acm`, and presses Enter.
  - A unit test checks the picker filter ranking (prefix beats infix; recents first).
- **Effort:** M. Depends on: Create, rename, delete and nest folders (mailbox table), "Sync every folder" (P0), and Multi-select (batch).

<a id="mark-all-as-read-per-folder"></a>

### Mark all as read, per folder

*Roadmap: P1 · Organization*

- **Why:** It is the most-voted Mailspring GitHub request in this area (#159, 41 👍). People want to clear newsletter or notification folders in one action.
- **Competitors:**
  - **Mailspring:**
    - Sidebar "Mark All as Read" (1.24.1) queries all unread threads in the category (requiring `inAllMail` unless Spam/Trash) and queues **one** undoable `ChangeUnreadTask` (`sidebar-item.ts:173-203`). The engine then runs per-folder `UID STORE +FLAGS (\Seen)` over the known UIDs, not `1:*` (`mailsync/TaskProcessor.cpp:407-419`).
    - The Edit-menu item "Mark All as Read" (1.9.0, `thread-list:mark-all-as-read`) covers only loaded threads (`thread-list.tsx:308-326`).
  - **Thunderbird:** Shift+C marks a folder read; "Mark Folder Read" from the context menu.
  - **Gmail:** select all, then "Mark as read" with the "all N conversations" banner.
- **Spec:** RFC 9051 §6.4.6 STORE, §6.4.9 UID. RFC 7162 CONDSTORE makes the change cheap to sync back.
- **Alice today:** Inbox shows an unread count (`App.tsx:349`). There is no mark-all action; `read` acts on one conversation's `unread_ids` (`App.tsx:267`).
- **Plan:**
  1. **Daemon endpoint.** `POST /v1/messages/batch` with `{query: {folder, account_id, q?}, patch: {is_read: true}}`, from the batch item.
     - **Pure folder (no `q`):** for each mailbox in scope, send `SELECT` then `UID STORE 1:* +FLAGS.SILENT (\Seen)`. This also covers mail outside the sync window, which is what "mark folder read" means. Then update local rows with one SQL `UPDATE`.
     - **Search or unified views:** use the matched ids only.
  2. **Undo.** Record the unread ids that were in the local store, so undo restores exactly those. Mail outside the window can't be restored; say so in the toast.
  3. **UI.** Sidebar folder context menu "Mark all as read", plus a list-header button when `unread_total > 0` (the counts already arrive in `ThreadPage`, `model.rs:169-175`), and `Shift+I` with a selection (Gmail preset). Confirm only above 500 messages.
- **Pitfalls:**
  - **Gmail.** Marking INBOX `1:*` read marks those messages read everywhere, which is correct for labels. In "Starred" or a label view, run it in that label's mailbox, not All Mail.
  - **Unified inbox** spans accounts: do one STORE per account.
  - **CONDSTORE.** The next sync will see the MODSEQ bump; make sure it doesn't re-download.
  - **Without CONDSTORE,** the full flag rescan interval applies.
- **Done when:** A Dovecot fixture with 200 unread (including mail older than the window) → mark all read → `STATUS INBOX (UNSEEN)` returns 0, using one STORE command. A unit test checks the local SQL update and the undo record.
- **Effort:** S. Depends on: Multi-select and bulk actions (batch endpoint).

<a id="unread-counts-on-every-folder-and-optionally-across-all-accounts"></a>

### Unread counts on every folder, and optionally across all accounts

*Roadmap: P1 · Organization*

- **Note:**
  - Version correction: per-folder sidebar counts arrived in Mailspring **1.24.1** ("Show unread counts for all folders / labels", `core.workspace.showUnreadForAllCategories`, default **false**; `MS-app/src/config-schema.ts:71-75`).
  - 1.23 added `core.notifications.countBadgeAllAccounts`, which is the **dock badge** across accounts (`config-schema.ts:381-385`). That half belongs with the P1 "Dock / taskbar unread badge" item.
- **Why:** Without counts, folders that rules or other clients fill are invisible.
- **Competitors:**
  - **Mailspring** stores `ThreadCounts(categoryId, unread, total)` (`mailspring-schema.sql:27`).
    - It maintains the table incrementally from per-thread category diffs in `Thread::afterSave` (`mailsync/Models/Thread.cpp:393-418`). Label counts only include mail also in All Mail (:235).
    - The client re-reads it, throttled to 1 s (`thread-counts-store.ts:20-42`).
    - Counts are conversations, not messages. 1.26 repaired drifted counts.
  - **Thunderbird and Apple Mail** count messages and use `STATUS` for folders they haven't opened.
- **Spec:** RFC 9051 §6.3.11 STATUS (`UNSEEN`, `MESSAGES`); RFC 5819 LIST-STATUS (`LIST "" "*" RETURN (STATUS (UNSEEN))` in one round trip).
- **Alice today:** Only the Inbox count, computed by running the whole `threads()` grouping with `limit 1` (`daemon.rs:366-382`), which is expensive. There is no per-folder count.
- **Plan:**
  1. **Counts endpoint.** `GET /v1/counts` → `[{account_id, folder|mailbox, unread, total}]`, computed with one SQL `GROUP BY account_id, folder` over `message_index` (`store.rs:1160-1162`, which already has folder, account and is_read).
     - Count **messages** for non-Inbox folders. Count conversations only for Inbox, to match the Unread tab.
     - Cache it per store `revision` (`store.rs:72-74`).
  2. **Folders outside the sync window or not synced:** take the server's `UNSEEN` from `STATUS` (or LIST-STATUS when advertised), fetched during the sync run, and store it in `mailbox_sync`. Show local counts when the folder is fully synced, server counts otherwise.
  3. **Desktop.** A setting "Show unread counts for all folders" (default on, unlike Mailspring). Show the per-account Inbox count beside each account and a unified Inbox total.
  4. **Performance.** After the P0 "move hot fields to indexed columns" item, this becomes a covering-index query.
- **Pitfalls:**
  - **Message counts vs conversation counts** confuse people. Be consistent per folder and label it in the tooltip.
  - **Gmail** labels overlap, so don't sum them into an account total.
  - **Spam/Trash counts** are usually hidden; Gmail shows a Spam count only.
  - **STATUS on the selected mailbox** is disallowed by some servers (RFC 9051 §6.3.11 says avoid it).
  - **Polling every 2 s** (`App.tsx:139`): only refetch when the revision changes.
- **Done when:** A store test seeds 3 folders and checks the counts. A fixture test checks that a folder outside the window shows the server UNSEEN. The UI shows counts updating after mark read.
- **Effort:** S–M. Depends on: "Sync every folder" (P0) and "Move hot fields … into real indexed columns" (P0) for speed.

<a id="one-click-unsubscribe-via-list-unsubscribe-list-unsubscribe-post-rfc-2369-rfc-80"></a>

### One-click unsubscribe via `List-Unsubscribe` / `List-Unsubscribe-Post` (RFC 2369, RFC 8058)

*Roadmap: P1 · Organization*

- **Why:** It is the fastest way to shrink inbox volume. Gmail, Apple Mail and Outlook all show it in the header, so users expect it.
- **Competitors:**
  - **Mailspring header extraction:** mailsync extracts `List-Unsubscribe` and `List-Unsubscribe-Post` only after the body is fetched (`mailsync/MailProcessor.cpp:510-521`) and stores them as `hListUnsub`/`hListUnsubPost`.
  - **Mailspring client logic** (`MS-app/internal_packages/list-unsubscribe/lib/unsubscribe-service.ts`):
    - Parses `<…>` URIs: https, http and mailto (:30-54).
    - Treats a message as one-click only if the Post header is exactly `List-Unsubscribe=One-Click` (:62-68).
    - POSTs `application/x-www-form-urlencoded` with body `List-Unsubscribe=One-Click`, `credentials: 'omit'` and `redirect: 'error'` (:82-127).
    - Priority order (:174-203): one-click; then mailto, which opens the composer for review (:137-146); then the web link in the browser; then a body-link heuristic with a multilingual regex list (`main.tsx:13-65, 79-111`, LRU cache of 150).
  - **Mailspring UI:** a link above the body (`message:BodyHeader`) with loading/success/error states (`unsubscribe-header.tsx:64-130`).
  - **Mailspring changelog:**
    - 1.9.0: body-link unsubscribe (#2275).
    - 1.10.7: verifies the URL opens in the default browser.
    - 1.17.2: header-based RFC 2369/8058 (#2576).
  - **Gap in Mailspring:** no DKIM check before one-click.
  - **Apple Mail:** a banner with an "Unsubscribe" button and a confirmation sheet. The mailto form is sent silently after confirming.
  - **Gmail:** an "Unsubscribe" link next to the sender, and "Manage subscriptions" listing senders by volume.
- **Spec:**
  - RFC 2369 §3.2: angle-bracketed, comma-separated URIs.
  - RFC 8058:
    - §3.1: the header must contain an HTTPS URI; the POST MUST NOT include cookies, HTTP auth or other context; the sender must not redirect.
    - §3.2: the receiver MUST NOT POST without user consent. The body is `List-Unsubscribe=One-Click`, sent as multipart/form-data (SHOULD) or x-www-form-urlencoded (MAY).
    - §4: at least one valid DKIM signature MUST cover both headers in `h=`, otherwise the receiver SHOULD NOT offer one-click.
    - §8: examples.
  - RFC 9979 §6.1 `$canunsubscribe` and §6.2 `$unsubscribed` keywords.
  - RFC 8601 §5: receiving MTAs remove forged `Authentication-Results` carrying their own authserv-id.
- **Alice today:**
  - Full raw headers are fetched (`BODY.PEEK[HEADER]`, `remote.rs:168-200`), but `message_from` keeps only selected fields (`providers.rs:340-424`). List-* headers are discarded.
  - Links open externally via `openEmailLink` (`apps/desktop/src/api.ts`).
  - mailto opens compose (`App.tsx:207-214`).
  - `reqwest` is a dependency already (workspace).
- **Plan:**
  1. **Model.** Add `#[serde(default)] unsubscribe: Option<Unsubscribe { https: Option<String>, mailto: Option<String>, one_click: bool, dkim_ok: bool }>` to `Message` (`model.rs:88-117`). Parse it in `message_from`, keeping the first https and first mailto URI. Also keep `list_id` (RFC 2919); the classifier and `list:` search need it.
  2. **DKIM, the lazy way.** Don't verify signatures. Set `dkim_ok` when an `Authentication-Results` header whose authserv-id matches the account's provider (`mx.google.com`, `*.outlook.com`, the IMAP host's domain) says `dkim=pass` with a `header.d` aligned to the From domain. Without that, offer only the browser or mailto path.
  3. **Daemon endpoint.** `POST /v1/messages/{id}/unsubscribe` dispatches on the method:
     - **One-click.** `reqwest` POST over HTTPS only, body `List-Unsubscribe=One-Click`, content type `application/x-www-form-urlencoded`, no cookie store, `redirect::Policy::none()`, 15 s timeout. Treat 2xx as success.
     - **mailto.** Return the parsed address, subject and body so the desktop opens a pre-filled compose. Later, an "auto-send after confirmation" option, as in Apple Mail.
     - **Afterwards.** Set `$unsubscribed` (when PERMANENTFLAGS allows) and a local flag, so the banner shows "Unsubscribed".
  4. **Desktop banner.** In `MessageView.tsx`, above the body: "Unsubscribe from <list or sender>".
     - Confirm dialog, then a spinner, then success or error with a fallback "Open unsubscribe page" button.
     - After success, offer "Archive all from this sender" or "Block sender".
     - Hide the banner in Spam: unsubscribing there confirms the address to spammers. Gmail shows "Report spam" instead.
  5. **No body-link heuristic at first.** It is a phishing vector. If added later, open in the browser only, labelled "Link found in the message".
- **Pitfalls:**
  - **Header shapes:** folded headers, multiple `List-Unsubscribe` headers, mailto `?subject=…&body=…` parameters, IDN hosts, `http://`-only (browser only).
  - **Redirects:** some ESPs answer 302 anyway. Mailspring treats that as an error; show "Open page" as the fallback.
  - **Privacy:** the POST reveals the user's IP. That's acceptable after explicit consent, and remote-image blocking doesn't apply.
  - **Never auto-unsubscribe.** Bulk unsubscribe needs per-sender consent.
  - **Phishing:** the header can carry a lookalike URL. Show the host in the confirm dialog.
- **Done when:**
  - Unit tests parse the RFC 8058 §8 examples and edge cases (mailto with parameters, two headers, bad brackets).
  - A local HTTPS mock (rustls test server or `axum` in `#[tokio::test]`) asserts the method, body, content type, no `Cookie` header and that a redirect is not followed.
  - The fixture message gets `unsubscribe.one_click == true` only when `Authentication-Results` passes.
  - The banner is hidden in Spam.
- **Effort:** M. Depends on: nothing hard. It shares header retention with "Split inbox / smart categories".

<a id="permanent-delete-from-trash-and-spam-empty-folder"></a>

### Permanent delete from Trash and Spam, empty folder

*Roadmap: P1 · Organization*

- **Why:** Trash and Spam grow forever on servers without auto-purge, eating quota. People also expect "Delete forever" for sensitive mail.
- **Competitors:**
  - **Mailspring:**
    - A bar appears only when the view is entirely Trash or Spam: "Showing %@ threads with deleted/spam messages" with an "Empty %@ now" button (`thread-list-empty-folder-bar.tsx:42-92`).
    - **No confirmation dialog**, and no per-thread delete forever: remove or swipe in Trash/Spam does nothing (`mailbox-perspective.ts:563-566`).
    - `ExpungeAllInFolderTask` sends `UID STORE 1:* +FLAGS (\Deleted)` then a **plain EXPUNGE** (`mailsync/TaskProcessor.cpp:2568-2591`). That also purges messages other clients only flagged `\Deleted`; 1.26 fixed exactly that for moves, but not here.
    - GitHub #57.
  - **Gmail:** "Delete forever" and "Empty Trash now", with confirmation.
  - **Apple Mail:** "Erase Deleted Items" with confirmation, plus automatic erase-after settings.
- **Spec:** RFC 9051 §6.4.6 STORE `\Deleted`, §6.4.3 EXPUNGE (removes **all** `\Deleted` messages); RFC 4315 / RFC 9051 §6.4.9 UID EXPUNGE (only the given UIDs).
- **Alice today:**
  - Moving to Trash only. Neither `discard` (`daemon.rs:342-346`, drafts only) nor any path expunges.
  - Gmail's OAuth scope `https://mail.google.com/` (`oauth.rs:27`) permits deletion.
- **Plan:**
  1. **Batch op.** `delete_forever` is allowed only when every message's folder is `trash` or `spam`. Send `UID STORE <set> +FLAGS.SILENT (\Deleted)` then `UID EXPUNGE <set>` (UIDPLUS), and never plain EXPUNGE. Without UIDPLUS: refuse, or EXPUNGE only if no other `\Deleted` messages exist (`UID SEARCH DELETED` is empty).
  2. **Empty folder.** `POST /v1/accounts/{id}/mailboxes/{trash|spam}/empty`: `UID SEARCH ALL` (includes mail outside the window), then the same STORE + UID EXPUNGE in chunks, then delete local rows.
  3. **UI.**
     - In Trash and Spam, "Delete forever" replaces "Move to Trash" in the toolbar and menu; Delete/Backspace maps to it there.
     - A list-header bar "Empty Trash (N)" uses N from `STATUS (MESSAGES)`.
     - Always confirm: "Permanently delete N messages? This cannot be undone." Not undoable.
  4. **CLI:** `alice delete --forever ID` and `alice empty trash`.
- **Pitfalls:**
  - **Gmail.** Expunging from `[Gmail]/Trash` or `[Gmail]/Spam` deletes permanently, which is fine. Don't try it from other Gmail mailboxes; there it only removes a label, depending on the user's "Auto-Expunge" settings.
  - **Outlook/O365** "Deleted Items" may have recoverable-items retention. Tell the user it may remain recoverable by an admin.
  - **Large Trash folders** (100k): chunk the work and show progress, and hold the account lock only per chunk.
  - **Never act on the spam folder for an empty `spam` mailbox name** (`model.rs:37-39`).
- **Done when:** Dovecot fixture: one message flagged `\Deleted` by "another client" in Trash survives "Empty Trash", and the rest are gone (`STATUS MESSAGES` = 1). A per-message delete-forever test checks the confirmation is required (the UI test).
- **Effort:** S. Depends on: Multi-select and bulk actions (batch endpoint).

<a id="block-sender-move-future-mail-to-spam"></a>

### Block sender (move future mail to Spam)

*Roadmap: P1 · Organization*

- **Why:** It is a direct fix for persistent unwanted senders that spam filters miss. Apple Mail and Outlook users expect a menu item.
- **Competitors:**
  - **Mailspring:** none (no block code).
  - **Apple Mail:** "Block Contact". Settings → Junk Mail → Blocked offers "Mark as blocked mail, but leave it in Inbox" or "Move it to the Trash". The list is shared with Phone and Messages through iCloud.
  - **Outlook:** "Block sender" adds the address to the server-side Blocked Senders list and moves mail to Junk. Graph v1.0 has no API for that list; it is exposed through Exchange PowerShell `Set-MailboxJunkEmailConfiguration` (verify before relying on it).
  - **Gmail web:** "Block <sender>" sends future mail to Spam; managed under Settings → Filters and Blocked Addresses. Via the Gmail API, filters cannot add `SPAM` (only `TRASH`, `STARRED` or a user label; `removeLabelIds: ['SPAM']` means "never spam").
- **Spec:** RFC 5228 Sieve (`if address :is "from" "x@y" { fileinto "Junk"; stop; }`) via ManageSieve RFC 5804 on servers that offer it. IANA `$Junk` keyword (set it when moving to Spam, so server-side learners such as Dovecot antispam or Fastmail train on it).
- **Alice today:** "Mark spam" moves a conversation to the Junk mailbox (`App.tsx:266`, `providers.rs:126`). Nothing recurring.
- **Plan:**
  1. **Table.** `blocked_senders(account_id NULL, pattern TEXT, kind 'address'|'domain', action 'spam'|'trash', created_at)` with endpoints `/v1/blocked` GET/POST/DELETE.
  2. **On arrival.** Apply it as a built-in first rule in the import hook (`crates/alice-core/src/sync.rs:572-574`): a match is moved via the batch path to Spam (or Trash), and no arrival or notification is recorded. The rules item reuses this hook.
  3. **Context menu.** "Block <address>" in the thread and message menus. It confirms, adds the entry, and moves that sender's existing conversations in the current view to Spam (batch).
  4. **Settings.** Mail accounts → Blocked senders, with a list and an unblock button.
  5. **Server-side when possible** (phase 2): Sieve via the rules export, and a Gmail filter `from:` → `TRASH` via the Gmail filters item. Otherwise it stays local, and other devices don't block. Say so.
  6. **Keywords.** Set `$Junk` and clear `$NotJunk` on Spam moves; reverse on "Not spam".
- **Pitfalls:**
  - **From spoofing** means blocking one address is weak. Offer a domain block, but refuse freemail domains (gmail.com, outlook.com) and the user's own domain.
  - **Mailing lists:** suggest Unsubscribe first.
  - **Contacts:** never block someone in the user's contacts without a second confirmation.
  - **Rules order:** blocks run before user rules, and before OTP autocopy.
- **Done when:** A unit test checks the address and domain matchers. A fixture test delivers mail from a blocked address and finds it in Junk after sync, with no `/v1/arrivals` entry. Unblock stops it.
- **Effort:** M. Depends on: Multi-select (batch) and the rules arrival hook. Server-side export depends on "Mail rules / filters" and "Sync Gmail filters…".

<a id="conversation-view-toggle-show-individual-messages-instead-of-conversations"></a>

### Conversation view toggle: show individual messages instead of conversations

*Roadmap: P1 · Organization · new*

- **Why:** It is the #1 Mailspring forum request by a wide margin (#291, 172 votes; also #2504). Alice's desktop only shows conversations, grouped by subject (README "Current boundaries"), which also merges unrelated mail with identical subjects.
- **Competitors:**
  - **Mailspring:** never implemented; threading is mandatory.
  - **Gmail:** Settings → "Conversation view" on/off.
  - **Apple Mail:** View → "Organize by Conversation".
  - **Outlook:** "Show as Conversations" per folder or for all.
  - **Thunderbird:** View → Threads → Threaded/Unthreaded/Grouped.
- **Alice today:**
  - `/v1/messages` already lists individual messages (`daemon.rs:384-389`, `store.rs:82-137`). The CLI and TUI use it.
  - The desktop only calls `/v1/threads` (`App.tsx:104-134`).
  - The reader renders `Conversation` (`apps/desktop/src/Conversation.tsx:20-47`), which works for a single message (`single` branch).
- **Plan:**
  1. Add `conversation_view: bool` (default true) to `Preferences` (`model.rs:311-337`) and `PreferencesPatch`, under Settings → Reading.
  2. **Off:** the desktop fetches `/v1/messages` and wraps each message as a one-message `Thread` (`id = message:<id>`, `ids=[id]`). The existing `message:` id convention for drafts (`store.rs:181-183`) shows that shape already works.
  3. **Reader:** show just that message, plus a "Show conversation (N)" link that loads `/v1/threads/{id}`.
  4. **Actions:** apply to that message only, which also covers the next item.
- **Pitfalls:**
  - **Sent copies:** with conversations off, a Sent copy of your reply only appears in Sent, as in other clients.
  - **Unread count semantics:** count messages, not conversations, when off.
  - **Keep the list paging** identical (both endpoints page by 50).
- **Done when:** Toggling the setting switches the list without a restart. Archiving in message mode moves only that message (fixture). Search works in both modes.
- **Effort:** S. Depends on: nothing; better after the P0 "Thread by Message-ID" item.

<a id="per-message-actions-inside-a-conversation-move-delete-archive-mark-one-message"></a>

### Per-message actions inside a conversation: move, delete, archive, mark one message

*Roadmap: P1 · Organization · new*

- **Why:** People need to delete one wrong reply or file one message from a long thread (Mailspring forum #320, 26 votes; GitHub #426, 27 👍).
- **Competitors:**
  - **Mailspring:** mostly missing. `ChangeFolderTask` accepts `messageIds` but the UI only offers thread actions, and `ChangeLabelsTask` rejects message-level changes (`change-labels-task.ts:103-104`).
  - **Gmail:** a per-message "⋮" menu with Delete this message, Mark unread from here, Report spam.
  - **Apple Mail and Thunderbird:** act on messages natively.
- **Alice today:** Right-clicking a message in the reader builds `threadMenu(thread, …)`, so its actions hit the **whole conversation** (`App.tsx:400`, `Conversation.tsx:28`).
- **Plan:**
  1. Add `messageActions(mail)` in `App.tsx`, mirroring `threadActions` but with `ids=[mail.id]`: Archive, Trash, Spam, Move…, Mark unread (Gmail's "Mark unread from here" marks it and all later messages), and Star.
  2. Use it in `menuFor`, plus a "⋯" button on each expanded message header (`Conversation.tsx:33-36`).
  3. After the action, refresh the conversation. If it is now empty, return to the list.
- **Pitfalls:**
  - **Gmail:** moving one message out of INBOX only changes that message's labels; the conversation may still show in Inbox through other messages.
  - **Subject-based grouping:** the conversation key is subject-based today (`store.rs:274-292`). After a move, the message may still appear in the conversation from its new folder. That is correct, but show its folder chip.
- **Done when:** Fixture: trash one of three messages → the conversation shows 2 in Inbox, and 1 appears in Trash.
- **Effort:** S. Depends on: Multi-select (batch) for consistency; "Move to any folder" for Move….

<a id="mark-as-read-delay-setting-instantly-after-05-s-after-2-s-or-manually"></a>

### Mark-as-read delay setting: instantly, after 0.5 s, after 2 s, or manually

*Roadmap: P1 · Organization · new*

- **Why:** Skimming with `j`/`k` currently marks every conversation passed over as read. Many users want "manually only".
- **Competitors:**
  - **Mailspring:** `core.reading.markAsReadDelay`, options `[0, 500, 2000, -1]` labelled Instantly / After 0.5 Seconds / After 2 Seconds / Manually, default **500** (`MS-app/src/config-schema.ts:130-141`). Applied in `message-store.ts:216-237`, which cancels the timer when focus moves; not undoable.
  - **Outlook:** "Mark items as read when viewed in the Reading Pane: wait N seconds" (default off).
  - **Apple Mail:** none.
- **Alice today:** `choose()` marks all `unread_ids` read immediately (`App.tsx:176-181`).
- **Plan:**
  1. Add `mark_read_delay_ms: i32` to `Preferences` (`model.rs:311-337`; values 0, 500, 2000, -1; default 500 to match Mailspring, or 0 to keep current behaviour).
  2. In `choose()`, start a timer instead of calling `change` directly. Clear it when `selectedId` changes.
  3. Keep the reader showing the "unread" styling until it fires (`setOpenedUnread` already tracks that).
  4. Settings → Reading select.
- **Pitfalls:** The TUI and CLI `read` command keep their explicit behaviour. Notification-opened messages follow the same delay.
- **Done when:** A UI test: with 2 s, pressing `j` three times quickly marks only the final conversation after 2 s; with "Manually", none are marked.
- **Effort:** S.

## P2: Productivity

<a id="snooze-with-natural-language-times-2d-monday-9am-backed-by-an-imap-folder-so-it"></a>

### Snooze with natural-language times ("2d", "monday 9am"), backed by an IMAP folder so it survives reinstalls

*Roadmap: P2 · Productivity*

- **Why:** Snooze turns the inbox into a to-do list ("deal with this Monday"). It is the most-used feature in Superhuman and Spark, and was one of Mailspring's paid hooks.
- **Competitors:**
  - **Mailspring folder** (`MS-app/internal_packages/thread-snooze`):
    - mailsync creates the folder `Mailspring/Snoozed` (container `Mailspring`, V1 `[Mailspring]`) **at folder sync for every account, whether or not snooze is used** (`mailsync/SyncWorker.cpp:844-913`, `constants.h:31-32`). It is matched to role `snoozed` by path (`MailUtils.cpp:380-398`). It is skipped when `create_helper_folders` is false (O365 shared mailboxes), which makes snooze unavailable there.
    - Users dislike this folder: forum #677 (20 votes) and GitHub #1262 "Remove Mailspring/Snoozed".
  - **Mailspring snooze action:**
    - Gmail: `ChangeLabelsTask` adds Snoozed and removes Inbox. IMAP: `ChangeFolderTask` moves only the Inbox copy (`snooze-utils.ts:36-56`).
    - Then `SyncbackMetadataTask` saves `{expiration}` under plugin id `thread-snooze` (`snooze-store.ts:35-75`).
  - **Mailspring wake:**
    - The local `MetadataExpirationWorker` emits `metadata-expiration` (`mailsync/MetadataExpirationWorker.cpp:61-125`). The client moves the thread back to Inbox and either marks it unread, or by default (`core.notifications.unsnoozeToTop`, true) **sends a hidden self-addressed email** "Mailspring Reminder: This thread has been moved to the top of your inbox" to bump it (`MS-app/src/flux/stores/draft-factory.ts:298-320`).
    - No wake happens while the app is closed; overdue snoozes fire about 15 s after launch.
    - A reply does not end the snooze early.
    - There is no manual "unsnooze" button.
    - Metadata roams only through Mailspring's identity server, and snooze requires a Mailspring ID plus a monthly quota (`feature-usage-store.tsx:112-150`).
  - **Mailspring presets** (`snooze-popover.tsx:9-21`, times in `MS-app/src/date-utils.ts:272-309`):
    - Later Today (now + 3 h, on the hour)
    - Tonight 20:00
    - Tomorrow 09:00
    - This Weekend (Sat 09:00)
    - Next Week (Mon 09:00)
    - Next Month (the 1st, 09:00)
  - **Mailspring custom field** "next Monday at 2PM" (`date-input.tsx:43-91`):
    - `expandDateLikeString` (`date-utils.ts:175-221`) rewrites shorthands: `600`→6:00, `2h`/`2d`/`2w`/`2m`, `t`/`tom` → "tomorrow morning", `nw` → "next Monday".
    - Then chrono-node casual with a future-biased refiner parses it (`date-utils.ts:103-137, 378-389`; `"chrono-node": "^2.9.0"`). A past date gives "Sorry, we can't parse %@ as a valid date."
  - **Mailspring shortcuts:** `z` (base), plus `h` in the Gmail preset, `ctrl+h` in Outlook, `mod+h` in Apple Mail (1.9.2).
  - **Mailspring changelog:**
    - 1.0.8: stuck in Snoozed; dates synced.
    - 1.1.2/1.1.3: folder naming with delimiters.
    - 1.2.2: crash with more than 200 expirations.
    - 1.7.0: `2d` shorthands.
    - 1.7.4: Brazil DST.
    - 1.12.0: `600` shorthand; label shows month and year.
  - **Possible Mailspring bug:** `snooze-store.ts:97-98` clears `expiration` for every thread in an expiration batch, not only the ones being woken.
  - **Gmail:** server-side snooze; returns early on reply; not exposed over IMAP.
  - **Fastmail:** server-side snooze, with the mailbox marked by the RFC 9979 `Snoozed` attribute.
  - **Superhuman:** `H` "Remind me" with natural language.
- **Spec:**
  - RFC 9979 §8.1 `Snoozed` mailbox attribute: identifies where snoozed messages wait, but "does not itself define the snoozing mechanism".
  - RFC 9979 §7.7 `$new` (emphasise a message returning from snooze).
  - RFC 5464 METADATA: per-server or per-mailbox `/private/vendor/...` entries. It cannot annotate a message. Advertised by Dovecot (with `mail_attribute_dict`) and Cyrus; not Gmail or Outlook.
  - RFC 6851 MOVE keeps INTERNALDATE, so other clients keep sorting a woken message in its old position.
- **Alice today:**
  - Nothing. README: "Automatic classification, snooze … are not included yet".
  - The background loop (`sync.rs:222-243`) and `Notify` wake (`sync.rs:138-140`) are the pattern a scheduler would follow.
- **Plan:**
  1. **Scheduler.** New `crates/alice-core/src/schedule.rs`.
     - Table `jobs(id, account_id, kind 'snooze'|'send'|'remind', due_at INTEGER, payload TEXT, state 'pending'|'done'|'failed', attempts)`.
     - One tokio task: sleep until `MIN(due_at)` or until a `Notify` fires on insert. On daemon start, run anything overdue.
     - Execute under `state.hold(account_id)` (`daemon.rs:61`).
     - Shared by send later, reminders and undo send.
  2. **Storage design.** It responds to the #677 complaint.
     - **(a) Default, "server folder, created on first use".** On the first snooze in an account, reuse a mailbox with the RFC 9979 `Snoozed` attribute or named `Snoozed`; otherwise `CREATE "Snoozed"` (top level, no `Mailspring/` container) and SUBSCRIBE. Never create it at account setup.
     - On snooze, `UID MOVE` the conversation's Inbox copies to Snoozed. On Gmail, MOVE from INBOX to the `Snoozed` label removes `\Inbox` and adds the label, so no labels support is needed.
     - Other devices and webmail then see it out of the Inbox, and the folder contents survive a reinstall.
     - **(b) Option "Keep snoozed mail in the Inbox on the server".** A local-only hide with no server change. Other clients still show the mail.
     - **Wake times** live in `jobs` locally (phase 1).
       - A message found in Snoozed without a local job, after a reinstall or on a second device, shows in the "Snoozed" view as "No wake time set — Wake now / Reschedule". The folder contents are the source of truth.
     - **Phase 2, roaming wake times:** store `{message-id: due_at}` as JSON in `SETMETADATA "Snoozed" (/private/vendor/alice/snooze …)` when the server advertises METADATA. Otherwise use a single hidden state message in Snoozed, with header `X-Alice-State: snooze`, replaced with APPEND-new then UID EXPUNGE-old.
     - **Don't encode times in keywords.** Dovecot maildir allows only 26 keywords per mailbox and Cyrus 128, and keyword lists never shrink.
  3. **Wake.** `UID MOVE` back to the recorded return mailbox (usually INBOX), then mark unread.
     - Set `$new` if keywords are allowed, and a local `resurfaced_at` that the list sort uses as `max(date, resurfaced_at)`.
     - Post a desktop notification "Snoozed conversation is back".
     - **No self-sent bump emails.** That is Mailspring's default, and it adds junk messages to the thread.
  4. **Early wake.** In the import hook, a new message in a snoozed conversation (P0 Message-ID threading) wakes it, as Gmail does. Make this a setting, default on.
  5. **Time parser.** `crates/alice-core/src/when.rs`, shared with search dates (past-biased) and send later (future-biased). Hand-roll about 120 lines on `chrono`, which is already a dependency:
     - shorthands `2h 3d 1w 1m` and `600`/`6pm`/`18:00`
     - `today`, `tonight`, `tomorrow [morning|evening|9am]`
     - weekday names (next occurrence)
     - `next week|month`, `weekend`
     - ISO dates; `in N hours|days`
     - English first; German weekday and month names, since Alice ships `de`
     - Morning and evening hours configurable (defaults 09:00 / 20:00).
     - Alternative: the `interim` crate (a chrono-english fork, MIT). Decide after trying the 120-line version.
  6. **API and UI.**
     - `POST /v1/messages/batch {ids, snooze_until}` and `DELETE` to unsnooze; `folder=snoozed` in list queries.
     - A sidebar "Snoozed" item, shown only once used, with wake times on each row.
     - Snooze popover: Mailspring's 6 presets plus a free-text field with a live preview ("Mon 14 Oct, 09:00").
     - Shortcut `z` (and `h` in the Gmail preset); the command palette entry "Snooze…".
     - CLI: `alice snooze ID 2d`.
- **Pitfalls:**
  - **Multi-device double wake:** the MOVE fails harmlessly once the UID is gone, so treat "no such UID" as done.
  - **The device that holds the job is off at the due time:** mail stays snoozed until it starts. Show that clearly ("Wakes when Alice is running").
  - **DST and time zones:** compute in `chrono::Local` and store UTC (Mailspring 1.7.4 bug).
  - **Folder deleted by the user:** jobs whose message is gone are dropped.
  - **Gmail Snoozed label vs Gmail's own snooze:** don't touch `[Gmail]/Snoozed` if it appears.
  - **The Snoozed folder must be excluded from the classifier, rules and notifications.**
- **Done when:**
  - `when.rs` unit tests with a fixed `now`: `2d`, `600`, `monday 9am` on a Monday (next week), `tom`, and a DST boundary.
  - Fixture: snooze → message in `Snoozed` on Dovecot → advance the clock (inject `now` into the scheduler) → back in INBOX and unread.
  - Restart the daemon between snooze and wake to confirm persistence.
  - The snooze folder is not created until first use.
- **Effort:** L. Depends on: Multi-select (batch), keyword support (section findings 6), "Thread by Message-ID…" (P0) for early wake, and "Launch at login" (P1) for reliability.

<a id="send-later-with-send-now-instead"></a>

### Send later, with "send now instead"

*Roadmap: P2 · Productivity*

- **Why:** People write at night and send in the morning, and work across time zones. All competitors have it.
- **Competitors:**
  - **Mailspring:**
    - Draft metadata `{expiration}` under plugin `send-later` (`MS-app/internal_packages/send-later/lib/send-later-button.tsx:99-101`).
    - At expiry the client's main window clears it and calls `Actions.sendDraft` (`send-later/lib/main.ts:26-71`).
    - It needs the app running; overdue sends go out about 15 s after launch.
    - Drafts are never synced to the server (`mailsync/TaskProcessor.cpp:922-923`), so only the scheduling device sends.
    - Presets: In 1 hour, In 2 hours, Later Today, Tomorrow Morning (09:00), Tomorrow Evening (20:00), This Weekend, Next Week, plus custom (`send-later-popover.tsx:5-13`).
    - UI: "Unschedule Send"; drafts-list status "Scheduled for %@" with an X "Cancel Send Later" (`send-later-status.tsx:52-99`).
    - "Send now instead" exists in the **undo-send** toast (1.10.6, `undo-redo-toast.tsx:22-37`). Undo send reuses the same metadata with `isUndoSend` (`draft-store.ts:519-630`).
    - Quota-gated at first schedule.
    - Changelog: 1.4.0 undo-send countdown; 1.20.0 draft expiration races; 1.22.0 "Send now" crash.
    - Forum #391 asks for server-side send later.
  - **Gmail web:** schedule send runs server-side; not available via IMAP or the Gmail API.
  - **Spark and Superhuman:** use their own servers.
  - **Outlook:** "Delay delivery" is held server-side on Exchange, and in the local Outbox for IMAP accounts.
- **Spec:**
  - RFC 9979 §8.2 `Scheduled` mailbox attribute (where queued messages live).
  - RFC 5550 `$SubmitPending` / `$Submitted` keywords.
  - RFC 7162 §3.1.3 `UNCHANGEDSINCE`: compare-and-set for a cross-device "I'm sending this" claim.
- **Alice today:**
  - Immediate send only: `POST /v1/messages/{id}/send` (`daemon.rs:472-497`).
  - At-most-once delivery via `claim_delivery`/`finish_delivery` (`store.rs:399-460`).
  - The composer calls send directly (`apps/desktop/src/Compose.tsx:73-78`).
- **Plan:**
  1. **Endpoints.** `POST /v1/messages/{id}/send` accepts an optional `{at: RFC3339}`. With `at`, insert a `jobs` row (`kind='send'`) and return the draft with `scheduled_at` (new `#[serde(default)]` field on `Message`). `DELETE /v1/messages/{id}/schedule` cancels. `POST …/send` without `at` while scheduled means "send now instead".
  2. **Scheduler run.** Call the existing `send` body (refactor `daemon.rs:472-497` into a function), so `claim_delivery` still guarantees at most once.
  3. **Editing.** Opening a scheduled draft in the composer pauses it; saving asks "Keep schedule?".
  4. **UI.**
     - The Compose send button gets a menu: "Send later…" with presets and the natural-language field from `when.rs`.
     - A "Scheduled" sidebar view (drafts with `scheduled_at`): "Send now", "Cancel", "Edit".
     - The toast after scheduling: "Scheduled for Tue 09:00 · Undo".
  5. **Late sends.** Send overdue jobs on start. If one is more than 10 minutes late, notify "Sent at 09:42 (Alice wasn't running at 09:00)".
  6. **Phase 2, roaming.** Once drafts are APPENDed to the server (P0), add header `X-Alice-Send-At`. Before sending, claim with `UID STORE <uid> (UNCHANGEDSINCE <modseq>) +FLAGS ($SubmitPending)`. A `MODIFIED` response means another device claimed it. After sending, set `$Submitted` and delete the draft. Without CONDSTORE, only the scheduling device sends.
- **Pitfalls:**
  - **The daemon must be running** (P1 "Launch at login"). Say so in the popover.
  - **Undo send** (P1 Composer) is the same job with `due = now + delay`, so build them together and avoid two code paths. Mailspring's history shows the race bugs (1.4.0, 1.20.0).
  - **Account deleted or token revoked** before the due time: mark the job failed and notify.
  - **Time zones:** show the absolute time with zone.
  - **A sent copy** must appear in Sent at the actual send time, so set the `Date` header at send time, not schedule time.
- **Done when:**
  - A scheduler test with an injected clock: schedule +1 h → not sent → advance → sent exactly once, even with two concurrent ticks.
  - Restart persistence.
  - Cancel returns the draft to Drafts.
  - A fixture SMTP capture checks that the `Date` header equals the send time.
- **Effort:** M. Depends on: the scheduler from the Snooze item, and "Undo send with a configurable delay" (P1), which should share it.

<a id="follow-up-reminders-remind-me-if-nobody-replies-by-a-date"></a>

### Follow-up reminders: remind me if nobody replies by a date

*Roadmap: P2 · Productivity*

- **Why:** Chasing unanswered requests is a common sales and management workflow, and it otherwise needs a to-do app.
- **Competitors:**
  - **Mailspring data:** `send-reminders`; metadata keys `expiration`, `shouldNotify`, `sentHeaderMessageId`, `lastReplyTimestamp` (`send-reminders-utils.ts:45-62`).
    - Set from the composer: stored on the draft under `thread:send-reminders`, moved to the thread at send (`mailsync/TaskProcessor.cpp:2490-2504`).
    - Set from the toolbar: offered only when the last message is from the user (`send-reminders-popover-button.tsx:23-43`).
  - **Mailspring reply detection:** any change of `lastMessageReceivedTimestamp` versus the baseline clears the reminder (`send-reminders-store.ts:40-92`). Changelog 1.24.1: reminders created from drafts never fired.
  - **Mailspring at expiry:** sends the hidden self-email as a reply to bump the thread, and sets `shouldNotify` for a highlighted row and a "Reminder" header. A "Reminders" sidebar view lists pending ones.
  - **Mailspring presets:** In 3 Hours, Tomorrow Morning, Tomorrow Evening, In 3 Days, In 1 Week, In 2 Weeks, In a Month (`send-reminders-popover.tsx:6-14`). Requires a Mailspring ID and quota.
  - **Superhuman:** "Remind me if no reply" on the snooze picker.
  - **Apple Mail:** "Remind Me" (resurface a message at a time) and automatic "Follow Up" suggestions that bring sent mail with no reply back to the top after about 3 days.
- **Alice today:** Nothing. Sent mail is cached locally (README; `store.rs` deliveries), and `in_reply_to` is stored per message (`model.rs:110`).
- **Plan:**
  1. **Job.** `jobs` `kind='remind'` with payload `{sent_message_id, thread_key, account_id, baseline_date}`.
     - Created from the composer's "Remind me if no reply ▸" (stored with the draft, activated by a successful send).
     - Or from the thread menu when the newest message is from the user.
  2. **Reply detection.** In the import hook (`sync.rs:572`), any new message from someone else whose `In-Reply-To`/`References` contains `sent_message_id`, or which is in the same conversation, cancels the job. This needs `References` (P0 "Correct MIME for replies" stores it). Until then, use the subject key with a sender ≠ me.
  3. **At expiry,** with no server change:
     - add a local `reminded_at` to the conversation;
     - Inbox queries include conversations with `reminded_at` not null, even if all copies are in Sent/Archive;
     - show a "No reply yet · set Mon 9 Oct" chip and post a notification.
     - Opening the conversation clears `reminded_at`.
  4. **A "Reminders" view** lists pending jobs with "Cancel".
  5. **Presets** as in Mailspring, plus the `when.rs` free text.
- **Pitfalls:**
  - **Auto-replies** (out of office, `Auto-Submitted: auto-replied`, RFC 3834) must not count as replies.
  - **Replies to an alias** or from another of your own accounts.
  - **Replies the user sent from another device:** your own message shouldn't cancel the reminder; only others' should.
  - **Reinstall** loses reminders (local only). That is acceptable, but document it.
- **Done when:** Fixture: send with "remind in 1 min" (injected clock) → no reply → the conversation appears in Inbox with the chip. A second run with a reply (and one with only an auto-reply) checks cancellation.
- **Effort:** M. Depends on: the scheduler (Snooze item) and "Thread by Message-ID / References…" (P0).

<a id="templates-with-subject-and-quick-insert-in-the-composer"></a>

### Templates with subject, and quick-insert in the composer

*Roadmap: P2 · Productivity*

- **Why:** It removes the retyping of recurring replies (support, recruiting, sales). Mailspring forum #435 asked for subjects; #434 for snippets.
- **Competitors:**
  - **Mailspring storage:**
    - Templates are plain HTML files in `<config>/templates` (`MS-app/internal_packages/composer-templates/lib/template-store.ts:30`), watched with `fs.watch` (:66-70), with "Show Templates Folder…" in Preferences → Templates.
    - The subject is stored as `<meta name="subject" content="…"/>` on the first line (`template-file.ts:1-12, 41-64`), since 1.24.0 (#2794).
  - **Mailspring variables:** `<code class="var empty">` regions made with the toolbar's Tag button; Tab jumps between them. Send warns "The message contains an empty template area." (`template-composer-extension.ts:4-9`).
  - **Mailspring insertion** (`template-store.ts:319-371`):
    - It replaces the body **above** the signature or quote, finding the first of `<signature`, `gmail_quote_attribution` or `blockquote.gmail_quote`.
    - It asks "Replace draft contents?" when the draft isn't pristine.
    - It never changes the subject of a reply or forward.
  - **Mailspring save and pick:**
    - "Save draft as template" uses the draft subject as the name (:156-182).
    - The picker in the composer footer filters by name and subject (`template-picker.tsx:29-40`).
  - **Mailspring changelog:** 1.1.0 editor; 1.7.6 first-line style; 1.24.0 subject.
  - **Thunderbird:** templates are messages in a Templates IMAP folder, so they roam.
  - **Gmail:** templates are stored as drafts.
  - **Superhuman:** Snippets (`;` trigger, `{first_name}` variables).
- **Alice today:**
  - Plain-text compose with a signature (`model.rs:323`, `draft_body` `model.rs:362-372`; `Compose.tsx`).
  - Compose has no rich text (README).
- **Plan:**
  1. **Storage and API.** Table `templates(id, name, subject, body, format 'plain'|'html', updated_at)` with `/v1/templates` CRUD. Store plain text now and HTML when rich compose lands.
  2. **Variables.** `{{first_name}}`, `{{name}}`, `{{email}}` (first To recipient), `{{my_name}}` and `{{cursor}}`, resolved on insert. Anything still matching `\{\{[^}]+\}\}` at send time triggers a warning, Mailspring's check adapted to plain text.
  3. **Insert behaviour.** Replace the body above the `-- \n` signature delimiter (the one `draft_body` writes) and above quoted text. Set the subject only for new messages; confirm if the subject or body is non-empty.
  4. **UI.**
     - A composer toolbar button "Templates" with a type-ahead list (name and subject).
     - Quick insert: typing `;;name` in the body, or `Mod+;`, opens the same list at the caret.
     - "Save as template" in the composer menu.
     - Settings → Writing → Templates editor (new, rename, delete).
  5. **Phase 2.** Roam via a `Templates` IMAP folder (Thunderbird-compatible), now that drafts APPEND exists (P0).
- **Pitfalls:**
  - **Replies:** don't overwrite the `Re:` subject (Mailspring guards this).
  - **Signature duplication.**
  - **Variables with several recipients:** use the first, and show a preview.
  - **HTML templates** will need sanitising (dompurify is already a dependency) when rich text arrives.
- **Done when:** Unit tests cover variable substitution and the unresolved-placeholder warning. Composer tests: inserting keeps the signature, and a reply's subject is untouched.
- **Effort:** S–M. Depends on: "Rich text / HTML compose" (P1) for HTML templates; "Save drafts to the server" (P0) for roaming.

<a id="mail-rules-filters-match-on-from-to-subject-body-reply-to-list-actions-move-labe"></a>

### Mail rules / filters: match on from, to, subject, body, Reply-To, list; actions move, label, mark, star, forward, delete; run on arrival and on demand

*Roadmap: P2 · Productivity*

- **Why:** It is the power-user feature for high-volume inboxes. Mailspring forum requests: import/export rules (#130, 10 votes), global rules (#575), sync rules between computers (#1893), Sieve editor (#1122).
- **Competitors:**
  - **Mailspring model** (`MS-app/src/flux/stores/mail-rules-store.ts:21-44`):
    - `MailRule {id, accountId, name, disabled, disabledReason, conditionMode 'any'|'all', conditions[{templateKey, comparatorKey, value}], actions[{templateKey, value}]}`.
    - Stored in renderer **localStorage** under `MailRules-V2`, so it never syncs.
    - Per account: Preferences → Mail Rules, "Create a new Rule".
  - **Mailspring conditions** (`MS-app/src/mail-rules-templates.ts:5-104`): from, to, cc, bcc, anyRecipient, replyTo (1.12.0), anyAttachmentName, hasAttachment, starred, subject, body.
  - **Mailspring comparators** (`scenario-editor-models.ts:44-127`): contains, doesNotContain (every item, 1.5.2), beginsWith, endsWith, equals (1.8.0 matches empty), matchesExpression (`new RegExp(v,'gi')`).
  - **Mailspring actions:**
    - markAsRead, moveToTrash, star, forward (1.9.2).
    - Gmail adds moveToLabel, applyLabel, archive and markAsImportant; folder accounts add changeFolder (:106-174).
    - No unstar, mark unread, spam or "stop processing".
  - **Mailspring on arrival:**
    - mailsync emits `rulesReady` once per message after its body is stored and it has an incoming copy (`mailsync/Models/Message.cpp:378-397, 526-557`).
    - The client filters to messages dated after first launch (`MailRules-Auto-Since`) and not sent by the user, then runs rules in order (`mail-rules-processor.ts:189-293`).
    - Actions become tasks with `canBeUndone=false`.
    - An error disables the rule with `disabledReason`, shown in a notification bar "One or more of your mail rules have been disabled."
    - Possible duplicate runs: rules may run once per open window (no main-window guard).
  - **Mailspring "Process entire inbox":**
    - Pages 50 inbox threads at a time, newest first, using a keyset (`lessThan` timestamp, so moves don't shift offsets).
    - Waits 500 ms between batches, with a progress line and a Stop button.
    - It is refused when any rule has a body condition (`preferences-mail-rules.tsx:88-127`).
  - **Mailspring changelog:** 1.0.6; 1.6.1 runs after the body is fetched (#605); 1.18.0; 1.24.1 folder ids changed, so it now matches by unambiguous name; 1.26.0 self-addressed mail.
  - **Thunderbird:**
    - When filters run: on new mail before or after junk classification, manually, after sending, on archiving, or periodically.
    - Conditions can test any header.
    - Actions include tag (keyword), reply with template, and stop filter execution.
  - **Outlook:** server-side rules (Graph `messageRule`: conditions, exceptions, actions, `sequence`, `isEnabled`, `hasError`; `mailFolders/inbox/messageRules`).
- **Spec:**
  - RFC 5228 Sieve: `require`, `if`/`elsif`, `fileinto`, `redirect`, `keep`, `discard`, `stop`; tests `header`, `address`, `envelope`; `:is`/`:contains`/`:matches`.
  - Extensions: RFC 5232 imap4flags, RFC 5490 mailbox (`fileinto :create`), RFC 5229 variables, RFC 6609 include.
  - RFC 5804 ManageSieve: port 4190, `LISTSCRIPTS`/`PUTSCRIPT`/`CHECKSCRIPT`/`SETACTIVE`/`HAVESPACE`, SASL.
  - RFC 5550 `$Forwarded`.
  - Graph `messageRule` (learn.microsoft.com/en-us/graph/api/resources/messagerule).
- **Alice today:**
  - Nothing.
  - The arrival hook exists: `SyncEngine` calls `added` with each batch of newly imported messages (`sync.rs:60, 572-574`). Today it only feeds arrivals (`daemon.rs:46-50, 125-150`).
  - Bodies are downloaded during sync (`remote.rs` `contents`), so body conditions work at arrival, unlike Mailspring's wait.
- **Plan:**
  1. **Model.** `Rule {id, account_id: Option, name, enabled, query: String, actions: Vec<RuleAction>, stop: bool, disabled_reason: Option<String>}` in table `rules(id, position, data)`.
     - **The condition is an Alice search query** evaluated by `search::Matcher` (`search.rs:351-420`). That gives AND/OR/NOT, from/to/subject/body, has:attachment and dates.
     - Extend the grammar with `cc:`, `reply-to:`, `list:` (List-Id) and `header:"X-Foo:bar"`.
     - The editor builds the query from rows (field / comparator / value, with "any"/"all"), as Mailspring does, and offers "Edit as query".
  2. **Actions:** mark read/unread, star/unstar, move to mailbox, add label (Gmail), trash, spam, forward to an address, stop.
     - Executed through the batch path (server-side changes) and recorded as not undoable.
     - Forward builds a message via `providers::build_mail` and `deliver`. **Before forwarding,** check for the `$Forwarded` keyword (RFC 5550) and set it after, so two Alice installs never forward twice.
  3. **On arrival.** In the `added` hook, before arrivals are recorded so a rule-archived message doesn't notify:
     - Run only on messages whose UID is at or above the previous UIDNEXT (`plan.floor`, `sync.rs:241-252`).
     - Only in the Inbox or the All Mail Inbox copy; skip sent, drafts, spam and trash, and messages from the account itself.
     - Never during the first sync or backfill; store a per-account `rules_since` timestamp, like Mailspring's Auto-Since.
  4. **On demand.** `POST /v1/rules/run {rule_id?, account_id, folder: "inbox"}` starts a job that pages 50 conversations newest first by keyset (julian date, id) and reports progress (`GET /v1/rules/run/{job}`), with Stop. Body conditions work because bodies are cached, but text over 4 MB is truncated.
  5. **Errors.** A permanent failure (mailbox missing, invalid regex) disables the rule with a reason and posts a notification. Transient network errors retry on the next sync.
  6. **UI.** Settings → Rules: an ordered list with drag reorder, an enable toggle, the row editor, "Run on Inbox now", and import/export as JSON (forum #130). CLI: `alice rules list|add|run`.
  7. **Phase 2, server-side export.**
     - **ManageSieve servers** (probe port 4190 STARTTLS): compile the expressible rules to Sieve and upload a script `alice` with `PUTSCRIPT`/`CHECKSCRIPT`. Only `SETACTIVE` if no other script is active; otherwise show the generated `include :personal "alice";` line for the user to add (RFC 6609).
     - **Gmail:** export via the Gmail filters item.
     - **Outlook:** export via Graph `messageRules` (needs `MailboxSettings.ReadWrite`).
     - Rules that can't be expressed stay local.
     - Generate Sieve as text; no crate is needed. If parsing is needed, `sieve-rs` (Stalwart) is AGPL-3.0, which is compatible with Alice's `AGPL-3.0-only` (`Cargo.toml:10`). No mature Rust ManageSieve client exists, and the protocol is line-based, about 200 lines over the existing `native-tls`.
- **Pitfalls:**
  - **Duplicate execution** across devices (two Alice installs): moves and flags are idempotent, forwards are not. Use `$Forwarded`, or prefer server-side rules.
  - **Rules racing the user:** a message the user already opened or moved before the rule ran. Run inside the account lock, immediately after import.
  - **Renamed folders break rules** (Mailspring 1.24.1). Store the mailbox name plus the special-use role, and re-resolve by role.
  - **Regex:** use the linear-time `regex` crate (not a dependency yet), or drop `matches` and keep `contains`/`is`.
  - **OTP autocopy should still see codes** even when a rule archives the message.
  - **Notifications** must reflect the post-rule folder.
- **Done when:**
  - `rules.rs` unit tests: query match, ordering, `stop`, action plan.
  - Fixture: deliver a message from `news@example.test` → a rule moves it to `Newsletters` on the server before any arrival is emitted.
  - The reprocess job pages correctly while moving (keyset).
  - Forward dedupe: run twice → one forward, with `$Forwarded` set.
  - A disabled-reason case.
- **Effort:** L (local), plus L (server-side export). Depends on: Multi-select (batch), "Move to any folder…", keyword support, and the Search item (new fields).

<a id="sync-gmail-filters-and-vacation-responder-through-the-gmail-api"></a>

### Sync Gmail filters and vacation responder through the Gmail API

*Roadmap: P2 · Productivity*

- **Why:** Gmail users already have dozens of server filters. Editing them and the vacation reply in-app avoids the web UI, and the filters run while the computer is off.
- **Competitors:**
  - **Mimestream:** Settings → Filters tab plus "Filter Messages Like These…" in the message context menu. The filters are Gmail's own, so they are "synchronized with Gmail, and run on the server". The vacation responder is editable per account (mimestream.com/help/user-guide/account-settings).
  - **Mailspring:** none; its rules are local only.
- **Spec:**
  - Gmail API `users.settings.filters`:
    - Methods: `create`, `get`, `list`, `delete`. **There is no update**, so editing is delete plus create and changes the id. Max **1,000** filters.
    - Criteria: `from`, `to`, `subject`, `query`, `negatedQuery`, `hasAttachment`, `excludeChats`, `size`, `sizeComparison`.
    - Action: `addLabelIds`, `removeLabelIds`, `forward`.
    - Documented adds are `TRASH`, `STARRED` or one user label. `SPAM` is only valid in `removeLabelIds` ("never send to spam").
    - Source: developers.google.com/workspace/gmail/api/reference/rest/v1/users.settings.filters.
  - Vacation: `GET`/`PUT users/me/settings/vacation` (`enableAutoReply`, `responseSubject`, `responseBodyPlainText`, `responseBodyHtml`, `restrictToContacts`, `restrictToDomain`, `startTime`, `endTime` in epoch ms).
  - Both require scope `https://www.googleapis.com/auth/gmail.settings.basic`, classed **Restricted**: it needs restricted-scope verification, and a security assessment only if data is stored or sent through servers (developers.google.com/workspace/gmail/api/auth/scopes).
  - Label id ↔ name mapping needs `users.labels.list` (`gmail.labels`, non-sensitive).
  - The `forward` action requires an already-verified forwarding address. Adding one needs `gmail.settings.sharing`, which is Workspace service accounts only.
- **Alice today:**
  - Gmail OAuth requests `https://mail.google.com/` plus optional contacts and calendar scopes through `Integrations` (`oauth.rs:27-35, 63-78`; `model.rs:56-64`).
  - The incremental-consent endpoint is `/v1/accounts/{id}/integrations` (`daemon.rs:264`).
  - `reqwest` is in the tree.
- **Plan:**
  1. **Consent.** Add `Integrations.settings` with scopes `gmail.settings.basic` and `gmail.labels`, granted per account in Settings → Mail accounts, like contacts and calendar.
  2. **Client.** `crates/alice-core/src/gmail_settings.rs` with `list_filters`/`create_filter`/`delete_filter` and `get_vacation`/`put_vacation` against `https://gmail.googleapis.com/gmail/v1/users/me/settings/...`, plus `labels.list` for names.
  3. **Endpoints.** `GET/POST/DELETE /v1/accounts/{id}/gmail/filters` and `GET/PUT /v1/accounts/{id}/vacation`.
  4. **UI.**
     - Gmail account settings get "Filters" (a list rendering criteria as text; New/Delete; edit = replace) and "Vacation responder" (toggle, dates, subject, body, contacts-only).
     - The thread menu "Filter messages like these…" pre-fills `from:`.
  5. **Rules integration.** For Gmail accounts, the rules editor offers "Run on Gmail's servers" when a rule maps:
     - criteria → from/to/subject/query;
     - actions → archive (`removeLabelIds: INBOX`), read (`UNREAD`), star (`STARRED`), trash (`TRASH`), label, never-spam.
  6. **Vacation for other providers** (later):
     - Sieve `vacation` (RFC 5230) via ManageSieve.
     - Outlook: Graph `mailboxSettings.automaticRepliesSetting` (`MailboxSettings.ReadWrite`).
- **Pitfalls:**
  - **Restricted-scope verification** can take weeks. Ship it behind the per-account toggle, and keep the build working without it.
  - **Display:** filters created in Gmail web carry `query` strings with Gmail operators that Alice's parser doesn't know. Display them raw.
  - **Vacation times** are UTC milliseconds.
  - **HTML vs plain body:** set both.
  - **Token refresh** with the new scope: existing tokens lack it, so check the granted scopes (`oauth.rs:92-105` pattern).
  - **Quota:** filter list calls are cheap, but don't poll; refresh on open.
- **Done when:** A mocked HTTP test (axum test server) covers list → model, create payload shape, delete, and vacation round trip. The UI test is a manual check against a test Gmail account.
- **Effort:** M. Depends on: "Mail rules / filters" for the shared editor (optional).

<a id="saved-searches-smart-folders"></a>

### Saved searches / smart folders

*Roadmap: P2 · Productivity*

- **Why:** It is the most requested "organization without folders" feature (Mailspring forum #528, 20 votes; GitHub #514, 11 👍).
- **Competitors:**
  - **Mailspring:** none.
  - **Thunderbird:** Saved Search folders: criteria over chosen folders, optional online search.
  - **Apple Mail:** Smart Mailboxes: any/all criteria, include Trash/Sent toggles, synced via iCloud.
  - **Superhuman:** custom splits are saved queries.
- **Alice today:**
  - List queries already evaluate `q` within folder, account and category scope (`docs/api.md:61-63`, `store.rs:1207-1219`).
  - Sidebar "Collections" are placeholders (`App.tsx:351-352`).
- **Plan:**
  1. Table `saved_searches(id, name, query, account_id NULL, folder NULL, position, notify bool)` with `/v1/searches` CRUD.
  2. A sidebar section "Saved searches" replaces the sample Collections. Clicking one sets `query` and scope. Show an unread count from `ThreadPage.unread_total`, fetched only when the store `revision` changes.
  3. "Save search" button in the search field when `q` is non-empty, with rename and delete in the context menu.
  4. CLI `alice list --saved NAME`; MCP `search_mail` accepts a saved-search name.
  5. Option to pin a saved search as an Inbox tab (feeds "Split inbox").
- **Pitfalls:**
  - **Relative dates** (`after:7d`) are re-evaluated each time, which is what people want; say so.
  - **Cost:** counts for N searches at every refresh. The Rust matcher scans the scoped rows (`store.rs:1218`). Cap at about 20 and compute lazily.
  - **Local only** for now; roaming via a settings message later.
- **Done when:** Store CRUD tests. A saved `is:unread from:x` count equals the ad-hoc query count. A UI flow covers save → appears in sidebar → click → results.
- **Effort:** S–M. Depends on: the Search item (`in:`).

<a id="search-natural-language-dates-in-not-autocomplete-find-by-sender-subject-from-th"></a>

### Search: natural-language dates, `in:`, `NOT`, autocomplete, "find by sender / subject" from the context menu

*Roadmap: P2 · Productivity*

- **Note:** Alice already has `NOT`, `!`, `-`, `OR`, parentheses, `from:`/`to:`/`subject:`/`body:`, `is:`, `has:attachment`, and `after:`/`before:`/`date:` with `today`, `yesterday`, ISO dates and ages `7d 2w 3m 1y`, with typo tolerance (`search.rs:1-12, 217-330`). That is already ahead of Mailspring. What remains: natural-language dates, `in:`, autocomplete, the context-menu searches, and `cc:` once Cc is stored.
- **Why:** Search is how people find old mail. Autocomplete teaches the operators.
- **Competitors:**
  - **Mailspring grammar** (`MS-app/src/services/search/search-query-parser.ts`, a hand-written recursive descent parser):
    - Operators: `from:`, `to:` (to+cc+bcc), `subject:`, `is:read|unread|starred|unstarred`, `has:attachment`, `since:`/`after:`/`before:`, `in:` (folder role or path).
    - Implicit AND; OR binds tighter than AND.
    - `NOT` is **binary only**: `a NOT b`. A leading NOT fails (1.7.5), and there is a precedence bug (:344-353).
    - No `label:`, `cc:` or `-term`.
    - A parse failure falls back to an FTS prefix search.
    - Queries exclude Trash and Spam unless mentioned, with a "Search messages in trash and spam" button (`search-mailbox-perspective.tsx:39-71`).
  - **Mailspring dates:** chrono-node casual, past-biased (`date-utils.ts:139-172`). Hints: "yesterday", "2 days ago", "last week", "last month", ">=2018/05/31". An unparseable date likely yields malformed SQL.
  - **Mailspring autocomplete** (`search-bar-util.ts:22-129`):
    - `from:`/`to:` suggest 5 contacts.
    - `in:` suggests up to 8 folders by role.
    - Free text over 2 characters merges 10 subject suggestions and contacts.
    - Focusing the bar prefills the current view (`in:inbox` etc.).
  - **Mailspring context menu:** "Search for <email>" (a quoted generic term) and "Search for <subject>" (`subject:"…"`, unescaped) (`thread-list-context-menu.ts:84-104`).
  - **Mailspring changelog:** 1.0.10 `in:` any folder; 1.2.2 `subject:`; 1.3.0 new bar with autocomplete, natural dates and right-click search; 1.5.4 `has:attachment`; 1.7.5 `NOT`.
- **Alice today:**
  - Parser and matcher are in `search.rs`. `period()` handles only today, yesterday, ages and ISO (`search.rs:268-330`).
  - The folder comes only from the request scope (`scope_filter`, `store.rs:1108-1137`).
  - `to:` covers only To, because Cc isn't stored (`model.rs:94`).
  - The search field is a plain input with a 180 ms debounce (`App.tsx:155`).
  - Contact suggestions endpoint: `/v1/contacts?q=` (`daemon.rs:266`, `model.rs:508-517`).
- **Plan:**
  1. **Dates.** Route `after`/`before`/`date` values through the shared `when.rs` parser (past-biased): `monday`, `last week|month|year`, `this week|month`, `march`, `march 2025`, `3 days ago`. **Only inside date operators**, never free text.
  2. **`in:`.** Add `Expr::Folder(String)`, matched against the `message_index.folder` column (`store.rs:1160`).
     - When a query contains `in:`, the daemon drops the request's folder scope. Today's default exclusion of Spam (`store.rs:1129-1131`) stays unless `in:spam`.
     - After P0 multi-folder, `in:` matches any mailbox or label, and `label:` is an alias.
  3. **Other fields.** `cc:`, and `reply-to:`/`list:` (stored by the Unsubscribe item), are added to `search_projection` (`store.rs:1142-1146`). Bump `SEARCH_INDEX_VERSION` (`store.rs:1139`).
  4. **Autocomplete.** A dropdown under the search input (`App.tsx` header). It parses the token under the caret:
     - `from:`/`to:` → `/v1/contacts`;
     - `in:` → folder list;
     - `is:`/`has:` → fixed values;
     - date keys → example phrases;
     - empty → operator cheatsheet, which already exists in Settings (`Settings.tsx:148`).
     - Arrows, Enter and Escape.
  5. **Context menu.** Add "Find mail from <sender>" (`from:"addr"`) and "Find conversations with this subject" (`subject:"…"`, escaping `"` with `\"`, which the tokenizer supports: `search.rs:76-81`) to `threadMenu` (`App.tsx:274-277`) and to message menus.
  6. **CLI and TUI** share the grammar automatically. Update the docs (`docs/api.md:63-…`, Settings cheatsheet).
- **Pitfalls:**
  - **Ambiguous words** ("may", "march") as free text: parse dates only in date operators.
  - **Locale week start.**
  - **`in:` combined with the UI's folder tab:** show a "Searching all folders" hint.
  - **Typo tolerance must not apply to `in:` values.**
  - **Index rebuild cost** on upgrade for large stores (Mailspring 1.2.2 needed indexer fixes).
- **Done when:** `search.rs` tests with a fixed `now` for each new date form, `in:` overriding scope, and `cc:`. A UI test: typing `from:so` shows contacts, and Enter completes. The context-menu item produces the expected query.
- **Effort:** M. Depends on: "Cc and Bcc fields" (P0) for `cc:`; "Sync every folder" (P0) for `in:` beyond the role folders.

<a id="server-side-search-fallback-imap-search-gmail-x-gm-raw-for-mail-outside-the-sync"></a>

### Server-side search fallback (IMAP SEARCH, Gmail `X-GM-RAW`) for mail outside the sync window

*Roadmap: P2 · Productivity*

- **Why:** The default window is 90 days. Without server search, older mail is unfindable in Alice even though it is on the server.
- **Competitors:**
  - **Mailspring:** none.
    - `performRemoteSearch()` is an empty TODO, "local search is good enough for v1" (`MS-app/internal_packages/thread-search/lib/search-query-subscription.ts:78-86`).
    - `IMAPSearchQueryBackend` compiles to SEARCH keys but is never called, and throws on dates (`search-query-backend-imap.ts:135-137`).
    - mailsync has no SEARCH or X-GM-RAW.
    - Mailspring caches bodies only for 3 months (`mailsync/SyncWorker.cpp:1496-1506`).
  - **Thunderbird:** "Search on server" in the search dialog and saved searches.
  - **Apple Mail:** automatically searches IMAP servers and shows "Searching server…".
- **Spec:**
  - RFC 9051 §6.4.4 SEARCH: `FROM`, `TO`, `CC`, `SUBJECT`, `BODY`, `TEXT`, `SINCE`, `BEFORE`, `SEEN`/`UNSEEN`, `FLAGGED`, `KEYWORD`, `HEADER`, `OR`, `NOT`. IMAP4rev2 requires UTF-8 charset support, and `CHARSET UTF-8` works on most rev1 servers.
  - RFC 4731 ESEARCH (`RETURN (COUNT MAX)`).
  - RFC 9394 PARTIAL for paged results.
  - Gmail `X-GM-RAW "<gmail query>"` with the same syntax as the web UI (developers.google.com/workspace/gmail/imap/imap-extensions).
- **Alice today:**
  - `UID SEARCH SINCE` is already used for the window (`remote.rs:138-147`).
  - Header and content fetch helpers exist (`remote.rs:168-236`).
  - Search is local only (`store.rs:1207`).
- **Plan:**
  1. **Translate the query.** `search::to_imap(&Expr) -> Option<String>`:
     - Text → `TEXT`/`FROM`/`TO`/`SUBJECT`/`BODY`; `is:` → `SEEN`/`UNSEEN`/`FLAGGED`; dates → `SINCE`/`BEFORE` with `d-Mon-yyyy`; `OR`/`NOT`.
     - `has:attachment` → `HEADER Content-Type multipart/mixed` (approximate).
     - Return `None` if something can't be expressed.
     - For Gmail, translate to Gmail syntax for `X-GM-RAW` (from/to/subject/`after:YYYY/MM/DD`/`is:unread`/`has:attachment`).
  2. **Endpoint.** `GET /v1/search/remote?account_id&q&mailbox?`:
     - Gmail: run one `UID SEARCH X-GM-RAW` in `[Gmail]/All Mail`. Others: run it in the selected folder, or Inbox+Archive+Sent.
     - Drop UIDs already cached (`known_uids`, `sync_store.rs:322`) and take the newest 50 by UID.
     - Fetch headers and text via `Remote::headers`/`contents`, and return transient `Message`s with `remote_only: true`. **Do not store them**, or the window pruning will delete them on the next sync.
     - Opening one fetches its body on demand, as attachments are fetched today (`daemon.rs:521-575`).
  3. **UI.** After local results, a row "Search on server for older mail" (automatic when local results are fewer than 10). Results are tagged "On server". Actions (archive, move) work through `remote_mailbox`/`uid`.
  4. **Concurrency.** Hold the account lock briefly, and use `SyncEngine::pause` (`sync.rs:129`) so search doesn't wait behind a backfill.
- **Pitfalls:**
  - **Non-ASCII queries** need `CHARSET UTF-8` and literals; the `imap` crate's `uid_search` takes a string, so build a literal-safe command.
  - **O365 BODY/TEXT search** is slow and can time out; set a 30 s timeout and show partial results.
  - **Some servers** (Yahoo) cap results.
  - **Gmail X-GM-RAW quoting.**
  - **Result identity:** the same message may later arrive through sync. Key on Message-ID so it isn't shown twice.
- **Done when:**
  - Unit tests for `to_imap` (including NOT/OR nesting and unsupported → `None`).
  - Fixture: a message dated 2 years ago on Dovecot with a 30-day window is found by server search, opened, then archived.
  - Gmail is a manual check.
- **Effort:** M. Depends on: "Two long-lived IMAP connections per account" (P0) for speed; the Search item for the grammar.

<a id="command-palette-for-every-action"></a>

### Command palette for every action

*Roadmap: P2 · Productivity*

- **Why:** It makes every action discoverable and teaches shortcuts. Superhuman's Cmd+K is its signature feature.
- **Competitors:**
  - **Superhuman:** Cmd/Ctrl+K "Superhuman Command" lists every action with its shortcut, including settings (e.g. "Split Inbox Library") and "Shortcuts" (help.superhuman.com/article/478-superhuman-command).
  - **Mailspring:** none; it uses menus. Its Cmd+K is "insert link" in the composer.
- **Alice today:**
  - Commands are spread across `shortcuts.ts:20-49`, `goTo`, `threadActions` (`App.tsx:253-272`) and the list menu (`App.tsx:376`).
  - `Mod+K` and `/` focus search (`shortcuts.ts:22`).
- **Plan:**
  1. **Registry.** A `commands()` function in `App.tsx` returns `MenuItem & {id, group}` for:
     - global actions (compose, sync, settings sections, assistant, go to each folder);
     - the current thread or selection (`threadActions`);
     - dynamic entries: "Move to <folder>" per mailbox, saved searches, snooze presets, "Switch account".
  2. **`CommandPalette.tsx`.** A modal with a text input and filtered list.
     - Subsequence or fuzzy matching on folded labels; recently used first (localStorage).
     - Shows `label(shortcut)` at the right; Enter runs; Escape closes.
     - Reuse `ContextMenu`'s keyboard handling (`ContextMenu.tsx:115-125`).
  3. **Binding.** `Mod+K` opens the palette and `/` keeps focusing search, matching Superhuman. "Search mail…" is the palette's first entry, so muscle memory still works.
  4. **Accessibility.** `role="dialog"` with `aria-activedescendant` on a `listbox`.
- **Pitfalls:**
  - **Commands disabled** for the current state show greyed with the reason.
  - **The palette must not open over the composer**, which has its own `Mod+K` for links once rich text exists. Scope the shortcut.
  - **i18n:** labels come from `t()`; match on both the localized and English labels.
- **Done when:** A unit test checks the fuzzy ranking. A UI test: `Mod+K`, type "arch", Enter → the conversation is archived; disabled commands are not runnable.
- **Effort:** S–M. Depends on: nothing; better after the shortcut-presets registry.

<a id="shortcut-presets-gmail-outlook-superhuman-and-remapping"></a>

### Shortcut presets (Gmail, Outlook, Superhuman) and remapping

*Roadmap: P2 · Productivity*

- **Why:** Switchers keep their muscle memory: "e" archives in Gmail, Ctrl+Q marks read in Outlook.
- **Competitors:**
  - **Mailspring layering** (`MS-app/keymaps/README.m*`): base.json, then platform files, then a **template** that *replaces* each command it mentions, then the user `keymap.json` that replaces again.
    - Default template: **Gmail** (`config-schema.ts:108-111`, `core.keymapTemplate`).
    - Templates: Gmail, Outlook, Apple Mail, Inbox by Gmail.
    - Mousetrap gives key sequences (`g i`, `* a`).
    - Preferences → Shortcuts: preset dropdown, every binding editable, Restore Defaults; sections Application, Actions, Composer, Navigation, Jumping, Selection.
  - **Mailspring Gmail preset** (`MS-app/keymaps/templates/Gmail.json`):
    - Navigation: `g i` inbox, `g u` unread, `g s` starred, `g t` sent, `g d` drafts, `g a` all, `g c`/`g k`/`g l` (no-ops), `j`/`k`, `o`/Enter open, `u`/Esc back, `n`/`p` next/previous message, `;`/`:` expand/collapse all.
    - Selection: `x` select; `* a`/`* n`/`* r`/`* u`/`* s`/`* t`.
    - Compose and reply: `c`/`d` compose; `r`, `a` reply all, `f` forward (Shift = new window).
    - Actions: `e` archive, `#` delete, `!` spam, `s` star, `z`/`h` snooze, `l` label, `v` move, `m` mute (no handler exists), `y` remove from view, `]`/`[` remove and next/previous (also `}`/`{`), `Shift+I`/`Shift+U` read/unread, `+`/`=` and `-` important/not, `.` more, `,` toolbar, `/` search, `?` help, `mod+z` undo.
  - **Mailspring Outlook preset:**
    - Read/unread and flags: `ctrl+q` read, `ctrl+u` unread, `ctrl+shift+g` flag, `ctrl+h`/`z` snooze, `mod+shift+v` move.
    - Search and find: `f3`/`mod+e` search, `mod+shift+f`/`f4` find in thread.
    - Messages: `mod+f` forward, `mod+d` delete, `mod+r` reply, `mod+shift+r` reply all, `mod+n`/`mod+shift+m` new, `alt+s` send, `mod+z`/`alt+backspace` undo.
    - Navigation: `mod+shift+l` inbox, `mod+shift+o` sent.
  - **Mailspring Apple Mail preset:**
    - Navigation: `cmd+ctrl+1…8` go-to, `cmd+[`/`cmd+]` previous/next.
    - Messages: `mod+r`/`mod+shift+r`/`mod+shift+f` reply, reply all, forward; `mod+shift+d` send.
    - Actions: `mod+shift+j` spam, `mod+shift+u` unread, `mod+shift+l` star, `cmd+ctrl+a` archive, `mod+h`/`z` snooze.
    - Search: `mod+alt+f`.
  - **Mailspring base:** `mod+z` undo, `mod+shift+z`/`mod+y` redo, `mod+1…9` select account, `f5` sync, `z` snooze, `mod+p` print, `backspace`/`del` remove from view (`base.json`).
  - **Mailspring changelog:**
    - 1.5.3: Outlook Ctrl-Q.
    - 1.6.0: Outlook Ctrl-F conflict; find moved to Ctrl-Shift-F.
    - 1.7.2: Ctrl as a modifier on macOS.
    - 1.9.2: snooze and spam shortcuts.
    - 1.20.1 and 1.21.0: global shortcuts firing while typing in the composer.
    - 1.24.0: Outlook read/unread on Windows.
    - 1.24.1: Windows shows Ctrl/Alt instead of `mod`.
    - Forum #579: can't add an alternative shortcut when editing.
  - **Superhuman** (Cmd+K → Shortcuts for the full list; key subset from third-party lists, verify): `E` done/archive, `H` remind/snooze, `J`/`K`, `Enter`, `R`, `A` reply all, `F` forward, `C` compose, `/` search, `U` unread, `S` star, `#` trash, `!` spam, `L` label, `V` move, `G` then `I` inbox.
- **Alice today:**
  - Fixed Gmail-like bindings in `shortcuts.ts:20-49`; `Mod+1…7` for folders.
  - `matches()` supports Mod/Shift, rejects Alt, and has no sequences (`shortcuts.ts:58-67`).
  - Settings shows a read-only list (`Settings.tsx:22-25, 148`).
- **Plan:**
  1. **Ids.** The keys of `shortcuts` and `goTo` become command ids. Add the missing commands: `undo`, `select`, `moveTo`, `label`, `snooze`, `forward`, `replyAll`, `markRead`, `markUnread`, `nextMessage`, `prevMessage`, `goUnread`.
  2. **Presets as data.** In `shortcuts.ts`: `presets: {gmail, outlook, apple, superhuman}` with per-command replace semantics, as in Mailspring. "Alice default" is the current set.
  3. **Storage.** `Preferences.keymap {preset: String, overrides: Map<id, Vec<String>>}` (`model.rs:311`; it is `deny_unknown_fields`, so add the field explicitly) so it survives reinstall with the daemon data.
  4. **Sequences and Alt.** Extend `matches()` into a small state machine with a pending prefix (`g`, `*`) and a 1.5 s timeout, showing a "g…" hint.
     - Support `Alt` by comparing `event.code` (Option changes `event.key` on macOS).
  5. **Settings → Keyboard shortcuts.**
     - A preset select.
     - Per row: "Add shortcut" (record next keypress or sequence), remove; allow multiple alternatives (forum #579).
     - Conflict detection within a scope (list, reader, composer).
     - Restore defaults.
  6. **TUI.** Keeps its own keys (`crates/alice/src/tui.rs:469-560`); out of scope.
- **Pitfalls:**
  - **Typing guard:** single keys must not fire while typing. The existing guard is `App.tsx:285-287`; Mailspring regressed here twice (1.20.1, 1.21.0).
  - **OS-reserved keys:** don't allow `Mod+H` (macOS Hide) or `Mod+Q`.
  - **Keyboard layouts:** `#` and `!` are shifted symbols; keep the current "symbols ignore Shift" rule (`shortcuts.ts:5-7`).
  - **Windows labels:** show Ctrl, never "mod" (Mailspring 1.24.1).
- **Done when:**
  - Unit tests for `matches()` with sequences, Alt and the timeout.
  - Each preset table has no in-scope conflicts (a test iterating presets).
  - UI: switch preset → archive key changes; an override persists across restart.
- **Effort:** M. Depends on: the command registry (Command palette item).

<a id="go-to-unread-and-other-g-prefixed-navigation"></a>

### "Go to unread" and other `g`-prefixed navigation

*Roadmap: P2 · Productivity*

- **Why:** Keyboard users jump between views without the mouse. `g u` was added in Mailspring 1.25.
- **Competitors:**
  - **Mailspring:** `navigation:go-to-*` commands (`MS-app/src/flux/stores/focused-perspective-store.ts:61-79`).
    - inbox, sent, starred, drafts and all are real.
    - `go-to-unread` sets an Unread perspective (1.25.0; View menu "Go to Unread").
    - **`go-to-contacts`, `go-to-tasks` and `go-to-label` are TODO no-ops,** though the Gmail preset binds `g c`/`g k`/`g l`.
  - **Gmail:** `g` + `i`/`s`/`t`/`d`/`a`/`l` (label picker)/`k` (tasks).
- **Alice today:**
  - `Mod+1…7` go to folders (`shortcuts.ts:45-49`).
  - An Unread tab exists in the list (`unread` query flag).
  - The TUI binds `g` already (`tui.rs:494`).
- **Plan:**
  1. Built on the sequence engine (presets item), bind `g i` inbox, `g u` inbox with the unread filter, `g s` starred, `g t` sent, `g d` drafts, `g a` archive/All Mail, `g p` spam, and `g z` snoozed.
  2. Make `g l` / `g f` real: open the folder picker in "go" mode, jumping to any mailbox or saved search (unlike Mailspring's no-op).
  3. Add the commands to the palette and the View menu (Tauri menu).
- **Pitfalls:** A pending `g` must not swallow the next key if the sequence fails. Show the hint and fall through after the timeout.
- **Done when:** UI test: `g` then `u` shows Inbox Unread; `g` then a non-bound key does nothing harmful.
- **Effort:** S. Depends on: Shortcut presets (sequence support).

<a id="send-and-archive"></a>

### Send and archive

*Roadmap: P2 · Productivity*

- **Why:** Inbox-zero users answer and file in one step.
- **Competitors:**
  - **Mailspring:** a composer send action "Send and Archive", available only when the draft has a `threadId` (`MS-app/internal_packages/send-and-archive/lib/send-and-archive-extension.ts:5-24`). It queues the send, then archive tasks for the thread.
  - Preference "Default send behavior" (`core.sending.defaultSendType`: `send` | `send-and-archive`; `config-schema.ts:306-312`), added in 1.9.0.
  - Hidden for new messages since 1.5.4.
  - **Gmail:** the "Send and archive" button setting.
  - **Superhuman:** sends and marks done by default.
- **Alice today:** Send posts `/v1/messages/{id}/send` (`Compose.tsx:73-78`). Replies carry `in_reply_to` (`model.rs:228`). Archiving a conversation is `move(row, 'archive')` (`App.tsx:224-229`).
- **Plan:**
  1. Add `default_send: send|send_and_archive` to `Preferences`.
  2. In the composer, when `in_reply_to` is set, offer a split button "Send and archive" with `Mod+Shift+Enter`.
  3. After a successful send, archive that conversation's **Inbox** copies only (the listed `ids` filtered to `folder=='inbox'`) via the batch path.
  4. With Undo send or Send later, archive when the send actually happens. The scheduler job carries `archive_thread: true`.
- **Pitfalls:**
  - **Send failure** → don't archive.
  - **Gmail** archive = MOVE INBOX → All Mail, which is fine.
  - **A conversation the user re-opened in another folder** (no Inbox copies) → no-op.
- **Done when:** Fixture: reply with send-and-archive → the reply is in Sent, and the conversation's Inbox copies are in Archive.
- **Effort:** S. Depends on: Undo send / Send later for correct ordering, if present.

<a id="pin-conversations-to-the-top"></a>

### Pin conversations to the top

*Roadmap: P2 · Productivity*

- **Why:** It keeps a few active conversations visible above new mail without starring everything.
- **Competitors:**
  - **Mailspring:** none. Forum #2716 asks for Spark-style "New / Pinned / Seen".
  - **Spark:** "Pin" puts mail in a Pins section of Smart Inbox. Pins map to the server's flag/star, so they show as flagged elsewhere (verify in Spark help).
  - **Apple Mail:** the todo tag says AM; current macOS Mail offers flags and VIPs rather than a documented pin, so treat it as unverified.
- **Alice today:** Starred exists (`is_starred`; `folder=starred` view, `store.rs:1119-1124`). The list is sorted by date only.
- **Plan:**
  - **Recommended, lazy version:** "Pin" = star, plus an Inbox option "Show starred conversations at the top", which sorts by `is_starred DESC, date DESC` within the Inbox query. That needs no new state and roams via `\Flagged`.
  - **If a separate pin is wanted:** a local `pinned(thread_key, pinned_at)` table with a "Pinned" section header. This doesn't roam unless a private keyword is used.
- **Pitfalls:**
  - **Paging:** pinned items must appear on page 1 even if old. Do it in SQL ordering, not client-side.
  - **Too many pins** defeat the purpose; cap the section at about 10 with "Show all".
- **Done when:** A store test checks ordering with the option on. The UI shows a pinned section.
- **Effort:** S.
- **Note:** Consider merging into Star, as Spark does, to avoid a fourth flag concept alongside star, snooze and set-aside.

<a id="mute-conversation"></a>

### Mute conversation

*Roadmap: P2 · Productivity*

- **Why:** It stops noisy reply-all threads without leaving them.
- **Competitors:**
  - **Gmail:** `m` mutes. Future replies skip the Inbox unless the user is the only addressee. Exposed only through search (`is:muted`), not as a documented IMAP label.
  - **Outlook:** "Ignore Conversation" moves the conversation and future replies to Deleted Items.
  - **Thunderbird:** Ignore Thread (`k`) and Watch Thread (`w`), both local.
  - **Mailspring:** the Gmail preset binds `core:mute-conversation` to `m`, but **no handler is registered anywhere** (grep of `MS-app/src` and `internal_packages`), so the key does nothing.
- **Spec:** RFC 9979 §7.6 `$muted` (thread-level; "servers may reduce prominence of new messages in it") and §7.2 `$followed`.
- **Alice today:** Nothing. Threads are grouped by subject (`store.rs:139-273`).
- **Plan:**
  1. Mute sets `$muted` on the conversation's messages when PERMANENTFLAGS allows (roams; Fastmail understands it). Always add a local `muted(account_id, thread_key)` row.
  2. **On arrival** (rules hook): a new message in a muted conversation, where the user is not the sole To recipient, is archived immediately (stays unread, as in Gmail), and no arrival or notification is recorded.
  3. UI: thread menu "Mute"/"Unmute", `m` in the Gmail preset, a muted icon in the conversation header, and search `is:muted`.
- **Pitfalls:**
  - **Subject grouping would mute unrelated same-subject mail.** Ship this only after P0 Message-ID threading, or match on `References` directly in the hook.
  - **Direct mail** (sole recipient) must still surface.
  - **Gmail's own mute** isn't visible over IMAP, so the two are independent; say so.
- **Done when:** Fixture: mute a thread → a reply arrives with the user in Cc → it lands in Archive, unread, with no notification. A reply addressed only to the user stays in Inbox.
- **Effort:** S–M. Depends on: "Thread by Message-ID…" (P0), the rules arrival hook, and keyword support.

<a id="imap-keywords-as-tags-with-label-colors"></a>

### IMAP keywords as tags, with label colors

*Roadmap: P2 · Productivity · new*

- **Why:** Non-Gmail users want Gmail-like labels without folders (Mailspring forum #540, 15 votes; GitHub #1006). Everyone wants colors (forum #658; GitHub #526, 18 👍).
- **Competitors:**
  - **Mailspring:** labels only for Gmail.
  - **Thunderbird:** tags are IMAP keywords. Defaults `$label1…$label5` (Important, Work, Personal, To Do, Later) with colors kept in local prefs; keys 1–9 toggle tags.
  - **Apple Mail:** 7 flag colors encoded in `$MailFlagBit0-2` (RFC 9979 §3.1).
  - **Fastmail:** shows keywords as labels.
  - **Gmail:** label colors only via the Gmail API (`labels.color`), not IMAP.
- **Spec:** RFC 9051 §2.3.2 (keywords) and §7.1 PERMANENTFLAGS `\*`; IANA IMAP/JMAP keywords registry; RFC 9979 §3.1 flag colors.
- **Alice today:** Only `\Seen` and `\Flagged` are read (`remote.rs:33-37, 419-426`).
- **Plan:**
  1. Store `keywords: Vec<String>` per message from `FETCH FLAGS` during sync. Apply changes in `apply_flags` (`sync_store.rs:362`).
  2. `MessagePatch.add_keywords`/`remove_keywords` → `UID STORE ±FLAGS`.
  3. A tag catalogue in Preferences `{keyword, name, color}`. Map Thunderbird's `$label1-5` names by default.
  4. Show tag chips in the list and reader; `tag:` in search; a Tag picker (reuse the move picker UI).
  5. Flag colors: write `$MailFlagBit*` with `\Flagged`, for Apple Mail interop.
- **Pitfalls:**
  - **Servers without `\*`** in PERMANENTFLAGS (some Exchange setups): hide tagging.
  - **Keyword limits:** Dovecot maildir allows 26 per mailbox.
  - **Keyword syntax:** no spaces, `(`, `)`, `{`, `%`, `*`, `"`, `\`, `]`; map display names to safe atoms.
  - **Gmail:** keep tags off and use labels there.
- **Done when:** Dovecot fixture: tag → `FETCH FLAGS` shows the keyword; a tag set by Thunderbird (`$label1`) appears as "Important".
- **Effort:** M. Depends on: keyword plumbing (section findings 6).

<a id="auto-advance-after-archive-or-delete-with-a-setting"></a>

### Auto-advance after archive or delete, with a setting

*Roadmap: P2 · Productivity · new*

- **Why:** Keyboard triage flow: after `e`, the next conversation should open (forum #493).
- **Competitors:**
  - **Mailspring:** the commands "Remove and show next" / "Remove and show previous" (Thread menu; Gmail `]`/`[`; `thread-toolbar-buttons.tsx:132-135`). There is no setting to make plain archive auto-advance.
  - **Gmail:** Settings → Auto-advance (newer / older / back to list).
  - **Outlook:** "After moving or deleting an item: open the next item / open the previous item / return to the current folder".
- **Alice today:** After a move, refresh keeps the selection if the thread still exists. Otherwise, in split layouts it selects the **first row** of the list, and in single-pane it returns to the list (`App.tsx:133`). That is neither next nor previous.
- **Plan:**
  1. Add `after_move: next|previous|list` to `Preferences`.
  2. In `move()` (`App.tsx:224-229`), remember the neighbour before the request and `choose()` it after success.
  3. Add `]`/`[` "archive and next/previous" to the Gmail preset.
- **Pitfalls:** Neighbour on page boundaries; with multi-select, use the row after the last selected.
- **Done when:** UI test: archive the 3rd of 5 → the 4th opens (next) or the 2nd (previous).
- **Effort:** S.

<a id="edit-as-new-resend"></a>

### Edit as new / resend

*Roadmap: P2 · Productivity · new*

- **Why:** It resends a sent message to someone else, or reuses a past email as a starting point (forum #456).
- **Competitors:**
  - **Mailspring:** none.
  - **Thunderbird:** "Edit As New Message" (Ctrl+E).
  - **Apple Mail:** "Send Again" (Shift+Cmd+D).
  - **Outlook:** "Resend This Message".
- **Alice today:** `createCompose(mail)` builds replies only (`App.tsx:207-214`). Compose has no attachments yet (P0).
- **Plan:** Add a message menu item "Edit as new" that opens Compose with the same subject (no `Re:`), body, To and Cc (for sent mail) or empty To (for received mail), no `in_reply_to`, and the original attachments once forwarding attachments works (P0 "Forward, with original attachments carried over").
- **Pitfalls:** Strip `Message-ID`, `References` and tracking headers. HTML bodies need the rich composer.
- **Done when:** The menu item on a Sent message opens a prefilled draft that sends as a new thread.
- **Effort:** S. Depends on: "Forward, with original attachments carried over" (P0) for attachments.

<a id="sidebar-customization-hide-show-reorder-and-favorite-folders"></a>

### Sidebar customization: hide, show, reorder and favorite folders

*Roadmap: P2 · Productivity · new*

- **Why:** Accounts with many folders bury the ones people use (forum #469, #1904, #688).
- **Competitors:**
  - **Mailspring:** fixed order; requests open.
  - **Thunderbird:** Folder pane modes (All, Unified, Favorite, Recent, Unread) and drag reorder (115+).
  - **Apple Mail:** a Favorites section with drag reorder.
  - **Outlook:** Favorites, and show/hide.
- **Alice today:** The folder list is hardcoded (`App.tsx:20-22`).
- **Plan:** Add `Preferences.sidebar {hidden: [mailbox ids], order: [...], favorites: [...]}`, with a context menu Hide/Add to favorites, drag reorder within a section, and "Show hidden folders" at the bottom.
- **Pitfalls:** Folders renamed on the server lose their customisation; key on the special-use role where possible.
- **Done when:** A hidden folder stays hidden after restart, and reorder persists.
- **Effort:** S–M. Depends on: "Sync every folder…" (P0).

<a id="select-by-state-select-all-read-unread-starred-unstarred"></a>

### Select by state: select all read, unread, starred, unstarred

*Roadmap: P2 · Productivity · new*

- **Why:** Quick bulk clean-up, such as "select all read → archive".
- **Competitors:**
  - **Mailspring:** the Edit menu "Select All Read/Unread/Starred/Unstarred" (`MS-app/menus/darwin.js:72-75`, `thread-list.tsx:127`).
  - **Gmail:** `* r`, `* u`, `* s`, `* t`.
- **Plan:** Add four commands on the multi-select state that select the visible rows matching `unread_ids.length` or `is_starred`, plus palette entries and the Gmail preset keys.
- **Effort:** S. Depends on: Multi-select and bulk actions.

<a id="move-messages-between-accounts"></a>

### Move messages between accounts

*Roadmap: Merged into another item · new*

- **Why:** Consolidating mailboxes when changing providers is a common request (forum #244, 37 votes; GitHub #580, 25 👍).
- **Competitors:**
  - **Mailspring:** not supported (product-direction).
  - **Thunderbird and Apple Mail:** drag between accounts (copy or move).
- **Spec:** RFC 9051 §6.3.12 APPEND (with flags and INTERNALDATE), §6.4.5 FETCH `BODY.PEEK[]`.
- **Alice today:** Moves are per-account `UID MOVE` (`providers.rs:124-170`). Full raw fetch exists for attachments (`remote.rs:308-362`).
- **Plan:** For each message, fetch `BODY.PEEK[]`, then `APPEND` to the destination account's mailbox with the original flags and INTERNALDATE, verify it via APPENDUID (RFC 4315), then delete from the source (MOVE to Trash, or `\Deleted` + UID EXPUNGE for a true move). Show progress for large batches, and offer "Copy" as well as "Move".
- **Pitfalls:**
  - **Partial failure:** never delete the source until the APPEND is confirmed.
  - **Size limits** on APPEND (Gmail about 25 MB).
  - **Gmail labels** are lost.
  - **Duplicates** if retried; check Message-ID in the destination first.
- **Done when:** Fixture with two Dovecot users: move 3 messages → present in the destination with flags and dates intact, gone from the source.
- **Effort:** M. Depends on: Multi-select (batch) and the folder picker.

<a id="custom-sort-order-by-sender-subject-size-unread-first-oldest-first"></a>

### Custom sort order: by sender, subject, size, unread first, oldest first

*Roadmap: P3 · new*

- **Why:** Forum #470 (21 votes) and GitHub #361. Sorting by sender clears newsletters fast.
- **Competitors:**
  - **Mailspring:** date only.
  - **Thunderbird:** View → Sort by date, from, subject, size, starred, unread, tags or attachments, ascending or descending, per folder.
  - **Apple Mail:** a sort menu (date, from, size, subject, to, unread, flags, attachments).
- **Alice today:** `ORDER BY julianday(date) DESC` in both list paths (`store.rs:87, 149`).
- **Plan:**
  1. Add a `sort` query parameter (`date|from|subject|unread`, with `asc|desc`) to `MessageQuery`, validated against a whitelist, never interpolated raw.
  2. For threads, sort by the conversation's newest message attribute.
  3. Store the choice per folder in Preferences.
- **Pitfalls:**
  - **Sorting on JSON fields is slow** until the P0 indexed-columns item.
  - **Date group dividers** (`App.tsx:378`) only make sense for date sort.
- **Done when:** A store test for each sort key. Paging stays stable with ties (secondary key `id`).
- **Effort:** S–M. Depends on: "Move hot fields … into real indexed columns" (P0).

## P2: Inbox triage

<a id="split-inbox-smart-categories-people-notifications-newsletters"></a>

### Split inbox / smart categories: people, notifications, newsletters

*Roadmap: P2 · Inbox triage*

- **Why:** Person-to-person mail gets buried under receipts and newsletters. Every modern client now separates them, and Alice's sidebar already shows placeholder "Collections".
- **Competitors:**
  - **Mailspring:** none. It only shows Gmail "important" markers (`core.workspace.showImportant`, `config-schema.ts:66-70`). Forum #2716 asks for Spark-style views.
  - **Spark Smart Inbox:** People, Notifications, Newsletters, Pins and Seen sections. Notifications go only to People by default.
  - **Superhuman Split Inbox:**
    - Defaults: Important, Other, Team (your domain), VIP, News.
    - Custom splits are saved queries by sender, domain, recipient, subject or label, OR-combined, made from Cmd+K → "New Split Inbox".
    - Disabling a split returns its mail to Important/Other. Superhuman recommends 7 splits or fewer (help.superhuman.com/article/558-custom-split-inboxes).
  - **Outlook Focused/Other:**
    - Classification is server-side and exposed only through Graph: `message.inferenceClassification` = `focused`/`other`.
    - Per-sender `inferenceClassificationOverride {classifyAs, senderEmailAddress}` affects future mail only (learn.microsoft.com/en-us/graph/api/resources/manage-focused-inbox).
    - Not visible over IMAP.
  - **Apple Mail categories** (iOS 18.2, macOS 15.4):
    - Primary, Transactions, Updates, Promotions; on by default; off via "Show Mail Categories".
    - Context menu "Categorize Sender" moves current and future mail from that sender.
    - Time-sensitive mail from other categories also shows in Primary (support.apple.com/guide/mail/use-categories-mlhl64d76621/mac).
- **Spec (signals):**
  - RFC 2369 `List-*` and RFC 2919 `List-Id`.
  - RFC 3834 `Auto-Submitted`.
  - De facto `Precedence: bulk|list`, `Feedback-ID` (Gmail bulk senders) and `X-Auto-Response-Suppress`.
  - RFC 8058 `List-Unsubscribe-Post`.
- **Alice today:**
  - `Message.category` exists, indexed and filterable (`model.rs:107, 134`; `store.rs:27, 1132-1135`), but is hardcoded to `"personal"` on import (`providers.rs:411`, `store.rs:383`).
  - The sidebar "Collections" (work, personal, newsletters) are samples (`App.tsx:351-352`; README "Collections are sample categories"). `categoryName` maps ids (`App.tsx:38`).
- **Plan:**
  1. **Classifier.** `crates/alice-core/src/classify.rs`, deterministic and local. Input: the parsed headers (keep `List-Id`, `List-Unsubscribe`, `Precedence`, `Auto-Submitted`, `Feedback-ID`; see the Unsubscribe item) plus "do I correspond with this sender" (the contacts table `Mail` source, `store.rs:26`). Order:
     - (a) **people:** the user has written to this sender, or the sender is in an address book, and there are no bulk headers;
     - (b) **newsletters:** `List-Unsubscribe` or `List-Id` plus `Precedence: bulk|list`, or `Feedback-ID`;
     - (c) **notifications:** `Auto-Submitted: auto-generated`, a `no-reply`/`noreply`/`notifications@` local part, or receipts/OTP (`otp.rs` detection);
     - (d) **otherwise people.** Unknown goes to People, so nothing important hides.
  2. **Per-sender override.** Table `sender_category(account_id, pattern, category)`, set from the context menu "Always put mail from <sender> in…". It beats the classifier, like Apple's Categorize Sender and Outlook's override.
  3. **Where it runs.** Classify in `message_from` (`providers.rs:340-424`). Reclassify existing rows once on upgrade with one pass, and again whenever an override changes.
  4. **UI.**
     - Replace Collections with Inbox tabs: People · Notifications · Newsletters. Each shows an unread count from the existing `category` filter.
     - A setting turns tabs off.
     - Desktop notifications default to People only (`Arrival` filter, `daemon.rs:125-150`).
  5. **Custom splits** = saved searches pinned as tabs (Saved searches item).
  6. **Provider-native data, optional:**
     - Gmail categories (next item).
     - Outlook `inferenceClassification` once a Graph `Mail.Read` integration exists (today Graph is requested only for contacts and calendar, `oauth.rs:36-38`; no `Mail.Read`).
     - An opt-in BYOK AI pass for ambiguous mail (`crates/alice-core/src/ai.rs`).
- **Pitfalls:**
  - **False negatives hide real mail.** Keep a "Recently filtered" peek, and make thread category = the category of its newest message from someone else.
  - **Categories are local:** other clients are unaffected, which is good, but don't move mail on the server.
  - **Rules and categories interplay:** rules run first.
  - **Spam** is excluded.
  - **Re-classifying** on contact-book changes must stay cheap; batch it.
- **Done when:**
  - `classify.rs` unit tests with header fixtures: list mail → newsletters; `Auto-Submitted` → notifications; a reply from a known correspondent → people; an override wins.
  - UI: tabs show correct counts, and turning tabs off restores a single Inbox.
- **Effort:** M. Depends on: header retention (Unsubscribe item). The Saved searches item enables custom splits.

<a id="gmail-category-tabs-primary-social-promotions-updates"></a>

### Gmail category tabs (Primary, Social, Promotions, Updates)

*Roadmap: P2 · Inbox triage*

- **Why:** Gmail users already trained Gmail's categories, and seeing a different split in Alice is jarring.
- **Competitors:**
  - **Mimestream:** choose which of Social, Promotions, Updates and Forums to show, independently of the web setting. Categories appear in the sidebar. It uses the Gmail API.
  - **Mailspring:** none (forum #393).
- **Spec:**
  - Gmail API system labels `CATEGORY_PERSONAL`, `CATEGORY_SOCIAL`, `CATEGORY_PROMOTIONS`, `CATEGORY_UPDATES`, `CATEGORY_FORUMS`.
  - Over IMAP, `UID SEARCH X-GM-RAW "category:promotions"` (Gmail search syntax; developers.google.com/workspace/gmail/imap/imap-extensions).
  - Categories are **not documented** as `X-GM-LABELS` values; test on a live account, but expect them to be absent.
  - Moving mail between categories requires the Gmail API `messages.modify` (`gmail.modify`, restricted).
- **Alice today:** Gmail is synced by IMAP only (`oauth.rs:27`). Nothing reads categories.
- **Plan:**
  1. During Gmail INBOX sync (capability `X-GM-EXT-1`), after refreshing the window, run 4 searches: `UID SEARCH SINCE <window> X-GM-RAW "category:social"`, then promotions, updates and forums. Everything else is Primary.
  2. Store the result in `Message.category` as `gmail_social`, `gmail_promotions` and so on. With tabs set to "Gmail categories", show Primary/Social/Promotions/Updates(/Forums) instead of Alice's own classifier.
  3. Re-run each sync for INBOX only (4 cheap searches). Skip when INBOX UIDNEXT and HIGHESTMODSEQ are unchanged.
  4. "Move to Primary" = a local override; optionally a Gmail API `messages.modify` later.
- **Pitfalls:**
  - **Category changes may not bump MODSEQ.** Re-check periodically (every 10 min) even when unchanged.
  - **Mail outside the window** is ignored.
  - **Search cost:** keep it to INBOX.
  - **Tab settings:** users who disabled categories in Gmail web still get them from search; offer an "off" setting.
- **Done when:** A unit test checks mapping from search results to categories. A manual check against a test Gmail account: promotions mail lands in the Promotions tab.
- **Effort:** S–M. Depends on: "Split inbox / smart categories" (tabs UI).

<a id="sender-screener-for-first-time-senders"></a>

### Sender screener for first-time senders

*Roadmap: P2 · Inbox triage*

- **Why:** It stops cold outreach and new newsletters from reaching the inbox until the user approves the sender once.
- **Competitors:**
  - **HEY "The Screener":** first-time senders that aren't spam wait for a yes/no. Approved senders go to the Imbox (or Feed or Paper Trail); screened-out mail is hidden and the sender is never told. Screener History reverses decisions; screening someone back in shows their mail from the last 90 days (help.hey.com/article/722-the-screener).
  - **Spark Gatekeeper:** off by default; new senders held with Accept/Block.
  - **Mailspring:** none.
- **Alice today:** The contacts table knows people you have written to and who wrote to you (`store.rs:26`; `ContactSource::Mail`, `model.rs:491-499`). No screening.
- **Plan:**
  1. Table `screened_senders(account_id, address, decision 'in'|'out', decided_at)`.
  2. **On import** (hook): when the screener is on and the sender is not decided, not in any address book, not someone the user wrote to, and not replying to the user's own message (`In-Reply-To` ∈ the user's sent Message-IDs), mark the message `screening` (local field). Inbox queries exclude it.
  3. **A "Screener" view** lists pending **senders**, one row each with a preview, with "Let in" and "Screen out". Screen out = local hide, plus optionally Block (move to Spam/Trash) via the Block sender item.
  4. **Bootstrap:** turning it on marks every existing correspondent "in", so the inbox doesn't empty out.
  5. **Exemptions:** OTP and security mail (`otp.rs` recognises codes), and mail to a role address the user chose.
  6. **Notifications** are suppressed for screening mail. Show a badge with the pending count instead.
- **Pitfalls:**
  - **Password resets and receipts from new senders** get hidden. Exempt OTP, and put "Paper Trail"-like notifications in a separate tab rather than screening them (combine with categories).
  - **Local only:** other devices and webmail still show everything; the decision list doesn't roam. Phase 2: export "out" decisions as Sieve or a Gmail filter.
  - **Aliases and forwarding addresses.**
- **Done when:** A unit test checks the first-time sender decision logic. Fixture: a new sender lands in Screener, not Inbox; "Let in" moves all their mail to Inbox and future mail bypasses; a reply to the user's own mail bypasses.
- **Effort:** M. Depends on: the rules arrival hook and Block sender.

<a id="read-later-set-aside-list"></a>

### Read later / set aside list

*Roadmap: P3*

- **Why:** It gives a place for "not now but not done" mail without misusing star or snooze.
- **Competitors:**
  - **HEY:** "Set Aside" (a shelf at the bottom of the Imbox) and "Reply Later" (a stack with a "Focus & Reply" mode).
  - **Spark:** "Set Aside".
  - **Mailspring:** none.
- **Spec:** RFC 9979 has no registered "later" keyword; `$followed` means "a thread the user cares about", which is different.
- **Alice today:** Star exists; snooze is planned.
- **Plan:**
  - **Lazy version:** a "Later" list as a keyword `$SetAside` (roams when PERMANENTFLAGS allows `\*`), with a local column fallback.
  - UI: a "Set aside" action (`b`?) removes the conversation from the Inbox view **without moving it on the server**, and shows it in a "Set aside" strip at the bottom of the Inbox with a count.
  - A "Reply later" variant uses a second keyword `$ReplyLater`.
- **Pitfalls:**
  - **Unregistered keywords are Alice-only** semantics; other clients ignore them.
  - **Overlap with snooze ("someday") and star** confuses users. Consider implementing "Snooze → Someday" (no wake time) instead.
- **Done when:** Setting aside hides from Inbox and shows in the strip; it survives restart, and on Dovecot the keyword is set.
- **Effort:** S. Depends on: keyword support.
- **Note:** Low value relative to snooze and star. Suggest P3, or fold into Snooze as a "Someday" preset.

<a id="local-adaptive-spam-classifier"></a>

### Local adaptive spam classifier

*Roadmap: P3 · new*

- **Why:** Many IMAP providers (small hosts, self-hosted) filter poorly (forum #834 "Adaptive junk filtering").
- **Competitors:**
  - **Thunderbird:** an adaptive Bayesian junk filter per account, trained by "Junk"/"Not junk". It sets `$Junk`/`$NotJunk` and can trust SpamAssassin headers.
  - **Apple Mail:** a junk filter with training.
  - **Mailspring:** none.
- **Spec:** IANA `$Junk`/`$NotJunk` keywords. Common `X-Spam-Flag`/`X-Spam-Status` headers.
- **Alice today:** "Mark spam" moves mail to Junk (`providers.rs:126`). There is no classifier.
- **Plan:**
  1. A multinomial naive Bayes classifier over tokens of the subject, sender domain and body, in about 150 lines with no crate. Token counts go in SQLite `spam_tokens(account_id, token, spam, ham)`.
  2. Train on spam and not-spam actions, and on the existing Junk folder at enable time.
  3. In the import hook, score new Inbox mail. Above the threshold, move it to Junk and set `$Junk`. Show "Alice thinks this is junk" in Spam with "Not junk".
  4. Respect server headers (`X-Spam-Flag: YES`) as a strong feature.
- **Pitfalls:**
  - **False positives:** default off, a conservative threshold, and never auto-junk known correspondents.
  - **Storage** growth needs token pruning.
  - **Two devices** train separately.
- **Done when:** A unit test on a small labelled corpus reaches the expected precision, and a fixture moves a trained-spam message to Junk on arrival.
- **Effort:** M. Depends on: the rules arrival hook and keyword support.
