# Composer

Research behind the matching items in [`todo.md`](../../todo.md). Each entry covers why it matters, how Mailspring and other clients do it, the relevant specs, what Alice has today, a build plan, pitfalls, acceptance criteria and effort. Code citations point at the Alice working tree as of 2026-10-13; Mailspring paths are relative to [Foundry376/Mailspring](https://github.com/Foundry376/Mailspring) (`mailspring/`) and [Foundry376/Mailspring-Sync](https://github.com/Foundry376/Mailspring-Sync) (`mailsync/`).

Path conventions: `mailspring/…` = `scratchpad/repos/mailspring/app/…`, `mailsync/…` = `scratchpad/repos/mailsync/…`, `mailcore2/…` = `scratchpad/repos/mailsync/Vendor/mailcore2/src/core/…`. Alice paths are relative to the repo root. Changelog versions are from `ms.md`; demand counts (GitHub 👍 / forum votes) are from `ms-issues-top.txt` and `ms-forum-top.txt`.

## Section findings

1. **One data-model change unblocks most of the P0 items.** `Compose` has only `id, account_id, to, subject, body, in_reply_to` (`crates/alice-core/src/model.rs:222-229`), and `Message` has no `cc`, `bcc` or `references` (`model.rs:89-117`). Sync parses only To and Reply-To (`crates/alice-core/src/providers.rs:399-400`). Do one migration with serde-defaulted fields:
   - `Compose`: `from`, `cc`, `bcc`, `reply_to`, `references`, `html_body`, `attachments`.
   - `Message`: `cc`, `bcc`, `references`.

   `Compose` is `deny_unknown_fields`, so the desktop, CLI and daemon must ship together (they already do). The localStorage backup validator in `apps/desktop/src/drafts.ts:10-16` must accept the new optional fields.

2. **Draft construction should live in the daemon.** Today reply recipients, the `Re:` prefix and the signature are rebuilt in four places:
   - desktop: `apps/desktop/src/App.tsx:207-213`
   - CLI: `crates/alice/src/main.rs:1154-1167` and `:1512-1516`
   - MCP: `crates/alice/src/mcp.rs:217-247`
   - TUI: `crates/alice/src/tui.rs:534-571`

   Reply-all, forward, quoting, References, identities, auto-Cc and signatures would each have to be written four times. Add side-effect-free endpoints that return a prepared `Compose`:
   - `GET /v1/messages/{id}/reply?all=`
   - `GET /v1/messages/{id}/forward`
   - `GET /v1/compose/new?account_id=`

   Every frontend keeps saving through the existing `POST /v1/messages` (`crates/alice-core/src/daemon.rs:472-481`). Mailspring keeps the equivalent logic in one module, `mailspring/src/flux/stores/draft-factory.ts`, plus `Message.participantsForReply*` (`mailspring/src/flux/models/message.ts:341-399`).

3. **lettre 0.11.23, already a dependency, covers the MIME work; no new crate is needed.**
   - It provides `MultiPart::mixed/alternative/related` and `Attachment::new_inline` (which sets `Content-ID` and `inline`).
   - Filenames are RFC 2231-encoded automatically.
   - Bcc is stripped from the formatted message after the envelope is built (`MessageBuilder::build`, `drop_bcc` defaults to true); `keep_bcc()` is available for a Sent copy.
   - `html2text` 0.16 is already in the tree for generating the text/plain alternative.
   - lettre does **not** parse the EHLO `SIZE` value (its `Extension` enum has only 8BITMIME, SMTPUTF8, STARTTLS and AUTH), so size limits must come from per-provider constants plus mapping the `552` error.

   Use the tree mailcore2 builds for mailsync (`mailcore2/rfc822/MCMessageBuilder.cpp:705-768`): `multipart/mixed { multipart/alternative { text/plain, multipart/related { text/html, inline images } }, attachments }`, with each wrapper used only when needed.

4. **Attachments need storage and transport that do not exist yet.**
   - Alice never stores attachment bytes (README "Current boundaries").
   - The daemon caps every request body at 2 MB (`daemon.rs:302`).
   - The desktop reaches the daemon only through JSON `daemon_request` IPC (`apps/desktop/src/api.ts:66-67`, `apps/desktop/src-tauri/src/main.rs:82`).
   - The Tauri window keeps the default `dragDropEnabled`, which swallows HTML5 file drops.
   - CSP `img-src` lacks `blob:` (`apps/desktop/src-tauri/tauri.conf.json`).

   Build a per-draft file store under `client::data_dir()` and a raw-bytes upload route before forward, inline images or drag and drop.

5. **The send pipeline is synchronous and at-most-once.** `POST /send` claims a `deliveries` row and then talks SMTP (`daemon.rs:482-506`, `crates/alice-core/src/store.rs:399-412`, `:447-466`). Undo send, and later send-later, fit as one more state in the same table: `scheduled`, with a `send_at` time. The daemon is a detached process (`crates/alice-core/src/client.rs:126-140`) with background tasks (`daemon.rs:1104-1105`), so a daemon-side outbox survives the window closing.

6. **Mailspring lessons.**
   - Its composer is stuck on a forked Slate 0.45 (`mailspring/package.json:58-66`), with a monkey-patch for Chrome IME (`mailspring/src/components/composer-editor/patch-chrome-ime.ts`). The patch's own comment says upgrading Slate is "effectively a rewrite".
   - The changelog shows composer crash and IME fixes every year: 1.2.0, 1.6.3, 1.13.2, 1.13.3, 1.17.2, 1.19.0, 1.20.0, 1.23.0, 1.24.0.
   - mailsync never builds a full References chain (`mailsync/MailSync/TaskProcessor.cpp:2148`, "todo: lookup thread reference entire chain?").
   - The attachment cap is a hard 25 MB raw.
   - The "attach" keyword check is English-only.

   Alice can do better than Mailspring on every one of these.

7. **Editor choice.** Tauri renders with three engines: WKWebView on macOS, WebKitGTK on Linux and WebView2 (Chromium) on Windows. Raw contenteditable produces different HTML on each and inherits each engine's IME quirks, so a model-based editor matters more here than it did for Electron-only Mailspring. Recommendation: TipTap 3 (ProseMirror), loaded as a lazy Vite chunk. Keep the current `<textarea>` as the plain-text editor. Details are under "Rich text / HTML compose".

8. **Recommended build order.**
   1. Data model and daemon draft factory: Cc/Bcc, References, quoting (NEW), From display name (NEW).
   2. Reply-all.
   3. Attachment store and upload.
   4. Forward.
   5. Signatures, then identities and auto Cc/Bcc.
   6. Undo send. It is independent and can ship any time.
   7. Rich text, then inline images, paste sanitizing and plain-text mode.
   8. Warnings, pop-out window, spellcheck verification, contact groups.

9. **Missing items added below (NEW).**
   - P0: quote the original in replies and forwards. Alice replies start empty today.
   - P0: send a display name in From. `build_mail` sends a bare address (`providers.rs:195`).
   - P1: automatic Cc/Bcc per account.
   - P2: format=flowed and hard wrap at 72 for plain text.
   - P2: Markdown shortcuts.

## P0 → Composer basics

<a id="cc-and-bcc-fields"></a>

### Cc and Bcc fields

*Roadmap: P0 · Composer basics*

- **Why:** Users cannot copy anyone, cannot blind-copy, and reply-all is impossible. Every switcher hits this on day one.
- **Competitors:**
  - **Mailspring UI:**
    - The composer shows To and From. Cc and Bcc appear through header links (`mailspring/internal_packages/composer/lib/composer-header-actions.tsx`) or the shortcuts Mod+Shift+C and Mod+Shift+B (`mailspring/internal_packages/composer/keymaps/composer.json`).
    - They also appear automatically when a draft already has values (`composer-header.tsx:99-110`).
    - Dragging a chip between fields moves it rather than copying it. `participants-text-field.tsx:156-166` removes the address from every other field first.
    - Reply-To became an optional header field in 1.21.0.
    - 1.19.1 fixed VoiceOver on these token fields.
  - **Mailspring send path:**
    - mailsync writes To, Cc and Bcc into the header (`mailsync/MailSync/TaskProcessor.cpp:2158-2174`).
    - mailcore2 strips Bcc from the bytes sent over SMTP (`mailcore2/smtp/MCSMTPSession.cpp:897-899`, `dataWithFilteredBcc`), except on Outlook servers.
    - The copy APPENDed to Sent keeps Bcc (`TaskProcessor.cpp:2350`, `messageDataForSent`), so the sender can see who was blind-copied.
  - **Other clients:** Gmail, Apple Mail and Thunderbird behave the same way. Thunderbird uses one address-row list with a per-row type.
- **Spec:**
  - RFC 5322 §3.6.3 (destination fields). Of the three Bcc methods it allows, use "remove the Bcc line from every transmitted copy". The SMTP envelope (RFC 5321 §3.3 RCPT TO) carries the actual recipients.
  - Google Workspace limit: 100 recipients per message over SMTP, counting To + Cc + Bcc (https://support.google.com/a/answer/166852).
- **Alice today:**
  - `Compose` has `to` only (`model.rs:222-229`).
  - `Message` has `to` and `reply_to`, no cc (`model.rs:89-117`).
  - Sync parses To and Reply-To only (`providers.rs:399-400`).
  - `build_mail` adds To only and requires it to be non-empty (`providers.rs:193-211`).
  - `draft()` limits `to.len() > 100` (`daemon.rs:478`).
  - The desktop has a single To input split on `,`/`;` (`apps/desktop/src/Compose.tsx:38`, `:91`, `:103-108`).
  - CLI `alice compose` has `--to` only (`crates/alice/src/main.rs:57-70`).
  - The MCP `draft_email` tool has `to` only (`crates/alice/src/mcp.rs:127`, `:217-247`).
  - README: "Alice does not keep the names written in To headers".
- **Plan:**
  1. **Model** (`model.rs`):
     - `Compose` gets `#[serde(default)] cc: Vec<String>, bcc: Vec<String>`.
     - `Message` gets `cc: Option<Vec<String>>`, where `None` means the row was cached before Cc was parsed (see Reply-all step 6), and `bcc: Vec<String>`.
     - Entries are RFC 5322 mailbox strings, so either `a@b` or `Name <a@b>`; lettre's `Mailbox: FromStr` already accepts both.
  2. **Sync** (`providers::message_from`): read `Cc` with the existing `addresses` closure. Read `Bcc` too; it only exists on Sent copies.
  3. **MIME** (`providers::build_mail`):
     - Add `.cc()` and `.bcc()` per address.
     - Require at least one recipient across to, cc and bcc.
     - Keep lettre's default Bcc stripping for the SMTP copy.
     - Any Sent APPEND (the P0 "Server-side mail state" item) builds a second message with `.keep_bcc()`.
  4. **Daemon and store:**
     - `draft()` caps to + cc + bcc at 100 combined (Gmail's SMTP limit).
     - `store.save_draft` copies the new fields (`store.rs:368-391`).
     - Document them in `docs/api.md`.
  5. **Desktop** (`Compose.tsx`):
     - Generalize `RecipientInput` with `id` and `label` props.
     - Add "Cc" and "Bcc" toggles on the To row, plus Mod+Shift+C / Mod+Shift+B in `shortcuts.ts`.
     - Show a field automatically when it is non-empty.
     - Dedupe an address across fields: adding it to one field removes it from the others.
     - Update `drafts.ts` to back up `cc` and `bcc`.
  6. **CLI, MCP, TUI:** add CLI `--cc` and `--bcc` (comma-delimited like `--to`), MCP `cc`/`bcc` array properties on `draft_email`, and Cc/Bcc lines in the TUI composer.
- **Pitfalls:**
  - **Bcc leakage is the critical bug class.** Never pass `keep_bcc()` for the SMTP copy. mailcore2 deliberately leaves Bcc in the data for Outlook servers (`MCSMTPSession.cpp:897`) and trusts Exchange to strip it. Do not copy that without a live test; a server that doesn't strip it would expose every Bcc recipient.
  - **Server Sent copies lose Bcc.** With SMTP plus server auto-save (Gmail, Exchange), the server's copy has no Bcc. When sync merges the server copy into the local delivery row (`crates/alice-core/src/sync_store.rs:509-530`), keep the local `bcc`.
  - **Display names with commas break the split.** A name like `"Doe, Jane" <j@x>` breaks the naive `,`/`;` split (`Compose.tsx:38`). Use a quote-aware splitter or lettre's `Mailboxes::from_str`, or switch to chips.
  - **Recipient limit:** 100 recipients over Gmail SMTP. Exchange Online allows more but has a 10,000 recipients/day SMTP AUTH limit.
- **Done when:**
  - `providers::tests::cc_and_bcc_reach_the_envelope_but_bcc_not_the_headers` passes. It builds a draft with to/cc/bcc and asserts that `formatted()` contains `Cc:` and no `Bcc:`, and that `envelope().to()` contains the Bcc address.
  - A `parse_message` test reads `Cc`.
  - A daemon test rejects 101 combined recipients.
  - The desktop smoke test (`scripts/smoke.mjs`) reveals Cc, saves it, reopens the draft, and finds the value.
- **Effort:** M (2–3 days). Prerequisite for Reply-all and Automatic Cc/Bcc.

<a id="reply-all-dedupe-own-addresses-honor-reply-to"></a>

### Reply-all (dedupe own addresses, honor Reply-To)

*Roadmap: P0 · Composer basics*

- **Why:** Replying in a group thread silently drops everyone except the sender. Mailspring users asked for a visible "Reply all" button (forum #5510), and Mailspring makes Reply All the default reply.
- **Competitors:**
  - **Mailspring's rule** (`Message.participantsForReplyAll`, `mailspring/src/flux/models/message.ts:341-371`):
    1. If Reply-To is set and its first address is not one of mine, To = Reply-To.
    2. Otherwise, if the message is from me, To = original To and Cc = original Cc.
    3. Otherwise To = From, Cc = original To + Cc.
    4. Cc then drops me (all accounts and aliases, via `AccountStore.accountForEmail`, `account-store.ts:302-314`) and the From addresses.
    5. Both lists are deduped by `toEquivalentEmailForm`, which lowercases and strips the `+tag` (`mailspring/src/flux/models/utils.ts:356-361`).
  - **Mailspring reply details:**
    - The original's Bcc is never used.
    - `participantsForReply` follows the same logic without Cc (`message.ts:375-393`).
    - `canReplyAll()` is true when there are more than 1 To or any Cc (`message.ts:307-310`).
    - Switching reply ↔ reply-all on an existing draft adds or removes only the reply-all extras and keeps manual additions (`draft-factory.ts:361-399`).
    - The From of a reply is the alias the original was addressed to (`draft-factory.ts:401-429`).
  - **Mailspring settings and shortcuts:**
    - Setting `core.sending.defaultReplyType`, default `reply-all`, labeled "Default reply behavior" (`mailspring/src/config-schema.ts:313-319`).
    - Shortcuts (shipped `keymaps/templates/Gmail.json`): `a`, Mod+Shift+R, Mod+Alt+Shift+R; Shift+A opens it in a new window.
  - **Thunderbird** (`nsMsgCompose.cpp`):
    - Reply goes to Mail-Reply-To, then Reply-To, then From. When Reply-To is a munged list address it replies to From.
    - Reply All uses Mail-Followup-To when present.
    - "Reply to List" uses List-Post (since TB 3.0).
    - Replying to your own message keeps the original To/Cc and switches identity.
    - It removes only the composing identity, not your other identities (Mailspring removes all of them).
  - **Mutt:** `$honor_followup_to` (default yes).
  - **Gmail:** Reply-To first, then everyone else in Cc. Default reply behavior is a setting.
- **Spec:**
  - RFC 5322 §3.6.2: Reply-To "indicates the address(es) to which the author of the message suggests that replies be sent".
  - RFC 2369 §3.4: List-Post.
  - Mail-Followup-To: https://cr.yp.to/proto/replyto.html and expired draft-ietf-drums-mail-followup-to-00 §2.4. Mutt and Thunderbird honor it; Gmail and Outlook ignore it.
- **Alice today:**
  - `Message::reply_recipients` returns Reply-To, falling back to From (`model.rs:119-125`). The CLI (`main.rs:1157`), TUI (`tui.rs:549`) and MCP (`mcp.rs:222`) use it.
  - The desktop re-implements it (`App.tsx:211`).
  - There is no reply-all anywhere, and no Cc is stored.
  - `replyTarget` skips messages in Sent (`App.tsx:90`), so replying to your own last message is not handled.
  - From is locked for replies (`Compose.tsx:90`, `disabled={… !!initial.in_reply_to}`).
- **Plan:**
  1. **Recipient logic:** add a pure function `fn reply_recipients(m: &Message, own: &OwnAddresses, all: bool) -> (Vec<String>, Vec<String>)` in `alice-core`. Implement Mailspring's table plus Thunderbird's Mail-Followup-To rule for `all`. Replace `Message::reply_recipients`.
  2. **Own addresses** = every account's email plus every identity (see "Send-as identities"), compared case-insensitively.
     - Strip `+tag` only when comparing against own addresses.
     - Fold dots only for own `gmail.com` and `googlemail.com` addresses.
     - Never normalize other people's addresses for dedupe beyond lowercasing the domain.
  3. **Draft factory endpoint:** `GET /v1/messages/{id}/reply?all=true|false` returns a `Compose` without persisting it. It contains recipients, `Re:` subject (skip if already prefixed, case-insensitive), `in_reply_to`, `references`, the quoted body (NEW item below), the signature, and the identity the parent was addressed to. The desktop (`App.tsx:207-213`, `:238-244`), CLI `reply_compose` (`main.rs:1154`), MCP (`mcp.rs:217-247`) and TUI (`tui.rs:534-571`) all call it, and their copies are deleted.
  4. **Desktop:**
     - Add a "Reply all" action to `threadActions` (`App.tsx:262`), the reader toolbar and the context menus.
     - Shortcuts: `a` (free in `shortcuts.ts`) and Mod+Shift+R. Disable it when `can_reply_all` is false; the daemon returns that flag on `Message`.
     - Add a "Default reply" preference (Reply / Reply all). The default should be Reply: Gmail's default, and the safer choice. Mailspring defaults to Reply all.
  5. **CLI and MCP:** `alice reply --all`; MCP `draft_email` gains `reply_all: bool`.
  6. **Old cached rows** have no `cc`. Make `Message.cc` an `Option<Vec<String>>` so "never parsed" (`None`) is distinct from "empty". On reply-all with `None`, fetch `BODY.PEEK[HEADER.FIELDS (CC REFERENCES MAIL-FOLLOWUP-TO)]` for that UID via `Remote` (`crates/alice-core/src/remote.rs:168`) and store the result.
- **Pitfalls:**
  - **List Reply-To munging:** lists that set Reply-To to the list make plain "Reply" go to the whole list. This is RFC-correct and Gmail does the same; Thunderbird detects it via List-Post and replies to From. Optional: show a "Replying to the list" hint when Reply-To equals List-Post.
  - **Reply-To pointing at yourself:** Mailspring falls back in that case (`message.ts:351`); do the same.
  - **Group syntax:** `undisclosed-recipients:;` yields no addresses. `mailparse` handles groups (`providers.rs:366-372`).
  - **From in To/Cc:** the sender also listed in To/Cc (people cc themselves) must not appear twice.
  - **Reply-all to your own sent message:** never add the original Bcc recipients; Mailspring ignores bcc. Combined with the to/cc split, this is Thunderbird's "reply to self".
  - **Recipient count:** large threads can exceed Gmail's 100-recipient SMTP limit. See the reply-all warning in the Missing-attachment item.
- **Done when:**
  - A table test `reply_recipients_cases` covers: plain sender; Reply-To list; Reply-To = me; from me (To/Cc reused, Bcc dropped); own alias in Cc removed; duplicates across To/Cc; `Me+x@Example.com` treated as own; group syntax; Mail-Followup-To with `all`.
  - A daemon test for `GET /v1/messages/{id}/reply?all=true` returns the expected to/cc, subject and references.
  - The smoke test presses `a` on a fixture thread and sees the Cc row populated.
- **Effort:** M (2–3 days). Depends on Cc and Bcc fields. Benefits from Send-as identities, which supply the own-address set.

<a id="forward-with-original-attachments-carried-over"></a>

### Forward, with original attachments carried over

*Roadmap: P0 · Composer basics*

- **Why:** Forwarding is one of the three core actions, and it is missing entirely. Without carried attachments, users forward invoices and documents by downloading and re-attaching them.
- **Competitors:**
  - **Mailspring** (`createDraftForForward`, `draft-factory.ts:231-273`):
    - Starts downloading every file (`Actions.fetchFile`).
    - The body contains "---------- Forwarded Message ---------" followed by From/Subject/Date/To/Cc lines (localized labels) and the original body.
    - Subject gets `Fwd:` (`Utils.subjectWithPrefix`). `files: message.files` carries all files.
    - `forwardedHeaderMessageId` sets References (not In-Reply-To) on send (`TaskProcessor.cpp:2154-2156`).
    - The draft stays in the same thread (1.0.6 fix).
    - The quoting helper deletes every `<img src="cid:…">` from the quoted HTML (`draft-factory.ts:45-51`, `// TODO: Fix inline images`). Forwarded inline images therefore arrive as plain attachments, not in place.
    - No forward-as-attachment. Since 1.24.0 you can drag a thread into the composer to attach it as `.eml`.
    - Keymap: `f`, Mod+Shift+F, and Shift+F for a new window (Gmail template).
  - **Other clients:**
    - Gmail: inline forward by default, plus "Forward as attachment" (`message/rfc822`).
    - Outlook: Forward and "Forward as attachment" (Ctrl+Alt+F).
    - Thunderbird: Message → Forward As → Inline / As Attachment, with a default in preferences.
    - Apple Mail: Message → Forward as Attachment.
- **Spec:**
  - RFC 2046 §5.2.1: `message/rfc822` may use only 7bit, 8bit or binary encoding, never base64.
  - RFC 2183: Content-Disposition.
  - RFC 5322 §3.6.4: a forward is not a reply, so no In-Reply-To. Putting the original's id in References keeps threading in Gmail and Mailspring.
- **Alice today:**
  - No forward action in any frontend.
  - Received attachments are listed in `Message.attachments` (`model.rs:73-87`) and fetched on demand from IMAP (`remote.rs:308-360`, `GET /v1/messages/{id}/attachments/{index}`, `daemon.rs:521-575`).
  - Drafts cannot hold attachments; `save_draft` hard-codes `attachments: vec![]` (`store.rs:384`).
- **Plan:**
  1. **Draft attachment model:** draft attachments get a source: `Upload { file_id }` from the Attachment upload item, or `Message { message_id, index }`. A forward stores references, not bytes.
  2. **Resolve at send:** in `send()`, before `claim_delivery` (`daemon.rs:494`), fetch the bytes with the existing `Remote::attachment` path. A failure then never consumes the at-most-once claim, and the error can say "the original attachment is no longer on the server; remove it or re-sync".
     - Upgrade path if offline sends matter: copy the bytes into the draft store in the background when the forward is first saved, as Mailspring does.
  3. **`GET /v1/messages/{id}/forward?as_attachment=false` returns a `Compose` with:**
     - a `Fwd:` subject, not doubled with existing `Fwd:`/`Fw:`/`WG:` prefixes (reuse the prefix list thread grouping uses);
     - a localized header block (From, Date, Subject, To, Cc) and the original text;
     - for HTML, the original HTML after the rich-text item lands;
     - `references` = the original References + Message-ID, and no In-Reply-To;
     - every attachment, with `content_id` ones kept as related parts once HTML compose exists.
  4. **Forward as attachment:** fetch `BODY.PEEK[]` and attach it as `message/rfc822` named `<subject>.eml`.
     - Build it with `Body::new_with_encoding(raw, ContentTransferEncoding::EightBit)`. lettre's `Body::new(Vec<u8>)` always picks base64, which RFC 2046 forbids.
     - If that returns `Err` (lines over 998 characters or NULs), fall back to `application/octet-stream` with the `.eml` name.
  5. **Desktop:**
     - Add a Forward action (shortcut `f`, free in `shortcuts.ts`) to the reader toolbar, thread context menu and reader menu, with "Forward as attachment" in the menu.
     - The composer shows carried attachments as removable chips.
  6. **CLI and MCP:** `alice forward <id> --to … [--as-attachment]`. MCP: add `forward_of` to `draft_email`, gated like `send_draft` under Full access.
- **Pitfalls:**
  - **Total size:** the carried files count toward the size budget (see Attachment upload). Warn before send, not at forward time.
  - **Stale mailbox:** a UIDVALIDITY change or moved message makes the source unreachable. `Remote::attachment` already detects this (`remote.rs:319-321`); map it to a clear error.
  - **Old cached rows** without section numbers fall back to a full-message fetch (`remote.rs:340-360`). That is slow but works.
  - **Invitations:** forwarding one must carry the `.ics` as a plain attachment, not trigger an RSVP.
  - **Signed or encrypted mail:** S/MIME and PGP messages lose their signature when forwarded inline; offer "as attachment".
  - **Remote images:** the quoted HTML of the original may include remote images and trackers. Sanitize it the same way the reader does (`apps/desktop/src/emailHtml.ts`). For the daemon, render text only until HTML compose lands.
- **Done when:**
  - A daemon test runs: forward the fixture message with two attachments → `Compose` has the `Fwd:` subject, references and two `Message` sources → send against a mock SMTP. The MIME parses with `mailparse`, and the attachment bytes' SHA-256 matches the source.
  - An `as_attachment` test finds a `message/rfc822` part with 7bit or 8bit encoding that `mailparse` can parse recursively.
  - A send with a vanished source fails before `claim_delivery`, and the draft stays editable.
- **Effort:** M–L (3–5 days). Depends on Attachment upload (the draft attachment model) and Correct MIME for replies (references).

<a id="attachment-upload-file-picker-drag-and-drop-paste-files"></a>

### Attachment upload: file picker, drag and drop, paste files

*Roadmap: P0 · Composer basics*

- **Why:** Users cannot send a file at all. This is the largest functional gap in the composer.
- **Competitors:**
  - **Mailspring** (`mailspring/src/flux/stores/attachment-store.ts`):
    - **Ways to attach:**
      - Attach button at the left of the bottom bar (moved in 1.6.2 to avoid hitting Delete Draft).
      - Mod+Shift+A, and a native multi-select dialog (`:418-433`).
      - Drag-drop onto the composer. Image drops become inline images (`mailspring/internal_packages/composer/lib/composer-view.tsx:318-415`).
      - Paste files from Finder or Explorer (1.0.9). 1.24.0 fixed the "Pasted File" name and 1.24.1 fixed pasting several files at once.
    - **Storage:** each file is copied into `<config>/files/<id>/` first (`_copyToInternalPath`). Adds are serialized per draft to keep their order (`:434-460`, 1.24.1).
    - **Limits:** 25 MB per file and in total, decimal and pre-encoding (`:485-505`, raised in 1.2.2). The total check runs on the existing files *before* adding the new one, so the total can exceed 25 MB. Directories are rejected.
    - **Send path:** mailsync reads the files from disk at send (`TaskProcessor.cpp:2200-2219`).
    - **Bugs:** 1.15.1 drag-drop path error; GitHub #822 (10 👍) "Mailspring sometimes don't attach files"; 1.21.0 removed the `?attach=` mailto parameter for security.
  - **Large-file features:** Gmail turns files over 25 MB into Drive links; Outlook uses OneDrive links; Thunderbird has Filelink; Apple Mail has Mail Drop.
- **Spec and provider limits:**
  - Base64 (RFC 2045 §6.8) adds 4/3, plus CRLF every 76 characters, about 37%.
  - Content-Disposition: RFC 2183. Non-ASCII filenames: RFC 2231. The SMTP SIZE extension: RFC 1870 §3 and §6. An oversize message gets `552`; do not retry it.
  - **Gmail:**
    - 25 MB send limit (https://support.google.com/mail/answer/6584). Google counts sizes after encoding (receiving limits: https://knowledge.workspace.google.com/admin/gmail/gmail-receiving-limits-in-google-workspace).
    - smtp.gmail.com advertises `SIZE 35882577`, about 34.2 MiB encoded, which fits 25 MB of raw files. This comes from an observed transcript, not Google documentation.
    - The Gmail API limit is 35 MiB.
  - **Exchange Online:** 35 MB send by default, configurable up to 150 MB, with a 33% encoding increase for external mail, at most 250 attachments, and 256 KB of headers (https://learn.microsoft.com/en-us/office365/servicedescriptions/exchange-online-service-description/exchange-online-limits).
  - **Graph** is not used by Alice for mail. For reference: under 3 MB in one POST; 3–150 MB through `createUploadSession` (https://learn.microsoft.com/en-us/graph/outlook-large-attachments).
  - **Outlook.com consumer:** 25 MB (https://support.microsoft.com/office/sending-limits-in-outlook-com-279ee200-594c-40f0-9ec8-bb6af7735c2e).
- **Alice today:**
  - Nothing in the composer. The README says "attachment upload are not implemented".
  - The global `DefaultBodyLimit::max(2_000_000)` applies to every request (`daemon.rs:302`); drafts are capped at a 1 MB body (`daemon.rs:477-479`).
  - Desktop IPC is JSON only (`daemon_request`).
  - Tauri has no dialog plugin and `dragDropEnabled` is at its default.
  - `lettre::message::Attachment` is available but unused.
- **Plan:**
  1. **Store.** Keep files in `client::data_dir()/drafts/<draft-id>/<file-id>`, created 0700/0600.
     - `Compose.attachments: Vec<DraftAttachment { id, name, size, content_type, content_id: Option<String>, source }>` is stored in the draft's message JSON.
     - `save_draft` must **preserve** the existing attachments, not reset them (`store.rs:384`).
     - Delete the directory in `discard_draft` and after `finish_delivery(true)`.
     - Sweep orphan directories at daemon start.
  2. **Routes:**
     - `POST /v1/messages/{id}/attachments` accepts a raw body with `Content-Type` and a percent-encoded `X-Alice-Filename` header, mirroring the existing download header (`daemon.rs:569`).
     - Give that route its own `DefaultBodyLimit` (e.g. 40 MB). Stream to disk while counting bytes. Reject when a `deliveries` row exists, as `save_draft` does (`store.rs:358-366`).
     - `DELETE /v1/messages/{id}/attachments/{file_id}` removes one file.
     - Return the updated `Message`.
  3. **Size budget per account:**
     - Gmail: 25 MB raw total.
     - Outlook / Office 365: 25 MB raw, the consumer and default-tenant-safe value; a tenant may allow more.
     - Generic IMAP: 25 MB raw, overridable later.
     - Compute the estimate as raw × 1.37 + body.
     - Show the remaining budget under the chips and refuse uploads past it with a localized message.
     - Map SMTP `552` / `5.3.4` in `deliver()` (`providers.rs:213-236`) to the same "too large" text.
     - Optional, for exact limits: `lettre::transport::smtp::client::SmtpConnection` can send a raw EHLO, and `Response::message()` can be parsed for `SIZE n`. Skip this until a user hits a non-standard server.
  4. **Desktop:**
     - A paperclip button and Mod+Shift+A open a hidden `<input type="file" multiple>`. WKWebView, WebView2 and WebKitGTK each show their native picker, so no `tauri-plugin-dialog` dependency is needed.
     - Drop zone over the composer. Set `"dragDropEnabled": false` on the window in `tauri.conf.json` so HTML5 `drop` delivers `File` objects (Tauri's drag-drop interception is on by default). Alternatively, listen to Tauri's `onDragDropEvent` and send paths to a Tauri command that reads them; that is two code paths, so prefer the first.
     - Paste: `clipboardData.files`.
     - All three paths call one `upload(file)` that sends `await file.arrayBuffer()` through a new Tauri command `daemon_upload`. Tauri v2's `invoke(cmd, Uint8Array, { headers })` passes raw bytes; the web build uses `fetch`.
     - Show chips with name, size, a progress spinner and a remove button.
  5. **MIME** (`build_mail`): wrap the body in `MultiPart::mixed()` when there are attachments, and add `Attachment::new(name).body(bytes, content_type)`. Use the file's `type` from the browser, falling back to `application/octet-stream`.
  6. **CLI:** `alice compose --attach PATH` (repeatable). The CLI reads the file and uploads it through the same route.
     - Do **not** add attach-by-path to MCP or the HTTP API (see Pitfalls).
- **Pitfalls:**
  - **Security: no attach-by-path API.** An "attach this path" endpoint would let any local API caller, including an MCP agent under prompt injection, mail out `~/.ssh/id_ed25519`. Only accept bytes from authenticated clients. The CLI reading a path the user typed is fine.
  - **Encoded size counts, not raw size.** Mailspring's 25 MB raw cap happens to fit Gmail's encoded SIZE. A 30 MB raw file passes a naive check and then fails at SMTP.
  - **Paste from Finder in WKWebView:** pasting a copied file may yield only an icon image or a generic name. Mailspring needed 1.24.0 for original filenames. Test on all three engines.
  - **Packages and folders:** `.app`, `.pages` and folders arrive as directories or zero-byte files. Reject them with "compress it first", as Mailspring does.
  - **Blocked file types:** Gmail rejects executables (`.exe`, `.js`, `.bat`, `.scr` and others, even inside archives) at SMTP with a `552-5.7.0` "blocked for security reasons" error. Warn when the file is attached.
  - **Non-ASCII filenames** are RFC 2231-encoded by lettre. Some older Outlook versions only read the RFC 2047 `name=` parameter on Content-Type; test with Outlook.
  - **The localStorage draft backup** (`drafts.ts`) cannot hold files. Back up metadata only, and show "missing" if the daemon lost the file.
  - **Memory:** lettre builds the whole message in memory. 35 MB is fine; don't raise the limit to 150 MB without streaming.
- **Done when:**
  - Daemon tests: upload two files (one named `Größe – 2026.pdf`), then GET the draft lists both; send against a mock SMTP produces `multipart/mixed` with byte-exact parts and an RFC 2231 filename.
  - Further daemon tests: an upload past the budget returns 413 with a localized message; discard removes the directory; an upload after `claim_delivery` is refused.
  - Desktop smoke: the picker, a synthetic drop and a paste each produce a chip.
- **Effort:** L (about 1 week). Blocks Forward (carried attachments) and Inline images.

<a id="correct-mime-for-replies-references-header-chain-not-only-in-reply-to"></a>

### Correct MIME for replies: `References` header chain, not only `In-Reply-To`

*Roadmap: P0 · Composer basics*

- **Why:** Clients and servers that thread by References alone file a reply without the chain as a new thread or attach it to the wrong one. This includes Gmail, Apple Mail, Thunderbird and the IMAP THREAD=REFERENCES algorithm. Alice's own planned threading item ("Sync correctness") needs the field stored too.
- **Competitors:**
  - **Mailspring gets this wrong too.** mailsync sets References to only the parent id (`TaskProcessor.cpp:2148-2152`, with the comment "todo: lookup thread reference entire chain?"). For forwards it sets only the forwarded id (`:2154-2156`).
  - 1.17.2 fixed In-Reply-To parsing on iCloud spam with garbage values.
  - Thunderbird, Apple Mail and Gmail send the full chain.
- **Spec:**
  - RFC 5322 §3.6.4: In-Reply-To = the parent's Message-ID.
  - References = the parent's References followed by the parent's Message-ID. If the parent has no References but its In-Reply-To holds a single id, use that id followed by the parent's Message-ID.
  - RFC 5256 §2.2 (IMAP THREAD REFERENCES) and JWZ threading consume this field.
- **Alice today:**
  - `build_mail` sets both In-Reply-To and References to `in_reply_to` (`providers.rs:207-209`); `rsvp_mail` does the same (`providers.rs:309-311`).
  - `Message` has no `references` field, and sync reads only `In-Reply-To` (`providers.rs:413`).
  - Message-IDs are `<uuid@alice.local>` (`store.rs:385`).
- **Plan:**
  1. Add `references: Vec<String>` (serde default) to `Message` and `Compose`.
  2. In `message_from`, parse `References` with a tolerant msg-id tokenizer: take each `<…>` token, ignore garbage, dedupe and keep the order.
  3. The reply factory (Reply-all item) fills `references = parent.references or [parent.in_reply_to if single] + [parent.message_id]`.
  4. Cap the chain at 20 ids, keeping the first (the thread root) and the most recent 19. Spec allows any length, but Exchange caps total headers at 256 KB and 32 KB per header, and some MTAs choke on very long ones.
  5. `build_mail` calls `.references(refs.join(" "))` **once**. lettre's `Headers::set` replaces the value on each call, and lettre folds long values. Apply the same to `rsvp_mail`.
  6. For cached rows without the field, fall back to `[in_reply_to] + [message_id]`. Re-parse happens naturally as rows re-sync; the on-demand header fetch from the Reply-all plan also fills it.
- **Pitfalls:**
  - Garbage In-Reply-To values (MS 1.17.2): never copy a value without angle brackets.
  - Id domain: `alice.local` is RFC-valid and keeps the hostname out of headers (Mailspring 1.1.5 removed the hostname from Message-IDs for privacy). Using the sender's domain (`<uuid@example.com>`) is what Thunderbird does and looks less unusual to spam filters. That is an optional one-line change in `save_draft`.
  - Drafts saved before the change have no references; compute them at send from `in_reply_to` if empty.
- **Done when:**
  - Unit test `references_chain`: a parent with `References: <a@x> <b@x>` and `Message-ID: <c@x>` produces `In-Reply-To: <c@x>` and `References: <a@x> <b@x> <c@x>`. A parent with only `In-Reply-To: <x@y>` produces `<x@y> <c@x>`. A 30-id chain is capped to 20 with the root kept. Every formatted line is ≤ 998 characters.
  - A `parse_message` test reads References.
- **Effort:** S (≤1 day).

<a id="per-account-signatures"></a>

### Per-account signatures

*Roadmap: P0 · Composer basics*

- **Why:** Work and personal accounts need different sign-offs. Today one global plain-text signature goes on every account.
- **Competitors:**
  - **Mailspring settings** (Preferences → Signatures, `mailspring/internal_packages/composer-signature/lib/`, data in `mailspring/src/flux/stores/signature-store.ts`):
    - Named signatures, each with a "Default for: N accounts" picker. Defaults are keyed by **email address**, so each alias can have its own.
    - Six layout templates plus Raw HTML, and structured fields: name, title, phone, fax, email, address, website, theme color, social links.
    - Pasting HTML into Raw was fixed in 1.24.0.
  - **Mailspring insertion** (`signature-composer-extension.ts`, `signature-utils.ts`):
    - Inserted when the draft is created, wrapped in `<signature id=…>`, one `<br>` above the quoted text.
    - A composer dropdown switches it (`signature-composer-dropdown.tsx`). Changing account swaps it (1.1.3).
  - **Mailspring bugs:**
    - 1.5.4 extra whitespace; 1.6.2 a signature `<style>` restyled the whole app; 1.7.6 switching mangled the draft.
    - 1.24.0 crash applying to an empty document; 1.24.1 signature images hosted on Mailspring's servers were overwritten in already-sent mail.
  - **Demand:**
    - #755 (11 👍) and forum #265 (40 votes): sync signatures across devices.
    - #676: separate defaults for new mail and replies.
    - #751: plain-text signatures.
    - #1426: change the signature within a message.
    - #769: image-only signatures.
  - **Other clients:**
    - Gmail: per send-as signature, separate new/reply defaults, and "insert before quoted text".
    - Thunderbird: per identity, plain/HTML or from file, optional `-- ` separator, and "place signature above quote".
    - Apple Mail: per account, plus "Place signature above quoted text".
- **Spec:** RFC 3676 §4.3: the `-- ` (dash dash space) separator line for plain text. Many clients strip everything after it when quoting.
- **Alice today:**
  - One global `Preferences.signature: String` (`model.rs:323`), edited in Settings → Writing (`apps/desktop/src/Settings.tsx:148`, "Added to new messages and replies").
  - `Preferences::draft_body` appends `\n\n-- \n<sig>` at the end of the body (`model.rs:362-372`). It is applied on the client in four places: desktop `App.tsx:212` (a TS re-implementation), CLI `main.rs:1512-1516`, MCP `mcp.rs:245`, TUI `tui.rs:566`.
  - The signature always goes at the very end, so once quoting exists (NEW item) it would land below the quote.
- **Plan:**
  1. **Model:**
     - Add `signatures: Vec<Signature { id, name, text, html: Option<String> }>` and `signature_defaults: BTreeMap<String /* identity email */, SignatureChoice { new: Option<id>, reply: Option<id> }>` to Preferences.
     - Migrate the existing `signature` into one "Signature" entry that is the default for every account (one-time, in `Store::open`, like the existing MCP migration in `store.rs:35-40`).
  2. **Insertion in the daemon draft factory** (new, reply and forward endpoints from the Reply-all item):
     - Insert the signature for the draft's From identity **above the quoted text**.
     - Plain text uses the `-- \n` separator. HTML uses `<div data-alice-signature="id">…</div>`.
     - Delete `draft_body` from all four frontends.
  3. **Switching:** changing From in the composer swaps the signature if the marked block is unchanged. Keep the marker in HTML and the `-- ` delimiter in text.
  4. **Settings:** a Signatures pane with a list, an editor, and "Default for new mail / replies" per account and identity. Start with plain text; add HTML once rich text lands. Allow pasting HTML and sanitize it with DOMPurify, which is already a dependency.
  5. **Sync across devices** (#755): import Gmail `sendAs.signature` (HTML) when importing identities (see "Send-as identities"). This gives real cross-device sync for Gmail users without a cloud service.
- **Pitfalls:**
  - **Placement:** above the quote is right for top-posting. With a plain-text `-- ` above the quote, Thunderbird and Mutt treat the quote as part of the signature when the other side replies, and strip it. That is acceptable.
  - **Reopened drafts:** don't re-insert the signature when a draft is reopened. Today's suffix check (`model.rs:366-370`) breaks once the user edits the signature text.
  - **Signature images:** images must be inline `cid:` parts or remote URLs. Remote images are blocked by default in many clients and are a tracking vector. Mailspring hosted them on its own servers (1.24.1 bug). Use `cid:` once Inline images lands.
  - **HTML hygiene:** sanitize HTML signatures and inline `<style>` (MS 1.6.2).
  - **Attachment check:** exclude the signature text from the missing-attachment check (MS 1.1.3).
  - **Agent drafts:** CLI and MCP drafts get signatures today; keep that behavior (MCP-written mail should still look like the user's).
- **Done when:**
  - Store tests: with two accounts, a new draft for B gets B's signature; a reply uses the reply default, placed above the "On … wrote:" block; the legacy signature migrates once.
  - CLI, MCP and TUI drafts get the signature from the daemon. A grep for `draft_body` finds no frontend callers.
  - A desktop smoke test switches From and sees the signature swap.
- **Effort:** M (2–4 days). Depends on the daemon draft factory (Reply-all item). Per-alias defaults depend on Send-as identities.

<a id="quote-the-original-message-in-replies-and-forwards"></a>

### Quote the original message in replies and forwards

*Roadmap: P0 · Composer basics · new*

- **Why:** Alice's replies start empty: `body = ''` on desktop (`App.tsx:207`, `:212`), the bare user text in CLI `reply_compose`, and `draft_body("")` in the TUI. Recipients get no context, and Alice's own quoted-text folding has nothing to fold. Every mainstream client quotes by default.
- **Competitors:**
  - **Mailspring:**
    - Writes the attribution "On Mon D YYYY, at h:mm a, Name <email> wrote:" (`message.ts:434-441`) followed by `<blockquote class="gmail_quote" style="margin:0 0 0 .8ex;border-left:1px #ccc solid;padding-left:1ex;">` (`draft-factory.ts:206-228`).
    - Plain text uses `> ` quoting (`mailspring/src/components/composer-editor/plaintext.ts:7-9`).
    - The quoted HTML is sanitized and style-inlined (`draft-factory.ts:35-55`).
    - 1.1.3 made draft creation 16× faster with heavy quoted text. Long quotes are a known lag source: #720 (12 👍).
  - **Gmail** collapses the quote behind "…". **Thunderbird** offers reply above or below the quote.
- **Spec:** none normative. RFC 3676 §4.5 covers `>` quote depth in flowed text.
- **Alice today:**
  - The reader already recognizes `gmail_quote`, `blockquote[type=cite]` and the "On … wrote:" / "Am … schrieb …:" attributions for folding (`apps/desktop/src/emailHtml.ts:5-6`).
  - Nothing generates them.
- **Plan:**
  1. In the daemon reply and forward factory, build a localized attribution matching the reader's regex. en: `On {date}, {name} <{email}> wrote:`; de: `Am {date} schrieb {name} <{email}>:`. Use `crate::t!` with the date formatted in the user's locale; chrono with `unstable-locales` is already enabled.
  2. For the text body, prefix the parent text with `> ` per line, increasing depth for lines already quoted.
  3. When HTML compose exists, add `<div class="gmail_quote"><div>{attribution}</div><blockquote class="gmail_quote" style="…">{sanitized parent HTML}</blockquote></div>`. Using Gmail's classes means other clients fold it too.
  4. The cursor goes above the quote, with the signature between the text and the quote.
- **Pitfalls:**
  - **Size:** the quoted HTML can be megabytes. The 1 MB body limit in `daemon.rs:478` then needs raising for HTML. Consider leaving the quote out of the editor and appending it at send (Gmail-style "…"), which also addresses the P2 "very long quoted text" item.
  - **Image privacy:** strip `cid:` images from quoted HTML the way Mailspring does, or carry them as related parts. Never turn them into remote fetches.
- **Done when:**
  - A test shows that replying to the fixture gives a body with `> ` lines and the localized attribution, and the reader's `quoteStart` finds the quote in the generated HTML (`scripts/test-html.mjs`).
- **Effort:** S for text, M with HTML. Depends on the Reply-all draft factory.

<a id="from-header-carries-the-accounts-display-name"></a>

### From header carries the account's display name

*Roadmap: P0 · Composer basics · new*

- **Why:** `build_mail` sends `From: alex@example.com` with no name (`providers.rs:195`), and `rsvp_mail` does the same (`providers.rs:304`). Recipients see a bare address, which looks like spam and sorts badly in their inbox. Mailspring's "Sender Identity" name (1.5.2) is the minimum switchers expect.
- **Competitors:** Every client sends `Name <address>`. Mailspring builds it with `MailUtils::addressFromContactJSON` (`TaskProcessor.cpp:2182-2183`).
- **Spec:** RFC 5322 §3.4 (`name-addr`) and RFC 2047 for non-ASCII names. lettre encodes these.
- **Alice today:** `Account.name` exists (`model.rs:14`) but `build_mail` ignores it.
- **Plan:** `.from(Mailbox::new((!account.name.trim().is_empty()).then(|| account.name.clone()), account.email.parse()?))` in both builders. The identity item later replaces `account.name` with the identity's name.
- **Pitfalls:** names containing quotes, commas or non-ASCII characters (lettre quotes and encodes them); an empty name must not produce `"" <addr>`.
- **Done when:** a test checks that `formatted()` has `From: =?utf-8?…?= <…>` for a name with ü, and a plain `From: Alex Doe <…>` for an ASCII name.
- **Effort:** S (an hour).

## P1 → Composer

<a id="rich-text-compose-send-html-text-replacing-the-textarea-with-the-tiptap-editor-s"></a>

### Rich text compose: send HTML + text, replacing the textarea with the Tiptap editor (see Notion-style editor below)

*Roadmap: P1 · Composer*

- **Why:** Most business mail is HTML. Plain-text-only output cannot keep links, lists or emphasis, and replying inside HTML threads breaks their formatting. Demand includes Markdown (#19, #1091, #891, forum #322) and default composing font (#111, #1088, forum #711).
- **Competitors:**
  - **Mailspring editor:**
    - A Slate editor (`mailspring/src/components/composer-editor/`) on a forked Slate 0.45 (`mailspring/package.json:58-66`).
    - Toolbar: bold, italic, underline, font, size, color, lists, quote, code, links and RTL (1.1.0).
    - Gmail shortcuts since 1.1.1: Mod+B/I/U, Mod+K link, Mod+Shift+7/8 lists, Mod+Shift+9 quote (shipped `mailspring-app/keymaps/base.json:49-55`).
    - Markdown auto-replace (`markdown-plugins.tsx`).
    - Setting `core.composing.html`, "Enable rich text and advanced editor features" (`config-schema.ts:189-197`).
  - **Mailspring send path:** HTML goes to mailsync, and mailcore2 adds a text/plain alternative generated with `flattenHTML()` (`MCMessageBuilder.cpp:731-748`).
  - **Pain history:**
    - IME/composition fixes: 1.2.0, 1.6.3 (`patch-chrome-ime.ts`), 1.13.2 dead keys, 1.13.3 macOS press-and-hold.
    - Crashes: 1.19.0 select-all and delete; 1.20.0 null range; 1.23.0 grammar decorations mid-composition and cut; 1.24.0 unset selection point, unpaired surrogate, link mark, signature on empty document.
    - 1.17.2 input lag where Enter and Backspace needed several presses.
    - 1.5.4 sent blank or incomplete drafts with long replies. Related demand: #948 "sending empty e-mail instead of what I wrote" and #851 "half of a message got deleted".
    - 1.2.2 RTL justification for recipients.
    - The patch file's own comment says moving past Slate 0.50 "is effectively a rewrite".
  - **Other clients:**
    - Gmail and Outlook web use their own contenteditable editors.
    - Apple Mail uses WebKit editing.
    - Thunderbird uses Gecko's editor.
- **Spec:**
  - RFC 2046 §5.1.4: in multipart/alternative, parts go in increasing faithfulness, so text/plain first and text/html last.
  - RFC 2045: Content-Transfer-Encoding. lettre picks quoted-printable or base64.
- **Editor options** (measured by the research subagent: esbuild, minified, gzip -9, React external, versions as of 2026-10-13):

  | Option | min+gz | IME / CJK track record | HTML in/out | Notes |
  |---|---|---|---|---|
  | TipTap 3.31 (`@tiptap/react` + `starter-kit`) | ~126 KB | Inherits ProseMirror; bugs mostly in extensions (e.g. #7271) | `getHTML()` / `setContent()`, schema drops unknown markup | Fastest to build; StarterKit includes Markdown input rules |
  | ProseMirror (view/state/model/transform + list, commands, keymap, history) | ~67 KB | Composition handling built into prosemirror-view, steadily fixed (1.33.3, 1.41.4) | `DOMParser.fromSchema` / `DOMSerializer` | Most control; more code to write |
  | Lexical 0.52 (+ rich-text, list, link, html) | ~89 KB | Designed for composition; few open issues | `@lexical/html`; per-node import/export | Weaker legacy-HTML import; React-centric |
  | Slate 0.133 + slate-react | ~61 KB | Weakest: 9 open IME issues, including crashes | Write your own | Mailspring's experience is a warning |
  | Raw contenteditable + `execCommand` | 0 KB | Native per engine | Engine-specific HTML | Deprecated API; HTML differs between WebKit and WebView2 |

- **Recommendation:** TipTap 3, imported with `import()` only when the composer opens, so the inbox's initial bundle doesn't grow.
  - StarterKit gives bold, italic, strike, code, lists, blockquote, history, hard break and Markdown input rules.
  - Add the Link, Underline, TextStyle + Color, FontFamily, TextAlign and Image extensions.
  - Add about 20 lines of a global-attributes extension that sets `dir="auto"` on paragraphs and headings for RTL.
  - If the bundle matters more than build time, the same design works on raw ProseMirror at about half the size.
- **Alice today:**
  - Plain `<textarea>` (`Compose.tsx:93`). `build_mail` sends `text/plain` (`providers.rs:196`). `Compose` has no HTML field.
  - The desktop dependencies are minimal (React, motion, lucide, DOMPurify; `package.json`).
  - The reader already sanitizes with DOMPurify (`emailHtml.ts`).
- **Plan:**
  1. **Model:**
     - Add `Compose.html_body: Option<String>`; `None` means a plain-text draft.
     - `body` stays the text version for the CLI, TUI, AI, search and MCP. **The daemon regenerates `body` from `html_body` with `html2text`** (already a dependency) when saving, so the two never diverge and clients can't send mismatched alternatives.
     - Raise the draft limit for `html_body` to about 5 MB.
  2. **MIME** (`build_mail`): when `html_body` is set, build `MultiPart::alternative().singlepart(plain).multipart(related_or_html)`. Wrap it in `mixed` when there are attachments. Wrap the HTML in a minimal document with `<meta charset>` and a default font style from the user's preference.
  3. **Desktop:**
     - Add a new `Editor.tsx` (lazy-loaded) with a compact toolbar: B, I, U, link, bulleted and numbered lists, quote, text color, font (system sans / serif / mono), and clear formatting (#533/#603 "No formatting").
     - Shortcuts follow Mailspring/Gmail: Mod+B/I/U, Mod+K link, Mod+Shift+7/8, Mod+Shift+9.
     - When the composer is focused it owns Mod+K. App-level shortcuts are already skipped while composing (`App.tsx:280`).
  4. **Email-safe output:**
     - Serialize with inline styles only, because clients strip `<style>`.
     - Lists as `<ul>/<ol>`, quotes as `<blockquote style="margin:0 0 0 .8ex;border-left:1px solid #ccc;padding-left:1ex">`, and `dir="auto"` on blocks.
     - No CSS variables, flex or grid; the Outlook desktop Word engine ignores them.
  5. **Quoted history:**
     - Keep it out of the editable document: a "…" toggle that inserts it read-only, or append it at send. That avoids the long-quote lag (#720, MS 1.1.3).
     - The daemon appends the quote block from the NEW quoting item.
  6. **Preference:** "Compose format: Rich text / Plain text", with Alt/Option-click for a one-off (see the Plain-text item). Also a default font and size (#111/#1088).
- **Pitfalls:**
  - **IME:** test Japanese, Chinese (Pinyin) and Korean input on all three engines, along with macOS press-and-hold accents (MS 1.13.3) and dead keys (MS 1.13.2).
  - Don't toggle `spellcheck` while typing. Mailspring did that for performance, and it swallowed Enter and Backspace (`composer-editor.tsx:186-215`, MS 1.17.2).
  - **Data loss** (MS 1.5.4, #948, #851): autosave must serialize the editor's current document, never a stale prop. Keep the existing 400 ms debounce plus localStorage backup (`Compose.tsx:54-61`), and back up `html_body` too.
  - **Dark mode:** don't store theme colors as explicit text colors (MS 1.6.2). Default text color must be "unset", not `#fff`.
  - **XSS:** a script in the composer webview can call Tauri `daemon_request` and use the whole API. All HTML that enters the editor (paste, quoted original, signature) is schema-filtered and passed through DOMPurify. See the Paste item.
  - **Size:** `html2text` output for large HTML. Cap the work, or run it on `spawn_blocking`.
- **Done when:**
  - A Rust test: a draft with `html_body` sends `multipart/alternative` with text/plain first, then text/html. The text part contains link URLs.
  - A jsdom test (`scripts/test-html.mjs` pattern) checks the editor serializer: lists, link, blockquote, and `dir="auto"` on a Hebrew paragraph.
  - A manual IME checklist is recorded in `docs/`.
  - Smoke: bold text survives save, reopen and send.
- **Effort:** L–XL (about 2 weeks). Depends on the data model. Inline images, Paste and Match Style and Plain-text mode build on it.

<a id="inline-images-paste-drag-resize"></a>

### Inline images: paste, drag, resize

*Roadmap: P1 · Composer*

- **Why:** Screenshots pasted into mail are routine; today they cannot be sent. Demand: forum #971 drag-drop inline, #729 resizing.
- **Competitors:**
  - **Mailspring flow:**
    - Pasted or dropped images become draft files with a generated `contentId` (`attachment-store.ts:497`). The editor inserts an inline-attachment node (`inline-attachment-plugins.tsx`) whose HTML references `cid:`.
    - At send, inline files no longer referenced in the body are dropped (`mailspring/src/flux/stores/draft-store.ts:578-583`).
    - mailsync attaches them with `addRelatedAttachment` (`TaskProcessor.cpp:2211-2216`), and mailcore2 nests them as `multipart/related` with the HTML (`MCMessageBuilder.cpp:719-723`).
  - **Mailspring changelog:**
    - 1.0.9 fixed sending inline images on all providers. 1.3.0 made dropping reliable.
    - 1.6.2 brought drag/copy/paste of inline images. 1.7.0 shows non-inline images as files. 1.10.8 added drag-to-resize.
    - 1.24.1 recognizes JPEG, GIF, BMP and WebP from the clipboard.
    - 1.10.6 security fix: `cid:` is allowed only in `<img src>`.
    - 1.23.0 fixed a crash on quoted emails with inline images.
    - Quoted inline images are stripped from replies (`draft-factory.ts:45-51`).
  - **Apple Mail** has an image-size menu (Small / Medium / Large / Actual Size) that recompresses.
- **Spec:**
  - RFC 2387 (`multipart/related`, `type=` parameter).
  - RFC 2392 (`cid:` URLs match `Content-ID` without the angle brackets).
  - RFC 2183 (`inline` disposition).
- **Alice today:**
  - The reader displays `cid:` images (README parity list; `Attachment.content_id`, `model.rs:84-86`).
  - The composer has nothing.
  - CSP: `img-src 'self' data: https:`, with no `blob:` (`tauri.conf.json`).
- **Plan:**
  1. Reuse the Attachment upload route with an `inline=true` flag. The daemon assigns `content_id = <uuid>@alice` and returns it.
  2. **Editor:**
     - The TipTap Image node gets a `data-cid` attribute.
     - While editing, `src` is a `data:` URL, which the CSP allows. Alternatively add `blob:` to `img-src` and use `URL.createObjectURL`, which is lighter on memory.
     - The serializer writes `src="cid:…"`.
  3. **Resize:**
     - A corner handle or a context menu with Small (320 px) / Medium (640 px) / Original.
     - Store both the `width` **attribute** and `style="max-width:100%"`. Outlook desktop honors the attribute but not CSS width.
  4. **Paste and drop:**
     - Images become inline; other files become attachments (Mailspring rule).
     - Convert TIFF, HEIC and BMP clipboard data to PNG (or JPEG for photos) via canvas before upload.
     - Offer downscaling when an image is over about 2 MB or 2000 px wide.
  5. **MIME:** `MultiPart::related().singlepart(html).singlepart(Attachment::new_inline_with_name(cid, name).body(bytes, ct))` inside the alternative. Before sending, drop inline files whose `cid:` is absent from `html_body`.
- **Pitfalls:**
  - **Formats:** macOS clipboard images from Photos or Preview are often TIFF or HEIC, which most recipients can't display. WebP may not render in classic Outlook for Windows (verify).
  - **Content-ID formatting:** lettre adds the angle brackets to Content-ID; the `src` must not have them.
  - **Recipients:** some clients (Apple Mail on iOS) also list inline images as attachments, which is normal.
  - **Size budget:** inline images count toward it. Retina screenshots are 5–15 MB as PNG.
  - **Replies:** for quoted inline images, either re-attach the originals as related parts (Gmail behavior) or drop them with a placeholder. Never leave a dangling `cid:`.
- **Done when:**
  - Daemon test: an inline upload returns a `content_id`. The sent MIME is `alternative{ text, related{ html with src="cid:X", image/png with Content-ID <X> and Content-Disposition inline } }`. `parse_message` round-trips `content_id`. An unreferenced inline file is not sent.
  - Desktop smoke: pasting a PNG shows it in the editor.
- **Effort:** M (3–4 days). Depends on Attachment upload and Rich text.

<a id="plain-text-mode-per-draft-or-globally"></a>

### Plain-text mode per draft or globally

*Roadmap: P1 · Composer*

- **Why:** Mailing lists, kernel and patch workflows, and plain-text purists need text-only mail. This was Mailspring's most-voted composer issue: GitHub #52 with 101 👍.
- **Competitors:**
  - **Mailspring:**
    - Setting `core.composing.html` (default on). Holding Alt/Option while clicking Compose or Reply creates a one-off plain draft (`draft-factory.ts:30-33`, `config-schema.ts:189-197`).
    - The plain editor is a `<textarea>` (`composer-editor-plaintext.tsx`) with hard wrap at 72. Soft breaks are marked with zero-width spaces (`plaintext.ts:1-60`) and stripped at send (`mailspring/src/flux/tasks/send-draft-task.ts:61-65`).
    - Replies quote with `>` (`deepenPlaintextQuote`).
    - Bugs: 1.7.5 got stuck creating every draft in plaintext; 1.7.5 lost leading spaces; 1.7.7 lost CRLF on paste; 1.7.7 plaintext quotes were converted to HTML in rich replies.
    - Demand: #751 plain-text signatures, #843 plain-text font.
  - **Thunderbird:** per identity "Compose messages in HTML format", Shift-click on Write for the opposite format, and Options → Send Format (Auto / Plain / HTML / Both).
  - **Apple Mail:** Format → Make Plain Text (Shift+Cmd+T) per message.
- **Spec:** RFC 2046 §4.1.3 (text/plain). Format=flowed is covered in the NEW P2 item below.
- **Alice today:** Alice is plain-text only (`Compose.tsx:93`, `providers.rs:196`). This item becomes real work only once Rich text lands.
- **Plan:**
  1. Add a preference `compose_format: rich | plain` (default rich). A draft is plain when `html_body` is `None`.
  2. Alt/Option-click on Compose, Reply or Forward inverts the format (`event.altKey`), as in Mailspring. Add a "Switch to plain text / rich text" item in the composer menu.
     - Rich → plain converts with `html2text`; warn that formatting and inline images will be lost.
     - Plain → rich wraps paragraphs in `<p>`.
  3. Keep the existing `<textarea>` as the plain editor with a monospace option (#843). Do not hard-wrap here; see the format=flowed item.
  4. In plain mode, replies quote the parent's text body (or `html2text` of HTML-only parents) with `> `, and signatures use the text variant.
- **Pitfalls:**
  - Don't get stuck in the wrong mode (MS 1.7.5): the per-draft flag must not overwrite the global preference.
  - CRLF in pasted text (MS 1.7.7, 1.26.0).
  - RTL: use `dir="auto"` on the textarea.
- **Done when:**
  - With the preference set to plain, a new draft sends a single `text/plain` part.
  - Alt-click inverts the format for one draft only.
  - A rich → plain toggle test keeps the text and link URLs.
  - Plain replies contain `> ` quotes.
- **Effort:** S–M (1–2 days) after Rich text.
- **Note:** this is essentially a mode of the Rich text item; consider merging the two.

<a id="send-as-identities-and-aliases-including-gmail-send-mail-as-import"></a>

### Send-as identities and aliases, including Gmail "Send mail as" import

*Roadmap: P1 · Composer*

- **Why:** Many people send from aliases (role addresses, custom domains routed through Gmail). Replies must come from the address the mail was sent to.
  - Demand: GitHub #581 (10 👍) and forum #294 (24 votes) want to reply or forward from a different account; forum #655 too.
  - #1133 (7 👍) wants a different SMTP server per alias.
  - Forum #760 wants Gmail aliases and signatures retrieved automatically.
  - Forum #768 wants a Reply-To header in the composer.
- **Competitors:**
  - **Mailspring:**
    - Preferences → Accounts per account: "Sender Identity" name (1.5.2) and a manual Aliases list of `Name <email>` strings with a default alias (`mailspring/src/flux/models/account.ts:63-72`, `:161-190`; editor in `preferences-account-details.tsx:85-140`).
    - The From dropdown in the composer lists every account and alias (`account-contact-field.tsx`). Replies auto-pick the alias the mail was addressed to (`draft-factory.ts:401-429`).
    - Send checks that From belongs to an account or alias (`send-draft-task.ts:118-133`). 1.26.0 explains the failure instead of doing nothing.
    - Moving a draft between accounts (`ensureCorrectAccount`) lost tracking metadata until 1.24.1.
    - Reply-To field since 1.21.0.
    - No Gmail import and no per-alias SMTP.
  - **Mimestream (MM):** imports Gmail send-as addresses including names, reply-to and signatures.
  - **Thunderbird:** multiple identities per account, each with name, email, reply-to, signature, auto Cc/Bcc and **its own outgoing SMTP server**. The composer also has "Customize From address".
  - **Gmail web:** "When replying to a message: reply from the same address the message was sent to".
- **Spec:**
  - RFC 5322 §3.6.2 (From, Sender, Reply-To).
  - Gmail API `users.settings.sendAs.list` (https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.settings.sendAs/list). Fields: `sendAsEmail`, `displayName`, `replyToAddress`, `signature` (HTML), `isPrimary`, `isDefault`, `treatAsAlias`, `smtpMsa`, `verificationStatus`. Any of `gmail.readonly`, `gmail.settings.basic`, `gmail.modify` or `https://mail.google.com/` works.
  - Exchange: alias sending requires `Set-OrganizationConfig -SendFromAliasEnabled $true`. Without it, a non-primary From is rewritten or rejected (`5.7.60`). Its general availability was announced in October 2026 (https://techcommunity.microsoft.com/blog/exchange/sending-from-email-aliases-%E2%80%93-general-availability/4562543); verify behavior over SMTP AUTH live. Graph `GET /me?$select=proxyAddresses` lists aliases.
- **Alice today:**
  - `Account { name, email }` only (`model.rs:12-29`).
  - From is a bare `account.email` (`providers.rs:195`); see the NEW display-name item.
  - The desktop From `<select>` lists account emails and is disabled for stored drafts and replies (`Compose.tsx:90`).
  - `save_draft` refuses to move a draft to another account (`store.rs:354`).
  - The Google OAuth scope is `https://mail.google.com/` (`crates/alice-core/src/oauth.rs:27`), which already allows `sendAs.list`.
- **Plan:**
  1. **Model:**
     - `Identity { email, name, reply_to: Option<String>, signature_id: Option<String>, auto_cc, auto_bcc, smtp: Option<SmtpOverride>, source: Manual | Gmail }`.
     - `Account.identities: Vec<Identity>` (serde default). The account's own address is the implicit default identity.
     - `Compose.from: Option<String>`, the identity email.
     - The daemon validates `from` against the account's identities and refuses otherwise with "The From address is not one of your accounts or aliases" (MS 1.26.0 wording).
  2. **MIME:** `build_mail` uses `Mailbox::new(Some(identity.name), identity.email)` and adds `Reply-To` from `identity.reply_to` or the draft's own `reply_to` field. Add an optional Reply-To row in the composer (#768, MS 1.21.0).
  3. **Gmail import:**
     - `GET https://gmail.googleapis.com/gmail/v1/users/me/settings/sendAs` with the account's existing token. Keep `verificationStatus == "accepted"` or the primary address.
     - Map `displayName`, `replyToAddress` and `signature` (into the Signatures store).
     - Refresh on account connect and daily.
     - The Gmail API must be enabled on Alice's Google Cloud project; this is a one-time console change.
  4. **Per-identity SMTP** (#1133): `SmtpOverride { host, port, username }` with the password in the keyring, the same way `vault` stores account secrets. `deliver()` (`providers.rs:213-236`) uses the identity's transport when it is set.
     - Gmail's `smtpMsa` is informational only: Gmail relays through it server-side when you send via smtp.gmail.com, so Alice need not.
  5. **Reply or forward from a different account** (forum #294):
     - Let the From select list every account's identities, including for replies.
     - Changing From to another account **moves the draft**. Allow `save_draft` to change `account_id` while no delivery exists: drop the `m.account_id != account.id` refusal for drafts. Keep `in_reply_to` and `references`.
     - Optional default rule: "Reply from the address the message was sent to".
  6. **Reply default:** the reply factory picks the identity whose email appears in the parent's To, Cc, `Delivered-To` or `X-Original-To`, else the account default.
  7. **Settings:** under Accounts, an Identities list per account (add / edit / remove; "Import from Gmail" button) with name, address, reply-to, signature, auto Cc/Bcc and an advanced SMTP section.
- **Pitfalls:**
  - **Gmail From rewriting:** Gmail SMTP silently rewrites From to the primary address when the alias is not a verified send-as (observed behavior, not documented by Google). Validate against the imported list rather than trusting the user's text.
  - **Exchange and Outlook.com:** without `SendFromAliasEnabled`, expect `5.7.60` or a rewrite. Outlook.com's native aliases appear in its own From dropdown, but SMTP behavior is undocumented, so test live.
  - **Generic servers:** many reject a From they do not own, or DMARC alignment fails at the recipient. Show the server's SMTP error verbatim.
  - **Secrets:** per-identity SMTP passwords must never land in the store JSON (follow the account secret pattern).
  - **Moving drafts** between accounts: the localStorage backup key is the draft id, which stays valid.
  - **Reply-all:** the own-address set for reply-all must include every identity (Reply-all item).
- **Done when:**
  - Tests: `build_mail` with an alias identity has `From: Name <alias>` and `Reply-To`. A draft with a From that isn't one of the account's identities is rejected. A `sendAs.list` JSON fixture is parsed into identities, pending ones skipped. A reply to mail addressed to an alias defaults `from` to the alias.
  - A draft moved to another account sends through that account's SMTP. An identity with an SMTP override uses it (mock server).
- **Effort:** L (1–2 weeks: Gmail import M, per-identity SMTP M). Depends on the NEW From display name item. Feeds Reply-all, Signatures and Automatic Cc/Bcc.

<a id="undo-send-with-a-configurable-delay"></a>

### Undo send with a configurable delay

*Roadmap: P1 · Composer*

- **Why:** Typos and wrong recipients are noticed in the first seconds after Send. Every modern client offers a short undo window.
- **Competitors:**
  - **Mailspring:**
    - Setting "After sending, enable undo for" 5 / 15 / 30 / 60 s / Disable, default 5 s (`config-schema.ts:320-332`).
    - Implemented as send-later metadata `{expiration, isUndoSend}` on the draft (`draft-store.ts:505-640`). The send-later plugin sends when the metadata expires (`mailspring/internal_packages/send-later/lib/main.ts:26-61`), so it survives a restart.
    - The toast has a countdown (1.4.0) and "Send now" (crash fixed in 1.22.0).
    - Bugs: worked first in 1.0.8; 1.0.9 closed other composer windows; 1.4.0 fixed stuck emails, a locked inline composer and re-sending the original after undo.
  - **Other clients:**
    - **Gmail** (web/app only): 5 / 10 / 20 / 30 s. SMTP and the API send immediately, so a third-party client must hold mail itself.
    - **Outlook.com:** up to 30 s.
    - **Apple Mail** (macOS 13+): Off / 10 / 20 / 30 s, default 10 s.
    - **Superhuman:** fixed 10 s.
    - **Spark:** default 5 s, mail held in its Outbox.
    - (Sources: support.google.com/mail/answer/2819488, support.apple.com/guide/mail/mlhlbf24d020/mac, sparkmailapp.com/help/sending-emails/customize-undo-send-timer.)
- **Spec:**
  - None. Server-side delay exists only as Exchange deferred delivery and the rarely deployed SMTP FUTURERELEASE (RFC 4865).
  - A local delay is the only portable approach.
- **Alice today:**
  - `POST /v1/messages/{id}/send` is synchronous (`daemon.rs:482-506`). It runs `claim_delivery` (status `pending`) → SMTP → `finish_delivery` (`sent` / `unknown`) (`store.rs:399-412`, `:447-466`).
  - The desktop awaits it on Send and closes (`Compose.tsx:73-81`).
  - `save_draft` already refuses edits when a `deliveries` row exists (`store.rs:358-366`).
- **Plan:**
  1. **Preference:** `undo_send_seconds: u8` with choices Off / 5 / 10 / 20 / 30 / 60, default 10 (Apple's default; the cost is just a short wait).
  2. **Schedule on send:**
     - `POST /send` accepts `{ "delay_ms": n }`; when the field is absent, the preference applies to every client.
     - With a delay, insert `deliveries(id, status='scheduled', send_at)`. Add a column through the existing migration path.
     - Return `202` with the `Message` plus `send_at`.
     - Validate and build the MIME, and resolve forwarded attachments, **before** scheduling, so errors show while the composer is still open.
  3. **Outbox task:**
     - A daemon task, `tokio::spawn` next to `run_background` (`daemon.rs:1104`), wakes at the earliest `send_at`, or on a 1 s tick for simplicity.
     - It moves the row with `UPDATE deliveries SET status='pending' WHERE id=? AND status='scheduled'`, then calls the current deliver path.
     - At startup, scheduled rows past `send_at` are sent immediately: the user confirmed them. Notify the user so it isn't surprising.
  4. **Cancel:** `DELETE /v1/messages/{id}/send` runs `DELETE FROM deliveries WHERE id=? AND status='scheduled'`. Zero rows means it's too late: return 409.
  5. **Desktop:**
     - A toast reading "Sending… Undo · Send now" with a countdown, matching Mailspring and Spark.
     - Undo reopens the composer with the draft. Send now updates `send_at` to now.
     - The Drafts list shows scheduled drafts as "Sending in 8 s".
  6. **CLI and MCP:**
     - The CLI defaults to `delay_ms: 0` (scripts expect immediate sends) with `--undo-window`.
     - **MCP `send_draft` should use the preference.** An undo window on agent-initiated sends is a useful safety net. Report "scheduled" back to the agent.
  7. **Send later:** the P2 "Send later" item reuses the same `scheduled` state with an absolute `send_at`.
- **Pitfalls:**
  - **Clocks:** use wall-clock `send_at`, not monotonic sleeps, so the window survives sleep and wake. On wake, overdue scheduled mail sends immediately.
  - **Restarts:** the daemon can be replaced by a newer build (`client.rs:100-122`); scheduled rows must be persisted, not kept in memory.
  - **At-most-once:** the scheduled → pending transition must be the single atomic gate. A cancel racing the outbox gets either 409 or success, never a double send. MS 1.4.0 re-sent the original after an undo.
  - **Warnings** (missing attachment, large reply-all) must run before scheduling, not after the window.
  - **Multiple frontends:** a TUI cancel and a desktop toast must agree. The desktop polls the snapshot `revision` today, so the toast should handle 409 gracefully.
- **Done when:**
  - Store and daemon tests:
    - Scheduling with a 200 ms delay and cancelling at 50 ms makes no SMTP call, and the draft is editable again.
    - Scheduling then waiting delivers exactly once.
    - A cancel after the transition returns 409.
    - A scheduled row survives `Store::open` and a restart sends it.
    - `send_at` in the past at startup sends immediately.
  - Desktop smoke: Send shows the toast; Undo reopens the draft.
- **Effort:** M (3–4 days). Independent of the other items. Unblocks P2 "Send later".
- **Note:** the tag `MS 1.4` should be `MS 1.0.8`: undo send "works correctly" from 1.0.8, and 1.4.0 added the countdown.

<a id="spellcheck-language-choice-and-right-click-suggestions"></a>

### Spellcheck language choice and right-click suggestions

*Roadmap: P1 · Composer*

- **Why:** Misspellings in outgoing mail are embarrassing, and multilingual users need to pick the language. Demand: forum #5555, "spellcheck language switcher".
- **Competitors:**
  - **Mailspring:**
    - Settings `core.composing.spellcheck` (default on) and `spellcheckDefaultLanguage` (1.0.9), under Preferences → General → Composing.
    - Uses Electron's spellchecker. macOS auto-detects the language (1.10.0); Windows needs language packs (1.15.1, fixed in 1.17.0).
    - Right-click suggestions since 1.2.1.
    - It turns spellcheck off while typing and back on after 800 ms (`composer-editor.tsx:186-215`). That caused swallowed keystrokes, worked around with requestAnimationFrame.
    - Grammar check via a hosted LanguageTool (1.18.0, off by default; snippets go to Mailspring's server).
  - **Other clients:**
    - Apple Mail follows the system spelling settings (Edit → Spelling and Grammar, "Automatic by Language").
    - Thunderbird has a per-draft language menu.
- **Spec:** none.
- **Alice today:**
  - Nothing is configured. The body `<textarea>` has the browser default (spellcheck on). The recipient field disables it (`Compose.tsx:149`).
  - The global contextmenu blocker keeps the system menu in inputs and `[contenteditable]` (`apps/desktop/src/ContextMenu.tsx:26-30`, `apps/desktop/src/main.tsx:13`). On macOS WKWebView and Windows WebView2, native spelling suggestions therefore probably already work; this is unverified.
  - WebKitGTK on Linux has spell checking disabled unless the app enables it.
- **Plan:**
  1. **Verify first** on macOS, Windows and Linux: underline, right-click suggestions, "Learn spelling". Record the results.
  2. Add a `spellcheck: bool` preference that sets the `spellCheck` attribute on the subject and body. Never toggle it while typing.
  3. **Language per platform:**
     - **macOS:** defer to the system ("Automatic by Language"); say so in Settings.
     - **Linux:** call `webkit_web_context_set_spell_checking_enabled` and `set_spell_checking_languages` through `WebviewWindow::with_webview` (webkit2gtk). Add a Settings language picker on Linux only.
     - **Windows WebView2:** uses the OS input languages; document that.
  4. Optional per-draft language: set `lang` on the editor root. Its effect varies by engine, so keep it low priority.
- **Pitfalls:**
  - **Per-engine behavior differs;** no single API covers all three.
  - **Large drafts:** spellcheck costs CPU on very long text (MS 1.6.1 added a cache). Exclude the quoted history (Mailspring sets `spellCheck={false}` on blockquotes, `base-block-plugins.tsx:130`).
  - **Grammar checking** via a remote service is a privacy issue. If added, make it opt-in and local or BYO; AI already exists as BYOK.
- **Done when:**
  - A manual matrix in `docs/` shows that a misspelled word is underlined and right-click offers suggestions on all three OSes.
  - Turning the toggle off removes the underlines.
  - The Linux language choice persists across restarts.
- **Effort:** S–M (mostly verification; Linux plumbing about 1 day).
- **Note:** probably partly working already. Verify before scheduling build work.

<a id="paste-and-match-style-sanitize-pasted-html"></a>

### Paste and Match Style; sanitize pasted HTML

*Roadmap: P1 · Composer*

- **Why:** Pasting from web pages, Word or Google Docs drags in fonts, colors and junk markup, and recipients see a patchwork. Unsanitized HTML in the composer is also a security hole. Demand: #603 / forum #533, a "No formatting" button.
- **Competitors:**
  - **Mailspring `onPaste`** (`composer-editor.tsx:268-330`):
    - Slate fragments paste natively. Files become attachments.
    - HTML goes through `SanitizeTransformer`, then the `InlineStyleTransformer` (a main-process hop), then `convertFromHTML` into the Slate schema.
    - Plain text has CRLF normalized.
    - The code comments that an `<img onerror>` would run "with full Node access".
  - **Mailspring "Paste and Match Style":** in the context menu and as `core:paste-and-match-style`, using Electron's `webContents.pasteAndMatchStyle()` (`mailspring/src/window-event-handler.ts:143`, `:242-244`).
  - **Mailspring changelog:** 1.21.0 improved paste sanitization; 1.24.0 fixed Windows colors and line spacing; 1.24.1 prefers HTML from Excel and OpenOffice; 1.26.0 strips carriage returns; 1.6.2 stopped dark-theme text colors from transferring.
  - **Other clients:** macOS apps use Opt+Shift+Cmd+V; Chrome and Windows use Ctrl+Shift+V.
- **Spec:** none.
- **Alice today:**
  - The textarea only accepts plain text, so paste is inherently safe.
  - DOMPurify 3.4 is already a dependency (`package.json`, used in `emailHtml.ts`).
  - The desktop sends API calls over Tauri IPC from the same webview. An XSS in the composer could call `daemon_request` and use the whole local API, including sending mail.
- **Plan:**
  1. With ProseMirror/TipTap, pasted HTML is parsed through the editor schema, which drops unknown tags and attributes.
  2. Add `transformPastedHTML`:
     - DOMPurify with a strict allowlist (no `style` except color, font-weight, font-style and text-decoration; no `class`, `id` or event attributes).
     - Strip text colors equal to the current theme's foreground or background.
     - Unwrap Google Docs' `<b style="font-weight:normal" id="docs-internal-guid-…">`.
     - Drop Word `mso-` styles and conditional comments.
     - Drop images with `file:` URLs.
  3. Paste and Match Style:
     - ProseMirror already pastes plain text when Shift is held (`view.input.shiftKey`).
     - Bind Mod+Shift+V explicitly, plus Opt+Shift+Cmd+V on macOS.
     - Check whether the native contenteditable context menu offers "Paste and Match Style" on WKWebView; if it doesn't, add an item in the editor's menu.
  4. Add a "Clear formatting" toolbar button (#533), using TipTap's `unsetAllMarks()` and `clearNodes()`.
  5. Normalize CRLF in plain-text paste (MS 1.7.7, 1.26.0).
- **Pitfalls:**
  - Excel tables need HTML preferred over the image flavor (MS 1.24.1). Remote images in pasted HTML are a tracking vector and break offline; keep them visible, but they are subject to the CSP `https:`.
  - Huge `data:` images in pasted HTML should be converted to inline attachments (see Inline images) or dropped.
  - The sanitizer must run **before** any DOM insertion (Mailspring's comment).
- **Done when:**
  - jsdom tests with paste fixtures from Word, Google Docs, Excel, a Gmail message and a page with `<img onerror>`, `<script>` and `javascript:` links. The output matches the expected clean HTML with no handlers or scripts.
  - A Shift-paste test inserts text only.
- **Effort:** S–M (1–2 days) after Rich text.

<a id="contact-groups-expand-in-recipient-fields"></a>

### Contact groups expand in recipient fields

*Roadmap: P2 · Composer*

- **Why:** Teams and families mail fixed groups; typing ten addresses every time is slow and error-prone.
- **Competitors:**
  - **Mailspring** (1.7.0 address book):
    - Syncs Google and CardDAV groups.
    - `ContactStore.searchContactGroups` returns up to 4 groups by name prefix (`mailspring/src/flux/stores/contact-store.ts:17-25`).
    - Choosing a group token expands it immediately into member contacts in that field (`mailspring/src/components/participants-text-field.tsx:138-170`).
    - 1.7.7 fixed groups of more than 999 contacts hitting a SQLite variable limit.
  - **Apple Mail:** Contacts groups expand, with an option to keep the group name.
  - **Thunderbird:** address-book mailing lists expand at send.
  - **Outlook:** contact groups expand with "+".
- **Spec:**
  - RFC 6350 §6.1.4 (`KIND:group`) and §6.6.5 (`MEMBER`, usually `urn:uuid:` or `mailto:`).
  - Apple and iCloud use `X-ADDRESSBOOKSERVER-KIND:group` and `X-ADDRESSBOOKSERVER-MEMBER`.
  - Google People API: `contactGroups.list` and `person.memberships`, available with the existing `contacts.readonly` scope (`oauth.rs:29`).
  - macOS: `CNGroup` and `CNContact.predicateForContactsInGroup`.
  - Microsoft Graph has no endpoint for personal contact groups (Outlook "contact lists"); verify before promising Outlook support.
- **Alice today:**
  - The `contacts` table is `(owner, source, email, name)` (`store.rs:26`) with no groups.
  - The vCard reader ignores groups (`crates/alice-core/src/contacts.rs:746`).
  - macOS import has no groups (`apps/desktop/src-tauri/src/contacts.rs`).
  - Recipients are a text field, not chips (`Compose.tsx:115-163`).
- **Plan:**
  1. Add tables `contact_groups(owner, source, id, name)` and `contact_group_members(owner, source, group_id, email)`, replaced on each contacts sync, as contacts are today.
  2. **Sources:**
     - Google: `contactGroups.list` (user groups only) plus memberships.
     - CardDAV: `KIND:group`/`MEMBER` and the Apple `X-ADDRESSBOOKSERVER-*` variants. Resolve `urn:uuid` to member `UID` and accept `mailto:`.
     - macOS: `CNGroup`; add the feature to the `objc2-contacts` dependency.
  3. `/v1/contacts` returns group suggestions as `{ name, email: "", group: { id, count } }`.
  4. Accepting one expands it into member addresses in the current field. Show "Team (8)" in the suggestion row.
- **Pitfalls:**
  - Nested groups need cycle detection. Members without email are skipped.
  - Big groups exceed Gmail's 100-recipient SMTP limit; suggest Bcc for large groups.
  - Members of a stale group may have changed; that is acceptable because expansion is a snapshot.
  - Keep the expansion per account owner, since groups belong to an address book.
- **Done when:**
  - Parser tests for an RFC 6350 group vCard and an Apple-style one.
  - A Google `contactGroups` fixture test.
  - A daemon test: `/v1/contacts?q=tea` returns the group, and expanding it yields its members.
  - Smoke: accepting the group fills the field.
- **Effort:** M–L (4–6 days). Depends on Cc and Bcc fields.
- **Note:** demand is low in the Mailspring data (no top issue). Consider moving this to P2.

<a id="missing-attachment-warning-attached-with-no-file-and-optional-external-recipient"></a>

### Missing-attachment warning ("attached" with no file) and optional external-recipient warning

*Roadmap: P1 · Composer*

- **Why:** "See attached" with nothing attached is the most common sending mistake. External-recipient and large reply-all warnings prevent leaks and reply-all storms. Mailspring's "Warn on Reply All" is still an open request (#2299).
- **Competitors:**
  - **Mailspring:**
    - `validateDraftForSending` (`mailspring/src/flux/stores/draft-editing-session.ts:277-311`) warns on an empty subject, and on the substring "attach" with no files after stripping quoted HTML and the signature. The check is English-only.
    - `validateDraftRecipients` (`:313-388`) errors on invalid addresses. It warns when the salutation ("Hi Ben") matches no recipient name (1.0.6), with "don't warn again" stored in localStorage (`:391-404`).
    - 1.10.7 made recipient warnings optional. 1.1.3 stopped "attachments" in the signature from triggering. 1.13.2 added more invalid-recipient errors.
    - No external-recipient or reply-all warnings.
  - **Thunderbird:**
    - `mail.compose.attachment_reminder` (default on). Keywords are localized; en-US: `.doc,.pdf,.xls,.ppt,.rtf,.pps,attachment,attach,attached,attaching,enclosed,CV,cover letter`.
    - It scans in a worker while typing, shows a notification bar, and alerts at send if that was ignored (`_aggressive`).
    - It skips blockquotes, quoted reply headers and the signature. It uses word boundaries, and extension keywords ignore URLs.
  - **Other clients:**
    - Gmail and Outlook warn at send.
    - Exchange MailTips "large audience" warns above 25 recipients by default (`MailTipsLargeAudienceThreshold`), and has an external-recipients MailTip.
- **Spec:** none.
- **Alice today:** no warnings. Only the server-side "recipient required" and draft limits exist (`daemon.rs:477-479`).
- **Plan:**
  1. Add a pure `fn send_warnings(draft, account, prefs) -> Vec<Warning>` in alice-core, exposed as `GET /v1/messages/{id}/warnings`, so the desktop and TUI share it:
     - **Missing attachment:** strip the quoted part and the signature marker, then match localized keywords with word boundaries. Add a `compose.attachment_keywords` key per locale: en as in Thunderbird; de `Anhang, angehängt, anbei, beigefügt, Anlage, .pdf, .doc`. Warn only when there are no attachments, checking the subject only if the user edited it.
     - **Large audience:** total recipients ≥ 25 (Exchange's default threshold), or a reply-all that adds more than N people beyond the original sender (#2299).
     - **External recipients** (opt-in per account): any recipient whose domain is not the account or identity domain. Skip this check for consumer domains such as gmail.com and outlook.com.
     - **Empty subject.**
  2. The desktop calls it on Send, before scheduling (Undo send). A dialog lists the warnings with "Send anyway" (Enter) and "Back to draft". Per-type "Don't warn again" toggles live in Settings, not localStorage.
  3. The TUI shows the same warnings as a confirm prompt. The CLI and MCP skip them; MCP could return warnings in `send_draft`'s response.
- **Pitfalls:**
  - **False positives:** quoted text and signatures (MS 1.1.3), "attached" meaning a link, keywords inside URLs. Thunderbird's exclusions are the model.
  - **i18n:** an English-only check misses German users, and Alice ships de.
  - The dialog must not trap keyboard-only senders.
  - A recipient on both To and Bcc counts once.
- **Done when:**
  - Unit tests: en and de keywords trigger; the same words in quote or signature don't; `.pdf` inside a URL doesn't; an attachment present means no warning.
  - Unit tests: 25 recipients triggers the large-audience warning; external detection respects identity domains.
  - Smoke: "see attached" plus Send shows the dialog, and "Send anyway" sends.
- **Effort:** S (1–2 days).
- **Note:** the `MS 1.10.7` tag refers to "recipient warnings are now optional". The attachment warning existed by 1.1.3.

<a id="pop-out-composer-window-titled-with-the-subject"></a>

### Pop-out composer window, titled with the subject

*Roadmap: P1 · Composer*

- **Why:** Writing long mail while reading other threads requires a separate window. Switchers from Apple Mail and Outlook expect windowed compose. Demand: forum #771 "use popout composer for all emails".
- **Competitors:**
  - **Mailspring:**
    - Replies are composed inline in the thread by default, and new mail opens in a window.
    - `_onPopoutDraft` commits the session and calls `AppEnv.newWindow({ windowType: 'composer', windowKey: 'composer-<id>' })`, reusing the existing window for that draft (`draft-store.ts:395-416`).
    - The window title is the subject, live-updated (`mailspring/internal_packages/composer/lib/main.tsx`, 1.18.0).
    - Shift+R / Shift+A / Shift+F reply or forward in a new window (Gmail keymap). mailto links and "Mail files" open pop-outs (`draft-store.ts:417-445`).
    - Fixes: 1.24.1 wrong toolbar and a double title bar on Windows; 1.0.3 a double window bar on Linux.
  - **Other clients:** Apple Mail and Outlook compose in windows by default; Gmail web pops out with Shift-click.
- **Spec:** none.
- **Alice today:**
  - A modal `<dialog>` in the main window (`Compose.tsx:84-99`).
  - One window in `tauri.conf.json` (`titleBarStyle: Overlay`).
  - Capabilities apply to `windows: ["main"]` with `core:default` and `core:window:allow-start-dragging` (`apps/desktop/src-tauri/capabilities/default.json`).
- **Plan:**
  1. **Window:**
     - From JS: `new WebviewWindow('compose-' + draftId, { url: '/#compose=' + draftId, title, width: 720, height: 640 })`. If a window with that label exists, focus it.
     - Capabilities: add `"compose-*"` to `windows` and the permissions `core:webview:allow-create-webview-window`, `core:window:allow-set-title`, `core:window:allow-close` and `core:window:allow-set-focus`.
  2. **Entry and title:**
     - `main.tsx` checks `location.hash` and renders a `ComposeWindow`: the same form as `Compose.tsx`, but not modal. It loads the draft with `GET /v1/messages/{id}`, or a prepared `Compose` passed through the URL for unsaved drafts. Saving first is simpler.
     - The title is `subject.trim() || t('compose.newMessage')`, set on change with `getCurrentWindow().setTitle`.
  3. **Opening it:**
     - A pop-out button in the modal saves, closes the modal and opens the window.
     - Shift+R, Shift+A and Shift+F open the reply or forward in a window.
     - A preference "Open the composer in a new window" (#771).
  4. **Lifecycle:**
     - `onCloseRequested` saves the draft, the same as Esc today.
     - Send closes the window. The main window refreshes through the existing snapshot revision polling.
     - On app quit, every compose window saves first.
- **Pitfalls:**
  - **One window per draft:** two windows editing the same draft would race autosaves, so key windows by draft id.
  - **Shortcuts:** the compose window must not run the main window's global shortcuts. It loads only the composer, not App.
  - **Overlay title bar:** needs a drag region in the composer header.
  - **localStorage backup** is shared by origin, which is fine and lets drafts be recovered from either window.
  - **Windows:** watch for a double title bar (MS 1.24.1).
- **Done when:**
  - Clicking pop-out opens a native window titled with the subject, and typing a new subject updates the title.
  - Closing the window saves the draft, and it appears in Drafts.
  - Opening the same draft again focuses the existing window.
  - Sending from the window updates the main list.
- **Effort:** M (2–3 days). Pairs with the P1 `mailto:` handler, which should open a compose window.

<a id="automatic-ccbcc-per-account"></a>

### Automatic CC/BCC per account

*Roadmap: P1 · Composer · new*

- **Why:** People who archive outgoing mail to a CRM, ticketing address or a second mailbox need every message copied automatically. The lead observed this setting in Mailspring 1.26's account preferences.
- **Competitors:**
  - **Mailspring** (1.0.12): Preferences → Accounts, "When composing, automatically [CC / BCC]: addresses". It is stored as `account.autoaddress { type: 'cc' | 'bcc', value }` (`account.ts:63-66`, `:126-130`) and added to every new draft, replies included, in `DraftFactory.createDraft` (`draft-factory.ts:79-85`).
  - **Thunderbird:** per identity, "Cc these email addresses" and "Bcc these email addresses".
- **Spec:** RFC 5322 §3.6.3.
- **Alice today:** nothing.
- **Plan:**
  1. Add `auto_cc` and `auto_bcc: Vec<String>` on the account's default identity; per identity once the Send-as identities item lands.
  2. The daemon draft factory adds them for new, reply and forward drafts, skipping addresses already present (dedupe as in Cc/Bcc).
  3. Add Settings fields under Accounts and show the result in the composer, so it is never a hidden copy.
- **Pitfalls:**
  - A reply-all to a thread that already includes the auto address must not duplicate it.
  - An automatic Bcc counts toward the 100-recipient limit.
  - Don't apply it to RSVP replies (`rsvp_mail`) unless the user asks.
- **Done when:** a factory test shows an auto-Bcc on new and reply drafts, with no duplicate when the address is already present.
- **Effort:** S (≤1 day). Depends on Cc and Bcc fields and the daemon draft factory.

<a id="formatflowed-and-hard-wrap-at-72-for-plain-text-mail"></a>

### format=flowed and hard wrap at 72 for plain-text mail

*Roadmap: P2 · Composer · new*

- **Why:** Mailing lists and patch workflows (the Linux kernel, many open-source projects) expect plain text wrapped at about 72 characters. Format=flowed makes such mail reflow on phones. This was Mailspring's top composer request: #52 with 101 👍; plus #46 (28 👍) and forum #785 "Line wrapping, plaintext support for Linux Kernel community".
- **Competitors:**
  - **Mailspring:** hard-wraps plain drafts at 72 with zero-width-space soft breaks (`plaintext.ts:1-60`). It never sends `format=flowed`, so recipients get hard breaks.
  - **Thunderbird:** `mailnews.wraplength` 72 and `mailnews.send_plaintext_flowed` true by default.
  - **Kernel guidance:** for patches, the kernel docs tell Thunderbird users to set `send_plaintext_flowed=false` and `wraplength=0`. Format=flowed "can cause unexpected and unwanted line breaks", and clients must not wrap or alter patch lines, including tabs and trailing whitespace (https://docs.kernel.org/process/email-clients.html).
  - **Gmail web** is listed as unsuitable for patches: it wraps at 78, turns tabs into spaces and base64-encodes non-ASCII.
- **Spec:** RFC 3676:
  - §4.1–4.2: a line ending in a space is a soft break. Generated lines should be ≤ 78 characters, typically wrapped at about 72.
  - §4.4: space-stuff lines starting with a space, `>` or `From `.
  - §4.5: quote depth; no flowed line before a depth change.
  - §4.3: the `-- ` signature line is neither flowed nor fixed.
  - DelSp (default No).
  - Avoid quoted-printable unless needed for 8-bit content.
- **Alice today:**
  - Plain text is sent as-is. lettre picks 7bit or quoted-printable and converts newlines to CRLF.
  - No wrapping and no `format=flowed`.
- **Plan:**
  1. Add a plain-text preference "Line wrapping": Off (default, safe for patches) / Wrap at 72 with format=flowed / Hard wrap at 72.
  2. **Flowed mode:** a pure `fn flow(text: &str, width: usize) -> String` in `providers.rs`:
     - Wrap paragraphs at spaces (≤ 72, never splitting words).
     - Add the trailing space for soft breaks and space-stuff the §4.4 cases.
     - Preserve `>` depth per paragraph and leave the `-- ` line alone.
     - Set `Content-Type: text/plain; charset=utf-8; format=flowed`. lettre's `ContentType::parse` accepts the parameter. Don't add DelSp.
  3. **Hard-wrap mode:** the same wrapping without trailing spaces and without the format parameter.
  4. **Preview:** show a 72-column guide in the plain-text editor when wrapping is on. Wrap only at send, so editing stays natural (avoids Mailspring's zero-width-space hack).
  5. Never wrap lines that look like patch content (start with `+`, `-`, `@@`, `diff `) in hard-wrap mode. Document "Off" as the setting for patches.
- **Pitfalls:**
  - **Quoted-printable** must not eat the soft-break trailing spaces. With 7bit or 8bit, trailing spaces survive. If lettre picks quoted-printable (non-ASCII text without 8BITMIME), the trailing space becomes `=20`, which RFC 3676 tolerates, but verify that receivers decode it.
  - **Width:** CJK text has no spaces; count width with `unicode-width`, or simply don't wrap long unbroken runs.
  - **Signed mail:** PGP inline signatures break with flowed text (RFC 3676 §4.6).
- **Done when:**
  - Unit tests for `flow()`: a paragraph wraps at 72 with trailing spaces; lines starting with `From ` or `>` are stuffed; the `-- ` line is untouched; a quoted paragraph keeps `> `; patch-like lines are untouched in hard-wrap mode.
  - The MIME has `format=flowed`, and `mailparse` round-trips it.
- **Effort:** S–M (1–2 days). Depends on Plain-text mode.

<a id="markdown-shortcuts-in-the-composer"></a>

### Markdown shortcuts in the composer

*Roadmap: Merged into another item · new*

- **Why:** Many technical users type Markdown out of habit. Demand: GitHub #19 (21 👍), #1091 (19 👍), #891 (7 👍), forum #322 (13 votes).
- **Competitors:**
  - **Mailspring:** "markdown shortcuts" since 1.1.0 (`mailspring/src/components/composer-editor/markdown-plugins.tsx`):
    - `**bold**` and `*italic*` while typing (slate-auto-replace).
    - ```` ``` ```` for a code block, and `` `code` ``.
    - `- ` and `1. ` start lists (`base-block-plugins.tsx:534-546`).
    - Users still asked for full Markdown composing (write Markdown, send HTML).
  - **Other clients:**
    - Markdown Here (browser extension) renders a selection.
    - Thunderbird, Apple Mail and Outlook have none natively.
    - Superhuman and Spark support basic shortcuts.
- **Spec:** CommonMark (https://spec.commonmark.org/) for any "paste Markdown" conversion.
- **Alice today:** nothing (plain textarea).
- **Plan:**
  1. **WYSIWYG shortcuts first.** With TipTap 3, StarterKit's input rules already convert `# `, `- `, `1. `, `> `, ```` ``` ````, `**…**`, `*…*`, `` `…` `` and `~~…~~` as you type. Keep them enabled, verify they don't fire inside code, and add a Link input rule for `[text](url)`.
  2. Document the shortcuts in Settings → Shortcuts (`shortcuts.ts` group "Formatting").
  3. **Optional "Paste as Markdown"** (later): convert pasted Markdown text to HTML.
     - Daemon side: `pulldown-cmark` is a **new** dependency, but small and pure Rust.
     - Desktop side: TipTap's Markdown extension.
     - Only with a modifier, or when the paste is plain text that parses to block structure.
- **Pitfalls:**
  - Accidental conversion (`*` in math, `-` at line start in plain notes): Mod+Z must undo the transform, which ProseMirror input rules support (`undoInputRule`).
  - Rules must not apply in plain-text mode.
  - In code blocks, never convert.
- **Done when:** jsdom editor tests: typing `- item` makes a list, `**b**` makes bold, Mod+Z after the rule restores the literal text, and nothing converts inside a code block.
- **Effort:** S (≤1 day) once Rich text uses TipTap. M if full Markdown paste is added.
