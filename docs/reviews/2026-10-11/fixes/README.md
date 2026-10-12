Method: dual-agent (A: /root/ui_design_review · B: /root/ui_evidence_review)

# Alice audit fix confirmation

All **20 ranked findings and four original minor UI observations** have fixes. Two Ponytail code passes and two bounded post-fix UI/UX batches were completed using Impeccable **4.5.2**. Follow-up findings were also fixed. The final technical pass caught a remaining assistant focus override; that last CSS correction was checked against the known token and measured surfaces, then the app was rebuilt. It was not followed by a third browser inspection round.

The [resolution matrix](resolution-matrix.md) maps every original finding to its implementation and evidence. The [original audit](../README.md) remains unchanged. [Verification](evidence/verification.md), [performance measurements](evidence/performance.md), [design assessment A](final-design-a.md), and [technical assessment B](final-technical-b.md) preserve the practical limits of each check.

## Code outcome

What this repo does: Alice provides a desktop email client and Rust command-line/terminal interfaces. The review assumed one person using those interfaces together, with multiple accounts and a growing local cache.

The corrected flows include serialized send/edit/discard, durable unfinished drafts, provider-and-origin-bound keys, sanitized terminal output, Reply-To, complete inline MIME bodies, moved-message reconciliation, provider-backed Sent flags, Sent-copy merging, real-instant sorting and scoped SQL pagination/counts. The terminal refreshes accounts, keeps input responsive during network work, pages through results and edits at a visible UTF-8 cursor. No dependency or additional platform layer was added.

The second code pass found that a missing scoped key could still fall back to an older provider-wide key. A regression first failed against that behavior. Removing the fallback makes explicit key re-entry necessary; the request never reaches the wrong endpoint.

**Verdict:** all listed code findings are resolved. Provider mutations are supported by controlled fixtures and source tracing; live provider execution remains unverified.

## Design specificity and overall impression

Alice feels authored for reading and answering email: a restrained paper/graphite/violet palette, a sans-serif scanning list and larger serif conversation heading. Familiar folders and reader controls fit the task. The fixes preserve that identity while making its calm promises credible: saved state is explicit, drafts recover, counts match rows and the assistant has usable space.

The strongest improvement is trust during writing. At 850×640, completion controls and save state remain visible while long text scrolls inside the editor. Account registration is still the most technical part of the v0.1 journey, now disclosed before connection with specific guidance.

## Design health

All ten Nielsen heuristics apply to this Operate surface. Assessment A's final score is **31/40 — Good**, compared with **20/40** in the original combined audit and **29/40** in its first post-fix assessment.

| # | Heuristic | /4 | Evidence or remaining scope limit |
|---|---|---:|---|
| 1 | System status | 3 | Visible persistence state and scoped counts; live provider progress untested. |
| 2 | Real-world match | 3 | Familiar mail vocabulary; app registration remains technical. |
| 3 | Control and freedom | 3 | Back, Escape, search clear and discard; no general undo for every mail action. |
| 4 | Consistency and standards | 4 | Named dialogs, state semantics, coherent controls and focus treatment. |
| 5 | Error prevention | 3 | Immediate journal, recovery, send validation and shortcut guards. |
| 6 | Recognition rather than recall | 4 | Visible compose actions, account context, scope and setup guidance. |
| 7 | Flexibility and efficiency | 2 | Useful keyboard paths; bulk/customization remains limited in v0.1. |
| 8 | Aesthetic and minimalist design | 3 | Calm reading hierarchy; some repeated assistant branding. |
| 9 | Error recovery | 3 | Last-edit recovery confirmed; forced provider/backup failure untested. |
| 10 | Help and documentation | 3 | Task-focused OAuth/model guidance and official links. |
| **Total** | | **31/40** | **Good** |

## Technical UI health

B's final observed score is **15/20 — Good**, compared with **10/20** in the original combined audit. This score is retained rather than inventing a post-patch rescore. Its sole final finding, assistant focus, has since been corrected in source.

| Dimension | /4 | Evidence or limit |
|---|---:|---|
| Accessibility | 3 | Names, states, text/icons and stronger focus; native assistive technology untested. |
| Performance | 3 | Bounded pages, small bundle, short motion; large-cache store probe improved separately. |
| Theming | 2 | Shared semantic text/focus tokens; many light surface colors remain literal. |
| Responsive design | 3 | All four supported desktop sizes fit; genuine 200% text enlargement untested. |
| Implementation integrity | 4 | Clean detector, consistent semantics, accurate status/count/setup behavior. |
| **Total** | **15/20** | **Good** |

The deterministic detector returned **0 primary findings, 0 advisory findings, no rule/file hits and no false positives**. Browser measurements found issues outside its pattern checks, including the last assistant focus override. Read-only DOM/style inspection, screenshots and accessibility snapshots supplied evidence; injected overlays were unavailable.

## What is working

- Reading stays dominant. At 1440×900 and 1366×768 the assistant receives its own column; at 1024×768 and 850×640 it uses the workspace pane, with predictable Escape and focus return.
- Writing is recoverable. The controlled last edit survived immediate reload, recipient/subject/body remained intact, and discard followed by reload did not reopen the test draft.
- Completion is visible. The minimum-window compose has Send, Save, Discard, persistence status and its demo-delivery explanation in view. Category chips are now 10px; fields, search and assistant focus share the dark violet.

## Priority issues and minor observations

**Zero known unaddressed findings from these audits.** The final assistant input change is specifically documented in [assistant-final-focus.json](evidence/assistant-final-focus.json): 2px dark-violet outline plus 2px offset, 6.654:1 against white and 6.470:1 against the observed surrounding panel. These are final CSS/contrast calculations, not a post-change browser measurement. The existing 18px margins leave room for the outline without changing layout.

The original minor observations are resolved: Settings stays accessible in the sidebar footer; attachment metadata states provider-only viewing/downloading; platform hints distinguish Cmd and Ctrl; selected-message AI scope says This email. Subsequent compose-footer, field-focus, message-disclosure and Gmail-direction findings are also corrected.

## Cognitive load, emotion and personas

Routine reading/reply has a clear focus, grouping, hierarchy and visible context. The strict four-choice checklist still produces two failures: six familiar folder siblings and five reader actions. That is moderate load under the checklist, without a concrete reason to hide standard mail controls. Registering an OAuth app and choosing a BYOK model remain intrinsic setup work; the guide now makes those requirements explicit.

The inbox is a calm entrance. Recovery and visible saved state reassure the writer; visible completion controls remove the prior small-window friction. Technical account registration remains the emotional low point, now explained before the user tries to connect.

Alex's keyboard paths have scoped shortcuts, clear focus and predictable exits. Sam has named controls, exposed states and reachable completion actions; native screen-reader use remains unverified. Jordan sees demo boundaries and exact setup choices before connection. No new persona-specific defect is asserted beyond the corrected findings.

## Validation and deliverables

All **14 Rust tests**, full-workspace Clippy with warnings denied, formatting, strict TypeScript/Vite, real CLI/API smoke and Unix PTY checks pass. The optimized CLI and macOS app/DMG build successfully. A normal packaged-app startup in a disposable workspace started its daemon and exposed Reply-To and scoped counts. This was a startup/API check, not a native visual review.

The optimized 10,000-message probe improved three one-row Inbox queries from **188ms to 9ms**, with peak resident memory reduced from approximately **107MiB to 10.8MiB**. It excludes first-time index setup and measures local store queries. Full Unicode body search retains only the requested page but still scans scoped bodies; that latency limit is documented in source.

- CLI: `target/release/alice`
- macOS ARM64 installer: `target/release/bundle/dmg/Alice_0.1.0_aarch64.dmg`
- App: `target/release/bundle/macos/Alice.app`
- Updated preview: `docs/preview.jpg`

Questions skipped: zero remaining Priority Issues, fewer than three.

**Not checked:** live Gmail/Outlook/IMAP/SMTP and paid AI execution, Windows/Linux packaging, native VoiceOver/visual UI and genuine 200% text enlargement. The last assistant focus correction was not re-rendered in another browser pass.
**Risk:** local packages are unsigned; missing or older unscoped AI keys require key re-entry. No real email was sent.

## Run notes

Stable target/slug: apps/desktop/src/App.tsx / apps-desktop-src-app-tsx. Ignore list absent. A and B stayed isolated; B's findings entered the parent only after A completed in both post-fix rounds. B ran the detector once per round, with exact JSON retained. The available Brave CUA backend supplied screenshots, computed styles and accessibility snapshots; overlay injection was unavailable because evaluation was read-only. No extra review server was started. Review tabs were closed and viewport/media overrides reset. Controlled draft cleanup was confirmed. Original temporary independent reports are retained alongside their project copies; the snapshot's temporary body is deleted after its write. The user's development server and updated release daemon remain running; the Codex preview opening was queued.
