# Alice parity roadmap

Goal: full feature parity with Mailspring and the other mainstream desktop clients, without giving up what Alice already does better.

Each item below has one line on why it matters, an effort estimate with its dependencies, and a link to its research. The research pages in [`docs/roadmap/`](docs/roadmap/) cover, per item: how Mailspring and other clients do it (with file paths into Mailspring's source and sync engine), the relevant RFCs and provider docs, what Alice has today (with `path:line` citations), a build plan, pitfalls, and acceptance criteria.

**Tiers.** **P0** blocks daily use. **P1** is expected by anyone switching from another client. **P2** covers power users and polish. **P3** is differentiators and nice-to-haves. **P4** is the long tail. Within a subsection, items are in rough build order. *(new)* marks items the research added.

**Effort.** S is a day or less, M is 2–4 days, L is 1–2 weeks, XL is more.

**Sources.**
- Mailspring 1.26's source: [client](https://github.com/Foundry376/Mailspring) and [sync engine](https://github.com/Foundry376/Mailspring-Sync), with commit history.
- The installed Mailspring 1.26 app: Preferences, menus and keymaps.
- Mailspring's full changelog.
- The most-voted Mailspring feature requests on GitHub and its community forum.
- RFCs, provider documentation and competitor help pages.
- Alice's working tree as of 2026-10-13. Other work is in flight, so re-check citations before starting an item.

**Research pages.**
- [Composer](docs/roadmap/composer.md)
- [Notion-style composer editor](docs/roadmap/composer-editor.md)
- [Sync, server-side state, push and offline](docs/roadmap/sync.md)
- [Sync fixes from the Mailspring comparison](docs/roadmap/sync-fixes.md)
- [Performance, diagnostics and support](docs/roadmap/performance.md)
- [Organization, productivity and inbox triage](docs/roadmap/organization.md)
- [Reading, accounts, localization and accessibility](docs/roadmap/reading-and-accounts.md)
- [Platform integration, security, contacts and calendar](docs/roadmap/platform-security-pim.md)
- [Differentiators and long tail](docs/roadmap/long-tail.md)

**Appendices.**
- [Mailspring settings and menus, mapped to Alice](docs/roadmap/mailspring-settings.md)
- [What Mailspring users ask for](docs/roadmap/user-demand.md)
- [How Mailspring's sync engine works](docs/roadmap/mailspring-sync-engine.md)

## Build these foundations first

Most features sit on a handful of shared pieces. Building them first, once, avoids writing the same plumbing several times. The order below is the recommended sequence; 4 can run in parallel with 2 and 3.

1. **Benchmark fixture.** Commit a 100k-message, 5-account fixture and track cold start, list-open and search latency in CI. The earlier 10,000-message probe never made it into the repo. → [Performance](docs/roadmap/performance.md#add-a-benchmark-fixture-eg-100k-messages-5-accounts-and-track-cold-start-list-open-and-search-latency-in-ci)
2. **New IMAP stack and a connection actor per account.**
   - **Why the current crate has to go.** The `imap` 2.4 crate cannot parse Gmail's `X-GM-*` attributes or QRESYNC `VANISHED`. It turns unparseable lines into a fake `BYE`, and its IDLE cannot be interrupted.
   - **The replacement.** Move to `async-imap` 0.12 on `tokio-rustls` with `rustls-platform-verifier`, and switch lettre to rustls as well. This also fixes TLS 1.3-only servers on macOS: `native-tls` there uses Secure Transport, which stops at TLS 1.2.
   - **The connection actor.** One actor per account keeps two long-lived sessions: one for IDLE and user actions, one for background sync.
   - → [Sync](docs/roadmap/sync.md#two-long-lived-imap-connections-per-account-one-for-idle-one-for-sync-and-actions), [rustls](docs/roadmap/sync.md#imap-and-smtp-over-rustls-so-tls-13-only-servers-connect-on-macos)
3. **One schema migration.** Change the database in a single pass:
   - Move hot fields out of the JSON blob into indexed columns, and bodies into their own table.
   - Give messages an `INTEGER PRIMARY KEY` so VACUUM cannot break the search index.
   - Add a `mailboxes` table, membership-based views (one message in many folders, as in Mailspring 1.26) and a `threads` table.
   - Add a small pool of read connections beside the single writer, with a revision counter the writer bumps.

   → [Performance](docs/roadmap/performance.md#move-hot-fields-date-account-mailbox-unread-starred-thread-id-out-of-the-json-blob-into-real-indexed-columns), [Multi-folder model](docs/roadmap/sync.md#multi-folder-message-model-one-message-can-belong-to-several-mailboxeslabels-at-once)
4. **Composer data model and a draft factory in the daemon.**
   - Add `from`, `cc`, `bcc`, `reply_to`, `references`, `html_body` and `attachments` to `Compose`; add `cc`, `bcc` and `references` to `Message`.
   - Reply, reply-all, forward, quoting and signatures are rebuilt today in four places: desktop, CLI, TUI and MCP. Move them into side-effect-free daemon endpoints.
   - Add a per-draft attachment store and a raw upload route. The daemon currently caps request bodies at 2 MB.
   - **The editor.** Build it on Tiptap 3's MIT packages with Alice's own menus, aiming for a Notion-style experience. Save the editor document with each draft, and render email-safe HTML and plain text from it. Tiptap's Notion-like template isn't an option: it needs a paid plan, sits under a license that conflicts with Alice's AGPL, and is wired to Tiptap Cloud.
   - → [Composer](docs/roadmap/composer.md#section-findings), [Notion-style editor](docs/roadmap/composer-editor.md#section-findings)
5. **Batch actions and a persisted operations queue.**
   - Today every action is one PATCH per message, and each PATCH opens a fresh TLS IMAP login, so archiving 50 conversations means about 150 logins.
   - Add a batch endpoint that sends one `UID STORE`/`UID MOVE` per mailbox.
   - Add a local-first `operations` table: it applies a change at once, then replays it to the server in order.
   - Undo, the offline queue, Drafts/Sent APPEND and cross-account moves all build on these two pieces.
   - → [Organization](docs/roadmap/organization.md#section-findings), [Offline](docs/roadmap/sync.md#offline-action-queue-archive-move-flag-and-send-work-offline-and-replay-in-order-when-online)
6. **Smaller shared pieces**, each built once and used by several features:
   - **Generic IMAP keyword support:** mute, tags, `$Forwarded`, `$Junk`.
   - **One daemon scheduler** (a `jobs` table plus one timer): snooze, send later, follow-up reminders and undo send.
   - **A raw-message endpoint** (`GET /v1/messages/{id}/raw`) and an auxiliary window with no app permissions: show original, print, export, forward as attachment, TNEF, PGP/S/MIME.
   - **New account fields** with a `PATCH /v1/accounts/{id}` route.
   - **A desktop entry-point layer** (single instance, launch arguments, macOS `Opened`): `mailto:`, notification actions, start hidden, relaunch after update.
   - **One command registry:** palette, shortcut presets, remapping.
   - **A local contacts and calendar cache** that keeps provider identity (etag, sync token).
   - **Structured logging:** the daemon's output goes to `/dev/null` today.

**Quick wins** that need none of the above:
- Two likely sync bugs found by comparing with Mailspring's engine:
  - `uid_next` is saved before new mail is stored.
  - The UID search is skipped on servers without `UIDNEXT`.
- From header with a display name
- `References` header on replies
- MOVE fallback
- Re-sync on wake
- Sent-copy detection
- DAV redirect credential fix
- CalDAV presets
- Hiding inline images from the attachment list
- The `cid:` README fix

## Already at parity or ahead

Keep these working; a regression here counts as P0.

- **Accounts:** Gmail OAuth (PKCE), Outlook OAuth (untested live), generic IMAP/SMTP, app passwords, multiple accounts, unified inbox. No vendor account needed: Mailspring's second most-requested change ever (GitHub #33, 399 👍) was making its account optional.
- **Sync:** CONDSTORE flag sync, incremental sync, a configurable sync window, newest-first background backfill.
- **Reading:**
  - Sanitized, sandboxed HTML reader. Its CSP blocks scripts, forms, remote CSS and web fonts, which is stricter than Mailspring.
  - Remote-image blocking, `cid:` inline images, quoted-text folding.
  - Three layouts (single, horizontal, vertical); dark theme keeps HTML mail on a light card.
- **Search:** local FTS5 with `from:`/`to:`/`subject:`/`body:`, `is:`, `has:attachment`, dates and ages, `OR`, `NOT`, parentheses and typo tolerance. Mailspring's grammar is weaker.
- **Contacts and calendar:** contact autocomplete from Google, Graph, CardDAV and Mac Contacts; invite RSVP, add to calendar, `.ics` export.
- **App:** native OS notifications with summaries (they follow Do Not Disturb), tray, keyboard shortcuts, typed i18n keys with CLDR plurals, accessibility basics. Downloaded programs are revealed, not opened.
- **Ahead of Mailspring:**
  - OTP detection with hardened copy.
  - Bring-your-own-key AI actions and per-message summaries and drafts.
  - CLI and TUI on the shared daemon.
  - MCP server with off / read-only / full access.
  - At-most-once sends.
  - Tauri rather than Electron.

## P0: blocks daily use

Without these, people cannot switch to Alice.

### Composer basics

- [ ] **Cc and Bcc fields** — Users cannot copy anyone, cannot blind-copy, and reply-all is impossible.
  Effort: M (2–3 days). Prerequisite for Reply-all and Automatic Cc/Bcc. [Research](docs/roadmap/composer.md#cc-and-bcc-fields)
- [ ] **Reply-all (dedupe own addresses, honor Reply-To)** — Replying in a group thread silently drops everyone except the sender.
  Effort: M (2–3 days). Depends on Cc and Bcc fields. Benefits from Send-as identities, which supply the own-address set. [Research](docs/roadmap/composer.md#reply-all-dedupe-own-addresses-honor-reply-to)
- [ ] **Forward, with original attachments carried over** — Forwarding is one of the three core actions, and it is missing entirely.
  Effort: M–L (3–5 days). Depends on Attachment upload (the draft attachment model) and Correct MIME for replies (references). [Research](docs/roadmap/composer.md#forward-with-original-attachments-carried-over)
- [ ] **Attachment upload: file picker, drag and drop, paste files** — Users cannot send a file at all.
  Effort: L (about 1 week). Blocks Forward (carried attachments) and Inline images. [Research](docs/roadmap/composer.md#attachment-upload-file-picker-drag-and-drop-paste-files)
- [ ] **Correct MIME for replies: `References` header chain, not only `In-Reply-To`** — Clients and servers that thread by References alone file a reply without the chain as a new thread or attach it to the wrong one.
  Effort: S (≤1 day). [Research](docs/roadmap/composer.md#correct-mime-for-replies-references-header-chain-not-only-in-reply-to)
- [ ] **Per-account signatures** — Work and personal accounts need different sign-offs.
  Effort: M (2–4 days). Depends on the daemon draft factory (Reply-all item). Per-alias defaults depend on Send-as identities. [Research](docs/roadmap/composer.md#per-account-signatures)
- [ ] **Quote the original message in replies and forwards** *(new)* — Alice's replies start empty: `body = ''` on desktop (`App.tsx:207`, `:212`), the bare user text in CLI `reply_compose`, and `draft_body("")` in the TUI.
  Effort: S for text, M with HTML. Depends on the Reply-all draft factory. [Research](docs/roadmap/composer.md#quote-the-original-message-in-replies-and-forwards)
- [ ] **From header carries the account's display name** *(new)* — `build_mail` sends `From: alex@example.com` with no name (`providers.rs:195`), and `rsvp_mail` does the same (`providers.rs:304`).
  Effort: S (an hour). [Research](docs/roadmap/composer.md#from-header-carries-the-accounts-display-name)

### Server-side mail state

- [ ] **Save drafts to the server Drafts folder (IMAP APPEND) so they roam across devices** — A draft started on the laptop is invisible on the phone or in webmail, and is lost if the device dies.
  Effort: M. Depends on "Sync every folder…" (Drafts discovery), "Offline action queue…" (the runner), "Cc and Bcc fields", and "Attachment upload…" (for drafts with files). [Research](docs/roadmap/sync.md#save-drafts-to-the-server-drafts-folder-imap-append-so-they-roam-across-devices)
- [ ] **Append sent mail to the server Sent folder for providers that don't do it themselves** — On iCloud, Fastmail and most generic IMAP hosts, mail sent from Alice exists only in Alice's local "Sent".
  Effort: M. Depends on "Two long-lived IMAP connections per account…" (needs no new crate, but runs on the actor), "Offline action queue…" (crash-safe op), and "Cc and Bcc fields" (for the Bcc header). [Research](docs/roadmap/sync.md#append-sent-mail-to-the-server-sent-folder-for-providers-that-dont-do-it-themsel)
- [ ] **Fall back to COPY + `\Deleted` + UID EXPUNGE when the server has no MOVE** — Archive, trash and spam fail outright on servers without MOVE ("mail.imap.no_safe_move"), so mail cannot be organised at all there.
  Effort: S. No dependencies; ship before the crate migration. [Research](docs/roadmap/sync.md#fall-back-to-copy-deleted-uid-expunge-when-the-server-has-no-move)

### Sync correctness

- [ ] **Save a mailbox's `uid_next` and `highest_modseq` only after its new mail is stored** *(new)* — If downloading new mail fails mid-run, those messages are later treated as old backfill.
  Effort: S. [Research](docs/roadmap/sync-fixes.md#save-a-mailboxs-uid_next-and-highest_modseq-only-after-its-new-mail-is-stored)
- [ ] **Never skip the UID search on servers that report no `UIDNEXT` or `UIDNEXT 0`** *(new)* — On those servers, one arrival plus one deletion between runs leaves both compared values unchanged.
  Effort: S. [Research](docs/roadmap/sync-fixes.md#never-skip-the-uid-search-on-servers-that-report-no-uidnext-or-uidnext-0)
- [ ] **Thread by `Message-ID` / `References` / `In-Reply-To`, plus `X-GM-THRID` on Gmail; subject-only grouping as a last resort** — Today any two mails with the same subject in an account merge ("Invoice", "Meeting", newsletters), while a reply whose subject was edited splits off.
  Effort: L. Depends on "Move hot fields … into real indexed columns" (shared migration), "Two long-lived IMAP connections per account…" (crate migration, for X-GM-THRID), and "Correct MIME for replies: `References` header chain" (so Alice's own replies thread elsewhere). [Research](docs/roadmap/sync.md#thread-by-message-id-references-in-reply-to-plus-x-gm-thrid-on-gmail-subject-onl)
- [ ] **Multi-folder message model: one message can belong to several mailboxes/labels at once** — A message in Inbox and a user folder, or with two Gmail labels, shows only in one place, and moves or deletes can act on the wrong copy.
  Effort: L. Depends on "Sync every folder…" (more mailboxes), "Move hot fields …" (same migration), and "Page the thread list in SQL…" (the membership joins). [Research](docs/roadmap/sync.md#multi-folder-message-model-one-message-can-belong-to-several-mailboxeslabels-at)
- [ ] **Gmail labels via `X-GM-LABELS`: read, add, remove; show labels in the list and reader** — Labels are how Gmail users organise mail.
  Effort: L. Depends on "Two long-lived IMAP connections per account…" (crate migration), "Multi-folder message model…", "Sync every folder…" (label list and tree UI), and "Move to any folder with type-ahead picker" (picker). [Research](docs/roadmap/sync.md#gmail-labels-via-x-gm-labels-read-add-remove-show-labels-in-the-list-and-reader)
- [ ] **Sync every folder, not only the five special-use roles; show the folder tree in the sidebar** — Users who file mail into folders (or whose server rules do) cannot see any of it in Alice, and cannot move mail to those folders.
  Effort: L. Depends on "Multi-folder message model…" (views by membership) and "Move to any folder with type-ahead picker" (UI overlap). [Research](docs/roadmap/sync.md#sync-every-folder-not-only-the-five-special-use-roles-show-the-folder-tree-in-th)

### Push and connections

- [ ] **IMAP and SMTP over rustls so TLS 1.3-only servers connect on macOS** *(new)* — On macOS, `native-tls` uses Secure Transport, which cannot negotiate TLS 1.3.
  Effort: S. No dependencies; do it first or inside the connection migration. [Research](docs/roadmap/sync.md#imap-and-smtp-over-rustls-so-tls-13-only-servers-connect-on-macos)
- [ ] **IMAP IDLE on Inbox (and NOTIFY where supported) instead of 60s polling** — New mail and verification codes arrive up to 60 s late, and changes made elsewhere lag just as long.
  Effort: M (L if counted with the crate migration). Depends on "Two long-lived IMAP connections per account…". [Research](docs/roadmap/sync.md#imap-idle-on-inbox-and-notify-where-supported-instead-of-60s-polling)
- [ ] **Two long-lived IMAP connections per account (one for IDLE, one for sync and actions)** — Every star, archive and attachment open costs DNS, TCP, TLS and XOAUTH2 login, several round trips and often more than a second.
  Effort: L. Prerequisite for "IMAP IDLE on Inbox…", "Gmail labels via `X-GM-LABELS`…", the X-GM-THRID part of threading, and "Offline action queue…". [Research](docs/roadmap/sync.md#two-long-lived-imap-connections-per-account-one-for-idle-one-for-sync-and-action)
- [ ] **Re-sync immediately on wake from sleep and when the network comes back** — After opening the laptop, mail can stay stale for a minute or more.
  Effort: S. A first version (wake leads to `sync_all` plus `wake()`) ships on today's code; IDLE interruption follows "IMAP IDLE on Inbox…". [Research](docs/roadmap/sync.md#re-sync-immediately-on-wake-from-sleep-and-when-the-network-comes-back)

### Performance foundations

- [ ] **Add a benchmark fixture (e.g. 100k messages, 5 accounts) and track cold start, list-open and search latency in CI** — Nothing guards large-mailbox performance today.
  Effort: M. No dependencies. Build it first. [Research](docs/roadmap/performance.md#add-a-benchmark-fixture-eg-100k-messages-5-accounts-and-track-cold-start-list-op)
- [ ] **Move hot fields (date, account, mailbox, unread, starred, thread id) out of the JSON blob into real indexed columns** — Lists, counts and sync lookups go through `json_extract` expression indexes over a blob that includes the bodies.
  Effort: L. Depends on: "Add a benchmark fixture" (to measure). Enables: "Page the thread list in SQL", "Periodic SQLite maintenance", "Automatic repair of folder and unread counts". Coordinate with "Thread by `Message-ID` / `References`…" and "Multi-folder message model". [Research](docs/roadmap/performance.md#move-hot-fields-date-account-mailbox-unread-starred-thread-id-out-of-the-json-bl)
- [ ] **Page the thread list in SQL; stop loading every message id and every thread row on each `threads()` call** — List-open, every paging step and every 2 s `/v1/state` poll cost O(all messages of the account) in CPU and memory.
  Effort: M. Depends on: "Move hot fields … into real indexed columns". [Research](docs/roadmap/performance.md#page-the-thread-list-in-sql-stop-loading-every-message-id-and-every-thread-row-o)
- [ ] **Separate read connections (pool) so sync writes don't block UI reads; keep the single writer** — While a sync batch or a large flag refresh holds the only connection, every list, open and search waits.
  Effort: M. No dependencies; do it before or with the schema migration. It enables "Push store changes to the UI". [Research](docs/roadmap/performance.md#separate-read-connections-pool-so-sync-writes-dont-block-ui-reads-keep-the-singl)

## P1: expected by switchers

Anyone coming from Mailspring, Apple Mail, Outlook or Thunderbird will look for these in the first week.

### Composer

- [ ] **Rich text compose: send HTML + text, replacing the textarea with the Tiptap editor (see Notion-style editor below)** — Most business mail is HTML.
  Effort: L–XL (about 2 weeks). Depends on the data model. Inline images, Paste and Match Style and Plain-text mode build on it. [Research](docs/roadmap/composer.md#rich-text-compose-send-html-text-replacing-the-textarea-with-the-tiptap-editor-s)
- [ ] **Inline images: paste, drag, resize** — Screenshots pasted into mail are routine; today they cannot be sent.
  Effort: M (3–4 days). Depends on Attachment upload and Rich text. [Research](docs/roadmap/composer.md#inline-images-paste-drag-resize)
- [ ] **Plain-text mode per draft or globally** — Mailing lists, kernel and patch workflows, and plain-text purists need text-only mail.
  Effort: S–M (1–2 days) after Rich text. [Research](docs/roadmap/composer.md#plain-text-mode-per-draft-or-globally)
- [ ] **Send-as identities and aliases, including Gmail "Send mail as" import** — Many people send from aliases (role addresses, custom domains routed through Gmail).
  Effort: L (1–2 weeks: Gmail import M, per-identity SMTP M). Depends on the NEW From display name item. Feeds Reply-all, Signatures and Automatic Cc/Bcc. [Research](docs/roadmap/composer.md#send-as-identities-and-aliases-including-gmail-send-mail-as-import)
- [ ] **Undo send with a configurable delay** — Typos and wrong recipients are noticed in the first seconds after Send.
  Effort: M (3–4 days). Independent of the other items. Unblocks P2 "Send later". [Research](docs/roadmap/composer.md#undo-send-with-a-configurable-delay)
- [ ] **Spellcheck language choice and right-click suggestions** — Misspellings in outgoing mail are embarrassing, and multilingual users need to pick the language.
  Effort: S–M (mostly verification; Linux plumbing about 1 day). [Research](docs/roadmap/composer.md#spellcheck-language-choice-and-right-click-suggestions)
- [ ] **Paste and Match Style; sanitize pasted HTML** — Pasting from web pages, Word or Google Docs drags in fonts, colors and junk markup, and recipients see a patchwork.
  Effort: S–M (1–2 days) after Rich text. [Research](docs/roadmap/composer.md#paste-and-match-style-sanitize-pasted-html)
- [ ] **Missing-attachment warning ("attached" with no file) and optional external-recipient warning** — "See attached" with nothing attached is the most common sending mistake.
  Effort: S (1–2 days). [Research](docs/roadmap/composer.md#missing-attachment-warning-attached-with-no-file-and-optional-external-recipient)
- [ ] **Pop-out composer window, titled with the subject** — Writing long mail while reading other threads requires a separate window.
  Effort: M (2–3 days). Pairs with the P1 `mailto:` handler, which should open a compose window. [Research](docs/roadmap/composer.md#pop-out-composer-window-titled-with-the-subject)
- [ ] **Automatic CC/BCC per account** *(new)* — People who archive outgoing mail to a CRM, ticketing address or a second mailbox need every message copied automatically.
  Effort: S (≤1 day). Depends on Cc and Bcc fields and the daemon draft factory. [Research](docs/roadmap/composer.md#automatic-ccbcc-per-account)

### Notion-style editor

- [ ] **Build the composer on Tiptap 3's MIT packages with Alice's own UI (no Pro template, no Tiptap Cloud)** *(new)* — Settles the editor stack once, so every later block and menu builds on the same base, and keeps Alice's source fully publishable under the AGPL.
  Effort: M (2–3 days). It replaces step 3 of the existing "Rich text / HTML compose" item, and everything else in this section depends on it. [Research](docs/roadmap/composer-editor.md#build-the-composer-on-tiptap-3s-mit-packages-with-alices-own-ui-no-pro-template)
- [ ] **Save the editor document with each draft and generate the email HTML and plain text from it** *(new)* — Notion-style blocks (toggles, callouts, checked to-dos, image sizes) do not survive an HTML round trip.
  Effort: M–L (4–6 days). Depends on the Tiptap scaffold and the composer data-model item (`html_body`). [Research](docs/roadmap/composer-editor.md#save-the-editor-document-with-each-draft-and-generate-the-email-html-and-plain-t)
- [ ] **Markdown shortcuts and Notion keyboard map** *(new)* — Typing `# `, `- `, `[] `, `> ` or ```` ``` ```` is the fastest way to structure text, and Notion users expect it.
  Effort: S–M (1–2 days). This replaces the P2 "Markdown shortcuts in the composer" item. [Research](docs/roadmap/composer-editor.md#markdown-shortcuts-and-notion-keyboard-map) · [earlier research](docs/roadmap/composer.md#markdown-shortcuts-in-the-composer)
- [ ] **Slash command menu for blocks and email actions** *(new)* — `/` is the single entry point that makes block editing discoverable, and it can expose email actions in the same place.
  Effort: M (2–3 days). Depends on the Tiptap scaffold. [Research](docs/roadmap/composer-editor.md#slash-command-menu-for-blocks-and-email-actions)
- [ ] **Floating selection toolbar (bubble menu)** *(new)* — Formatting where the selection is beats a fixed toolbar, and it's the Notion and Medium interaction people expect.
  Effort: S–M (1–2 days). Depends on the Tiptap scaffold. [Research](docs/roadmap/composer-editor.md#floating-selection-toolbar-bubble-menu)
- [ ] **Block handle: drag to reorder, and a block menu to turn into, duplicate, move or delete** *(new)* — Reorganizing a long message by dragging paragraphs, and turning a paragraph into a to-do or callout, is the core of the Notion feel.
  Effort: M (3–4 days). Depends on the slash menu (shared registry). [Research](docs/roadmap/composer-editor.md#block-handle-drag-to-reorder-and-a-block-menu-to-turn-into-duplicate-move-or-del)
- [ ] **Placeholders and empty-state hints** *(new)* — "Write, or press / for commands" teaches the slash menu without onboarding, and per-block placeholders ("Heading 1", "To-do") make empty blocks legible.
  Effort: S. [Research](docs/roadmap/composer-editor.md#placeholders-and-empty-state-hints)

### Organization

- [ ] **Multi-select and bulk actions (shift/cmd-click, select all)** — Cleaning up an inbox one conversation at a time is the main complaint of anyone coming from Gmail, Outlook or Apple Mail.
  Effort: M. Depends on: "Two long-lived IMAP connections per account" (P0). It works without that, just slower. [Research](docs/roadmap/organization.md#multi-select-and-bulk-actions-shiftcmd-click-select-all)
- [ ] **Undo for archive, trash, move, mark read** — Single-key triage (`e`, `#`, `!`) without undo loses mail, and people trust keyboard triage only once undo exists.
  Effort: M. Depends on: Multi-select and bulk actions (batch endpoint). Pairs with the Offline action queue (P1). [Research](docs/roadmap/organization.md#undo-for-archive-trash-move-mark-read)
- [ ] **Create, rename, delete and nest folders and labels** — Users who file mail need to make folders where they work.
  Effort: M. Depends on: "Sync every folder, not only the five special-use roles" and "Gmail labels via X-GM-LABELS" (P0). [Research](docs/roadmap/organization.md#create-rename-delete-and-nest-folders-and-labels)
- [ ] **Move to any folder with type-ahead picker** — Filing into project folders is the core workflow for non-Gmail users.
  Effort: M. Depends on: Create, rename, delete and nest folders (mailbox table), "Sync every folder" (P0), and Multi-select (batch). [Research](docs/roadmap/organization.md#move-to-any-folder-with-type-ahead-picker)
- [ ] **Mark all as read, per folder** — It is the most-voted Mailspring GitHub request in this area (#159, 41 👍).
  Effort: S. Depends on: Multi-select and bulk actions (batch endpoint). [Research](docs/roadmap/organization.md#mark-all-as-read-per-folder)
- [ ] **Unread counts on every folder, and optionally across all accounts** — Without counts, folders that rules or other clients fill are invisible.
  Effort: S–M. Depends on: "Sync every folder" (P0) and "Move hot fields … into real indexed columns" (P0) for speed. [Research](docs/roadmap/organization.md#unread-counts-on-every-folder-and-optionally-across-all-accounts)
- [ ] **One-click unsubscribe via `List-Unsubscribe` / `List-Unsubscribe-Post` (RFC 2369, RFC 8058)** — It is the fastest way to shrink inbox volume.
  Effort: M. Depends on: nothing hard. It shares header retention with "Split inbox / smart categories". [Research](docs/roadmap/organization.md#one-click-unsubscribe-via-list-unsubscribe-list-unsubscribe-post-rfc-2369-rfc-80)
- [ ] **Permanent delete from Trash and Spam, empty folder** — Trash and Spam grow forever on servers without auto-purge, eating quota.
  Effort: S. Depends on: Multi-select and bulk actions (batch endpoint). [Research](docs/roadmap/organization.md#permanent-delete-from-trash-and-spam-empty-folder)
- [ ] **Block sender (move future mail to Spam)** — It is a direct fix for persistent unwanted senders that spam filters miss.
  Effort: M. Depends on: Multi-select (batch) and the rules arrival hook. Server-side export depends on "Mail rules / filters" and "Sync Gmail filters…". [Research](docs/roadmap/organization.md#block-sender-move-future-mail-to-spam)
- [ ] **Conversation view toggle: show individual messages instead of conversations** *(new)* — It is the #1 Mailspring forum request by a wide margin (#291, 172 votes; also #2504).
  Effort: S. Depends on: nothing; better after the P0 "Thread by Message-ID" item. [Research](docs/roadmap/organization.md#conversation-view-toggle-show-individual-messages-instead-of-conversations)
- [ ] **Per-message actions inside a conversation: move, delete, archive, mark one message** *(new)* — People need to delete one wrong reply or file one message from a long thread (Mailspring forum #320, 26 votes; GitHub #426, 27 👍).
  Effort: S. Depends on: Multi-select (batch) for consistency; "Move to any folder" for Move…. [Research](docs/roadmap/organization.md#per-message-actions-inside-a-conversation-move-delete-archive-mark-one-message)
- [ ] **Mark-as-read delay setting: instantly, after 0.5 s, after 2 s, or manually** *(new)* — Skimming with `j`/`k` currently marks every conversation passed over as read.
  Effort: S. [Research](docs/roadmap/organization.md#mark-as-read-delay-setting-instantly-after-05-s-after-2-s-or-manually)

### Offline

- [ ] **Offline action queue: archive, move, flag and send work offline and replay in order when online** — On a plane or flaky Wi-Fi, every archive and star fails with an error, and sending is impossible.
  Effort: L. Depends on "Two long-lived IMAP connections per account…" (runner), "Multi-folder message model…" (pending markers), and "Undo for archive, trash, move, mark read" (shares the queue). [Research](docs/roadmap/sync.md#offline-action-queue-archive-move-flag-and-send-work-offline-and-replay-in-order)
- [ ] **Optional full offline mode: download all bodies and attachments within the window** — Travellers and people on poor connections cannot open attachments, inline images or very long messages offline.
  Effort: M. Depends on "Two long-lived IMAP connections per account…" (bg session) and the P2 "Per-account and per-folder sync settings" (UI). Inline `cid:` rendering is owned by the Reading/Composer items. [Research](docs/roadmap/sync.md#optional-full-offline-mode-download-all-bodies-and-attachments-within-the-window)

### Sync

- [ ] **Keep a message whose last copy vanished until the run ends, so a move made in another client isn't a delete and a re-download** *(new)* — When mail is moved from Inbox to Archive in webmail, Alice sees it leave Inbox first.
  Effort: S–M. Superseded later by the "Multi-folder message model" item, but worth doing first because it is small. [Research](docs/roadmap/sync-fixes.md#keep-a-message-whose-last-copy-vanished-until-the-run-ends-so-a-move-made-in-ano)
- [ ] **Relink messages after a `UIDVALIDITY` change instead of re-downloading the mailbox** *(new)* — Dovecot changes UIDVALIDITY when it rebuilds an index, and Cyrus does on `reconstruct`.
  Effort: M. [Research](docs/roadmap/sync-fixes.md#relink-messages-after-a-uidvalidity-change-instead-of-re-downloading-the-mailbox)
- [ ] **Store flag changes on every server copy of a message** *(new)* — Alice reads and writes read and starred state only on a message's primary copy.
  Effort: S–M. Depends on the batch action endpoint. [Research](docs/roadmap/sync-fixes.md#store-flag-changes-on-every-server-copy-of-a-message)

### Reading

- [ ] **In-app attachment preview: images, PDF, text, Office docs; Quick Look on macOS** — Opening each attachment today writes it into Downloads and launches another app.
  Effort: M. Related: "Show original: raw source and full headers" (shares the on-demand fetch path). [Research](docs/roadmap/reading-and-accounts.md#in-app-attachment-preview-images-pdf-text-office-docs-quick-look-on-macos)
- [ ] **Print and Save as PDF** — Receipts, tickets and contracts still get printed or archived as PDF.
  Effort: M. Shares the auxiliary-window code with "Clip very long messages with a 'show full message' popout to keep scrolling fast". [Research](docs/roadmap/reading-and-accounts.md#print-and-save-as-pdf)
- [ ] **Show original: raw source and full headers** — Diagnosing phishing (Authentication-Results, Received chains), delivery problems and broken rendering needs the raw message.
  Effort: S–M. **Note:** build this first in the Reading section. The same raw-fetch primitive is needed by print fidelity, .eml/mbox export, forward-as-attachment and dragging threads into the composer. [Research](docs/roadmap/reading-and-accounts.md#show-original-raw-source-and-full-headers)
- [ ] **Clip very long messages with a "show full message" popout to keep scrolling fast** — Huge newsletters, log digests and machine-generated mail freeze the reader while they are sanitized and laid out, and they create a scroll-inside-scroll trap.
  Effort: S (S–M including the popout window). Shares the window with "Print and Save as PDF". [Research](docs/roadmap/reading-and-accounts.md#clip-very-long-messages-with-a-show-full-message-popout-to-keep-scrolling-fast)
- [ ] **Dark-mode body rendering option: keep light card by default, optional smart inversion** — In dark theme a white card is safe, but bright at night.
  Effort: M (S for the preference plus transparent-when-safe). Depends on nothing. [Research](docs/roadmap/reading-and-accounts.md#dark-mode-body-rendering-option-keep-light-card-by-default-optional-smart-invers)

### Accounts and setup

- [ ] **Autodiscovery: Thunderbird ISPDB / autoconfig, MX lookup, `.well-known`** — Today a non-Gmail/Outlook user must know four hostnames, two ports and the TLS mode before Alice will connect.
  Effort: L. Depends on "Presets with setup tips" and "IMAP STARTTLS on port 143, and custom ports" (the security and username fields). [Research](docs/roadmap/reading-and-accounts.md#autodiscovery-thunderbird-ispdb-autoconfig-mx-lookup-well-known)
- [ ] **Presets with setup tips: iCloud, Yahoo, AOL, Fastmail, Zoho, GMX, Yandex, Proton Bridge, Outlook.com, O365** — Most of these providers need an app password or a toggle that users don't know about.
  Effort: M. Depends on "IMAP STARTTLS on port 143, and custom ports" (Bridge, security field, username). [Research](docs/roadmap/reading-and-accounts.md#presets-with-setup-tips-icloud-yahoo-aol-fastmail-zoho-gmx-yandex-proton-bridge)
- [ ] **IMAP STARTTLS on port 143, and custom ports** — Self-hosted servers, Proton Bridge, DavMail and many ISP servers only offer IMAP on 143 with STARTTLS.
  Effort: S–M for STARTTLS, security fields and SMTP validation; M if the rustls move is done in the same change. No dependencies; do this first in this section. [Research](docs/roadmap/reading-and-accounts.md#imap-starttls-on-port-143-and-custom-ports)
- [ ] **Verify Outlook/O365 OAuth against a live account; ship a built-in Microsoft client id** — Today every Outlook user must register their own Entra app (`AddAccount.tsx:44`), which almost nobody will do.
  Effort: M, plus publisher-verification lead time. Unblocks "Office 365 shared mailboxes". Pairs with "Re-authenticate flow when a token is revoked, with a clear error". [Research](docs/roadmap/reading-and-accounts.md#verify-outlooko365-oauth-against-a-live-account-ship-a-built-in-microsoft-client)
- [ ] **Office 365 shared mailboxes** — Team inboxes (support@, sales@) in Microsoft 365 are shared mailboxes with no password of their own.
  Effort: M. Depends on "Verify Outlook/O365 OAuth against a live account; ship a built-in Microsoft client id" and on the `username` field from "IMAP STARTTLS on port 143, and custom ports". [Research](docs/roadmap/reading-and-accounts.md#office-365-shared-mailboxes)
- [ ] **Edit an existing account's settings; reorder accounts; per-account color** — A server or password change today means removing and re-adding the account.
  Effort: M. Splits cleanly: label, sender name, color and order (S); connection editing and rebuild (S–M, after "IMAP STARTTLS on port 143, and custom ports"). [Research](docs/roadmap/reading-and-accounts.md#edit-an-existing-accounts-settings-reorder-accounts-per-account-color)
- [ ] **Re-authenticate flow when a token is revoked, with a clear error** — Revoked Google/Microsoft grants and changed passwords are routine.
  Effort: M. Related: "Verify Outlook/O365 OAuth against a live account; ship a built-in Microsoft client id" (error mapping). [Research](docs/roadmap/reading-and-accounts.md#re-authenticate-flow-when-a-token-is-revoked-with-a-clear-error)
- [ ] **Manual folder role mapping** *(new)* — Servers without SPECIAL-USE, and localized or legacy folder names, make Alice pick the wrong Sent/Archive/Trash or no Spam folder.
  Effort: S–M. Depends on "Edit an existing account's settings; reorder accounts; per-account color" (PATCH endpoint and detail view). [Research](docs/roadmap/reading-and-accounts.md#manual-folder-role-mapping)
- [ ] **Send Microsoft 365 mail through Graph when SMTP AUTH is disabled** *(new)* — Microsoft disables SMTP AUTH Basic by default for existing tenants at the end of December 2026, and many tenants already turn SMTP AUTH off entirely (security defaults do), so Outlook work accounts that still read over IMAP cannot send.
  Effort: M. Depends on "Verify Outlook/O365 OAuth against a live account; ship a built-in Microsoft client id". [Research](docs/roadmap/long-tail.md#send-microsoft-365-mail-through-graph-when-smtp-auth-is-disabled)

### Platform integration

- [ ] **Register as `mailto:` handler and offer "make default mail client"** — Clicking an email link in a browser or a PDF should open Alice's composer.
  Effort: M. Depends on "Cc and Bcc fields". Shares the single-instance work with "Notification actions" and "Launch at login". [Research](docs/roadmap/platform-security-pim.md#register-as-mailto-handler-and-offer-make-default-mail-client)
- [ ] **Dock / taskbar unread badge (macOS, Windows, KDE/Unity)** — It shows at a glance whether there is new mail, without opening the window.
  Effort: M. Benefits from "Page the thread list in SQL" and "Unread counts on every folder". [Research](docs/roadmap/platform-security-pim.md#dock-taskbar-unread-badge-macos-windows-kdeunity)
- [ ] **Launch at login (LaunchAgent, Windows Run key, XDG autostart / Flatpak portal)** — Alice only notifies, copies OTP codes and syncs in the background while it is running.
  Effort: S–M. Pairs with "(NEW) Start hidden at login". [Research](docs/roadmap/platform-security-pim.md#launch-at-login-launchagent-windows-run-key-xdg-autostart-flatpak-portal)
- [ ] **Real auto-update: download, verify signature, install (Tauri updater)** — Security fixes, such as the ones in this section, only matter once they are installed.
  Effort: L (mostly CI and release engineering). Depends on "Linux packaging: Flatpak, Snap, deb, rpm, AppStream metadata; Wayland and tray fixes" for channel decisions, and on code signing for Windows. [Research](docs/roadmap/platform-security-pim.md#real-auto-update-download-verify-signature-install-tauri-updater)
- [ ] **Follow system light/dark theme** — Most people run "Auto" appearance.
  Effort: S. No dependencies. [Research](docs/roadmap/platform-security-pim.md#follow-system-lightdark-theme)
- [x] **Respect Do Not Disturb / Focus** — Mostly done: OS notifications already follow DND / Focus. Verify on each OS.
  [Research](docs/roadmap/platform-security-pim.md#respect-do-not-disturb-focus)
- [ ] **Notification actions: archive, mark read, reply** — Most new mail needs only a glance and a dismissal.
  Effort: M (macOS + Linux + Windows buttons). L with Windows cold activation and inline reply. Depends on "Correct MIME for replies: `References` header chain", "Reply-all", and the single-instance work in the mailto item. [Research](docs/roadmap/platform-security-pim.md#notification-actions-archive-mark-read-reply)
- [ ] **Start hidden at login: open to the tray / Dock without showing the window** *(new)* — Most people want notifications and sync after login, not a mail window over their desktop.
  Effort: S. Depends on "Launch at login". [Research](docs/roadmap/platform-security-pim.md#start-hidden-at-login-open-to-the-tray-dock-without-showing-the-window)

### Security

- [ ] **Ship a built-in Google OAuth client safely (the bundled-secret shortcut is already gone)** — Switchers expect "Sign in with Google" to just work.
  Effort: M for code, plus weeks to months of external lead time for Google verification and CASA. Depends on "Verify Outlook/O365 OAuth against a live account; ship a built-in Microsoft client id". [Research](docs/roadmap/platform-security-pim.md#ship-a-built-in-google-oauth-client-safely-the-bundled-secret-shortcut-is-alread)
- [ ] **Never fetch remote content in replies, forwards, previews or "show all" when images are blocked** — Remote-image blocking is only as strong as its weakest path.
  Effort: S now for the policy and tests. M when the HTML composer and print land, and the work should be built into those items. Depends on "Rich text / HTML compose", "Forward, with original attachments carried over", "Print and Save as PDF" and "Clip very long messages with a 'show full message' popout". [Research](docs/roadmap/platform-security-pim.md#never-fetch-remote-content-in-replies-forwards-previews-or-show-all-when-images)
- [x] **Block remote CSS and web fonts along with images** — Done: the reader's CSP already blocks them. Add a regression test.
  [Research](docs/roadmap/platform-security-pim.md#block-remote-css-and-web-fonts-along-with-images)
- [ ] **Never auto-load images in Spam** — Spam is where tracking pixels and "is this address live" probes are densest.
  Effort: S. Should be revisited after "Multi-folder message model". [Research](docs/roadmap/platform-security-pim.md#never-auto-load-images-in-spam)
- [ ] **Don't send DAV Basic credentials across hosts on redirect** *(new)* — `dav::Session::request` follows up to five redirects by hand.
  Effort: S. No dependencies. [Research](docs/roadmap/platform-security-pim.md#dont-send-dav-basic-credentials-across-hosts-on-redirect)

### From the settings and menu audit

- [ ] **Hide inline images from the attachment list** *(new)* — Images embedded in the HTML body (`cid:`) also show in the attachment strip as "Attachment.png", because `Attachments.tsx` ignores `content_id`. Mailspring hides them (1.5.7).
  Effort: S. [Research](docs/roadmap/long-tail.md#update-readmemd-current-boundaries-cid-inline-images-are-implemented)

## P2: power users and polish

### Composer

- [ ] **Contact groups expand in recipient fields** — Teams and families mail fixed groups; typing ten addresses every time is slow and error-prone.
  Effort: M–L (4–6 days). Depends on Cc and Bcc fields. [Research](docs/roadmap/composer.md#contact-groups-expand-in-recipient-fields)
- [ ] **format=flowed and hard wrap at 72 for plain-text mail** *(new)* — Mailing lists and patch workflows (the Linux kernel, many open-source projects) expect plain text wrapped at about 72 characters.
  Effort: S–M (1–2 days). Depends on Plain-text mode. [Research](docs/roadmap/composer.md#formatflowed-and-hard-wrap-at-72-for-plain-text-mail)

### Notion-style editor

- [ ] **To-do lists that send as ☐ / ☑** *(new)* — Action items in mail ("Next steps") are common.
  Effort: S. [Research](docs/roadmap/composer-editor.md#to-do-lists-that-send-as)
- [ ] **Toggle blocks that send expanded** *(new)* — Toggles let long mails hide detail (logs, background, FAQs) while drafting and reading in Alice.
  Effort: S–M. [Research](docs/roadmap/composer-editor.md#toggle-blocks-that-send-expanded)
- [ ] **Callout blocks** *(new)* — Highlighting a key decision, deadline or warning is one of Notion's most used blocks, and it reads well in mail.
  Effort: S–M. [Research](docs/roadmap/composer-editor.md#callout-blocks)
- [ ] **Tables in the composer** *(new)* — Comparisons, schedules and pricing in mail are tables; today people paste screenshots instead.
  Effort: M. [Research](docs/roadmap/composer-editor.md#tables-in-the-composer)
- [ ] **Code blocks with syntax highlighting** *(new)* — Developers mail snippets daily; monospace with highlighting is far more readable than plain text.
  Effort: S–M. [Research](docs/roadmap/composer-editor.md#code-blocks-with-syntax-highlighting)
- [ ] **Emoji picker on `:`** *(new)* — `:smile:`-style shortcodes are muscle memory for Slack, GitHub and Notion users.
  Effort: S. [Research](docs/roadmap/composer-editor.md#emoji-picker-on)
- [ ] **@mention contacts to add them as recipients** *(new)* — In Outlook and Gmail, "@Name" in the body adds that person to the recipients.
  Effort: M. Depends on the Cc and Bcc fields. [Research](docs/roadmap/composer-editor.md#mention-contacts-to-add-them-as-recipients)
- [ ] **Image and file blocks: paste, drop, resize, align and caption, sent as inline parts and attachments** *(new)* — Screenshots and files belong where they're discussed in the text, not only in an attachment strip.
  Effort: M. Depends on "Attachment upload" and "Inline images" (this item is their editor UI). [Research](docs/roadmap/composer-editor.md#image-and-file-blocks-paste-drop-resize-align-and-caption-sent-as-inline-parts-a)
- [ ] **Templates, snippets and variables through the slash menu** *(new)* — Reusable replies ("/intro", "/pricing") with variables like `{first_name}` turn repetitive mail into seconds.
  Effort: M. Depends on the slash menu and the P2 Templates storage. [Research](docs/roadmap/composer-editor.md#templates-snippets-and-variables-through-the-slash-menu)
- [ ] **Signature and quoted history as protected blocks** *(new)* — Treating the signature and the quoted original as special blocks avoids classic bugs: signatures duplicated or edited by accident, and slow editing with long quoted threads (MS #720, MS 1.1.3).
  Effort: M. Depends on Per-account signatures and "Quote the original message". [Research](docs/roadmap/composer-editor.md#signature-and-quoted-history-as-protected-blocks)
- [ ] **AI writing in the editor with your own key** *(new)* — "Write a polite decline", "make this shorter", "fix grammar" and "translate to German" right where you write.
  Effort: M–L. Depends on the bubble menu and the AI `complete()` refactor. [Research](docs/roadmap/composer-editor.md#ai-writing-in-the-editor-with-your-own-key)
- [ ] **Find and replace inside the composer** *(new)* — Editing long messages and templates ("replace Q3 with Q4") needs it, and Mod+F in the composer should search the draft, not the mailbox.
  Effort: S. [Research](docs/roadmap/composer-editor.md#find-and-replace-inside-the-composer)
- [ ] **Editor accessibility and IME test matrix** *(new)* — Block editors are where screen-reader and IME regressions hide.
  Effort: M. Run it before the editor ships to all users. [Research](docs/roadmap/composer-editor.md#editor-accessibility-and-ime-test-matrix)

### Productivity

- [ ] **Snooze with natural-language times ("2d", "monday 9am"), backed by an IMAP folder so it survives reinstalls** — Snooze turns the inbox into a to-do list ("deal with this Monday").
  Effort: L. Depends on: Multi-select (batch), keyword support (section findings 6), "Thread by Message-ID…" (P0) for early wake, and "Launch at login" (P1) for reliability. [Research](docs/roadmap/organization.md#snooze-with-natural-language-times-2d-monday-9am-backed-by-an-imap-folder-so-it)
- [ ] **Send later, with "send now instead"** — People write at night and send in the morning, and work across time zones.
  Effort: M. Depends on: the scheduler from the Snooze item, and "Undo send with a configurable delay" (P1), which should share it. [Research](docs/roadmap/organization.md#send-later-with-send-now-instead)
- [ ] **Follow-up reminders: remind me if nobody replies by a date** — Chasing unanswered requests is a common sales and management workflow, and it otherwise needs a to-do app.
  Effort: M. Depends on: the scheduler (Snooze item) and "Thread by Message-ID / References…" (P0). [Research](docs/roadmap/organization.md#follow-up-reminders-remind-me-if-nobody-replies-by-a-date)
- [ ] **Templates with subject, and quick-insert in the composer** — It removes the retyping of recurring replies (support, recruiting, sales).
  Effort: S–M. Depends on: "Rich text / HTML compose" (P1) for HTML templates; "Save drafts to the server" (P0) for roaming. [Research](docs/roadmap/organization.md#templates-with-subject-and-quick-insert-in-the-composer)
- [ ] **Mail rules / filters: match on from, to, subject, body, Reply-To, list; actions move, label, mark, star, forward, delete; run on arrival and on demand** — It is the power-user feature for high-volume inboxes.
  Effort: L (local), plus L (server-side export). Depends on: Multi-select (batch), "Move to any folder…", keyword support, and the Search item (new fields). [Research](docs/roadmap/organization.md#mail-rules-filters-match-on-from-to-subject-body-reply-to-list-actions-move-labe)
- [ ] **Sync Gmail filters and vacation responder through the Gmail API** — Gmail users already have dozens of server filters.
  Effort: M. Depends on: "Mail rules / filters" for the shared editor (optional). [Research](docs/roadmap/organization.md#sync-gmail-filters-and-vacation-responder-through-the-gmail-api)
- [ ] **Saved searches / smart folders** — It is the most requested "organization without folders" feature (Mailspring forum #528, 20 votes; GitHub #514, 11 👍).
  Effort: S–M. Depends on: the Search item (`in:`). [Research](docs/roadmap/organization.md#saved-searches-smart-folders)
- [ ] **Search: natural-language dates, `in:`, `NOT`, autocomplete, "find by sender / subject" from the context menu** — Search is how people find old mail.
  Effort: M. Depends on: "Cc and Bcc fields" (P0) for `cc:`; "Sync every folder" (P0) for `in:` beyond the role folders. [Research](docs/roadmap/organization.md#search-natural-language-dates-in-not-autocomplete-find-by-sender-subject-from-th)
- [ ] **Server-side search fallback (IMAP SEARCH, Gmail `X-GM-RAW`) for mail outside the sync window** — The default window is 90 days.
  Effort: M. Depends on: "Two long-lived IMAP connections per account" (P0) for speed; the Search item for the grammar. [Research](docs/roadmap/organization.md#server-side-search-fallback-imap-search-gmail-x-gm-raw-for-mail-outside-the-sync)
- [ ] **Command palette for every action** — It makes every action discoverable and teaches shortcuts.
  Effort: S–M. Depends on: nothing; better after the shortcut-presets registry. [Research](docs/roadmap/organization.md#command-palette-for-every-action)
- [ ] **Shortcut presets (Gmail, Outlook, Superhuman) and remapping** — Switchers keep their muscle memory: "e" archives in Gmail, Ctrl+Q marks read in Outlook.
  Effort: M. Depends on: the command registry (Command palette item). [Research](docs/roadmap/organization.md#shortcut-presets-gmail-outlook-superhuman-and-remapping)
- [ ] **"Go to unread" and other `g`-prefixed navigation** — Keyboard users jump between views without the mouse.
  Effort: S. Depends on: Shortcut presets (sequence support). [Research](docs/roadmap/organization.md#go-to-unread-and-other-g-prefixed-navigation)
- [ ] **Send and archive** — Inbox-zero users answer and file in one step.
  Effort: S. Depends on: Undo send / Send later for correct ordering, if present. [Research](docs/roadmap/organization.md#send-and-archive)
- [ ] **Pin conversations to the top** — It keeps a few active conversations visible above new mail without starring everything.
  Effort: S. [Research](docs/roadmap/organization.md#pin-conversations-to-the-top)
- [ ] **Mute conversation** — It stops noisy reply-all threads without leaving them.
  Effort: S–M. Depends on: "Thread by Message-ID…" (P0), the rules arrival hook, and keyword support. [Research](docs/roadmap/organization.md#mute-conversation)
- [ ] **IMAP keywords as tags, with label colors** *(new)* — Non-Gmail users want Gmail-like labels without folders (Mailspring forum #540, 15 votes; GitHub #1006).
  Effort: M. Depends on: keyword plumbing (section findings 6). [Research](docs/roadmap/organization.md#imap-keywords-as-tags-with-label-colors)
- [ ] **Auto-advance after archive or delete, with a setting** *(new)* — Keyboard triage flow: after `e`, the next conversation should open (forum #493).
  Effort: S. [Research](docs/roadmap/organization.md#auto-advance-after-archive-or-delete-with-a-setting)
- [ ] **Edit as new / resend** *(new)* — It resends a sent message to someone else, or reuses a past email as a starting point (forum #456).
  Effort: S. Depends on: "Forward, with original attachments carried over" (P0) for attachments. [Research](docs/roadmap/organization.md#edit-as-new-resend)
- [ ] **Sidebar customization: hide, show, reorder and favorite folders** *(new)* — Accounts with many folders bury the ones people use (forum #469, #1904, #688).
  Effort: S–M. Depends on: "Sync every folder…" (P0). [Research](docs/roadmap/organization.md#sidebar-customization-hide-show-reorder-and-favorite-folders)
- [ ] **Select by state: select all read, unread, starred, unstarred** *(new)* — Quick bulk clean-up, such as "select all read → archive".
  Effort: S. Depends on: Multi-select and bulk actions. [Research](docs/roadmap/organization.md#select-by-state-select-all-read-unread-starred-unstarred)

### Inbox triage

- [ ] **Split inbox / smart categories: people, notifications, newsletters** — Person-to-person mail gets buried under receipts and newsletters.
  Effort: M. Depends on: header retention (Unsubscribe item). The Saved searches item enables custom splits. [Research](docs/roadmap/organization.md#split-inbox-smart-categories-people-notifications-newsletters)
- [ ] **Gmail category tabs (Primary, Social, Promotions, Updates)** — Gmail users already trained Gmail's categories, and seeing a different split in Alice is jarring.
  Effort: S–M. Depends on: "Split inbox / smart categories" (tabs UI). [Research](docs/roadmap/organization.md#gmail-category-tabs-primary-social-promotions-updates)
- [ ] **Sender screener for first-time senders** — It stops cold outreach and new newsletters from reaching the inbox until the user approves the sender once.
  Effort: M. Depends on: the rules arrival hook and Block sender. [Research](docs/roadmap/organization.md#sender-screener-for-first-time-senders)

### Sync

- [ ] **Per-account and per-folder sync settings: sync window, which folders sync, and Gmail All Mail scope** *(new)* — One global window does not fit a 15-year archive account next to a small work account.
  Effort: M. Depends on "Sync every folder…" and "Edit an existing account's settings; reorder accounts; per-account color". [Research](docs/roadmap/sync.md#per-account-and-per-folder-sync-settings-sync-window-which-folders-sync-and-gmai)
- [ ] **Cross-account move and copy of messages and folders** *(new)* — Mail arrives at the wrong account (personal vs work), and users consolidating providers want to move whole folders.
  Effort: M for messages; +M for folders. Depends on "Offline action queue…", "Sync every folder…" (pickers), and "Create, rename, delete and nest folders and labels" (folder transfer). [Research](docs/roadmap/sync.md#cross-account-move-and-copy-of-messages-and-folders) · [UI research](docs/roadmap/organization.md#move-messages-between-accounts)
- [ ] **Pause an account: stop syncing, IDLE and sends for an account without removing it** *(new)* — When a company server is down for maintenance, or an account is temporarily unwanted, the client keeps retrying, logging errors and draining battery.
  Effort: S. Better after "Two long-lived IMAP connections per account…" and "Offline action queue…", but the sync-skip part ships now. [Research](docs/roadmap/sync.md#pause-an-account-stop-syncing-idle-and-sends-for-an-account-without-removing-it)

### Notifications and settings

- [ ] **Notification rules per account and folder: custom sound, per-account on/off and "only people I know", notify for folders other than Inbox** *(new)* — All-or-nothing notifications fail people with a work and a personal account, and people whose server filters skip the Inbox.
  Effort: M. Depends on "Sync every folder, not only the five special-use roles", "Multi-folder message model" and "Respect Do Not Disturb / Focus". [Research](docs/roadmap/platform-security-pim.md#notification-rules-per-account-and-folder-custom-sound-per-account-onoff-and-onl)
- [ ] **Settings export / import, and sync across devices** *(new)* — Setting up a second machine, or reinstalling, means redoing accounts, signatures, the AI provider and every preference.
  Effort: M (file), L (IMAP sync). Depends on "Per-account signatures", "Templates with subject…" and "Mail rules / filters" (for more payload), and "Sync every folder…" (to hide the settings folder). [Research](docs/roadmap/platform-security-pim.md#settings-export-import-and-sync-across-devices)

### Contacts and calendar

- [ ] **Contact editor: create, edit, delete; groups; notes; sync back to Google, Graph, CardDAV** — People switching clients expect to fix a misspelled name, add a phone number or build a group from the address book they already use.
  Effort: XL. **Note:** Split this into "Local address book cache with incremental sync" (L) and "Contact editor with write-back" (L). Depends on "Offline action queue" (share the outbox), and enables "Contact groups expand in recipient fields" and "vCard import and export". [Research](docs/roadmap/platform-security-pim.md#contact-editor-create-edit-delete-groups-notes-sync-back-to-google-graph-carddav)
- [ ] **vCard import and export** — It is the way people move address books between apps and back them up.
  Effort: M after the cache from "Contact editor" exists. Without the cache, export of synthesised cards alone is S. [Research](docs/roadmap/platform-security-pim.md#vcard-import-and-export)
- [ ] **Contact sidebar in the reader: avatar, recent threads, other addresses** — It answers "who is this and what have we talked about" without leaving the message, which is the context people use to triage.
  Effort: M. Benefits from the cache in "Contact editor" for other addresses and photos; works without it. [Research](docs/roadmap/platform-security-pim.md#contact-sidebar-in-the-reader-avatar-recent-threads-other-addresses)
- [ ] **Calendar view: day, week, month, agenda; create, edit and drag events; recurrence editing; time zones** — This is the most requested Mailspring feature ever: GitHub #199 (497 👍) and forum #85 (68 votes).
  Effort: XL in total (phases L + L + L + M). **Note:** Ship phases 1 and 2 (cached, read-only week/agenda) as the MVP. That alone beats Mailspring's "Preview" for Google and Microsoft users, since Mailspring has no Graph support. Depends on "CalDAV discovery for Fastmail, Yahoo, iCloud" and "Local address book cache" (which shares the sync-loop design). Phase 4 benefits from "Respect Do Not Disturb / Focus". [Research](docs/roadmap/platform-security-pim.md#calendar-view-day-week-month-agenda-create-edit-and-drag-events-recurrence-editi)
- [ ] **CalDAV discovery for Fastmail, Yahoo, iCloud** — Typing `https://caldav.icloud.com` and knowing to make an app-specific password is where people give up.
  Effort: S–M. Depends on "Presets with setup tips: iCloud, Yahoo, …" (shares the table) and "(NEW) Don't send DAV Basic credentials across hosts on redirect". [Research](docs/roadmap/platform-security-pim.md#caldav-discovery-for-fastmail-yahoo-icloud)

### Reading

- [x] **Layout: list above message (vertical stack) in addition to current layouts** — Done: Alice ships Single, Horizontal and Vertical layouts. Only a resizable split is missing.
  [Research](docs/roadmap/reading-and-accounts.md#layout-list-above-message-vertical-stack-in-addition-to-current-layouts)
- [ ] **Full-width message view with optional max width** — On wide or ultrawide screens, wide HTML newsletters and tables get squeezed into a column and scroll horizontally, while half the window is empty.
  Effort: S. [Research](docs/roadmap/reading-and-accounts.md#full-width-message-view-with-optional-max-width)
- [ ] **Interface zoom** — Users with low vision or high-DPI/small screens need the whole UI larger or smaller.
  Effort: S. [Research](docs/roadmap/reading-and-accounts.md#interface-zoom)
- [ ] **Export messages and folders as .eml and mbox; drag a thread into the composer to attach it** — People keep records, move to other clients and send evidence.
  Effort: M for export. Drag-to-attach is S after "Composer attachments", which lives in another section. [Research](docs/roadmap/reading-and-accounts.md#export-messages-and-folders-as-eml-and-mbox-drag-a-thread-into-the-composer-to-a)
- [ ] **Permalinks to a thread** — Linking a mail from notes, tasks or calendar events ("see alice://…") is a common power-user workflow (Apple Mail `message:` links in Notes and OmniFocus).
  Effort: M. [Research](docs/roadmap/reading-and-accounts.md#permalinks-to-a-thread)
- [ ] **TNEF (winmail.dat) decoding** *(new)* — Outlook and Exchange sometimes send "Rich Text" messages whose real attachments are packed into a single `winmail.dat` (`application/ms-tnef`).
  Effort: S–M (S for attachments only). [Research](docs/roadmap/reading-and-accounts.md#tnef-winmaildat-decoding)

### Diagnostics and support

- [ ] **Structured logging to rotating files** — When sync fails, the user and the maintainers see one sentence (`account.sync_error`).
  Effort: S–M. No dependencies. [Research](docs/roadmap/performance.md#structured-logging-to-rotating-files)
- [ ] **Copy last N sync log lines from account settings** — Users can't attach useful details to a bug report.
  Effort: S. Depends on: "Structured logging to rotating files (`tracing`)". [Research](docs/roadmap/performance.md#copy-last-n-sync-log-lines-from-account-settings)
- [ ] **Verbose IMAP/SMTP protocol log during account setup** — Most setup failures (TLS versions, auth mechanisms, wrong port or security, server quirks) are obvious in a transcript and invisible in one error sentence.
  Effort: M (S for IMAP only). Depends on: "Structured logging to rotating files (`tracing`)" for the shared redaction helper. [Research](docs/roadmap/performance.md#verbose-imapsmtp-protocol-log-during-account-setup)
- [ ] **Opt-in crash reporting** — Today a daemon panic is invisible.
  Effort: M for local reports and UI; +M for opt-in Sentry. Depends on: "Structured logging to rotating files (`tracing`)". [Research](docs/roadmap/performance.md#opt-in-crash-reporting)
- [ ] **Reset cache for one account** — This is the standard fix for a corrupted or confused local state (stuck sync, phantom messages) without removing and re-adding the account, which loses its settings.
  Effort: S. No dependencies. [Research](docs/roadmap/performance.md#reset-cache-for-one-account)
- [ ] **Automatic repair of folder and unread counts** — Badges and folder counts that disagree with the list erode trust.
  Effort: S once the schema exists. Depends on: "Move hot fields … into real indexed columns" (phase 2). [Research](docs/roadmap/performance.md#automatic-repair-of-folder-and-unread-counts)
- [ ] **Periodic SQLite maintenance (VACUUM / incremental vacuum, ANALYZE)** — Deleting and re-downloading mail fragments the file and leaves free pages.
  Effort: S–M. Depends on: "Move hot fields … into real indexed columns". [Research](docs/roadmap/performance.md#periodic-sqlite-maintenance-vacuum-incremental-vacuum-analyze)
- [ ] **Database corruption recovery** *(new)* — A malformed `mail.sqlite3` currently fails `Store::open`, and the daemon cannot start.
  Effort: M. Depends on: "Structured logging to rotating files (`tracing`)". [Research](docs/roadmap/performance.md#database-corruption-recovery)

### Performance

- [ ] **Virtualized message list with infinite scroll** — Today the list shows 50 conversations per page with Previous/Next buttons.
  Effort: M. Depends on: "Page the thread list in SQL…", and preferably "Push store changes to the UI". [Research](docs/roadmap/performance.md#virtualized-message-list-with-infinite-scroll)
- [ ] **Push store changes to the UI (SSE / websocket) instead of 2s polling** — Polling costs a full `/v1/state` every 2 s per client, even when idle; today that includes a whole-mailbox `threads()` scan.
  Effort: S–M. Depends on: "Separate read connections (pool)…" for the post-commit revision signal. [Research](docs/roadmap/performance.md#push-store-changes-to-the-ui-sse-websocket-instead-of-2s-polling)
- [ ] **Chunk sync writes and skip no-op updates (batching mostly done)** — Write cost and lock hold time directly limit how long the UI waits during sync.
  Effort: S. Depends on: "Separate read connections (pool)…" for measurement; step 4 depends on the hot-columns item. [Research](docs/roadmap/performance.md#chunk-sync-writes-and-skip-no-op-updates-batching-mostly-done)
- [ ] **Cap and measure daemon memory under large mailboxes; add a long-running soak test** — The daemon runs all day in the background.
  Effort: M. Depends on: "Add a benchmark fixture…", and "Page the thread list in SQL…" for the main cap. [Research](docs/roadmap/performance.md#cap-and-measure-daemon-memory-under-large-mailboxes-add-a-long-running-soak-test)
- [ ] **Composer stays responsive with very long quoted text and many recipients** — Typing lag in a reply to a long thread, or in a message to a large list, is very noticeable and makes the app feel broken.
  Effort: S (steps 1, 3), M (step 2). Depends on: "Forward, with original attachments carried over", "Reply-all…", "Cc and Bcc fields". [Research](docs/roadmap/performance.md#composer-stays-responsive-with-very-long-quoted-text-and-many-recipients)

### Localization and accessibility

- [ ] **More locales: fr, es, it, pt-BR, ja, zh-CN, ko, ru, nl, pl, sv, tr, uk, he, ar (machine-first, human-reviewed later)** — Most people outside the US and UK run their OS in another language.
  Effort: M for infrastructure plus S per machine-translated language (L total for 15). Depends on "Full RTL interface" for he/ar; ship those two after RTL or keep them hidden. [Research](docs/roadmap/reading-and-accounts.md#more-locales-fr-es-it-pt-br-ja-zh-cn-ko-ru-nl-pl-sv-tr-uk-he-ar-machine-first-hu)
- [ ] **Full RTL interface** — Hebrew and Arabic users need a mirrored UI.
  Effort: M (content direction S; CSS conversion M). [Research](docs/roadmap/reading-and-accounts.md#full-rtl-interface)
- [ ] **Screen-reader pass: roving tabindex in lists, focus traps in modals, landmarks** — Email is a core task for blind and low-vision users.
  Effort: M. Related: "Full RTL interface" (shared `styles.css` pass). [Research](docs/roadmap/reading-and-accounts.md#screen-reader-pass-roving-tabindex-in-lists-focus-traps-in-modals-landmarks)

### From the settings and menu audit

These come from comparing every Mailspring preference and menu command with Alice ([Appendix A](docs/roadmap/mailspring-settings.md)). Each is small.

- [ ] **24-hour clock override** *(new)* — Alice follows the locale only. Mailspring has a "Use 24-hour clock" switch. Effort: S. [Appendix A](docs/roadmap/mailspring-settings.md#general)
- [ ] **Delete-key behavior: archive or trash** *(new)* — Mailspring's "Move to trash (not archive) on swipe / backspace". Users coming from Apple Mail expect Delete to trash. Effort: S. [Appendix A](docs/roadmap/mailspring-settings.md#reading)
- [ ] **Sounds for sent and new mail** *(new)* — Mailspring plays a sent sound and a new-mail sound, each with its own switch. Effort: S. [Appendix A](docs/roadmap/mailspring-settings.md#sending)
- [ ] **Attachment download behavior** *(new)* — Choose between opening the file and revealing it in the folder. Mailspring has "Open containing folder after downloading attachment". Effort: S. [Appendix A](docs/roadmap/mailspring-settings.md#composing-and-attachments)
- [ ] **Find in thread** *(new)* — Search inside the open conversation, with Next/Previous. This is Mailspring's Edit › Find in Thread. Effort: S. [Appendix B](docs/roadmap/mailspring-settings.md#appendix-b-mailspring-menu-commands)
- [ ] **Full recipient names option** *(new)* — Show first and last names of every recipient instead of short names. Effort: S. [Appendix A](docs/roadmap/mailspring-settings.md#reading)
- [ ] **Reset accounts and settings** *(new)* — A factory reset in Settings, next to the per-account cache reset. Effort: S. [Appendix A](docs/roadmap/mailspring-settings.md#local-data)

## P3: differentiators and nice-to-haves

- [ ] **Read later / set aside list** — It gives a place for "not now but not done" mail without misusing star or snooze.
  Effort: S. Depends on: keyword support. [Research](docs/roadmap/organization.md#read-later-set-aside-list)
- [ ] **Import from mbox / Thunderbird / Apple Mail** — Switchers have years of archived local mail (Thunderbird "Local Folders", Apple Mail "On My Mac") that exists on no server.
  Effort: L. [Research](docs/roadmap/reading-and-accounts.md#import-from-mbox-thunderbird-apple-mail)
- [ ] **OpenPGP: sign, encrypt, decrypt, verify; key import and WKD/Autocrypt lookup** — The most requested missing Mailspring feature (GitHub #25, 144 👍; forum #83, 60 votes).
  Effort: XL. Depends on "Rich text / HTML compose..." (for HTML in signed mail), "Attachment upload..." (key attachment), "Correct MIME for replies..." and "Cc and Bcc fields" (encrypting to all recipients). [Research](docs/roadmap/long-tail.md#openpgp-sign-encrypt-decrypt-verify-key-import-and-wkdautocrypt-lookup)
- [ ] **S/MIME: sign, encrypt, verify with system certificates** — Many companies, universities and governments issue S/MIME certificates and require signed or encrypted mail; Outlook and Apple Mail users expect it (Mailspring GitHub #28, #940).
  Effort: XL (phase 1 L). Shares most UI and MIME plumbing with the OpenPGP item; do them together. [Research](docs/roadmap/long-tail.md#smime-sign-encrypt-verify-with-system-certificates)
- [ ] **Read receipts: request and respond to `Disposition-Notification-To`** — Workplace users (Outlook-heavy organisations) send and expect read receipts.
  Effort: M. Depends on "Cc and Bcc fields" for the To/Cc check; otherwise independent. [Research](docs/roadmap/long-tail.md#read-receipts-request-and-respond-to-disposition-notification-to)
- [ ] **Translation of incoming mail, using the BYOK AI provider** — People who get mail in languages they don't read can understand it in place, without pasting it into a web translator (which leaks the mail to yet another service).
  Effort: M. No dependencies; shares the `complete()` refactor with "AI thread summaries and reply drafts using the existing BYOK setup". [Research](docs/roadmap/long-tail.md#translation-of-incoming-mail-using-the-byok-ai-provider)
- [ ] **AI thread context and inline summaries (per-message summaries and drafts already ship)** — Catching up on a long conversation in one paragraph, and starting a reply from a draft that already knows the thread, is the most-used AI feature in Superhuman and Spark.
  Effort: M. Depends on "Thread by `Message-ID` / `References` / `In-Reply-To`..." (P0) for correct context; shares the `complete()` refactor with translation. [Research](docs/roadmap/long-tail.md#ai-thread-context-and-inline-summaries-per-message-summaries-and-drafts-already)
- [ ] **Grammar check (optional, local or self-hosted LanguageTool)** — Catches grammar and style mistakes before sending, for people writing in a second language in particular, without sending drafts to Grammarly.
  Effort: M. Easier after "Rich text / HTML compose..." is designed, since the underline layer must work with whichever editor ships. [Research](docs/roadmap/long-tail.md#grammar-check-optional-local-or-self-hosted-languagetool)
- [ ] **Masked-address generation through Fastmail, SimpleLogin or addy.io** — Giving each correspondent or service its own address, and turning off one that starts getting spam, is a strong privacy feature that people currently do in a separate app.
  Effort: M. Depends on "Send-as identities and aliases, including Gmail "Send mail as" import" for sending from the alias. [Research](docs/roadmap/long-tail.md#masked-address-generation-through-fastmail-simplelogin-or-addyio)
- [ ] **Image proxy for remote images to hide IP and open time** — Loading remote images tells senders your IP address (approximate location), your browser engine and the moment you opened the message.
  Effort: M for proxy-on-open, L with prefetch and cache. Depends on "Never auto-load images in Spam" and "Block remote CSS and web fonts along with images" (P1); IP hiding depends on "Proxy support (SOCKS5 / HTTP) for IMAP, SMTP and HTTP APIs". [Research](docs/roadmap/long-tail.md#image-proxy-for-remote-images-to-hide-ip-and-open-time)
- [ ] **Tracking-pixel detection and stripping, with a "trackers blocked" indicator** — Even people who load images don't want to report every open to marketing tools; showing "2 trackers blocked" makes the protection visible.
  Effort: S (reader only), M with the list badge. [Research](docs/roadmap/long-tail.md#tracking-pixel-detection-and-stripping-with-a-trackers-blocked-indicator)
- [ ] **Linux packaging: Flatpak, Snap, deb, rpm, AppStream metadata; Wayland and tray fixes** — Linux users install from Flathub, their distro's format or the Snap Store; a single `.deb` built on Ubuntu 24.04 reaches only part of them.
  Effort: L. Related: "Windows jump list and Linux tray icon theme", "Launch at login (... XDG autostart / Flatpak portal)". [Research](docs/roadmap/long-tail.md#linux-packaging-flatpak-snap-deb-rpm-appstream-metadata-wayland-and-tray-fixes)
- [ ] **Windows jump list and Linux tray icon theme** — Right-clicking the taskbar icon to compose is standard on Windows; on Linux a full-colour app icon in a monochrome panel looks wrong and is hard to see on dark panels.
  Effort: M. Depends on "Register as `mailto:` handler..." (single-instance plumbing) and "Dock / taskbar unread badge..." for unread state. [Research](docs/roadmap/long-tail.md#windows-jump-list-and-linux-tray-icon-theme)
- [ ] **Swipe gestures on trackpads, with an option to disable** — Two-finger swipe to archive is muscle memory for people coming from Apple Mail, Spark and Mailspring on Mac laptops.
  Effort: M. Benefits from "Undo for archive, trash, move, mark read" and "Snooze with natural-language times...". [Research](docs/roadmap/long-tail.md#swipe-gestures-on-trackpads-with-an-option-to-disable)
- [ ] **Screenshot / privacy mode that blurs content** — Lets people share their screen, record a demo or take screenshots for bug reports without exposing mail.
  Effort: S. [Research](docs/roadmap/long-tail.md#screenshot-privacy-mode-that-blurs-content)
- [ ] **Proxy support (SOCKS5 / HTTP) for IMAP, SMTP and HTTP APIs** — Needed behind corporate proxies, in countries that block mail providers, and for people who route mail through Tor.
  Effort: M. Related: "Two long-lived IMAP connections per account..." (P0) changes where connections are opened; do this after it. [Research](docs/roadmap/long-tail.md#proxy-support-socks5-http-for-imap-smtp-and-http-apis)
- [ ] **Column layouts that send as tables** *(new)* — Side-by-side content (two options, before/after) is common in newsletters and proposals.
  Effort: M. [Research](docs/roadmap/composer-editor.md#column-layouts-that-send-as-tables)
- [ ] **Link bookmark cards** *(new)* — Pasting a URL and getting a titled card is a Notion and Slack habit, and it reads better than a bare link.
  Effort: M. [Research](docs/roadmap/composer-editor.md#link-bookmark-cards)
- [ ] **Math blocks rendered to images for sending** *(new)* — Academics and engineers write formulas.
  Effort: M. [Research](docs/roadmap/composer-editor.md#math-blocks-rendered-to-images-for-sending)
- [ ] **Table of contents for long messages** *(new)* — Long updates and newsletters benefit from an outline while editing; Notion's TOC block shows it in the page.
  Effort: S. [Research](docs/roadmap/composer-editor.md#table-of-contents-for-long-messages)
- [ ] **Focus mode and a page-style composer** *(new)* — Writing a long mail in a full-height, centered page (subject as the page title) is the Notion feel.
  Effort: S–M. Depends on the pop-out composer window. [Research](docs/roadmap/composer-editor.md#focus-mode-and-a-page-style-composer)
- [ ] **Local draft history snapshots** *(new)* — "I deleted the paragraph I needed" is recoverable in Notion.
  Effort: S–M. [Research](docs/roadmap/composer-editor.md#local-draft-history-snapshots)
- [ ] **Paste Markdown as blocks and copy as Markdown** *(new)* — Many people draft in Markdown (notes apps, ChatGPT, GitHub).
  Effort: S. [Research](docs/roadmap/composer-editor.md#paste-markdown-as-blocks-and-copy-as-markdown)
- [ ] **Custom sort order: by sender, subject, size, unread first, oldest first** *(new)* — Forum #470 (21 votes) and GitHub #361.
  Effort: S–M. Depends on: "Move hot fields … into real indexed columns" (P0). [Research](docs/roadmap/organization.md#custom-sort-order-by-sender-subject-size-unread-first-oldest-first)
- [ ] **Local adaptive spam classifier** *(new)* — Many IMAP providers (small hosts, self-hosted) filter poorly (forum #834 "Adaptive junk filtering").
  Effort: M. Depends on: the rules arrival hook and keyword support. [Research](docs/roadmap/organization.md#local-adaptive-spam-classifier)
- [ ] **Attachment position option: show attachments above the body** *(new)* — In long messages and threads, attachments at the bottom are easy to miss, especially on invoices and forwards.
  Effort: S. [Research](docs/roadmap/reading-and-accounts.md#attachment-position-option-show-attachments-above-the-body)
- [ ] **Local folders: keep mail on this device only, outside any account** *(new)* — People archive old mail off the server to stay under quota or keep it after leaving a job (Mailspring forum #784, 10 votes).
  Effort: L. Depends on "Export messages and folders as .eml and mbox..." and "Import from mbox..." (P2), which share the raw-message storage, and "Create, rename, delete and nest folders and labels" (P1) for the folder UI. [Research](docs/roadmap/long-tail.md#local-folders-keep-mail-on-this-device-only-outside-any-account)
- [ ] **Choose which accounts the unified inbox includes** *(new)* — People with a work and a personal account, or a noisy shared mailbox, want "All Inboxes" without one of them (Mailspring GitHub #38, 20 👍; forum #136, 12 votes).
  Effort: S. [Research](docs/roadmap/long-tail.md#choose-which-accounts-the-unified-inbox-includes)
- [ ] **Mouse back/forward buttons and history navigation** *(new)* — People with five-button mice expect "back" to return from a message to the list, or to the previous folder (Mailspring forum #761, 6 votes).
  Effort: S. Related: "Swipe gestures on trackpads..." (a two-finger swipe could later map to back/forward in the reader). [Research](docs/roadmap/long-tail.md#mouse-backforward-buttons-and-history-navigation)

## P4: long tail

- [ ] **Microsoft Graph mail backend for tenants that block IMAP (not EWS: it is being switched off)** — Many Microsoft 365 tenants disable IMAP, and SMTP AUTH is going away by default, so Outlook work accounts may not be able to use Alice at all (Mailspring GitHub #34, 67 👍; #258, 16 👍).
  Effort: XL (send-only fallback: M). Depends on "Multi-folder message model..." (P0), "Sync every folder..." (P0) and "Verify Outlook/O365 OAuth against a live account; ship a built-in Microsoft client id" (P1). [Research](docs/roadmap/long-tail.md#microsoft-graph-mail-backend-for-tenants-that-block-imap-not-ews-it-is-being-swi)
- [ ] **JMAP backend (Fastmail, Stalwart)** — JMAP is faster and simpler than IMAP (one HTTP request per batch, server-side threads, cheap change tracking, push over HTTP), and it is the only way to reach Fastmail features such as Masked Email.
  Effort: L–XL. Depends on "Multi-folder message model..." and the backend split from the Graph item (do Graph or JMAP first, then the other is cheaper). [Research](docs/roadmap/long-tail.md#jmap-backend-fastmail-stalwart)
- [ ] **Mail merge from CSV** — Small organisations, teachers and event organisers send the same message to many people with personal fields and want each to arrive as a normal one-to-one email from their own account.
  Effort: M. Depends on "Cc and Bcc fields" and, for real use, "Attachment upload..." and "Rich text / HTML compose...". [Research](docs/roadmap/long-tail.md#mail-merge-from-csv)
- [ ] **Custom themes now; defer a plugin system** — Lets the community add niche features and looks without growing the core app (Mailspring GitHub #363 "Global directory for plugins", 44 👍; forum #318).
  Effort: S for themes; XL for plugins. [Research](docs/roadmap/long-tail.md#custom-themes-now-defer-a-plugin-system)
- [ ] **Mobile apps (Android, iOS) on the shared Rust core** — People read mail on phones; a desktop-only client is a second-class choice for many.
  Effort: XL. Depends on "Two long-lived IMAP connections per account..." and "Push store changes to the UI (SSE / websocket)..." for an efficient in-process design. [Research](docs/roadmap/long-tail.md#mobile-apps-android-ios-on-the-shared-rust-core)
- [ ] **Tasks and notes tied to messages** — Turning an email into a to-do, and keeping a private note on a conversation ("waiting for invoice"), is how many people manage work in their inbox.
  Effort: M (notes S). Related to "Follow-up reminders" (P2) and "Snooze..." (P2), which cover the most common "remind me" need. [Research](docs/roadmap/long-tail.md#tasks-and-notes-tied-to-messages)
- [ ] **LDAP and Global Address List directory lookup** *(new)* — In companies and universities, colleagues are in the directory, not in personal contacts; without directory lookup autocomplete fails for most internal recipients (Mailspring GitHub #864, 13 👍; forum #330).
  Effort: M. Related: "S/MIME: sign, encrypt, verify with system certificates" (certificate lookup). [Research](docs/roadmap/long-tail.md#ldap-and-global-address-list-directory-lookup)
- [ ] **Receive-only and send-only accounts** *(new)* — Some mailboxes are only read (archives, shared or no-reply inboxes, providers whose SMTP is blocked), and some senders are only an SMTP relay for a domain (Mailspring forum #134, 8 votes).
  Effort: S for receive-only; send-only depends on "Send-as identities and aliases, including Gmail "Send mail as" import". [Research](docs/roadmap/long-tail.md#receive-only-and-send-only-accounts)
- [ ] **Portable Windows build** *(new)* — People run mail from a USB drive or on machines where they cannot install software (Mailspring GitHub #922, 13 👍).
  Effort: M. [Research](docs/roadmap/long-tail.md#portable-windows-build)
- [ ] **GNOME Online Accounts / KDE accounts integration** *(new)* — GNOME users expect an account added in Settings → Online Accounts to appear in their mail client (Mailspring forum #718, 11 votes; GitHub #466).
  Effort: S–M. Low value relative to effort elsewhere; keep in P4. [Research](docs/roadmap/long-tail.md#gnome-online-accounts-kde-accounts-integration)

## Housekeeping

- [ ] **Update `README.md` "Current boundaries": `cid:` inline images are implemented** — The README tells users and contributors a shipped feature is missing.
  Effort: S. [Research](docs/roadmap/long-tail.md#update-readmemd-current-boundaries-cid-inline-images-are-implemented)
- [ ] **Update `README.md` as each item above ships** — 
  Effort: S per item. [Research](docs/roadmap/long-tail.md#update-readmemd-as-each-item-above-ships)

## Won't do

Researched and deliberately left out. Revisit only if demand changes.

- **Shared drafts and team comments on threads** — Needs a hosted collaboration service that stores other people's mail. Forwarding a draft covers the rare need. [Research](docs/roadmap/long-tail.md#shared-drafts-and-team-comments-on-threads)
- **Share a thread as a public link** — Publishing mail to a public URL is a privacy liability and needs hosting. Print to PDF, .eml export and `alice://` permalinks cover sharing. [Research](docs/roadmap/long-tail.md#share-a-thread-as-a-public-link)
- **Open and link tracking for outgoing mail (`MS` Pro) — consider skipping on privacy grounds** — Needs a public server, contradicts Alice's tracker blocking, and Apple Mail Privacy Protection makes open data meaningless. Read receipts (P3) cover the honest need. [Research](docs/roadmap/long-tail.md#open-and-link-tracking-for-outgoing-mail-ms-pro-consider-skipping-on-privacy-gro)
- **POP3 accounts** — IMAP is near-universal; POP3 removes mail from the server and breaks multi-device state. Mailspring rejected it too (GitHub #45). [Research](docs/roadmap/long-tail.md#pop3-accounts)
- **Exchange Web Services (EWS)** — Microsoft is switching EWS off in Exchange Online (phased from 2026-10-01, fully off 2027-04-01). Use Microsoft Graph (P4) instead. [Research](docs/roadmap/long-tail.md#microsoft-graph-mail-backend-for-tenants-that-block-imap-not-ews-it-is-being-swi)
