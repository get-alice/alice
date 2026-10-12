# Alice audit fixes

The original audit remains in [../README.md](../README.md). All 20 ranked findings and its four minor UI observations have implementation fixes. Evidence distinguishes controlled fixture checks, browser behavior, and source tracing; no live email-provider or paid AI-provider result is claimed.

| # | Original finding | Change | Verification |
|---|---|---|---|
| 1 | Send/edit/discard race | Draft writes and discard acquire the existing shared delivery/sync lane before validation and storage. | Authenticated HTTP concurrent edit/discard test; send validation and duplicate-send smoke checks. |
| 2 | Draft loss and false saved status | Immediate device journal, ordered daemon autosaves, unfinished-recipient storage, explicit save state, startup recovery. Recovery cannot replace an already open compose. | Both browser recovery checks; actual daemon rejects malformed recipients only at delivery and permits subsequent editing. |
| 3 | Key reused at another AI origin | Credential IDs include provider and canonical origin; changed origin requires a fresh key. Removed the legacy provider-wide fallback. | Two loopback origins and an isolated key store; independent configurations retain their own keys. Missing scoped key plus orphan legacy key produces no request. |
| 4 | Terminal controls in email | Remove control characters from human CLI/TUI output while preserving raw JSON data and ordinary line breaks. | Real CLI smoke fixture includes ESC, C1, OSC and bell; readable body remains, JSON retains the original body. |
| 5 | Reply-To ignored | Parse and store Reply-To; desktop, CLI and TUI prefer it and fall back to From. | MIME fixture, shared reply helper callers, actual TUI reply payload. |
| 6 | Mixed MIME body truncated | Choose one multipart/alternative representation; join readable inline mixed sections and exclude attachments. | Mixed-body fixture contains two inline sections, alternatives and an attachment. |
| 7 | Moved/deleted cache ghosts | Track COPYUID destination UID/validity; verify a Message-ID fallback; reconciliation removes unresolved or absent provider records. | Actual MOVE response parser and store deletion fixtures. Live IMAP mutation remains untested. |
| 8 | Sent flags only local | Real Sent flags pass through provider credentials and IMAP; missing provider identity reports a synchronization error. | Authenticated real-account Sent fixture fails without provider credentials instead of returning local success; source traces remote modify. |
| 9 | Two Sent copies | Merge imported Sent copy with the delivery record by account and Message-ID, retaining local UUID and delivery status. Restrict receipt merging to Sent. | Store fixture plus formatted SMTP Message-ID; an incoming same-ID record cannot overwrite the frozen Sent receipt. |
| 10 | Mixed-offset date order | Normalize imported INTERNALDATE to UTC; sort existing cache values by actual instant with a stable ID tie-break. | Different-offset fixture reproduces the original failure and now sorts correctly. |
| 11 | Category paging and false counts | SQL category filtering precedes paging; all/unread totals share folder/account/category/search scope. | Store page/count/Unicode-search fixture; browser Work and unread counts. |
| 12 | Assistant overlap and trapped focus | Reserve a fourth column where it fits; use the content pane at smaller sizes; focus opening control/input and restore the opener on Escape. | Four shipped desktop sizes, browser geometry and keyboard behavior. |
| 13 | Weak text/icon contrast | Semantic foreground tokens for text, placeholders and icons; stronger focus indicators for fields and search. | Browser-computed contrast pairs and keyboard focus measurements. |
| 14 | Missing dialog/state semantics | Named native dialogs; current/pressed/read/star/expanded states. Message actions disclosure has an ID and controls relation. | Browser accessibility snapshots and open/closed disclosure checks. |
| 15 | Global character shortcuts | Character commands require mail-list/reader focus; interactive targets, modifiers, composition and dialogs are excluded. | Source tracing and keyboard check from a focused toolbar button. |
| 16 | Stale TUI accounts | Refresh accounts with each page; prefer real accounts and show From. F2 selects an account for new drafts. | A real binary in a PTY sees an externally added fixture account and submits from new-real. |
| 17 | Network freezes TUI | Background read/mutation tasks; input and rendering continue while network work is pending. | Quit during a four-second PATCH completes immediately in the PTY fixture. |
| 18 | TUI first-page limit | Request 50-message pages; bracket keys change offset; show total and page range. | Real TUI fixture displays 51–100 of 125. |
| 19 | Setup prerequisites too late | Show provider registration steps before fields, official help links, Gmail app-password alternative, model/endpoint/key guidance. | Browser Gmail/Outlook/AI Settings; official provider documentation verified. |
| 20 | TUI cursor ignored | Visible UTF-8 cursor; insertion/deletion, arrows, line navigation, Home/End and editor scrolling. | Unicode unit test and real PTY Hello + Left + X produces HellXo. |

## Minor observations resolved

- Settings stays in a pinned sidebar footer; the accounts/navigation area scrolls separately.
- Attachments state the provider-only viewing/downloading limitation beside their metadata and are not presented as working download buttons.
- Shortcut hints use Cmd on macOS and Ctrl elsewhere.
- AI's selected-message scope is named This email.

## Additional findings resolved in the final batch

- Compose's editor now shrinks and scrolls at the 850×640 minimum. Actions, save status and demo-delivery explanation stay visible. Long error feedback scrolls separately.
- Fields and the compose body inherit a dark violet focus ring; search and the assistant input expose the same color around their containers.
- More message actions exposes expanded state and its disclosure target.
- Gmail guidance names Use a Gmail app password below rather than pointing above it.
- Compact list category labels increased from 7px to 10px.

## Review passes

Ponytail fix pass 1 traced delivery, synchronization, parser, store, credential, CLI and TUI paths with their callers. Pass 2 revisited trust boundaries and persistence after the changes; it caught the legacy-key fallback, which was removed after a failing regression check. No dependencies or new platform layers were added.

Impeccable 4.5.2 used isolated design and technical reviewers. Their first post-fix batch covered 1440×900, 1366×768, 1024×768 and 850×640, then all remaining concrete UI findings were fixed together. The second batch is the bounded confirmation pass. Assessment A finishes before the parent reads Assessment B. Temporary review tabs and viewport/media overrides are cleaned up by their owners.

## Verification limits

Live Gmail/Outlook/IMAP/SMTP operations, paid AI responses, native VoiceOver and actual 200% browser text enlargement were not exercised. Windows/Linux build jobs are configured but were not run locally. Mobile remains outside this desktop fix scope. Unsigned local packages are development artifacts.

The final technical confirmation caught one remaining assistant focus override. It was corrected after that bounded browser pass, using the same independently measured dark-violet treatment already confirmed in search. [assistant-final-focus.json](evidence/assistant-final-focus.json) records the final rule and contrast calculation. No third browser inspection round was run; its post-change browser measurement is explicitly not claimed.
