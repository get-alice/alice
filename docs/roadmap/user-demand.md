# What Mailspring users ask for

Back to [`todo.md`](../../todo.md).

## Appendix C: What Mailspring users ask for

Two sources, pulled on 2026-10-13:

- **GitHub:** issues on [Foundry376/Mailspring](https://github.com/Foundry376/Mailspring) sorted by reactions. Most were closed or migrated to the forum, so the counts measure historical demand, not open work.
- **Forum:** the "features" category of [community.getmailspring.com](https://community.getmailspring.com/c/features/12), sorted by votes. This is where Mailspring tracks feature requests today.

Counts are GitHub 👍 + ❤️ and forum votes. "Alice" is the state today; the last column names the roadmap item that covers it, or marks a **NEW** item this appendix adds.

### Top requests

| Request | GitHub | Forum | Alice | Roadmap item |
|---|---|---|---|---|
| Calendar support | 497 (#199) | 68 (#85) | invite RSVP and add-to-calendar only | P2 Calendar view |
| Use the app without a vendor account (Mailspring ID) | 399 (#33), 61 (#349) | — | no account needed | Alice ahead: keep it that way |
| Make threading / conversation view optional | — | **172** (#291), 7 (#2504) | conversations only in desktop | **NEW** P1 Conversation view toggle |
| PGP / GPG | 144 (#25), 14 (#940) | 60 (#83) | gap | P3 OpenPGP |
| Plain-text-only sending, line wrapping for mailing lists (Linux kernel style) | 101 (#52), 28 (#46) | 2 (#785) | plain text only, no wrapping rules | P1 Plain-text mode; **NEW** P2 `format=flowed` and hard wrap at 72 |
| Exchange / EWS / O365 modern auth | 67 (#34), 16 (#258) | 5 (#944) | O365 via IMAP OAuth | P4 Exchange / Graph backend |
| Proxy support | 52 (#235) | 19 (#185) | gap | P3 Proxy support |
| Flatpak | 54 (#345) | — | gap | P3 Linux packaging |
| Proton Mail Bridge | 45 (#429) | — | untested | P1 Presets (Proton Bridge) |
| Plugin directory | 44 (#363) | 6 (#318) | gap | P4 Plugin system |
| Mark a whole folder read / unread | 41 (#159) | — | gap | P1 Mark all as read |
| Sync settings, templates, signatures across devices | 47 (#44), 11 (#755) | 40 (#265), 5 (#1893) | gap | **NEW** P2 Settings export / import and sync |
| Move emails and folders between accounts | 25 (#580) | 37 (#244), 3 (#1713) | gap | **NEW** P2 Cross-account move and copy |
| Export / back up mail | 18 (#1368), 7 (#1012) | 27 (#325) | gap | P2 Export as .eml and mbox |
| Move or delete a single message in a thread | 27 (#426) | 26 (#320) | actions apply to the conversation | **NEW** P1 Per-message actions inside a conversation |
| Reply / forward from a different account or alias | 10 (#581), 7 (#1133) | 24 (#294), 3 (#655) | From is fixed | P1 Send-as identities (include per-identity SMTP) |
| Mobile apps | — | 23 (#94) | gap | P4 Mobile apps |
| Custom sort order (sender, subject, size) | 9 (#361) | 21 (#470) | date only | **NEW** P3 Custom sort order |
| Saved searches / smart folders | 11 (#514) | 20 (#528) | gap | P2 Saved searches |
| Don't create a "Snoozed" folder on the server | 8 (#1262) | 20 (#677) | n/a | P2 Snooze: keep snooze state local by default (see note) |
| Sync window / storage settings | 7 (#473) | 17 (#170), 7 (#626), 5 (#474) | global window (30d/90d/1y/all) | **NEW** P2 Per-account and per-folder sync settings |
| Labels / tags on non-Gmail IMAP (IMAP keywords) | 9 (#1006) | 15 (#540) | gap | **NEW** P2 IMAP keywords as tags |
| Custom notification sound; per-account and per-folder notifications; notify for folders other than Inbox | 11 (#415), 7 (#360), 18 (#667) | 15 (#530), 14 (#133), 7 (#323) | one on/off switch, Inbox only | **NEW** P2 Notification rules per account and folder |
| Markdown in the composer | 21 (#19), 19 (#1091), 7 (#891) | 13 (#322) | gap | **NEW** P2 Markdown shortcuts in the composer |
| Grammar check | 14 (#1402) | 12 (#329), 3 (#5555) | gap | P3 Grammar check |
| Disable or filter the unified inbox | 20 (#38) | 12 (#136) | unified "All inboxes" always available | **NEW** P3 Choose which accounts the unified inbox includes |
| Custom CardDAV server for contacts | 10 (#1668) | 11 (#531), 4 (#441) | CardDAV supported | parity: confirm custom URL entry works |
| Address book | 19 (#86) | 7 (#752) | read-only import | P2 Contact editor |
| GNOME Online Accounts | — | 11 (#718), 8 (#466) | gap | **NEW** P4 GNOME Online Accounts / KDE accounts integration |
| Import / export mail rules | — | 10 (#130) | n/a | P2 Mail rules (include import/export) |
| Local-only folders (on-device archive) | — | 10 (#784) | gap | **NEW** P3 Local folders |
| Color-code accounts and labels | 18 (#526) | 5 (#658) | gap | P1 Per-account color; **NEW** P2 Label colors |
| Edit as new message / resend | — | 8 (#456) | gap | **NEW** P2 Edit as new |
| Read-only or send-only accounts (IMAP or SMTP optional) | — | 8 (#134) | gap | **NEW** P4 Receive-only and send-only accounts |
| Hide / show folders in the sidebar; customize sidebar | — | 8 (#469), 7 (#1904), 4 (#688) | fixed set | **NEW** P2 Sidebar customization |
| Attachments shown at the top of the message | 7 (#738) | 8 (#772) | below body | **NEW** P3 Attachment position option |
| Mouse back / forward buttons | — | 6 (#761) | gap | **NEW** P3 Mouse back/forward navigation |
| Customize auto-advance after archive / delete | — | 6 (#493) | gap | **NEW** P2 Auto-advance after archive or delete |
| Quick mark read/unread from the message list | — | 6 (#1601) | context menu and shortcut | parity: add hover action |
| Adaptive junk filtering | — | 5 (#834) | gap | **NEW** P3 Local spam classifier |
| Smart inbox views ("New", "Pinned", "Seen") | — | 5 (#2716) | gap | P2 Split inbox |
| Default composing font | 8 (#111), 7 (#1088) | 4 (#711) | n/a | P1 Rich text compose (font defaults) |
| Sieve editor for server-side filters | — | 4 (#1122) | gap | P2 Mail rules (Sieve backend note) |
| S/MIME, signing with a certificate | 12 (#28) | 4 (#2330) | gap | P3 S/MIME |
| LDAP / Global Address List | 13 (#864) | 3 (#330) | gap | **NEW** P4 LDAP and GAL directory lookup |
| Block email tracking | 22 (#244, tracking off by default) | 3 (#828) | no trackers sent | P3 Tracking-pixel detection |
| Warn on Reply All | 7 (#2299, still open) | 3 (#630) | n/a | P1 Recipient warnings (add large reply-all warning) |
| View message source | 13 (#441) | — | gap | P1 Show original |
| Vertical reading pane | 13 (#116) | — | three layouts | parity |
| Automatic dark theme | 7 (#1748) | — | gap | P1 Follow system theme |
| Run minimized at startup | 7 (#1953) | — | gap | P1 Launch at login (add "start hidden") |
| Sync local drafts to the server | 7 (#1417) | — | gap | P0 Save drafts to the server |
| Empty Trash / Spam | 6 (#57) | 2 (#1466) | gap | P1 Permanent delete, empty folder |
| Portable Windows build | 13 (#922) | — | gap | **NEW** P4 Portable Windows build |
| Winmail.dat (TNEF) attachments | — | 1 (#712) | gap | **NEW** P2 TNEF decoding |
| Temporarily disable an account | — | 3 (#442) | gap | **NEW** P2 Pause an account |
| POP3 | 8 (#45, rejected) | — | gap | skip, like Mailspring |
| Migrate from Electron to Tauri | — | 4 (#5944) | Alice is Tauri | Alice ahead |

### Recurring complaints worth designing against

- **Local database and sync problems:** "problem with local email database" (120, #2373), stuck "Syncing your Mail" (#158), three-day initial syncs that slow the machine (#1007), high CPU (#1344), lag on long threads (#720). Alice should cap initial sync cost (it already backfills newest first), run a benchmark fixture in CI, and clip very long threads.
- **Disappearing or duplicated mail:** emails disappearing from Inbox (#304), duplicate Sent copies on Outlook.com (#937), sending duplicates (#1086), empty or lost sends (#948, #1107, #851). Alice's at-most-once send claim already covers duplicates; the multi-folder model (P0) and Sent-copy detection (P0) cover the rest.
- **Account setup failures:** Outlook / O365 (#1844, #1118, #1208, #1811), SMTP errors 17 and 27 (#620, #341), TLS 1.3-only servers (#2507, still open). Autodiscovery plus a verbose setup log (P1, P2) address most of these. Alice has the same TLS 1.3 problem today: IMAP and SMTP go through `native-tls` (`crates/alice-core/Cargo.toml:26`, `crates/alice-core/src/providers.rs:45`), which on macOS uses Secure Transport and stops at TLS 1.2. Switching mail connections to rustls (already in the dependency tree through reqwest) fixes it; see P1 IMAP STARTTLS item. Add a TLS 1.3-only server to the IMAP test matrix.
- **Config corruption on crash** (#463). Not a risk for Alice today: preferences live in the SQLite store, and the only plain file the daemon writes is its token (`crates/alice-core/src/daemon.rs:1097`). Keep it that way when settings export (NEW P2) adds files.
- **Black-on-black text in dark mode** (#1290) and inverted images (#536). Keep the light card default; any dark rendering must handle explicit colors.
