# Verification after Alice audit fixes

Recorded 2026-10-11T08:31:22.859435+00:00. Checks below ran against the completed source changes. The UI confirmation reports provide browser details separately.

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | Pass |
| `cargo test --workspace` | Pass: 14 tests, including core, CLI/TUI and the AI-origin integration test; desktop target compiles |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass |
| `npm run build` | Pass: strict TypeScript and Vite production build |
| `npm run test:smoke` | Pass against the real newly built debug CLI/daemon in a disposable data directory |
| `python3 scripts/test-tui.py` | Pass against the real TUI in a Unix PTY and a local HTTP fixture |
| `cargo build -p alice --release` | Pass |
| `CI=true npm run desktop:build` | Pass: Alice.app and Alice_0.1.0_aarch64.dmg |
| Packaged normal app startup | Pass: native app remains running, starts its own daemon, returns 12 demo Inbox messages, includes Reply-To and scoped-count fields |
| `git diff --check` | Pass |

Production bundle: JS 273.97kB (85.49kB gzip), CSS 33.83kB (7.48kB gzip). The app bundle is approximately 14.40MiB; the unsigned ARM64 DMG approximately 6.63MiB. Build output includes the upstream imap-proto future-Rust-compatibility diagnostic; current tests/build/Clippy succeed.

## Observable regressions

The mixed MIME and mixed-offset date fixtures failed before their fixes. The AI-origin test failed before origin-bound credentials; its later orphan-legacy-key case also failed before removal of the fallback. Each now passes against the production paths, with no real credential store or provider credentials used by the AI fixture.

Smoke output:

```text
Alice smoke passed: auth, draft validation, shared CLI/API state, reply drafts, send idempotency, terminal control sanitization, restart persistence, terminal isolation, and graceful shutdown.
```

PTY output:

```json
{
  "cursor_edit": "HellXo",
  "reply_to": "support@example.invalid",
  "quit_during_request_seconds": 0.0,
  "new_account_from": "new-real",
  "second_page": "51–100 of 125"
}
```

The packaged-app probe used its normal entry point with a disposable data directory and loopback port. Its web frontend initialized the Tauri bridge, which started the daemon; the authenticated API then exposed the new contract. It terminated the app, requested daemon shutdown and removed its data directory. It did not visually inspect native WebKit or OS dialogs.

Real provider authentication, IMAP mutations, SMTP acceptance and paid AI requests were not tested. Windows/Linux packaging, VoiceOver, native visual review and genuine 200% browser text enlargement remain unverified locally. No external email was sent. The fixture probes and review drafts were cleaned up.

After the final assistant-focus-only CSS correction, the TypeScript/Vite and app/DMG builds were repeated. Rust/CLI/TUI code did not change. Browser reports precede this last CSS correction; the final rule and independent contrast calculation are recorded separately.
