# Reading, accounts, localization and accessibility

Research behind the matching items in [`todo.md`](../../todo.md). Each entry covers why it matters, how Mailspring and other clients do it, the relevant specs, what Alice has today, a build plan, pitfalls, acceptance criteria and effort. Code citations point at the Alice working tree as of 2026-10-13; Mailspring paths are relative to [Foundry376/Mailspring](https://github.com/Foundry376/Mailspring) (`mailspring/`) and [Foundry376/Mailspring-Sync](https://github.com/Foundry376/Mailspring-Sync) (`mailsync/`).

## Section findings

**Two primitives unblock most of Reading.**

1. **A raw-message fetch.** Add `GET /v1/messages/{id}/raw`, reusing the `UID FETCH … BODY.PEEK[]` path that `Remote::attachment` already has (`crates/alice-core/src/remote.rs:339-345`). Alice stores only parsed JSON, so this endpoint is what makes these possible:
   - Show original
   - .eml/mbox export
   - forward-as-attachment and dragging threads into the composer
   - TNEF unpacking
2. **An auxiliary window in the Tauri shell.** A `WebviewWindow` with its own near-empty capability that hosts sanitized mail. It serves print and "show full message", and later a pop-out reader.

Suggested Reading build order: Show original → aux window + print → clipping → attachment preview → adaptive dark mode.

**Accounts: data model first.** The `Account` model (`crates/alice-core/src/model.rs:12-29`) has none of these fields: username, per-protocol security, sender name, color, order, folder overrides. `POST /v1/accounts` is the only write path. It validates IMAP but never SMTP (`crates/alice-core/src/daemon.rs:792-802`).

Add the fields with serde defaults (no migration needed) and a `PATCH /v1/accounts/{id}`. Then build in this order:
1. STARTTLS, security mode, username and SMTP validation
2. presets
3. re-authentication
4. edit / reorder / color, plus folder mapping
5. built-in Microsoft client id and live verification
6. shared mailboxes
7. autodiscovery (largest; it reuses the presets and the security fields)

**TLS stack finding.** On macOS, `native-tls` uses Secure Transport, which cannot negotiate TLS 1.3. Alice therefore cannot reach TLS-1.3-only servers on macOS. Mailspring has the same problem, still open as #2507.

Moving IMAP/SMTP to rustls with `rustls-platform-verifier` fixes this and also gives clean per-account certificate pinning for Proton Bridge. Both crates are already in `Cargo.lock` via reqwest, and lettre has matching features.

**Already done or ahead.**
- The "list above message" layout already exists (`MailLayout::Horizontal`).
- The dark theme already keeps a light card for HTML mail.
- Runnable attachments are revealed in the folder rather than opened; Mailspring opens them with no check.
- The i18n system already has typed keys and CLDR plurals, which Mailspring lacks.
- Compose uses a native modal `<dialog>`.

**Localization and accessibility.**
- Keep the in-house i18n. The work is infrastructure: lazy catalogs, region-aware matching, Rust plural rules for ru/pl/uk/ar/he, and a machine-translation script.
- Ship `dir="auto"` for mail content before any RTL UI locale.
- The biggest accessibility gaps are the thread list (every row is a Tab stop) and the lack of a persistent live region.

**New dependencies this section would add:**
- `hickory-resolver` (MX/SRV)
- `objc2-quick-look-ui` (macOS)
- `pdfjs-dist` (PDF on Windows/Linux)
- `tauri-plugin-deep-link` and `tauri-plugin-single-instance` (permalinks)
- `tnef`
- `eslint-plugin-jsx-a11y` (dev)
- optionally `tauri-plugin-dialog` (import)

Everything else uses what is already in the tree: `quick-xml`, lettre's `test_connection`, `imap`'s `secure()` and `append_with_flags_and_date`, rustls.

**Additions to the roadmap.**
- New items: "Manual folder role mapping" (P1 Accounts), "TNEF (winmail.dat) decoding" (P2 Reading), "Attachment position option" (P3, filed under P2 Reading).
- Notes: the P2 layout item is done, and import is a differentiator rather than parity.

## P1 → Reading

<a id="in-app-attachment-preview-images-pdf-text-office-docs-quick-look-on-macos"></a>

### In-app attachment preview: images, PDF, text, Office docs; Quick Look on macOS

*Roadmap: P1 · Reading*

- **Why:** Opening each attachment today writes it into Downloads and launches another app. Glancing at a PDF or photo should not leave a file behind or switch windows.
- **Competitors:**
  - **Mailspring** (`repos/mailspring/app/src/quickpreview/index.ts`):
    - Limits: `FileSizeLimit` 5 MB (`:42`); larger files get no preview. `PreviewTimeout` 5000 ms (`:44`).
    - Strategy (`:64-119`):
      - On macOS, the native Quick Look panel (Electron `previewFile`, `:226-230`) handles everything except a blacklist (images, archives, dmg, exe, ics, `:49-62`) and code/markdown.
      - Elsewhere it renders in a separate 800×600 window with its own session partition and CSP (`:17-33, 249-257`). It uses pdf.js for PDF, mammoth for docx, xlsx for spreadsheets/CSV, snarkdown for Markdown, and Prism for code and text.
    - Thumbnails: macOS shells out to `qlmanage -t` (`:455-500`); other platforms use a hidden capture window (`:313-453`).
    - Timeout fix (1.24.0, #2862, `ms.md:212`): thumbnail jobs now time out after 5 s and are killed with SIGKILL. `qlmanage` hung on extensionless files and exits 0 on SIGTERM, and leaked processes exhausted Quick Look's thread pool.
    - Inline images: jpg/png/gif/webp/bmp between 512 B and 5 MB render under the message (`app/src/flux/models/utils.ts:175-190`).
    - Keyboard and setting: Space opens the preview when an attachment is focused (`app/src/components/attachment-items.tsx:159-164`). The setting `core.attachments.displayFilePreview` defaults to true.
    - Other changelog entries:
      - 1.5.5: previews on all platforms.
      - 1.5.6: code/text previews.
      - 1.10.2: Quick Look remote-code-execution fix for crafted attachments.
      - 1.22: preview CSP was clobbering the app CSP.
      - 1.26: links in preview windows open only after a real click.
    - There is no warning before opening executables (`attachment-store.ts:168-175`); only the filename is sanitized (`file.ts:80-97`).
  - **Apple Mail and Mimestream:** Space opens Quick Look.
  - **Outlook:** in-pane previewers for Office and PDF.
- **Spec:** pdf.js: use ≥ 6.2.108. CVE-2026-16633 is arbitrary JS when `enableScripting` is on. Also CVE-2024-4367; set `isEvalSupported: false` and `enableScripting: false`. Apple Quick Look: `QLPreviewPanel` via the `objc2-quick-look-ui` crate 0.3; `qlmanage -p` is documented as a debug tool.
- **Alice today:**
  - **Click to open:** clicking saves the file once into Downloads and opens it with the default app (`apps/desktop/src-tauri/src/downloads.rs:93-160`).
  - **Programs:** programs and scripts are revealed in the folder rather than run (`downloads.rs:299-309`). This is ahead of Mailspring.
  - **Bytes over IPC:** `read_attachment` already returns raw bytes (`downloads.rs:78-90`, used by `attachmentBytes` in `apps/desktop/src/api.ts:113-118`). Inline `cid:` images use it to build `data:` URLs (`apps/desktop/src/MessageView.tsx:12-26`).
  - **CSP:** the app CSP allows `img-src 'self' data: https:` and no `blob:` frames (`apps/desktop/src-tauri/tauri.conf.json`).
  - **Missing:** no previews, thumbnails or Quick Look.
- **Plan:**
  1. **Cache command.** Add `preview_attachment(message_id, index)` in `downloads.rs`. It writes the file to `app_cache_dir()/previews/<message>/<safe name>` (reusing `file_name()`; never Downloads) and returns the path. Prune the folder on startup (files older than 1 day).
  2. **macOS Quick Look.** In a new `src-tauri/src/quicklook.rs`, use `objc2-quick-look-ui`'s `QLPreviewPanel::sharedPreviewPanel()` with a `define_class!` data source returning `NSURL`s. Alice already defines an objc2 delegate this way (`notification.rs:88-144`). Run it on the main thread (`app.run_on_main_thread`).
     - This covers PDF, Office, iWork, images, video and text natively.
     - Fallback if the panel/responder-chain work slips: spawn `qlmanage -p <path>`. Kill the previous child before starting a new one; never leave children running (Mailspring 1.24 lesson).
  3. **In-app overlay (all platforms).** In a new `apps/desktop/src/Preview.tsx`, open a native `<dialog>` (like Compose):
     - **Images:** (png/jpeg/gif/webp, plus heic/tiff on macOS) as `<img src=blob:>`. Add `blob:` to `img-src` in `tauri.conf.json`.
     - **Text:** `text/*`, `.log`, `.csv`, `.json`, `.md` as escaped `<pre>`, capped at 1 MB, decoded with `TextDecoder(charset)`.
     - **Everything else:** "Open" and "Show in Finder" buttons.
     - **Navigation:** arrows move between the message's attachments, and Esc closes.
  4. **PDF on Windows/Linux.** Use `pdfjs-dist` (new dependency, ≥ 6.2.108), lazy-loaded only in the preview dialog. Render to `<canvas>` with `isEvalSupported:false` and `enableScripting:false`, with the worker from `'self'`.
     - Do not rely on WebView PDF plugins in iframes: WKWebView/WebKitGTK only handle main-frame PDF navigation.
     - Defer Office documents on these platforms to "Open". Mailspring's mammoth/xlsx renderers are heavy dependencies for a rare need.
  5. **Keyboard and menus.** In `Attachments.tsx`, Space on a focused attachment opens the preview (Quick Look on macOS). Add "Quick Look"/"Preview" to the attachment context menu (`Attachments.tsx:53`). Add the shortcut to `shortcuts.ts` so it lists in Settings.
  6. **Optional thumbnails.** Defer them; they need the bytes of every attachment and Alice deliberately fetches attachments on demand (`remote.rs:1-7`).
- **Pitfalls:**
  - **Never render untrusted HTML/SVG in a frame.** A `blob:` URL inherits the app origin, so script in it would reach Tauri IPC. Show SVG only through `<img>` and HTML as text.
  - **Memory:** large files cross IPC whole. Cap previews at 25 MB and fall back to Open.
  - **Quick Look itself:** has had exploitable parsers (Mailspring 1.10.2). Keep the macOS sandbox defaults and never preview runnable types (reuse `runs_when_opened`).
  - **Cache privacy:** preview files are plaintext copies of mail. Delete them on account removal, not just by age.
  - **Offline:** attachments need the server today, so show "Not available offline" instead of a spinner.
- **Done when:**
  - A unit test in `downloads.rs` shows preview paths stay under the cache directory for hostile names (`../`, bidi overrides, reserved Windows names) and never touch Downloads.
  - A test for a pure `previewKind(contentType, name)` covers image, text, pdf and other.
  - Manual: Space on PDF, DOCX and JPEG attachments on macOS opens Quick Look. On Windows/Linux a PDF opens in the overlay with scripting disabled, and `.exe` offers only "Show in folder".
- **Effort:** M. Related: "Show original: raw source and full headers" (shares the on-demand fetch path).

<a id="print-and-save-as-pdf"></a>

### Print and Save as PDF

*Roadmap: P1 · Reading*

- **Why:** Receipts, tickets and contracts still get printed or archived as PDF. Printing the window today would include the sidebar and cut long messages.
- **Competitors:**
  - **Mailspring entry points:** File → "Print Current Thread" (`menus/darwin.js:55`), `mod+p` (`keymaps/base.json:24`), and a print icon in the subject line.
  - **Mailspring print window:**
    - `internal_packages/print/lib/printer.ts:7-37` clones each message iframe's document and removes the dark render-mode style (1.25 fix).
    - `print-window.ts` opens a visible window titled "Print - <subject>" with Close, Print and "Save as PDF" buttons (`:33-90`). Subject and participants are HTML-escaped (1.13.3 security fix).
    - "Save as PDF" calls `webContents.printToPDF({pageSize:'Letter', margins 0, printBackground:true})` (`:92-122`).
    - `static/print-styles.css` hides UI chrome and collapsed messages and warns that collapsed messages won't print (1.4.0).
  - **Mailspring changelog fixes:** 1.2.2 (Windows menu bar), 1.5.4 (reliability + Save as PDF), 1.22 (Windows crash from file URL), 1.25 (print inverted in dark email mode).
  - **Other clients:** Apple Mail and Thunderbird print through the system dialog, whose PDF button gives Save as PDF.
- **Spec:**
  - Tauri `WebviewWindow::print()`: macOS uses NSPrintOperation, Windows evaluates `window.print()`, Linux runs a GTK PrintOperation (`docs.rs/tauri` `WebviewWindow::print`; wry `print`, local copy `scratchpad/rwry.rs:1620`).
  - On macOS Tauri replaces `window.print` with an IPC call that needs the `core:webview:allow-print` capability, which is not in the defaults.
  - `iframe.contentWindow.print()` does not work in WKWebView.
- **Alice today:**
  - No print command, no `@media print`, no `print` capability (`apps/desktop/src-tauri/capabilities/default.json`).
  - The HTML body lives in a sandboxed `srcdoc` iframe whose height is clamped to 12,000 px with inner scrolling (`MessageView.tsx:105-111`). Printing the main webview would clip long mail.
- **Plan:**
  1. **Print document.** Add `printDocument(messages, {format, loadImages, locale})` in `apps/desktop/src/emailHtml.ts`.
     - For each open message: an escaped header block (From, To, Cc, Date, Subject via `i18n.date`), the body from `emailDocument()` (same sanitizer and CSP meta; HTML or plain text per the reader setting), and a list of attachment names.
     - Force `color-scheme: light` and `@page { margin: 16mm }`.
     - Leave out collapsed messages, and say so in a footer, as Mailspring does.
  2. **Print window.** Add a Tauri command `print_html(html)` in a new `src-tauri/src/print.rs`.
     - It creates a hidden `WebviewWindow` labelled `print`, loads a bundled `print.html`, and injects the document with `webview.eval` or an event.
     - After load it calls `window.print()` from Rust (`WebviewWindow::print`), then closes the window.
     - Give it its own capability file with only `core:webview:allow-print`. Mail HTML must never run in a window with daemon access.
  3. **Entry points.** Add Mod+P in `shortcuts.ts`, a "Print…" item in the thread and message context menus (`App.tsx:274` `threadMenu`), and a toolbar button in the reader.
  4. **Save as PDF.** Rely on the system dialog's PDF option: macOS PDF menu, Windows "Microsoft Print to PDF", GTK "Print to File".
     - A one-click "Save as PDF" needs platform code: `NSPrintOperation` with `NSPrintSaveJob` on macOS, WebView2 `PrintToPdf` on Windows, via `with_webview`. Note that `WKWebView.createPDF` makes one unpaginated page. Defer it.
  5. **CLI.** Not needed; `alice show` output can be piped.
- **Pitfalls:**
  - **Dark mode:** printing must ignore any dark rendering (Mailspring 1.25).
  - **Remote images:** remote images print only if already allowed for that message. Printing must not trigger new tracking loads.
  - **Escaping:** escape every header value; Mailspring had an injection here (1.13.3).
  - **Embedded images:** `cid:` images must be resolved into `data:` before printing (reuse `useEmbedded`).
  - **Windows:** `window.print()` on WebView2 shows Edge's print preview. Test that it prints the hidden window's content and not the main window.
- **Done when:**
  - A `scripts/test-html.mjs` case shows `printDocument` output has an escaped `<script>` subject, no `<script>` elements, a light colour scheme, and every open message's header.
  - Manual: a 3-message thread with a 30,000 px newsletter prints completely on macOS and Windows with no sidebar. Printing to PDF from the dialog works.
- **Effort:** M. Shares the auxiliary-window code with "Clip very long messages with a 'show full message' popout to keep scrolling fast".

<a id="show-original-raw-source-and-full-headers"></a>

### Show original: raw source and full headers

*Roadmap: P1 · Reading*

- **Why:**
  - Diagnosing phishing (Authentication-Results, Received chains), delivery problems and broken rendering needs the raw message.
  - Support requests to Alice will need it too.
- **Competitors:** Mailspring:
  - Menu: "Show Original" is in the message's reply dropdown and "…" menu, next to "Download as .eml" (`internal_packages/message-list/lib/message-controls.tsx:42-54, 119-155`). There is no shortcut.
  - Fetch: `GetMessageRFC2822Task` makes mailsync fetch the message by UID from any folder copy (`repos/mailsync/MailSync/TaskProcessor.cpp:2593-2631`) into a temp file.
  - Display: the file opens in an 800×600 window with JavaScript disabled.
  - Missing message: "Could not retrieve the original message. Please try again."
  - Changelog: 1.0.8, 1.10.3, 1.23 (crash when fetch failed), 1.26 (link guard).
  - Headers: a separate toggle shows full parsed headers (`core.reading.detailedHeaders`, default off, `message-item.tsx:229-255`).
- **Other clients:**
  - Gmail's "Show original" adds an SPF/DKIM/DMARC summary plus "Download original" and "Copy to clipboard".
  - Apple Mail: View → Message → Raw Source (⌥⌘U).
  - Thunderbird: View → Message Source (Ctrl+U).
- **Spec:** RFC 5322 (message format), RFC 8601 (Authentication-Results).
- **Alice today:**
  - Raw source is not stored: messages are kept as parsed JSON (`crates/alice-core/src/store.rs:20`), and the raw header block seen during sync (`remote.rs:41-49`) is discarded.
  - `Remote::attachment` already falls back to `UID FETCH … BODY.PEEK[]` (`remote.rs:339-345`), so the fetch path exists.
  - The reader shows From, To and Date only (`apps/desktop/src/Conversation.tsx:35`).
- **Plan:**
  1. **Fetch.** Add `Remote::raw(mailbox, validity, uid) -> Vec<u8>` in `remote.rs`. It checks UIDVALIDITY like `attachment()` and caps at `FULL_MESSAGE_LIMIT` (30 MB, `remote.rs:21`).
  2. **Endpoint.** Add `GET /v1/messages/{id}/raw` in `daemon.rs`.
     - It returns `message/rfc822` with `Content-Disposition: attachment; filename="<subject>.eml"`.
     - Factor the connect/credentials/`spawn_blocking` boilerplate out of `attachment()` (`daemon.rs:521-575`) into one helper shared by both.
     - Locally created messages without a remote UID (drafts, sends not yet synced) return a clear "not on the server yet" error.
     - Document it in `docs/api.md`.
  3. **Viewer.** Add `apps/desktop/src/Original.tsx`, a native `<dialog>` with:
     - a summary table: Message-ID, Date, From, To, Subject, and the first `Authentication-Results` header's spf/dkim/dmarc verdicts, parsed in JS;
     - the raw text in a `<pre>` (UTF-8 lossy, first 2 MB, with "Download .eml" for the rest);
     - "Copy" and "Download .eml" buttons, saving to Downloads through the existing attachment saving path.
     - Entry points: a "Show original" item in the message context menu (`App.tsx:400`) and Mod+Alt+U.
  4. **CLI and MCP.** Add `alice show --raw <id>` (writes bytes to stdout) and optionally a read-only MCP tool.
- **Pitfalls:**
  - **Untrusted text:** treat the raw source as untrusted. Render it with `textContent` only and never linkify inside it.
  - **Moved messages:** UIDs change when a message moves. A validity or UID mismatch gives `mail.message.no_longer_available` with "sync and try again".
  - **Gmail copies:** Gmail keeps one copy across labels; any `remote_mailbox` works.
  - **Authentication-Results:** only the top-most header added by the receiving provider is trustworthy. Lower ones can be forged by the sender.
- **Done when:**
  - A `scripts/test-mail.py` Dovecot test APPENDs a message and checks `GET /raw` returns identical bytes.
  - A unit test covers filename sanitization (control characters, `/`, 200+ chars).
  - The UI shows the summary and Copy places the raw text on the clipboard.
- **Effort:** S–M. **Note:** build this first in the Reading section. The same raw-fetch primitive is needed by print fidelity, .eml/mbox export, forward-as-attachment and dragging threads into the composer.

<a id="clip-very-long-messages-with-a-show-full-message-popout-to-keep-scrolling-fast"></a>

### Clip very long messages with a "show full message" popout to keep scrolling fast

*Roadmap: P1 · Reading*

- **Why:** Huge newsletters, log digests and machine-generated mail freeze the reader while they are sanitized and laid out, and they create a scroll-inside-scroll trap.
- **Competitors:**
  - **Mailspring:** `MessageBodyProcessor.MAX_DISPLAY_LENGTH = 300000` characters (`app/src/flux/stores/message-body-processor.ts:14`). It is applied to the raw body before sanitizing (`:155-201`).
    - The reader shows `[Message Clipped - Show All]` (`internal_packages/message-list/lib/message-item-body.tsx:213`).
    - Show All writes the unclipped, sanitized body to a temp file and opens it in an 800×600 window with JavaScript disabled (`:110-172`).
    - Fixes: 1.10.6 (link broken), 1.17 (encoding: meta charset), 1.26 (Show All now sanitizes and blocks remote images like the reader).
  - **Gmail:** clips at about 102 KB with "[Message clipped] View entire message".
- **Alice today:**
  - Text parts over 4 MB are not downloaded at all; a notice is appended instead (`crates/alice-core/src/mime.rs:14`, `providers.rs:379-382`).
  - The HTML frame height is clamped to 12,000 px and longer mail scrolls inside the frame (`MessageView.tsx:110-111`).
  - `emailDocument` parses the HTML twice when it has quoted history (`emailHtml.ts:36-48`).
  - Plain text renders every paragraph (`MessageBody.tsx:5`).
- **Plan:**
  1. **Threshold.** In `emailHtml.ts`, add `const CLIP = 300_000` (Mailspring's value). `emailDocument(html, …, {full})` slices `html` to `CLIP` characters before parsing when `!full`, and returns `clipped: true`. The HTML parser repairs a cut tag.
  2. **Plain text.** `splitQuoted`/`MessageBody` get the same clip.
  3. **Reader controls.** `MessageView.tsx` shows a `html-controls` bar ("This message is long. Show full message") below the frame when clipped.
  4. **Show full message.** Open a reader window: the same hidden-capability auxiliary `WebviewWindow` as the print plan, holding the full sanitized document with the same remote-image decision and CSP. This keeps the main reader fast and gives a real window to scroll.
  5. **Height cap.** Keep the 12,000 px clamp; clipped mail rarely reaches it.
- **Pitfalls:**
  - **Units:** the limit counts UTF-16 code units, not bytes. CJK mail clips later in bytes, which is fine.
  - **Quote folding:** a short reply that quotes a huge thread will be cut inside its folded quote. Show the "clipped" bar only when the cut falls in the visible part, or when "Show earlier messages" is expanded. Otherwise every long reply chain looks clipped.
  - **Search:** must still index the full text body. It does, because search uses the stored body, not the rendered one.
- **Done when:**
  - A `scripts/test-html.mjs` case: 500 KB of HTML gives `clipped: true` and a document under the limit; `{full:true}` gives the whole message.
  - Manual: opening a 2 MB newsletter does not freeze the reader. Show full opens a window with all content and remote images still blocked.
- **Effort:** S (S–M including the popout window). Shares the window with "Print and Save as PDF".

<a id="dark-mode-body-rendering-option-keep-light-card-by-default-optional-smart-invers"></a>

### Dark-mode body rendering option: keep light card by default, optional smart inversion

*Roadmap: P1 · Reading*

- **Why:** In dark theme a white card is safe, but bright at night. Plain personal mail with no styling has no reason to be white.
- **Competitors:**
  - **Mailspring setting:** `core.reading.emailRenderMode` (`'light'`, the default, or `'dark'`, `app/src/config-schema.ts:177-183`), with a light-bulb toggle in the subject line (1.23, #2737).
  - **Mailspring "dark" mode:** a full CSS inversion, `body { filter: invert(100%) hue-rotate(180deg) }`, with images inverted back (`internal_packages/message-list/lib/email-frame-styles-store.ts:45-58`).
  - **Mailspring "light" mode in a dark theme** (the interesting part, `email-color-detection.ts:94-125`):
    - If any element paints a background, or any text colour has contrast below 1.5 against the dark app background, the email gets a white page.
    - Otherwise it renders transparent on the theme background, with near-black text (all channels ≤ 32) rewritten to the theme text colour.
  - **Mailspring changelog:**
    - 1.24: legacy theme inversion removed; plain text no longer inverted.
    - 1.24.1: explicit black text in Apple Mail/iCloud signatures shown white on dark instead of forcing a white page.
    - 1.25: print window no longer inverted.
  - **Thunderbird:** DarkReader (`mail/base/content/modules/DarkReader.mjs`, local copy `scratchpad/tb/DarkReader.mjs`) does not invert.
    - It skips mail that declares dark support.
    - It strips `bgcolor`/`text` attributes.
    - It removes inline backgrounds with luminance above 200 or contrast below 3.5, removes dark text colours once the background is gone, and drops gradients and blend modes.
  - **Other clients:** Apple Mail only darkens mail that declares `color-scheme: light dark`. Outlook fully or partly inverts and has a per-message sun toggle.
- **Spec:** CSS Color Adjustment (`color-scheme`, `prefers-color-scheme` inside iframes follows the frame's used color-scheme). Litmus dark-mode client survey (https://www.litmus.com/blog/the-ultimate-guide-to-dark-mode-for-email-marketers).
- **Alice today:** "Keep light card by default" is already done:
  - In dark theme the frame gets a white rounded card with 20 px padding (`apps/desktop/src/styles.css:270-271`).
  - The frame document forces `color:#222` (`emailHtml.ts:80`).
  - Plain text follows the app theme.
  - There is no adaptive mode or per-message toggle.
- **Plan:**
  1. **Preference.** Add `message_theme: 'light' | 'adaptive'` to `Preferences` (`crates/alice-core/src/model.rs`, `apps/desktop/src/api.ts:46-47`), default `light`, shown under Settings → Reading only when the theme is dark.
  2. **`adaptDark(doc, themeText, themeBackground)` in `emailHtml.ts`**, run from the frame `onLoad` (the frame is same-origin, so `contentDocument` is accessible, `MessageView.tsx:106-109`). Port Mailspring's light-mode test:
     - Any element with a background colour or image, or any coloured text with contrast below 1.5 against the dark background → keep the white card.
     - Otherwise, set the frame body background to transparent and colour to the theme text colour, and rewrite near-black inline colours to `inherit`.
     - Mail that declares `color-scheme: … dark` or has a `prefers-color-scheme: dark` rule → set the frame's `color-scheme: dark` and let its own CSS run.
     - Optionally add Thunderbird's stripping pass behind the same setting later. Do not ship full CSS inversion: it shifts brand colours and photos in CSS backgrounds.
  3. **Per-message toggle.** A "View in light" sun button in `html-controls` for the current message, held in component state.
  4. **Plain text** is unchanged; it already follows the theme.
  5. **Print** always uses the light document (see the print plan).
- **Pitfalls:**
  - **Hidden text:** transparent elements (alpha 0) are hidden preheader text. Ignore them, as Mailspring does.
  - **Transparent logos:** PNGs with dark ink vanish on a dark background. If an `<img>` is the only content, keep the card.
  - **Re-run on image load:** running `adaptDark` again after "Load images" must not flip the layout unexpectedly. Decide once per message per images state.
  - **Contrast:** the result must meet WCAG 1.4.3 (4.5:1) for normal text.
- **Done when:**
  - `scripts/test-html.mjs` (jsdom resolves inline styles) checks:
    - a plain `<p>` mail with `color:#000` becomes transparent with inherited colour;
    - a mail with `<td bgcolor=#fff>` keeps the card;
    - a mail declaring `color-scheme: light dark` gets `color-scheme: dark`.
  - Manual: compare 20 real newsletters and personal mails in dark theme.
- **Effort:** M (S for the preference plus transparent-when-safe). Depends on nothing.

## P1 → Accounts and setup

<a id="autodiscovery-thunderbird-ispdb-autoconfig-mx-lookup-well-known"></a>

### Autodiscovery: Thunderbird ISPDB / autoconfig, MX lookup, `.well-known`

*Roadmap: P1 · Accounts and setup*

- **Why:** Today a non-Gmail/Outlook user must know four hostnames, two ports and the TLS mode before Alice will connect. Most switchers only know their address and password.
- **Competitors:** Mailspring (`repos/mailspring/app/internal_packages/onboarding/lib/onboarding-helpers.ts:107-224`, `expandAccountWithCommonSettings`) works in this order:
  1. It always runs `dns.resolveMx(domain)` (`:71-82`); errors resolve to an empty list.
  2. It matches the domain or MX host against a bundled mailcore2 `providers.json` (`mailcore-provider-settings.json`, 34 entries with `domain-match`/`mx-match` regexes, `:121-159`). This is the 1.1.3 change: an MX of `*.google.com` skips the IMAP form.
  3. It looks up a bundled, ISPDB-derived table (`mailspring-provider-settings.json`, 1064 keys incl. aliases, `:84-90`).
  4. For password accounts only, it fetches `https://autoconfig.<domain>/mail/config-v1.1.xml` and then `https://<domain>/.well-known/autoconfig/mail/config-v1.1.xml` (`TryThunderbirdAutoconfig`, `:445-562`, added in 1.16 #2493).
  5. Otherwise it guesses `imap.<domain>`/`smtp.<domain>`, with port and security inference (`:171-204`).

  It never queries the live ISPDB (`autoconfig.thunderbird.net`) or SRV records, and mailsync does no discovery. Later fixes:
  - 1.1.5: alias domains such as `hotmail.it`.
  - 1.14: database refresh.
  - 1.25: autoconfig no longer overrides Mailspring's own provider entries; `proton.me` was not recognized before (#2886).
  - 1.26: OAuth accounts always use the provider's servers (#2907).

  Users asked Mailspring to adopt Thunderbird's algorithm (forum #805).

  Thunderbird runs the full chain: local, ISP, `.well-known`, ISPDB, MX, ISPDB on the MX domain, Exchange Autodiscover, then guessing with live probes. Apple Mail and Spark use their own server-side databases.
- **Spec:**
  - IETF draft-ietf-mailmaint-autoconfig-06 (https://www.ietf.org/archive/id/draft-ietf-mailmaint-autoconfig-06.html; parked WG document, replaces draft-bucksch-autoconfig):
    - §4.1 ISP URLs (the `?emailaddress=` form is required; `.well-known` is optional).
    - §4.2 ISPDB.
    - §4.3 MX: only the highest-priority MX, base domain via the Public Suffix List.
    - §4.5 Autodiscover, SRV and guessing are lower priority.
    - §5.1 show the configuration and the second-level domains for confirmation.
    - §7.2/§7.3 DNS- and HTTP-derived configurations MUST be explicitly confirmed by the user.
    - §8.2 SRV deliberately not used.
  - XML format: §3.7–3.8 and https://wiki.mozilla.org/Thunderbird:Autoconfiguration:ConfigFileFormat.
    - `socketType` is SSL / STARTTLS / plain.
    - Placeholders `%EMAILADDRESS%`, `%EMAILLOCALPART%`, `%EMAILDOMAIN%`.
    - `authentication` is password-cleartext, OAuth2, etc.
    - §3.7.3: if STARTTLS fails, the client MUST disconnect without authenticating.
  - ISPDB: `https://autoconfig.thunderbird.net/v1.1/<domain>` answers directly. Thunderbird's default `live.thunderbird.net` redirects there and returns 403 to non-browser User-Agents. The draft's alternative is `https://v1.ispdb.net/`. No published usage policy.
  - Thunderbird's actual order (local copy `scratchpad/tb/FindConfig.mjs:44-80`): `fromDisk`, `fromISP`, `fromDB`, `forMX` run in parallel and the highest-priority success wins; Exchange Autodiscover runs alongside; `GuessConfig` probes 993/143 and 465/587 only after all of them fail.
  - RFC 6186 §3–4 (SRV `_imaps._tcp`/`_imap._tcp`/`_submission._tcp`; a target of `.` means unavailable; §6: without DNSSEC the target must be inside the queried domain or the user must confirm). RFC 8314 §5.1 adds `_submissions._tcp`.
  - Microsoft Autodiscover v2 (`autodiscover-s.outlook.com/autodiscover/autodiscover.json?Email=…&Protocol=…`) cannot return IMAP or SMTP (`Protocol_UnsupportedProtocol`). Its only use for Alice would be detecting "this is Exchange Online". MX does that more cheaply.
- **Alice today:**
  - Hostnames for Gmail and Outlook are hardcoded in `connect()` (`crates/alice-core/src/daemon.rs:724-731`).
  - IMAP requires typed hosts (`apps/desktop/src/AddAccount.tsx:48`).
  - There is no DNS library: reqwest is HTTP-only (`Cargo.toml:41`).
  - `quick-xml 0.42` is already a dependency (`crates/alice-core/Cargo.toml:34`) and can parse `config-v1.1.xml`.
- **Plan:**
  1. Add a new module `crates/alice-core/src/autoconfig.rs` with `pub async fn discover(email) -> Vec<Candidate>`. A `Candidate` holds IMAP and SMTP `{host, port, security, username}`, plus `auth: Password | OAuth(Provider)`, `source`, and an optional `preset` id. Run the steps concurrently with a 5 s timeout each, then rank the results in this order:
     - built-in presets (next item);
     - ISP `https://autoconfig.<domain>/mail/config-v1.1.xml?emailaddress=<email>`;
     - `https://<domain>/.well-known/autoconfig/mail/config-v1.1.xml`;
     - ISPDB `https://autoconfig.thunderbird.net/v1.1/<domain>`;
     - MX lookup, then presets and ISPDB keyed by the MX host's registrable domain (e.g. `aspmx.l.google.com` → Gmail OAuth; `*.mail.protection.outlook.com` → Microsoft 365 OAuth);
     - RFC 6186 SRV (`_imaps._tcp`, `_imap._tcp`, `_submissions._tcp`, `_submission._tcp`);
     - guesses `imap.`/`mail.`/`smtp.` + domain.
  2. DNS: add `hickory-resolver` (new dependency) and use the system configuration, so split-horizon corporate DNS works. DNS-over-HTTPS through reqwest would avoid the dependency but breaks internal MX/SRV and leaks the domain to a third party. Parse XML with `quick-xml` (already a dependency).
  3. Add `GET /v1/autoconfig?email=` to `daemon.rs` and document it in `docs/api.md`. It returns candidates with no credentials involved.
  4. Change `AddAccount.tsx` to start with "Email + Continue":
     - Call `/v1/autoconfig`.
     - An OAuth match switches to the Gmail/Outlook flow.
     - A password match shows the discovered servers read-only, with an "Edit server settings" disclosure and the preset's tip.
     - When nothing is found, open the manual form prefilled with the best guess.
  5. `connect()` keeps validating before saving: IMAP login + LIST as now, plus SMTP auth (see the STARTTLS item).
  6. The CLI gets `alice accounts add --email x --provider auto`.
- **Pitfalls:**
  - Plain-HTTP autoconfig can be spoofed to steer the password to an attacker. Fetch over HTTPS only, or require explicit confirmation of hosts outside the user's domain. Never send the password during discovery.
  - ISPDB returns `%EMAILLOCALPART%` usernames for some ISPs (e.g. Yandex in Mailspring's table). Keep a `username` field (see the STARTTLS item).
  - Convert IDN domains to punycode before DNS lookup.
  - Respect Mailspring's 1.25 lesson: presets win over autoconfig.
  - Microsoft 365 custom domains are discovered only via MX. Gmail Workspace domains also need MX, otherwise they fall into the IMAP form and hit the Gmail-on-IMAP warning that Mailspring shows (`create-page-for-form.tsx:144-163`).
  - Cache negative results per domain for the session.
- **Done when:**
  - Unit tests parse three ISPDB XML fixtures: SSL, STARTTLS, and `%EMAILLOCALPART%`.
  - An axum mock-server test checks the order: ISP 404 → `.well-known` 404 → ISPDB hit.
  - MX mapping tests: `google.com` → Gmail and `outlook.com` protection hosts → Outlook.
  - A manual run over 15 real domains (gmx.de, web.de, posteo.de, mailbox.org, a Workspace and an M365 custom domain) prefills correct settings.
- **Effort:** L. Depends on "Presets with setup tips" and "IMAP STARTTLS on port 143, and custom ports" (the security and username fields).

<a id="presets-with-setup-tips-icloud-yahoo-aol-fastmail-zoho-gmx-yandex-proton-bridge"></a>

### Presets with setup tips: iCloud, Yahoo, AOL, Fastmail, Zoho, GMX, Yandex, Proton Bridge, Outlook.com, O365

*Roadmap: P1 · Accounts and setup*

- **Why:** Most of these providers need an app password or a toggle that users don't know about. A tile with the right servers and a one-line tip removes the most common setup failure.
- **Competitors:** Mailspring's picker (`internal_packages/onboarding/lib/account-providers.tsx:3-169`) has 9 tiles: Gmail, Office 365, Yahoo, Outlook.com, iCloud, FastMail, GMX, Yandex, IMAP. Each carries a note and a help link:
  - Yahoo/iCloud/FastMail: create an app password.
  - GMX/Yandex: enable IMAP first.
  - O365: admin must enable IMAP and Authenticated SMTP.
  - Outlook.com: enable IMAP.

  There is no Proton tile. Proton Bridge is a domain preset (`protonmail`, `pm.me`, `proton.me`) with 127.0.0.1:1143 STARTTLS / 127.0.0.1:1025 STARTTLS, `allow_insecure_ssl: true` and `container_folder: "Folders"` (`mailspring-provider-settings.json` @4688/5029/5032). History:
  - 1.4.0 (`ms.md:1952-1954`) added the Yandex/GMX presets and tips.
  - 1.9.2 (`ms.md:1234`) added the Bridge integration.
  - 1.5.2 appends the provider tip to "View Log" on failure.
  - 1.25 fixed Proton losing its container folder.

  Apple Mail and Thunderbird have no tips; Spark shows a provider grid with similar notes.
- **Alice today:**
  - Three choices: Gmail, Outlook, IMAP/SMTP (`apps/desktop/src/AddAccount.tsx:42`).
  - Guide text covers only the BYO-OAuth steps and a generic IMAP note (`:43-44`).
  - Login always uses the email address as the username (`crates/alice-core/src/providers.rs:63`).
- **Plan:**
  1. Add one shared table, `crates/alice-core/src/presets.rs` (a Rust const array, served at `GET /v1/presets` so desktop, CLI and TUI share it). Each entry has id, name, domains, mx_suffixes, IMAP and SMTP host/port/security, username format (`email` or `local`), `auth` (`oauth_google` | `oauth_microsoft` | `app_password` | `password`), a help URL, and a tip key.
  2. Starting values:

     | Provider | IMAP | SMTP | Username | Auth / tip |
     |---|---|---|---|---|
     | iCloud | `imap.mail.me.com:993` TLS | `smtp.mail.me.com:587` STARTTLS | email | app-specific password from account.apple.com |
     | Yahoo | `imap.mail.yahoo.com:993` | `smtp.mail.yahoo.com:465` | email | app password |
     | AOL | `imap.aol.com:993` | `smtp.aol.com:465` | email | app password |
     | Fastmail | `imap.fastmail.com:993` | `smtp.fastmail.com:465` | email | app password with IMAP/SMTP access |
     | Zoho | `imap.zoho.<region>:993` | `smtp.zoho.<region>:465` | email | region select (com/eu/in/com.au/jp); `imappro.zoho.*` for org domains; "enable IMAP access" |
     | GMX | `imap.gmx.<com|net>:993` | `mail.gmx.<com|net>:587` STARTTLS | email | enable IMAP in web settings |
     | Yandex | `imap.yandex.com:993` | `smtp.yandex.com:465` | email | enable IMAP + app password |
     | Proton Bridge | `127.0.0.1:1143` STARTTLS | `127.0.0.1:1025` STARTTLS | email | Bridge-generated password; trust Bridge certificate |
     | Outlook.com, O365 | Microsoft OAuth (existing) | | | |

  3. Show a provider grid in `AddAccount.tsx`. A domain match from the email field auto-selects a tile. The tip appears in the existing `setup-guide` `<details>`.
  4. Add translations for every tip in `apps/desktop/src/locales/*/settings.json`.
  5. On auth failure, append the preset's tip to the error (Mailspring 1.5.2 behaviour).
- **Pitfalls:**
  - **Proton Bridge** is the most-upvoted provider request in Mailspring's tracker (GitHub #429, 45 👍). It uses a self-signed certificate for 127.0.0.1, so verification fails. Offer "Trust this certificate" by fetching the leaf certificate, showing its SHA-256 fingerprint, and storing it per account. RFC 8314 §5.4 allows pinning at setup if it is per account and revocable. Implement it as a pinning verifier after the rustls move (STARTTLS item), or via `TlsConnector::builder().add_root_certificate` with `native-tls`.

    Mailspring's "Allow insecure SSL" does **not** bypass certificate validation. It only lowers the OpenSSL security level (`repos/mailsync/MailSync/MailUtils.cpp:865-875`), and its advice text tells users to install the certificate in the OS trust store.

    Never ship a global "accept invalid certs" switch.
  - Zoho data-centre mismatch gives "authentication failed", not "wrong server".
  - GMX `.com` and `.net` are different services.
  - Yandex's IMAP host varies by region (`.ru`/`.com`).
  - iCloud custom domains (iCloud+) need MX matching (`mx01.mail.icloud.com`).
  - Mailspring disables QRESYNC for `imap.mail.me.com` (`MailUtils.cpp:877-884`). Note this if Alice adopts QRESYNC.
- **Done when:**
  - A unit test asserts every preset has a valid host, a port in {993,143} / {465,587,1025}, and a security value consistent with the port.
  - A domain→preset table test covers `me.com`, `ymail.com`, `fastmail.fm`, `proton.me`, `pm.me`.
  - A Bridge fixture test: a self-signed Dovecot with a pinned fingerprint connects, and the same server with another certificate is refused.
- **Effort:** M. Depends on "IMAP STARTTLS on port 143, and custom ports" (Bridge, security field, username).

<a id="imap-starttls-on-port-143-and-custom-ports"></a>

### IMAP STARTTLS on port 143, and custom ports

*Roadmap: P1 · Accounts and setup*

- **Why:**
  - Self-hosted servers, Proton Bridge, DavMail and many ISP servers only offer IMAP on 143 with STARTTLS.
  - Alice cannot connect to any of them.
  - SMTP is limited to 587/465 in the UI.
- **Competitors:**
  - **Mailspring UI** (`internal_packages/onboarding/lib/page-account-settings-imap.tsx:7-8, 84-201`):
    - Security select: "SSL / TLS", "STARTTLS", "None".
    - IMAP ports: [143, 993]. SMTP ports: [25, 465, 587] plus "Custom".
    - Picking 143 sets security to **None**, not STARTTLS, which is a weak default.
  - **mailsync** maps "STARTTLS" to `ConnectionTypeStartTLS` and anything else except "SSL / TLS" to cleartext (`MailUtils.cpp:858-864, 907-913`).
  - **Changelog:**
    - 1.10.0 (`ms.md:1225`): SMTP defaults to 465 per RFC 8314.
    - 1.19.1: custom port input fix.
    - 1.24 (#119): falls back to legacy TLS settings when the handshake is rejected.
  - **Thunderbird:** offers SSL/TLS, STARTTLS and None, with an explicit warning on None.
- **Spec:** RFC 8314 (local copy `scratchpad/rfc8314.txt`):
  - §3: implicit TLS recommended.
  - §3.3: support both 587 STARTTLS and 465.
  - §5: MUAs MUST implement TLS 1.2+ and MUST NOT treat an unvalidated certificate as confidential.
  - §5.2: the default minimum confidentiality MUST require certificate validation. No operation other than capability discovery or STARTTLS is allowed before TLS.
  - §5.4: pinning is allowed only at account setup, per account, and must be revocable.

  RFC 3501 §6.2.1 (STARTTLS; re-issue CAPABILITY after it). Autoconfig draft §3.7.3 (if STARTTLS fails, do not authenticate).
- **Alice today:**
  - **IMAP:** always implicit TLS. `TlsConnector::connect` runs immediately after TCP connect (`crates/alice-core/src/providers.rs:45`), so port 143 fails in the handshake. The port field is labelled "IMAP TLS port" (`AddAccount.tsx:48`).
  - **SMTP:** port 465 means implicit TLS; any other port means STARTTLS required (`providers.rs:214-218`). The UI offers only 587/465 (`AddAccount.tsx:48`), while the CLI accepts any port (`crates/alice/src/main.rs:236-241`). There is no cleartext path, which is good.
  - **Username:** always the email address (`providers.rs:53, 63, 227`).
  - **SMTP validation:** none at setup. `connect()` validates IMAP only (`daemon.rs:792-802`).
- **Plan:**
  1. In `model.rs`, give `Account` the fields `imap_security`, `smtp_security: Security { Tls, StartTls }` and `username: Option<String>`.
     - Use `#[serde(default)]` and derive the missing values on load: 993 → Tls, else StartTls; 465 → Tls, else StartTls. No migration is needed.
     - Mirror the fields in `ConnectAccount` (`model.rs:398-409`) and in `api.ts`.
  2. Make `providers::session()` branch on `imap_security`:
     - `StartTls`: `imap::Client::new(tcp)` → `read_greeting()` → require `STARTTLS` in CAPABILITY, otherwise bail with a new `mail.imap.starttls_unavailable` message (never fall back to cleartext) → `client.secure(host, &tls)`.
     - `secure()` is at `imap-2.4.1/src/client.rs:262`, and the `Session<TlsStream<TcpStream>>` type stays the same. Do not read a second greeting after the upgrade.
  3. Make `providers::deliver()` choose `SmtpTransport::relay` vs `starttls_relay` from `smtp_security`, not the port, and allow any port (25, 2525, 1025).
  4. Validate SMTP in `connect()`: call `transport.test_connection()`. In lettre 0.11 this connects, upgrades to TLS and authenticates (`lettre-0.11.23/src/transport/smtp/transport.rs:385-414`), so wrong SMTP credentials fail at setup instead of at first send, and no message is sent.
     - Mailspring instead sends a real "Mailspring SMTP Test Email" to the user (1.10.7, `mailcore2 MCSMTPSession.cpp:788-872`). That catches relay refusals but clutters the inbox.
     - Offer that only as an explicit "Send test message" button.
  5. Update the UI (`AddAccount.tsx`): add a Security select next to each port, and auto-fill the port when security changes unless it was edited. Add a "Username (if different from email)" field. Add CLI flags `--imap-security`, `--smtp-security`, `--username`.
  6. **Move mail TLS from `native-tls` to rustls.**
     - On macOS, `native-tls` uses Secure Transport, which cannot negotiate TLS 1.3 (Apple: "TLS clients using the SecureTransport APIs can't use TLS 1.3"; https://github.com/sfackler/rust-native-tls/issues/140). Alice therefore cannot reach TLS-1.3-only servers on macOS. Mailspring has the same failure, still open as #2507.
     - Everything needed is already in the dependency tree: `rustls 0.23` and `rustls-platform-verifier 0.7` come in via reqwest (`Cargo.lock`), and lettre 0.11 can use them through its `rustls` and `rustls-platform-verifier` features. The `imap` client is generic over `Read + Write` (`rustls::StreamOwned`). Only the `Session` type alias changes (`providers.rs:16`).
     - With rustls, certificate pinning for Bridge-style self-signed servers becomes a small custom `ServerCertVerifier` that accepts exactly the stored SHA-256. That is cleaner than `native-tls` `add_root_certificate`.
- **Pitfalls:**
  - **STARTTLS stripping:** treat STARTTLS absence or failure as fatal.
  - **Pre-TLS data:** `bufstream::into_inner` must discard bytes buffered before TLS (the CVE-2011-0411 command-injection class). Add a test where the fake server sends extra bytes after `OK Begin TLS`.
  - **LOGINDISABLED** before STARTTLS is normal. Re-check capabilities after the upgrade; the `imap` crate fetches them lazily.
  - **Changing security or port** of an existing account must not wipe cached mail unless the host changes. `connect()` wipes only on host, port or email change (`daemon.rs:813-818`); exclude the port from that check.
  - **DavMail and Bridge** use `127.0.0.1` with a self-signed certificate (see the Presets item).
  - **Error messages:** Mailspring's most-voted setup bugs were opaque SMTP codes:
    - "SMTP error 17" (GitHub #620, 20 👍) is libetpan `MAILSMTP_ERROR_AUTH_LOGIN`: credentials rejected, usually an app password needed or SMTP AUTH disabled.
    - "mailsmtp error 27" (#341) is `MAILSMTP_ERROR_SSL`: the security mode does not match the port, e.g. SSL on 587. The codes are from `repos/mailsync/Vendor/libetpan/src/low-level/smtp/mailsmtp_types.h:48-75`.

    Alice must say which side failed (IMAP vs SMTP) and what to change, e.g. "Port 587 expects STARTTLS". Never show a numeric code.
  - **Legacy TLS:** Mailspring 1.24 added a legacy-TLS fallback with advice (`MailUtils.cpp:789-834`). Do not copy the fallback (RFC 8314 §5.2), but copy the advice text approach.
- **Done when:**
  - Against the Dovecot fixture (`scripts/mail-fixture.sh`, CI `ci.yml:204`), an account on 143+STARTTLS and on 993+TLS both sync.
  - A fake-server unit test (pattern: `remote.rs` `ListStream` test) proves a 143 server without STARTTLS is refused and no LOGIN is sent.
  - A deserialization test shows old account JSON gets `Tls`/`StartTls` from its ports.
  - A wrong SMTP password fails `POST /v1/accounts` with an SMTP-specific message.
  - After the rustls move, a Dovecot fixture with `ssl_min_protocol = TLSv1.3` connects on macOS CI.
- **Effort:** S–M for STARTTLS, security fields and SMTP validation; M if the rustls move is done in the same change. No dependencies; do this first in this section.

<a id="verify-outlooko365-oauth-against-a-live-account-ship-a-built-in-microsoft-client"></a>

### Verify Outlook/O365 OAuth against a live account; ship a built-in Microsoft client id

*Roadmap: P1 · Accounts and setup*

- **Why:**
  - Today every Outlook user must register their own Entra app (`AddAccount.tsx:44`), which almost nobody will do.
  - Basic auth is gone for Outlook.com and Exchange Online, so OAuth is the only way in.
- **Competitors:** Mailspring:
  - Ships client id `8787a430-6eee-41e1-b914-681d90d35625` (override via env `MS_O365_CLIENT_ID`). It is a public client with PKCE S256 and no secret (`internal_packages/onboarding/lib/onboarding-constants.ts:38-79`).
  - Uses tenant `common`, `prompt=select_account` and redirect `http://localhost:12141/desktop` (`onboarding-helpers.ts:395-407`).
  - Requests these scopes at consent: openid, email, profile, user.read, offline_access, Contacts/Calendars ReadWrite(.Shared), `https://outlook.office.com/IMAP.AccessAsUser.All`, `https://outlook.office.com/SMTP.Send`. The code is exchanged for a Graph token, and the email comes from Graph `/me` → `mail || userPrincipalName` (`:282-340`).
  - mailsync refreshes with the outlook.office.com scopes only (`repos/mailsync/MailSync/NetworkRequestUtils.cpp:63-80`), caches the access token until 60 s before expiry, and persists rotated refresh tokens (`XOAuth2TokenManager.cpp:90-144`).
  - Changelog: 1.10.3/1.10.4 (O365 fixes, account chooser), 1.14 (Outlook.com moved to OAuth), 1.20 (`+` in codes), 1.22 (email resolution), 1.25 (offline error instead of "unable to connect").
- **Spec:**
  - Microsoft, "Authenticate an IMAP, POP or SMTP connection using OAuth" (https://learn.microsoft.com/en-us/exchange/client-developer/legacy-protocols/how-to-authenticate-an-imap-pop-smtp-application-by-using-oauth):
    - Scopes: `https://outlook.office.com/IMAP.AccessAsUser.All`, `…/SMTP.Send`, `offline_access`.
    - Works for Microsoft 365 and Outlook.com.
    - SASL XOAUTH2 is `user=…^Aauth=Bearer …^A^A`.
  - Auth code flow with PKCE, public clients without a secret, rotating refresh tokens: https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-auth-code-flow.
  - Redirect rules: https://learn.microsoft.com/en-us/entra/identity-platform/reply-url
    - The port is ignored only for `localhost`.
    - An http `127.0.0.1` URI must be added through the manifest.
    - `[::1]` is not supported.
    - Personal-account apps cannot use query strings in the redirect.
  - Publisher verification: https://learn.microsoft.com/en-us/entra/identity-platform/publisher-verification-overview.
  - SMTP AUTH (https://learn.microsoft.com/en-us/exchange/clients-and-mobile-in-exchange-online/authenticated-client-smtp-submission): `Set-TransportConfig -SmtpClientAuthenticationDisabled`, per-mailbox `Set-CASMailbox`. Security defaults turn it off.
  - Basic-auth SMTP AUTH is disabled by default for existing tenants from the end of December 2026 (Exchange Team, 27 Jan 2026). OAuth is the only durable path.
  - Thunderbird ships client id `9e5f94bc-e8a4-4e73-b8be-63364c29d753` with the same three scopes, an external browser and a `127.0.0.1` loopback (local copy `scratchpad/tb/OAuth2Providers.mjs:30-35, 315-324`).
- **Alice today:**
  - **Client id:** BYO, from a form field or env `ALICE_MICROSOFT_CLIENT_ID` (`daemon.rs:766-776`).
  - **Endpoints:** `/common` authorize and token (`crates/alice-core/src/oauth.rs:56-59`).
  - **Scopes:** `offline_access …/IMAP.AccessAsUser.All …/SMTP.Send` (`oauth.rs:36`); the Graph token is redeemed separately (`:379-397`).
  - **Redirect and listener:** a fixed redirect `http://127.0.0.1:4816/callback` (`:26`) and a fixed listener port (`:220`), so a busy port gives `mail.oauth.port_busy`.
  - **Flow details:** PKCE S256 and `login_hint` (`:224-238`); refresh tokens rotate under a mutex (`:342-344`).
  - **XOAUTH2:** `user=account.email` (`providers.rs:25`).
  - **Live testing:** never tested live (README "Current boundaries").
- **Plan:**
  1. **Register "Alice" in Entra** as a public client:
     - Audience "Accounts in any organizational directory and personal Microsoft accounts".
     - Platform "Mobile and desktop applications".
     - Delegated permissions: `IMAP.AccessAsUser.All`, `SMTP.Send`, `offline_access`, plus the Graph `Contacts.Read` / `Calendars.ReadWrite` already used.
     - Complete **publisher verification**; many tenants block user consent to unverified multi-tenant apps.
  2. **Redirect:** switch to an ephemeral loopback port.
     - Bind `127.0.0.1:0` and send `redirect_uri=http://localhost:<port>/callback`. Use the same string in the token request.
     - Microsoft ignores the port only for `localhost` URIs. An `http://127.0.0.1` URI must be added through the manifest (`replyUrlsWithType`), and it is unclear whether the port is ignored there (https://learn.microsoft.com/en-us/entra/identity-platform/reply-url, "Localhost exceptions" and "Prefer 127.0.0.1 over localhost").
     - Since the browser may resolve `localhost` to `::1`, also bind `[::1]:<same port>` or fall back. Google desktop clients accept any loopback port, so one code path covers both.
  3. **Embed the client id:** add `const MICROSOFT_CLIENT_ID` in `oauth.rs`, used when `secret.client_id` is empty.
     - Move the client-id field under an "Advanced" disclosure in `AddAccount.tsx`.
     - Drop the six-step BYO guide for Outlook.
  4. **Map errors** in `oauth.rs` `finish()`/`redeem()`: parse `error`/`error_codes`/`error_description` from the token response instead of the blanket `mail.oauth.exchange_failed`.
     - AADSTS65001 or 90094: needs admin consent. Show the `https://login.microsoftonline.com/organizations/adminconsent?client_id=…` link.
     - AADSTS50020: wrong tenant.
     - AADSTS700016: app not found in tenant.
     - IMAP `AUTHENTICATE failed`: IMAP disabled for the mailbox.
     - SMTP 535 5.7.139: SMTP AUTH disabled in the tenant.
  5. **Live checklist** for outlook.com, hotmail.com, an M365 Business tenant with defaults, a tenant with SMTP AUTH disabled, and a tenant requiring admin consent. Record the results in `docs/`.
- **Pitfalls:**
  - **Mailspring's track record:** Outlook/O365 setup failures are its most common account bugs: GitHub #1844 "not Adding Outlook Account" (18 👍), #1118, #1208 "Can't add office365 account", and #1811 "Office365 IMAP does not login with correct app password". The last is basic auth being off, not a password problem. Every Microsoft failure needs a specific message, and the IMAP form must refuse Microsoft hosts with a password, pointing to OAuth instead.
  - **SMTP AUTH:** disabled by default in new tenants (`SmtpClientAuthenticationDisabled`), so receiving works and sending fails. Detect this and explain it, as Mailspring's O365 tile note does.
  - **Token scope:** a token can carry only one resource's scopes. Keep the separate Graph redeem.
  - **Multiple accounts:** `prompt=select_account` helps when several Microsoft accounts are signed in to the browser (Mailspring 1.10.4).
  - **Embedded id is not secret:** an embedded client id can be copied, which is acceptable for a public client. Never embed a client secret. Mailspring encrypts its Google secret in the bundle, which only obscures it.
  - **Sovereign clouds:** GCC High and 21Vianet use other endpoints. Out of scope; say so.
- **Done when:**
  - Unit tests:
    - The authorize URL uses the built-in id when none is given.
    - The `redirect_uri` is identical in the authorize and token requests and contains the bound port.
    - The AADSTS codes map to localized messages.
  - The live checklist above passes, and README "untested live" is removed.
- **Effort:** M, plus publisher-verification lead time. Unblocks "Office 365 shared mailboxes". Pairs with "Re-authenticate flow when a token is revoked, with a clear error".

<a id="office-365-shared-mailboxes"></a>

### Office 365 shared mailboxes

*Roadmap: P1 · Accounts and setup*

- **Why:** Team inboxes (support@, sales@) in Microsoft 365 are shared mailboxes with no password of their own. Without this, those users cannot switch.
- **Competitors:** Mailspring 1.23.0 (`ms.md:254`, #2763):
  - **Entry point:** Preferences → Accounts → "Add Shared Mailbox…", shown only on Office 365 accounts (`internal_packages/preferences/lib/tabs/preferences-account-details.tsx:171-178, 399-403`). It is not offered in first-run setup.
  - **UI note** (`internal_packages/onboarding/lib/page-account-settings-o365.tsx:61-124`): the user signs in with their own account, and needs "Full Access" to read and "Send As" to send; without Send As the mailbox is read-only.
  - **Mechanism** (`onboarding-helpers.ts:342-361`):
    - Email and `imap_username` are the shared address. The XOAUTH2 IMAP identity is the shared mailbox, using the owner's token.
    - `smtp_username` is the owner, who sends as the shared address.
    - `smtp_verification: 'login'`, so no test email is sent.
    - The account id is derived from the shared address, so the shared mailbox is a separate account.
  - **Other clients:** Outlook adds shared mailboxes automatically via automapping. Thunderbird needs the same manual XOAUTH2 trick.
- **Spec:** Microsoft's OAuth IMAP page, "Shared mailbox access": get the token as the user, then put the shared mailbox address in the `user=` field of the XOAUTH2 string (same URL as the previous item). Exchange permissions: `Add-MailboxPermission -AccessRights FullAccess` for reading and `Add-RecipientPermission -AccessRights SendAs` for sending.
- **Alice today:**
  - XOAUTH2 `user=` and the SMTP credentials both use `account.email` (`providers.rs:25, 53, 227`).
  - `login_hint` is `account.email` (`oauth.rs:237`).
  - If the shared address is entered as the email, the user is asked to sign in as the mailbox, which has no password. Nothing exists for this case.
- **Plan:**
  1. Reuse the new `username` field from the STARTTLS item: for shared mailboxes `email` is the shared address and `username` is the signing-in owner.
     - `oauth::begin` sends `login_hint=username`.
     - XOAUTH2 for IMAP uses `user=<shared address>`.
     - SMTP authenticates as the owner (`Credentials::new(username, token)`), with `From:` set to the shared address.
  2. Add `Account.shared_mailbox: bool` (serde default false). Shared accounts never request Graph integrations; that would need `*.Shared` scopes.
  3. **Daemon:** `connect()` accepts `{provider: outlook, shared_mailbox: true, email: shared, username: owner}`. Validation does IMAP login + LIST against the shared mailbox. For SMTP, use `test_connection()` (auth only) and say that Send As is verified on first send.
  4. **UI:**
     - Add an "Add shared mailbox" button on an Outlook account in Settings → Mail accounts.
     - The form prefills the owner and asks for the shared address, with Mailspring's permissions note translated.
     - The account list shows "Shared · via owner@".
  5. **Storage:** store a separate refresh token per shared account, i.e. a separate sign-in. This is simple and avoids cross-account token coupling.
- **Pitfalls:**
  - **Permission propagation:** Full Access and Send As can take up to an hour to apply after an admin grants them. Errors in that window look like auth failures, so say so.
  - **Sent copy:** with Send As, the sent copy lands in the owner's Sent folder unless the admin sets `MessageCopyForSendAsEnabled`. Alice caches its own sent receipt locally, so that receipt is unaffected, but server-side Sent may differ.
  - **Shared folders:** some tenants expose only some folders. Folder discovery must tolerate a missing Archive (`remote.rs:503-580` already does).
  - **Dedup:** `connect()` matches existing accounts by email (`daemon.rs:713-723`). A shared mailbox added twice by two owners must update, not duplicate.
- **Done when:**
  - A unit test shows the XOAUTH2 initial response for a shared account contains `user=shared@…` while the authorize URL `login_hint` is the owner.
  - A unit test shows the SMTP credentials use the owner.
  - A live test reads and sends from a real shared mailbox with Full Access + Send As.
- **Effort:** M. Depends on "Verify Outlook/O365 OAuth against a live account; ship a built-in Microsoft client id" and on the `username` field from "IMAP STARTTLS on port 143, and custom ports".

<a id="edit-an-existing-accounts-settings-reorder-accounts-per-account-color"></a>

### Edit an existing account's settings; reorder accounts; per-account color

*Roadmap: P1 · Accounts and setup*

- **Why:** A server or password change today means removing and re-adding the account. With several accounts there is no way to tell their mail apart in the unified inbox or to put the main one first.
- **Competitors:** Mailspring Preferences → Accounts (`internal_packages/preferences/lib/tabs/preferences-account-details.tsx:312-410`):
  - **Inline fields:** Account Label, Sender Name, Automatic CC/BCC, Aliases (with "Default for new messages"), and Account Color (`<input type=color>` + "Reset Account Color").
  - **Buttons:** "Manage Contacts", "Re-authenticate…" (Gmail) or "Update Connection Settings…" (others), "Add Shared Mailbox…" (O365), and "Rebuild Cache…".
  - **Connection settings** are not inline. They reopen onboarding prefilled with the account and its secrets (`:161-165`).
  - **Re-adding** keeps label, aliases and color (`app/src/flux/stores/account-store.ts:238-265`).
  - **Reorder:** drag in the list, `Actions.reorderAccount` (`account-store.ts:223-231`); 1.7.0, `ms.md:1532`.
  - **Color** (1.8.0, `ms.md:1324`) shows as:
    - a 4 px bar on every thread row (`app/src/components/account-color-bar.tsx`);
    - the sidebar section title colour;
    - a composer From tint;
    - an 8 px border in the account list.
  - Thunderbird edits everything inline in Account Settings. Apple Mail edits it in Settings → Accounts → Server Settings.
- **Alice today:**
  - **Settings → Mail accounts:** shows name, email and status, with Remove only (`apps/desktop/src/Settings.tsx:149-150`).
  - **Re-adding an account:** a `POST /v1/accounts` with the same email updates the account in place, keeping its id, folders and integrations (`daemon.rs:713-746`). It wipes cached mail if the host, port or email changed (`:813-818`).
  - **Account fields:** `Account` has no sender name, color or order (`model.rs:12-29`).
  - **Ordering:** `accounts()` orders by `rowid` (`store.rs:58`). `save_account` is an upsert, so the order is stable (`:75-80`).
  - **From header:** carries no display name (`providers.rs:195`).
- **Plan:**
  1. **Model:** add `Account.sender_name: Option<String>`, `color: Option<AccountColor>` and `position: i64`.
     - `AccountColor` is an enum of 8 palette tokens that pass contrast in both themes, not free hex.
     - Order `accounts()` by `json_extract(data,'$.position'), rowid`.
  2. **Daemon:**
     - Add `PATCH /v1/accounts/{id}` with a `deny_unknown_fields` `AccountPatch {name, sender_name, color}`. It saves under `state.hold(id)`, like `save_integrations` (`daemon.rs:920-944`).
     - Connection fields (hosts, ports, security, username, password) go through a refactored `validate_and_save()` extracted from `connect()`, so edits reuse its pause-sync + validate + wipe-if-host-changed logic.
     - Add `PUT /v1/accounts/order {ids:[…]}`.
     - Add `POST /v1/accounts/{id}/rebuild`, which calls `forget_other_mailboxes(id, &[])` (`sync_store.rs:302`) and then a CatchUp sync.
     - Document all of these in `docs/api.md`.
  3. **Desktop:** clicking an account row in `Settings.tsx` opens a detail view with these sections:
     - Identity: label, sender name, colour swatches.
     - Server: fields from the STARTTLS item, hidden for OAuth accounts.
     - Sign-in: Re-authenticate / change password, from the next item.
     - Maintenance: Rebuild cache, Remove.

     Reorder with "Move up/Move down" buttons; drag-only would fail keyboard users. Show the colour as a stripe in the sidebar account list (`App.tsx:357`) and on unified-inbox rows.
  4. **Sending:** in `providers::build_mail`, use `Mailbox::new(sender_name, email)`. Coordinate with the Composer section, which owns aliases and auto-CC/BCC.
  5. **CLI:** add `alice accounts edit <account> --name/--sender-name/--color` and `alice accounts order`.
- **Pitfalls:**
  - **Changing the email:** this makes a different account, so reject it in PATCH.
  - **Host changes:** warn before a host change because it re-downloads mail.
  - **Colour:** must not be the only cue; keep the account name in row labels and `aria-label`.
  - **Concurrent sync:** writes must take the account lock (`daemon.rs:804-821` pattern), or a finishing sync will overwrite them via `sync_store.rs:645-669`.
- **Done when:**
  - Daemon tests:
    - PATCH name/color persists and leaves `folders` and `last_synced` alone.
    - PATCH host revalidates and clears mailbox rows.
    - The order endpoint changes `GET /v1/accounts` order.
    - Rebuild empties and refills one account only.
  - UI: the colour appears in the sidebar and rows, and keyboard reordering works.
- **Effort:** M. Splits cleanly: label, sender name, color and order (S); connection editing and rebuild (S–M, after "IMAP STARTTLS on port 143, and custom ports").
- **Note:** "Automatic CC/BCC" and "Aliases" belong with the Composer identity items. They are listed here only because Mailspring puts them on this screen.

<a id="re-authenticate-flow-when-a-token-is-revoked-with-a-clear-error"></a>

### Re-authenticate flow when a token is revoked, with a clear error

*Roadmap: P1 · Accounts and setup*

- **Why:** Revoked Google/Microsoft grants and changed passwords are routine. Today the account silently shows a red dot, and the only fix is to re-add it from scratch.
- **Competitors:**
  - **mailsync:** maps `ErrorAuthentication` and refused OAuth refreshes (`invalid-xoauth2-resp`) to non-retryable exceptions (`repos/mailsync/MailSync/XOAuth2TokenManager.cpp:118-125`, `SyncException.cpp:45-63`).
  - **Client:** after the crash tracker sees repeated exits, it sets `syncState: 'invalid'` when stderr contains 401/403/`invalid_grant`/`ErrorAuthentication` (`app/src/flux/mailsync-bridge.ts:354-380`).
  - **UI:**
    - A notification bar: "Cannot authenticate with <email>" with "Check Again" and "Reconnect" (`internal_packages/notifications/lib/items/account-error-notif.tsx:71-133`).
    - A Preferences banner: "Mailspring can no longer authenticate with %@. The password or authentication may have changed." with Reconnect and "Error Details…" (last 3000 bytes of the log) (`preferences-account-details.tsx:266-310`).
    - Offline errors are kept separate ("Offline - Retrying…").
  - **Changelog:**
    - 1.7.6: re-auth no longer gets stuck.
    - 1.13.1: red accounts after password loss.
    - 1.24 Flatpak and 1.24.1 KWallet: secret-store changes forced re-auth or lost accounts.
    - 1.25: malformed token responses no longer crash.
- **Alice today:**
  - **Token refresh:** any non-2xx response, including 5xx and timeouts, becomes `mail.oauth.authorization_expired` (`oauth.rs:369-372`).
  - **IMAP auth failures:** become "IMAP authentication failed: {error}" (`providers.rs:69-74`).
  - **Storage:** both are stored as the free-text `account.sync_error` (`sync_store.rs:660-666`).
  - **UI:** an error dot with a tooltip (`App.tsx:357`) and "Needs attention" (`Settings.tsx:149`), with no action.
  - **Sync behaviour:** sync keeps retrying with the bad credentials on every cycle.
- **Plan:**
  1. Add `Account.attention: Option<Attention { Auth, Offline, Server, Certificate }>` next to `sync_error` (serde default), and set it in `sync_store.rs:660-666` from a typed error. Classify:
     - Token endpoint 400/401 with `error=invalid_grant` (Google "Token has been expired or revoked"; Microsoft AADSTS70008/50173/700082) → Auth.
     - Token endpoint 5xx, timeout or non-JSON response (captive portal, as in mailsync `:118-125`) → Offline.
     - IMAP `NO [AUTHENTICATIONFAILED]` (RFC 5530) or a LOGIN failure → Auth.
     - `[UNAVAILABLE]` → Server.
     - TLS verify failure → Certificate.
  2. Make `sync.rs` skip accounts in `Auth` until credentials change. Repeated bad logins lock out Yahoo, iCloud and corporate accounts. Offline errors keep backing off.
  3. Add `POST /v1/accounts/{id}/reauthenticate` to `daemon.rs`:
     - OAuth: `oauth::begin(existing account)` → `{flow_id, url}`, with `login_hint` and the existing integrations, reusing `track_flow`.
     - Password: `{password}` → validate IMAP+SMTP → `vault::save`.
     - Never call `forget_other_mailboxes`, and clear `attention` on success.
  4. **Desktop:**
     - A banner above the message list when any account is in `Auth`: "Sign-in for x@y.z has expired" + "Sign in again".
     - The same action on the sidebar account row and in the account detail view.
     - Show `Offline` as a quiet "Retrying" state, not an error.
     - One OS notification per transition into `Auth`, via `desktop.rs`.
  5. **CLI/TUI:** add `alice accounts reauth <account>`. The TUI status line names the account.
- **Pitfalls:**
  - **Google:** OAuth clients in "Testing" publishing status issue refresh tokens that expire after 7 days. This hits every BYO Google client, so detect it and explain.
  - **Microsoft:** revokes on password reset, conditional-access changes, and 90 days of inactivity.
  - **Secret store migrations:** these must never drop credentials silently (Mailspring 1.24.1 KWallet bug). Have `vault::load` failures map to Auth with a specific message.
  - **Mail sends:** sends in flight during an auth failure must keep their at-most-once guarantee.
- **Done when:**
  - Unit tests for the classifier:
    - `{"error":"invalid_grant"}` → Auth.
    - HTTP 503 → Offline.
    - IMAP `NO [AUTHENTICATIONFAILED]` → Auth.
  - A scheduler test shows an Auth account is not contacted until reauth.
  - A daemon test shows reauth keeps messages and folders and clears `attention`.
  - The UI shows the banner with an account fixture in `Auth`.
- **Effort:** M. Related: "Verify Outlook/O365 OAuth against a live account; ship a built-in Microsoft client id" (error mapping).

<a id="manual-folder-role-mapping"></a>

### Manual folder role mapping

*Roadmap: P1 · Accounts and setup · new*

- **Why:**
  - Servers without SPECIAL-USE, and localized or legacy folder names, make Alice pick the wrong Sent/Archive/Trash or no Spam folder.
  - Archive and Trash then move mail to the wrong place.
  - There is no way to correct this.
- **Competitors:**
  - Mailspring has Preferences → Folders per account. It maps Inbox/Sent/Drafts/Spam/Archive/Trash to any server folder and sets a "Default Container Folder" for its helper folders.
  - The IMAP onboarding page also has a "Custom Container Folder" field (`page-account-settings-imap.tsx:206-249`). The Proton presets set `container_folder: "Folders"`, and 1.25 fixed Proton losing it.
  - Thunderbird: Account Settings → Copies & Folders and Junk Settings.
  - Apple Mail: Mailbox → "Use This Mailbox For".
- **Spec:** RFC 6154 (SPECIAL-USE attributes).
- **Alice today:**
  - `Mailboxes {inbox, sent, archive, trash, spam}` (`crates/alice-core/src/model.rs:31-40`), with no Drafts role.
  - Discovered on every sync (`crates/alice-core/src/sync.rs:281`) by `discover_folders` (`crates/alice-core/src/remote.rs:503-580`): special-use attribute first, then the saved value or a name list.
  - The result overwrites `account.folders` (`sync_store.rs:657-658`), so any manual value would be replaced whenever a special-use attribute exists.
  - No UI.
- **Plan:**
  1. Add `Account.folder_overrides: FolderOverrides { sent, archive, trash, spam: Option<String> }` (serde default).
  2. In `discover_folders`, apply an override first when that mailbox exists and is selectable; otherwise log and fall back.
  3. Add `GET /v1/accounts/{id}/mailboxes`, returning the LIST names and attributes for the picker. Overrides are written through `PATCH /v1/accounts/{id}` from the edit-settings item.
  4. **UI:** a "Folders" section in the account detail view with four selects ("Automatic (Sent Messages)" plus every server folder).
  5. Skip Drafts and the container folder until server-side drafts or Snooze exist (YAGNI).
- **Pitfalls:**
  - **Changing Archive or Trash:** the previous mailbox's cached rows are forgotten by `forget_other_mailboxes` on the next sync. That is correct, but warn that the old folder's mail will no longer show under that role.
  - **Gmail:** keep Gmail's `\All` handling, and disallow overriding Gmail Archive to anything except All Mail.
- **Done when:**
  - Unit test: `discover_folders` with an override beats a `\Sent` attribute.
  - Unit test: an override naming a missing folder falls back.
  - A sync test shows the override survives two syncs.
- **Effort:** S–M. Depends on "Edit an existing account's settings; reorder accounts; per-account color" (PATCH endpoint and detail view).

## P2 → Reading

<a id="layout-list-above-message-vertical-stack-in-addition-to-current-layouts"></a>

### Layout: list above message (vertical stack) in addition to current layouts

*Roadmap: P2 · Reading · done*

- **Note:** Already done; resizable split is missing. Alice ships three layouts:
  - `MailLayout { Single, Horizontal, Vertical }` (`crates/alice-core/src/model.rs:240`).
  - "Horizontal split" is described as "List above the reader" (`apps/desktop/src/Settings.tsx:137`). Its grid puts the mailbox in row 2 and the reader in row 3 (`apps/desktop/src/styles.css:759-766`).
  - Windows narrower than the breakpoint fall back to a single pane (`App.tsx:15`, `Settings.tsx:137` note).

  Gap against Mailspring: its "Vertical Reading Pane" (`core.workspace.mode = 'splitVertical'`, 1.9.0 #2243) puts the list in a `ResizableRegion` with a draggable bottom edge (min 100 px, default 400, persisted via `AppEnv.storeThreadListVerticalHeight`; `internal_packages/thread-list/lib/thread-list-vertical.tsx`). It also offers the layouts in the View menu (`menus/darwin.js:98-139`). Alice's list height is fixed at `minmax(260px, 46%)`.
- **Plan:** Optional follow-up:
  1. A splitter `<div role="separator" aria-orientation="horizontal" tabIndex=0>` that changes a `--list-height` CSS variable, with Up/Down keys for keyboard users.
  2. Persist it as a `list_height` preference.
  3. Drop the `todo.md` line or rename it "Resizable list/reader split".
- **Done when:** dragging or arrow keys resize the list, the size survives a restart, and the minimum keeps 3 rows visible.
- **Effort:** S.

<a id="full-width-message-view-with-optional-max-width"></a>

### Full-width message view with optional max width

*Roadmap: P2 · Reading*

- **Why:** On wide or ultrawide screens, wide HTML newsletters and tables get squeezed into a column and scroll horizontally, while half the window is empty.
- **Competitors:** Mailspring 1.6.2 (`ms.md:1596`, #251) made the message viewer and composer stretch to full width by default. `core.reading.restrictMaxWidth` (default false, "Restrict width of messages to maximize readability", `app/src/config-schema.ts:157-161`) caps them at 800 px for the UI and 840 px inside the email frame (`message-list.less:4, 139-150`; `email-frame.less:107-109`). Gmail and Apple Mail are full width.
- **Alice today:** the content is always capped:
  - the reader column at 780 px (`.message-content`, `styles.css:247`);
  - plain-text bodies at 660 px (`styles.css:262`);
  - the HTML frame at 100% of that column (`styles.css:270`).
- **Plan:**
  1. Add a preference `reader_width: 'readable' | 'full'` (`model.rs` `Preferences`, `api.ts:46-47`, default `readable`) with a two-option control in Settings → Reading.
  2. Under `.app-shell.reader-full`, `.message-content` has `max-width: none` with side padding kept. Plain text stays at a 75ch measure, since long lines hurt reading.
  3. Do the same for the compose dialog width if the Composer section wants it.
- **Pitfalls:** many HTML mails are built at 600 px and centre themselves, so full width mostly helps tables and plain HTML. Keep the reader padding so text doesn't touch the edges.
- **Done when:** toggling the setting immediately changes the reader width, and the choice persists in `/v1/preferences`.
- **Effort:** S.

<a id="interface-zoom"></a>

### Interface zoom

*Roadmap: P2 · Reading*

- **Why:** Users with low vision or high-DPI/small screens need the whole UI larger or smaller. Alice's sizes are fixed px tokens (`styles.css:13`), so OS text size does not help.
- **Competitors:**
  - **Mailspring:** `core.workspace.interfaceZoom` (default 1, advanced), set with Preferences → Appearance → "Scaling", a slider from 0.8 to 1.4 in 0.05 steps (`internal_packages/preferences/lib/tabs/preferences-appearance.tsx:9-56`). It is applied with `webFrame.setZoomFactor` in every window (`app/src/flux/stores/workspace-store.ts:90-98`).
    - There are no zoom shortcuts; Window → Zoom is the OS maximize.
    - Changelog: 1.0.12 (#176); 1.5.2 fixed spell-check squiggles and the Add Account window under zoom (onboarding is forced to 1).
  - **Others:** Thunderbird and browsers use Ctrl/⌘ +, −, 0.
- **Spec:** Tauri `getCurrentWebview().setZoom(scale)` / Rust `Webview::set_zoom`; needs the capability `core:webview:allow-set-webview-zoom`. Window config `zoomHotkeysEnabled`:
  - On Windows it turns on WebView2's native zoom.
  - On macOS and Linux it injects a ±20% polyfill. It is not persisted.

  Source: https://tauri.app/reference/config, https://tauri.app/reference/javascript/api/namespacewebview.
- **Alice today:** nothing. The capability file only has `core:default` and `core:window:allow-start-dragging` (`apps/desktop/src-tauri/capabilities/default.json`).
- **Plan:**
  1. Add a preference `zoom: f32` (0.8–1.5, default 1.0) in `model.rs`/`api.ts`.
  2. In `desktop.rs` `apply` (where preferences are applied, `:256`), call `window.set_zoom(zoom)` at startup and on change, from Rust, so no new JS capability is needed.
  3. Add shortcuts in `shortcuts.ts`: Mod+=/Mod++ (zoom in 10%), Mod+- (out), Mod+0 (reset). Persist with `PATCH /v1/preferences`.
  4. Add a Settings → Appearance "Interface size" select (80%–150%).
  5. Leave `zoomHotkeysEnabled` off so there is one source of truth.
- **Pitfalls:**
  - **Context menu:** positions use `clientX/Y` from the zoomed page and stay consistent, but verify the iframe right-click mapping in `MessageView.tsx:123-131`.
  - **Frame height:** the iframe's height comes from `getBoundingClientRect`, so it scales correctly.
  - **First run:** Mailspring had to force zoom 1 in onboarding windows because of clipping. Check `Setup`/`Welcome` at 150%.
- **Done when:** the shortcuts and setting change the zoom, the value survives a restart, and no horizontal scrollbar appears at 150% with a 1280×800 window.
- **Effort:** S.

<a id="export-messages-and-folders-as-eml-and-mbox-drag-a-thread-into-the-composer-to-a"></a>

### Export messages and folders as .eml and mbox; drag a thread into the composer to attach it

*Roadmap: P2 · Reading*

- **Why:** People keep records, move to other clients and send evidence. Without export, mail outside the sync window is effectively locked in the provider.
- **Competitors:** Mailspring:
  - **Message export:** message "…" → "Download as .eml", and the thread menu "Save as .eml…" (`core:save-as-eml`, no default key). Multi-select saves several threads to a folder; each thread maps to its newest non-draft message (`app/src/services/eml-utils.ts:22-69`). Filenames are sanitized, subjects capped at 80 characters, and duplicates are not de-duplicated.
  - **Folder export:** sidebar "•••" → "Export folder as .eml files…" or "…as .mbox file…" (`app/src/components/outline-view-item.tsx:300-307`; 1.20 #2652, 1.21 bulk engine #107, 1.23 mbox #2765).
    - mailsync `GetManyRFC2822Task` (`repos/mailsync/MailSync/TaskProcessor.cpp:2704-2889`) fetches by UID from the **server**, 50 per chunk, with a 1 s pause between chunks.
    - It writes `{index:05d} - {subject} - {date}.eml`, records `{exported, failed, lastUID}` progress so the export can resume, and supports cancellation (1.23 fix).
    - The client assembles **mboxrd** (`app/src/services/mbox-utils.ts`): a `From <Return-Path|From|MAILER-DAEMON> <asctime UTC>` separator, `>From` quoting and LF line endings. It writes to `.incomplete` and renames atomically when done.
  - **Drag to composer:** dragging threads into the composer attaches them as .eml (1.24 #2800, `internal_packages/composer/lib/composer-view.tsx:311-392`). There is also "Forward as Attachment" (`message-list.tsx:148-184`).
  - **Other clients:** Thunderbird exports via ImportExportTools NG. Apple Mail: Mailbox → Export Mailbox (mbox).
- **Spec:** RFC 4155 (application/mbox; mboxrd quoting), RFC 5322, RFC 2046 §5.2.1 (`message/rfc822` attachments).
- **Alice today:**
  - No raw-message endpoint (see "Show original: raw source and full headers").
  - The local cache holds only the sync window and parsed JSON (`store.rs:20`).
  - Compose has no attachment upload (README "Current boundaries"), so drag-to-attach has nothing to attach to.
- **Plan:**
  1. **Single message.** "Save as .eml" in the message and thread menus uses `GET /v1/messages/{id}/raw` (Show original item). In the desktop app, write it to Downloads with the existing collision-safe `write_new` (`downloads.rs:398`) and reveal it. In the CLI: `alice show --raw <id> > x.eml`.
  2. **Folder export in the daemon.** Add `GET /v1/export?account_id&folder&format=mbox|eml-zip`.
     - It streams the body (axum `Body::from_stream`).
     - It walks `UID SEARCH ALL` on the server mailbox in chunks of 50 `BODY.PEEK[]`, so mail outside the sync window is included. It reuses `Remote` (read-only `EXAMINE`).
     - The daemon never writes to arbitrary paths. The client chooses the destination.
  3. **mbox writer** in a new `crates/alice-core/src/mbox.rs`, about 40 lines:
     - mboxrd quoting (`^>*From ` gets one more `>`);
     - the separator line uses the Return-Path or From address and `asctime` of INTERNALDATE in UTC;
     - write bytes, not strings.
     - For the eml-zip format, skip it unless asked; the CLI can write a directory of `.eml` files instead.
  4. **Desktop flow.** Folder context menu → "Export…". The Tauri side writes the stream to `Downloads/<folder>.mbox.partial`, then renames it.
     - Show a progress toast, using the `X-Total` header from `STATUS (MESSAGES)`, with Cancel.
     - Add the CLI command `alice export --account a --folder inbox --format mbox -o inbox.mbox`.
  5. **Drag thread to composer and Forward as attachment.** Do these after the Composer attachment work: stage the raw bytes as a `message/rfc822` part. Track them under the Composer item's dependency.
- **Pitfalls:**
  - **Gmail duplicates:** labels mean exporting All Mail duplicates Inbox and others. Offer per-label export and warn.
  - **Large mailboxes:** Gmail IMAP has daily bandwidth limits (about 2,500 MB download per day; https://support.google.com/a/answer/1071518). For big exports, throttle as Mailspring does with its 1 s sleep, and make the export resumable from `lastUID`.
  - **Bytes, not strings:** never decode to strings, since mbox must keep 8-bit bodies byte-exact (Mailspring passes latin1 through).
  - **Filenames:** sanitize as in `downloads.rs:350` (`file_name`).
- **Done when:**
  - A unit test for `mbox.rs`: a body line `From me` becomes `>From me` and `>From` becomes `>>From`, and parsing back with `mailparse` yields identical messages.
  - A Dovecot integration test exports 120 messages (3 chunks) whose count and Message-IDs match the server.
  - A cancelled export leaves no file at the destination.
- **Effort:** M for export. Drag-to-attach is S after "Composer attachments", which lives in another section.

<a id="import-from-mbox-thunderbird-apple-mail"></a>

### Import from mbox / Thunderbird / Apple Mail

*Roadmap: P3*

- **Why:** Switchers have years of archived local mail (Thunderbird "Local Folders", Apple Mail "On My Mac") that exists on no server.
- **Competitors:**
  - **Mailspring:** no import at all. Opening a `.eml`/`.mbox` file attaches it to a new draft (`app/src/flux/stores/draft-store.ts:428-442`).
  - **Thunderbird:** Tools → Import (profiles, mbox, eml).
  - **Apple Mail:** File → Import Mailboxes (Apple Mail, Thunderbird, mbox).
  - **eM Client:** imports from most clients.
- **Spec:** RFC 4155 (mbox variants: mboxo/mboxrd); RFC 3501 §6.3.11 APPEND (flags and date-time); Apple `.emlx` (a byte-count line, the RFC 5322 message, then a trailing XML plist with flags; `.partial.emlx` lacks attachments).
- **Alice today:** nothing.
  - `Remote` is read-only by design (`remote.rs:1-2`).
  - There are no local-only folders; every message belongs to a server mailbox.
  - `imap 2.4.1` has `append_with_flags_and_date` (`imap-2.4.1/src/client.rs:1187`).
- **Plan:**
  1. Import means uploading to a chosen server folder (e.g. "Imported/Thunderbird"). Alice has no local-only store, and the server copy then syncs normally. Make this explicit in the UI.
  2. Add a new `crates/alice-core/src/import.rs` with three readers:
     - **mbox:** split on `\nFrom ` lines and un-escape `>From` for mboxrd.
     - **eml:** a directory of `.eml` files.
     - **emlx:** read the byte count, take that many bytes, and parse flags from the plist with `quick-xml` (already a dependency).

     Thunderbird profiles are mbox files without an extension plus `.msf` indexes (ignore those). Detect the profile at `~/Library/Thunderbird/Profiles/*/Mail|ImapMail` or `%APPDATA%\Thunderbird\Profiles`.
  3. Add `POST /v1/import` (multipart upload of a file or stream) → background job. For each message, `APPEND` with `\Seen`/`\Flagged` from the source flags and the Date header as INTERNALDATE. Skip messages whose Message-ID already exists in the target (`UID SEARCH HEADER Message-ID`). Report progress through a job endpoint.
  4. Desktop: Settings → Mail accounts → "Import mail…" with a file/folder picker. This needs `tauri-plugin-dialog` (new dependency) or a drag-and-drop zone. CLI: `alice import --account a --folder Imported path/`.
- **Pitfalls:**
  - **macOS permissions:** reading `~/Library/Mail` needs Full Disk Access. Prefer letting the user export from Apple Mail (Mailbox → Export) and import the `.mbox` folder instead.
  - **Partial Apple Mail files:** `.partial.emlx` files have attachments stored separately under `Attachments/`. Reassembling them is out of scope; warn and import the partial message.
  - **Upload limits:** Gmail IMAP uploads are limited to about 500 MB per day, so large imports must pause and resume.
  - **Bad dates:** malformed Date headers make APPEND fail. Fall back to omitting the date.
- **Done when:**
  - Unit tests parse an mboxrd fixture (with a quoted `>From` line), an emlx fixture (byte count plus plist flags), and a Thunderbird folder fixture.
  - A Dovecot test imports 50 messages with flags preserved, and a re-run imports 0 (dedupe).
- **Effort:** L.
- **Note:** Mailspring lacks this entirely, so it is a differentiator rather than parity. Consider P3, after export.

<a id="permalinks-to-a-thread"></a>

### Permalinks to a thread

*Roadmap: P2 · Reading*

- **Why:** Linking a mail from notes, tasks or calendar events ("see alice://…") is a common power-user workflow (Apple Mail `message:` links in Notes and OmniFocus).
- **Competitors:**
  - **Mailspring:** "Copy mailbox permalink" (thread menu and thread-list context menu, Ctrl+L) generates `mailspring://thread?subject=<subject>&date=<first message unix time>` (`app/src/flux/models/thread.ts:218-222`).
    - The app registers the `mailspring` protocol (`app/src/browser/application.ts:199`).
    - The handler finds a thread with the exact subject and a first-message time within ±60 s, then opens it in a popout. Otherwise it shows "The thread %@ does not exist in your mailbox!" (`internal_packages/thread-list/lib/thread-permalink-handler.ts:14-69`).
    - Changelog: 1.6.2, `ms.md:1589`; 1.9.2 decoupled it from the cloud Thread Sharing feature.
  - **Apple Mail:** uses `message:%3C<Message-ID>%3E`, which survives moves and devices.
- **Spec:** RFC 2392 (`mid:` URLs use the Message-ID); Tauri deep-link plugin (https://tauri.app/plugin/deep-linking).
- **Alice today:**
  - No URL scheme.
  - Notification clicks already open a specific message through `desktop::open(Target::Message)`, which emits `open-message` and the page polls `take_opened` (`apps/desktop/src-tauri/src/desktop.rs:432-455`, `apps/desktop/src/api.ts:167`).
  - The search index keeps `message_id` per message (`store.rs:1160`).
- **Plan:**
  1. Use the URL form `alice://message/<percent-encoded Message-ID>`. This is better than Mailspring's subject+date: it is stable across moves, devices and resyncs, and it is unambiguous. Offer it only for messages that have a Message-ID.
  2. Daemon: `GET /v1/messages?message_id=` (exact match via `message_index.message_id`) in `daemon.rs`/`store.rs`.
  3. Desktop:
     - Add `tauri-plugin-deep-link` (new dependency), with `plugins.deep-link.desktop.schemes = ["alice"]` in `tauri.conf.json`.
     - On Windows and Linux, also add `tauri-plugin-single-instance` (new dependency), so a second launch forwards the URL.
     - Route the URL to the existing `open(Target::Message(id))` after lookup.
     - Add a "Copy link to message" item in the message context menu.
  4. CLI: `alice open alice://…` prints or opens the message.
- **Pitfalls:**
  - **Untrusted input:** URLs arrive from any app. Only look up and show; never act on them. Reject other hosts and paths.
  - **Duplicates:** the same Message-ID can exist in several accounts, and as Sent plus Inbox copies. Prefer the non-Sent copy, then the first account.
  - **Outside the sync window:** say "not on this device" instead of searching the server.
  - **Linux:** registration only happens once the app is installed (`.desktop` file). In development, call `register_all()`.
- **Done when:**
  - A unit test for URL parsing round-trips `<a/b@c>` and rejects `alice://evil/…`.
  - Manual: a link clicked in Notes or a browser opens and selects the message on all three OSes while the app is already running.
- **Effort:** M.

<a id="tnef-winmaildat-decoding"></a>

### TNEF (winmail.dat) decoding

*Roadmap: P2 · Reading · new*

- **Why:**
  - Outlook and Exchange sometimes send "Rich Text" messages whose real attachments are packed into a single `winmail.dat` (`application/ms-tnef`).
  - Recipients outside Outlook see one opaque file.
  - Mailspring users asked for decoding (forum #712, open since 2021).
- **Competitors:**
  - **Mailspring:** no TNEF code in the client or mailsync (grep finds none). The forum thread points to Thunderbird's LookOut add-ons.
  - **Other clients:** Gmail's web interface lists the files inside `winmail.dat`. Thunderbird needs an add-on.
- **Spec:** [MS-OXTNEF] (TNEF stream: attribute levels, `attAttachTitle`, `attAttachData`, `attAttachment` MAPI properties such as `PR_ATTACH_LONG_FILENAME`). [MS-OXRTFCP] for compressed RTF bodies (`PR_RTF_COMPRESSED`, LZFu) and [MS-OXRTFEX] for HTML encapsulated in RTF.
- **Alice today:**
  - A TNEF part is listed as an ordinary attachment named `winmail.dat` (`crates/alice-core/src/mime.rs:410`, `attachments()`).
  - Attachments are fetched only on demand (`remote.rs:1-7`), so contents are unknown at sync time.
- **Plan:**
  1. **Crate.** Use `tnef` 0.1.1 (MIT/Apache, pure Rust, about 400 lines, https://github.com/newpavlov/tnef). `tnef::read_attachments(&bytes)` returns attachments from the legacy `attAttachTitle`/`attAttachData` attributes.
     - New dependencies: `tnef` + `codepage`. `byteorder`, `chrono` and `encoding_rs` are already in `Cargo.lock`.
     - It is unmaintained since 2019 and ignores MAPI properties: no long Unicode filenames, no embedded messages, no RTF body.
     - Alternatives: `ytnef` (FFI to the C libytnef; avoid C), or a small own parser if long names matter. Ceiling: add `PR_ATTACH_LONG_FILENAME` parsing (about 60 lines) when 8.3-truncated names show up.
  2. **Endpoint.** Add `GET /v1/messages/{id}/attachments/{index}/parts` in `daemon.rs`. It fetches the TNEF attachment through the existing `attachment()` path, decodes it, and returns `[{name, size}]`. `GET …/attachments/{index}/parts/{n}` returns bytes, with the same headers as `attachment()` (`daemon.rs:548-573`).
  3. **Desktop.** In `Attachments.tsx`, show `content_type == application/ms-tnef` or the name `winmail.dat` as "Outlook attachments (winmail.dat) · Show files". Expanding it lists the inner files with the same open/reveal/preview actions. Extend `downloads.rs` `fetch` with an optional inner index.
  4. **RTF body.** If the outer message has no text body and the TNEF holds `PR_RTF_COMPRESSED`, show "This message's text is in Outlook format". Decoding LZFu plus RTF→HTML de-encapsulation is about 300 lines; defer until demand.
- **Pitfalls:**
  - **Untrusted input:** TNEF is attacker-controlled binary. Fuzz the decode path once (`cargo fuzz` target or a property test with truncated inputs) and cap the output size.
  - **Inner filenames:** pass them through `file_name()` (`downloads.rs:350`). The existing `runs_when_opened` rule applies to inner files too.
  - **Calendar items:** some TNEF payloads are invitations (`IPM.Microsoft Schedule.MtgReq`). Leave those as-is.
- **Done when:**
  - A unit test decodes a `winmail.dat` fixture with two attachments (names and bytes match).
  - A truncated fixture returns an error rather than a panic.
  - The UI lists and opens the inner files.
- **Effort:** S–M (S for attachments only).

<a id="attachment-position-option-show-attachments-above-the-body"></a>

### Attachment position option: show attachments above the body

*Roadmap: P3 · new*

- **Note:** This is P3 by demand (Mailspring forum #772, 8 votes; GitHub #738, 7 👍). It is filed here because it touches the same reader code.
- **Why:** In long messages and threads, attachments at the bottom are easy to miss, especially on invoices and forwards.
- **Competitors:**
  - Mailspring always renders attachments below the body (`internal_packages/message-list/lib/message-item.tsx:122-139`) and has no option.
  - Outlook shows attachments above the body. Gmail and Apple Mail show them below.
- **Alice today:** `Conversation.tsx:39-40` renders `MessageView` and then `Attachments`, so attachments are always below.
- **Plan:**
  1. Add a preference `attachments_position: 'below' | 'above'` (`model.rs` Preferences, `api.ts`), default `below`, in Settings → Reading.
  2. `Conversation.tsx` swaps the two elements based on it.
  3. Optional: when there are 3+ attachments above, show a compact chip row instead of full rows.
- **Pitfalls:** keep DOM order equal to visual order (no CSS `order`), so screen readers and Tab follow what is seen.
- **Done when:** toggling moves the attachments in open messages immediately, and the preference persists.
- **Effort:** S.

## P2 → Localization and accessibility

<a id="more-locales-fr-es-it-pt-br-ja-zh-cn-ko-ru-nl-pl-sv-tr-uk-he-ar-machine-first-hu"></a>

### More locales: fr, es, it, pt-BR, ja, zh-CN, ko, ru, nl, pl, sv, tr, uk, he, ar (machine-first, human-reviewed later)

*Roadmap: P2 · Localization and accessibility*

- **Why:** Most people outside the US and UK run their OS in another language. With only English and German, Alice is a hard sell in most markets, and Mailspring offers 19 reviewed languages.
- **Competitors:** Mailspring:
  - **Catalogs:** 109 `app/lang/*.json` files keyed by the English source string (`"%1$@ of %2$@": "%1$@ von %2$@"`). About 80 of them are full machine translations of roughly 884 keys.
  - **How they were made** (`LOCALIZATION.md`): a database of human-translated mail terms plus Google's translation API.
    - 1.5.0 (`ms.md:1885-1892`): core strings were hand-localized in 38 languages, the rest machine-translated.
    - 1.5.2: English is used for languages no contributor has reviewed. `VERIFIED_LANGS` now has 19 entries (`app/src/intl.ts:30-50`); the others sit under "Experimental" in Preferences → General → Language (`core.intl.language`, relaunch required).
    - 1.17.3: an LLM pass refreshed old machine translations. Languages were promoted to verified over time (ja 1.6, zh/ru/uk 1.7.5, cs/ko/pt 1.9, hu 1.17, he 1.24).
  - **Weak spots to avoid:**
    - No plurals: English ternaries such as `days === 1 ? '1 day' : '%@ days'`.
    - Dates follow `navigator.language` through moment, not the chosen UI language (`app/src/date-utils.ts:4`).
    - The setting's title was misspelled and untranslated ("Interface Langauge").
- **Spec:** CLDR plural rules (https://www.unicode.org/cldr/charts/latest/supplemental/language_plural_rules.html); BCP 47 language tags (RFC 5646) and matching (RFC 4647 lookup).
- **Alice today:** a small, solid in-house system; no i18next.
  - **Rust** (`crates/alice-core/src/i18n.rs`):
    - JSON catalogs per namespace, embedded with `include_str!` (`:401-414`).
    - `{name}` placeholders and CLDR plural objects (`{one, other}`), with `count` selecting the form (`:341-391`).
    - Plural rules are hand-written for en/de only (`:55-65`).
    - Locale comes from sys-locale, gettext env vars, or `Accept-Language` with q-values (`:89-136`).
    - Dates use chrono `unstable-locales` with strftime patterns taken from the catalogs (`:321-330`).
  - **Desktop** (`apps/desktop/src/i18n.ts`):
    - Keys are type-checked against en-US (`:2-15`).
    - Catalogs are loaded **eagerly** via `import.meta.glob(..., {eager:true})` (`:28`).
    - Plurals use `Intl.PluralRules`; numbers, dates and lists use `Intl` (`:75-112`).
    - `matchLocale` matches the base language only (`:38-45`).
  - **Locales:** en-US and de. 579 Rust messages and 571 desktop messages, about 7,600 words in total. Plus `InfoPlist.strings` per `.lproj` and `CFBundleLocalizations` (`apps/desktop/src-tauri/Info.plist`).
  - **Checks:** `scripts/check-i18n.mjs` checks key parity, placeholder parity, plural categories per locale (via `Intl.PluralRules`), and that every Rust `t!` key exists.
- **Plan:**
  1. **Keep the in-house format.** It already has typed keys, CLDR plurals and a lint. i18next or ICU MessageFormat would add runtime and migration cost for features Alice doesn't use (select/gender).
  2. **Adding a locale** means touching:
     - a `Locale` variant in `i18n.rs` (tag, native name, English name for AI prompts, chrono locale, plural rule) and `files()`;
     - the `languages` array in `i18n.ts`;
     - `locales/<tag>/*.json` in both roots;
     - a `<tag>.lproj/InfoPlist.strings`, `CFBundleLocalizations`, and the `tauri.conf.json` `bundle.resources` list.

     Write this as a checklist in `i18n.rs`'s module doc; it is partly there.
  3. **Plural rules in Rust:** add integer CLDR rules for each new language:
     - one/other: fr (0 and 1 are "one"), es, it, pt, nl, sv, tr;
     - other only: ja, zh, ko;
     - one/few/many/other: ru, uk, pl;
     - one/two/other: he;
     - zero/one/two/few/many/other: ar.

     Either hand-write them (about 30 lines) or add `icu_plurals` 2.x (new dependency, ICU4X). Lazy choice: hand-write, plus a generated fixture. A node script emits `Intl.PluralRules(tag).select(n)` for n in 0..=200 into a JSON fixture, and a Rust test asserts `Locale::plural` matches it for every locale.
  4. **Region-aware matching** in both `matchLocale` (`i18n.ts:38`) and `Locale::matching` (`i18n.rs:70`):
     - `pt-*` → pt-BR.
     - `zh-Hans`/`zh-CN`/`zh-SG` → zh-CN.
     - `zh-TW`/`zh-HK`/`zh-Hant` → **no match** (English), until zh-TW exists. Simplified Chinese is wrong for Traditional readers.
  5. **Lazy catalogs.** Load non-English catalogs lazily: `import.meta.glob` without `eager`, awaited in `applyLanguage`/`updateLocale`. With 17 locales, eager loading would add about 1 MB to startup JS. Keep en-US eager as the fallback.
  6. **Machine-first pipeline** in a new `scripts/translate.mjs`:
     - For each missing key, send the en-US value, the key path as context, a glossary (Archive, Spam, Inbox, conversation, app password) and the target's plural categories to the chosen MT/LLM provider.
     - Placeholders `{…}` must be preserved; `check-i18n.mjs` already enforces this.
     - Mark machine-only locales with `reviewed: false` in `languages`, and show "(beta)" in the picker, as Mailspring's Experimental group does.
  7. **Human review:** Hosted Weblate (free Libre plan for libre projects; AGPL qualifies) or Crowdin for Open Source.
     - Alice's nested `{one, other}` objects match Weblate's "go-i18n JSON v2" style. Verify plural round-tripping before committing.
     - i18next v4 `key_one` suffixes would be the other well-supported option. Not worth converting unless the platform requires it.
- **Pitfalls:**
  - **Shared namespace:** `common.json` is shared byte-for-byte between Rust and desktop. Translate it once.
  - **Hidden strings:** Rust has a few hardcoded English literals left (e.g. `store.rs:770,808` "Message not found", `mcp.rs:258`). Fix them first or they show through in every locale.
  - **Text expansion:** German is already in, but ru/pl/fr run 20–35% longer. Check the fixed-width parts of `styles.css` (sidebar 248 px, layout buttons).
  - **CJK:** CJK UI needs fallback fonts. DM Sans and Newsreader have no CJK glyphs, so add `system-ui` and platform CJK families to the font stacks.
  - **AI prompts:** these use `english_name`, and AI replies follow the UI language. Check that every new locale has a sensible English name.
- **Done when:**
  - `npm run lint:i18n` passes with all new locales.
  - The Rust plural-fixture test passes.
  - The picker lists the languages with "(beta)" markers.
  - Switching to ja, ru, ar and he renders every screen without missing keys or overflow (manual screenshot pass).
  - The main JS bundle grows by less than 100 KB.
- **Effort:** M for infrastructure plus S per machine-translated language (L total for 15). Depends on "Full RTL interface" for he/ar; ship those two after RTL or keep them hidden.

<a id="full-rtl-interface"></a>

### Full RTL interface

*Roadmap: P2 · Localization and accessibility*

- **Why:** Hebrew and Arabic users need a mirrored UI. Everyone who reads RTL mail needs correct text direction in subjects, previews and bodies, and that part matters even in an English UI.
- **Competitors:** Mailspring 1.5.0 (`ms.md:1885-1892`):
  - Runs `rtlcss` over all compiled LESS (`app/src/compile-cache-less.ts:26-34`, separate `less-rtl` cache) and sets `dir="rtl"` on the root (`app/src/app-env.ts:867-869`) for 19 `RTL_LANGS` (`intl.ts:5-25`).
  - Swaps macOS window controls back (`sheet-toolbar.tsx:219-230`) and moves the scrollbar left (`scroll-region.tsx:157`).
  - In the composer, sets `dir=rtl` per block (`composer-editor/base-block-plugins.tsx:103-104`).
  - Strips bidi control characters from attachment names (`app/src/flux/models/file.ts:84`).
- **Spec:** HTML `dir="auto"` (WHATWG HTML §3.2.6.4); CSS Logical Properties Level 1; Unicode bidi (UAX #9); WCAG 1.3.2 Meaningful Sequence.
- **Alice today:**
  - No `dir`, `:dir()` or `direction` anywhere in the desktop app. `i18n.ts:62` sets only `lang`.
  - `styles.css` has about 64 physical inline-axis declarations (`margin-left`, `left:`, `border-right`, `text-align:left`, …) against 20 logical ones.
  - Message bodies, subjects and previews carry no `dir="auto"` (`MessageBody.tsx:27`, `App.tsx` rows, `Conversation.tsx:30`).
- **Plan:**
  1. **Content direction first** (S, independent of RTL locales):
     - `dir="auto"` on each plain-text `<p>` in `MessageBody.tsx`, on subject, sender and preview spans in list rows and the reader heading, and on compose inputs and the textarea (`Compose.tsx`).
     - In `emailHtml.ts`, add `dir="auto"` to the frame `<body>` when the email sets no `dir`.
  2. **UI mirroring:**
     - In `i18n.ts` `updateLocale`, set `document.documentElement.dir = rtl.has(base) ? 'rtl' : 'ltr'` with `rtl = {ar, he, fa, ur}`.
     - Convert `styles.css` physical properties to logical ones (`margin-inline-start`, `inset-inline-start`, `border-inline-end`, `text-align: start`). Mechanical; about 64 edits.
     - Mirror directional icons (ChevronRight/Left, back arrows, Reply) with `:dir(rtl) .directional { transform: scaleX(-1) }`. Do not mirror clocks, checkmarks or media icons.
  3. **macOS:** with `titleBarStyle: Overlay` the traffic lights stay on the physical left. Keep the sidebar's traffic-light padding physical (`padding-left`), as Mailspring does by swapping controls back.
  4. **Guard against regressions:** a `lint:css` check (a few lines in `scripts/check-i18n.mjs`) that fails on new `margin-left|padding-left|left:|text-align: left` outside an allowlist.
  5. **Out of scope:** TUI. Terminal bidi support is inconsistent.
- **Pitfalls:**
  - **Mixed numbers and addresses:** "re: 2 messages" and email addresses inside RTL text need `<bdi>` around inserted values (sender names, addresses) in `t()` output. `rich()` (`i18n.ts:114-124`) can wrap slots in `<bdi>`.
  - **Keyboard:** shortcuts with ArrowLeft/Right keep their meaning; Alice uses Up/Down/j/k, so that is fine.
  - **Motion:** horizontal slide animations (`motion.tsx`) must flip direction.
- **Done when:**
  - With `he` selected, every screen is mirrored (manual screenshot pass).
  - A Hebrew email renders right-aligned in an English UI.
  - The lint rejects a new `margin-left`.
  - A `scripts/test-reader.mjs` case asserts `dir="auto"` on plain-text paragraphs.
- **Effort:** M (content direction S; CSS conversion M).

<a id="screen-reader-pass-roving-tabindex-in-lists-focus-traps-in-modals-landmarks"></a>

### Screen-reader pass: roving tabindex in lists, focus traps in modals, landmarks

*Roadmap: P2 · Localization and accessibility*

- **Why:** Email is a core task for blind and low-vision users. Today the thread list puts every row in the Tab order (50+ stops per page) and nothing announces sync or new mail.
- **Competitors:** Mailspring:
  - **1.19.0** (`ms.md:587`, #2638): landmarks, ARIA attributes, focus traps for modals, labels on buttons.
  - **1.19.1** (`ms.md:546, 551`):
    - VoiceOver fixes for the To/Cc/Bcc token fields.
    - Arrow keys plus roving tabindex between messages in a thread (`internal_packages/message-list/lib/message-list.tsx:264-300, 356-358`, `data-usesarrowkeys` suppressing global shortcuts, `app/src/keymap-manager.ts:76-82`).
    - Account sidebar as a `role="tree"` with roving tabindex (`app/src/components/outline-view.tsx:258-355`).
    - An ARIA label on the email frame.
  - **Thread list:** uses `role="listbox"` + `aria-activedescendant`. It must refocus the listbox after each move to get announcements (`app/src/components/multiselect-list.tsx:239-243, 307-310`). Rows get composed `aria-label`s (`thread-list-aria-utils.ts`).
  - **Gaps despite the changelog:** `Modal` has no `role="dialog"`/`aria-modal` and no Tab trap (`app/src/components/modal.tsx`), no `aria-live` anywhere, and no `prefers-reduced-motion`.
- **Spec:**
  - WAI-ARIA APG: Listbox (https://www.w3.org/WAI/ARIA/apg/patterns/listbox/), the "E-mail Inbox" treegrid example (https://www.w3.org/WAI/ARIA/apg/patterns/treegrid/examples/treegrid-1/), and keyboard focus with roving tabindex vs `aria-activedescendant` (https://www.w3.org/WAI/ARIA/apg/practices/keyboard-interface/).
  - Modal Dialog pattern (https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/).
  - WCAG 2.2: 2.4.1 Bypass Blocks, 2.4.3 Focus Order, 2.4.11 Focus Not Obscured, 2.5.7 Dragging Movements, 2.5.8 Target Size (24×24), 4.1.3 Status Messages.
- **Alice today:** good basics, weak lists and announcements.
  - **Landmarks:** `<aside>` sidebar, `<nav aria-label>` with `aria-current`, `<section aria-label="Message list">`, `<main class="reader" tabIndex=0>` (`App.tsx:344-388`).
  - **Compose:** a native modal `<dialog>` via `showModal()`, which gives containment and Esc; focus is restored on close (`Compose.tsx:28-34, 84`).
  - **Context menu:** `role="menu"` with arrow keys and focus restore (`ContextMenu.tsx:91-126`).
  - **Motion:** reduced motion is respected (`styles.css:46, 875-878`; `main.tsx:22`).
  - **Toasts:** `role="status"`/`"alert"` (`App.tsx:407`).
  - **Thread list:** `div.email-list tabIndex=0` holding one `<button aria-pressed aria-label=…>` per row, every row tabbable. Navigation is by a global keydown handler (`App.tsx:279-313, 375-385`).
  - **Conversation:** open messages are `<section aria-label>` with no arrow navigation (`Conversation.tsx:32-41`).
  - **Missing:** no skip link, and no `forced-colors` styles. Only 5 `tabIndex` uses in the app.
- **Plan:**
  1. **Thread list → listbox with roving tabindex** (`App.tsx:375-385`):
     - `role="listbox"` with `aria-label` = folder name; rows `role="option"` + `aria-selected`.
     - The selected row has `tabIndex=0` and the others `-1`. j/k/Arrow and Home/End move focus to the row as well as selecting it. Date dividers become `role="group"` with `aria-label`.
     - Prefer real focus (roving) over `aria-activedescendant`: WebKit/VoiceOver announce real focus more reliably (Mailspring needed a refocus hack).
     - If rows ever gain interactive children (star or archive buttons), switch to the APG inbox treegrid.
  2. **Conversation:** Up/Down move focus between `thread-message` sections and collapsed buttons. Enter/Space toggles. Make them `role="article"` with `aria-labelledby` the sender line. Don't steal arrows while focus is inside the iframe.
  3. **Announcements:** one persistent visually-hidden `aria-live="polite"` region in `App.tsx`, mounted at startup (VoiceOver ignores regions inserted together with their text). Route toasts, "N new messages", sync finished/failed and "Conversation archived" through it.
  4. **Skip links** at the top of `App.tsx`: "Skip to message list" and "Skip to reader".
  5. **Dialogs:** add a Settings close shortcut (Esc) if it is missing. Confirm the Assistant panel moves focus in and out (`Assistant.tsx:25-53` does). Use native `<dialog>` for new modals (Preview, Original, Print) so focus containment comes free.
  6. **Other fixes:**
     - `@media (forced-colors: active)` rules for the selection and focus indicators that rely on background colour.
     - Audit icon buttons to 24×24 minimum (WCAG 2.5.8).
     - Non-drag alternatives for any future drag interactions (account reorder: Move up/down).
  7. **Tooling:**
     - Add `eslint-plugin-jsx-a11y` (new dev dependency) to `eslint.config.js`.
     - A manual VoiceOver (macOS WKWebView) and NVDA (Windows WebView2) checklist in `docs/`, covering: read the list, open a message, read the iframe body, reply, archive, settings.
- **Pitfalls:**
  - **Navigation loads mail:** list navigation that also selects triggers message loads. Keep the existing debounce or loading behaviour so arrowing through 50 rows doesn't fire 50 fetches. APG warns against selection following focus when it is expensive.
  - **Global shortcuts:** single-key shortcuts conflict with screen-reader quick-navigation keys (VoiceOver QuickNav, NVDA browse mode). Alice already ignores them inside inputs (`App.tsx:285-287`). Document that NVDA users switch to focus mode in the list, and keep `aria-keyshortcuts` accurate.
  - **Iframe body:** the email iframe must remain in the accessibility tree; never `aria-hidden` it. Its `title` is already localized (`MessageView.tsx:105`).
- **Done when:**
  - Tab from the search box reaches the list in one stop, and the next Tab reaches the reader.
  - VoiceOver reads "Inbox, list box, <row label>, 1 of 50" and announces "Conversation archived" and new mail.
  - The NVDA checklist passes.
  - `eslint-plugin-jsx-a11y` runs clean in CI.
- **Effort:** M. Related: "Full RTL interface" (shared `styles.css` pass).
