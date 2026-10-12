# Independent technical assessment B

Target: apps/website/src/pages/index.astro with apps/website/src components/layouts/scripts; production preview http://localhost:4400/. Audit only; no source edits.

## Scores

Accessibility 2/4; Performance 3/4 (code review only; parent owns detailed measurements); Theming 3/4; Responsive 2/4; Implementation integrity 2/4. Total 12/20, Acceptable.

The implementation expresses a coherent product-specific system: real Alice desktop screenshots, an actual terminal rendering, local-service/optional-AI explanations, consistent semantic sections and tokens. However progressive enhancement and a mobile download loop are functional failures. The deterministic detector found zero implementation patterns; that does not invalidate browser-discovered defects.

## Verified issues

1. P1 — Nondecorative secondary text fails contrast in both themes. Base.astro:128,162 define --ink-3; Mirror.astro:94 uses it for three command notes; Cheshire.astro:42 uses it for AI scope/privacy fine print; Download.astro:51 uses it for install instructions/meta. Axe measures 3.89:1 light (#7c7889/#eef5fb), 4.45:1 dark (#7d798b/#0d1120), both below 4.5:1. Five unique meaningful text nodes fail in desktop/mobile light/dark. WCAG 1.4.3. Darken/lighten this shared token or use --ink-2 where body-sized information must be readable. Suggested: impeccable harden/colorize.

2. P1 — Animated CLI commands disappear from the accessibility tree. story.ts:156 splits generic data-type spans from Mirror.astro:38 into aria-hidden character divs and gives the generic parent an unsupported aria-label. Axe aria-prohibited-attr reports three nodes. The captured Chromium accessibility tree contains an empty code node in each list item, followed only by its explanatory note: none of the three actual commands are exposed. Keep an unsplit accessible copy (or disable SplitText automatic ARIA for this target and preserve semantic text); revert the split when typing finishes. WCAG 1.3.1/4.1.2. Suggested: impeccable harden.

3. P1 — Module download failure hides all section titles and the hero action/copy. Base.astro:46 opts into .motion before the module works; Base.astro:283 hides all data-split headings; Hero.astro:139 hides data-rise. With only external .js requests aborted and inline head code allowed, all nine section headings, the hero description, and hero CTA group remain visibility:hidden. Pure no-JS is safer; this is the partial-JS/network failure path. Set the motion-ready state only after initialization is viable and preserve visible fallback on failure. Suggested: impeccable harden.

4. P1 — Enlarged default text clips the fixed navigation action. At 390x844 with actual Chromium Page.setFontSizes standard32 (default16), the Download control starts at x407 and ends x614, wholly outside the viewport; document width remains390 because overflow is clipped. The same result occurs with 200% root-font emulation. Base.astro:229-234 and248 retain one-row fixed-height grid/flex; Base.astro:185,194 clip horizontal overflow. Reflow or simplify the header at enlarged text sizes; allow controls to wrap rather than clipping them. WCAG 1.4.4. Suggested: impeccable adapt.

5. P2 — Mobile primary Download Alice points back to its current section. DownloadButton.astro:11 sets unknown/mobile OS URL to #download. In Download.astro:20-23 this same component is already inside #download. An emulated iPhone detects data-os=other; clicking the large button simply changes /#download to #download and keeps the visitor in place. Platform GitHub links provide a workaround. At this location point to actual release choices/release page, or replace the button with explicit desktop-platform guidance. Suggested: impeccable clarify/harden.

6. P2 — Several mobile tap targets miss the audit 44px target. Header theme/GitHub are40x40 and Download is106x40 (Base.astro:257,274); standalone platform links are137x20 (Download.astro:49). FAQ rows75px and primary52px CTA are good. Increase actual hit padding for header/platform controls. This is a motor-accessibility/touch usability improvement, not a claim of WCAG2.5.8 failure: spacing/inline exceptions matter. Suggested: impeccable adapt.

Counts: P0 0, P1 4, P2 2, P3 0. No source changes recommended outside these responsible layers.

## Deterministic and browser evidence

The requested detector was run exactly once: impeccable detect --json apps/website/src; exit0, complete output [] saved as detector.json. Findings0, rule names none, locations none, false positives0.

Axe4.12.1 on desktop1440x900 and iPhone12-emulated390x844, each light and dark after scrolling every section to settle reveal animations: every configuration reports2 rule groups: aria-prohibited-attr3 nodes and color-contrast7 nodes;49 passing rules,40 inapplicable,1 incomplete contrast group. Eight unique nodes are actionable (three CLI, five meaningful text); two TUI text nodes are false positives for this page-level accessibility audit. The terminal is an image equivalent with aria-label and its pre is aria-hidden, so its screenshot-like internal text is not live readable UI. Do not count those two4.32:1 TUI nodes as two additional WCAG defects. Contrast over canvas/background images generates incomplete results (151 nodes desktop-light), so this is not an AA certification. Stored axe files preserve all details.

Keyboard: first Tab exposes Skip to content with visible outline; Enter targets main; Product and Download anchors position content below the fixed header. FAQ summary focus outline is visible and Enter expands its answer. Native comparison range responds to ArrowLeft (100 to99 with matching --split99%).

Touch: separate CDP Input.dispatchTouchEvent gesture under iPhone12 emulation and reduced motion changes native range99 to82, preserves page scrollY3174, and updates --split82%. Actual touch was synthesized in Chromium; this is stronger than a viewport screenshot but is not a physical iPhone/Safari test.

Responsive: no nondecorative overflow at default text at320x740,390x844,768x1024. Root document width matches viewport; intentional decorative image/canvas extents are excluded. Enlarged default text fails as above.

Reduced motion: no hidden headings/reveal content, no pin spacers, no remaining CSS/Web Animation API animations after settling; native slider and FAQ remain functional. Canvas code draws still frames, intentionally preserving hierarchy.

Theming: both themes and switch action/ARIA label work; screenshots swap with tokens. Shared --ink-3 contrast is the material defect. Hard-coded truecolor terminal/sun/flower palettes are intentional illustrative/product palettes, not automatically theme-token violations.

Positive code/performance practices: screenshots become responsive WebP srcsets with dimensions, decoding async and below-fold lazy loading (Shot.astro); one shared canvas RAF loop, DPR capped2, IntersectionObserver gating, reduced-motion still rendering (nature.ts); short transform/opacity-based animation structure and mostly no layout loops. Risks for parent to weigh with trace: fixed full-viewport air stays visible and therefore keeps RAF alive at rest; canvas meadow reads one bounding box per visible scene each frame; Hero.astro:88 leaves will-change on every frag. These are code costs, not measured jank and not independently counted as issues.

## Overlay and cleanup provenance

Own named browser session alice-audit-technical, headless Chromium153.0.0.0. Mutable DOM title and script insertion were supported. Inline preflight script did not execute; production CSP allows self plus script hashes. Live-server started PID32744/port8400; injection of its external detect.js returned script load failure because CSP excludes localhost8400. No overlay was rendered and no human-visible overlay is claimed. Browser console provided no impeccable messages; DOM/axe evidence is the fallback signal. Live-server stopped with stop --keep-inject; injected script node removed; browser closed; shared preview remains running. Evidence files intentionally retained under /tmp/alice-landing-audit for the parent archive. No repo source edits.

Limits: Chromium only; iPhone UA/viewport/touch emulation is not WebKit; Firefox, physical touch hardware, VoiceOver/NVDA, actual installer downloads and install flows were not exercised. Parent owns build/tests and detailed performance results. Questions skipped: this is the isolated Assessment B subtask; the parent synthesizes the final user questions.
