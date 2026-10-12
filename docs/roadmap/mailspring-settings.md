# Mailspring settings and menus, mapped to Alice

Back to [`todo.md`](../../todo.md).

## Appendix A: Mailspring settings inventory

Every setting in Mailspring 1.26's Preferences window (checked in the running app and in `app/src/config-schema.ts`), with Alice's equivalent. Alice's preferences live in `Preferences` at `crates/alice-core/src/model.rs:313`. Rows marked **gap** are covered by a roadmap item above, named in the last column; rows marked **NEW** are added to the roadmap by this appendix.

### General

| Mailspring setting | Default | Alice | Roadmap item |
|---|---|---|---|
| Use Mailspring as default mail client | off | gap | P1 Register as `mailto:` handler |
| Launch on system start | off | gap | P1 Launch at login |
| Show icon in menu bar / system tray | on | `menu_bar` | parity |
| Tray icon style: blue for unread, red for new + blue for unread, or none (`trayIconStyle`) | blue | gap | P1 Dock / taskbar unread badge |
| Tray icon light/dark theme on Linux (`traySystemTheme`) | automatic | gap | P3 Windows jump list and Linux tray icon theme |
| Show Gmail-style important markers (Gmail only) | on | gap | P0 Gmail labels (`\Important` is a label) |
| Show unread counts for all folders / labels | off | gap | P1 Unread counts on every folder |
| Use 24-hour clock | off | follows locale only (`apps/desktop/src/i18n.ts:84`) | **NEW** P2 24-hour clock override |
| Interface language | automatic | `language` (en-US, de) | P2 More locales |
| Interface scaling slider (`interfaceZoom`) | 1.0 | gap | P2 Interface zoom |

### Reading

| Mailspring setting | Default | Alice | Roadmap item |
|---|---|---|---|
| Mark as read: immediately, after 0.5 s, after 2 s, or never (`markAsReadDelay`) | 0.5 s | marks read immediately on open (`apps/desktop/src/App.tsx:180`), not configurable | **NEW** P1 Mark-as-read delay setting |
| Automatically load images | on | `remote_images` (ask / always) | parity |
| Show full message headers by default | off | gap | P1 Show original |
| Show first and last names of all recipients | off | gap | **NEW** P2 Full recipient names option |
| Restrict width of messages | off | gap | P2 Full-width message view |
| Move to trash (not archive) on swipe / backspace | off | gap | **NEW** P2 Delete-key behavior (archive vs trash) |
| Disable swipe gestures | off | n/a (no swipe yet) | P3 Swipe gestures |
| Conversations in descending order | off | `thread_order` | parity |
| Email body appearance: light or dark mode | light | light card only | P1 Dark-mode body rendering option |

### Sending

| Mailspring setting | Default | Alice | Roadmap item |
|---|---|---|---|
| Message sent sound | on | gap | **NEW** P2 Sounds for sent and new mail |
| Default send behavior: Send or Send and Archive | Send | gap | P2 Send and archive |
| Default reply behavior: Reply or Reply All | Reply All | reply only | P0 Reply-all |
| Undo send window: 5, 15, 30, 60 s or off | 5 s | gap | P1 Undo send |
| Send new messages from: selected account or a fixed one | selected account | `default_account_id` | parity |
| Reset emails that ignore warnings | — | gap | P1 Missing-attachment and recipient warnings |

### Notifications

| Mailspring setting | Default | Alice | Roadmap item |
|---|---|---|---|
| Notify for new unread messages | on | `notifications` | parity |
| Notify for repeated opens / clicks | on | n/a | P4 Open and link tracking |
| Play sound for new mail | on | gap | **NEW** P2 Sounds for sent and new mail |
| Resurface messages to the top of the inbox when unsnoozing | on | n/a | P2 Snooze |
| App icon badge: hide, unread count, or total count | unread | gap | P1 Dock / taskbar unread badge |
| Count unread in all accounts, not just the selected folder | off | gap | P1 Dock / taskbar unread badge |

### Composing and attachments

| Mailspring setting | Default | Alice | Roadmap item |
|---|---|---|---|
| Enable rich text (hold Alt/Option for a single plain-text draft) | on | plain text only | P1 Rich text / HTML compose; P1 Plain-text mode |
| Check spelling, spellcheck language | on, system | webview default only | P1 Spellcheck language choice |
| Check grammar (LanguageTool) | off | gap | P3 Grammar check |
| Open containing folder after download | off | always opens file (reveals executables) | **NEW** P2 Attachment download behavior setting |
| Thumbnail previews for attachments | on | gap | P1 In-app attachment preview |

### Local data

| Mailspring setting | Alice | Roadmap item |
|---|---|---|
| Reset Cache (all accounts) | gap | P2 Reset cache for one account |
| Reset Accounts and Settings | gap | **NEW** P2 Factory reset |
| Enable verbose IMAP / SMTP logging (`core.sync.verboseUntil`, time-limited) | gap | P2 Verbose protocol log during account setup |

### Other Preferences tabs

| Tab | What it does in Mailspring | Alice | Roadmap item |
|---|---|---|---|
| Accounts | Per account: label, sender name, automatic CC/BCC addresses, aliases list (with a provider-setup note), account color with reset, Manage Contacts, Re-authenticate, Rebuild Cache. Accounts reorder by drag. | name and settings at setup only | P1 Edit an existing account's settings; P1 Send-as identities; P1 Re-authenticate flow; **NEW** P1 Automatic CC/BCC per account |
| Appearance | Layout picker: Single Panel, Two Panel, Two Panel Vertical. Theme picker (light, dark, darkside, taiga, ubuntu, less-is-more, automatic). Use system accent color. Interface scaling slider. Tray icon style. | layouts at parity (`MailLayout` single / horizontal / vertical, `crates/alice-core/src/model.rs:240`); light and dark themes; 2 densities | P1 Follow system theme; P2 Interface zoom |
| Shortcuts | Preset dropdown (Gmail, Outlook, Apple Mail, Inbox by Gmail), every binding editable, Restore Defaults. Sections: Application, Actions, Composer, Navigation, Jumping, Selection. | fixed shortcuts (`apps/desktop/src/shortcuts.ts`) | P2 Shortcut presets and remapping |
| Mail Rules | Per-account rule list, "Create a new Rule", and a "Process entire inbox" button (rules otherwise run on new mail only). | gap | P2 Mail rules / filters |
| Folders | Per-account mapping of Inbox, Sent, Drafts, Spam, Archive and Trash to any server folder, plus a "Default Container Folder" for new folders. | automatic special-use detection only (`crates/alice-core/src/remote.rs:503`) | **NEW** P1 Manual folder role mapping |
| Signatures | Named signatures with a "Default for: N accounts" picker. Six layout templates plus Raw HTML. Structured fields: name, title, phone, fax, email, address, website, theme color, and links for Facebook, LinkedIn, Medium, GitHub, YouTube, X/Twitter and Instagram. | one global plain-text signature | P0 Per-account signatures |
| Templates | Named templates with an optional subject, rich body, stored as files in a templates folder ("Show Templates Folder…"). | gap | P2 Templates |
| MCP Server | One on/off switch. | `mcp` off / read-only / full | Alice ahead |
| Subscription | Mailspring ID and Pro plan. | n/a | — |

## Appendix B: Mailspring menu commands

From `menus/darwin.js` in the 1.26 bundle. These are the actions Mailspring exposes from the menu bar; each should have an Alice equivalent in the command palette (P2) and, where marked, a roadmap item.

- **File:** Sync New Mail Now, New Message, Print Current Thread (P1 Print).
- **Edit:** Undo / Redo (P1 Undo), Paste and Match Style (P1), Select All Read / Unread / Starred / Unstarred (P1 Multi-select), Mark All as Read (P1), Find in Mailbox, Find in Thread with Next/Previous (**NEW** P2 Find in thread).
- **View:** Reading Pane Off, Horizontal Reading Pane, Vertical Reading Pane (Alice has all three); Go to Inbox, Unread, Starred, Sent, Drafts, All Mail (P2 `g`-prefixed navigation).
- **Thread:** Reply, Reply All, Forward (P0), Forward as Attachment (P0 Forward), Mark Read/Unread, Report Spam / Not Spam, Mark as Important / Not Important (P0 Gmail labels), Star, Snooze (P2), Share this thread (P4), Copy mailbox permalink (P2 Permalinks), Save as .eml (P2 Export), Remove from view / Remove and show next / previous (**NEW** P2 Auto-advance after archive or delete), Move to Trash, Move to Archive, Move to Folder (P1), Apply Label (P0 Gmail labels), Expand / collapse conversation.
- **Developer:** Toggle Screenshot Mode (P3), Open Mailsync Logs (P2 Structured logging), Calendar Preview (P2 Calendar view), Install a Plugin (P4).
- **Window:** Message Viewer, Contacts (P2 Contact editor).
