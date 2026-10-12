# Alice post-fix technical UI / UX assessment B

Method: independent Assessment B (`/root/ui_evidence_review`), first batched post-fix pass. Assessment A and the parent synthesis were not read. Target: `apps/desktop/src/App.tsx`, with the current related frontend files. Live demo: `http://127.0.0.1:1420/`. Impeccable skill 4.5.2; bundled engine version file 0.1.14. Context was not rerun.

## Implementation Integrity Verdict

**Pass, with isolated remaining defects.** The implementation expresses a coherent local mail product: a folder/account sidebar, a scannable message list, a reading surface, explicit demo semantics, and an assistant that asks users to review inbox changes. The deterministic detector returned no findings. Current source and browser evidence support the dialog naming, state semantics, assistant reflow, minimum-window footer access, and draft-status improvements. The remaining defects are weak field focus contrast, one message disclosure without expanded state, and one incorrect directional sentence in account setup.

## Audit Health Score

| # | Dimension | Score | Key finding |
|---|---|---:|---|
| 1 | Accessibility | 3/4 | Ordinary text, placeholders, and enabled icons pass sampled contrast; author-supplied field focus indicators remain below 3:1. |
| 2 | Performance | 3/4 | Bounded 50-message pages, debounced search, stale-response guards, small transform/opacity transitions; no runtime performance benchmark. |
| 3 | Theming | 2/4 | Twelve root color tokens now carry text and semantic colors; many surface/border colors remain literal values. |
| 4 | Responsive Design | 3/4 | All four shipped desktop sizes work, with safe assistant reflow and scrollable dialogs; real 200% text enlargement could not be verified. |
| 5 | Implementation Integrity | 3/4 | Clean detector and coherent product structure; isolated menu semantics and setup-copy defect. |
| **Total** | | **14/20** | **Good — address weak dimensions.** |

Scores apply to the current desktop web implementation. They are not a declaration of full WCAG conformance or a native WebKit / VoiceOver test.

## Executive Summary

**Three concrete defects:** P0 **0**, P1 **1**, P2 **2**, P3 **0**. The former broad text-contrast, unnamed-dialog, CSS-only selection, assistant occlusion, and minimum-height sidebar-footer problems are absent from the current evidence. The remaining P1 is a shared focus-color problem, so one token-level correction can cover settings and compose; search has its own weak focus treatment.

Recommended next batch: strengthen form and search focus indicators, expose the message actions disclosure state, and replace the incorrect “above” reference in Gmail setup. Finish with one targeted confirmation rather than another broad exploration cycle.

## Detailed Findings by Severity

### B-F1 — [P1] Field focus indicators remain too faint

- **Location:** [styles.css:14](/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/apps/desktop/src/styles.css:14), [styles.css:76](/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/apps/desktop/src/styles.css:76), [styles.css:111](/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/apps/desktop/src/styles.css:111).
- **Category:** Accessibility.
- **Verified trigger:** Tab from Model ID into API base URL; Tab from compose Subject into Message body; focus the search field. The DOM snapshot marks the target active, and `:focus-visible` is true. No field contents were entered in compose/settings.
- **Observed:** The settings/compose input outline computes to `#b3a0d0`, 2px solid, against white: **2.371:1**. The compose body outline computes to `#ded2ed`, 2px solid, inset on white: **1.444:1**. Search explicitly removes the input outline and uses a `#b6a4cc` border on `#faf9fc` (**2.183:1**) plus a `#eae2f5` ring against the mailbox (**1.237:1**).
- **Impact:** A keyboard user with reduced contrast sensitivity can lose their place while entering recipients, composing, configuring AI, or searching. The dark focus outline on buttons does not fix the more specific form-field rules.
- **Standard:** Author-supplied focus indicators need 3:1 against their adjacent background under [WCAG 1.4.11 Non-text Contrast](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html). This finding concerns the focus state, not the intentionally subtle unfocused borders.
- **Recommendation:** Use a shared focus token that reaches at least 3:1 on the light field surfaces; apply it to inputs, selects, compose body, and the search focus-within treatment. Retain the existing width and visible offset/inset behavior.
- **Suggested command:** `$impeccable harden`, then a targeted `$impeccable audit`.
- **Evidence:** `b-04-ai-field-focus.json/.jpg`, `b-15-850-compose-focus.json/.jpg`, `b-search-focus.json`, `b-22-850-long-search.jpg`, and `b-summary.json`.

### B-F2 — [P2] More message actions exposes no disclosure state

- **Location:** [App.tsx:183](/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/apps/desktop/src/App.tsx:183).
- **Category:** Accessibility / Implementation Integrity.
- **Verified trigger:** Open More message actions at 850×640.
- **Observed:** The button has a correct accessible name and is keyboard reachable, but `aria-expanded`, `aria-controls`, and `aria-haspopup` are all absent. The displayed popover has no ID or role. The accessibility snapshot continues to announce a plain “More message actions” button while Mark read, Move to Inbox, and Ask Alice are visible. Escape closes the disclosure successfully.
- **Impact:** A screen-reader user has no announced open/closed state or programmatic relation from the trigger to the newly exposed options. The options remain reachable, so this is a contained usability gap rather than a task blocker.
- **Standard:** Disclosure state should be programmatically available; see [WCAG 4.1.2 Name, Role, Value](https://www.w3.org/WAI/WCAG22/Understanding/name-role-value.html). This report does not claim a fully implemented ARIA menu is necessary for these three ordinary action buttons.
- **Recommendation:** Add `aria-expanded={menu}` and an ID/`aria-controls` relation to this disclosure. If adopting menu semantics, implement the corresponding keyboard behavior as well; otherwise keep the simple button disclosure pattern.
- **Suggested command:** `$impeccable harden`.
- **Evidence:** `b-10-850-more-menu.txt/.json/.jpg`.

### B-F3 — [P2] Gmail setup points users above an option that is below

- **Location:** [Settings.tsx:56](/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/apps/desktop/src/Settings.tsx:56); the referenced checkbox is at [Settings.tsx:59](/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/apps/desktop/src/Settings.tsx:59).
- **Category:** Implementation Integrity / UX copy.
- **Verified trigger:** Mail accounts → Add a mail account → default Gmail setup.
- **Observed:** The expanded setup guide says “For a personal account, the app password option above may be quicker.” The Use a Gmail app password checkbox appears after the guide and the Account name / Email address fields. At the minimum size it is below the initial viewport, while the sentence telling the user to look above is visible.
- **Impact:** A first-time user who wants the simpler personal-account setup looks in the wrong direction and may unnecessarily start the OAuth registration steps.
- **Recommendation:** Replace the directional reference with the exact control label, for example: “For a personal account, select ‘Use a Gmail app password’ below.” Moving the checkbox above the guide is also valid, but changing the sentence is the smaller fix.
- **Suggested command:** `$impeccable clarify`.
- **Evidence:** `b-18-850-account-setup.txt/.jpg` and `b-19-850-account-footer.txt/.jpg` show the sentence and actual order together.

## Deterministic Detector

Command, executed once for this post-fix pass:

```sh
rtk proxy /Users/amruth/.agents/skills/impeccable/scripts/impeccable detect --json apps/desktop/src > docs/reviews/2026-10-11/fixes/evidence/b-detector.json
```

- Exit code: **0**.
- Complete stdout: **`[]`**, three bytes including newline.
- Primary findings: **0**; advisory findings: **0**; total findings: **0**.
- Rule names: **`[]`**.
- File locations: **`[]`**.
- Detector false positives: **none**.
- The scan is a clean implementation-pattern signal, not a contrast or complete accessibility certificate. The browser/manual defects above were not detector findings.

## Browser Method and Verification Matrix

A new Brave Work browser tab was created through CUA: **1076629024**. Existing tabs were not reused. The in-app browser was not exposed in this selected browser session; the documented CUA browser backend was used. All DOM/style evaluation was read-only. No script injection, detector overlay, CDP Runtime mutation, custom browser automation, or extra review server was used.

The four requested viewport sizes formed one batched post-fix inspection. Forms were measured at a wide desktop size and the minimum shipped size; the remaining two sizes validated the principal layout transitions. This is the first pass, not a repeated polish loop.

| Size | Views captured | Result |
|---|---|---|
| 1440×900 | Inbox/reader, assistant, AI settings and focused field | Three mail columns remain usable; assistant takes a fourth grid column, starting x=1120 with width=320. Reader ends at x=1120. Toolbar controls do not sit beneath the assistant. |
| 1366×768 | Inbox/reader and assistant | Assistant x=1046, width=320; reader ends at x=1046. No horizontal document overflow or hidden reader-toolbar control. |
| 1024×768 | Assistant and reader after closing assistant | Assistant replaces the content region, x=190, width=834. Reader and list are `display:none` while it is open. Closing restores the reader and its back control. |
| 850×640 | Reader, message disclosure, inbox, assistant, assistant keyboard scroll, compose, account/AI settings, footers, long search, demo sync | Document width remains 850. Reader/list/assistant use the 660px content region separately. Settings is fully reachable in the anchored sidebar footer. Dialog actions scroll into view with keyboard focus. |

Measurements use computed foreground colors and composited ancestor backgrounds, with WCAG luminance ratios. Screenshots confirm the active surface and geometry; measurements are not based on screenshot pixel guessing. Inert content behind a modal backdrop is not used as an active-surface contrast verdict. Geometry probes can mark content below an inner scroll boundary as covered; those candidates were inspected in context and not reported as overlay defects.

### Ordinary Text, Placeholder, and Icon Contrast

| Actual surface | Foreground / background | Ratio | Result |
|---|---|---:|---|
| Sidebar section labels | `#68616f` / `#f7f7f9` | 5.563 | AA text pass |
| Selected unread subject / assistant invitation | `#644582` / `#f0edf7` | 6.688 | AA text pass |
| Read subject on list paper | `#68616f` / `#fdfdfe` | 5.855 | AA text pass |
| Personal category badge | `#68616f` / `#edf2eb` | 5.244 | AA text pass |
| Work category badge | `#644582` / `#eee7f6` | 6.406 | AA text pass |
| Search placeholder/value | `#68616f` / `#faf9fc` | 5.675 | AA text pass |
| Settings input value | `#644582` / `#fefcfe` | 7.575 | AA text pass |
| Settings input placeholder | `#68616f` / `#fefcfe` | 5.830 | AA text pass |
| Compose placeholder | `#68616f` / white | 5.952 | AA text pass |
| Assistant placeholder | `#68616f` / white | 5.952 | AA text pass |
| Primary button label | white / `#8067ad` | 4.707 | AA text pass |
| Assistant footnote | `#644582` / `#fdfbff` | 7.520 | AA text pass |

Enabled toolbar and header icon samples passed 3:1. Disabled controls are excluded. Two low-ratio text candidates were explicitly excluded: the decorative/large brand dot and the Outlook logo mark. The brand dot is 32px and its 4.399 ratio already exceeds the large-text threshold; the Outlook mark is a provider logo with accompanying readable provider/account text. Neither is a new detector finding or ordinary-label contrast failure.

### Accessible Names, States, and Keyboard Reachability

- Settings exposes native modal dialog name **Make yourself at home.** through `aria-labelledby="settings-title"`. Compose exposes **A new conversation** through `aria-labelledby="compose-title"`. All observed form controls have associated labels or an explicit accessible name.
- Folder buttons expose `aria-current="page"`; collection/account filters, All mail / Unread, selected message rows, star, assistant navigation, settings tabs, provider choices, and assistant scope expose pressed states. Message row names include read/unread and starred information. Assistant toggle exposes `aria-expanded` and controls the labelled assistant aside.
- The message menu is the isolated state exception described in B-F2.
- Settings Escape returns focus to **Settings**, confirmed in `b-keyboard-settings-return.txt`. Closing AI settings returns focus to **Connect AI provider**; assistant Escape returned to its toggle in the earlier wide-desktop check. These are browser keyboard checks, not physical screen-reader tests.
- At 850×640, Settings button is x=151, y=595.5, 24×30, bottom=625.5 and unobscured. Sidebar footer spans y=580…640; the middle navigation/account region scrolls independently (client height=395, scroll height=470).
- Compose has scroll height=642 versus client height=561. Tabbing from body to Save draft moves scrollTop to 81 and exposes the action row at y=501.4…573.4. The footer is reachable and no task-blocking clipping was found.
- Gmail connection form has scroll height=1002. Tabbing from its final password field to Connect account moves scrollTop to 441; the button is visible at y=531.6…568.6. AI settings similarly scrollTop=215.5 exposes Connect Alice at y=531.2…568.2. Buttons were focused without submission.
- At minimum height, keyboard focus on What needs my attention? scrolls assistant content to 237.5 and places the full button above the pinned input. No hidden focused assistant control was found.
- Character shortcuts are source-gated to mailbox/reader containers and exclude inputs, buttons, links, selects, summaries, contenteditable, composing input, and modifier chords ([App.tsx:107](/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/apps/desktop/src/App.tsx:107)). Destructive shortcut actions were not executed.

### Overflow / Long Content

All sampled document client/scroll widths match at their requested size. A 1,520-character search value remained inside the search input, left Clear search visible and operable, and produced the contextual empty state with recovery copy. Clearing restored the message list. Plain-text message body uses `overflow-wrap:anywhere` and pre-wrap paragraph content; long real mail subjects, attachments, and multi-account data beyond the demo fixture were not injected.

### Reduced Motion

`Emulation.setEmulatedMedia` set `prefers-reduced-motion: reduce`; the matchMedia result was true. The assistant, controls, and dialogs compute animation `none`, duration `0s`, with `0s` button transitions. Current [styles.css:123](/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/apps/desktop/src/styles.css:123) uses targeted animation removal rather than a global 0.01ms duration kill. Text status remains present: compose displays **Not saved yet** in `role="status"`, and a demo sync produced **Demo inbox is up to date** in `role="status"`. Spinner transitions during real network operations were not exercised.

### 200% Zoom / Text Scaling Limitation

A documented CUA CDP attempt used `Emulation.setEmulatedOSTextScale` with scale=2. The command returned `{}`, but the viewport remained 850×640, DPR=1, root font=16px, settings heading=30px, and field font=11px; dialog dimensions were unchanged. This produced no actual enlarged text, so it is **not a 200% text-resize pass**. This backend exposes no verified host browser zoom control, and visual pinch/page scaling would not establish layout reflow. Genuine browser zoom, native OS text scaling, and enlarged-text task completion remain untested. The attempted text emulation was reset to scale=1.

## Persistence and Status Copy

Runtime observations:

- Blank compose says **Not saved yet** in a status region and distinguishes **Send demo** from the ordinary send action. Its note explicitly says no email is delivered. Closing a blank compose did not create or edit a draft.
- A safe demo sync produced **Demo inbox is up to date** in a status notification. Mailbox counts remained explicit (All mail 12; Unread 4; 1–12 of 12).
- Account setup explains the bring-your-own app-registration requirement and provides task-specific help links. AI settings explains exact model IDs, endpoint-origin/key rules, the newest-20-message boundary, and review of proposed inbox changes. No help URL, provider sign-in, or AI request was opened/sent.

Source-only persistence verification:

- [Compose.tsx:45](/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/apps/desktop/src/Compose.tsx:45) records a device backup immediately and enqueues a daemon save after 400ms. Copy distinguishes device backup, saving to Alice, saved to Alice, and a save needing attention. Storage failure provides an explicit keep-window-open instruction; save errors preserve recovery guidance.
- [drafts.ts:5](/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/apps/desktop/src/drafts.ts:5) serializes the draft; [App.tsx:128](/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/apps/desktop/src/App.tsx:128) attempts latest-backup recovery against the daemon on startup.
- This audit did not create a draft or simulate daemon failure, refresh, or crash. Persistence code and wording were reviewed; end-to-end recovery is not claimed. Provider connections, AI settings writes, external mail sends, key entry, draft saves/discards, mail read/star/folder mutations, and native OS settings were avoided.

## Performance, Tokens, and Other Limits

- Current source requests state and a bounded 50-message page together, guards stale responses, and debounces search by 180ms. Polling still occurs every two seconds and refreshes React state arrays; no visible delay was observed with the demo, but no large-mailbox benchmark was performed.
- Existing production artifacts on disk: JS **273,804 bytes** (**85,281 gzip**), CSS **33,418 bytes** (**7,482 gzip**), five WOFF2 subsets totaling **161,420 bytes**. No bundle rebuild or automated performance test was run for this source-only audit. Dev Vite behavior is not a production timing benchmark.
- Motion is scoped to short opacity/transform entry effects and small loading indicators. No unbounded `will-change`, layout-property animation, or visible layout-thrash issue was identified. The modal backdrop blur is limited to an open modal.
- Twelve root color tokens exist; 121 literal hexadecimal occurrences remain, largely decoration, surfaces, borders, and backdrop treatments. The light palette is coherent, but it is not a complete switchable theme system. A dark theme was not required or exercised; absence of an unrequested dark theme is not counted as a concrete defect.
- Minimum native window is 850×640. Phone/touch targets and touch gestures were not treated as shipping requirements. This was Chromium browser rendering, not a native Tauri/WebKit accessibility or VoiceOver test. Native macOS control was not probed.
- Console warning/error query returned **0** entries. A 1Password extension status region appeared during form focus; it was excluded from Alice semantics/status scoring.
- The current source fingerprint is retained in `b-source-fingerprint.json`. No implementation files were edited by this assessment.

## Positive Findings to Preserve

1. Text, placeholders, enabled icons, names, and state semantics now have consistent measurable foundations. Product-specific content, category labels, demo semantics, and restrained paper/violet character remain coherent.
2. Assistant reflow uses actual grid space at laptop widths and a single content region at smaller shipped widths. Reader controls are not obscured, and inner scrolling brings keyboard-focused actions into view.
3. Native modal dialogs have names, field labels, Escape behavior, and observed focus return. The fixed sidebar footer and scrollable middle section keep Settings reachable at the minimum height.
4. Draft-status wording separates device backup from daemon persistence, and demo-send wording clearly distinguishes simulation from email delivery. Final sync feedback has a status role.

## Recommended Actions

1. **[P1] `$impeccable harden`:** strengthen settings, compose, and search focus indicators to at least 3:1 on their actual surfaces.
2. **[P2] `$impeccable harden`:** add expanded state and a programmatic relation to More message actions.
3. **[P2] `$impeccable clarify`:** correct the Gmail app-password directional reference using the exact control label.
4. **`$impeccable polish`:** final targeted check of those changed elements after the fix batch.

The parent owns combined-report synthesis, persistence/trend bookkeeping, and any targeted user questions. This file is independent technical evidence, not the combined critique.

## Run Notes and Evidence Archive

- Stable target: `apps/desktop/src/App.tsx`; parent owns the combined critique slug/snapshot. Ignore list `.impeccable/critique/ignore.md` is absent.
- Updated SKILL.md, audit.md, and critique.md guidance was read. No context rerun and no subagents spawned.
- Assessment A and prior synthesis were not read. Findings remain withheld until the parent completes the independence gate.
- Detector executed once, full JSON retained at `docs/reviews/2026-10-11/fixes/evidence/b-detector.json`. It was not rerun.
- Browser tab 1076629024 was our own fresh tab. Identity and localhost URL were checked before closure. Media and viewport overrides were reset; text scale was reset to 1. Existing tabs and the development server remain open. `b-cleanup.json` records cleanup.
- Overlay unavailable because evaluation is read-only. No mutable injection attempt or live-server workaround was made. No extra review server required cleanup. Actual screenshots, accessibility snapshots, and computed-style measurements are the fallback evidence.
- Evidence directory: `docs/reviews/2026-10-11/fixes/evidence/`. `b-summary.json` inventories 23 screenshots and measurement files; individual JSON/TXT files accompany the named views. Screenshots use JPEG as required by the browser backend.
- Retained artifacts: this report at `/tmp/alice-postfix-evidence-b.md` and its copy in the evidence directory, plus all screenshots/measurements. No source edits; no unrelated final repository status or verification round was performed.
