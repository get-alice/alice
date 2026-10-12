---
target: Alice landing page design UX story and performance audit
total_score: 20
max_score: 32
na_heuristics: 7,10
p0_count: 0
p1_count: 5
target_identity: "file:/Users/amruth/Projects/alice/apps/website/src/pages/index.astro"
target_fingerprint: "sha256:45fb7a666aabd8dcbb48b497c0825cdfb27db930995ac296dfe04e658d371c9c"
target_path: /Users/amruth/Projects/alice/apps/website/src/pages/index.astro
timestamp: 2026-10-12T17-39-55Z
slug: apps-website-src-pages-index-astro
closed: true
---
# Alice landing page audit — 12 October 2026

Method: independent design and technical agents, plus a separate performance measurement pass.
Target: apps/website/src/pages/index.astro, built from the current workspace.
Rendered target: existing production preview at http://localhost:4400/.
Scope: design, user experience, product story, accessibility, theming, responsive behavior, performance. Audit only; no source fixes.

> 🧠 **From Hindsight memory (Alice landing page (getalice.email))** — The redesign aims to encourage downloads of a free, open-source, lightweight email client while preserving the cheerful meadow, flower identity, real screenshots, desktop/terminal comparison, and optional AI story. Those elements were verified in the current implementation.

## Verdict

Preserve the visual world. Alice feels authored: the flower, meadow, day/night atmosphere and editorial typography make the calm-inbox promise recognizable. Real app screenshots and the same conversation across desktop, terminal and AI connect the presentation to the actual product.

The implementation is not ready for a confident download launch. The weakest point is the handoff from interest to installation, followed by accessibility and resilience. The mobile story takes too long to deliver readable product proof.

Implementation integrity: passes visual coherence, fails readiness checks. The deterministic detector is clean, but live testing exposes multiple concrete defects; a clean detector is not a release gate.

## Audit Health Score

| Dimension | Score | Key finding |
|---|---:|---|
| Accessibility | 2/4 | Contrast failures and CLI commands absent from the accessibility tree |
| Performance | 3/4 | Healthy local measurements; continuous decorative animation remains an efficiency observation |
| Theming | 3/4 | Cohesive day/night system; secondary text contrast fails in both |
| Responsive design | 2/4 | Default layouts fit; enlarged default text clips header Download |
| Implementation integrity | 2/4 | Script-dependent visibility and download promise do not survive real edge cases |
| Total | **12/20** | **Acceptable — significant work needed** |

Issue count after deduplication: **0 P0, 5 P1, 6 P2, 0 P3**.
Prioritize the five P1 findings before release. The P2 findings improve the mobile persuasion journey and usability.

## Design and story

The intended arc is specific: inbox noise → calm → real product → lightness → terminal capability → optional assistance → privacy reassurance → free download. The product screenshot is the credibility peak. The dandelion and comparison are memorable moments. The valleys are the prolonged mobile terminal sequence and the uncertain download handoff.

Three strengths to maintain:
- The meadow and day/night worlds express calmness while retaining a recognizable flower identity.
- Real screenshots provide evidence, and repeating Sophie's conversation makes desktop, terminal and AI feel like one product.
- Privacy reassurance names concrete mechanisms: direct provider connection, local storage, system keychain and public source. Optional AI explains control and limits.

The strongest story improvement is earlier, more readable proof of one everyday mail task. Keep the technical differentiators, but connect them to outcomes before naming implementation details.

### Nielsen heuristic assessment

| # | Heuristic | Score | Key finding |
|---|---|---:|---|
| 1 | Visibility of system status | 2/4 | Fallback release state is unexplained |
| 2 | Match with the real world | 3/4 | Plain hero language; concentrated infrastructure jargon |
| 3 | User control and freedom | 2/4 | Mobile shortcuts disappear; scroll controls comparison |
| 4 | Consistency and standards | 3/4 | Cohesive styling; Download has inconsistent outcomes |
| 5 | Error prevention | 2/4 | OS detection works, but mobile final CTA returns to itself |
| 6 | Recognition rather than recall | 3/4 | Visible platform choices; tiny product details and hidden shortcuts |
| 7 | Flexibility and efficiency | n/a | Persuade surface |
| 8 | Aesthetic and minimalist design | 3/4 | Distinct world; mobile scenes overstay their information value |
| 9 | Error recovery | 2/4 | GitHub fallback offers no readiness explanation |
| 10 | Help and documentation | n/a | Persuade surface; FAQ and GitHub instructions present |
| | Total | **20/32** | **Acceptable** |

## P1 findings

### 1. Download promise has no public installer behind it

Category: implementation integrity / UX / story.
Locations: apps/website/src/lib/release.ts:15 and :36; apps/website/src/components/DownloadButton.astro:7; apps/website/src/components/Download.astro:27.

The latest-release API returned 404 during this audit. The public releases list returned an empty array. GitHub's latest-release URL redirected to the general releases page. The build correctly takes its fallback, but the primary desktop CTA still reads “Download for macOS” and equivalent platform labels. Every platform row currently leads to the same release fallback, with no version shown.

Impact: the page asks visitors to install a product but cannot deliver an installer. This is a launch-readiness and trust problem, even though GitHub/source-build instructions offer an alternative.

Recommendation: ship actual platform assets before presenting installer CTAs. Until then, label fallback destinations “View releases” or “Build from source” and state readiness honestly. Rebuild the static page when release assets become available.
Suggested command: $impeccable clarify, then $impeccable harden.
Live source: https://github.com/get-alice/alice/releases
Evidence: evidence/2026-10-12-landing/desktop-download.png.

### 2. Failed external JavaScript hides the product story and hero actions

Category: implementation integrity / accessibility / resilience.
Locations: apps/website/src/layouts/Base.astro:46 and :283; apps/website/src/components/Hero.astro:139; apps/website/src/scripts/story.ts:25.

The inline first-paint script opts into the motion class before the animation module initializes. Aborting external .js requests while allowing inline code leaves all nine headings, the hero explanation and the hero CTA invisible. This is a partial-script-failure case; it does not mean a fully JavaScript-disabled page was tested or fails identically.

Impact: visitors on a failed or interrupted script load get decorative scenery instead of the product explanation and primary action.

Recommendation: keep content visible by default. Enable initial hiding only when animation initialization can reveal it, with an error/fallback path.
Suggested command: $impeccable harden.
Evidence: evidence/2026-10-12-landing/module-failure.png and module-failure.json.

### 3. Animated CLI commands disappear for assistive technology

Category: accessibility.
Locations: apps/website/src/scripts/story.ts:156; apps/website/src/components/Mirror.astro:38.
Standard: WCAG 1.3.1 / meaningful text exposure; axe aria-prohibited-attr.

SplitText creates aria-hidden character elements inside generic spans and gives the spans aria-label values that those generic elements cannot use. The Chromium accessibility tree exposes empty code nodes followed only by their explanations for all three commands. Unlike the decorative terminal capture, these command examples are meaningful page text.

Recommendation: preserve whole commands in the accessibility tree. Avoid splitting the actual accessible command, or animate an aria-hidden copy and retain unsplit text. Reverting SplitText after typing should also be verified to restore the intended semantics.
Suggested command: $impeccable harden.
Evidence: evidence/2026-10-12-landing/cli-accessibility.json.

### 4. Secondary text fails contrast in both themes

Category: accessibility / theming.
Locations: apps/website/src/layouts/Base.astro:128 and :162; apps/website/src/components/Mirror.astro:94; apps/website/src/components/Cheshire.astro:42; apps/website/src/components/Download.astro:51.
Standard: WCAG 1.4.3, normal text 4.5:1.

Five meaningful elements fail: three CLI explanations, the AI privacy/control fine print, and download instructions/meta. Measured ratios were 3.89:1 in light and 4.45:1 in dark against the evaluated backgrounds. Contrast over canvas/gradient/image backgrounds also needs manual assessment where axe reported incomplete checks.

Impact: useful explanations and reassurance are harder to read for low-vision visitors.

Recommendation: strengthen the shared ink-3 token in each theme; verify it against the actual sky and surface backgrounds, not only a solid fallback.
Suggested command: $impeccable harden.
Evidence: evidence/2026-10-12-landing/axe-desktop-light.json and axe-desktop-dark.json.

### 5. Enlarged default text removes the header Download action

Category: accessibility / responsive design.
Locations: apps/website/src/layouts/Base.astro:185, :194, :229, :248.
Standard: WCAG 1.4.4, resize text without losing content/functionality.

At a 390px viewport with Chromium's default standard font increased from 16 to 32, the Download button begins around x407 and ends around x614. Horizontal overflow is clipped, so that action is outside the visible viewport. A root-font-size simulation produced the same geometry; the agent also confirmed it using actual browser default-font settings.

Recommendation: reflow or simplify the fixed header as text grows. Allow the brand/action layout to wrap or reduce secondary controls without clipping the primary action.
Suggested command: $impeccable adapt.
Evidence: evidence/2026-10-12-landing/text-zoom.png and text-zoom.json.

## P2 findings

### 6. Mobile final Download Alice CTA returns to itself

Category: responsive design / UX.
Locations: apps/website/src/components/DownloadButton.astro:11; apps/website/src/components/Download.astro:20.

The correctly detected other/mobile OS gets href="#download". That component also appears inside the download section. Clicking there only navigates to the section already on screen. Platform rows are a workaround.

Recommendation: make this action “Choose a desktop version” and point to the platform chooser, or provide clear desktop download guidance. Avoid repeating a section-navigation CTA in its own destination.
Suggested command: $impeccable clarify.
Evidence: evidence/2026-10-12-landing/mobile-download.png and mobile-download.json.

### 7. Mobile terminal comparison is long and too small to inspect

Category: responsive design / story / user control.
Locations: apps/website/src/components/Mirror.astro:50 and :65; apps/website/src/scripts/story.ts:151.

At 390×844, the section occupies roughly 2,813 CSS pixels. The desktop comparison is scaled down while typography follows 100cqw / 94, and the pinned scroll sweep applies on mobile too. Multiple swipes reveal little readable detail.

Recommendation: retain the large-screen comparison, but use a compact unpinned mobile treatment with a readable crop, a clear Desktop/Terminal cue and one useful command.
Suggested command: $impeccable adapt.
Evidence: evidence/2026-10-12-landing/mobile-terminal.png.

### 8. Mobile removes all section shortcuts

Category: responsive design / discoverability.
Location: apps/website/src/layouts/Base.astro:259.

Below 52rem, Product, Terminal, Privacy and FAQ disappear while theme, GitHub and Download remain. Visitors checking provider support or privacy must scroll through the entire story or leave.

Recommendation: preserve direct access through a compact accessible “Explore” menu or equivalent existing navigation pattern, keeping Download prominent.
Suggested command: $impeccable adapt.

### 9. The lightweight story leads with implementation terms

Category: story / copy / cognitive load.
Location: apps/website/src/components/Small.astro:2.

The concrete 8 MB / Apple-silicon argument is followed by Rust, Tauri, daemon, CLI and SQLite. That concentrates jargon before a first-time mail-client visitor sees the practical consequences.

Recommendation: lead with the consequences already supported by the product: a small download, one synchronized inbox across interfaces, and local mail you can back up. Keep implementation terms as secondary credibility.
Suggested command: $impeccable clarify.

### 10. The mobile opening delays real product proof

Category: design / story / responsive design.
Locations: apps/website/src/components/Hero.astro:11 and :141; apps/website/src/scripts/story.ts:79.

Cropped notifications dominate the upper mobile view. The real app first appears approximately 1,772 CSS pixels down in the tested phone layout. The noise-to-calm concept fits Alice, but its setup is too long for a distracted visitor.

Recommendation: reduce mobile chips, shorten the mobile hero pin, and bring a recognizable slice of the real inbox into the next screen. Preserve the meadow and bloom.
Suggested command: $impeccable quieter, then $impeccable adapt.
Evidence: evidence/2026-10-12-landing/mobile-hero.png.

### 11. Some touch targets miss the audit's 44px comfort criterion

Category: responsive design / motor accessibility.
Locations: apps/website/src/layouts/Base.astro:257 and :274; apps/website/src/components/Download.astro:49.

Header theme/GitHub controls are 40×40px; the small Download is about 106×40px; platform release links are about 137×20px. This is a comfort/usability finding. It is not asserted as a WCAG 2.5.8 failure because spacing and exceptions must be considered.

Recommendation: enlarge hit areas with padding, particularly platform links; preserve the compact visual layout.
Suggested command: $impeccable adapt.

## Performance evidence

Measured in local Chromium, without CPU/network throttling, using a reused browser/cache. Phone results are emulation, not physical Safari.

| Metric | Desktop | iPhone 15 emulation |
|---|---:|---:|
| LCP | 1.808 seconds | 0.784 seconds |
| CLS | 0 | 0 |
| FCP | 0.124 seconds | 0.100 seconds |
| TTFB | 7.2 ms | 3.1 ms |
| INP | not measured | not measured |

Reduced-motion mobile LCP was 0.064 seconds. The difference illustrates entrance-animation delay; it is not a controlled real-device benchmark.

A 6.516-second desktop idle trace recorded zero Layout events and a maximum renderer-main task of 2.69ms. No main-thread jank was demonstrated. These measurements are encouraging and do not justify a broad animation rewrite.

Build output JavaScript: 136,384 raw bytes across three scripts, about 53,398 bytes combined gzip. Fonts: three self-hosted files, approximately 340 KiB. Gzip values are computed artifact sizes, not verified production transfer sizes.

Positive implementation practices:
- Responsive WebP screenshots, explicit image dimensions, async decoding and lazy loading.
- DPR capped at 2 for canvas.
- Shared canvas loop and visibility gating; offscreen bento loops pause.
- No client React hydration.
- Immutable caching configuration for hashed assets.

Efficiency observations, not measured defects: the fixed air canvas is always in view, so its RAF loop runs at rest; visible meadows read one bounding rectangle each frame; hero fragments retain will-change and repeating tweens. The local trace showed little main-thread work, so these are not prioritized as proven performance regressions.

Good Core Web Vitals reference thresholds are LCP <=2.5s and CLS <=0.1. Lab results do not establish field performance: https://web.dev/articles/vitals.

## Detector and browser verification

Impeccable detector ran exactly once against apps/website/src: exit 0, output [], zero findings and zero detector false positives.

Axe 4.12.1 was run after scrolling/settling sections at 1440×900 and iPhone12 emulation 390×844, both themes. Each run reported two rule types:
- aria-prohibited-attr: three command nodes, verified.
- color-contrast: seven nodes, five meaningful failures and two excluded decorative terminal-rendering hits.
- 49 passing rules, 40 inapplicable, one incomplete contrast group.

The two terminal-rendering contrast hits belong to an aria-hidden pre inside a role=img with an accessible description. They were not counted as extra live-text failures. Do not mistake incomplete contrast checks for passes or call this WCAG certification.

Verified positives:
- Visible keyboard focus and skip link.
- FAQ opens with Enter.
- Product/download anchor content sits below the fixed header.
- Native range comparison updates with ArrowLeft (100→99).
- CDP synthesized touch updates the slider (99→82) without moving page scroll.
- No nondecorative default-text overflow at 320×740, 390×844 and 768×1024.
- Reduced motion leaves no hidden headings/content or pin spacers after settling.
- Theme switching, labels and matching real screenshots work.

Cognitive-load findings: concentrated jargon, hidden mobile navigation, unexpected release-context switch, a comparison controlled by scrolling while being inspected, and overly prolonged mobile notification setup. No decision point offers more than four equally weighted actions; the three platform groups are reasonable.

Persona findings:
- First-timer: understands the hero but gets jargon and unexplained release readiness.
- Distracted phone visitor: loses Privacy/FAQ shortcuts, spends several swipes on the terminal, and the final CTA returns to itself.
- Accessibility-dependent visitor: loses CLI text, encounters low-contrast explanations, and can lose the header action with enlarged text.
- Stress tester: sees platform-specific promises resolve to the same fallback and sees the entire headline/hero action disappear on module failure.

## Recommended order

1. $impeccable harden: preserve content when scripts fail, restore accessible commands, correct secondary contrast.
2. $impeccable clarify: make release/fallback/mobile actions honest and improve benefit-led technical copy.
3. $impeccable adapt: fix enlarged-text header, shorten mobile pinned scenes, restore section navigation and improve hit areas.
4. $impeccable quieter: trim mobile opening noise while retaining the narrative.
5. $impeccable polish: one bounded visual confirmation after the fixes.

The release state must be resolved separately; UI polishing cannot produce missing public installers. Commands can be run one at a time or together. Re-run $impeccable audit after fixes.

No implementation changes were made. Physical devices, WebKit/Firefox, VoiceOver/NVDA, installation success, production throttled loading and field INP were not checked.
