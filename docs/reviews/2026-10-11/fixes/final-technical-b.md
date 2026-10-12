# Alice final technical confirmation — Assessment B

Implementation integrity passes: the desktop expresses a coherent mail product, with folder navigation, contextual reader actions, honest demo labels, named settings and compose dialogs, and an assistant that adapts to available space. The final detector found no deterministic implementation findings. One browser-verified accessibility defect remains in the assistant input's focus treatment.

| Dimension | Score | Evidence |
|---|---:|---|
| Accessibility | 3/4 | Named dialogs, labeled fields, selected/read/star state, keyboard reachability, and most measured contrast pass. One author-defined focus state remains below 3:1. |
| Performance | 3/4 | Small current bundle, bounded message pages and short transform/opacity motion; no visible stall or console warning/error during this confirmation. Large-mailbox performance was not benchmarked. |
| Theming | 2/4 | Twelve shared color tokens cover core text and focus colors, but 117 literal hex/RGB color occurrences remain in the stylesheet. The light palette is coherent. |
| Responsive Design | 3/4 | Requested desktop sizes fit, compose actions remain visible initially, and the assistant reserves or replaces the content region. Actual 200% text scaling was unavailable. |
| Implementation Integrity | 4/4 | Zero detector findings; corrected setup wording, consistent state semantics, and product-specific behavior. |
| **Total** | **15/20 — Good** | **P0: 0; P1: 1; P2: 0; P3: 0.** |

## Scope and independence

This is B's second and final bounded post-fix confirmation, following the first batched post-fix assessment. It inspected current source and the existing live renderer at `http://127.0.0.1:1420/`, using Impeccable v4.5.2 and detector engine 0.1.14. Current audit and critique guidance was read; context was not rerun. Assessment A and the prior combined synthesis were not read. Findings were withheld until the parent confirmed A had finished.

Browser interaction used the documented CUA APIs in fresh temporary Brave browser tabs. The available browser backend did not expose an in-app browser. Read-only DOM evaluation supplied geometry and computed styles; it did not inject scripts or change the DOM. An injected detector overlay was unavailable under this read-only API, so plain screenshots, DOM snapshots, source locations, and computed measurements are the evidence. No review server was started.

Requested desktop viewports were 1440×900, 1366×768, 1024×768, and the shipped native minimum 850×640. No phone-layout or touch-target defect is assigned to this desktop release. No provider connection, key entry, external message, mail-state mutation, or source edit was performed.

## Remaining concrete defect

### [P1] Assistant keyboard focus still uses a pale border with no outline

**Location:** `apps/desktop/src/styles.css:113`, the `.assistant-input textarea` and `.assistant-input:focus-within` rules; control at `apps/desktop/src/Assistant.tsx:48`. The shared focus rules at `styles.css:14` and `styles.css:132` do not survive the more specific textarea override.

**Reproduction:** At 1440×900, open the assistant, then press Tab from “What needs my attention?” to “Ask Alice”. This was exercised through the documented locator keyboard API without entering or sending a prompt.

**Measured result:** The textarea is the active element and matches `:focus-visible`, but its computed outline is `none`, width `0px`. Its form has no outline or shadow. The only authored focus indicator is a 1px border, computed `rgb(181,154,204)` / `#b59acc`: **2.483:1 against the white input surface**, and **2.414:1 against the surrounding `#fdfbff` panel**. The text and placeholder themselves pass contrast.

**Impact:** Keyboard users with low vision get a substantially weaker focus signal in the assistant than in search, compose, or settings. This falls short of the 3:1 non-text contrast requirement for an author-defined focus state. See [WCAG 2.2 non-text contrast](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html).

**Recommendation:** Apply the same 2px `--violet-dark` outline to `.assistant-input:focus-within` and use that token for its focused border, or remove the textarea outline override so the shared rule can apply. The existing search treatment supplies a consistent model. Suggested command: `$impeccable harden`; finish with `$impeccable polish`.

**Evidence:** `docs/reviews/2026-10-11/fixes/evidence/final-b-assistant-input-focus-1440x900.jpg`, `final-b-assistant-input-focus.txt`, and the `assistant-input-focus-1440x900` record in `final-b-measurements.json`.

## Focus, copy, and state corrections verified

The previously measured settings, search, and compose focus defects are fixed in their tested surfaces:

| Actual surface | Computed focus treatment | Contrast |
|---|---|---:|
| Settings API base URL, keyboard focus | 2px solid `#68518e`, 2px offset | 6.517:1 against `#fefcfe`; 6.654:1 against adjacent white |
| Compose body, keyboard focus | 2px solid `#68518e`, −3px inset offset | 6.654:1 against white |
| Search input's containing box, keyboard focus | Border plus 2px solid `#68518e`, 2px offset | 6.343:1 against `#faf9fc` |
| Connect account button, keyboard focus | 2px solid `#68518e`, 3px offset | 6.654:1 against adjacent white |

Outside button outlines must be compared with the adjacent surrounding surface. The measurement helper also records a contrast number against the button's own fill; that number is not the relevant contrast for a ring separated from the button by a white 3px gap. The Connect account ring therefore passes.

Source locations: shared field/button focus at `styles.css:13–14`, search at `styles.css:76`, compose body at `styles.css:111`. The light compose-body override is gone.

“More message actions” changes `aria-expanded` from `false` to `true` on opening and back to `false` on closing. Its `aria-controls` is `message-actions`, and the open popover has that exact ID. Source: `App.tsx:183`. Open/closed snapshots and `final-b-menu-open-1231x673-setup.jpg` preserve the actual browser setup viewport; these were not mislabeled as a requested-size layout test. Closed state was also captured in the requested 1440×900 main view.

The Gmail guidance now says: “For a personal account, select ‘Use a Gmail app password’ below for a quicker setup.” The checkbox has that exact accessible name and is below the guidance in the document. At 850×640 the copy occupied y559.3–579.1, with the checkbox label y695.6–715.6 before scrolling. Source: `Settings.tsx:56` and `Settings.tsx:59`. This is ordinary scrollable form content; Tab to Connect account scrolls the dialog to 441px, making the button visible at y531.6–568.6. No submission occurred.

Category chips are now computed 10px text, 15px tall. The Work chip retains 6.406:1 text contrast. Source: `styles.css:92`.

## Compose footer and responsive confirmation

The bounded flex form, scrollable body, nonshrinking actions/note, and bounded feedback are implemented at `styles.css:111`; feedback/status/actions are at `Compose.tsx:81–85`.

| Viewport | Initial compose body | Actions | Demo note | Outer form scrolling |
|---|---:|---|---|---|
| 1440×900 | 310px | y672–744 | y744–771 | client=scroll 642px; top 0 |
| 1366×768 | 310px | y606–678 | y678–705 | client=scroll 642px; top 0 |
| 1024×768 | 310px | y606–678 | y678–705 | client=scroll 642px; top 0 |
| 850×640 | 229.2px | y501.6–573.6 | y573.6–600.6 | client=scroll 561px; top 0 |

Send demo, Save draft, Discard draft, and the delivery note were fully inside the dialog and viewport on initial display at all four sizes; control center hit tests passed. Send demo was disabled in the untouched blank compose. The 850×640 body shrinks while preserving the footer rather than pushing it below the dialog.

The first temporary tab initially displayed an already present controlled recovery draft. It was inspected without changing its content: its body had 964px scroll height within a 310px textarea, with no horizontal overflow, while the actions remained visible. After the parent reported that its recovery check and cleanup were finished, that cached tab was closed without invoking compose Save, Close, or Discard. A fresh tab then displayed a clean inbox. B did not test or claim ownership of the recovery scenario.

| Viewport | Main content | Assistant layout |
|---|---|---|
| 1440×900 | Mail list and reader | Panel x1120, width320; reader ends x1120 |
| 1366×768 | Mail list and reader | Panel x1046, width320; reader ends x1046 |
| 1024×768 | Selected reader width834 | Assistant replaces content, x190, width834 |
| 850×640 | Selected reader or message list width660 | Assistant replaces content, x190, width660 |

Document scroll width equals viewport width at every requested size. Assistant opening samples can contain the transient translation from its 180ms entry animation; those samples were not reported as persistent overflow. Reduced-motion measurements verified its settled boundaries exactly at 1366, 1024, and 850, while the 1440 settled geometry was captured in the normal mode. Closing the assistant restored the main content. At 850×640, Settings remains visible at x151, y595.5, 24×30, bottom625.5; its center hit test passed. Back to message list restores a full-width list and search.

## Accessibility, motion, and status regression evidence

The settings dialog's accessible name is “Make yourself at home.” and blank compose is “A new conversation”; both use native dialogs with `aria-labelledby`. Source: `Settings.tsx:50`, `Compose.tsx:74–76`. Form fields have real labels or specific aria-labels. Closing settings with Escape returns focus to Settings; closing untouched compose returns focus to Compose.

Selected message rows expose `aria-pressed`; names include read/unread and starred state. The star action exposes its pressed state and names the action “Unstar message” for the current starred message. Source: `App.tsx:175`, `App.tsx:183`. Assistant scope and settings choices expose pressed state. These state observations did not execute archive, trash, mark-read, or star mutations.

Representative current computed text contrast: selected unread subject 6.688:1, read subject 5.855:1, Work badge 6.406:1, search placeholder 5.675:1, settings field text 7.575:1, settings placeholder 5.830:1, compose and assistant placeholder 5.952:1, white primary-button text 4.707:1. Enabled toolbar icons use the readable muted color; Settings is 5.563:1 against the sidebar. Disabled controls and provider logos were excluded from ordinary text/icon failure counts.

With `prefers-reduced-motion: reduce`, the compose animation changed from `0.16s ease-out modal-in` to `none`, button transitions changed from 0.15s to `none`, and assistant animation was `none`. “Not saved yet” and the assistant footnote remained present. Emulation was reset afterward. Source: `styles.css:123`; evidence: `final-b-motion.json`, `final-b-assistant-motion.json`.

Blank compose accurately showed “Not saved yet” in `role=status`, with disabled Send demo and “No email is delivered.” The already recovered controlled draft displayed “Saved to Alice”. These are observed status-copy states, not a new B persistence fault-injection or recovery test. Save/send/error paths and feedback caps were reviewed in source only; no error was forced. Extension-injected 1Password status text is recorded in raw snapshots but is outside Alice's UI responsibility.

## Detector and performance evidence

The final detector was invoked exactly once:

```text
/Users/amruth/.agents/skills/impeccable/scripts/impeccable detect --json apps/desktop/src
```

Exit code 0. Full stdout saved as `final-b-detector.json`: exactly `[]` plus newline, 3 bytes. Total findings 0; primary 0; advisory 0; rules with findings `[]`; file/line locations `[]`. No false positives or ignored hits require classification. The empty JSON result does not enumerate the number of files or enabled checks, so no invented scan coverage count is reported. The assistant focus defect comes from the browser measurement and cascade inspection, not the detector.

Current existing build assets: JavaScript 273,966 bytes / gzip 85,323; CSS 33,761 / gzip 7,513; five WOFF2 assets total 161,420 bytes. These are local build-artifact sizes, not freshly generated performance benchmarks. Current source hashes, asset sizes, twelve root tokens, and literal-color count are retained in `final-b-source-fingerprint.json`. No layout-property motion loop or unbounded animation was identified in the reviewed source. Fifty-message pages, request guards, and short animations provide bounded behavior, but larger datasets, frame timing, and native memory use remain unmeasured. Captured browser warning/error count: 0 (`final-b-console.json`).

## Limits and cleanup

This final confirmation did not successfully exercise browser 200% zoom or text-only enlargement. The earlier B attempt using documented OS text-scale emulation at scale 2 returned successfully but left root 16px, heading 30px, and field 11px unchanged; it was reset. That is not evidence of a 200% pass. The final pass did not repeat the ineffective technique or substitute pinch zoom. Native Tauri/VoiceOver, physical keyboard assistive technology, provider sign-in/delivery, real AI responses, forced long errors, and large-mailbox performance remain outside the observed evidence. Known v0.1 limitations such as sample collections and plaintext email do not become new defects merely because a shipping capability is absent.

The final verification ends here. No source fixes or further broad audit round were performed by B. The only recommended corrective action is the measured assistant focus treatment above.

Both B-created temporary tabs, 1076629046 and 1076629052, were closed. The final tab ID and localhost URL were checked before closing. Viewport override and reduced-motion emulation were reset; before close the browser returned to 1231×673/DPR2, reduced-motion false, root 16px, with no open dialog. Existing user tabs and the development server were left open. B entered no draft text, credentials, provider settings, or external message. See `final-b-cleanup.json`.

All evidence is under `docs/reviews/2026-10-11/fixes/evidence/` with `final-b-` names: 23 JPEG screenshots, 26 computed measurement captures, relevant DOM snapshots, motion and copy JSON, detector stdout, source fingerprints, console observations, and cleanup confirmation. The requested-size shots explicitly contain the actual requested viewport dimensions; the 1231×673 setup shots are separately named. This report is also archived there as `final-b-technical-report.md`.
