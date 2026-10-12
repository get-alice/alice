# Provider and platform verification — 11 October 2026

## Provider coverage

| Path | Result | Evidence and limits |
| --- | --- | --- |
| Live Gmail | Passed | Real macOS account connected with an app password. Three labeled self-addressed messages were delivered through the API, CLI reply, and native app reply. |
| Gmail inbox management | Passed | Read/unread, Inbox and Sent stars, archive, trash, restore, receipt import, duplicate-send protection, and UTF-8/reply headers survived provider sync. |
| Native app ↔ CLI | Passed | A native reply draft had the same ID, recipient, and UTF-8 body in the CLI; the native app sent it and rendered the received reply. |
| IMAP | Passed against isolated Dovecot | Validated TLS certificates, localized special-use mailboxes, UID moves, external flag changes/deletion, body-to-UID association across batches, and independent Inbox/Sent copies. |
| SMTP | Passed against isolated Postfix and live Gmail | Required STARTTLS on 587 and implicit TLS on 465. Independent IMAP delivery confirmed recipient, body, reply headers, and one delivery after repeated send. |
| Credentials | Passed on macOS and Linux | Gmail used macOS Keychain. Isolated tests restarted the daemon and reloaded fixture credentials from an actual GNOME Secret Service. |
| Outlook | Skipped at the user's request | Outlook OAuth, token refresh, and live send/receive remain unverified. |
| Google OAuth | Unverified | Gmail live testing used an app password. It does not verify Google OAuth consent or token refresh. |

Live tests touched only the uniquely labeled test messages. Addresses, credentials, tokens, and private mailbox contents are omitted from this report. Three test messages remain in Gmail for inspection.

## Fixes and verification passes

The first protocol pass reproduced connection failure when DNS returned IPv6 before the server's working IPv4 address, incorrect Gmail All Mail discovery, loss of a Sent copy also present in Inbox, and missing SMTP UTF-8 content type. The shared provider/store paths were repaired and the isolated suite passed.

The second pass reproduced stale archived flags when a Sent copy with the same Message-ID suppressed import of the received copy. The regression independently changes the archived flag on the server; it failed before the deduplication repair and passes afterward. Store import also preserves existing remote row IDs and merges provider Sent copies into acknowledged SMTP receipts.

Live Gmail exposed repeated body downloads. Sync now batches new bodies and reuses cached bodies only for the same account, mailbox, UIDVALIDITY, and UID. Flags and mailbox membership still come from the server; reconnecting an account performs a fresh body import. The immutability rule is defined in [IMAP RFC 9051 §2.3.1.1](https://www.rfc-editor.org/rfc/rfc9051.html#section-2.3.1.1).

The baseline live sync request took 58.39 seconds. Batching alone measured 62.52 seconds and did not establish a speed improvement. With body reuse, three repeated requests took 5.85, 5.57, and 8.13 seconds. These are observations for this account and network, including any waiting for background sync.

Restart verification exposed another defect: macOS Keychain could block the async runtime while waiting for permission, stalling the local API. The shared credential read/write functions now run on Tokio's blocking pool. A real authenticated HTTP regression holds credential reads and writes open and checks that the health endpoint responds; it reproduced the stall before the repair. OAuth, mail credentials, and AI settings all use the repaired shared functions.

The user also reported a non-clickable URL in “Test 123.” The reader rendered every paragraph as plain text. Text emails now render HTTP/HTTPS URLs as underlined, keyboard-accessible links, preserving query strings and surrounding punctuation. Links open through a native command that rejects file/script schemes and embedded credentials. The reader regression failed before the repair and passes afterward. No test followed the live email's tracking URL.

The Gmail/Alice screenshots then exposed the underlying HTML display gap. The shared MIME importer now preserves `html_body` alongside the text body. Existing JSON rows remain compatible; the cache excludes old rows missing the HTML field until one fresh import. The MIME and cache regressions failed before the change and pass afterward. Plain text remains available to the CLI, TUI and AI.

The desktop sanitizes HTML with DOMPurify and renders it inside a separate iframe with scripts, forms and navigation privileges disabled. A restrictive document policy blocks scripts and remote resources; HTTPS images become available only after **Load images** is chosen. Image source sets and legacy background attributes are removed. Link labels, styles and tables are preserved. This follows the [iframe sandbox model](https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/iframe) and [Content Security Policy](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Content-Security-Policy).

The sanitizer regression covers executable tags, event handlers, unsafe URLs, remote images, permitted inline bitmap images, CSS preservation and explicit HTTPS image loading. A separate browser fixture confirmed styled tables/button links, blocked image defaults, opt-in image download, frame resizing, plain-text/formatted toggling, and links opening outside the email frame. No tracking links or image resources from the real mailbox were opened. Embedded CID images and HTML compose remain outside the current implementation.

The final native macOS reader displayed the real Djinni newsletter's styled layout and **Browse Jobs** button, and the **Mailspring** link label in **Test 123**. Both messages retain text bodies. After restarting the final binary, Gmail synced in 4.32 seconds with no errors; the three distinct Inbox/Sent test pairs, CLI/native reply receipt IDs, and both HTML examples remained intact. A follow-up layout fixture reproduced continuous frame growth from viewport-relative body styles. Body sizing is now normalized and automatic frame height is capped at 12,000 pixels; longer content remains scrollable. The regression confirmed a stable 380-pixel layout and a bounded 20,000-pixel email.

A mixed multipart message with several inline sections uses its complete plain text body when a single HTML section would hide the other content. The existing mixed-message regression reproduced this omission and now checks that the partial HTML is not used. Standard HTML alternatives remain formatted.

Automated native screenshots captured the reader at the faded start of its entry animation, while the user's screenshot showed it fully opaque. The animation now changes position only, keeping text fully readable even when animation progress is paused. The brief slide and reduced-motion preference remain supported.

Source review checked cache account isolation, UIDVALIDITY matching, remote flags/deletion reconciliation, send receipt identity, reconnect behavior, TLS validation, credential callers, daemon serialization, and email-link trust boundaries.

## Platform coverage

| Target | Build | Runtime checks |
| --- | --- | --- |
| macOS ARM64 | Release CLI, `.app`, and `.dmg` passed | Real native Gmail workflows, CLI/API shared state, 18 Rust tests, workspace Clippy, TypeScript/Vite, reader/HTML regressions, API smoke, and PTY TUI. |
| Linux ARM64 | Ubuntu 24.04 release CLI and `.deb` passed | Workspace tests/Clippy, isolated mail lifecycle, API/CLI smoke, PTY TUI, and packaged WebView startup with embedded daemon. |
| Linux x64 | Ubuntu 24.04 release CLI and `.deb` passed under x86 emulation | Workspace tests/Clippy, API/CLI smoke, PTY TUI, and packaged WebView startup with embedded daemon. |
| Windows x64 | MSVC-target release CLI and NSIS installer passed using cargo-xwin | Cross-compiled with Microsoft CRT/SDK, LLVM and NSIS. Native Windows installer launch, GUI, Credential Manager, and TUI have not been run. |

Linux package startup is exercised under Xvfb inside a disposable container. The WebKit sandbox is disabled for that isolated test process because of container restrictions; this is not a test of desktop sandbox behavior. No production setting is changed.

Artifacts are saved locally under `target/platform-verification/`; macOS bundles remain under `target/release/bundle/`. Sizes and SHA-256 hashes for all ten final artifacts are recorded in [the artifact manifest](evidence/artifacts.json). [Source hashes](evidence/final-source.json) confirm that both builders used the final local reader, provider, store, dependency and license sources. The final Linux packages passed their startup checks, and the disposable builders were removed after their artifacts and logs were saved. Packages are unsigned. This workspace has no Git remote, so the GitHub Actions matrix has not been dispatched. The CI mail job now runs the disposable certificate-validated Dovecot/Postfix lifecycle test.

The upstream `imap-proto 0.10.2` dependency emits a future Rust compatibility warning. It does not fail current builds. Windows cross-build warnings include unavailable PDB symbols and skipped installer signing.

## Reproduction

Run the checks in the README's **Build and verify** section. `scripts/test-mail.py` is the mail lifecycle owner; it targets only `example.test` fixture accounts. `crates/alice-core/tests/mail_provider.rs` checks localized Gmail `\All` discovery through an actual IMAP server and is explicitly ignored without its fixture.

`scripts/mail-fixture.sh` must run only in a disposable container. It installs a short-lived fixture CA and configures local Dovecot/Postfix; it refuses to run outside Docker. The CI `mail-protocol` job supplies the dependencies and invokes both tests. It is not intended for a user's workstation or an existing mail server.

The existing UI/design review evidence remains in [the review directory](../../reviews/2026-10-11/fixes/README.md). This verification changed provider/store/daemon paths, credential handling, test/build configuration, reader hyperlinks, HTML display and reader entry opacity. The inbox layout remains intact. The scoped Impeccable mechanical scan reported no issues; browser/native reader inspection supplies the visual check. The HTML follow-up source and interaction passes are recorded in [the reader review](reader-review.md).
