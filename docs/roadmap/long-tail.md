# Differentiators and long tail (P3, P4)

Research behind the matching items in [`todo.md`](../../todo.md). Each entry covers why it matters, how Mailspring and other clients do it, the relevant specs, what Alice has today, a build plan, pitfalls, acceptance criteria and effort. Code citations point at the Alice working tree as of 2026-10-13; Mailspring paths are relative to [Foundry376/Mailspring](https://github.com/Foundry376/Mailspring) (`mailspring/`) and [Foundry376/Mailspring-Sync](https://github.com/Foundry376/Mailspring-Sync) (`mailsync/`).

Researched 2026-10-13 against Alice's working tree, Mailspring 1.26.0 (source, shipped bundle, changelog) and primary web sources. Line numbers refer to the files as they are today and will drift.

## Section findings

- **Three prerequisites recur.** Most items here sit on P0/P1 work rather than on each other:
  - *Whole-message access.* Alice caches only headers, text/HTML parts and invitations (`mime.rs:316-324`) and fetches attachments on demand. OpenPGP/S/MIME verification, local folders, Graph `$value` and import/export all need the exact raw message. `remote.rs` `whole()` already fetches it; nothing stores it.
  - *Richer outgoing MIME.* `providers::build_mail` sends one `text/plain` part to `To` only. PGP/MIME, S/MIME, read-receipt requests and responses, mail merge and tracker stripping in replies all wait on "Rich text / HTML compose", "Cc and Bcc fields", "Attachment upload" and a `send_raw` path.
  - *A second mail backend.* `Message` and sync are IMAP-shaped (`remote_mailbox`, `remote_uid`, `uid_validity`). Graph and JMAP both need `remote_id` and a per-account backend switch, and both assume the P0 "Multi-folder message model". Whichever backend is built first pays for the abstraction.
- **Microsoft deadlines make one P4 item urgent.**
  - EWS is being switched off in Exchange Online now: phased from 2026-10-01, fully off from 2027-04-01. Graph is the only non-IMAP path, so EWS should be dropped from the roadmap.
  - SMTP AUTH Basic is disabled by default for existing tenants at the end of December 2026, and many tenants already disable SMTP AUTH outright. I added **"Send Microsoft 365 mail through Graph when SMTP AUTH is disabled" (NEW)**, which should move to P1.
- **Privacy items form one pipeline.** Build them in this order:
  - tracker stripping (S, inside `emailDocument`)
  - image proxy on open (M; reuses the `cid:` → `data:` path in `MessageView.tsx`)
  - prefetch to hide open time (L)
  - proxy/Tor support to hide the IP (M)
  - The proxy item is also the place to fix a gap: Alice builds reqwest without `system-proxy`, so macOS/Windows system proxy settings are ignored today.
- **AI items are half done.** Ask Alice already offers per-message "Summarize" and "Draft reply" (`Assistant.tsx:56`). What's missing:
  - thread context, an inline summary and translation in place;
  - a shared `complete()` refactor of `ai::run`.
  - Thread summaries should wait for header-based threading. Otherwise subject-grouped strangers' mail leaks into the prompt.
- **Encryption.** Use rPGP (`pgp` 0.21, MIT/Apache-2.0, pure Rust, audited 2024).
  - Sequoia is LGPL-2.0-or-later. That *is* compatible with Alice's AGPL-3.0-only.
  - Sequoia still loses on practical grounds: its default backend is Nettle, a C library, and `sequoia-net` drags in reqwest 0.12 and native-tls.
  - S/MIME is simplest through the `openssl` crate (vendored on macOS/Windows). Phase 2 uses keychain and Windows-store identities.
  - Build PGP and S/MIME together. They share the crypto status model, the whole-message fetch and the composer/reader UI.
- **Linux packaging has a clock on it.** GNOME removed WebKitGTK 4.1 from the Flatpak SDK (2026-09-28). A Tauri v2 Flatpak is capped at the GNOME 51 runtime, supported until September 2027, unless Tauri's GTK4 port lands first. Two other fixes:
  - Build the `.deb` on Ubuntu 22.04, not 24.04.
  - Flathub expects the Secret portal (`oo7`), not raw `org.freedesktop.secrets`.
- **Recommended skips:**
  - open/link tracking (needs a server, contradicts tracker blocking, made meaningless by Apple MPP)
  - public thread links
  - shared drafts and team comments
  - POP3 (Mailspring rejected it too)
  - EWS
  - a JS plugin system. Ship user `theme.css` over the existing CSS variables instead, and use WASM (Extism) if plugins are ever needed.
- **Suggested build order.**
  - P3: tracking pixels → unified-inbox account choice (NEW) → mouse back/forward (NEW) → screenshot mode → translation and thread summaries → read receipts → image proxy → proxy support → Linux packaging → jump list / tray → swipe → grammar → aliases → OpenPGP + S/MIME → local folders (NEW).
  - P4: Graph send fallback (promote) → Graph backend → JMAP → LDAP/GAL (NEW) → tasks/notes → mail merge → receive-only accounts (NEW) → portable Windows (NEW) → GOA import (NEW) → mobile.
- **Demand (Mailspring votes, from the lead):**
  - OpenPGP is the #3 request overall (GitHub #25, 144 👍; forum #83, 60).
  - Exchange (#34, 67 👍), proxy (#235, 52 👍) and plugin directory (#363, 44 👍) follow.
  - Several NEW items come from the same lists.
- **Corrections to todo.md:**
  - The "AI thread summaries" item is partly shipped.
  - Hide-my-email should drop iCloud (no API) for SimpleLogin/addy.io.
  - The tracker item's `SP` tag looks wrong.
  - "Shared drafts", "public thread link" and "open/link tracking" are better marked won't-do.
  - "Plugin / theme system" should be split.

## P3: differentiators and nice-to-haves

<a id="openpgp-sign-encrypt-decrypt-verify-key-import-and-wkdautocrypt-lookup"></a>

### OpenPGP: sign, encrypt, decrypt, verify; key import and WKD/Autocrypt lookup

*Roadmap: P3*

- **Why:** The most requested missing Mailspring feature (GitHub #25, 144 👍; forum #83, 60 votes). People who already use PGP cannot switch to a client that cannot read their mail.
- **Competitors:** Mailspring has no OpenPGP at all (nothing in `repos/mailspring` or `repos/mailsync`, nothing in the changelog; the old Nylas Keybase plugin was dropped). Thunderbird (since 78) has built-in OpenPGP on RNP + Botan with its own key store, not GnuPG: per-identity key, key manager with import/export, WKD and keys.openpgp.org lookup, Autocrypt header on outgoing mail, PGP/MIME by default, per-message encrypt/sign toggles, and optional subject protection ([wiki](https://wiki.mozilla.org/Thunderbird:OpenPGP)). Delta Chat (rPGP) is Autocrypt-first: encrypt automatically once both sides have keys. eM Client and Canary offer PGP with key generation and lookup in the composer.
- **Spec:**
  - OpenPGP RFC 9580 (July 2024, obsoletes RFC 4880): MTI SHA2-256, AES-128, OCB, Ed25519/X25519; v6 keys preferred, v4 accepted; never create MD5/SHA-1 signatures.
  - PGP/MIME RFC 3156: `multipart/encrypted; protocol="application/pgp-encrypted"` (part 1 `Version: 1`, part 2 `application/octet-stream`) and `multipart/signed; protocol="application/pgp-signature"; micalg=pgp-sha256`. Signed parts must be CRLF, 7-bit (QP/base64), with no trailing whitespace.
  - Autocrypt Level 1.1 ([docs.autocrypt.org/level1.html](https://docs.autocrypt.org/level1.html)): header `Autocrypt: addr=…; [prefer-encrypt=mutual;] keydata=<base64>`, `addr` must equal From, `keydata` last, ≤10 KiB, more than one valid header means ignore all. Peer state per address: `last_seen`, `autocrypt_timestamp`, `public_key`, `prefer_encrypt`, `gossip_timestamp`, `gossip_key`. Recommendation: `disable` / `discourage` (gossip-only key, or key older than `last_seen` by 35+ days) / `available` / `encrypt` (replying to encrypted mail, or both `mutual`). `Autocrypt-Gossip` goes inside the encrypted part for To/Cc only. Draft v2 (draft-autocrypt-openpgp-v2-cert-03) adds PQ-hybrid and rotating subkeys; not needed yet.
  - WKD: draft-koch-openpgp-webkey-service-22 (July 2026). Advanced `https://openpgpkey.<domain>/.well-known/openpgpkey/<domain>/hu/<zbase32(sha1(lowercase-ASCII local part))>?l=<local part>`, then direct `https://<domain>/.well-known/openpgpkey/hu/…` only if the `openpgpkey` subdomain does not exist. Binary key response.
  - keys.openpgp.org: `GET /vks/v1/by-email/<address>` (verified addresses only; 1 request/min per address, burst 50).
  - Header protection RFC 9788 (Aug 2025): protects Subject inside the signed/encrypted part (`hp="cipher"`, outer Subject "[...]"); RFC 9787 gives MUA guidance.
- **Alice today:** Nothing. Sync downloads only text and calendar parts (`crates/alice-core/src/mime.rs:316-324`); an encrypted message's `application/octet-stream` part becomes an attachment. Raw headers are fetched (`remote.rs:168-198`, `BODY.PEEK[HEADER]`) but only a few fields are kept (`providers.rs:340-413`), so `Autocrypt` headers are dropped. Whole messages can be fetched up to 30 MB (`remote.rs:285-306`). Sending builds a single `text/plain` part with lettre (`providers.rs:193-211`). Secrets live in the OS credential store (`crates/alice-core/src/vault.rs`).
- **Plan:**
  1. Crate: **rPGP** (`pgp` 0.21, MIT/Apache-2.0, pure Rust, RFC 9580 v4+v6, Autocrypt 1.1, used by Delta Chat; NLnet-funded audit by Radically Open Security in Dec 2024, CVE-2024-53856/53857 fixed). Sequoia (`sequoia-openpgp` 2.4.1) is **LGPL-2.0-or-later, which is compatible with Alice's AGPL-3.0-only** (LGPL 2.1+/3 into (A)GPLv3 is allowed per the FSF FAQ), but its default Nettle backend is a C library that complicates Windows builds, and `sequoia-net` (WKD) pulls in reqwest 0.12 and native-tls beside Alice's reqwest 0.13. WKD and Autocrypt are small enough to write directly. New deps: `pgp`, and `sha1` for the WKD hash (z-base-32 is a 15-line function).
  2. Keys (`crates/alice-core/src/pgp.rs`, new): generate Ed25519/X25519 per account, import armored public/secret keys (passphrase prompted, never stored unless asked), export public key. Store public keys and Autocrypt peer state in new tables `pgp_keys(fingerprint, account_id NULL, data, is_secret)` and `autocrypt_peers(addr, last_seen, autocrypt_timestamp, public_key, prefer_encrypt, gossip_timestamp, gossip_key)`. Keep secret keys in a file encrypted with a random key stored in the vault: Windows Credential Manager limits a secret to 2,560 bytes, too small for an armored RSA-4096 secret key.
  3. Receive: in `providers::message_from` read `Autocrypt` (and `Autocrypt-Gossip` after decryption) into the peers table. Add `Message.crypto: Option<Crypto { encrypted, signature: Option<SignatureStatus{ valid, signer, fingerprint, key_known }> }>`. For `multipart/encrypted` and `multipart/signed` roots, fetch the whole message (`remote.rs` `whole()`), because verification needs the exact bytes of the signed part including its MIME headers. Decrypt on open, not at sync: keep the ciphertext in the cache, keep plaintext in memory only, and do not put it in the FTS index unless the user opts in. Also handle inline PGP (`-----BEGIN PGP MESSAGE-----` / `SIGNED MESSAGE` in `text/plain`).
  4. Send: build the inner MIME entity with lettre, take its bytes (`MultiPart::formatted()`), sign/encrypt with rPGP, and wrap with `MultiPart::signed(...)` / `MultiPart::encrypted(...)` (both exist in lettre 0.11). Add `Autocrypt:` to every outgoing message from an account that has a key. Encrypt to all recipients plus the sender's own key so Sent stays readable.
  5. Lookup: on adding a recipient without a key, try local peers, then WKD, then keys.openpgp.org, with a 5-second timeout and a cache of misses; show a lock state per recipient in the composer and the overall Autocrypt recommendation (encrypt/available/discourage/disable).
  6. UI: Settings → Accounts → "End-to-end encryption" (generate/import/export, prefer-encrypt toggle); composer encrypt/sign toggles; reader banner "Encrypted" / "Signed by <name> (verified|unknown key|bad signature)"; key details sheet.
  7. AI and MCP: decrypted content never goes to the AI provider or MCP clients unless the user explicitly asks for that message (extend the OTP-style guard in `ai.rs:149-156`).
- **Pitfalls:** EFAIL (2018): never merge decrypted and unencrypted parts into one HTML document, and do not load remote content in decrypted mail. Show "signed" only when the whole displayed body is covered by the signature; partially signed mail (mailing-list footers) must not show a green state. Mailing lists and some gateways re-encode bodies and break signatures; canonicalize CRLF before verifying. Subjects leak unless header protection is used; send `hp="cipher"` only after testing with Thunderbird. Autocrypt Setup Messages have published forgery issues (IACR ePrint 2024/918); do not import them automatically. Expired, revoked and SHA-1-only keys need clear errors. Search over encrypted mail is a product decision.
- **Done when:** Alice can exchange signed and encrypted mail with Thunderbird both ways (PGP/MIME and inline), shows correct verified/unknown/bad states, picks up a correspondent's key from the Autocrypt header of their first message, and finds a key via WKD for a test domain. Tests: rPGP round-trip unit tests in `pgp.rs`; fixture messages (Thunderbird-signed, -encrypted, inline, tampered body) parsed through `parse_message`; a WKD URL test against draft examples (`Joe.Doe@Example.ORG` → `iy9q119eutrkn8s1mk4r39qejnbu3n5q`); the Autocrypt recommendation table as a pure-function test.
- **Effort:** XL. Depends on "Rich text / HTML compose..." (for HTML in signed mail), "Attachment upload..." (key attachment), "Correct MIME for replies..." and "Cc and Bcc fields" (encrypting to all recipients).

<a id="smime-sign-encrypt-verify-with-system-certificates"></a>

### S/MIME: sign, encrypt, verify with system certificates

*Roadmap: P3*

- **Why:** Many companies, universities and governments issue S/MIME certificates and require signed or encrypted mail; Outlook and Apple Mail users expect it (Mailspring GitHub #28, #940).
- **Competitors:** Mailspring has none. Apple Mail uses the login keychain: a personal certificate enables the sign button; encryption needs every recipient's certificate, which Mail collects from signed mail it receives ([Apple](https://support.apple.com/guide/mail/mlhlp1180/mac)). Outlook uses the Windows certificate store and can fetch recipient certificates from the Global Address List. Thunderbird keeps its own NSS store: Account Settings → End-To-End Encryption → Manage S/MIME Certificates → Import (.p12), smartcards via PKCS#11.
- **Spec:** RFC 8551 (S/MIME 4.0): `multipart/signed; protocol="application/pkcs7-signature"; micalg=sha-256` (`smime.p7s`) or opaque `application/pkcs7-mime; smime-type=signed-data`; `application/pkcs7-mime; smime-type=enveloped-data | authEnveloped-data` (`smime.p7m`). Content encryption MUST AES-128/256-GCM (AuthEnvelopedData), MUST- AES-128-CBC; signatures RSA PKCS#1 v1.5 + SHA-256 (MUST-), ECDSA P-256, Ed25519 (receive). Certificates: RFC 8550. Header protection: RFC 9788.
- **Alice today:** Nothing; same MIME limits as OpenPGP above. `native-tls` uses SChannel (Windows), Secure Transport via `security-framework` (macOS) and OpenSSL (Linux), so OpenSSL is only already linked on Linux.
- **Plan:**
  1. Phase 1, cross-platform with the `openssl` crate (0.10): `CmsContentInfo::verify/decrypt/encrypt/sign` and `Pkcs7` cover all four operations; OpenSSL 3 produces AuthEnvelopedData when given a GCM cipher. Use `openssl` with the `vendored` feature on macOS/Windows. The RustCrypto `cms` crate has builders but no verify/decrypt helpers, so it would mean writing CMS processing by hand.
  2. Trust: build an `X509Store` from the OS roots (`rustls-native-certs` returns them as DER on all three platforms), verify signer chain, check that the signer's email (SAN rfc822Name) matches From, report expired/untrusted/mismatch separately.
  3. Keys: import a `.p12` per account (password prompted), store it encrypted on disk with the wrapping key in the vault (same pattern as OpenPGP secret keys). Phase 2: use non-exportable identities from the macOS keychain (`security-framework` `SecIdentity`/`SecKey::create_signature`/`decrypt_data`, or its `CMSEncoder`/`CMSDecoder`) and the Windows `MY` store (`schannel::CertStore::open_current_user("MY")` plus NCrypt through the `windows` crate). That is what "system certificates" means to Outlook and Apple Mail users.
  4. Recipient certificates: harvest from every valid signed message into `smime_certs(email, der, not_after, fingerprint)`; later from LDAP/GAL `userCertificate` (see the LDAP item).
  5. Message model, sending and UI: share the `Message.crypto` status, the whole-message fetch for signed/encrypted roots, the lettre `MultiPart::signed/encrypted` wrappers and the composer/reader indicators with the OpenPGP item. One "Security" control per draft chooses PGP or S/MIME.
- **Pitfalls:** Opaque signed mail (`smime.p7m` signed-data) must be unwrapped to show anything. Exchange and some gateways rewrite MIME and break `multipart/signed`. Prefer AES-256-CBC EnvelopedData for sending unless the recipient is known to handle AuthEnvelopedData, since older clients fail on GCM; accept both on receive. The Graph API caps S/MIME payloads at 4 MB. Vendored OpenSSL needs security updates in Alice's own release cadence.
- **Done when:** A message signed by Apple Mail or Outlook verifies with the right signer and trust state; Alice-signed and Alice-encrypted mail opens in Thunderbird and Apple Mail; a tampered body shows "invalid". Tests: fixture `.eml` files (detached-signed, opaque-signed, enveloped CBC, authEnveloped GCM, expired cert, From mismatch) verified in unit tests with a test CA generated at build time.
- **Effort:** XL (phase 1 L). Shares most UI and MIME plumbing with the OpenPGP item; do them together.

<a id="read-receipts-request-and-respond-to-disposition-notification-to"></a>

### Read receipts: request and respond to `Disposition-Notification-To`

*Roadmap: P3*

- **Why:** Workplace users (Outlook-heavy organisations) send and expect read receipts. Answering them safely, and on the user's terms, matters more than requesting them.
- **Competitors:** Mailspring's "read receipts" are its tracking-pixel open tracking (see "Open and link tracking for outgoing mail"); it does not implement MDN at all. Thunderbird: Settings → Return Receipts; requests are off by default; answering is "Allow for some messages" with Never/Always/Ask per case (not in To/Cc, sender outside my domain, all other), all defaulting to Ask (`mailnews.js`, `mail.mdn.report.*`). Outlook (classic and new): Always / Never / Ask each time under Mail → Message handling. Gmail offers read receipts only to Workspace accounts, controlled by the admin.
- **Spec:** RFC 8098 (MDN, STD 85): request header `Disposition-Notification-To`; response `multipart/report; report-type=disposition-notification` with a human-readable part, a `message/disposition-notification` part (`Reporting-UA`, `Final-Recipient`, `Original-Message-ID`, `Disposition: manual-action/MDN-sent-manually; displayed`) and optionally the original headers. Must not be sent automatically if the address differs from `Return-Path`, if `Return-Path` is missing, or if several addresses are listed; never answer an MDN; at most one MDN per message; default should be not to send. RFC 3503: `$MDNSent` IMAP keyword, set when a receipt is sent *or declined*, never cleared; only usable if `PERMANENTFLAGS` includes `\*` or `$MDNSent`.
- **Alice today:** Nothing. Headers are parsed only in `providers::message_from` (`crates/alice-core/src/providers.rs:340-413`); flags read are only `\Seen` and `\Flagged` (`remote.rs:419-425`); flag writes are `+FLAGS.SILENT` in `providers::modify` (`providers.rs:98-111`). SMTP sends a lettre `Message` (`providers.rs:213-237`).
- **Plan:**
  1. Parse `Disposition-Notification-To` and `Return-Path` in `message_from` into `Message.receipt_request: Option<String>` (only when exactly one address), and read `$MDNSent` in `remote_flags` into `Message.receipt_handled: bool`.
  2. Preference `read_receipts: Never | Ask | Always` (default `Ask`, as Thunderbird), where `Always` still asks when the address differs from `Return-Path`, the sender is outside your domain, you are not in To/Cc, or the message is in Spam.
  3. Reader: a bar "Ana asked for a read receipt — Send / Don't send". Either choice stores `$MDNSent` (`UID STORE +FLAGS.SILENT ($MDNSent)`); if the server cannot store it, do not offer sending.
  4. Send: lettre has no `multipart/report` kind, so build the report bytes by hand (three parts, CRLF) and send with `Transport::send_raw(&Envelope, &bytes)` through a `providers::deliver_raw` beside `deliver`. Do not save MDNs to Sent.
  5. Request: a composer toggle "Request read receipt" adds `Disposition-Notification-To: <from>` with `MessageBuilder::raw_header`. Incoming reports (`report-type=disposition-notification`) are matched by `Original-Message-ID` to the sent message and shown as "Read by Ana, 14:02" instead of as a normal message.
- **Pitfalls:** Read receipts reveal when you read mail; the default must never send silently. Mailing-list mail and Spam must never trigger receipts. Gmail consumer accounts can still answer MDNs, since an MDN is ordinary mail. Some servers don't allow custom keywords; then Alice must remember handled requests locally.
- **Done when:** A Thunderbird-sent message with a receipt request shows the bar once; choosing Send produces an MDN that Thunderbird recognises as a receipt, and the bar does not reappear on another Alice device (`$MDNSent`). Unit tests: header parsing including a `Return-Path` mismatch, MDN byte layout against RFC 8098 §9 examples, and "never answer an MDN".
- **Effort:** M. Depends on "Cc and Bcc fields" for the To/Cc check; otherwise independent.

<a id="translation-of-incoming-mail-using-the-byok-ai-provider"></a>

### Translation of incoming mail, using the BYOK AI provider

*Roadmap: P3*

- **Why:** People who get mail in languages they don't read can understand it in place, without pasting it into a web translator (which leaks the mail to yet another service).
- **Competitors:** Mailspring 1.7.0 added "Incoming Message Translation" (`app/internal_packages/translation/`). `lib/message-header.tsx` waits 1 s after a message mounts, takes the first 1,000 characters of the rendered iframe's `innerText`, skips messages from you and texts under 50 characters ("language detection seems unreliable for very short emails"), and detects the language with Chromium's CLD via `chrome.i18n.detectLanguage` (a bundled extension, `app/static/extensions/chrome-i18n/main.js`). If it differs from the UI locale it shows a bar offering to translate, with "Never translate <language>" (`core.translation.disabled`) and "Always translate" (`core.translation.automatic`, Pro only). `lib/service.ts` walks text nodes (skipping `<style>`, `.notranslate`, bare URLs, US-style addresses, monospace fonts and text under 12 px), wraps each in `<b>` and posts at most ~5,000 characters per email to Mailspring's server (`/api/translate`, Yandex at launch). An LRU cache of 1,000 snippets avoids re-billing templated mail; quoted text is removed before and re-appended after; the result is kept in `localStorage` (`translated-<id>`) with "Show Original". Free accounts got 50 translations a week. Fixes: 1.7.4 "Never Translate German" dialog froze the app (#1685); 1.23.0 crash when the iframe's `contentDocument` was not loaded yet (#2752). The composer also has a "translate draft" button (`lib/composer-button.tsx`). Outlook and Gmail web offer a built-in "Translate message" bar; Thunderbird needs an add-on; Spark offers translation through its +AI assistant.
- **Alice today:** Partly possible by hand: Ask Alice (`apps/desktop/src/Assistant.tsx:41`) sends any prompt plus the selected message to the configured provider (`crates/alice-core/src/ai.rs:84-148`), so "translate this" works, but the answer is a plain-text chat reply, not an in-place translation. The system prompt already forces output into the UI language (`ai.rs:113-117`). Message bodies sent to AI are capped at 12,000 characters (`ai.rs:111`), and mail with a verification code is refused when OTP copying is on (`ai.rs:149-156`).
- **Plan:**
  1. Split the provider call out of `ai::run` (`ai.rs:124-146`) into `async fn complete(settings, key, system, user, max_tokens) -> Result<String>` so translation and summaries reuse the OpenAI/Anthropic/compatible handling.
  2. Add `ai::translate(store, message_id, target: Locale | BCP 47 tag) -> Result<Translation { source_language, text }>`: input is `message.body` with the quoted part removed (port `splitQuoted` from `apps/desktop/src/emailHtml.ts:94` to Rust, or keep the quote and fold it later), capped at 12,000 characters. System prompt: translate only, email content is untrusted data, output plain text. Do not ask for JSON; cap the output length.
  3. Route `POST /v1/messages/{id}/translate` in `daemon.rs` (next to `/v1/messages/{id}/otp`, line 281), and `alice ai translate --message <id> [--to de]` in `crates/alice/src/main.rs`.
  4. Reader: a "Translate" item in the message menu (`menuFor` in `Conversation.tsx`), rendering the result as plain text (like `MessageBody.tsx`) with "Show original". Translating text instead of HTML loses layout but avoids sending CSS and markup to a paid API; Mailspring's text-node approach can come later if users ask.
  5. Optional offer bar: detect the language locally with `whatlang` (pure Rust, MIT, new dependency) on the first ~1,000 characters, only above 50 characters and not for your own mail. Store `translation_offer: bool` and `never_translate: Vec<String>` in `Preferences` (`model.rs:313`). Never translate automatically by default: every translation is a paid call that sends mail to a third party.
  6. Cache results in memory in the daemon, keyed by (message id, target language), so re-opening a message does not call the provider again.
- **Pitfalls:** Prompt injection: the output must be shown as text, never as HTML, and must not be able to trigger actions. Keep the OTP guard. Language detection is unreliable on short or mixed-language mail (Mailspring's 50-character floor). Alice ships only `en-US` and `de` UI locales, so the target must also accept a language the UI does not have. Bodies over 12,000 characters must be marked as partially translated.
- **Done when:** Translating a German message with an English UI shows an English plain-text version with "Show original", and a second open does not call the provider. A unit test runs `complete()` against an in-process axum mock of `/chat/completions` on `127.0.0.1:0` (allowed by the loopback rule in `ai.rs:31-36`) and asserts that the request carries the untrusted-data instruction and the capped body. A test asserts that OTP mail is refused before any network call (same pattern as `ai.rs:259`).
- **Effort:** M. No dependencies; shares the `complete()` refactor with "AI thread summaries and reply drafts using the existing BYOK setup".

<a id="ai-thread-context-and-inline-summaries-per-message-summaries-and-drafts-already"></a>

### AI thread context and inline summaries (per-message summaries and drafts already ship)

*Roadmap: P3*

- **Why:** Catching up on a long conversation in one paragraph, and starting a reply from a draft that already knows the thread, is the most-used AI feature in Superhuman and Spark.
- **Competitors:** Mailspring has no AI summaries or drafts (its `mcp-server` package, added in 1.23.0, exposes mail to external agents instead). Superhuman's "Auto Summarize" shows a one-line summary above every conversation that updates as new mail arrives, and "Instant Reply" offers draft replies; Spark +AI summarizes and drafts on request. Apple Mail (Apple Intelligence) and Gmail (Gemini) show a summary card at the top of a message or thread.
- **Alice today:** Mostly there for single messages. Ask Alice has "Summarize" and "Draft reply" quick prompts (`apps/desktop/src/Assistant.tsx:56`) that send the selected message only (`AiRequest { prompt, message_id }`, `crates/alice-core/src/model.rs:427-430`); a proposed draft opens in the composer (`Assistant.tsx:59`, `onDraft`) and needs an explicit send. The whole conversation is available as `Store::conversation` (`daemon.rs:407-409`) but is not passed to AI.
- **Note:** This item should be narrowed to "thread context and an inline summary": per-message summaries and drafts already ship.
- **Plan:**
  1. Add `thread_id: Option<String>` to `AiRequest`; in `ai::context_messages` (`ai.rs:149`) load `store.conversation(id)`, apply the same OTP filter per message, drop each message's quoted history, cap each body at ~4,000 characters and the total at ~40,000, oldest first, with from/date per message.
  2. `POST /v1/threads/{id}/summary`: returns `{ summary, newest_message_id }`, cached in memory (or a small `ai_summaries(thread_id, newest_message_id, locale, text)` table) so it is regenerated only when a new message arrives.
  3. Reader: a "Summarize" button in the conversation header (`Conversation.tsx`, beside the thread count); the summary card sits above the messages with a "Regenerate" action. Add a preference `ai_thread_summaries: Off | OnRequest | Automatic` (default on request) to `Preferences`.
  4. Reply drafts: the existing "Draft reply" uses the thread when one is open. Insert the draft into the reply composer (with the correct `In-Reply-To`/`References`) rather than a fresh compose.
  5. CLI: `alice ai ask --thread <id>`.
- **Pitfalls:** Alice groups conversations by subject (README "Current boundaries"), so an unrelated message with the same subject would be summarized with the thread and sent to the provider; ship after header-based threading or limit context to messages linked by `In-Reply-To`. Automatic summaries bill on every open and send every opened thread to a third party; keep them off by default and never run them in Spam. Summaries go stale; key the cache by newest message id. Treat thread content as untrusted (the existing system prompt does).
- **Done when:** Opening a 10-message thread and choosing Summarize shows one summary that covers all messages, a second open hits the cache, and a new reply invalidates it. A unit test checks that `context_messages` for a thread drops OTP mail, caps sizes and preserves order; the mock-provider test from the translation item checks the request body.
- **Effort:** M. Depends on "Thread by `Message-ID` / `References` / `In-Reply-To`..." (P0) for correct context; shares the `complete()` refactor with translation.

<a id="grammar-check-optional-local-or-self-hosted-languagetool"></a>

### Grammar check (optional, local or self-hosted LanguageTool)

*Roadmap: P3*

- **Why:** Catches grammar and style mistakes before sending, for people writing in a second language in particular, without sending drafts to Grammarly.
- **Competitors:** Mailspring 1.18.0 added `internal_packages/composer-grammar-check/` (#2612). Drafts are checked block by block (only changed blocks, with `AbortController` for stale requests) against a LanguageTool deployment run by Mailspring (`POST /api/grammar/check` on id.getmailspring.com; "requests are not logged"), metered for free users. Spelling matches (`issueType === 'misspelling'`) are dropped because the system spellchecker already covers them. Settings: `core.composing.grammarCheck` (off by default, `app/src/config-schema.ts:203`), `grammarCheckLanguage` (default `auto`), `grammarCheckDisabledRules` (comma list, filled by "ignore this rule"). 1.19.0 moved the setting into General → Composer (#2639); 1.23.0 fixed a composer crash when decorations applied mid-typing (#2775) and warnings when logged out. LanguageTool's own browser add-on and Thunderbird add-on work against the public API or a custom server.
- **Spec:** LanguageTool HTTP API, `POST /v2/check` (form fields `text` or `data`, `language` incl. `auto`, `motherTongue`, `preferredVariants`, `disabledRules`, `enabledOnly`, `level=picky`, `username`+`apiKey` for Premium); response `matches[]` with `offset`, `length`, `message`, `replacements[].value`, `rule{id, issueType, category}` ([swagger](https://languagetool.org/http-api/languagetool-swagger.json)). The free public API allows 20 requests/min, 75 KB/min and 20 KB per request per IP and asks not to send automated requests ([public API](https://dev.languagetool.org/public-http-api)). Self-hosting: Java 8+, `java -cp languagetool-server.jar org.languagetool.server.HTTPServer --port 8081`, optional 8 GB n-gram data (en/de/fr/es); LGPL-2.1, v6.8; community Docker image `erikvl87/languagetool:6.8`.
- **Alice today:** Nothing. The composer is a plain-text editor (`apps/desktop/src/Compose.tsx`), which makes LanguageTool offsets map directly onto the text. The app CSP allows `connect-src` only to IPC (`apps/desktop/src-tauri/tauri.conf.json`), so the frontend cannot call a grammar server directly.
- **Plan:**
  1. Preferences: `grammar: { enabled: bool, server_url: String, language: "auto" | tag, mother_tongue: Option<String>, disabled_rules: Vec<String> }`; Premium `username`/`api_key` in the vault. URL rules as for AI endpoints (`ai.rs:30-43`): HTTPS, or HTTP on loopback for a local server.
  2. `POST /v1/grammar { text, language }` in `daemon.rs` forwards to `<server_url>/v2/check` with `http_client()` and returns trimmed matches. Drop `misspelling` matches unless the user asks for them.
  3. Composer: debounce ~1.5 s after typing stops; send only the user's own text (exclude the quoted reply and signature, which Alice inserts itself via `Preferences::draft_body`, `model.rs:362`), at most 20 KB per request. Show underlines with a mirrored backdrop `<div>` behind the textarea, and a popover with replacements and "Ignore rule".
  4. Offer the public `api.languagetool.org` only as an explicit choice with a note on its limits and privacy; default to no server.
  5. A cheaper alternative already exists: "Proofread" as an Ask Alice quick prompt through the BYOK provider. Add it regardless; it is a one-line prompt.
- **Pitfalls:** Offsets are UTF-16 code units in LanguageTool's Java server, while Rust strings are UTF-8; convert in the frontend, which also uses UTF-16. Responses for an old version of the text must be discarded (the user kept typing). Rate limits on the public server. Drafts may contain confidential text; make it clear where they are sent.
- **Done when:** With a local LanguageTool container configured, typing "This are a test" underlines "This are" within 2 s and applying the suggestion fixes it; disabled rules stay disabled after restart. Unit test: the daemon forwards form fields correctly to an in-process mock server and rejects non-HTTPS remote URLs. Frontend test for UTF-16 offset mapping with emoji before the error.
- **Effort:** M. Easier after "Rich text / HTML compose..." is designed, since the underline layer must work with whichever editor ships.

<a id="masked-address-generation-through-fastmail-simplelogin-or-addyio"></a>

### Masked-address generation through Fastmail, SimpleLogin or addy.io

*Roadmap: P3*

- **Why:** Giving each correspondent or service its own address, and turning off one that starts getting spam, is a strong privacy feature that people currently do in a separate app.
- **Competitors:** Mailspring has none. Apple Mail (iCloud+) offers Hide My Email in Safari, Mail and iCloud settings. Fastmail creates Masked Emails from its web app, 1Password and Bitwarden. Bitwarden's username generator supports SimpleLogin, addy.io, Firefox Relay, Fastmail, Forward Email and (through an undocumented token) DuckDuckGo ([Bitwarden](https://bitwarden.com/help/generator/)).
- **Spec:**
  - Fastmail Masked Email is a JMAP extension, capability and OAuth scope `https://www.fastmail.com/dev/maskedemail` ([fastmail.com/dev](https://www.fastmail.com/dev/)). `MaskedEmail/get` and `MaskedEmail/set`; fields `id`, `email`, `state` (`pending` → deleted after 24 h unless mail arrives, `enabled`, `disabled` = mail goes to Trash, `deleted` = bounces; never back to pending), `forDomain`, `description`, `url`, `emailPrefix` (create-only, ≤64 chars `[a-z0-9_]`), `createdBy`, `lastMessageAt`. Auth: API token (Settings → Privacy & Security → Manage API tokens, Masked Email scope) as Bearer, or OAuth (PKCE; needs client registration through Fastmail partnerships). JMAP session `https://api.fastmail.com/jmap/session`.
  - iCloud Hide My Email has **no public API**; only iCloud settings, icloud.com, Safari and Mail can create addresses.
  - SimpleLogin: `POST /api/alias/random/new` with header `Authentication: <api key>` ([docs](https://github.com/simple-login/app/blob/master/docs/api.md)). addy.io: `POST https://app.addy.io/api/v1/aliases` with Bearer token ([docs](https://app.addy.io/docs/)).
- **Alice today:** Nothing. Accounts are IMAP/SMTP or OAuth (`crates/alice-core/src/model.rs:12-29`); no JMAP client; sending is only from the account address (`providers.rs:194-197`).
- **Note:** Drop iCloud from this item (no API; at most link to iCloud settings) and add SimpleLogin/addy.io, which have documented APIs and are what many non-Fastmail users already use.
- **Plan:**
  1. A small `aliases.rs` with one function per service (`create(service, for_domain, description) -> String`, `set_state(id, enabled|disabled|deleted)`, `list()`), using `http_client()`. Fastmail needs only two JMAP calls, so `jmap-client` is not required for this item (but reuse it if the JMAP backend lands first).
  2. Settings → Privacy → "Email aliases": choose service, paste an API token (stored in the vault).
  3. Composer: "New alias for this conversation" next to From creates an alias (Fastmail `state: "enabled"` at creation so it is not deleted after 24 h), sets it as the sender and records it on the draft.
  4. Reader: when mail arrived at an alias (To/Delivered-To matches a known alias), show it with "Disable this alias" (Fastmail `disabled`, SimpleLogin toggle). This is the most useful mail-client action: cutting off a leaked address from the spam it receives.
  5. Cache the alias list in a `settings` row for matching, refreshed on demand.
- **Pitfalls:** Sending from an alias requires the provider to accept it as a sender (Fastmail does for masked addresses; SimpleLogin uses reverse-alias addresses for replies, so replies must go to the reverse alias, not the original sender). Rate limits on alias creation (`rateLimit` SetError). API tokens are powerful; store them only in the vault and scope them to masked email.
- **Done when:** With a Fastmail test token, creating an alias from the composer returns a working address, a reply sent from it arrives from that address, and "Disable" moves future mail to Trash. Unit tests mock the JMAP endpoint and check the `MaskedEmail/set` request shape and state transitions.
- **Effort:** M. Depends on "Send-as identities and aliases, including Gmail "Send mail as" import" for sending from the alias.

<a id="image-proxy-for-remote-images-to-hide-ip-and-open-time"></a>

### Image proxy for remote images to hide IP and open time

*Roadmap: P3*

- **Why:** Loading remote images tells senders your IP address (approximate location), your browser engine and the moment you opened the message. Most people still want newsletters with their images.
- **Competitors:** Mailspring has no proxy; it blocks images unless allowed (globally via `core.reading.autoloadImages`, never in Spam, or per sender via `autoload-images-whitelist.txt`, `internal_packages/message-autoload-images/lib/autoload-images-store.ts`). 1.7.5 also blocked external stylesheets and web fonts; 1.21.0 blocked CSS `@import` (#2706); 1.26.0 stopped fetching remote images on reply/forward when autoload is off (#2901). Apple Mail Privacy Protection downloads remote content in the background for every message, whether opened or not, through two relays run by different companies (the first sees the IP, the second the content) ([Apple](https://www.apple.com/legal/privacy/data/en/mail-privacy-protection/)). Gmail has proxied images since December 2013 (`googleusercontent.com/proxy/…`, generic User-Agent, transcoded, fetched on open, so open time still leaks) ([Valsorda](https://words.filippo.io/how-the-new-gmail-image-proxy-works-and-what-this-means-for-you/)). Fastmail proxies images in its web and mobile apps. Thunderbird blocks remote content with per-message/sender/site exceptions.
- **Alice today:** Remote images are blocked unless allowed per message or by `RemoteImages::Always` (`crates/alice-core/src/model.rs:272-277`). When allowed, the sandboxed `srcdoc` frame loads them directly from the webview (`apps/desktop/src/emailHtml.ts:63-79`, CSP `img-src data: https:`), with `no-referrer` (`emailHtml.ts:80`, `MessageView.tsx:104`) but with the user's IP, the webview's User-Agent and whatever cookie policy the webview applies. Embedded `cid:` images already travel as `data:` URLs fetched through Rust (`MessageView.tsx:9-49`), which is the mechanism to reuse.
- **Plan:**
  1. `crates/alice-core/src/images.rs` (new): `fetch_remote_image(url) -> (content_type, bytes)` with a dedicated reqwest client: no cookies, fixed generic User-Agent, no Referer, `Accept: image/*`, HTTPS only, at most 3 redirects each re-validated, 10 s timeout, 5 MB cap while streaming, and only `image/png|jpeg|gif|webp` confirmed by magic bytes (no SVG). Refuse hosts that resolve to loopback, private, link-local or unique-local addresses; otherwise an email could make Alice probe `127.0.0.1:4815` or the home router.
  2. `POST /v1/messages/{id}/remote-image { url }` that only fetches URLs present in that message's `html_body`, so the endpoint cannot be used as a general fetcher. Tauri command `read_remote_image` beside `downloads::read_attachment`.
  3. `MessageView.tsx`: when images are allowed and the new preference `remote_images_via: Direct | Proxy` is `Proxy` (default), collect `img[src^="https:"]` like `useEmbedded`, fetch through the daemon, and pass a `url → data:` map into `emailDocument` alongside `embedded`; the CSP stays `img-src data:`. CSS `background` images stay blocked in proxy mode.
  4. Hiding the open time needs prefetching: a third mode `Prefetch` downloads remote images for new Inbox mail during sync into a size-capped cache under the data dir (counted in `/v1/storage`), never for Spam. This makes every message look "opened" at arrival, as MPP does.
  5. Hiding the IP needs a relay Alice does not run: route `images.rs` through the user's proxy or Tor (`socks5h://127.0.0.1:9050`) from "Proxy support (SOCKS5 / HTTP)...", with a separate "Use for images only" toggle.
- **Pitfalls:** SSRF (above) is the main security risk, including DNS rebinding: resolve once and connect to the checked address. Decompression bombs: cap bytes, and consider capping GIF dimensions. Per-recipient unique image URLs make prefetching equivalent to "opened"; combine with tracker stripping so known pixels are never fetched. Cached images are personal data; delete them with the message and when the account is removed.
- **Done when:** With proxy mode on, a packet capture shows no webview connections to image hosts, newsletters render with images, an email with `<img src="https://127.0.0.1:4815/v1/state">` or a private-range host fetches nothing, and prefetch mode loads images before the message is opened. Tests: Rust unit tests for URL/IP validation and size/type limits against an in-process server; a `scripts/test-html.mjs` case that proxied URLs become `data:` URLs while the CSP has no `https:`.
- **Effort:** M for proxy-on-open, L with prefetch and cache. Depends on "Never auto-load images in Spam" and "Block remote CSS and web fonts along with images" (P1); IP hiding depends on "Proxy support (SOCKS5 / HTTP) for IMAP, SMTP and HTTP APIs".

<a id="tracking-pixel-detection-and-stripping-with-a-trackers-blocked-indicator"></a>

### Tracking-pixel detection and stripping, with a "trackers blocked" indicator

*Roadmap: P3*

- **Why:** Even people who load images don't want to report every open to marketing tools; showing "2 trackers blocked" makes the protection visible.
- **Note:** The `SP` tag looks wrong: Spark's tracking feature adds pixels to outgoing mail, and I found no recipient-side blocker. HEY and Proton Mail are better references.
- **Competitors:** Mailspring's `internal_packages/remove-tracking-pixels/lib/main.ts` holds a hard-coded list of 25 URL substrings (NGPVan, Sidekick `t.signaux`/`t.sidekickopen`, Banana Tag, Boomerang `mailstat.us/tr`, Cirrus Insight, Yesware, Streak `mailfoogae.appspot.com`, LaunchBit, MailChimp `list-manage.com/track`, Postmark `cmail1.com/t`, iContact, Infusionsoft, Intercom `via.intercom.io/o`, Mandrill, HubSpot `t.hsms06.com`, RelateIQ, RJ Metrics, Mixpanel, Front, Mailtrack `mailtrack.io/trace`, Salesloft). Notably it strips these **only from messages you sent** (so viewing your own sent mail doesn't trigger your own tracking), plus Mailspring's own read-receipt pixels from any message; incoming third-party trackers are left alone and are only stopped by image blocking. It also runs on new drafts so quoted text doesn't carry pixels. Forum #828 "Block email tracking attempts" (3 votes). Simplify Gmail uses the maintained `leggett/simplify-trackers` list (BSD-3-Clause); Trocker (Apache-2.0) does the same in browsers; DuckDuckGo Email Protection strips known trackers and reports which ones it removed. HEY strips known spy pixels and tells you who placed them ([hey.com/spy-trackers](https://www.hey.com/spy-trackers/)); Proton Mail removes known pixels, proxies remote images, cleans tracking links and shows a tracker icon with a count ([Proton](https://proton.me/blog/how-to-stop-email-trackers)). I found no tracker blocking in Spark; its Pro "Read Statuses" feature adds pixels to outgoing mail.
- **Alice today:** No detection. With images blocked (default `RemoteImages::Ask`) pixels don't fire; with images allowed they do. `emailDocument` already iterates every `<img>` (`apps/desktop/src/emailHtml.ts:64-76`) and returns `remoteImages`, so detection fits there.
- **Plan:**
  1. Vendor a tracker pattern list as JSON (`apps/desktop/src/trackers.json`, from `leggett/simplify-trackers` with its BSD notice, plus Mailspring's patterns), refreshed at release time, never fetched at runtime.
  2. In `emailDocument`'s image loop, mark an image as a tracker if its URL matches the list, or if it is remote and declared ≤2×2 px by attributes or inline style, or hidden (`display:none`, `visibility:hidden`, `opacity:0`). Remove trackers from the document even when images are loaded, and return `trackers: string[]` (service names, "unknown" for heuristics).
  3. Reader: a shield in the `html-controls` bar (`MessageView.tsx:104`) "Blocked 2 trackers: Mailchimp, HubSpot", with the list in a popover.
  4. Run the same check in Rust at sync time (`include_str!` the same JSON) to store `Message.trackers: u8` so the thread list can show the shield.
  5. When HTML replies and forwards exist, strip trackers from quoted HTML (Mailspring's `prepareNewDraft`).
- **Pitfalls:** Many ESPs serve the pixel from the same host as real images, so patterns must include paths. Unique per-recipient URLs on normal images still reveal opens; the indicator should say "known trackers blocked", not "you are not tracked" (the image proxy item covers the rest). Removing 1×1 spacer GIFs is harmless visually. Tracking links (click redirects) are a separate problem; do not rewrite links.
- **Done when:** A Mailchimp newsletter fixture shows "1 tracker blocked" and makes no request to `list-manage.com/track` even with images allowed, while its content images still load. `scripts/test-html.mjs` cases: list match, 1×1 attribute, hidden style, normal image kept, tracker count; a Rust unit test that the sync-time count matches.
- **Effort:** S (reader only), M with the list badge.

<a id="linux-packaging-flatpak-snap-deb-rpm-appstream-metadata-wayland-and-tray-fixes"></a>

### Linux packaging: Flatpak, Snap, deb, rpm, AppStream metadata; Wayland and tray fixes

*Roadmap: P3*

- **Why:** Linux users install from Flathub, their distro's format or the Snap Store; a single `.deb` built on Ubuntu 24.04 reaches only part of them.
- **Competitors:** Mailspring ships `.deb`, `.rpm`, Snap (`snap/snapcraft.yaml`: `base: core24`, strict, `gnome` extension, LZO compression, libsecret 0.20.5 built from source, `password-manager-service` plug, `TMPDIR=$XDG_RUNTIME_DIR` "to ensure libappindicator has readable resources") and a Flathub Flatpak, with AppStream metadata at `usr/share/metainfo/mailspring.metainfo.xml`. Packaging fixes over time: 1.12.0/1.13.1 Snap lost users' passwords on upgrade; 1.16.0 Snap core24 (#2497), RPM `lsb-core` made optional (#2503); 1.15.1 RPM build-id links clashing with other Electron apps; 1.17.3 `.deb` dependencies for Ubuntu 25 and Mint 22 (t64 library renames) plus install checks on those distros in CI; 1.17.4 RPM dependencies for Fedora 43/openSUSE (#2590, #2591); 1.24.0 Flatpak moved to the Secret portal (users had to sign in again), autostart via portal in Flatpak (#2838), tray in Snap/Flatpak (#2863, #2825). Wayland: 1.17.0–1.17.4 activation fixes (#2599); 1.18.0 `--background` and a composer keyboard freeze (#2622, #2623); 1.19.1 Wayland detection with a fallback menu button where there is no tray (#2647); 1.20.0 Wayland `app_id`.
- **Spec:** Tauri [Debian](https://v2.tauri.app/distribute/debian/), [RPM](https://v2.tauri.app/distribute/rpm/), [AppImage](https://v2.tauri.app/distribute/appimage/), [Flatpak](https://v2.tauri.app/distribute/flatpak/), [Snap](https://v2.tauri.app/distribute/snapcraft/) guides; Flathub [requirements](https://docs.flathub.org/docs/for-app-authors/requirements), [MetaInfo guidelines](https://docs.flathub.org/docs/for-app-authors/metainfo-guidelines), [linter](https://docs.flathub.org/docs/for-app-authors/linter); `org.freedesktop.portal.Secret`.
- **Alice today:** CI builds only a `.deb` on `ubuntu-24.04` (`.github/workflows/ci.yml:82-83`, `:166`) with `libwebkit2gtk-4.1-dev` and `libayatana-appindicator3-dev` (`ci.yml:56`). `tauri.conf.json` has `"targets": "all"` and no `bundle.linux` section; no metainfo, Flatpak or Snap files. Credentials use `keyring` 3 with `sync-secret-service` (`crates/alice-core/Cargo.toml`), i.e. D-Bus Secret Service. The tray uses Tauri's appindicator backend (`desktop.rs:334-363`).
- **Plan:**
  1. deb/rpm: build on `ubuntu-22.04` (oldest with WebKitGTK 4.1) so the binary's GLIBC works on Ubuntu 22.04 and Debian 12; add `rpm` and `appimage` to the Linux bundle matrix; set `bundle.linux.deb.section = "mail"`. Add install-and-launch smoke jobs in `ubuntu:22.04`, `debian:12` and `fedora:latest` containers under Xvfb (`WEBKIT_DISABLE_COMPOSITING_MODE=1` is needed there), checking that the daemon answers `/v1/health`.
  2. AppStream: `apps/desktop/src-tauri/linux/email.getalice.alice.metainfo.xml` (id = Tauri identifier, `launchable type="desktop-id"`, `developer id=`, `project_license` AGPL-3.0-only, OARS rating, releases, screenshots pinned to a tag), installed through `bundle.linux.deb.files`/`rpm.files` into `/usr/share/metainfo/`; `appstreamcli validate` in CI (warnings are fatal on Flathub).
  3. Flatpak (separate `flathub/email.getalice.alice` repo): build from source offline with `flatpak-cargo-generator.py` (Cargo.lock) and `flatpak-node-generator npm` (package-lock.json); runtime `org.gnome.Platform` **51**; finish-args `--share=network`, `--share=ipc`, `--socket=wayland`, `--socket=fallback-x11`, `--device=dri`, `--talk-name=org.kde.StatusNotifierWatcher`, `--filesystem=xdg-run/tray-icon:create`, `--filesystem=xdg-download` (attachments are saved to Downloads). Secrets: Flathub requires portals where they exist, so use the Secret portal (the `oo7` crate's sandboxed file backend) in Flatpak builds instead of `org.freedesktop.secrets`; keyring 4.x splits stores into crates, which makes a per-build store choice easy.
  4. Snap: `snap/snapcraft.yaml` modelled on Tauri's guide and Mailspring's file (core24, `gnome` extension, stage `libwebkit2gtk-4.1-0` and `libayatana-appindicator3-1`, `password-manager-service` plug, which needs a Snap Store auto-connect request).
  5. Wayland: document `WEBKIT_DISABLE_DMABUF_RENDERER=1` for Nvidia blank windows ([tauri#9304](https://github.com/tauri-apps/tauri/issues/9304), [#10702](https://github.com/tauri-apps/tauri/issues/10702)) and consider setting it automatically when the proprietary Nvidia driver is loaded; make sure the Wayland `app_id` matches the `.desktop` file name so the dock shows the right icon.
- **Pitfalls:** GNOME removed WebKitGTK 4.1 (GTK3) from the Flatpak SDK on master (2026-09-28); runtime 51 is the last with it and is supported until September 2027, so Alice needs Tauri's GTK4/WebKitGTK 6.0 port (open [tauri#14684](https://github.com/tauri-apps/tauri/pull/14684); Tauri 3 alphas have a `gtk4` feature) before then. Inside Flatpak the data dir is `~/.var/app/email.getalice.alice/data/Alice`, so a host-installed `alice` CLI would use a different store and fight the Flatpak daemon for port 4815: ship the CLI inside the Flatpak (`flatpak run --command=alice …`) and document it. Moving credentials to the portal makes existing Flatpak users sign in again (Mailspring 1.24.0). The AppImage hook forces `GDK_BACKEND=x11` when unset and crashes without XWayland ([#15902](https://github.com/tauri-apps/tauri/issues/15902)); tauri-cli 2.12's feature-merging regression ([#16250](https://github.com/tauri-apps/tauri/issues/16250)) breaks Linux bundles if `tray-icon` is made target-specific. If `libayatana-appindicator3` is missing, Tauri panics at tray creation. Flathub does not accept AI-generated manifest content and requires AI-generated code to be disclosed.
- **Done when:** CI publishes `.deb`, `.rpm` and AppImage that install and start in the three containers; metainfo validates; a Flatpak builds offline from the manifest, passes `flatpak-builder-lint`, shows the tray on KDE and GNOME (with the AppIndicator extension), keeps the sign-in across restarts via the portal, and saves attachments to Downloads.
- **Effort:** L. Related: "Windows jump list and Linux tray icon theme", "Launch at login (... XDG autostart / Flatpak portal)".

<a id="windows-jump-list-and-linux-tray-icon-theme"></a>

### Windows jump list and Linux tray icon theme

*Roadmap: P3*

- **Why:** Right-clicking the taskbar icon to compose is standard on Windows; on Linux a full-colour app icon in a monochrome panel looks wrong and is hard to see on dark panels.
- **Competitors:** Mailspring 1.17.4 (#2594) added `app/src/browser/windows-taskbar-manager.ts`: `app.setJumpList` with "Compose New Message" (`mailto:`), "Inbox" (`mailspring://open-inbox`) and "Preferences" (`mailspring://open-preferences`), routed through the existing protocol handler so they work whether or not the app is running; the same file adds the taskbar overlay unread badge and flashing on new mail. Linux tray: 1.17.4 dark/light tray icons (#2602) and a fix for the icon showing as "•••" at launch on Ubuntu 25; 1.18.0 fixed theme detection on GNOME/Unity (#2610); 1.19.0 added a tray icon theme picker and a "none" style (#2628). Settings are `core.workspace.trayIconStyle` (`blue`/`red`/`none`) and `core.workspace.traySystemTheme` (`automatic`/`light`/`dark`); "automatic" picks the light icon on GNOME/Unity because their top bar is always dark (`internal_packages/system-tray/lib/system-tray-icon-store.ts:81-99`). Icons change for inbox-zero/unread/new states. 1.24.0 fixed the tray not appearing when Mailspring starts before the tray host and inside Snap/Flatpak (#2863, #2825); 1.24.1 shipped multi-size `.ico` tray icons so Windows does not blur them at 125%/150%.
- **Alice today:** The tray exists (`apps/desktop/src-tauri/src/desktop.rs:285-363`) with Open/Sync/Quit. macOS uses a template image (`icons/tray.png`, `desktop.rs:352-355`); Windows and Linux reuse the full-colour window icon (`desktop.rs:356-360`). There is no unread state in the tray, no jump list, and no single-instance or deep-link handling (`main.rs:174-197` registers no plugins).
- **Plan:**
  1. Linux: ship monochrome tray PNGs (`icons/tray-light.png`, `icons/tray-dark.png`, 22–24 px with padding, plus unread variants), choose by a `tray_icon: Auto | Light | Dark` preference, with "Auto" following Mailspring's rule (light icon on GNOME/Unity, otherwise `prefers-color-scheme`/GTK theme). Switch the icon with `TrayIcon::set_icon` when unread state changes.
  2. Windows: ship a multi-size `.ico` for the tray (16/20/24/32 px), as Mailspring 1.24.1 did.
  3. Jump list: Tauri has no API for it (feature request [tauri#14699](https://github.com/tauri-apps/tauri/issues/14699) is open; the only crate, `jumplist_win` 0.0.4, is tiny and stale). Call `ICustomDestinationList` through the `windows` crate (0.62, `Win32_UI_Shell`, `Win32_System_Com`, `Win32_UI_Shell_PropertiesSystem`; Alice only has `windows-sys` today): `BeginList` → `AddUserTasks(IObjectArray of IShellLink)` → `CommitList`, with tasks "New message" (`Alice.exe --compose`) and "Inbox" (`Alice.exe --open inbox`), once at startup.
  4. Handle those arguments in the running instance with `tauri-plugin-single-instance` (2.5.x; must be registered before every other plugin), whose callback receives `(app, argv, cwd)`; the same plumbing serves "Register as `mailto:` handler...". On Linux it uses D-Bus (`org.<id>.SingleInstance`), which needs `--own-name` in Flatpak.
  5. Later: tray-icon 0.26 (newer than the 0.25 Tauri 2.12 pins) adds `set_native_icon` with freedesktop icon names such as `mail-unread-symbolic`, which follow the desktop theme automatically; switch to it when Tauri exposes it instead of shipping PNG variants.
- **Pitfalls:** Tauri v2 on Linux delivers no tray click events (the icon only opens its menu, [tray-icon#104](https://github.com/tauri-apps/tray-icon/issues/104)), so every action must be in the menu; Alice already does this. On GNOME without the AppIndicator extension there is no tray at all; Alice already re-shows a hidden window when the tray is removed (`desktop.rs:309-320`), but the setting UI should say so. Snap/Flatpak write the tray icon to a temp path the host cannot see (Mailspring's snap sets `TMPDIR=$XDG_RUNTIME_DIR` for this, `snap/snapcraft.yaml`). Jump-list tasks must use the installed executable path, which changes on update for per-user NSIS installs.
- **Done when:** On Ubuntu GNOME the tray icon is a crisp white glyph and changes when unread mail arrives; on Windows right-clicking the taskbar icon shows "New message", which opens the composer in the running instance. A unit test covers icon choice per desktop/theme preference.
- **Effort:** M. Depends on "Register as `mailto:` handler..." (single-instance plumbing) and "Dock / taskbar unread badge..." for unread state.

<a id="swipe-gestures-on-trackpads-with-an-option-to-disable"></a>

### Swipe gestures on trackpads, with an option to disable

*Roadmap: P3*

- **Why:** Two-finger swipe to archive is muscle memory for people coming from Apple Mail, Spark and Mailspring on Mac laptops.
- **Competitors:** Mailspring's `app/src/components/swipe-container.tsx` (418 lines) wraps each thread-list row. It reads `wheel` events and starts a gesture only when `|deltaX| / |deltaY| >= 1.3`, moves the row by `deltaX / 3` per event, predicts where it settles (`currentX + velocity * 5`) and fires the action past a 110 px threshold, then animates with a spring. It inverts direction when macOS "natural scrolling" is off (`defaults read -g com.apple.swipescrolldirection`). It knows the fingers lifted from Electron's `webContents.on('input-event')` `gestureScrollBegin`/`gestureScrollEnd` (`app/src/window-event-handler.ts:154-160`); touchscreens use touch events. In `internal_packages/thread-list/lib/thread-list.tsx:152-210`, swipe right archives or trashes (trash when `core.reading.backspaceDelete` is set) and swipe left opens the snooze popover. Changelog: 1.6.1 made it "less touchy" and ignore mostly vertical moves (#690); 1.19.0 tuned detection and added `core.reading.swipeDisabled` ("Disable swipe gestures on the thread list", #2629). A code comment says trackpad gestures are not supported on Windows. Apple Mail and Spark use swipe left/right with configurable actions.
- **Alice today:** Nothing. The thread list is rendered in `apps/desktop/src/App.tsx`; archive and trash already exist as keyboard shortcuts (`apps/desktop/src/shortcuts.ts:32-33`).
- **Plan:**
  1. A `useSwipe(row, { onLeft, onRight })` hook in a new `apps/desktop/src/swipe.ts`: a non-passive `wheel` listener on the list container that claims the gesture once the horizontal ratio passes 1.3, translates the row with a CSS transform, and `preventDefault()`s only after claiming so vertical scrolling is untouched.
  2. Tauri has no equivalent of Electron's gesture begin/end events (wry also turns off WebKit's own back/forward swipe, `back_forward_navigation_gestures: false`, so the wheel events reach the page), so treat ~150 ms without a wheel event as "fingers lifted" and settle then. macOS momentum `wheel` events keep arriving after the lift, so ignore events after the settle until a 150 ms gap.
  3. Actions: right = archive (or trash, following the same rule as the `e`/`#` shortcuts), left = mark read/unread until "Snooze with natural-language times..." ships, then snooze. Show a coloured action strip under the row with an icon, and announce the result for screen readers.
  4. Preferences: `swipe_actions: bool` (default on for macOS, off elsewhere) in `Preferences` (`crates/alice-core/src/model.rs:313`) with a Settings → Reading toggle.
  5. Respect `prefers-reduced-motion` for the settle animation (`apps/desktop/src/motion.tsx`).
- **Pitfalls:** The direction depends on the macOS natural-scrolling setting, which a webview cannot read; Mailspring shells out to `defaults`. Alice would need a small Tauri command reading `com.apple.swipescrolldirection`, or a "reverse direction" option. Windows precision touchpads and WebKitGTK also emit horizontal `wheel` events, but behaviour differs; test before enabling there. Horizontal scroll inside a wide row must not trigger a swipe. Undo is needed (P1 "Undo for archive, trash, move, mark read") because accidental swipes happen.
- **Done when:** On a MacBook trackpad, a decisive horizontal swipe archives the row, a mostly vertical scroll never moves a row, and the toggle disables swipes. A unit test feeds synthetic wheel sequences to the gesture state machine (pure function, no DOM) and asserts claim/settle/threshold decisions.
- **Effort:** M. Benefits from "Undo for archive, trash, move, mark read" and "Snooze with natural-language times...".

<a id="screenshot-privacy-mode-that-blurs-content"></a>

### Screenshot / privacy mode that blurs content

*Roadmap: P3*

- **Why:** Lets people share their screen, record a demo or take screenshots for bug reports without exposing mail.
- **Competitors:** Mailspring's `internal_packages/screenshot-mode/` registers `window:toggle-screenshot-mode`, listed under Developer → "Toggle Screenshot Mode" on all three platforms (`mailspring-app/menus/darwin.js:237`). It injects `assets/screenshot-mode.css` (`body * { -webkit-text-security: disc; color: transparent; text-shadow: 0 0 6px rgba(0,0,0,.8) }`) into the main document and into every message iframe (via a `MessageViewExtension.renderedMessageBodyIntoDocument` hook so newly opened messages are covered), and pins the window title to "Mailspring" so the subject does not show. 1.17.2 fixed non-Latin text (#2577); 1.17.3 extended the blur to message bodies. The toggle is not persisted.
- **Alice today:** Nothing. Message bodies render in a sandboxed `srcdoc` iframe built by `emailDocument` (`apps/desktop/src/emailHtml.ts:34-81`); notifications show sender and subject (`apps/desktop/src-tauri/src/notification.rs`).
- **Plan:**
  1. A `privacy` flag in React state (not persisted, so a restart never leaves it on by surprise), toggled by a shortcut in `apps/desktop/src/shortcuts.ts` (for example `Mod+Shift+B`) and a Settings entry.
  2. When on, add `class="privacy"` to the app root and a CSS rule in `styles.css` that blurs text (`filter: blur(6px)` on list rows, reader, contact suggestions) and hides avatars' initials. Use `filter` rather than Mailspring's `-webkit-text-security`, which is WebKit/Blink-only.
  3. Pass `privacy` into `emailDocument` so the iframe document gets the same blur style; this also covers messages opened later, because the document is rebuilt on change.
  4. While on, suppress notification bodies (title "New mail" only) and keep the window title generic.
  5. Optional: `WebviewWindow::set_content_protected(true)` excludes the window from screen capture on macOS and Windows. Electron documents that ScreenCaptureKit-based recorders on newer macOS can still capture such windows, so present it as best-effort.
- **Pitfalls:** Blur radius must be large enough that short words (OTP codes) are unreadable; test with a 6-digit code. The OTP copy banner (`MessageView.tsx:79`) shows the code in plain text and must be blurred too. The compose window is out of scope unless explicitly covered.
- **Done when:** With privacy mode on, a screenshot shows no readable sender, subject, body or code, newly opened messages are blurred, notifications omit content, and the mode is off after restart. Add a `scripts/test-html.mjs` case asserting the privacy style is in the generated document.
- **Effort:** S.

<a id="proxy-support-socks5-http-for-imap-smtp-and-http-apis"></a>

### Proxy support (SOCKS5 / HTTP) for IMAP, SMTP and HTTP APIs

*Roadmap: P3*

- **Why:** Needed behind corporate proxies, in countries that block mail providers, and for people who route mail through Tor. Mailspring users asked for years (GitHub #235, 52 👍; forum #185, 19 votes).
- **Competitors:** Mailspring has no proxy support for IMAP/SMTP (mailcore2/libetpan connect directly; only curl-based HTTP calls see `CURLE_COULDNT_RESOLVE_PROXY`, `repos/mailsync/MailSync/SyncException.cpp:24`). Thunderbird: Settings → Network & Disk Space → Connection: No proxy, Auto-detect, **Use system proxy settings (default)**, Manual (HTTP/HTTPS/SOCKS host, SOCKS v4/v5, "No proxy for"), PAC URL, and "Proxy DNS when using SOCKS v5" ([Mozilla](https://support.mozilla.org/en-US/kb/advanced-panel-settings-in-thunderbird)). eM Client has a similar manual proxy setting.
- **Spec:** SOCKS5 RFC 1928 (use ATYP 3, domain name, so DNS happens at the proxy), username/password RFC 1929; HTTP `CONNECT host:port` (RFC 9110 §9.3.6).
- **Alice today:**
  - IMAP: `providers::session` opens a `std::net::TcpStream` with `connect_timeout`, wraps it in native-tls and hands it to `imap::Client::new` (`crates/alice-core/src/providers.rs:31-75`). `imap` 2.4 accepts any `Read + Write` stream, so a tunnelled `TcpStream` can be inserted before TLS.
  - SMTP: lettre's blocking `SmtpTransport::relay/starttls_relay` (`providers.rs:213-237`) opens its own socket; lettre has no proxy support ([lettre#595](https://github.com/lettre/lettre/issues/595), open) and no way to inject a stream into the blocking transport.
  - HTTP: one builder, `client::http_client()` (`crates/alice-core/src/client.rs:71-76`), used by AI, OAuth, contacts, calendars and DAV; `updates.rs:36` builds its own. reqwest reads `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY` by default, but Alice builds reqwest with `default-features = false` (root `Cargo.toml`), so the `system-proxy` feature (macOS/Windows system settings) and the `socks` feature are off.
- **Plan:**
  1. Preference `proxy: Off | System | Manual { kind: Socks5 | Http, host, port, username, remote_dns: bool }` (password in the vault). One `proxy.rs` module with `fn connect(host, port, timeout) -> io::Result<TcpStream>` that either connects directly or performs the SOCKS5 (with domain-name ATYP) or HTTP CONNECT handshake. Both handshakes are ~60 lines; the sync `socks` crate (0.3.4, last release 2022) is unmaintained, so write them.
  2. IMAP: replace the `TcpStream::connect_timeout` loop in `providers::session` with `proxy::connect`.
  3. SMTP without forking lettre: when a proxy is set, open a one-shot loopback listener on `127.0.0.1:0` whose single accepted connection is spliced to `proxy::connect(smtp_host, smtp_port)`, and point lettre at it with `builder_dangerous("127.0.0.1").port(local).tls(Tls::Wrapper | Tls::Required(TlsParameters::new(smtp_host)))`, so certificate checks still use the real host name. Alternative: move sending to lettre's async `AsyncSmtpConnection::connect_with_transport` over a tokio stream, which is a bigger change.
  4. HTTP: enable reqwest's `socks` feature and build `http_client()` with `.proxy(Proxy::all("socks5h://…" | "http://…"))`; for `System`, enable the `system-proxy` feature. The daemon client (`client.rs:90`, `:338`) talks to `127.0.0.1:4815` and must always use `.no_proxy()`.
  5. Settings → Network panel with a "Test connection" button that connects to the default account's IMAP host through the proxy.
- **Pitfalls:** DNS leaks: resolve through the proxy (`socks5h`, ATYP 3) or Tor is pointless. Google and Microsoft may challenge or block sign-ins from Tor exits and some VPN ranges. Long-lived IMAP IDLE connections through proxies hit proxy idle timeouts; use IDLE re-issue intervals under 10 minutes. The OAuth consent page opens in the system browser and follows the system proxy, not Alice's. PAC files and per-account proxies can wait.
- **Done when:** With `ssh -D 1080` or a Dante SOCKS server and direct egress blocked by a firewall rule, Alice syncs IMAP, sends via SMTP, refreshes OAuth tokens and calls the AI provider; with an authenticated HTTP CONNECT proxy (e.g. Squid in the CI Docker fixture), the same works. Unit tests for both handshakes against in-process fake proxies, including auth failure and remote-DNS request bytes; a test that the daemon client never uses the proxy.
- **Effort:** M. Related: "Two long-lived IMAP connections per account..." (P0) changes where connections are opened; do this after it.

<a id="local-folders-keep-mail-on-this-device-only-outside-any-account"></a>

### Local folders: keep mail on this device only, outside any account

*Roadmap: P3 · new*

- **Why:** People archive old mail off the server to stay under quota or keep it after leaving a job (Mailspring forum #784, 10 votes).
- **Competitors:** Mailspring has none. Thunderbird "Local Folders" is a per-profile local account stored as mbox by default, or maildir per account ([storage](https://source-docs.thunderbird.net/en/latest/backend/folder_storage.html)). Apple Mail "On My Mac" mailboxes (Mailbox → New Mailbox → Location) ([Apple](https://support.apple.com/guide/mail/create-or-delete-mailboxes-mlhlp1021/mac)). eM Client "Local folders" ([blog](https://www.emclient.com/blog/em-client-local-folders-643)).
- **Alice today:** Nothing. The cache keeps only headers, text/HTML parts and invitations (`crates/alice-core/src/mime.rs:316-324`); attachments are fetched from the server on demand (`remote.rs:308`), so a cached message is not a complete copy and cannot outlive its server copy.
- **Plan:**
  1. A built-in "On this device" account (`Provider::Local`, no hosts, no credentials) with user-created folders.
  2. "Move to local folder" downloads the full raw message (`BODY.PEEK[]`, already used by `remote.rs` `whole()`), writes it to `<data dir>/local/<uuid>.eml`, parses it through `providers::parse_message`, then deletes it from the server only after the file is fsynced (offer "Copy" as well).
  3. Attachments of local messages are read from the `.eml`, not from IMAP.
  4. Local messages join search and conversations like any other.
  5. Reuse the same storage for .eml/mbox import (P2 "Import from mbox / Thunderbird / Apple Mail").
- **Pitfalls:** Local folders are the only copy: account removal and "Reset cache for one account" must never touch them; show their size in Settings → Mail storage and remind users to back up the data directory. Mail is stored without application-level encryption (README).
- **Done when:** A message moved to a local folder disappears from the server, opens with attachments while offline, survives a cache rebuild, and is found by search. Test: unit test that a moved message's `.eml` round-trips through `parse_message` and that account removal leaves `local/` intact.
- **Effort:** L. Depends on "Export messages and folders as .eml and mbox..." and "Import from mbox..." (P2), which share the raw-message storage, and "Create, rename, delete and nest folders and labels" (P1) for the folder UI.

<a id="choose-which-accounts-the-unified-inbox-includes"></a>

### Choose which accounts the unified inbox includes

*Roadmap: P3 · new*

- **Why:** People with a work and a personal account, or a noisy shared mailbox, want "All Inboxes" without one of them (Mailspring GitHub #38, 20 👍; forum #136, 12 votes).
- **Competitors:** Mailspring always shows a unified inbox with every account. Thunderbird's Unified Folders are virtual folders; right-click the unified Inbox → Properties → Choose… to tick which inboxes it includes. Apple Mail "All Inboxes" includes every enabled account.
- **Alice today:** An empty `accountId` shows all accounts; clicking an account in the sidebar filters to it (`apps/desktop/src/App.tsx:52`, `:107`, `:355-356`); the daemon filters only by a single optional `account_id` (`MessageQuery`, `crates/alice-core/src/model.rs:129-137`; `store.rs` `thread_rows`). Unread counts in `/v1/state` cover all accounts.
- **Plan:**
  1. `Account.in_unified: bool` (serde default `true`) set from the account's context menu ("Include in All Inboxes") and Settings → Accounts.
  2. When `account_id` is absent, `thread_rows` and the message list exclude accounts with `in_unified = false`; selecting that account explicitly still shows it.
  3. Apply the same rule to the tray/dock unread count and new-mail notifications, with a per-account "Notify" toggle next to it.
- **Pitfalls:** If every account is excluded, show the empty state with a hint rather than nothing. Search across "All" should probably still include every account; decide and label it.
- **Done when:** Excluding one of two accounts hides its mail and unread count from All Inboxes while its own view still works. Unit test in `store.rs` for `thread_rows` with an excluded account.
- **Effort:** S.

<a id="mouse-backforward-buttons-and-history-navigation"></a>

### Mouse back/forward buttons and history navigation

*Roadmap: P3 · new*

- **Why:** People with five-button mice expect "back" to return from a message to the list, or to the previous folder (Mailspring forum #761, 6 votes).
- **Competitors:** Mailspring has no back/forward navigation. Browser-based mail (Gmail, Outlook web) gets it from browser history.
- **Alice today:** No navigation history: folder, account, query and selected thread are plain React state in `App.tsx`. In Tauri v2, wry intercepts the extra mouse buttons on macOS (buttons 3/4) and Linux (8/9) and dispatches synthetic `mousedown`/`mouseup` events with `button` 3 or 4 (untrusted), then calls `history.back()`/`forward()` unless the `mouseup` was `preventDefault()`ed ([tauri#4019](https://github.com/tauri-apps/tauri/issues/4019)); WebView2 delivers real `mouseup`/`auxclick` events with `button` 3/4 and navigates history on `mouseup` unless prevented.
- **Plan:**
  1. Keep a small in-memory history of view states `{ folder, accountId, query, threadId }` in `App.tsx`, pushed on folder/thread changes (not on every keystroke in search).
  2. A window-level `mouseup` listener: `button === 3` → back, `4` → forward, always `preventDefault()`. This works the same on all three platforms.
  3. Keyboard equivalents: `Mod+[` / `Mod+]` and `Alt+Left/Right`, registered in `apps/desktop/src/shortcuts.ts` so they appear in Settings → Keyboard shortcuts.
- **Pitfalls:** Events over the message iframe arrive at the iframe document; forward them like the existing click/context-menu listeners in `MessageView.tsx:114-133`. Don't navigate while the composer has focus.
- **Done when:** Opening a thread and pressing the mouse back button returns to the list with the same scroll position, on macOS, Windows and Linux. Unit test for the history stack (push, back, forward, truncate on new push).
- **Effort:** S. Related: "Swipe gestures on trackpads..." (a two-finger swipe could later map to back/forward in the reader).

## P4: long tail

<a id="microsoft-graph-mail-backend-for-tenants-that-block-imap-not-ews-it-is-being-swi"></a>

### Microsoft Graph mail backend for tenants that block IMAP (not EWS: it is being switched off)

*Roadmap: P4*

- **Why:** Many Microsoft 365 tenants disable IMAP, and SMTP AUTH is going away by default, so Outlook work accounts may not be able to use Alice at all (Mailspring GitHub #34, 67 👍; #258, 16 👍).
- **Competitors:** Mailspring has no Exchange backend; Office 365 works only over IMAP/SMTP with OAuth (1.23.0 added O365 shared mailboxes, #2763; 1.26.0 fixed adding some Workspace/O365 accounts, #2907). Thunderbird 145 (Nov 2025) shipped native EWS written in Rust; **Thunderbird 154 enabled Microsoft Graph** for Microsoft 365 mail (announced 2026-09-02, mail only; ESR 153 has EWS only) using scopes `User.Read MailboxFolder.ReadWrite Mail.ReadWrite Mail.Send offline_access` ([blog](https://blog.thunderbird.net/2026/09/thunderbird-desktop-new-protocol-support-microsoft-graph-api/), [wiki](https://wiki.mozilla.org/Thunderbird:Microsoft_Graph_API)). eM Client and Outlook speak Exchange natively.
- **Spec:**
  - **EWS is being switched off in Exchange Online**: phased disablement started 2026-10-01 and EWS is fully disabled from 2027-04-01 (MC1227454; [Learn](https://learn.microsoft.com/en-us/exchange/clients-and-mobile-in-exchange-online/deprecation-of-ews-exchange-online)). Until then it works only for app IDs on a tenant's `EWSAllowedAppIDs` list, which Microsoft auto-fills from the previous 60 days of use. EWS stays on on-premises Exchange Server.
  - **SMTP AUTH**: Basic auth for SMTP AUTH is disabled by default for existing tenants at the end of December 2026 (MC786329, revised); independently, SMTP AUTH (even with OAuth) is often disabled tenant-wide or per mailbox, and security defaults disable it ([Learn](https://learn.microsoft.com/en-us/exchange/clients-and-mobile-in-exchange-online/authenticated-client-smtp-submission)).
  - Graph mail: per-folder delta `GET /me/mailFolders/{id}/messages/delta` (follow `@odata.nextLink`, store `@odata.deltaLink`; deletions as `@removed`; `Prefer: odata.maxpagesize`; `$filter` only `receivedDateTime ge`, capped at 5,000) and `GET /me/mailFolders/delta` ([delta](https://learn.microsoft.com/en-us/graph/delta-query-messages)). `Prefer: IdType="ImmutableId"` on every request so IDs survive moves. Raw MIME via `GET /me/messages/{id}/$value`. Send with `POST /me/sendMail` taking base64 MIME (`Content-Type: text/plain`, 202 Accepted, `saveToSentItems` default true) ([sendMail](https://learn.microsoft.com/en-us/graph/api/user-sendmail?view=graph-rest-1.0)). Attachments over 3 MB need `createUploadSession` (≤150 MB, ≤4 MB chunks, no Authorization header to the upload URL). `$batch` holds ≤20 requests. Throttling per app+mailbox: 10,000 requests/10 min, 4 concurrent, 150 MB upload/5 min ([limits](https://learn.microsoft.com/en-us/graph/throttling-limits)). Webhooks need a public HTTPS endpoint, so a desktop client polls delta. Useful properties: `categories`, `flag`, `inferenceClassification` (Focused/Other), `conversationId`, `internetMessageId`.
- **Alice today:** Outlook accounts use OAuth with `IMAP.AccessAsUser.All` and `SMTP.Send` on outlook.office.com (`crates/alice-core/src/oauth.rs:35`), so they break when the tenant disables IMAP or SMTP AUTH. Graph is already used with plain reqwest for contacts and calendars (`calendar.rs:23`, `contacts.rs:633-650`), with a separate Graph token from the same refresh token (`vault.rs` `graph_token`). The sync engine and `Message` are IMAP-shaped (`remote_mailbox`, `remote_uid`, `uid_validity`, `model.rs:111-113`).
- **Note:** Build Graph, not EWS: EWS is being removed from Exchange Online right now, and on-premises Exchange almost always offers IMAP. Split out a smaller, more urgent item: **send Outlook mail through Graph `sendMail` when SMTP AUTH is refused** (M effort, P1), because SMTP AUTH defaults change in December 2026 and IMAP-only reading still works for many tenants.
- **Plan:**
  1. Scopes `Mail.ReadWrite Mail.Send` (Graph) requested with the account's existing consent flow (`oauth.rs` `feature_scopes`); handle tenants that require admin consent with a clear error.
  2. Send path first: in `daemon::send`, for `Provider::Outlook`, build the same lettre message (`providers::build_mail`), base64 its formatted bytes and `POST /me/sendMail`; fall back from SMTP to Graph on `535 5.7.139`/"SmtpClientAuthentication is disabled" and remember the choice per account. Keep the at-most-once receipt semantics (202 means accepted, not delivered).
  3. Backend split: add `Message.remote_id: Option<String>` (Graph immutable id; also JMAP ids), and a `graph.rs` with `folders()`, `delta(folder, link)`, `modify(id, read/flag/move)`, `attachment(id, index)`, `send(mime)`, mirroring what `remote.rs`/`providers.rs` offer for IMAP. Dispatch on an account field `mail_api: Imap | Graph` in `sync.rs`. Two real implementations (and JMAP as a third) justify a small trait here.
  4. Sync: per folder, store the deltaLink in `settings`; request `$select=subject,from,toRecipients,ccRecipients,replyTo,receivedDateTime,isRead,flag,internetMessageId,conversationId,hasAttachments,body` with `Prefer: outlook.body-content-type="html"` and fetch attachment metadata separately; poll every 60 s (cheap when nothing changed). Use `$batch` for flag changes.
  5. Mutations: `PATCH /me/messages/{id}` (`isRead`, `flag`), `POST /me/messages/{id}/move` (`destinationId`: `archive`, `deleteditems`, `junkemail` well-known names).
  6. Crates: plain reqwest + serde (already in the tree). `graph-rs-sdk` 3.0.1 is slow-moving; Thunderbird's `ews` crate (0.1.2, MPL-2.0) exists if on-prem EWS is ever needed.
- **Pitfalls:** Many tenants block user consent for third-party apps, so Alice needs a verified publisher app registration ("Verify Outlook/O365 OAuth against a live account; ship a built-in Microsoft client id"). Delta tokens expire (`syncStateNotFound` → full resync). Moving to the archive mailbox changes immutable IDs. Throttling returns 429 with `Retry-After`. Graph `body` is Exchange's rendering, not the original MIME; use `$value` when exact MIME matters (S/MIME, PGP). Gmail-style multi-label does not exist; one folder per message, plus categories.
- **Done when:** A Microsoft 365 test tenant with IMAP and SMTP AUTH disabled can add an account, sync Inbox/Sent/Archive/Deleted/Junk incrementally, read/flag/move mail, open attachments and send with attachments; sending falls back to Graph automatically when SMTP AUTH is refused. Unit tests replay recorded delta pages (including `@removed` and a `nextLink` chain) through the sync mapping; a mock server test covers 429 back-off.
- **Effort:** XL (send-only fallback: M). Depends on "Multi-folder message model..." (P0), "Sync every folder..." (P0) and "Verify Outlook/O365 OAuth against a live account; ship a built-in Microsoft client id" (P1).

<a id="jmap-backend-fastmail-stalwart"></a>

### JMAP backend (Fastmail, Stalwart)

*Roadmap: P4*

- **Why:** JMAP is faster and simpler than IMAP (one HTTP request per batch, server-side threads, cheap change tracking, push over HTTP), and it is the only way to reach Fastmail features such as Masked Email.
- **Competitors:** Mailspring has no JMAP. Clients with JMAP: Fastmail's own apps, Mailtemi (iOS/Android), Thunderbird has none in release. Servers: Fastmail (`https://api.fastmail.com/jmap/session`), Stalwart (Rust, AGPL-3.0), Cyrus IMAP 3.x, Apache James ([jmap.io](https://jmap.io/software/index.html)).
- **Spec:** RFC 8620 (core): session resource (autodiscovery `/.well-known/jmap`), `/get`, `/changes`, `/set`, `/query`, `/queryChanges` with state strings (`cannotCalculateChanges` → full resync), back-references, blob upload/download, and **EventSource push (§7.3)**, which suits a local daemon. RFC 8621 (mail): Mailbox, Thread, Email (`keywords` such as `$seen`/`$flagged`, `mailboxIds` = multi-mailbox membership), `Email/import` of raw RFC 5322 blobs, `EmailSubmission/set` with `onSuccessUpdateEmail` to move the sent draft, Identity. Related: RFC 8887 (WebSocket), RFC 9610 (contacts, Dec 2024), JMAP Calendars in the RFC Editor queue (draft-ietf-jmap-calendars-32).
- **Alice today:** Nothing. IMAP-shaped model and sync as described in the Exchange item; reqwest 0.13 already in the tree.
- **Plan:**
  1. Crate `jmap-client` 0.4.3 (Sep 2026, Stalwart Labs, Apache-2.0 OR MIT, depends on reqwest 0.13 like Alice; has a `blocking` feature). It covers session, mailbox, email, submission, identity and EventSource.
  2. Account setup: "Fastmail (JMAP)" preset with an API token (Bearer), or a generic JMAP server URL + username/password (Stalwart). Fastmail OAuth needs a registered client (partnerships contact), so start with tokens.
  3. `jmap.rs` implementing the same backend functions as the Graph item: `Mailbox/get` + `Mailbox/changes`; initial `Email/query` (sort `receivedAt` desc, filter by sync window) + `Email/get` with `bodyValues` for text/HTML (`fetchHTMLBodyValues`, `maxBodyValueBytes` = 4 MB to match `TEXT_LIMIT`); then `Email/changes` from the stored state; flag/move via `Email/set` on `keywords`/`mailboxIds`.
  4. Push: keep an EventSource connection per account (`types=Email,Mailbox`, `ping=60`) in the daemon to trigger `changes` immediately, replacing the 60 s poll for these accounts.
  5. Send: build the MIME with lettre as today, upload it as a blob, `Email/import` into Drafts, then `EmailSubmission/set` with `onSuccessUpdateEmail` moving it to Sent and clearing `$draft`; this gives at-most-once semantics via the submission id.
  6. Masked Email (the hide-my-email item) and JMAP contacts become small additions on the same session.
- **Pitfalls:** `mailboxIds` means one email is in several mailboxes, which Alice's one-folder model cannot represent ("Multi-folder message model" first). State strings can become invalid; handle `cannotCalculateChanges`. Server limits (`maxObjectsInGet`, `maxCallsInRequest`) from the session must bound batch sizes. Fastmail also offers IMAP, so the user-visible win is speed, push and Masked Email rather than access.
- **Done when:** A Fastmail test account and a Stalwart container (CI fixture alongside Dovecot) sync, update flags, move and send; a change made in Fastmail's web app shows in Alice within seconds via EventSource. Tests against Stalwart in Docker in the `mail-protocol` CI job.
- **Effort:** L–XL. Depends on "Multi-folder message model..." and the backend split from the Graph item (do Graph or JMAP first, then the other is cheaper).

<a id="mail-merge-from-csv"></a>

### Mail merge from CSV

*Roadmap: P4*

- **Why:** Small organisations, teachers and event organisers send the same message to many people with personal fields and want each to arrive as a normal one-to-one email from their own account.
- **Competitors:** Mailspring does not ship mail merge (an onboarding line about it is commented out, `internal_packages/onboarding/lib/page-tutorial.tsx:49`; leftover styles in `composer/styles/composer.less:502`). eM Client Pro has "Send as Mass Mail", which fills variables from contact fields only, without a CSV source ([forum](https://forum.emclient.com/t/mail-merge-using-external-data-source-xls-csv/78415)). Gmail (paid Workspace plans) has built-in mail merge: "Use mail merge" next to To, `@firstname`/`@lastname`/`@fullname` tags, and any column of a linked Google Sheet as a tag ([help](https://support.google.com/mail/answer/12921167)). Thunderbird needs the Mail Merge add-on (CSV or address book).
- **Alice today:** Nothing. Drafts carry only `to`, subject and body (`model.rs:222-230`). Sends are at-most-once with a recorded delivery per draft (`deliveries` table, `store.rs:22`; README "Architecture"), which is a good base for resumable bulk sending.
- **Plan:**
  1. Template = an ordinary draft whose subject/body contain `{{column}}` placeholders (and `{{first_name}}` etc. derived from the address book when no CSV).
  2. `POST /v1/merge { draft_id, csv, email_column, dry_run }` parses the CSV in the daemon with the `csv` crate (new dependency), validates every address and that every placeholder has a column, and returns the first rendered messages as a preview plus per-row errors.
  3. On confirm, create one draft per row with a UUID v5 of (template id, row index) so re-running never duplicates, then send them sequentially through the existing send path at a configurable pace (default one every 3 s), recording progress; the UI shows progress, failures and "resume".
  4. CLI: `alice merge --draft <id> --csv people.csv [--dry-run]`.
- **Pitfalls:** Provider limits (consumer Gmail about 500 recipients/day, Workspace 2,000; Microsoft 365 10,000 recipients/day and 30 messages/min) and spam reputation; pace sending and stop on the first rate-limit error. HTML-escape values once HTML compose exists. CSVs from Excel in German locales use `;` and may be Windows-1252 or UTF-8 with BOM. Each message lands in Sent individually. Bulk marketing needs unsubscribe handling, which is out of scope; say so in the UI.
- **Done when:** A 3-row CSV produces three correctly personalised messages to three recipients, a crash mid-run resumes without duplicates, and invalid rows are reported before anything is sent. Unit tests: placeholder rendering, missing column, `;` delimiter and BOM handling, idempotent draft ids.
- **Effort:** M. Depends on "Cc and Bcc fields" and, for real use, "Attachment upload..." and "Rich text / HTML compose...".

<a id="shared-drafts-and-team-comments-on-threads"></a>

### Shared drafts and team comments on threads

*Roadmap: Won't do*

- **Why:** Teams that share a support or sales inbox want to discuss an email privately and co-write the reply.
- **Competitors:** Mailspring has no team features (teams only share a Mailspring ID; 1.24.1 fixed shared signature images overwriting each other). Spark for Teams: shared threads with private comments, real-time co-edited shared drafts in a Shared Drafts folder, and shareable links that update with new mail and comments ([shared threads](https://support.readdle.com/spark/spark-for-teams-and-billing/shared-threads-discuss-emails-with-your-team), [shared drafts](https://support.readdle.com/spark/spark-for-teams/shared-drafts-write-emails-together-with-your-team)). All of it runs on Readdle's servers, which store the shared mail.
- **Alice today:** Nothing; Alice is local-first with no server component, and AGPL makes a hosted service possible but not free to run.
- **Note:** Recommend **won't do** for now. It needs a hosted collaboration service (accounts, sharing, real-time sync, storage of other people's mail), which is a different product. If the demand appears, the cheapest honest version is "share a draft by sending it": forward a draft to a colleague with a comment, which needs nothing new. Private notes on threads are covered by "Tasks and notes tied to messages".
- **Effort:** XL (service + client). No dependencies listed because it should not be scheduled.

<a id="share-a-thread-as-a-public-link"></a>

### Share a thread as a public link

*Roadmap: Won't do*

- **Why:** Lets people show a conversation to someone outside it without forwarding.
- **Competitors:** Mailspring 1.2.0 "Thread Sharing" (`internal_packages/thread-sharing/`, needs a Mailspring ID): toggling "Share this thread" uploads every non-hidden message of the thread as JSON and every attachment (`MailspringAPIRequest.postStaticAsset`) to Mailspring's cloud; the link is `https://shared.getmailspring.com/thread/<identity id>/<thread id>-<timestamp>`, "anyone with the secret link can read it and download attachments", and **new messages on the thread are re-uploaded automatically** 5 s after any change. Free accounts get a limited number of links ("All Sharing Links Used"). 1.6.2 added local `mailspring://` permalinks as a separate feature; 1.23.0 fixed null crashes (#2757). Spark has shareable links that also update.
- **Alice today:** Nothing, and no server to host pages.
- **Note:** Recommend **won't do** as specified. Publishing mail (including other people's messages and attachments) to a public URL that silently keeps updating is a privacy liability and needs hosting. Cover the need locally instead: "Print and Save as PDF" (P1) and "Export messages... as .eml" (P2) let users share a frozen copy through their own storage, and "Permalinks to a thread (`alice://`)" (P2) covers personal links.
- **Effort:** L if ever built (hosting, expiry, revocation). No dependencies.

<a id="open-and-link-tracking-for-outgoing-mail-ms-pro-consider-skipping-on-privacy-gro"></a>

### Open and link tracking for outgoing mail (`MS` Pro) — consider skipping on privacy grounds

*Roadmap: Won't do*

- **Why:** Sales users like knowing when mail is opened; this is the reason Mailspring Pro sells it.
- **Competitors:** Mailspring `internal_packages/open-tracking/` and `link-tracking/` (both `isIdentityRequired`, server `https://link.getmailspring.com`). Open tracking inserts `<img alt="" src="https://link.getmailspring.com/o/<base64url JSON {messageId, accountId, recipient}>.png">` before `.gmail_quote`, one body per recipient (`needsPerRecipientBodies`); the code comment says the tag deliberately omits zero dimensions, CSS classes and branded alt text because those are what tracker blockers detect (earlier, 1.1.4/1.5.4 had added alt text "Sent from Mailspring" for deliverability). Link tracking rewrites every `href` outside the quote to `…/link/<message id>/<index>?redirect=<url>&recipient=<base64 email>`. Results appear in an Activity view (1.0.8) with export (1.1.0). Bugs: own pixels firing when viewing your sent mail (1.6.2, #1494; the remove-tracking-pixels package exists partly for this), duplicate Sent copies in Gmail (1.5.7, #1279), Courier IMAP (1.4.2), lost when moving a draft between accounts (1.24.1). Users asked for it to be off by default (GitHub #244, 22 👍). Spark Pro "Read Statuses" works the same way.
- **Alice today:** Nothing; compose is plain text, so a pixel could not even be embedded.
- **Note:** Recommend **won't do**. It needs a public server receiving recipients' requests, contradicts Alice's own tracker-blocking item, is made unreliable by Apple Mail Privacy Protection (every Apple Mail recipient appears to open immediately) and Gmail's image proxy, and rewritten links hurt deliverability. The legitimate need, "did they get it / read it", is served by "Read receipts: request and respond to `Disposition-Notification-To`" (consensual) and "Follow-up reminders" (P2).
- **Effort:** L if ever built (server, per-recipient bodies, activity view).

<a id="custom-themes-now-defer-a-plugin-system"></a>

### Custom themes now; defer a plugin system

*Roadmap: P4*

- **Why:** Lets the community add niche features and looks without growing the core app (Mailspring GitHub #363 "Global directory for plugins", 44 👍; forum #318).
- **Competitors:** Mailspring: `app/src/package-manager.ts` loads packages from `<config>/packages/` (`package.json` with `engines.mailspring`, `lib/main` exporting `activate`/`deactivate`, auto-loaded `styles/*.less`, `keymaps/`, `menus/`, `windowTypes` choosing which windows load it). Themes are packages with `"theme": "ui"` overriding `ui-variables.less`; Preferences → Appearance → "Change Theme…". Install via Developer → "Install a Plugin…" (`window:install-package`). Its own architecture doc lists the limits: no management UI, no marketplace, no update mechanism, **no sandboxing ("Plugins have full API access")**, weak version checks. History: 1.6.3 plugins must be precompiled; 1.7.0 starter repo; 1.19.0 community plugin/theme docs; 1.21.0 plugin-trust notice on install; 1.24.0 community plugin errors no longer sent to Sentry; 1.24.1 `MessageActionMenuItem` extension point. Thunderbird MailExtensions are WebExtensions (MV3 since 128 ESR) with permissioned `messenger.*` APIs (`messages`, `compose`, `messageDisplay`, `folders`, `accounts`, …) and add-on review on addons.thunderbird.net.
- **Alice today:** No plugins. Themes are Light/Dark (`Theme` enum, `model.rs:233-238`) implemented with CSS custom properties (`apps/desktop/src/styles.css:7`, `:36` for dark; ~350 `var(--…)` uses). Extensibility already exists outside the app: CLI with `--json`, the documented local HTTP API (`docs/api.md`) and the MCP server with access levels.
- **Note:** Split into "Custom themes" (S, do it) and "Plugins" (defer). A Mailspring-style JS plugin system would run untrusted code inside the window that holds mail, with no sandbox.
- **Plan:**
  1. Themes: load an optional `theme.css` from the data directory after `styles.css` (via a Tauri command returning its text, injected as a `<style>`), documented as overriding the `:root` variables; Settings → Appearance "Open theme folder" / "Reload theme". Ship two or three alternates (e.g. high-contrast) as examples.
  2. Plugins, if ever: run them in the daemon as WASM via Extism (Rust SDK 1.41, BSD-3-Clause, wasmtime-based) with a manifest per plugin limiting `allowed_hosts`, memory pages and `timeout_ms`, exposing a few host functions (read a message, add a label, propose a draft) behind the same access levels as MCP. Start with arrival hooks (classification, rules), not UI.
- **Pitfalls:** User CSS can load remote resources inside the app (the app CSP allows `img-src https:`); acceptable for user-chosen themes but say so. Any plugin API becomes a compatibility promise.
- **Done when:** A `theme.css` that changes `--accent` and backgrounds applies on reload and survives updates; a broken file is ignored with an error in Settings.
- **Effort:** S for themes; XL for plugins.

<a id="mobile-apps-android-ios-on-the-shared-rust-core"></a>

### Mobile apps (Android, iOS) on the shared Rust core

*Roadmap: P4*

- **Why:** People read mail on phones; a desktop-only client is a second-class choice for many.
- **Competitors:** Mailspring is desktop-only. Delta Chat runs one Rust core on all platforms and solves background delivery with a notification relay: `chatmail/notifiers` forwards device tokens to APNs/FCM and sends silent "heartbeat" pushes (default every 20 min), and chatmail servers trigger real pushes via an encrypted token stored with IMAP `SETMETADATA` ([notifiers](https://github.com/chatmail/notifiers)).
- **Alice today:** README: "Android and iOS remain a later phase". The desktop starts the daemon as a separate process (`Client::connect(&["--daemon"])`, `apps/desktop/src-tauri/src/main.rs:28`) and talks HTTP to it. Credentials use `keyring` 3 with desktop-only features (`crates/alice-core/Cargo.toml`). OAuth uses a loopback redirect `http://127.0.0.1:4816/callback` (`oauth.rs:25`). The window has `minWidth: 850` (`tauri.conf.json`).
- **Plan:**
  1. Run the daemon in-process on mobile (iOS forbids child processes): start `alice_core::daemon` on the Tauri runtime with a random loopback port and token, or call store functions directly through Tauri commands; keep the HTTP API for desktop.
  2. Credentials: move to `keyring` 4.x (`apple-native-keyring-store` covers iOS; `android-native-keyring-store` 1.0 needs `ndk-context`, which Tauri mobile initialises).
  3. OAuth: replace the loopback redirect on mobile with app-claimed redirects (custom scheme / universal links via the deep-link plugin) and platform-specific client IDs.
  4. Background mail: iOS gives only system-scheduled `BGAppRefreshTask` and silent pushes (`content-available`, low priority, may be dropped, 30 s of run time); real-time mail needs a push relay like Delta Chat's, i.e. a server Alice would operate, plus Swift/Kotlin plugin code (Tauri's notification plugin is local-only; the push plugin PR #2066 is unmerged). Android: WorkManager (15-minute minimum) or a foreground service holding IMAP IDLE.
  5. UI: a phone layout (single pane, bottom actions, touch targets, swipe actions shared with the desktop swipe item); remove the 850 px minimum on mobile.
  6. Storage: smaller default sync window (30 days) and attachment caching off by default.
- **Pitfalls:** The blocking `imap` crate is fine in-process but each sync must finish within the OS background budget. App Store review requires account deletion paths and privacy labels for the AI feature. Push requires running infrastructure, which conflicts with Alice's no-server stance; decide that before starting.
- **Done when:** A TestFlight/APK build adds a Gmail account, syncs, reads, replies and receives new-mail notifications within the platform's background limits.
- **Effort:** XL. Depends on "Two long-lived IMAP connections per account..." and "Push store changes to the UI (SSE / websocket)..." for an efficient in-process design.

<a id="tasks-and-notes-tied-to-messages"></a>

### Tasks and notes tied to messages

*Roadmap: P4*

- **Why:** Turning an email into a to-do, and keeping a private note on a conversation ("waiting for invoice"), is how many people manage work in their inbox.
- **Competitors:** Mailspring has no tasks (only a plan for CalDAV VTODO sync, `repos/mailspring/plans/caldav-10-vtodo-support.md`). eM Client has Tasks and Notes modules ([features](https://www.emclient.com/features)). Outlook turns flagged mail into Microsoft To Do tasks (100 most recent flagged in the last 30 days, primary mailbox, Microsoft-hosted accounts) ([support](https://support.microsoft.com/en-us/todo/using-microsoft-to-do-with-flagged-email-from-outlook)). Gmail has "Add to Tasks", linking the task to the email.
- **Alice today:** Star exists. "Add to calendar" creates events in Google, Microsoft Graph or CalDAV calendars (`crates/alice-core/src/calendar.rs:331`), and AI can propose events (`ai.rs:244-252`); there are no tasks or notes.
- **Plan:**
  1. Notes: a local `thread_notes(key, account_id, text, updated_at)` table keyed by the thread's root `Message-ID` (so it survives re-sync), `GET/PUT /v1/threads/{id}/note`, and a note field at the top of the conversation. No sync in v1; say so.
  2. Tasks: "Create task" on a message writes to the user's existing task system rather than inventing one: CalDAV `VTODO` (reuse `dav.rs` `create` with `text/calendar`, `URL:mid:<message-id>` per RFC 2392 and the subject as `SUMMARY`), Microsoft To Do (`POST /me/todo/lists/{id}/tasks` with `linkedResources`, scope `Tasks.ReadWrite`), Google Tasks (`tasks` scope; put the link in `notes`, since `links` is read-only).
  3. Let the AI propose a task the same way it proposes events (`AiResult.event` → `task`).
- **Pitfalls:** Three task APIs with different fields (due date vs date-time, reminders). Notes are personal data stored unencrypted like the rest of the cache. New OAuth scopes need re-consent.
- **Done when:** A note survives app restart and account re-sync; "Create task" on a message creates a task in a Radicale CalDAV test server with the message link, and in Microsoft To Do on a test account.
- **Effort:** M (notes S). Related to "Follow-up reminders" (P2) and "Snooze..." (P2), which cover the most common "remind me" need.

<a id="send-microsoft-365-mail-through-graph-when-smtp-auth-is-disabled"></a>

### Send Microsoft 365 mail through Graph when SMTP AUTH is disabled

*Roadmap: P1 · Accounts and setup · new*

- **Why:** Microsoft disables SMTP AUTH Basic by default for existing tenants at the end of December 2026, and many tenants already turn SMTP AUTH off entirely (security defaults do), so Outlook work accounts that still read over IMAP cannot send.
- **Note:** This belongs in P1 "Accounts and setup", ahead of the full Graph backend. Details, spec links and the send plan are in step 2 of "Exchange EWS and/or Microsoft Graph mail backend…" above: same lettre MIME, base64 to `POST /me/sendMail`, automatic fallback on SMTP `535 5.7.139`, `Mail.Send` scope.
- **Done when:** On a tenant with `SmtpClientAuthenticationDisabled`, sending from Alice succeeds through Graph and lands in Sent once.
- **Effort:** M. Depends on "Verify Outlook/O365 OAuth against a live account; ship a built-in Microsoft client id".

<a id="ldap-and-global-address-list-directory-lookup"></a>

### LDAP and Global Address List directory lookup

*Roadmap: P4 · new*

- **Why:** In companies and universities, colleagues are in the directory, not in personal contacts; without directory lookup autocomplete fails for most internal recipients (Mailspring GitHub #864, 13 👍; forum #330).
- **Competitors:** Mailspring has none. Thunderbird LDAP directories: name, host, base DN, port (389, or 636 with SSL), bind DN; Advanced tab with scope, search filter and max results; enabled for autocomplete under Composition → Addressing → Directory Server. Outlook queries the GAL and pulls S/MIME certificates from it.
- **Spec:**
  - Microsoft Graph: `GET /me/people?$search=…` with `X-PeopleQuery-QuerySources: Mailbox,Directory` (scope `People.Read`, no admin consent; relevance-ranked) ([people](https://learn.microsoft.com/en-us/graph/people-insights-overview)); or `GET /users?$search="displayName:…"` with `ConsistencyLevel: eventual` (`User.ReadBasic.All`, delegated, no admin consent, but tenants can block user consent; not for personal accounts) ([users](https://learn.microsoft.com/en-us/graph/api/user-list)).
  - Google Workspace: `people:searchDirectoryPeople` (`query`, `readMask`, `sources`) with scope `directory.readonly` (a sensitive scope, so app verification) ([API](https://developers.google.com/people/api/rest/v1/people/searchDirectoryPeople)).
  - LDAP: RFC 4511; filter escaping RFC 4515.
- **Alice today:** Address books are synced into the `contacts` table from Google People, Graph `/me/contacts`, CardDAV and macOS Contacts, and suggestions are answered locally (`crates/alice-core/src/contacts.rs:63`, `daemon.rs:941-949`). No directory search.
- **Plan:**
  1. Query directories live at autocomplete time (debounced 300 ms, 3 s timeout) instead of syncing whole directories; merge results after local suggestions with source `directory`.
  2. Outlook accounts: add `People.Read` to the contacts consent (`oauth.rs` `GRAPH_CONTACTS`) and call `/me/people` with the directory source header.
  3. Google Workspace accounts: `searchDirectoryPeople` behind its own consent toggle, since the scope needs verification.
  4. LDAP: a "Directory server" source under Settings → Contacts (URL `ldaps://…`, base DN, optional bind DN with password in the vault, filter template defaulting to `(|(mail={q}*)(cn={q}*)(sn={q}*))`, max 20 results) using `ldap3` 0.12 (MIT/Apache-2.0, sync `LdapConn` wrapper, native-tls by default like Alice).
  5. Fetch `userCertificate;binary` / `userSMIMECertificate` when S/MIME lands.
- **Pitfalls:** Escape user input in LDAP filters (RFC 4515) to avoid filter injection. Plain `ldap://` sends bind passwords in clear; require LDAPS or StartTLS. Directory results can be large; always cap. Tenants may block user consent for `People.Read`.
- **Done when:** Typing a colleague's surname in To suggests them from the GAL on a Microsoft 365 test tenant and from an OpenLDAP container in CI. Unit tests: filter escaping, result mapping, timeout leaves local suggestions intact.
- **Effort:** M. Related: "S/MIME: sign, encrypt, verify with system certificates" (certificate lookup).

<a id="receive-only-and-send-only-accounts"></a>

### Receive-only and send-only accounts

*Roadmap: P4 · new*

- **Why:** Some mailboxes are only read (archives, shared or no-reply inboxes, providers whose SMTP is blocked), and some senders are only an SMTP relay for a domain (Mailspring forum #134, 8 votes).
- **Competitors:** Mailspring requires both IMAP and SMTP. Thunderbird separates accounts from outgoing servers: an identity picks any configured SMTP server, so a "send-only" address is an extra identity with its own server.
- **Alice today:** Account creation requires both an IMAP and an SMTP host (`crates/alice-core/src/daemon.rs:725-741`); `Account` has non-optional `smtp_host`/`smtp_port` (`model.rs:12-29`).
- **Plan:**
  1. Receive-only: allow an empty SMTP host for IMAP accounts (`send: false`); hide such accounts from the composer's From list; replying from one picks the default sending account and says so.
  2. Send-only: model it as an identity with its own SMTP server attached to an existing account (the P1 send-as identities work), not as a separate account with no mailbox; its sent copy goes to the parent account's Sent.
- **Pitfalls:** Many code paths assume every account can send (RSVP replies in `providers::rsvp_mail`, AI drafts, MCP `send_draft`); each must handle "cannot send from this account".
- **Done when:** A Dovecot-only account syncs without SMTP settings and never appears as a sender; a reply from it goes out from the default account. Unit test for the validation change.
- **Effort:** S for receive-only; send-only depends on "Send-as identities and aliases, including Gmail "Send mail as" import".

<a id="portable-windows-build"></a>

### Portable Windows build

*Roadmap: P4 · new*

- **Why:** People run mail from a USB drive or on machines where they cannot install software (Mailspring GitHub #922, 13 👍).
- **Competitors:** Mailspring has `-c/--config-dir-path` (`app/src/browser/main.js:53-67`, `:125`) but no portable package. Thunderbird has `-profile <path>`, and PortableApps.com ships Thunderbird Portable with the profile in `Data\profile`.
- **Alice today:** The data directory can be redirected with `ALICE_DATA_DIR` and the port with `ALICE_PORT` (`crates/alice-core/src/client.rs:52-62`). Credentials go to Windows Credential Manager via `keyring`, which is per user and per machine (keyring writes `CRED_PERSIST_ENTERPRISE`, which only roams with roaming profiles). The NSIS installer is the only Windows artifact (`.github/workflows/ci.yml:79-80`).
- **Plan:**
  1. A `portable` marker file next to `Alice.exe` switches `data_dir()` to `<exe dir>\Data` and the WebView2 data to `<exe dir>\Data\webview` via Tauri 2.12's `app.appDirectoriesOverride` (relative paths resolve next to the exe).
  2. Use a port chosen at first start and stored in the data dir, so a portable and an installed Alice can run on one machine.
  3. Secrets: Credential Manager does not travel; store them in the data dir encrypted with a key derived from a user passphrase (Argon2) asked at start, or accept signing in again on each machine. Make the choice explicit in the UI.
  4. Ship a zip (`Alice.exe`, `alice.exe`, marker) from CI; a second variant with the WebView2 fixed runtime (`webviewInstallMode: fixedRuntime`, ~180 MB) only if users hit machines without WebView2.
  5. Disable the auto-updater in portable mode (or update in place inside the folder).
- **Pitfalls:** SQLite on slow or FAT32 USB sticks; the exclusive `daemon.lock` must live on the stick. Unsigned zips trigger SmartScreen. A passphrase-protected secrets file is a new attack surface; use a vetted KDF and AEAD.
- **Done when:** Running the zip from a USB stick on two Windows machines shows the same accounts and mail, writes nothing to `%LOCALAPPDATA%` or the registry, and coexists with an installed Alice.
- **Effort:** M.

<a id="gnome-online-accounts-kde-accounts-integration"></a>

### GNOME Online Accounts / KDE accounts integration

*Roadmap: P4 · new*

- **Why:** GNOME users expect an account added in Settings → Online Accounts to appear in their mail client (Mailspring forum #718, 11 votes; GitHub #466).
- **Competitors:** Mailspring has none. Evolution and Geary use GNOME Online Accounts (GOA); Thunderbird declined to build it (bug 847989: "a feature that the core team will not develop").
- **Spec:** GOA 3.58 D-Bus API ([interfaces](https://gitlab.gnome.org/GNOME/gnome-online-accounts/-/raw/master/data/dbus-interfaces.xml)): `org.gnome.OnlineAccounts.Mail` (`EmailAddress`, `ImapHost`, `ImapUserName`, `ImapUseSsl`/`ImapUseTls`, `SmtpHost`, `SmtpAuthXoauth2`, …), `OAuth2Based.GetAccessToken()`, `PasswordBased.GetPassword("imap-password" | "smtp-password")`. Google and generic IMAP/SMTP accounts expose full mail settings; the Microsoft 365 provider exposes only the address and Graph-scoped tokens; Exchange is EWS. GOA's own docs say it is for core GNOME applications and ask third-party apps to talk to the maintainers first; its OAuth tokens are issued to GNOME's client ID, so using them makes Alice appear to Google as GNOME. KDE has no usable mail-account source today (KAccounts lacks mail; its replacement is a prototype).
- **Alice today:** Nothing; Linux credentials use the Secret Service via `keyring`.
- **Plan:**
  1. Linux only: in Add Account, "Import from GNOME Online Accounts" lists GOA accounts with a Mail interface (via `zbus`, a new dependency; ~100 lines of proxies) and pre-fills a normal Alice account.
  2. Generic IMAP/SMTP: copy hosts and read the passwords once with `GetPassword` into Alice's own vault.
  3. Google: pre-fill the address and run Alice's own OAuth rather than borrowing GOA's tokens (policy risk).
  4. Flatpak needs `--talk-name=org.gnome.OnlineAccounts` (Flathub accepts it for third-party apps such as Planify).
- **Pitfalls:** A live link (fetching GOA tokens on every sync) depends on GNOME's goodwill and Google's branding rules; avoid it. Accounts removed in GOA are not removed from Alice after a one-time import; say so.
- **Done when:** On Fedora/Ubuntu GNOME, an IMAP account configured in Online Accounts imports into Alice with one click and syncs.
- **Effort:** S–M. Low value relative to effort elsewhere; keep in P4.

<a id="pop3-accounts"></a>

### POP3 accounts

*Roadmap: Won't do · new*

- **Note:** Mailspring rejected POP3 ([GitHub #45](https://github.com/Foundry376/Mailspring/issues/45), closed 2020 as "rejected"): the maintainer said IMAP is near-universal, that POP3 removes mail from the server on download, and that "we will definitely never support POP3". The same reasoning applies to Alice, whose cache, multi-device drafts and server-side state all assume IMAP (or Graph/JMAP). Users with POP3-only providers can be pointed to providers' IMAP or to "Local folders" for archiving.

## Housekeeping

<a id="update-readmemd-current-boundaries-cid-inline-images-are-implemented"></a>

### Update `README.md` "Current boundaries": `cid:` inline images are implemented

*Roadmap: HK*

- **Why:** The README tells users and contributors a shipped feature is missing.
- **Alice today:** Confirmed stale. `README.md:293` (unchanged in the working tree) still says "Images embedded in HTML (`cid:`) and attachment upload are not implemented. Compose sends plain text." The feature landed in commit `ed6250b` (2026-10-12):
  - `mime.rs:30` parses `Content-ID` and `model.rs:83` stores it on `Attachment`; `mime.rs:824-834` tests it.
  - `MessageView.tsx:28-49` (`useEmbedded`) fetches PNG/JPEG/GIF/WebP parts referenced by `<img src="cid:…">` from the server via `attachmentBytes` (`api.ts:113-118`), caches them per message and turns them into `data:` URLs.
  - `emailHtml.ts:66` substitutes them before sanitizing; the CSP already allows `img-src data:`.
  - The other two claims in that sentence are still true: attachment upload is not implemented, and `providers::build_mail` sends `text/plain` only (`providers.rs:193-211`).
- **Plan:**
  1. Replace the sentence with: "Images embedded in HTML (`cid:`; PNG, JPEG, GIF and WebP) are fetched from the mail server when the message opens and shown without asking, since they are part of the message. Attachment upload is not implemented. Compose sends plain text."
  2. While there, add a `scripts/test-html.mjs` case for `emailDocument(html, false, false, { ii_1: 'data:image/png;base64,…' })` asserting the `cid:` image gets the `data:` URL and an unknown `cid:` gets no `src`; there is no frontend test of the substitution today.
- **Note:** Embedded images are still listed in the attachment strip under generic names such as "Attachment.png" (`Attachments.tsx` does not look at `content_id`). Mailspring hides inline images from the attachment list (1.5.7). Add a small P1 Reading item: hide attachments whose `content_id` the HTML body references.
- **Done when:** README matches behaviour, and the new HTML test passes.
- **Effort:** S.

<a id="update-readmemd-as-each-item-above-ships"></a>

### Update `README.md` as each item above ships

*Roadmap: HK*

- **Plan:** Make it part of each item's "Done when": the PR that ships a feature edits "Current boundaries" (and the relevant section, e.g. "Bring your own AI" for translation and summaries, "Architecture" for new backends) and `docs/api.md` for new endpoints. A PR template checkbox ("README/`docs/api.md` updated or not needed") keeps this from depending on memory.
- **Note:** Items from this section that change user-facing privacy behaviour (image proxy, tracker stripping, read receipts, AI translation/summaries, proxy) also need the README's privacy statements updated: what is sent where, and what is stored on disk.
- **Effort:** S per item.
