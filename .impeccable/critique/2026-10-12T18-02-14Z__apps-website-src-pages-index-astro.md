---
target: Alice landing page fixes and re-audit
total_score: 26
max_score: 32
na_heuristics: 7,10
p0_count: 0
p1_count: 0
target_identity: "file:/Users/amruth/Projects/alice/apps/website/src/pages/index.astro"
target_fingerprint: "sha256:5d645697396391499a210c9a2a17cd7d18e14b10a0582bbc39332f37724ea0bf"
target_path: /Users/amruth/Projects/alice/apps/website/src/pages/index.astro
timestamp: 2026-10-12T18-02-14Z
slug: apps-website-src-pages-index-astro
---
# Alice landing page — fixes and re-audit, 12 October 2026

Method: independent Impeccable design and technical assessments, followed by one small repair/confirmation batch. Target: apps/website/src/pages/index.astro and its shared components/scripts. Existing production preview: http://localhost:4400/. No deployment or installer publishing was performed.

## Outcome

All eleven original landing-page findings are addressed. The meadow, flower, dandelion, day/night atmosphere, real product screenshots and desktop comparison remain intact. The independent design assessment found no remaining design P0–P2 findings.

The website now distinguishes a real installer from an unavailable release. With today's unavailable installer metadata, every primary acquisition action says “Build from source” and goes to the existing GitHub Run instructions. No public installer was created or claimed.

## Audit Health Score

| Dimension | Before | After | Evidence |
|---|---:|---:|---|
| Accessibility | 2/4 | 4/4 | Site-copy contrast failures and prohibited CLI ARIA removed; keyboard and script-failure paths work |
| Performance | 3/4 | 3/4 | Healthy local timings; no dependency increase or broad motion rewrite |
| Theming | 3/4 | 4/4 | Shared text tokens work against the day/night sky and surfaces |
| Responsive design | 2/4 | 4/4 | Enlarged default text reflows; mobile gestures, section navigation and hit targets work |
| Implementation integrity | 2/4 | 4/4 | Real release availability controls labels/links; core content survives module failure |
| Total | **12/20** | **19/20 — Excellent** | Scoped audit, not WCAG certification |

Remaining verified findings after confirmation: **0 P0, 0 P1, 0 P2, 0 P3**.
Design heuristic assessment: **20/32 → 26/32**, Good. Heuristics 7 and 10 remain not applicable to the Persuade surface.

## Original finding clearance

| # | Finding | Resolution |
|---|---|---|
| 1 | Installer promise without a release | Missing assets are represented by empty platform lists. CTA labels use actual availability; unavailable builds offer source instructions. One explicit explanation replaces three identical fallback platform rows. Unknown versions are omitted from structured data. |
| 2 | Failed modules hide headings and hero CTA | Removed first-paint motion opt-in and CSS hiding of content. Inline platform/theme selection remains; content is visible before enhancement and after blocked external JS. |
| 3 | Inaccessible animated CLI commands | Removed character splitting. Commands stay intact in native code elements; ordinary section reveals keep the presentation consistent. |
| 4 | Secondary text contrast | Darkened day text and strengthened night secondary text through shared tokens. Text selection and footer focus use the page palette. |
| 5 | Enlarged text clips header action | Header/action group can wrap, buttons can wrap text, and mobile header simplifies to brand, section menu, theme and Get Alice. Desktop section navigation stays centered. |
| 6 | Mobile final CTA self-anchor | With installers available, “Choose a desktop version” targets the distinct focusable #desktop-downloads chooser. Without verified installers, it offers an external source-build action. |
| 7 | Overlong, unreadable mobile comparison | Mobile comparison is unpinned and compact, with 14px terminal text, a readable crop, a comparison hint and one command example. Desktop keeps the original sweep/comparison. |
| 8 | Missing mobile section navigation | Native details/summary menu exposes Product, Terminal, Privacy, FAQ and separately grouped source access. Keyboard, Escape, outside-click and link-selection dismissal work. |
| 9 | Infrastructure jargon leads | Small-download, shared-inbox and local-ownership benefits lead; Rust/Tauri/SQLite provide supporting detail. Terminal copy starts with real mail tasks. |
| 10 | Mobile hero delays product proof | Reduced mobile notification fragments to three, shortened the hero, removed mobile hero pinning and tightened the next section's spacing. Product proof starts around the first fold. |
| 11 | Small touch targets | Header and platform hit areas are at least 44px tall; footer navigation now has both minimum width and height of 44px. |

The technical first round found one additional P3 measurement edge: Terms was 43.48px wide. The final repair added a minimum footer-link width. Confirmation measured Terms at **44×44px**. A final geometry assertion also confirmed the restored desktop section navigation is centered, with **0px** center difference.

## Design and UX assessment

Specificity remains strong: the meadow and lavender flower express calmer email; day and night retain a single identity; screenshots substantiate the story. The mobile journey now moves from promise to concrete product evidence without the old pinned detours.

| Heuristic | Score |
|---|---:|
| Visibility of status | 3/4 |
| Match with real-world language | 3/4 |
| User control | 3/4 |
| Consistency | 4/4 |
| Error prevention | 4/4 |
| Recognition | 3/4 |
| Flexibility/efficiency | n/a |
| Aesthetic restraint | 3/4 |
| Recovery | 3/4 |
| Help/documentation | n/a |
| Total | **26/32** |

The four primary mobile section choices are recognizable and the source link is grouped separately. Benefits precede implementation detail. The acquisition path is honest; source compilation still requires technical ability and is not a replacement for a public installer launch.

## Browser and accessibility evidence

Independent technical assessment used Chromium at desktop 1440px and mobile 390px in both light/dark themes, with settled scroll reveals. Layout checks additionally covered 320px and 768px.

- All four settled axe runs passed 50 rules with no meaningful site-copy contrast failures or prohibited CLI labels.
- Two remaining automated contrast hits are inside the aria-hidden terminal rendering, presented as an image with a description. They reproduce the real TUI palette and were excluded as screenshot-only false positives. Background-image/canvas contrast checks marked incomplete were not counted as passes; shared live-copy tokens were checked against relevant sky/surface colors.
- Reduced motion leaves no hidden content or pin spacers and retains static natural scenes.
- Blocking all external .js requests, while allowing the inline first-paint script, leaves all nine headings and both source-build CTAs visible.
- Actual Chromium default standard font increased from 16 to 32px: no horizontal overflow; header actions remain in the viewport, and hero text starts below the enlarged header.
- Section menu opens with Enter; Escape returns focus to its summary; keyboard section selection navigates and closes it.
- FAQ toggles by keyboard.
- Native comparison range updates through keyboard and synthesized touch; a touch-cancel followed by another touch also works.
- Default layouts show no nondecorative overflow at 320, 390 and 768px.
- Mobile pin spacers: zero. Desktop retains the intended pinned scenes.
- A final accessibility snapshot exposes command strings as StaticText inside native code nodes again.
- Final footer target assertion: every navigation link at least 44×44px.

Complete Impeccable detector scan: **[]**, zero findings. The automatic design hook also reported no deterministic issue on the changed shared layout. No overlay was attempted again because production CSP blocked it during the original audit.

## Release-state regression proof

The existing absent-platform test was updated to assert that missing assets expose no installer. It failed against the old placeholder-as-download behavior and passed after the repair. Both release tests pass.

An isolated copy of the website was built with a synthetic partial-release response; the shared production build remained untouched. Browser assertions verified:
- macOS and Linux real asset URLs produce installer CTAs;
- missing Windows assets produce a source-build CTA;
- mobile actions target #desktop-downloads;
- native mobile click navigates to that chooser rather than its own section;
- available platform links have at least 44px hit height;
- supplied version 1.2.3 appears in structured data.

The fixture URLs were synthetic and were not downloaded. The actual public README was checked and contains the Run section used by source-build links. Today's production fallback is explicit about unavailable installer links.

## Performance

Local unthrottled Chromium, reused browser/cache; representative lab observations, not field Core Web Vitals or real-device benchmarks.

| Metric | Desktop 1440×900 | iPhone 15 emulation |
|---|---:|---:|
| LCP | 1.78 seconds | 0.056 seconds |
| FCP | 0.040 seconds | 0.056 seconds |
| CLS | 0 | 0 |
| TTFB | 2.4 ms | 2.1 ms |
| INP | not measured | not measured |

An early headless measurement returned no paint values and was discarded; valid readings were collected after ensuring the page had rendered. Mobile's latest LCP candidate was an app image, not the same element as the initial audit, so the difference must not be treated as a controlled speedup.

JavaScript artifact total: **136,471 raw bytes; 53,483 combined gzip bytes**. The original audit measured 136,384 / 53,398 respectively: practically unchanged. No dependencies were added. Responsive WebP, explicit image dimensions, lazy offscreen images, self-hosted fonts, reduced-motion alternatives, shared canvas loop and visibility gating remain.

## Validation and scope

- Final production build: 23 Astro files, zero errors/warnings/hints; all three routes build.
- Existing website tests: 2 passed; regression demonstrated failing before the repair.
- Source comparison against the saved baseline: 12 files, 122 additions and 77 deletions; no whitespace errors.
- No backend, desktop-app, legal-copy or image asset changes.
- Own audit browsers and isolated fixture server stopped. Existing shared preview left running.
- Original audit snapshot closed; this snapshot records the verified result and retains the previous trend.

Evidence is archived beside this report under evidence/2026-10-12-landing-fixed/. Raw technical-first-round.md predates the final footer-width and desktop-navigation alignment confirmation; final-touch-targets.json and final-navigation.json record those final outcomes.

Questions skipped: zero remaining priority issues; the requested verification is complete.

Not checked: physical devices, Safari/Firefox, VoiceOver/NVDA, installer execution, throttled production loading or field INP. Public installers remain unpublished; the current website uses an honest source-build fallback.
