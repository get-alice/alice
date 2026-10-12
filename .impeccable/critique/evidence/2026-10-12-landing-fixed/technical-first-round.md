# Independent technical re-audit B

Production http://localhost:4400, own Chromium agent-browser session alice-tech-reaudit (closed), one complete detector scan and one bounded browser assessment round. No source edits. No design-agent output read.

Scores: Accessibility 4/4; Performance 4/4 (implementation inspection, parent supplies measurements); Theming 4/4; Responsive 3/4; Implementation integrity 4/4. Total19/20, Excellent. Integrity passes: coherent product-specific desktop/terminal comparison, purposeful semantic native controls, explicit source fallback and matched screenshots. Detector apps/website/src returned [] (detector.json).

0 verified P0/P1/P2. One P3 strict measurement edge: footer Terms 43.484×44px at390px viewport, 0.516px below requested44×44 floor; WCAG2.5.8 target sizing passes. Optional minimum width2.75rem in Base.astro footer li a closes the literal floor. License links embedded in prose are inline-text target exceptions, not touch-control failures.

Prior technical concerns:
- Five meaningful ink-3 contrast failures cleared: light #414660 on #eef5fb8.41:1, dark #9da4c4 on #0d11207.64:1. Footer now light #1f3a1c on #74ad574.68:1/dark #cfe6c6 on #10241912.25:1. Four settled desktop1440/mobile390 light/dark axe results each pass50 rules, with no meaningful site-copy contrast violations. Two remaining contrast nodes are within the aria-hidden, role=img TUI screenshot: #8c8c9d on #2e2640 4.32:1. This intentionally reproduces the actual app palette, has its accessible image description and separate readable CLI text. Excluded as screenshot-only false positives rather than rewriting product imagery. Background-image/canvas incomplete contrast checks are manual-review entries, not failures.
- CLI aria omission cleared at source/settled DOM: three intact semantic code examples are visible, have no SplitText transform or aria-hidden parent. Mirror code retains list/read, draft reply, archive descriptions. Mobile intentionally presents the first example. Full AX-tree text was not separately exported; native DOM semantics plus the removal of the offending SplitText code verify this fix.
- Partial module failure cleared: blocked **/*.js (inline head script active), root os=mac/theme=light; hidden reveal/split elements0; all9 headings and both Build from source CTAs remain rendered. technical-module-failure.png/technical-checks.json.
- Browser default font doubled cleared: actual CDP Page.setFontSizes standard32/fixed26 gives root32px/body34px at390, overflow0; header expands to349.19px, hero heading begins384px below it; all header controls remain inside viewport. technical-font32.json and technical-font32-viewport.png. CSS font-size injection was not used for this evidence.
- Mobile final CTA self-anchor cleared on public no-installer build: both visible hero/final actions explicitly say Build from source and link https://github.com/get-alice/alice#run. Release-status copy honestly explains unavailable installers. Parent independently verifies release-available Choose desktop version behavior.
- Header40px/platform20px touch targets cleared: header Explore/theme44×44, Get Alice97.55×44, visible primary actions215.20×52; footer navigation minheight44. Parent separately verifies all release fixture platform links44px. Tiny Terms width edge above remains P3.

Additional verification:
- Desktop1440 light/dark/mobile390 light/dark axe after scrolling every section and waiting for reveals to finish. Raw results technical-*-axe.json. Each: one contrast rule with two screenshot-only nodes,50 passes.
-320,390,768: document overflow0, pin-spacers0.1440: expected3 desktop pins.
- Menu Enter opens, Tab/Escape closes and returns solid focus outline to summary. Enter/Tab/Enter selects Product, URL/#product, menu closed.
- FAQ Enter toggles open and closed.
- Native range ArrowRight50→51 with CSS split51%. Chromium synthesized touch drag50→81 with CSS split81%; after touchCancel, fresh touch changes29. technical-cdp.json. Native control does not maintain custom drag state.
- Reduced motion: query matches, motion class false, pin-spacers0, hidden content0. Static canvas alternative retained and theme.ts skips view transition under reduced motion.
- Current Shot.astro preserves responsive WebP widths, intrinsic sizes, async decoding and lazy offscreen images. Nature scenes render only while visible and still media draws static frames; theme values read outside per-frame loop. Parent owns measured performance checks.

Limitations: emulated Chromium and synthesized touch, no physical device/VoiceOver/Safari; no successful public installer delivery because none are published; actual release fixture covered by parent; no redundant performance trace. One P3 target-width measurement edge, no blocking technical issue.
