# Alice daemon API v1

Base URL: `http://127.0.0.1:4815`. Every request requires `Authorization: Bearer <daemon.token>`. The token is stored in the Alice data directory and is never a frontend setting. This is a local, same-user interface; do not expose it on the network.

JSON uses `snake_case` to match the shared Rust models. Input and output types live in `crates/alice-core/src/model.rs`; the desktop's corresponding types are in `apps/desktop/src/api.ts`.

## Language negotiation

Send `Accept-Language: en-US` or `Accept-Language: de` for Alice-owned human-readable messages. Weighted preference lists are supported, for example `de-AT, en-US;q=0.8`; regional variants match their supported language. Missing headers, unsupported languages, or ranges with no acceptable supported language fall back to `en-US`. Responses include `Content-Language` with the resolved tag.

The header affects Alice-owned errors, request-returned contact/calendar source errors, OAuth callback pages, RSVP email prose and the AI's requested response language. Status codes, JSON identifiers, protocol values and original mail content are unchanged. Raw provider/library messages and persisted background diagnostics are not translated. Clients should branch on HTTP status or `error.code`, never localized message text.

The `language` preference accepts `"system"` (default), `"en-US"` or `"de"`; unsupported values return 400. It controls desktop presentation, tray labels and notifications. It is not a daemon-wide request locale: clients resolve their preference and send `Accept-Language` for each request, so simultaneous clients can use different languages.

## Endpoints

| Method | Path | Input / result |
| --- | --- | --- |
| GET | `/v1/health` | `{status, version}` |
| POST | `/v1/shutdown` | Stops the daemon gracefully; `{status: stopping}` |
| GET | `/v1/state?account_id=…` | `{accounts, unread_count, ai, preferences, syncing, revision}`; `unread_count` counts unread Inbox conversations, as `/v1/threads` does; `syncing` is true while any account is syncing |
| GET | `/v1/preferences` | `Preferences`: `{language: system\|en-US\|de, theme: light\|dark, mail_layout: single\|horizontal\|vertical, show_avatars, density: comfortable\|compact, message_format: html\|plain, remote_images: ask\|always, thread_order: newest_first\|oldest_first, sync_window: 30_days\|90_days\|1_year\|all, signature, default_account_id, notifications, menu_bar, default_calendar, system_contacts}`. `language` selects desktop presentation (default `system`); `density` applies to the whole desktop app; `message_format` is how the reader shows mail that has HTML; `remote_images` applies to HTML; `thread_order` is where a conversation's newest message appears; `sync_window` is how far back mail is kept on this device (default `90_days`; changing it starts a sync); `notifications` turns desktop new-mail notifications on; `menu_bar` shows the desktop app's menu bar (system tray) icon; `default_calendar` is the `Calendar.id` that "Add to calendar" uses first (cleared when its account or server is removed); `system_contacts` makes the desktop app import the operating system's address book (macOS Contacts) |
| PATCH | `/v1/preferences` | Any subset of `Preferences`; returns the saved `Preferences`. Signatures are limited to 10,000 bytes |
| GET | `/v1/updates` | `{status: available\|current, version, url}`, `{status: unavailable\|error, message}` without or failing `ALICE_RELEASE_FEED` |
| GET | `/v1/accounts` | Account array, without secrets. Each account has `integrations: {contacts, calendar}`: the address book and calendar access it has granted besides mail |
| POST | `/v1/accounts` | `ConnectAccount`; `{account}` for password flow, or `{url, flow_id}` for OAuth. Signs in and finds the account's folders first; returns after the newest mail is downloaded, and older mail in the sync window follows in the background |
| DELETE | `/v1/accounts/:id` | Removes an account with its cached mail, drafts and delivery records, clears it as the default sending account, then deletes its stored sign-in. `{removed, credentials_removed, message}`; `credentials_removed: false` means the OS credential store refused and the item must be deleted manually. The demo inbox returns 400 until a real account exists. Provider mail is untouched |
| POST | `/v1/accounts/:id/integrations` | `{contacts?, calendar?}`. Turning access off is immediate and returns `{account}`; turning contacts off also forgets that account's address book. Turning on access the account has not granted opens a browser consent, as connecting does, and returns `{url, flow_id}`. Only Gmail and Outlook accounts signed in through Google or Microsoft can grant access; others return 400 and use CardDAV/CalDAV servers instead. Google asks for `contacts.readonly`, `contacts.other.readonly`, `calendar.readonly` and `calendar.events`; Microsoft for Graph `Contacts.Read` and `Calendars.ReadWrite`. If the person unticks a permission, the flow ends in `error` and the account keeps what was granted |
| POST | `/v1/demo` | `{}`; adds the demo inbox (account `demo`, messages `demo-1`…`demo-13`; `demo-13` is a calendar invitation) and returns its `Account`. Idempotent. New installs have no accounts until one is connected or the demo is added |
| GET | `/v1/connections/:id` | `{status: pending}` or `{status: connected, account_id}` (integration consents also return the updated `account`) or `{status: error, message}` |
| GET | `/v1/contacts?q=…&account_id=…&limit=…` | Recipient suggestions, best first: `[{name, email, sources}]`, `sources` from `mail`, `google`, `microsoft`, `carddav`, `system`. `q` matches the start of the name, any word of it, the address or its domain, ignoring case and accents; without `q`, the people you write to most. People you have written to rank far above people who only wrote to you, and recent mail counts more (90-day half-life); your own addresses and automated senders you never wrote to (`no-reply`, `mailer-daemon`, bounces, notifications) are left out. With `account_id`, that account's correspondents and address book come first. `limit` 1–50, default 8. Names from address books win over names in mail |
| POST | `/v1/contacts/sync` | `{}`; refreshes the Google, Microsoft and CardDAV address books now: `{contacts, errors: [{source, message}]}`. They also refresh at daemon start and every six hours. Address books are only read, never changed |
| PUT | `/v1/contacts/system` | `{contacts: [{name, email}]}`; replaces the operating system address book's contacts (sent by the desktop app); `{contacts}` stored. Invalid addresses are skipped |
| DELETE | `/v1/contacts/system` | `{removed}` |
| GET | `/v1/dav` | CardDAV and CalDAV servers: `[{id, kind: carddav\|caldav, name, url, username, last_synced, error}]` |
| POST | `/v1/dav` | `{kind, name, url, username, password}`; signs in and finds the address books or event calendars at `url` (a server root, an account's home or one collection; HTTPS required, HTTP only for localhost) before saving. The password goes to the OS credential store. A CardDAV server's contacts are imported at once; an import failure is recorded in `error`. Returns the server |
| DELETE | `/v1/dav/:id` | Forgets the server, its contacts and its stored password, and clears it as the default calendar. `{removed, credentials_removed, message}` as for accounts |
| GET | `/v1/messages` | `MessageQuery`; `{messages, total, all_total, unread_total, revision}` |
| GET | `/v1/threads` | `MessageQuery`; conversations, newest first: `{threads, total, all_total, unread_total, revision}`, counted in conversations. Messages of an account form one conversation when their subjects match after `Re:`/`Fwd:`-style prefixes are removed; drafts and empty subjects stand alone. Each thread is `{id, message, ids, unread_ids, count, participants, is_starred}`: `message` is the newest message in the view, `ids` the conversation's messages in the view (newest first), `unread_ids`/`count`/`participants` cover the whole conversation outside Trash, and the account's own messages appear as `me` in `participants` |
| GET | `/v1/threads/:id` | The conversation of message `:id`, oldest first. Trash is included only when `:id` is in Trash; a message stored in several folders (Gmail's All Mail) appears once |
| GET | `/v1/messages/:id` | One `Message`; does not mark read |
| PATCH | `/v1/messages/:id` | `{is_read?, is_starred?, folder?}`; updated `Message` |
| DELETE | `/v1/messages/:id` | Discards only an unsent local draft; never deletes provider mail |
| POST | `/v1/messages` | `Compose`; saves or updates a draft |
| POST | `/v1/messages/:id/send` | Explicitly sends a frozen draft; returns the Sent `Message` |
| POST | `/v1/messages/:id/rsvp` | `{response: accepted\|tentative\|declined, comment?}`; answers the message's invitation by mailing an iTIP `REPLY` (RFC 6047) to its organizer through the account's SMTP, then records `invite.response` on every stored copy and returns the `Message`. Only `REQUEST` invitations that are not cancelled can be answered. The demo inbox records the answer without sending |
| POST | `/v1/calendar/ics` | `{message_id}` or `{event}`; `{filename, ics}`, an iCalendar file to import into a calendar app. It has no `METHOD`, so apps add the event instead of treating it as another invitation; an invitation keeps its `UID` |
| GET | `/v1/calendars` | `{calendars: [{id, name, source: google\|microsoft\|caldav, owner, primary}], errors}`: the calendars Alice can add events to, from accounts with calendar access and CalDAV servers. `id` is opaque |
| POST | `/v1/calendar/events` | `{calendar, message_id}` or `{calendar, event}`; adds the invitation or event to that calendar and returns `{calendar, url?}`, `url` linking to the event when the provider gives one. Google imports invitations with their `UID`, attendees and recurrence; Microsoft receives one event without its recurrence; CalDAV stores the iCalendar file and refuses a duplicate `UID` with 409 |
| GET | `/v1/calendar/busy?from=…&to=…` | `{busy: [{start, end}], sources, errors}`: merged busy times across connected calendars, RFC 3339 UTC, no titles. `from` defaults to now and `to` to seven days later, at most 62 days. Google uses free/busy for your own calendars, Microsoft your default calendar's events not shown as free, CalDAV a server-expanded event query |
| GET | `/v1/messages/:id/attachments/:index` | The decoded bytes of attachment `index` of the message's `attachments`, fetched from the mail server on request (`application/octet-stream`). The original file name is in `Content-Disposition` and, percent-encoded UTF-8, in `X-Alice-Filename`. Attachments are never stored by sync |
| POST | `/v1/sync` | `{}`; `{synced, errors: [{account_id, message}]}`. Checks every account for new mail, flag changes and removals now, and returns when that is done; older mail in the sync window keeps downloading in the background. `synced` counts server messages added, removed or changed |
| GET | `/v1/storage` | `{database_bytes, accounts: [{account_id, messages, bytes, folders: [{folder, messages, bytes}], sync: {active, downloaded, total, last_synced, error}}]}`. `bytes` is cached mail (headers and bodies); `database_bytes` is the whole database file, including the search index. `downloaded`/`total` count server copies in the sync window that are on this device and on the server |
| GET | `/v1/arrivals?after=…` | `{cursor, messages}`: new mail recorded by this daemon's syncs after `cursor`, oldest first, each `{id, account_id, from_name, from_email, subject, preview}`. Without `after`, `messages` is empty and `cursor` marks now; pass the returned `cursor` next time. A cursor from an earlier daemon process returns everything the current one recorded (the newest 200). New mail is unread Inbox mail the server received since the account's previous successful sync, allowing ten minutes for mail that arrived during that sync and for clock differences; an account's first sync adds none |
| GET | `/v1/ai/settings` | Public `AiSettings`, without the key |
| POST | `/v1/ai/settings` | `SaveAiSettings`; stores a key in the OS vault, returns public settings |
| POST | `/v1/ai` | `{prompt, message_id?}`; validated `{text, draft?, actions, event?}` proposal. `event` is an `EventDraft` proposed for a calendar when asked |

List queries (`/v1/messages`, `/v1/threads`) accept `folder` (`inbox`, `starred`, `drafts`, `sent`, `archive`, `trash`), `account_id`, `q`, `unread`, `category`, `offset`, and `limit` (1–200, default 100). Without `q`, results are sorted by timestamp, newest first. Category and account filters apply before paging. `all_total` and `unread_total` share the folder, account, category, and search scope; `total` additionally reflects the unread filter. Without an explicit account, results include every account, demo included.

`q` is a search query, evaluated inside the current folder, account and category, so every client gets the same results:

| Syntax | Matches |
| --- | --- |
| `sophie invoice` | Messages containing every word, in sender, recipients, subject or body |
| `"studio notes"` | The exact phrase |
| `invoice OR receipt`, `\|\|` | Either side; `AND` / `&&` are optional, parentheses group |
| `-newsletter`, `!word`, `NOT word` | Excludes matches |
| `from:`, `to:`, `subject:`, `body:` | The word in that part only; `from:"sophie chen"` takes a phrase |
| `is:unread`, `is:read`, `is:starred`, `has:attachment` | Flags |
| `after:`, `before:`, `date:` | `today`, `yesterday`, `2026`, `2026-10`, `2026-10-11`, or an age: `7d`, `2w`, `3m`, `1y`. `after:` includes the period, `before:` ends where it starts, `date:` is within it (local time) |

Matching ignores case and accents. Sender, recipients and subject match any substring; bodies match words and word beginnings through a full-text index. An unquoted word of four or more letters that matches nothing exactly may match a sender, recipient or subject word with one typo (two for eight or more letters), and transposed letters count as one. Exclusions are never typo-tolerant. An unknown `key:value` is searched as text. Results come best match first: sender hits rank above subject hits, then recipients, then body-only hits; a typo match counts half as much as an exact match in the same place. Ties are newest first, and a conversation ranks by its best-matching message.

`revision` is an opaque string that changes whenever the daemon's stored data may have changed, including after a daemon restart. A page's `revision` is the one its rows were read at; state reads it before its other fields. A poller can request `/v1/state` alone and refetch a list only when `revision` differs from the list it already shows.

Account sign-in uses loopback port 4816, an unpredictable OAuth state, and PKCE. One sign-in can run at a time; it expires after five minutes. Reconnecting the same email/provider keeps its account ID.

Error responses have the form:

```json
{"error":{"code":"400","message":"Enter a valid account email"}}
```

Status codes: 400 invalid input or provider failure; 401 missing/invalid local authentication; 404 unknown resources; 409 pending, already-submitted, or uncertain delivery; 413 oversized request. Sync reports individual provider failures in its `errors` array so one disconnected account does not hide other accounts' results.

Sending uses the draft's UUID as the durable delivery intent. A UUID can be submitted once, and the saved payload becomes immutable before any SMTP call. A successful retry returns the stored receipt. A pending or uncertain delivery returns 409; it must not be retried with another UUID until the user has checked delivery. Creating a draft, changing flags, and moving a message are separate from sending.

AI output cannot execute side effects. Only `archive`, `mark_read`, and `star` proposals referring to supplied messages are accepted. A client reviews the plan and applies it through the ordinary PATCH endpoint. Apply sequentially and report partial failure; a batch is not transactional across remote mail servers.

Draft saves accept unfinished recipients so typing can be persisted. Send validates the complete recipient list and mail payload before claiming delivery. Draft edits, discard, sync and sending share a serialized mutation lane. The desktop also journals edits immediately on device, queues autosaves, and reopens an interrupted draft after reload.

Messages expose `reply_to` as an address array. Clients reply to it when nonempty, otherwise to `from_email`. `body` remains the plain text representation; `html_body` is the optional original HTML MIME body, or `null`. Older cache rows without this field remain readable and request one fresh provider import. Clients must treat both bodies as untrusted content; the desktop sanitizes HTML and renders it in a sandboxed frame. Human terminal output removes control characters; JSON retains the original text with JSON escaping.

A message carrying a calendar invitation (a `text/calendar` or `application/ics` part, inline or attached, up to 1 MB) has `invite`: `{method, uid, sequence, event, organizer, attendees, recurrence, cancelled, response, ics}`, otherwise `null`. `event` is an `EventDraft`, `{title, start, end?, all_day, location?, description?}`: timed events use RFC 3339 UTC times; all-day events use `YYYY-MM-DD` dates with an exclusive `end`, as iCalendar does. Time zones are read from IANA names, Windows zone names (Outlook) or the invitation's own `VTIMEZONE`. `organizer` and each attendee are `{email, name?, status?}` with `status` `accepted`, `tentative`, `declined`, or `null` before an answer; `response` is this account's answer, from the invitation and then from Alice's reply. `recurrence` is the `RRULE` text when the event repeats. Alice keeps no events of its own: invitations live in their messages, and calendars are read when needed.

AI credentials are stored per provider and endpoint origin. Changing provider, hostname, scheme, or port requires a fresh key. A model change on the same origin can retain the existing key. Missing scoped credentials and older unscoped keys require explicit key re-entry; Alice never falls back to a provider-wide key.
