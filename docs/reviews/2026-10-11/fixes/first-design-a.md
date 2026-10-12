# Alice post-fix Assessment A

Method: independent design and UX assessment by `/root/ui_design_review`, using Impeccable v4.5.2 critique and craft-floor guidance. No Assessment B, detector results, old synthesis report, or parent findings were read. This is Assessment A for the parent's dual-agent synthesis, not a standalone combined critique.

Target: `/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/apps/desktop/src/App.tsx`, with `Compose.tsx`, `Settings.tsx`, `Assistant.tsx`, `styles.css`, and draft-recovery source. Live target: `http://127.0.0.1:1420/`. Review mode: **Operate**. Alice's light paper, graphite, and violet identity was treated as a constraint. Desktop at 850×640 or larger is the shipped surface; future phone support was not graded as a current promise.

## Design specificity and overall judgment

Alice feels authored for reading and answering mail. The restrained folder rail and sans-serif message list give scanning a clear structure; the larger serif message subject and generous reading space make the selected conversation feel personal. Paper surfaces and violet emphasis remain coherent across mail, compose, Settings, and the assistant. Conventional folders and reader toolbars fit the task rather than weakening the identity.

The composition also supports the product's particular trust model: sample accounts are identified, demo sends are labelled, AI setup says what mail is shared, and proposed replies and actions require a subsequent user action in the source. The latter result states were inspected in source, not exercised through an AI provider.

This is a good v0.1 desktop foundation. In the rendered cases tested, the assistant uses available layout space, dialogs have names and useful initial focus, drafts show persistence status, and scoped counts match the actual rows. The clearest remaining opportunity is the small-window compose experience. No P0 or P1 defect was established in this pass.

## Nielsen heuristics

All ten apply to this operational email client. Scores are design judgments informed by the observed demo flows and source; they are not a certification of unexercised provider/error paths.

| Heuristic | Score /4 | Evidence and practical limit |
|---|---:|---|
| 1. Visibility of system status | 3 | Draft reports device/Alice persistence; global connection and busy states are named; filter counts and footer match the rendered list. Long provider operations were not exercised. |
| 2. Match with the real world | 3 | Familiar Inbox, Drafts, Archive, sender details, and Reply; OAuth terms now have setup steps and official help links. Registering a provider app still requires technical knowledge. |
| 3. User control and freedom | 3 | Back to list, clear search, assistant close/Escape, Settings Escape, and blank-compose Escape work. Review draft can be discarded. No general undo for mail changes was established. |
| 4. Consistency and standards | 3 | The visual system and active states are consistent; native HTML dialogs protect the modal task. Small-window compose actions and the body focus treatment are weaker than other controls. |
| 5. Error prevention | 3 | Live immediate-reload recovery succeeded; empty-recipient Send is disabled; modal and interactive-target keyboard guards are present. No real-send or provider validation trial was performed. |
| 6. Recognition rather than recall | 3 | Visible folders, sender/subject/read state, scoped counts, contextual AI scope, setup guides, and shortcut hints. Compose's initially hidden footer at 850×640 requires finding the scroll path. |
| 7. Flexibility and efficiency | 2 | Search/compose accelerators and focused list/reader navigation are provided. The desktop v0.1 feature set has limited bulk/customization options; this is a documented scope limit, not a new defect. |
| 8. Aesthetic and minimalist design | 3 | Strong hierarchy and calm reading composition. Repeated assistant invitation/branding uses some space, but does not block the tested tasks. |
| 9. Error recognition and recovery | 3 | Immediate draft reload restores content; source preserves drafts on save failure and describes the device backup; startup has retry and errors have alert roles. Simulated disconnection/provider failure was outside this pass. |
| 10. Help and documentation | 3 | Gmail/Outlook registrations have explicit steps; AI model and endpoint guidance is contextual. A small Gmail directional-copy error remains. |
| **Total** | **29/40** | **Good: solid foundation; address the remaining compose weaknesses.** |

## What's working

1. **Reading and scanning have distinct, useful rhythms.** At 1440×900 and 1366×768 the list and reader have stable boundaries; at 1024×768 and 850×640 a clear Back control supports switching between list and reader. The identity remains recognizable in both compositions. Sources: `App.tsx:168`, `App.tsx:182`, `styles.css:120`, `styles.css:121`.
2. **The assistant has a usable entrance and exit.** At 1366×768 it occupies a separate column, leaving the reader unobscured. At 1024×768 and 850×640 it takes the workspace pane. In the unconfigured state opening focused Connect AI provider; Escape from Ask Alice dismissed it and returned focus to Toggle Alice assistant. Sources: `Assistant.tsx:16-20`, `Assistant.tsx:37`, `styles.css:141-142`.
3. **Draft confidence is backed by observed behavior.** The reply opened with the recipient/subject populated and body focused. A uniquely named demo draft showed Saved to Alice; its last edit survived an immediate reload with recipient, subject, and body intact. The restored body was focused. Sources: `Compose.tsx:22-28`, `Compose.tsx:45-51`, `App.tsx:128-137`.

## Priority issues: two concrete remaining defects

### [P2] Compose hides its action footer on the supported minimum window

At **850×640**, the reply dialog opened at scrollTop 0 with a 561 px client height and 642 px scroll height. Its bottom was at y≈601.6. Send demo began at y≈600.4 and Save draft/Discard draft were below the clipped dialog content; the screenshot shows the save-status line but no footer actions. The demo explanation was also below the initial viewport.

The actions are reachable: pressing Tab from the body focused Send demo and scrolled the dialog by 81 px. This is an annoyance with a workaround, not a completion blocker. A first-time user who has finished typing must discover that the entire dialog scrolls before finding Send, Save, or Discard. Manually resizing the body can worsen it.

**Fix:** use a bounded flex layout with a scrolling editor and a visible action footer, or reduce editor height at short desktop windows. Keep the save state and demo-delivery explanation next to the actions. Suggested command: `$impeccable adapt`.

**Source:** `styles.css:110` sets dialog max-height to 88dvh; `styles.css:111` gives the body min-height 310 px and leaves the footer in normal flow; `Compose.tsx:80-85` places editor, save state, footer, and demo note sequentially. The only lower editor minimum is below 760 px width (`styles.css:122`), which does not cover the shipped 850 px window.

**Evidence:** `a-850-controlled-draft.jpg`, `a-850-compose-actions-after-tab.jpg`, `a-850-compose-keyboard-focus.jpg`.

### [P2] Compose body overrides the stronger keyboard focus color

On a blank compose dialog, keyboard traversal To → Subject → Message body produced `:focus-visible` on the textarea. Its computed outline was **2 px solid rgb(222,210,237), inset by 3 px**, against white. The color pair is approximately **1.44:1**. The large white writing area has a very faint boundary even though this is the primary working field. Other keyboard controls retain the darker palette focus ring.

This is a concrete contrast weakness for keyboard and low-vision use. It is not a claim that VoiceOver or a complete accessibility conformance audit was performed.

**Fix:** keep the inset offset but inherit the global darker outline color, or use another violet token with a clearly perceptible contrast against white. Suggested command: `$impeccable harden`.

**Source:** `styles.css:111` overrides `.compose-body:focus-visible` with `outline-color: #ded2ed`; the general ring is set at `styles.css:132`. The editable control is `Compose.tsx:80`.

**Evidence:** `a-compose-focus.json`, `a-850-compose-keyboard-focus.jpg`.

## Cognitive load

Checklist: single focus passes; grouping passes; hierarchy passes; one thing at a time passes for routine reading/replying; working memory passes because sender, subject, account, and AI context remain visible; progressive disclosure passes because setup complexity appears inside the requested account workflow. The strict ≤4 checklist has **two failures**: chunking and minimal choices. The folder group has six siblings; the reader toolbar exposes five message actions before pagination. That gives **moderate load** by the reference's counting rule, although the standard mail vocabulary and visual grouping make these options easier to recognize than six novel actions. These counts alone do not establish a priority defect.

Intrinsic load is highest in own-app OAuth registration and BYOK model selection. The rendered instructions make those requirements explicit before Connect, rather than forcing the user to infer them from failure. Remaining avoidable load is concentrated in discovering the compose footer at the minimum height. No redesign of the six standard mail folders is proposed just to satisfy the checklist numerically.

## Emotional journey

The initial inbox feels calm and personal; opening a conversation preserves that tone. Reply's populated fields and save-state text provide reassurance during the highest-stakes task. Recovering the immediately reloaded draft is the strongest confidence moment in this pass, and successful discard gives a clear end-state notification.

Account setup is the emotional valley: a user seeking less switching encounters Google Cloud or Microsoft Entra app registration. That remains a v0.1 capability constraint, but the guide now names the work and points to help before the user attempts connection. At 850×640 the hidden compose footer creates a smaller avoidable valley after writing: completion controls appear to be missing until the user scrolls or Tabs.

## Persona observations

- **Alex, power user:** the explicit shortcut-target guard prevents a focused Archive button's `e` key from archiving the current message; source also suppresses mail commands inside dialogs and while the assistant is open (`App.tsx:109-116`). Alex's remaining red flag is the low-contrast body focus ring during keyboard traversal. No claim is made about every shortcut on every OS.
- **Sam, returning frequent user:** consistent account/sender context and scoped counts reduce uncertainty. Work showed All mail 5 / Unread 3 / five rows; Unread showed three rows and 1–3 of 3. The observed draft recovery supports interruption and return. Sam's remaining red flag is action visibility at the minimum-height window.
- **Jordan, first-timer:** sample accounts, demo send labels, and setup guidance explain the current product boundary. Jordan can find the normal Reply route without using AI. The technical provider registration remains a hurdle, but is disclosed rather than treated as an unexplained input. The minimum-window compose footer and Gmail's incorrect direction word remain the concrete friction points.

## Minor observations

- **[P3] Gmail guide points in the wrong direction.** The guide says “the app password option above may be quicker”, but the checkbox is below the guide and Account name/Email address fields. This was visible in the Gmail screenshot. Change “above” to “below”, or name the option without a direction. Sources: `Settings.tsx:56`, `Settings.tsx:58-59`; evidence `a-1366-gmail-guidance.jpg`.
- Secondary metadata is compact (for example category tags at 7 px in `styles.css:92`). This is a reading-comfort observation, not an asserted accessibility violation or a separate priority issue.
- Text contrast spot checks support the darker semantic palette: ink/white ≈12.48:1, muted/white ≈5.95:1, accent-text/white ≈7.73:1, white/primary violet ≈4.71:1. These are selected palette pairs, not a full-state contrast audit.

## Walkthrough and evidence coverage

All evidence is under `/Users/amruth/Documents/Codex/2026-10-11-create-a-new-project-called-alice/docs/reviews/2026-10-11/fixes/evidence/`.

| Window | Observed views | Representative evidence |
|---|---|---|
| 1440×900 | Inbox, reader, controlled reply dialog | `a-1440-inbox.jpg`, `a-1440-controlled-draft.jpg` |
| 1366×768 | Reply, immediate reload, Work and unread filters, separate assistant column, Gmail/Outlook guidance, AI Settings | `a-1366-controlled-draft.jpg`, `a-1366-work-unread-counts.jpg`, `a-1366-assistant.jpg`, `a-1366-gmail-guidance.jpg`, `a-1366-ai-guidance.jpg`, `a-outlook-guidance.txt` |
| 1024×768 | Reader, workspace assistant, assistant Escape/focus return, message list accessibility state | `a-1024-reader.jpg`, `a-1024-assistant.jpg` |
| 850×640 | Reader/list, assistant, compose footer and keyboard focus, Settings keyboard traversal/Escape, draft cleanup | `a-850-inbox-list.jpg`, `a-850-assistant.jpg`, `a-850-controlled-draft.jpg`, `a-850-compose-actions-after-tab.jpg`, `a-850-settings-focus-trap.jpg`, `a-850-review-draft-cleanup.jpg` |

The filename `a-850-settings-focus-trap.jpg` records an attempted wraparound traversal, not a successful focus-loop certification. That attempt focused the dialog element; a later ordinary Tab step was observed entering AI provider, and Escape closed Settings and restored the Settings button. No trapping defect is asserted from the ambiguous wraparound attempt.

Controlled draft: subject **Alice assessment A — durability check**, recipient **alice-assessment-a@example.invalid**. Body was edited to an immediate-reload sentinel. No send was performed. After successful restoration, Discard draft was clicked; searching Drafts for the unique subject returned **0 messages**. Blank compose dialogs used for Escape/focus checks were closed without content or persistence. Final inbox returned to All mail 12 / Unread 4 / 1–12 of 12; no open dialogs or assistant, and no document horizontal overflow at 850×640 (`a-final-ui-state.json`).

Fresh CUA tab **1076629010** used browser 2 (Brave Work) as the available browser fallback; it was closed and viewport emulation reset. Existing user tabs and server were left alone. No source, credentials, provider connection, or unrelated mail mutation was performed. Native HTML dialogs were inspected in the browser; the locked native Mac surface and Tauri/OS dialogs were not tested. Real provider sign-in, AI result/apply runtime, external delivery, failure injection, VoiceOver, other OS browsers, and future mobile layouts were not exercised. They remain verification limits rather than invented findings.

Questions skipped: this Assessment A contains two Priority Issues, below Impeccable's three-issue question threshold; the parent will synthesize the independent assessments.
