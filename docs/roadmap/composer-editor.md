# Notion-style composer editor

Research behind the matching items in [`todo.md`](../../todo.md). Each entry covers why it matters, how Mailspring and other clients do it, the relevant specs, what Alice has today, a build plan, pitfalls, acceptance criteria and effort. Code citations point at the Alice working tree as of 2026-10-13; Mailspring paths are relative to [Foundry376/Mailspring](https://github.com/Foundry376/Mailspring) (`mailspring/`) and [Foundry376/Mailspring-Sync](https://github.com/Foundry376/Mailspring-Sync) (`mailsync/`).

## Section findings

**Goal.** Writing mail in Alice should feel like writing in Notion:
- type `/` for any block or action;
- select text to get a floating toolbar;
- grab a handle to drag or transform a block;
- Markdown shortcuts as you type;
- rich blocks (to-dos, toggles, callouts, tables, code);
- AI on demand.

What the recipient receives must still be ordinary, well-behaved email that renders in Gmail, Outlook, Apple Mail and plain-text clients.

**Decision: build it from Tiptap's MIT packages and Alice's own UI. Don't use the Notion-like template.**
- Tiptap's [Notion-like Editor template](https://tiptap.dev/docs/ui-components/templates/notion-like-editor) requires a paid Start plan in production and is covered by the [Tiptap Pro License](https://tiptap.dev/pro-license). That license allows use inside your business and bundling into products, but forbids standalone distribution and imposes confidentiality on license information. Alice is `AGPL-3.0-only` (`LICENSE`, `Cargo.toml:10`, `package.json:4`), so all of its source must be publishable under the AGPL. Pro-licensed source can't be.
- The template is also wired to Tiptap Cloud: collaboration and AI both need server-issued JWTs (`TIPTAP_COLLAB_TOKEN`, `TIPTAP_AI_TOKEN`), and image upload expects your own S3 endpoint. That contradicts Alice's local-first, bring-your-own-key design.
- Most of the template's Notion-specific UI pieces are not open source either. In the [UI components list](https://tiptap.dev/docs/ui-components/components/overview), the slash dropdown menu, drag context menu, turn-into dropdown, emoji and mention dropdowns, and color-text popover carry no "Open Source" badge. The slash menu even lists `@tiptap-pro/extension-ai` as a dependency.
- **Everything needed underneath is MIT** in Tiptap 3.31 (checked on npm, 2026-10-13):
  - `@tiptap/extension-drag-handle`, `-drag-handle-react`, `-details`, `-emoji`, `-mention`, `-file-handler`, `-unique-id`, `-table`, `-task-list`, `-mathematics`, `-table-of-contents`, `@tiptap/suggestion`, `@tiptap/static-renderer` and `@tiptap/markdown`.
  - The [Simple Editor template](https://tiptap.dev/docs/ui-components/templates/simple-editor) and its components are MIT: mark, heading, list, blockquote, code block and text-align buttons; link and color-highlight popovers; image upload; search and replace; undo/redo.
  - Alice writes its own slash menu, block menu, mention and emoji pickers on `@tiptap/suggestion` and the MIT drag handle. Each is a few hundred lines and matches Alice's design system instead of Tiptap's.
- **AI goes through Alice's existing BYOK pipeline** (`crates/alice-core/src/ai.rs`), not `@tiptap-pro/extension-ai`.

**Architecture: three representations, one source of truth.**

1. **Editor document (source of truth).** The ProseMirror JSON from `editor.getJSON()`, saved with the draft as `Compose.doc`. Reopening a draft restores every block exactly: toggle state, callout type, task checkmarks, image sizes. HTML round-trips would lose these.
2. **Email HTML (what recipients get).** Generated at save time by an email renderer: `@tiptap/static-renderer` with Alice's own `nodeMapping`/`markMapping`. Each block maps to inline-styled, table-safe markup with a deliberate fallback (table below). Stored as `Compose.html_body`.
3. **Plain text (the `text/plain` alternative, and what CLI, TUI, search, AI and MCP read).** Generated from the same document with `@tiptap/markdown`, so the plain part reads as clean Markdown (`- [ ]`, `>`, fenced code) instead of `html2text` output. Stored as `Compose.body`.
   - The daemon still regenerates `body` with `html2text` when a client sends HTML without text (CLI, MCP).
   - Generating both outputs from one document means the two alternatives never disagree.

**Email rendering of each block.** caniemail shows `<details>`, `<input type=checkbox>`, SVG, flexbox, grid, `<style>` and CSS variables are unreliable across Gmail, Outlook (Word engine) and Yahoo. The renderer therefore emits only inline styles and simple tables:

| Block in the editor | Sent HTML | `text/plain` |
|---|---|---|
| Paragraph, headings H1–H3 | `<p>`, `<h1>`–`<h3>` with inline `margin`/`font-size`/`line-height`; `dir="auto"` | text, `#` headings |
| Bold, italic, underline, strike, code, link, color, highlight | `<strong>`, `<em>`, `<u>`, `<s>`, `<code style>`, `<a>`, `<span style="color">`, `<span style="background-color">` | Markdown marks; links as `text (url)` |
| Bulleted / numbered list | `<ul>`/`<ol>` with inline margins | `-` / `1.` |
| To-do list | `<ul style="list-style:none;padding-left:0">` with ☐ / ☑ characters (never `<input>`) | `- [ ]` / `- [x]` |
| Toggle | Always expanded: bold "▸ summary" paragraph, content indented with `padding-left` | summary line, indented content |
| Callout | One-cell `<table role="presentation">` with background color, border-radius (ignored by Outlook, harmless) and the emoji icon | `> 💡 text` |
| Quote | `<blockquote style="margin:0 0 0 .8ex;border-left:2px solid #ccc;padding-left:1ex">` | `>` |
| Divider | `<hr style="border:0;border-top:1px solid #ddd">` | `---` |
| Table | `<table cellpadding border-collapse>` with inline cell borders | pipe table |
| Code block | `<pre style="white-space:pre-wrap;font-family:ui-monospace,Menlo,Consolas,monospace;background:#f6f8fa;padding:12px">` with highlight colors as inline `<span style>` | fenced block |
| Image | `<img src="cid:…" width height alt style="max-width:100%">`, sent as a `multipart/related` inline part | `[image: alt]` |
| File block | A real MIME attachment, plus an inline "📎 name · size" line | `[attachment: name]` |
| Mention (contact) | `<a href="mailto:…">@Name</a>` | `@Name` |
| Emoji | Unicode character | Unicode character |
| Columns (P3) | Two- or three-cell `<table>`, each cell `valign=top` | Columns in sequence |
| Bookmark card (P3) | Bordered table: title, description, URL text link. No remote image unless proxied or inlined as `cid:` | title and URL |
| Math (P3) | KaTeX rendered to PNG in the composer, sent as `cid:` with LaTeX in `alt` | LaTeX source |
| Signature, quoted history | The signature block as written; quoted history as a standard `<blockquote type="cite">` / Gmail-compatible quote | as today |

**Bundle strategy.** The composer is a lazy chunk, loaded on first compose, as the composer research already recommends: Tiptap 3 core + StarterKit is about 126 KB gzipped. Heavy pieces load on first use within it:
- `katex` (math): about 75 KB gzipped plus fonts.
- `lowlight` with a common-languages subset (code blocks).
- `emojibase-data` (the emoji extension's dataset): load one locale's compact dataset lazily.
- Table extensions.

**Watch out for:**
- **Drag handle dependencies.** `@tiptap/extension-drag-handle` 3.31.4 has hard peer dependencies on `@tiptap/extension-collaboration`, `@tiptap/y-tiptap` and `@tiptap/extension-node-range`, so `yjs` is installed even though Alice doesn't collaborate. Measure whether the Yjs code ends up in the chunk. If it does, write a small handle on ProseMirror instead: a hover decoration, then `NodeSelection`, then native drag. The MIT `node-range` package covers multi-block selection.
- **IME.** ProseMirror handles composition well, but every custom node view (callout, toggle, mention) is a new source of IME bugs. Test CJK input and macOS press-and-hold on WKWebView, WebKitGTK and WebView2 for every node (see the composer research's IME notes).
- **Shortcut clashes.** Mod+Enter is "send" in Mailspring and most clients. Don't bind it to a block action. Mod+K stays "link" inside the editor (the app already ignores shortcuts while composing, `apps/desktop/src/App.tsx:280`).
- **XSS.** Slash-menu inserts, templates, pasted HTML and quoted originals all enter through the schema, so unknown tags and attributes are dropped, and DOMPurify runs on any raw HTML before `setContent`. Link previews are fetched by the daemon without cookies.

**Build order.**
1. Foundation (P1): Tiptap scaffold → saved document + email renderer → Markdown shortcuts → slash menu → bubble menu → block handle → placeholders.
2. Blocks (P2): to-dos → toggles → callouts → tables → code → emoji → mentions → images and files → templates and variables → signature and quote blocks → AI → find and replace → accessibility and IME pass.
3. P3: columns, bookmark cards, math, table of contents, focus mode, draft history, Markdown paste and copy.

## P1 → Notion-style editor

<a id="build-the-composer-on-tiptap-3s-mit-packages-with-alices-own-ui-no-pro-template"></a>

### Build the composer on Tiptap 3's MIT packages with Alice's own UI (no Pro template, no Tiptap Cloud)

*Roadmap: P1 · Notion-style editor · new*

- **Why:** Settles the editor stack once, so every later block and menu builds on the same base, and keeps Alice's source fully publishable under the AGPL.
- **Competitors:**
  - Notion, Craft, Linear and Superhuman all ship block editors with slash menus. Tiptap's own Notion-like template shows the target UX (https://template.tiptap.dev/preview/templates/notion-like).
  - Mailspring is stuck on a forked Slate 0.45 (see the composer research), and its toolbar editor never gained block features.
  - Spark and Superhuman offer `/` snippet menus but not general blocks. Notion Mail has a Notion editor but only for Gmail.
- **Spec:** Tiptap 3 docs: [extensions](https://tiptap.dev/docs/editor/extensions/overview), [UI components and licenses](https://tiptap.dev/docs/ui-components/getting-started/overview), [Pro License](https://tiptap.dev/pro-license).
- **Alice today:** a plain `<textarea>` in `apps/desktop/src/Compose.tsx`. Its desktop dependencies are React 19, motion, lucide-react and DOMPurify (`package.json:27-30`).
- **Plan:**
  1. Add `@tiptap/react`, `@tiptap/pm` and `@tiptap/starter-kit`. StarterKit 3 already includes Link, Underline, ListKeymap, TrailingNode, Dropcursor/Gapcursor and UndoRedo.
  2. Add `@tiptap/extensions` (Placeholder, CharacterCount, Focus), `@tiptap/extension-typography`, `-text-align`, `-text-style` (TextStyle, Color, BackgroundColor, FontFamily), `-highlight`, `-image`, `-file-handler` and `-unique-id` (stable block ids for the block menu and AI edits).
  3. Create `apps/desktop/src/editor/` with `Editor.tsx`, `extensions.ts`, `emailRender.ts`, `menus/` (slash, bubble, block) and `nodes/` (callout and others), loaded through `React.lazy` when the composer opens.
  4. Copy the MIT Simple Editor components you want (mark, heading, list, link popover, color highlight popover, undo/redo) through `npx @tiptap/cli add`, then restyle them with Alice's CSS variables. Never add `@tiptap-pro/*` packages.
  5. Add a global-attributes extension that sets `dir="auto"` on block nodes, for RTL.
  6. Add a CI check that fails the build if any `@tiptap-pro` package or Tiptap Cloud token variable appears.
- **Pitfalls:**
  - Keep every `@tiptap/*` package on the same exact version; Tiptap releases them in lockstep.
  - The drag handle's Yjs peers: see the next items and the section findings.
  - Measure the chunk with `vite build --report` and keep the inbox's main bundle unchanged.
- **Done when:**
  - The composer opens an empty Tiptap editor in a lazy chunk.
  - `package.json` has no `@tiptap-pro` dependencies, and the CI check passes.
  - The initial inbox bundle size is unchanged; the composer chunk size is recorded in `docs/`.
- **Effort:** M (2–3 days). It replaces step 3 of the existing "Rich text / HTML compose" item, and everything else in this section depends on it.

<a id="save-the-editor-document-with-each-draft-and-generate-the-email-html-and-plain-t"></a>

### Save the editor document with each draft and generate the email HTML and plain text from it

*Roadmap: P1 · Notion-style editor · new*

- **Why:** Notion-style blocks (toggles, callouts, checked to-dos, image sizes) do not survive an HTML round trip. Without the document itself, reopening a draft would quietly flatten it, which would repeat Mailspring's draft-loss history (1.5.4, #948, #851).
- **Competitors:**
  - Notion and Craft store block JSON and export HTML or Markdown on demand.
  - Mailspring stores only the draft's HTML body.
- **Spec:**
  - ProseMirror JSON (`editor.getJSON()`).
  - [Static renderer](https://tiptap.dev/docs/editor/api/utilities/static-renderer) (`renderToHTMLString` with `nodeMapping`/`markMapping`).
  - [`@tiptap/markdown`](https://tiptap.dev/docs/editor/markdown) for the text alternative.
  - RFC 2046 §5.1.4 for `multipart/alternative` ordering.
- **Alice today:** `Compose` has `body` only (`crates/alice-core/src/model.rs:222-229`). The composer research plans `html_body`.
- **Plan:**
  1. **Model:** add `Compose.doc: Option<serde_json::Value>` beside the planned `html_body`. The daemon stores it opaquely, with a size cap of about 5 MB shared with `html_body`.
  2. **Autosave:** the desktop sends `doc`, `html_body` and `body` together. The 400 ms debounce and localStorage backup (`Compose.tsx:54-61`, `apps/desktop/src/drafts.ts`) cover all three.
  3. **Reopen:** load `doc` when present; otherwise fall back to `html_body` (drafts from older versions, or written by CLI or MCP); otherwise `body`.
  4. **Rendering:**
     - `emailRender.ts` holds the per-node mappings from the table in the section findings.
     - Text color defaults to "unset", so recipients in dark mode get their own colors.
     - Inline images are converted to `cid:` references to the draft's attachment store.
  5. **Plain text:** generated with `@tiptap/markdown`; it is also what the CLI, TUI, search and AI read.
  6. **Daemon safety net:** if `html_body` is present without `body`, it regenerates `body` with `html2text`.
- **Pitfalls:**
  - The document schema will evolve. Store a `doc_version` and migrate older documents in the editor, never on the server.
  - Remote image URLs in pasted content must not end up in the sent HTML without the user knowing. Convert them to inline attachments or keep them as links (see the "Never fetch remote content in replies" security item).
  - Outlook's Word engine ignores `border-radius` and handles `max-width` and `padding` on non-table elements unreliably. Prefer `<table>` cells for anything boxed.
- **Done when:**
  - A jsdom test renders a fixture document containing every P1/P2 block, and its HTML matches a snapshot that uses only inline styles (no `<style>`, `class`, `<details>`, `<input>` or `<svg>`).
  - The plain part matches a Markdown snapshot.
  - A Rust test confirms a draft round-trips `doc` unchanged.
  - Litmus or Email on Acid screenshots, or manual checks, in Gmail web, Outlook for Windows, Apple Mail and iOS Mail are recorded in `docs/`.
- **Effort:** M–L (4–6 days). Depends on the Tiptap scaffold and the composer data-model item (`html_body`).

<a id="markdown-shortcuts-and-notion-keyboard-map"></a>

### Markdown shortcuts and Notion keyboard map

*Roadmap: P1 · Notion-style editor · new*

- **Why:** Typing `# `, `- `, `[] `, `> ` or ```` ``` ```` is the fastest way to structure text, and Notion users expect it.
- **Competitors:**
  - Notion: Markdown input rules, `[]` for to-dos, `>` for toggles, `"` for quotes, `---` for dividers, Mod+Alt+1–3 for headings, Mod+Shift+7/8 for lists.
  - Mailspring has partial Markdown auto-replace (`markdown-plugins.tsx`).
  - Demand: GitHub #19, #1091, #891, forum #322.
- **Spec:** Tiptap input rules ([`markInputRule`, `wrappingInputRule`, `textblockTypeInputRule`](https://tiptap.dev/docs/editor/api/input-rules)).
- **Alice today:** none.
- **Plan:**
  1. StarterKit's rules cover headings, lists, blockquote, code, bold, italic, strike and inline code.
  2. Add rules for `[] ` and `[x] ` (to-do), `>> ` (toggle), `!> ` (callout) and `---` (divider). Keep `> ` as quote to match Markdown and email conventions; Notion's `>` toggle conflicts with that.
  3. Keymap: Mod+Alt+0–3 for paragraph and headings, Mod+Shift+7/8/9 for lists and quote, Mod+Alt+C for code block, Tab and Shift-Tab to indent, Mod+D to duplicate a block.
  4. Add a "Markdown shortcuts" toggle in Settings for people who type literal asterisks (MS forum #827).
- **Pitfalls:**
  - Input rules must not fire inside code blocks or inline code.
  - Leave Mod+Enter as send.
- **Done when:** an editor unit test types each shortcut and asserts the resulting node, and turning the setting off disables every rule.
- **Effort:** S–M (1–2 days). This replaces the P2 "Markdown shortcuts in the composer" item.

<a id="slash-command-menu-for-blocks-and-email-actions"></a>

### Slash command menu for blocks and email actions

*Roadmap: P1 · Notion-style editor · new*

- **Why:** `/` is the single entry point that makes block editing discoverable, and it can expose email actions in the same place.
- **Competitors:**
  - Notion's `/` menu offers grouped, fuzzy-searched blocks with aliases.
  - Superhuman's and Spark's `/` menus insert snippets.
  - Tiptap's slash dropdown menu is paid and depends on the Pro AI extension, so don't use it.
- **Spec:** [`@tiptap/suggestion`](https://tiptap.dev/docs/editor/api/utilities/suggestion) (`char: '/'`, `items`, and the `render` lifecycle `onStart`/`onUpdate`/`onKeyDown`/`onExit`).
- **Alice today:** none. The contact autocomplete in `Compose.tsx:117` is the existing pattern for a keyboard-navigable listbox.
- **Plan:**
  1. Create `menus/SlashMenu.tsx` on `@tiptap/suggestion` with a `ReactRenderer`, positioned by floating-ui (already a dependency of the drag handle).
  2. Each item is `{id, title, aliases, group, icon, run(editor)}`, filtered with fuzzy matching on title and aliases.
  3. Groups:
     - **Basic blocks:** Text, Heading 1–3, Bulleted list, Numbered list, To-do, Toggle, Quote, Callout, Divider, Code.
     - **Media:** Image, File, Table, Emoji.
     - **Email:**
       - Attach file
       - Insert template
       - Insert signature
       - Add Cc
       - Add Bcc
       - Schedule send (once "Send later" exists)
       - Remind me (once follow-up reminders exist)
     - **AI** (only when AI is configured): Write…, Continue writing, Summarize thread into a reply.
  4. Use ARIA `listbox` semantics with `aria-activedescendant`. Arrow keys, Enter, Escape and Tab behave as in Notion.
  5. Feed the same registry to the block handle's "Turn into" menu and to the app-wide command palette (P2).
- **Pitfalls:**
  - Don't open in the middle of a word (`a/b`) or inside code.
  - Typing `/` then a space must type a literal slash.
  - Localize item titles and aliases through the existing i18n catalogs.
- **Done when:** a test that types `/to` and presses Enter inserts a to-do list; another checks that `/` followed by Escape leaves a literal slash; keyboard-only operation works with VoiceOver.
- **Effort:** M (2–3 days). Depends on the Tiptap scaffold.

<a id="floating-selection-toolbar-bubble-menu"></a>

### Floating selection toolbar (bubble menu)

*Roadmap: P1 · Notion-style editor · new*

- **Why:** Formatting where the selection is beats a fixed toolbar, and it's the Notion and Medium interaction people expect.
- **Competitors:**
  - Notion: Turn into, B/I/U/S, code, link, color, comment, and "Ask AI".
  - Tiptap's `BubbleMenu` React component is MIT (`@tiptap/react/menus`).
- **Spec:** [BubbleMenu](https://tiptap.dev/docs/editor/extensions/functionality/bubble-menu).
- **Alice today:** none.
- **Plan:**
  1. Create `menus/BubbleMenu.tsx` with Turn into ▾, B, I, U, S, inline code, link (Mod+K popover from the MIT link-popover component), text color and highlight ▾, and clear formatting.
  2. Add AI actions ▾ once the AI item lands.
  3. Hide it for node selections (images and tables get their own menus) and inside code blocks.
  4. Keep a minimal fixed toolbar as well, because some users never select text. The existing composer research toolbar can be collapsed to an "Aa" button.
- **Pitfalls:**
  - The bubble menu must not cover the selection on small windows.
  - Hide it while an IME composition is active.
- **Done when:** selecting text shows the menu, and each button applies its mark (tested with `@testing-library` + jsdom using editor commands).
- **Effort:** S–M (1–2 days). Depends on the Tiptap scaffold.

<a id="block-handle-drag-to-reorder-and-a-block-menu-to-turn-into-duplicate-move-or-del"></a>

### Block handle: drag to reorder, and a block menu to turn into, duplicate, move or delete

*Roadmap: P1 · Notion-style editor · new*

- **Why:** Reorganizing a long message by dragging paragraphs, and turning a paragraph into a to-do or callout, is the core of the Notion feel.
- **Competitors:**
  - Notion's ⋮⋮ handle opens a menu with Turn into, Color, Duplicate, Move to and Delete.
  - Tiptap's Drag Context Menu UI component is paid. The `DragHandle` extension and its React wrapper are MIT.
- **Spec:** [DragHandle React](https://tiptap.dev/docs/editor/extensions/functionality/drag-handle-react): `nested` support for list items and quotes, `onNodeChange`, and `computePositionConfig`.
- **Alice today:** none.
- **Plan:**
  1. Add `<DragHandle editor nested={{ edgeDetection: { threshold: -16 } }}>` with a ⋮⋮ button. Clicking it selects the block and opens a block menu:
     - Turn into (the same registry as the slash menu)
     - Color
     - Duplicate (Mod+D)
     - Move up / down (Mod+Shift+↑/↓)
     - Copy as Markdown
     - Delete
  2. Use the RTL placement config from the Tiptap demo when the block's `dir` is RTL.
  3. Keyboard path: Escape on a block selects it; arrows and the shortcuts above act on it, so dragging isn't the only route.
- **Pitfalls:**
  - `@tiptap/extension-drag-handle` 3.31.4 has hard peer dependencies on `@tiptap/extension-collaboration`, `@tiptap/y-tiptap` and `yjs`. Check the built chunk. If Yjs is bundled, replace the extension with a small ProseMirror plugin (a hover widget decoration plus native HTML5 drag of a `NodeSelection`).
  - Dragging a signature block or the quoted-history block must be disallowed.
- **Done when:**
  - Dragging a paragraph below a list moves it, and undo restores it.
  - Every block-menu action has a keyboard shortcut and an automated test.
  - Bundle size is recorded with and without Yjs.
- **Effort:** M (3–4 days). Depends on the slash menu (shared registry).

<a id="placeholders-and-empty-state-hints"></a>

### Placeholders and empty-state hints

*Roadmap: P1 · Notion-style editor · new*

- **Why:** "Write, or press / for commands" teaches the slash menu without onboarding, and per-block placeholders ("Heading 1", "To-do") make empty blocks legible.
- **Competitors:** Notion shows a placeholder on the focused empty line and on empty headings and to-dos.
- **Spec:** [Placeholder extension](https://tiptap.dev/docs/editor/extensions/functionality/placeholder), exported from `@tiptap/extensions` in v3 (`placeholder` can be a function of the node).
- **Alice today:** a static textarea placeholder.
- **Plan:**
  1. Add `Placeholder.configure({ includeChildren: true, placeholder: ({ node }) => … })` with localized strings.
  2. Show the hint only on the focused empty line, so a reply's first line isn't cluttered.
- **Pitfalls:** screen readers must not read the placeholder as content. Expose it through `aria-placeholder` on the editor.
- **Done when:** an empty heading shows "Heading 1", and the hint disappears on typing.
- **Effort:** S.

## P2 → Notion-style editor

<a id="to-do-lists-that-send-as"></a>

### To-do lists that send as ☐ / ☑

*Roadmap: P2 · Notion-style editor · new*

- **Why:** Action items in mail ("Next steps") are common. Checkboxes make them scannable, and checking them off in a draft is satisfying.
- **Competitors:**
  - Notion, Apple Notes and Craft all have to-do blocks.
  - No mainstream mail client sends interactive checkboxes; caniemail shows `<input type=checkbox>` support is inconsistent, and state never syncs back anyway.
- **Spec:** `@tiptap/extension-task-list` / `-task-item` (MIT; `nested: true`).
- **Alice today:** none.
- **Plan:**
  1. Add TaskList and TaskItem.
  2. The renderer emits `<ul style="list-style:none;padding-left:0">` with ☐ / ☑ and strikes through done items.
  3. Plain text gets `- [ ]` / `- [x]`.
  4. When replying to mail that contains ☐/☑ lines, parse them back into a task list.
- **Pitfalls:** some fonts lack ☐/☑. Use U+2610 and U+2611, which every system font has, and never images.
- **Done when:** a renderer snapshot test passes, and a round trip from reply text to a task list works.
- **Effort:** S.

<a id="toggle-blocks-that-send-expanded"></a>

### Toggle blocks that send expanded

*Roadmap: P2 · Notion-style editor · new*

- **Why:** Toggles let long mails hide detail (logs, background, FAQs) while drafting and reading in Alice.
- **Competitors:**
  - Notion's toggle lists.
  - `<details>`/`<summary>` is stripped or ignored by Gmail and Outlook.
- **Spec:** `@tiptap/extension-details` (MIT; `persist` stores open state).
- **Alice today:** none.
- **Plan:**
  1. Add Details, DetailsSummary and DetailsContent with `persist: true`, so the draft remembers which toggles are open.
  2. The renderer always sends the content expanded: a bold "▸ summary" line, then an indented content block.
  3. Alice's own reader could restore toggles when it sees a `data-alice-toggle` attribute, an optional nicety for Alice-to-Alice mail.
- **Pitfalls:** don't rely on any hiding in sent mail. Recipients must always see the content.
- **Done when:** a snapshot shows the sent HTML has no `<details>` and all content is visible.
- **Effort:** S–M.

<a id="callout-blocks"></a>

### Callout blocks

*Roadmap: P2 · Notion-style editor · new*

- **Why:** Highlighting a key decision, deadline or warning is one of Notion's most used blocks, and it reads well in mail.
- **Competitors:** Notion callouts have an emoji icon and a background color. Tiptap has no callout extension, so it needs a custom node.
- **Spec:** a Tiptap custom `Node.create` (group `block`, content `block+`, attrs `icon` and `color`) with a React node view.
- **Alice today:** none.
- **Plan:**
  1. Create `nodes/Callout.ts` with a node view that has an emoji picker on the icon and a color menu (5 muted colors that work in light and dark).
  2. The renderer emits a one-cell `role="presentation"` table with `bgcolor` plus an inline `background-color`, and the emoji in its own cell.
  3. Plain text gets `> 💡 …`.
- **Pitfalls:** pick colors with enough contrast on both white and dark-mode inverted backgrounds (Gmail and Outlook invert some colors). Test in Outlook dark mode.
- **Done when:** the snapshot passes, and screenshots in Gmail and Outlook show the callout box.
- **Effort:** S–M.

<a id="tables-in-the-composer"></a>

### Tables in the composer

*Roadmap: P2 · Notion-style editor · new*

- **Why:** Comparisons, schedules and pricing in mail are tables; today people paste screenshots instead.
- **Competitors:**
  - Notion, Gmail and Outlook all support inserted tables.
  - Mailspring added Excel/HTML paste with CSS inlining in 1.7.4.
- **Spec:** `@tiptap/extension-table` (`TableKit`: table, row, header, cell; column resizing).
- **Alice today:** none.
- **Plan:**
  1. Add TableKit with a header row and resizable columns.
  2. Add a table bubble menu: add or remove rows and columns, toggle the header, delete the table.
  3. The renderer emits `<table style="border-collapse:collapse">` with inline cell borders and padding, plus explicit `width` attributes so Outlook keeps the layout.
  4. Pasting tables from Excel, Sheets or web pages keeps their structure (this ties to Paste and Match Style).
  5. Plain text gets pipe tables.
- **Pitfalls:**
  - Very wide tables need `max-width` and horizontal scroll in Alice's reader (MS 1.5.7).
  - Nested tables inside cells must be disallowed in the schema.
- **Done when:** the snapshot passes; pasting from Google Sheets keeps the cells; Outlook for Windows shows borders.
- **Effort:** M.

<a id="code-blocks-with-syntax-highlighting"></a>

### Code blocks with syntax highlighting

*Roadmap: P2 · Notion-style editor · new*

- **Why:** Developers mail snippets daily; monospace with highlighting is far more readable than plain text.
- **Competitors:**
  - Notion's code blocks have a language picker.
  - Tiptap's Notion-like template ships syntax-highlighted code blocks.
  - Mailspring's composer has inline code and a code block, with no highlighting.
- **Spec:** `@tiptap/extension-code-block-lowlight` with `lowlight` (highlight.js grammars).
- **Alice today:** none.
- **Plan:**
  1. Add CodeBlockLowlight with a common-languages subset, loaded lazily, and a language dropdown in the block's corner.
  2. The renderer emits `<pre>` with inline `<span style="color:…">` from a light theme, `white-space:pre-wrap` and `font-family` with fallbacks.
  3. Plain text gets fenced code with the language.
- **Pitfalls:**
  - Gmail clips messages over about 102 KB, and highlighting spans add bulk. Skip highlighting for blocks over a few hundred lines.
  - Disable input rules inside code.
- **Done when:** the snapshot passes, and a 1,000-line block still renders and sends within a size budget.
- **Effort:** S–M.

<a id="emoji-picker-on"></a>

### Emoji picker on `:`

*Roadmap: P2 · Notion-style editor · new*

- **Why:** `:smile:`-style shortcodes are muscle memory for Slack, GitHub and Notion users.
- **Competitors:** Notion, Slack and GitHub all use `:` shortcodes. The Tiptap emoji dropdown UI is paid, but the Emoji extension is MIT.
- **Spec:** `@tiptap/extension-emoji`, which brings `emojibase-data`, `emoji-regex` and `is-emoji-supported`.
- **Alice today:** none.
- **Plan:**
  1. Add the Emoji extension with a custom `suggestion.render` using the slash-menu component. Show the top 8 matches; Enter inserts the Unicode character.
  2. Load the locale's `emojibase-data` compact JSON lazily on the first `:`.
  3. Respect the system's skin-tone preference where available.
- **Pitfalls:** don't trigger inside URLs or times (`10:30`). Require a letter after `:`.
- **Done when:** typing `:tada` and Enter inserts 🎉, the emoji data is not in the main composer chunk, and `10:30` doesn't open the menu.
- **Effort:** S.

<a id="mention-contacts-to-add-them-as-recipients"></a>

### @mention contacts to add them as recipients

*Roadmap: P2 · Notion-style editor · new*

- **Why:** In Outlook and Gmail, "@Name" in the body adds that person to the recipients. It's a fast way to loop someone in, and it reads naturally.
- **Competitors:**
  - Outlook and Gmail web: typing `@` and a name in the body adds that person to the To line.
  - Notion: mentions of people and pages.
- **Spec:** `@tiptap/extension-mention` (MIT) with `suggestion` for the item list.
- **Alice today:** contact autocomplete ranked from mail history and address books (`Compose.tsx:117`, `crates/alice-core/src/contacts.rs:35`).
- **Plan:**
  1. Add the Mention extension. Items come from the existing contact autocomplete endpoint, and the menu reuses the slash-menu component.
  2. Inserting a mention adds the address to Cc, or to To if To is empty, with a small undo hint ("Added Pat to Cc · Undo").
  3. The renderer emits `<a href="mailto:…">@Name</a>`.
- **Pitfalls:**
  - Removing the mention must not silently remove the recipient. Ask once with an inline hint instead.
  - Don't trigger on email addresses (`a@b.com`).
- **Done when:** `@pa` + Enter inserts a mention and adds Pat to Cc, and typing an email address does not trigger the menu.
- **Effort:** M. Depends on the Cc and Bcc fields.

<a id="image-and-file-blocks-paste-drop-resize-align-and-caption-sent-as-inline-parts-a"></a>

### Image and file blocks: paste, drop, resize, align and caption, sent as inline parts and attachments

*Roadmap: P2 · Notion-style editor · new*

- **Why:** Screenshots and files belong where they're discussed in the text, not only in an attachment strip.
- **Competitors:**
  - Notion and Craft have image and file blocks.
  - Mailspring has drag/paste inline images (1.6.2), resizing (1.10.8), and dragging a thread in as `.eml` (1.24).
- **Spec:**
  - `@tiptap/extension-image` (resizable in v3).
  - `@tiptap/extension-file-handler` (`onPaste`, `onDrop`, `allowedMimeTypes`).
  - RFC 2387 `multipart/related` for `cid:` images.
- **Alice today:** none. The composer research plans the attachment store and upload route.
- **Plan:**
  1. FileHandler sends images to the draft attachment store and inserts an image node whose `src` is a local `blob:` URL, mapped to the stored file id.
  2. Other file types insert a file block (a 📎 chip with name, size and remove button) and register a regular attachment.
  3. Add an image toolbar: size presets (small, medium, full), alignment and alt text.
  4. The renderer emits `cid:` images with width and height, and file blocks as one text line plus the real MIME attachment.
- **Pitfalls:**
  - Large images: downscale over 2,000 px on insert, keeping the original if the user chooses "Send original size".
  - CSP needs `img-src blob:` (see the composer research).
  - Moving the file block in the text never changes MIME attachment order unexpectedly. Attachments are ordered by first appearance.
- **Done when:** a pasted screenshot sends as `multipart/related`, a dropped PDF sends as an attachment, and removing the block removes the attachment.
- **Effort:** M. Depends on "Attachment upload" and "Inline images" (this item is their editor UI).

<a id="templates-snippets-and-variables-through-the-slash-menu"></a>

### Templates, snippets and variables through the slash menu

*Roadmap: P2 · Notion-style editor · new*

- **Why:** Reusable replies ("/intro", "/pricing") with variables like `{first_name}` turn repetitive mail into seconds. Spark, Superhuman and Gmail templates all do this.
- **Competitors:**
  - Mailspring templates: name, subject and rich body, stored as files (`internal_packages/composer-templates`).
  - Superhuman snippets with variables.
  - Gmail templates (no variables).
- **Spec:** a custom inline atom node `variable` (attrs `name`, `fallback`) rendered as a chip.
- **Alice today:** none. The P2 "Templates with subject" item covers storage.
- **Plan:**
  1. Templates are stored as editor documents (JSON) plus subject, under the daemon (shared with the P2 Templates item).
  2. `/template` or `;;` opens a fuzzy picker. Selecting a template inserts its blocks at the cursor and fills the subject if empty.
  3. Add a variable node with a chip UI: `{first_name}`, `{last_name}`, `{company}`, `{my_name}`, `{date}`.
  4. At send, variables resolve from the first To recipient's contact data. Unresolved variables block sending with an inline warning.
  5. Save the current draft as a template from the composer menu.
- **Pitfalls:**
  - Variables must never send as raw `{…}`.
  - Mail merge (P4) reuses the same variable node.
- **Done when:** a template with `{first_name}` inserted for "Pat Lee" sends "Hi Pat", and an unresolved variable blocks send.
- **Effort:** M. Depends on the slash menu and the P2 Templates storage.

<a id="signature-and-quoted-history-as-protected-blocks"></a>

### Signature and quoted history as protected blocks

*Roadmap: P2 · Notion-style editor · new*

- **Why:** Treating the signature and the quoted original as special blocks avoids classic bugs: signatures duplicated or edited by accident, and slow editing with long quoted threads (MS #720, MS 1.1.3).
- **Competitors:**
  - Gmail trims quoted history behind "…".
  - Mailspring inserts the signature as a special block; users ask to switch signatures inside the draft (forum #1426).
  - Apple Mail marks quoted text as a quote level.
- **Spec:** custom block nodes `signature` (attr `signatureId`; `selectable` but not `draggable`) and `quotedHistory` (atom, collapsed by default).
- **Alice today:** none. The composer research's NEW "Quote the original" item plans the quote content.
- **Plan:**
  1. The signature node renders the selected signature for the current identity. Switching the From identity swaps it (ties to the Send-as and Signatures items), and a small ▾ on hover picks another signature.
  2. The quoted-history node shows "··· Show quoted text". Expanding it makes the quote editable inline (for inline replies).
  3. The renderer emits the signature HTML and a Gmail- and Outlook-compatible quote (`<div class="gmail_quote">` attribution line plus `<blockquote type="cite">`).
- **Pitfalls:**
  - Undo must not remove the signature node unexpectedly.
  - Inline-reply editing inside the quote must keep the quote's original HTML for everything not edited.
- **Done when:** changing From swaps the signature, a 200-message quoted thread doesn't slow typing (measured), and the quote renders correctly in Gmail.
- **Effort:** M. Depends on Per-account signatures and "Quote the original message".

<a id="ai-writing-in-the-editor-with-your-own-key"></a>

### AI writing in the editor with your own key

*Roadmap: P2 · Notion-style editor · new*

- **Why:** "Write a polite decline", "make this shorter", "fix grammar" and "translate to German" right where you write. Notion AI and Superhuman AI made this expected, and Alice already has BYOK AI.
- **Competitors:**
  - Notion AI: space on an empty line opens a prompt, and the selection menu has Improve / Shorten / Fix / Change tone / Translate, with streamed results to accept or discard.
  - Superhuman's "Write with AI".
  - Tiptap's AI extension is Pro and runs through Tiptap Cloud, so don't use it.
- **Spec:** Alice's AI module (`crates/alice-core/src/ai.rs`, `Assistant.tsx`), streaming over the daemon API.
- **Alice today:**
  - Ask Alice offers per-message "Summarize" and "Draft reply" (`apps/desktop/src/Assistant.tsx:56`); drafts arrive via `createCompose`.
  - The long-tail research recommends a shared `complete()` refactor of `ai::run`.
- **Plan:**
  1. Add a daemon endpoint `POST /v1/ai/edit {instruction, selection_markdown, context}` that streams Markdown back, using the user's configured provider and model.
  2. In the editor:
     - **Prompt entry:** pressing space on an empty line, or `/ai`, opens an inline prompt.
     - **Selection actions:** the bubble menu's AI ▾ offers Improve, Shorten, Lengthen, Fix spelling and grammar, Change tone (formal, friendly, direct), Translate… and Custom.
     - **Results:** streamed text appears as a preview decoration, not in the document. Accept replaces the selection (one undo step); Discard removes the preview; Retry regenerates.
  3. Context includes the thread being replied to (with the OTP exclusion the AI module already applies) and the user's writing language.
  4. Hidden entirely when AI isn't configured.
- **Pitfalls:**
  - Treat model output as untrusted. Insert it through Markdown parsing into the schema, never as raw HTML.
  - Respect the AI scope settings and never send drafts to a provider the user hasn't configured.
- **Done when:** with a mocked provider, "Shorten" on a paragraph streams a preview, Accept applies it in one undo step, and with AI off no AI entries appear anywhere.
- **Effort:** M–L. Depends on the bubble menu and the AI `complete()` refactor.

<a id="find-and-replace-inside-the-composer"></a>

### Find and replace inside the composer

*Roadmap: P2 · Notion-style editor · new*

- **Why:** Editing long messages and templates ("replace Q3 with Q4") needs it, and Mod+F in the composer should search the draft, not the mailbox.
- **Competitors:** Notion, Google Docs and Outlook all have it. Tiptap's MIT Simple Editor includes a search-and-replace component.
- **Spec:** the Tiptap Search and Replace UI component (MIT) and its extension.
- **Alice today:** none.
- **Plan:** add the component behind Mod+F and Mod+Alt+F while the composer is focused, with match case, whole word and regex options.
- **Pitfalls:** the matches must not include the collapsed quoted-history node unless it is expanded.
- **Done when:** replace-all changes all matches in one undo step.
- **Effort:** S.

<a id="editor-accessibility-and-ime-test-matrix"></a>

### Editor accessibility and IME test matrix

*Roadmap: P2 · Notion-style editor · new*

- **Why:** Block editors are where screen-reader and IME regressions hide. Mailspring needed fixes in 1.13.x, 1.19 and 1.24 after shipping.
- **Competitors:** Mailspring's 1.19 / 1.19.1 accessibility pass (landmarks, ARIA, focus traps, roving tabindex).
- **Spec:** WAI-ARIA APG patterns for menus, listboxes and toolbars; ProseMirror's accessibility notes.
- **Alice today:** accessibility basics in the app shell (ARIA labels, reduced motion).
- **Plan:**
  1. Give every menu ARIA roles and a keyboard path. Announce block transforms ("Turned into to-do") through a polite live region.
  2. Keep an IME checklist covering Japanese, Chinese Pinyin, Korean, macOS press-and-hold and dead keys, on WKWebView, WebKitGTK and WebView2, for every custom node view.
  3. Add Playwright or Tauri WebDriver smoke tests for keyboard-only block creation.
- **Pitfalls:** custom node views with `contenteditable=false` islands (chips, callout icons) are the usual trap for screen readers and IME.
- **Done when:** the checklist is recorded in `docs/` and passes, and the keyboard-only test passes in CI.
- **Effort:** M. Run it before the editor ships to all users.

## P3: Notion-style editor extras

<a id="column-layouts-that-send-as-tables"></a>

### Column layouts that send as tables

*Roadmap: P3 · new*

- **Why:** Side-by-side content (two options, before/after) is common in newsletters and proposals.
- **Competitors:** Notion columns; Tiptap has no MIT column extension, so it needs a custom node.
- **Spec:** custom `columns` / `column` nodes.
- **Alice today:** none.
- **Plan:** 2–3 equal columns via the slash menu; the renderer emits a `role="presentation"` table with `valign=top` cells. Media-query support varies across mail apps, so columns may stay side by side on phones; keep them narrow-content friendly.
- **Pitfalls:** keep columns flat (no columns inside columns) and cap the count at three.
- **Done when:** columns render side by side in Gmail web and Outlook.
- **Effort:** M.

<a id="link-bookmark-cards"></a>

### Link bookmark cards

*Roadmap: P3 · new*

- **Why:** Pasting a URL and getting a titled card is a Notion and Slack habit, and it reads better than a bare link.
- **Competitors:** Notion bookmarks; Spark and Superhuman show link previews only when reading.
- **Spec:** OpenGraph tags (`og:title`, `og:description`, `og:image`).
- **Alice today:** none.
- **Plan:**
  1. Paste a lone URL, then choose "Turn into card" from a small menu.
  2. The daemon fetches metadata without cookies and with a short timeout. It caps the response size and follows at most 3 redirects.
  3. The card renders as a bordered table with title, description and link. The preview image is inlined as `cid:` only if the user keeps it.
- **Pitfalls:**
  - Privacy: fetching reveals the user's IP to the site. Make it an explicit action, not automatic.
  - SSRF: block private IP ranges in the daemon fetcher.
- **Done when:** a pasted URL becomes a card in the sent mail, and fetching `http://127.0.0.1` is refused.
- **Effort:** M.

<a id="math-blocks-rendered-to-images-for-sending"></a>

### Math blocks rendered to images for sending

*Roadmap: P3 · new*

- **Why:** Academics and engineers write formulas. LaTeX source in plain mail is hard to read.
- **Competitors:** Notion's equation blocks; Thunderbird has an add-on.
- **Spec:** `@tiptap/extension-mathematics` (MIT, with `katex` as a peer dependency).
- **Alice today:** none.
- **Plan:**
  1. Add inline and block math, with KaTeX loaded lazily.
  2. At save, render each formula to a PNG at 2× scale (KaTeX HTML → canvas). Attach it as `cid:` with the LaTeX in `alt`.
  3. Plain text gets `$…$`.
- **Pitfalls:** don't send SVG; caniemail shows only partial support in Gmail. The KaTeX fonts must be bundled locally, never loaded from a CDN.
- **Done when:** a formula is visible in Gmail web and in plain text as LaTeX.
- **Effort:** M.

<a id="table-of-contents-for-long-messages"></a>

### Table of contents for long messages

*Roadmap: P3 · new*

- **Why:** Long updates and newsletters benefit from an outline while editing; Notion's TOC block shows it in the page.
- **Competitors:** Notion TOC block; Tiptap's MIT `@tiptap/extension-table-of-contents`.
- **Spec:** the TableOfContents extension (`onUpdate` gives anchors).
- **Alice today:** none.
- **Plan:**
  1. Add an outline sidebar in the pop-out composer, built from headings.
  2. An optional `/toc` block renders as a plain list of heading names. Don't use anchor links: Gmail and Outlook handle in-message anchors inconsistently.
- **Pitfalls:** none significant.
- **Done when:** the outline updates as headings change.
- **Effort:** S.

<a id="focus-mode-and-a-page-style-composer"></a>

### Focus mode and a page-style composer

*Roadmap: P3 · new*

- **Why:** Writing a long mail in a full-height, centered page (subject as the page title) is the Notion feel. A small reply box isn't.
- **Competitors:** Notion pages; iA Writer focus mode; Superhuman's full-screen compose.
- **Spec:** none.
- **Alice today:** a compose `<dialog>` (the reading-and-accounts research notes it).
- **Plan:**
  1. An "Expand" toggle in the pop-out composer gives a centered column about 720 px wide with the subject styled as an H1-sized title, and recipients collapsed to one line until focused.
  2. Optional typewriter scrolling and dimming of non-current blocks (the Focus extension).
- **Pitfalls:** the subject styled as a title must remain the real `Subject`, not a heading in the body.
- **Done when:** the toggle persists per user, and the subject edits the `Subject` header.
- **Effort:** S–M. Depends on the pop-out composer window.

<a id="local-draft-history-snapshots"></a>

### Local draft history snapshots

*Roadmap: P3 · new*

- **Why:** "I deleted the paragraph I needed" is recoverable in Notion. Undo is lost when the window closes.
- **Competitors:** Notion and Google Docs page history; Tiptap's version history is a paid Cloud feature.
- **Spec:** none; snapshots are stored locally.
- **Alice today:** autosave of the latest draft and a localStorage backup.
- **Plan:**
  1. Keep the last 20 snapshots of `doc` per draft, taken at most once a minute while typing, in the daemon store.
  2. Add a "Version history" panel with restore.
  3. Delete snapshots when the draft is sent or discarded.
- **Pitfalls:** storage growth. Cap the snapshot bytes per draft.
- **Done when:** restoring a snapshot from 5 minutes ago brings back deleted text, and sending clears the snapshots.
- **Effort:** S–M.

<a id="paste-markdown-as-blocks-and-copy-as-markdown"></a>

### Paste Markdown as blocks and copy as Markdown

*Roadmap: P3 · new*

- **Why:** Many people draft in Markdown (notes apps, ChatGPT, GitHub). Pasting should produce structured blocks, not literal `**`.
- **Competitors:** Notion converts pasted Markdown; Bear and Obsidian do too.
- **Spec:** `@tiptap/markdown` (parse via `marked`, serialize to Markdown).
- **Alice today:** none.
- **Plan:**
  1. When the clipboard has only `text/plain` and it looks like Markdown (headings, lists, fences), parse it into blocks. Mod+Shift+V pastes it literally instead.
  2. "Copy as Markdown" in the block menu and for the whole draft.
- **Pitfalls:** false positives on plain text that starts with `-` or `#`. Require two or more Markdown signals before converting.
- **Done when:** pasting a README section yields headings, lists and code blocks, and Mod+Shift+V keeps it literal.
- **Effort:** S.
