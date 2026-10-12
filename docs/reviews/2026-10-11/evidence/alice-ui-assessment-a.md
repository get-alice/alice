# Alice — independent design and UX assessment A

Method: independent Assessment A. No detector was run and Assessment B was not read. Target: `apps/desktop/src/App.tsx`, `Compose.tsx`, `Settings.tsx`, `Assistant.tsx`, and `styles.css` in `/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice`.

Guidance revalidated against **Impeccable v4.5.2** on 2026-10-11 by reading the updated `SKILL.md` and `reference/critique.md`. Surface mode is **Operate** for the mail workspace, Compose, Settings, and assistant; the reader also carries Read requirements. Scanability, native expectations, and the real desktop usage scene take priority, with brand expressed through precise details. The updated guidance reinforces the existing recommendations and does not materially change the verdict, 20/40 heuristic score, severity assignments, or priority order. Existing inspection evidence was reused; context, detector, and browser passes were not rerun.

Reviewed the current desktop foundation at 1280 × 720 and the supported minimum 850 × 640. This assessment does not treat phone support, bulk operations, attachment downloads, full historical sync, or an offline queue as promises of v0.1. The supplied product brief describes these current limits. No PRODUCT or DESIGN files were available; the implementation and brief are the design context.

## Design specificity verdict

Alice feels authored for personal email. Its paper-like reading surface, Newsreader subjects, compact DM Sans controls, soft violet mark, and unusually warm writing language create a coherent place for correspondence. The conventional folder/list/reader structure earns its place: it gives the unfamiliar brand a familiar operating model. This is more product-specific than a generic administration dashboard.

Its personality is stronger than its operational clarity. A carefully curated studio inbox makes the calm tone feel convincing, but connection setup, scope, counting, and small controls introduce uncertainty as soon as someone tries to use it for their own mail. The next improvement should make that calm reliable through readable controls, explicit prerequisites, accurate counts, and a reader that remains usable with Alice open. Adding more decorative character would have lower value.

This judgment was made independently before any detector findings entered this assessment.

## Nielsen design health

Scores are 0–4. All ten heuristics apply to this desktop application.

| # | Heuristic | Score | Key evidence |
|---|---|---:|---|
| 1 | Visibility of system status | 2 | Active views are visually clear, but Work shows five rows with an All mail count and footer of twelve. Global “All changes saved” depends on request connectivity, not a comprehensive operation state. |
| 2 | Match with the real world | 2 | Mail folders and compose fields are familiar. First-run setup exposes OAuth registration, a callback URL, and daemon environment terminology without a task guide. |
| 3 | User control and freedom | 2 | Native Compose and Settings dismiss with Escape. Assistant Escape works from a button but fails from its textarea; hidden reader controls remain focusable under the panel. |
| 4 | Consistency and standards | 2 | Visual controls form a coherent family. Selection is expressed through CSS classes rather than accessible states; the shortcut UI always shows macOS symbols even though Control is implemented. |
| 5 | Error prevention | 2 | Native form validation, disabled actions, close-to-save draft behavior, and explicit AI approval are useful safeguards. Recipient feedback is deferred, and the source has no periodic draft persistence or draft recovery from app termination. |
| 6 | Recognition rather than recall | 2 | Labeled mail navigation helps. Users must obtain and carry OAuth configuration from another service, and selected scope/filter state is unavailable programmatically. |
| 7 | Flexibility and efficiency | 2 | `/` search was verified; j/k, reply, archive, and Control/Command accelerators exist in source. Opening Alice leaves focus at its trigger and requires traversing the mail UI to reach assistant controls. Missing bulk actions are a documented foundation limit. |
| 8 | Aesthetic and minimalist design | 3 | A clear reading hierarchy and restrained surfaces work well. Very small secondary text and repeated brand reassurance consume space that matters at 850 × 640. |
| 9 | Error recognition and recovery | 2 | Search gives a useful empty-state suggestion and clear action to reset it. Operation errors generally expose the supplied API message, and technical setup lacks contextual recovery instructions. |
| 10 | Help and documentation | 1 | Shortcut help exists. Account registration and BYOK model selection have no linked task guide in the relevant UI. Repository documentation is not discoverable from these screens. |
| **Total** | | **20/40** | **Acceptable — significant UX improvements needed** |

The score reflects the current foundation, not a demand that v0.1 implement the future mobile or power-user roadmap.

## What works

1. **The reader has a distinct hierarchy.** The large serif subject, separated sender block, plain-text paragraphs, and restrained controls make reading feel like the main task. The Inbox list keeps sender, subject, preview, time, and category in a repeatable order. `App.tsx:146–169`; `styles.css:66–104`.
2. **The AI contract is unusually concrete.** Settings says one requested email or up to twenty inbox messages are shared. A generated reply enters Compose through “Use this draft”; proposed inbox changes require an explicit Apply action. These are important foundations for BYOK trust. `Settings.tsx:58`; `Assistant.tsx:34–42`; `App.tsx:171–172`. Result execution was reviewed in source, not invoked.
3. **Basic accessible controls and exits are already present.** Inputs have labels, toolbars use native buttons with accessible names, a visible focus outline exists, and native dialogs provide containment. Blank Compose focuses To and dismissed with Escape; Settings also dismissed with Escape. `/` focused Search mail, and a nonmatching query produced “Try another sender, subject, or phrase.” `Compose.tsx:12,30–38`; `Settings.tsx:47,52–58`; `styles.css:13–14`.

## Priority issues

### 1. [P1] Essential controls and instructions are too faint at desktop density

**Concrete case:** Open Settings → Alice & AI. Its section heading and entered input text are below 4.5:1, at 14px and 11px respectively. The screen is aesthetically consistent, but the configuration someone needs to read is physically difficult. The latest secondary-text overrides improve the original palette without getting several values over the line.

Read-only DOM inspection confirmed the current computed colors, backgrounds, and font sizes. Standard sRGB relative-luminance calculation produced:

| Element | Foreground / background | Size | Contrast |
|---|---|---:|---:|
| Settings section heading | `#8e789e` / white | 14px | 3.94:1 |
| Entered settings input text | `#8c789b` / `#fefcfe` | 11px | 3.90:1 |
| Selected settings tab | `#8f72a7` / white | 11px | 4.09:1 |
| Privacy note / field note | `#877090` / white | 10px | 4.42:1 |
| Main secondary text | `#7e7486` / white | commonly 8–10px | 4.44:1 |
| Global placeholder | `#a2a0ab` / `#fefcfe` | 11px in settings | 2.52:1 |
| Enabled standard icon button | `#9c97a5` / white | icon | 2.84:1 |

The ordinary small-text target is 4.5:1; enabled UI graphics need adequate non-text contrast. Disabled control contrast was not counted as a defect. Font size has no single universal WCAG minimum, but 8–11px secondary controls compound the measured contrast problem.

**Source:** `styles.css:14–20,61,85–92,110–113,127–132`. Settings headings and inputs are concentrated on line 111; global input placeholder is line 16.

**Fix:** Darken a small set of semantic text/control colors, verify them on their actual tinted surfaces, and make routine control text roughly 12–14px. Preserve the current type families and restrained palette. Important text should be readable before increasing ornamentation. Suggested command: `$impeccable typeset`, then `$impeccable harden`.

### 2. [P1] Alice covers the email and can completely obscure keyboard focus

**Concrete case at 1280 × 720:** Open the assistant while Sophie’s email is selected. The reader remains 696px wide at x=584; the 340px assistant begins at x=940. It covers the entire rightmost 340px of the reader without causing the subject or paragraphs to wrap around it. The subject and body visibly continue under the panel. At 850 × 640 the same overlay occupies 340px of the 660px reader, leaving only 320px of its unreflowed content visible.

**Keyboard reproduction:** Opening the panel kept focus on “Toggle Alice assistant.” Pressing Tab 24 times through the supported page reached “Next message” at x=1223, y=79.5, with a 30 × 30px rectangle. `document.elementFromPoint` at its center returned content inside `.assistant-panel`. The focused reader button was fully covered. No messages were activated during this test. This is an observable focus-obscuration problem, not a hypothetical screen-reader claim.

Escape from that reader button closed the assistant. Escape from the assistant’s empty “Ask Alice” textarea left the assistant open: the input/textarea guard precedes the Escape handling.

**Source:** `styles.css:112,118–120` (fixed panel; reflow reserved for widths ≥1500); `Assistant.tsx:31–42` (no opening/closing focus management); `App.tsx:100–114` (input guard at 105, Escape at 112).

**Impact:** Someone asking for a reply cannot fully read the source email alongside the proposed result. Sam can tab to invisible controls and lose the position of focus. The assistant’s core review workflow depends on context remaining usable.

**Fix:** Reflow the reader when Alice opens at supported widths, or use a deliberate focused assistant view with a clear route back at the minimum size. Move focus into an appropriate assistant control on opening and return it to the trigger on closing. Handle Escape before the editing-element guard. If a narrow overlay is made modal, make the covered interface inert and use proper dialog behavior. Suggested command: `$impeccable adapt`, then `$impeccable harden`.

### 3. [P1] Friendly onboarding hides a developer setup cliff

**Concrete walkthrough:** “Add account” → “Add a mail account” opens a Gmail-default form with Account name, Email address, an app-password checkbox, OAuth client ID, and optional client secret. The instruction to create a Desktop OAuth client in Google Cloud appears after these fields and is below the initially visible dialog area at both reviewed sizes. Outlook similarly requires a Microsoft public-client registration with delegated IMAP and SMTP permissions. There is no setup link or step sequence.

The UI leads with “Gmail, Outlook, and any account with IMAP and SMTP” and “One more inbox. A little less switching.” That establishes an ordinary mail-client expectation before revealing the developer prerequisite. BYO registration is an intentional v0.1 limit, so the review is not demanding a hosted OAuth service. The limitation needs to become an understandable entry path.

**Source:** `Settings.tsx:50–56`, particularly the fields and inline registration note on line 55. `styles.css:109,111` constrains the long form to a scrolling dialog.

**Impact:** Jordan can choose a supported provider and still have no feasible next action. The first confidence-building moment becomes an external cloud-registration task with secrets, callback configuration, and jargon. Technical early adopters also need an exact task guide rather than a remembered collection of provider concepts.

**Fix:** State the registration prerequisite before the form. Link a concrete provider-specific setup guide; make the Gmail app-password alternative explain its prerequisites and steps before requesting a password. If a maintainer-provided registration is configured, show that simpler route distinctly. Separate the connection task from optional account naming. Suggested command: `$impeccable onboard` and `$impeccable clarify`.

### 4. [P1] Collection results and pagination describe different sets of mail

**Verified demo case:** Click Work in “Your collections.” Five `.email-row` buttons exist, but the All mail tab says 12 and the footer says `1–12 of 12`. “Unread 4” also comes from the snapshot-wide count. This is a mismatch in the current supported UI, not a future scale speculation.

**Source root:** Category filtering occurs in memory on the current page (`App.tsx:47,61`). Requests ask for fifty messages with an offset and do not include category (`App.tsx:52–58`). The category control resets the page (`App.tsx:130`), while total/pagination remain based on the unfiltered server response (`App.tsx:150,157`).

**Inferred consequence:** When the inbox has more than fifty local messages, the same structure can display an empty collection on one page even though matching messages exist on another. That larger dataset was not injected or tested. “Your collections” also implies owned functionality although the brief identifies sample collections as a v0.1 limit.

**Impact:** A mail client needs trustworthy counts and trustworthy absence. Users cannot tell whether five visible messages represent their whole Work view, a partial page, or a missing group.

**Fix:** Make the filter, count, empty state, and pagination describe one set. If collections remain demonstration-only, label them as sample collections and avoid presenting them as a complete personal index. If they are supported for actual mail, filter before pagination and return its matching count. Suggested command: `$impeccable harden`, followed by `$impeccable clarify` for the scope label.

### 5. [P2] Current location and scope are visual-only; dialogs have no accessible name

**Verified DOM cases:** Work and All mail had `.active` styling but no `aria-current`, `aria-pressed`, or `aria-selected`. Assistant’s This conversation / Your inbox buttons likewise expose ordinary button names without the selected scope. Native Compose and Settings dialogs have neither `aria-label` nor `aria-labelledby`; the AX view reports an unnamed container with a heading inside it. Native modal containment works, but the dialog itself has no name.

Message list rows convey unread through a dot and font weight and selection through a background and border, without an equivalent accessible status in the button. This makes the visible interface more informative than the semantic one.

**Source:** `App.tsx:126–138,150,153–155`; `Assistant.tsx:33`; `Compose.tsx:30–32`; `Settings.tsx:47–48`.

**Impact:** Sam must infer which view and AI scope are current. In an email client, scope affects both message actions and what goes to an AI provider, so it should be available to assistive technology as well as sighted users.

**Fix:** Use `aria-current` for the current mail location, `aria-pressed` for genuine toggle/filter buttons, proper selected state for tab-like controls, concise row status labels, and `aria-labelledby` pointing to real dialog headings. Keep the existing native elements. Suggested command: `$impeccable harden`.

## Cognitive load

**Passes:** Mail navigation is labeled and grouped; the three familiar panes define a clear reading order; provider choices are limited to three; search can be cleared; there is one primary compose action.

**Jargon barrier / context switch / memory bridge:** Account onboarding requires users to interpret OAuth client, client secret, Desktop/public-client app, delegated IMAP/SMTP permissions, callback URL, and daemon environment. They must leave Alice, create a registration, obtain values, and return, without a guide or completion checklist.

**Decision points over four choices:** Six folder destinations are visible, though grouped in a conventional mail navigation. The reader has five message actions plus separate previous/next controls; More duplicates mark-read/unread. The full assistant flow asks the user to combine two scope options, three canned tasks, and a freeform prompt with connection setup. These are not all visible at once at minimum height; instead they compete through scrolling. Grouping mitigates the folder count, while assistant identity and marketing copy push practical actions below the fold.

**Visual noise / multitask demand:** The reader stays fixed underneath the assistant, requiring reading and prompting in a partly hidden context. Three separate entry affordances promote Alice (sidebar, list invitation, header), plus a reply-specific entry. The calm design can become calmer by making the first task more precise rather than repeating reassurance.

## Emotional journey

The **arrival and reading peak** are strong: Alice feels light, personal, and welcoming. A safe demonstration lowers the cost of exploration. The first valley is connecting real mail: cheerful copy abruptly gives way to registration prerequisites. The second valley is opening AI: context becomes physically covered, and at minimum height the connection action is below the initially visible part of the scroll area.

The planned **high-stakes moments** have a sound contract: demo sends say no mail will be delivered; AI changes require approval; drafts can be reviewed in Compose; a failed save preserves the current React draft. These were inspected in source, not exercised through sends or writes. The source lacks periodic draft persistence and uses “Saving…” for the entire send operation (`Compose.tsx:20–29,38`). Confidence at the end would improve with explicit Saving/Sending/Saved/Failed states and a clear statement of when a draft has become durable.

## Persona walkthroughs

**Alex — power user:** Starts in Inbox, uses `/` to focus search successfully, and gets a clear empty result that can be reset. Shortcuts in source support common mail actions. Clicking Work then displays five messages with twelve as the result count. Opening Alice retains focus at its trigger and requires 24 tabs to reach a reader control that is hidden behind the panel. Bulk actions are a documented v0.1 limit, not a surprise feature request; the limitation should remain explicit in release expectations.

**Sam — keyboard and low-vision user:** Blank Compose focuses To, visibly outlines it, and closes with Escape. Settings also closes with Escape. Setup instructions and input text nevertheless have measured low contrast at small sizes. Native dialogs lack accessible names, selected filters/scope are not exposed, and a reader Next message button can receive completely obscured focus. Escape from Ask Alice’s textarea does not dismiss the assistant. VoiceOver/NVDA was not run; these are keyboard, DOM, AX, and visual findings.

**Jordan — first-time user:** Understands Compose and Inbox immediately. “Add a mail account” looks simple until it requires an OAuth registration. The explanatory note and primary connect action are below the first view of the long form; no step-by-step task guide is linked. AI model selection asks for an exact Model ID with no help link. Attachments look like actionable file buttons, but source says clicking them only reveals that downloads are outside the preview. Disclosing that limit alongside the file would prevent a second surprise.

## Smaller observations

- **[P2] Compose minimum-height action visibility:** At 850 × 640, the bottom action row is partly clipped by the 88dvh dialog and the demo note is offscreen. Scrolling is available, so this is not a task blocker. A smaller minimum body height or persistent footer would keep editing and the send/save decision connected. `styles.css:109–110`.
- **[P2] Foundation limits at the action point:** Attachment buttons reveal the metadata-only limitation after activation (`App.tsx:167`). “Your collections” does not identify its sample nature (`App.tsx:129–130`). These limitations can be described proactively without implementing the missing features.
- **[P2] Draft durability needs an explicit contract:** Source keeps editing data in local component state, saves on Close/Escape, and deletes persisted drafts directly on Discard. There is no periodic persistence or source-level recovery/undo path. No populated draft was created or discarded during review. `Compose.tsx:7–29,38`.
- **[P2] AI scope wording:** “This conversation” sends only the selected message ID, and “Your inbox” is the newest twenty messages. There is no current account, folder, collection, or query in the request. Say “This email” and identify the accounts included in inbox scope. `Assistant.tsx:13,20,33–34`; the server selection itself was not audited.
- **[P3] Platform shortcut copy:** Control works in source, but Compose, Search, and Help show `⌘` universally. Derive the display label from the platform for Windows/Linux. `App.tsx:103–104,125,143,148`.
- **[P3] Duplicate provider names:** Provider buttons are announced “Gmail Gmail” and “Outlook Outlook” because the icon wrapper has a label and the button repeats the name. Make the redundant icon decorative in an already labeled control. `App.tsx:178–179`; `Settings.tsx:51`.
- **[P3] Global status wording:** “All changes saved” is gated by the absence of `connectionError`, while provider sync errors sit on individual account dots. A more specific connection/sync status would set clearer expectations. `App.tsx:41,60,134,143`.
- The source is light on motion and images. Loading has text as well as a spinner. No performance profile or bundle analysis was run, so this assessment does not infer a performance regression from component size or stylistic choices.
- Most colors are literal per-component values despite a few root tokens. Consolidating the contrast corrections into semantic tokens would keep the existing light appearance consistent. Dark mode was not a promised reviewed state.

## Targeted product questions

1. **Who should be able to connect mail successfully in v0.1?** Choices: **everyday mail users**, with a guided connection route and prerequisites explained before the form; or **technical early adopters**, with explicitly self-managed OAuth registration and linked exact setup steps.
2. **What should opening Alice do at the minimum desktop size?** Choices: **keep the email and assistant readable together through reflow**; or **open a focused assistant workspace with a clear Back to email route**.

## Evidence and limitations for parent synthesis

Two bounded browser batches were completed. Batch one inspected Inbox/reader, Accounts overview, Gmail-default onboarding, blank Compose, unconfigured assistant, and AI settings at 1280 × 720 and/or 850 × 640, including both sizes for onboarding, Compose, and AI settings. Batch two verified collection counts, computed colors, native dialog names, selected-state semantics, assistant overlap, obscured keyboard focus, Escape behavior, and keyboard search.

The requested `iab` identifier initially failed with “Browser is not available: iab.” One inventory exposed Brave Work as browser 2. A fresh isolated tab 1076629000 was created there; the existing personal Gmail tab was not used. Native macOS was locked and was not probed. The temporary viewport override was reset and the review tab was closed. No server was started or stopped. The existing local server was left running. The UI screenshots were rendered in tool outputs; no standalone screenshot files were exported.

No account or AI configuration was saved, no provider call was made, and no email/draft was edited, sent, archived, starred, marked read, or discarded. Search and view state were reset before cleanup. No detector, Assessment B output, automated performance measurement, real provider authentication, VoiceOver/NVDA, or populated-draft persistence test was run. Source-only risks are marked separately above. No source files were changed.
