# Sync fixes from the Mailspring comparison

Research behind the matching items in [`todo.md`](../../todo.md). Each entry covers why it matters, how Mailspring and other clients do it, the relevant specs, what Alice has today, a build plan, pitfalls, acceptance criteria and effort. Code citations point at the Alice working tree as of 2026-10-13; Mailspring paths are relative to [Foundry376/Mailspring](https://github.com/Foundry376/Mailspring) (`mailspring/`) and [Foundry376/Mailspring-Sync](https://github.com/Foundry376/Mailspring-Sync) (`mailsync/`).

## Section findings

These items come from comparing Alice's sync code with Mailspring-Sync's 2025–2026 bug fixes (Appendix D, "Lessons for Alice"). Two are likely bugs in Alice today. The other three are cheap hardening steps that Mailspring learned the hard way. Commit SHAs refer to [Foundry376/Mailspring-Sync](https://github.com/Foundry376/Mailspring-Sync).

## P0 → Sync correctness

<a id="save-a-mailboxs-uid_next-and-highest_modseq-only-after-its-new-mail-is-stored"></a>

### Save a mailbox's `uid_next` and `highest_modseq` only after its new mail is stored

*Roadmap: P0 · Sync correctness · new*

- **Why:** If downloading new mail fails mid-run, those messages are later treated as old backfill. They then skip new-mail notifications and the verification-code queue, and arrive late.
- **Competitors:**
  - Recording a cursor before the work behind it is committed is Mailspring's most repeated sync bug: `2076b05` (about 80% of each initial-sync chunk was never ingested on QRESYNC servers), `eba3adc`, and three cursor fixes inside `a83cc9a`.
  - Mailspring now advances cursors only after the matching messages are written.
- **Spec:** RFC 9051 §7.3.1 (STATUS UIDNEXT); RFC 7162 §3.1 (HIGHESTMODSEQ must only be stored once the client has processed changes up to it).
- **Alice today:**
  - `refresh()` writes the new `uid_next` and `highest_modseq` through `save_mailbox_state` (`crates/alice-core/src/sync.rs:469-475`) before `run_with` fetches the fresh UIDs (`sync.rs:302-317`).
  - On the next run, `floor` is the saved `uid_next` (`sync.rs:454-458`), so UIDs from the failed run fall below it and count as backfill.
  - The OTP queue only considers UIDs at or above `new_since` (`crates/alice-core/src/sync_store.rs:561`).
- **Plan:**
  1. Split mailbox state into "seen" values (validity, exists, window) saved in `refresh()`, and "committed" cursors (`uid_next`, `highest_modseq`) saved after the fresh batches for that mailbox are stored.
  2. When a fetch fails, leave the old cursors in place so the next run recomputes the same fresh set.
  3. Apply the same rule to `highest_modseq` and flag changes: store the modseq only after `apply_flags` commits.
- **Pitfalls:**
  - Don't move the save to the very end of the run. One failing mailbox must not hold back the cursors of the others.
  - Keep `server_total` in sync with the committed state, or the `unchanged` shortcut (`sync.rs:407-410`) can skip the search that would recover.
- **Done when:** a test with a mock remote fails the first fetch of a new UID, then succeeds. The message arrives as new mail: it is in the OTP candidates and the notification list, not backfill.
- **Effort:** S.

<a id="never-skip-the-uid-search-on-servers-that-report-no-uidnext-or-uidnext-0"></a>

### Never skip the UID search on servers that report no `UIDNEXT` or `UIDNEXT 0`

*Roadmap: P0 · Sync correctness · new*

- **Why:** On those servers, one arrival plus one deletion between runs leaves both compared values unchanged. Alice then skips the membership search and misses both changes until something else changes.
- **Competitors:** Mailspring-Sync `dfdb083` handles `UIDNEXT 0` (hMailServer, home.pl) and Coremail's missing `UIDNEXT`, and falls back to a full scan on a timer.
- **Spec:** RFC 9051 §7.3.1. UIDNEXT is required, but servers in the wild omit it or send 0.
- **Alice today:** `unchanged` compares only `uid_next`, `exists` and the window (`sync.rs:407-410`). A missing `uid_next` compares `None == None` and passes.
- **Plan:**
  1. Treat `uid_next` of `None` or `Some(0)` as "always search".
  2. On any server, force a full window search at least every 10 minutes, as Mailspring does for servers without CONDSTORE.
- **Pitfalls:** mind the cost on large windows. The full search is one `UID SEARCH`, so it's cheap compared with fetching.
- **Done when:** a mock-remote test with `uid_next = None` that adds one message and expunges another sees both changes in the next run.
- **Effort:** S.

## P1 → Sync

<a id="keep-a-message-whose-last-copy-vanished-until-the-run-ends-so-a-move-made-in-ano"></a>

### Keep a message whose last copy vanished until the run ends, so a move made in another client isn't a delete and a re-download

*Roadmap: P1 · Sync · new*

- **Why:** When mail is moved from Inbox to Archive in webmail, Alice sees it leave Inbox first. It deletes the message, then downloads it again from Archive, which loses local-only fields such as its category and costs bandwidth.
- **Competitors:** Mailspring keeps orphaned messages in a `MessageOrphan` state and sweeps them at the end of a pass, bounded by per-folder coverage and a 24-hour cap (`SyncWorker.cpp:356-390`, introduced with placements in `a83cc9a`).
- **Spec:** none.
- **Alice today:**
  - `settle` deletes a message as soon as it has no remote copy (`sync_store.rs:266-269`).
  - Inbox is refreshed before Archive in every run (`remote.rs:570-576`, `sync.rs:295-300`), so a move is always seen source-first.
- **Plan:**
  1. During a run, mark copy-less messages as pending instead of deleting them.
  2. At the end of the run, delete only those still without a copy, and only if every mailbox they could have moved to was scanned in that run.
  3. When a later mailbox in the same run finds the Message-ID, relink it to the existing row instead of downloading.
- **Pitfalls:** messages that leave the sync window entirely must still be removed. Use the window check, not just the copy count.
- **Done when:** in a fixture test, a message moved from INBOX to Archive between runs keeps its row id and `category`, and no body fetch happens.
- **Effort:** S–M. Superseded later by the "Multi-folder message model" item, but worth doing first because it is small.

<a id="relink-messages-after-a-uidvalidity-change-instead-of-re-downloading-the-mailbox"></a>

### Relink messages after a `UIDVALIDITY` change instead of re-downloading the mailbox

*Roadmap: P1 · Sync · new*

- **Why:** Dovecot changes UIDVALIDITY when it rebuilds an index, and Cyrus does on `reconstruct`. Alice then forgets the whole mailbox and downloads it again, which can take hours on a large mailbox.
- **Competitors:** Mailspring marks copies "UID unknown", re-reads headers for the window, relinks by Message-ID, and deletes leftovers only after a complete search (`SyncWorker.cpp:509-554`, `MailStore.cpp:1213-1235`).
- **Spec:** RFC 9051 §2.3.1.1. When UIDVALIDITY changes, cached UIDs are invalid, but the messages may be unchanged.
- **Alice today:** `forget_mailbox` drops every copy, deletes messages whose only copy was there, and re-downloads them (`sync.rs:358-367`, `sync_store.rs:313-319`, `675-688`). Alice's message ids depend only on the Message-ID, so relinking is possible.
- **Plan:**
  1. On a UIDVALIDITY change, set the copies' UIDs to unknown, keeping the rows.
  2. Fetch envelope and Message-ID for the window in batches, and relink by Message-ID.
  3. Download bodies only for messages with no match. Delete unmatched copies after the full window search.
- **Pitfalls:** messages without a Message-ID need a fallback key (Alice already derives ids; reuse that rule).
- **Done when:** a Dovecot fixture test that forces a UIDVALIDITY change re-links every message without refetching bodies.
- **Effort:** M.

<a id="store-flag-changes-on-every-server-copy-of-a-message"></a>

### Store flag changes on every server copy of a message

*Roadmap: P1 · Sync · new*

- **Why:** Alice reads and writes read and starred state only on a message's primary copy. Other copies drift on the server, and the unread state can flip back when the primary copy leaves a folder.
- **Competitors:** Mailspring fans flag changes out to every copy (`MailStore.cpp:1021-1041`, `TaskProcessor.cpp:1202-1206`).
- **Spec:** RFC 9051 §6.4.6 (STORE).
- **Alice today:** flags are read from and written to the primary copy only (`providers.rs:97-116`, `sync_store.rs:375`).
- **Plan:**
  1. Group a flag change by mailbox across all `remote_messages` rows for the message.
  2. Send one `UID STORE` per mailbox (reusing the batch-action path from the Organization research).
  3. On read, merge flags with "any copy unread means unread", as Mailspring's per-copy unread bit does.
- **Pitfalls:** on Gmail, labels share one message, so this applies only to non-Gmail servers until the Gmail label work lands.
- **Done when:** marking a message read that is in INBOX and a user folder sets `\Seen` in both mailboxes on a Dovecot fixture.
- **Effort:** S–M. Depends on the batch action endpoint.
