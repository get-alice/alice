---
target: Alice desktop UI and UX
total_score: 20
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 5
target_identity: "file:/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/apps/desktop/src/App.tsx"
target_fingerprint: "sha256:7a9e585aebe1b60c1de4aaabc4be35117b98e811b3e60aab6cc056f4abe1d2f0"
target_path: /Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/apps/desktop/src/App.tsx
timestamp: 2026-10-11T07-24-12Z
slug: apps-desktop-src-app-tsx
---
Method: dual-agent (A: /root/ui_design_review · B: /root/ui_evidence_review)

**Design specificity verdict:** Alice feels authored for email. The paper-like reader, serif message headings, restrained violet accents, and familiar mailbox structure form a coherent identity. Preserve it while repairing workflow trust and accessibility.

**Deterministic scan:** Impeccable v4.5.2 / engine v0.1.14 returned **0 findings**, 0 advisory findings, and no false positives for apps/desktop/src. Browser measurements and behavior tests found problems the detector does not cover. Read-only browser evaluation prevented overlay injection; no live detector overlay is available.

**Overall impression:** The visual direction is strong. The largest opportunity is making its calm promises match real behavior: recover drafts, keep mail visible while using AI, expose state, and explain setup.

**Design health**

| # | Nielsen heuristic | Score / 4 | Key issue |
|---|---|---:|---|
| 1 | Visibility of system status | 2 | Saved status and filter counts can be misleading |
| 2 | Match with the real world | 2 | Familiar mail vocabulary gives way to OAuth jargon |
| 3 | User control and freedom | 2 | Assistant Escape/focus behavior is inconsistent |
| 4 | Consistency and standards | 2 | Selected views lack accessible state |
| 5 | Error prevention | 2 | Human AI approval is good; draft recovery is missing |
| 6 | Recognition rather than recall | 2 | Provider setup requires external knowledge |
| 7 | Flexibility and efficiency | 2 | Useful shortcuts, but global scope and AI layout hinder use |
| 8 | Aesthetic and minimalist design | 3 | Strong hierarchy; tiny pale peripheral text |
| 9 | Error recovery | 2 | Some errors name the failure without guiding recovery |
| 10 | Help and documentation | 1 | Shortcut hints exist; account setup guidance is insufficient |
| **Total** | | **20/40** | **Acceptable; significant improvements needed** |

All ten apply to this Operate surface.

**Technical UI health**

| Dimension | Score / 4 | Key finding |
|---|---:|---|
| Accessibility | 2 | Contrast, names/state, character shortcuts, hidden focus |
| Performance | 3 | Small build and short motion; repeated mailbox polling |
| Theming | 1 | Six color tokens, but 255 literal-color occurrences and late overrides |
| Responsive design | 2 | Basic layouts fit 850×640; assistant obscures content |
| Implementation integrity | 2 | Coherent design, but saved-state and count promises are unreliable |
| **Total** | **10/20** | **Acceptable; significant work needed** |

Assessment B initially scored integrity 3 and total 11/20; the confirmed unsaved-draft loss and misleading “All changes saved” state justify the final integrity score of 2. Desktop priorities map to findings 2, 11–15, and 19; terminal UX maps to 16–18 and 20.

**What works**

- The message body stays visually dominant. Body contrast measured 5.68:1, unread sender text 6.67:1, and the primary button 4.71:1.
- BYOK disclosure, explicit AI scope, draft-only replies, and human approval create a understandable boundary around automation.
- Native controls and labeled fields support basic keyboard use. Blank compose/settings open with useful focus and close with Escape; long search input produced a useful no-results state without horizontal overflow.

**Five UI/UX priorities**

1. **[P1] Protect writing and make saved state truthful** — finding 2.
   **Why:** Losing a reply breaks the trust established by “All changes saved.”
   **Fix:** Persist incomplete work, recover it after reload, and distinguish saving/saved/error.
   **Suggested command:** $impeccable harden.

2. **[P1] Give the assistant a deliberate layout and focus model** — finding 12.
   **Why:** Asking about mail currently hides that mail and permits invisible keyboard focus.
   **Fix:** Reflow when space permits; use a focused assistant view otherwise, with Back, Escape, and focus restoration.
   **Suggested commands:** $impeccable adapt, $impeccable harden.

3. **[P1] Make secondary information and state accessible** — findings 13–15.
   **Why:** Pale text, unnamed dialogs, silent selection, and global character shortcuts exclude low-vision, speech, and keyboard users.
   **Fix:** Correct measured contrast pairs, name dialogs, expose active states, and scope character shortcuts.
   **Suggested commands:** $impeccable colorize, $impeccable harden.

4. **[P1] Align filters, pages, and counts** — finding 11.
   **Why:** Five visible Work messages with a total of twelve, and unread badges on empty Drafts, undermine navigation.
   **Fix:** Use one server-side filter scope for rows, pagination, totals, and unread counts.
   **Suggested commands:** $impeccable harden, $impeccable clarify.

5. **[P1] Make connecting a real account understandable** — finding 19.
   **Why:** A first-time user expects sign-in and meets app-registration vocabulary.
   **Fix:** Show prerequisites early, link exact setup steps, and clearly distinguish supported connection routes.
   **Suggested commands:** $impeccable onboard, $impeccable clarify.

**Browser evidence at 1280×720:** opening the assistant clips the subject and paragraphs beneath its 340px panel.

![Alice assistant covering the message reader](/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/docs/reviews/2026-10-11/assistant-overlap.jpg)

**Cognitive load:** The inbox has low load: familiar groups and a clear reading hierarchy. The account form has three checklist failures—one decision at a time, working-memory support, and progressive disclosure—so load becomes moderate. Its two top-level settings choices, three mail providers, and two AI scopes are manageable; six folder destinations exceed four but are a familiar grouped navigation set. The avoidable load is carrying registration/permissions/callback information between provider consoles and Alice.

**Emotional journey:** The reader is the high point: space, legibility, and a calm rhythm. Account registration is the first valley; the assistant covering mail is the second. Human approval reassures users around AI, but losing a draft creates the worst ending to a writing session.

**Personas**

- **Alex, power user:** Work counts disagree with rows; assistant navigation reaches a covered button; TUI requests block input and browsing stops after 100 messages.
- **Sam, accessibility-dependent user:** Text/icon contrast misses AA, dialogs are unnamed, active views are not announced, and character shortcuts can fire away from mail. Screen-reader behavior itself was not tested.
- **Jordan, first-time user:** Client IDs and delegated permissions appear before sufficient guidance. AI setup also asks for an exact model ID; linking provider model/setup guidance would reduce guesswork.

**Four smaller observations left unranked**

- At 850×640, Settings initially lies below the viewport; the sidebar scrolls, so it remains reachable.
- Attachment buttons explain the preview's download limit only after clicking; expose that boundary in the affordance.
- Windows/Linux shortcut copy shows the Command symbol although the handlers also support Control.
- “This conversation” currently means one selected email; use wording that matches the documented absence of threading.

**Motion and responsive limits:** Short transform/opacity transitions are appropriate and no layout-property animation was found. Reduced-motion emulation confirmed the blanket .01ms rule; sync loses its moving indicator without a changing text label, but the busy sync path was not exercised. True 200% zoom, native WebKit behavior, physical devices, and frame-rate smoothness remain unverified; phone layouts were not treated as shipping requirements.

**Recommended sequence:** $impeccable harden for persistence/state/accessibility; $impeccable adapt for the assistant; $impeccable colorize for measured contrast; $impeccable onboard and $impeccable clarify for setup; then $impeccable polish. Re-run the audit and critique after fixes.

**Questions for the next pass**

- Which first batch matters most: draft/security/mail correctness, or keyboard/accessibility/assistant layout?
- At the minimum desktop size, should AI open a focused assistant view with Back, or share a reflowed workspace with the email?
