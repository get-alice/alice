# Email reader follow-up reviews

The earlier whole-repository Ponytail audits and dual-agent Impeccable design/UX rounds are retained in `docs/reviews/2026-10-11/fixes/`. These follow-up passes cover the provider/store changes and reader introduced during live verification. The expected load remains one person using the desktop and CLI together.

## Code pass 1 — MIME, cache and trust boundaries

Traced the MIME importer through persistence, cache reuse, API serialization and desktop rendering. HTML is optional, and old cache rows remain readable. Old rows request a fresh body import once; explicit `null` keeps plain-text cache reuse. CLI/TUI rendering and AI context still use the plain text body.

Traced inert template parsing, DOMPurify sanitization, iframe isolation, document resource policy and external-link dispatch. Scripts, forms, embedded frames, event handlers and unsafe links are removed. Remote image attributes are blocked until opt-in. Frame sandbox privileges omit scripts, forms, popups and top navigation. Sender CSS stays in the email document. The sanitizer regression exercises both image states and preserves layout/link labels.

The MIME/cache tests failed against the prior implementation, then passed with the shared fixes. The sanitizer, reader, workspace and protocol checks pass. No additional correctness finding remained in these paths.

## UX pass 1 — browser interactions and layout

The isolated HTML fixture preserved its table, spacing and blue button link. A public image with no mailbox token loaded only after **Load images**; its natural width was 544 pixels. The frame resized from 340 to 417 pixels. Plain text/formatted switching retained the correct body, and clicking the fixture link opened a separate tab. Host content stayed unchanged after malicious email content was removed. The temporary tabs and fixture workspace were cleaned up.

A viewport-dependent CSS fixture exposed unbounded frame growth: `body { min-height: 100vh; padding: 20px }` grew from 460 to 10,660 pixels between observations. The reader now resets the email document's body height constraints and caps automatic frame sizing at 12,000 pixels. Longer emails retain an internal scrollbar.

## Code/UX pass 2 — confirmation

The viewport-dependent fixture settled at 380 pixels after the fix. A 20,000-pixel email produced a 12,000-pixel frame while retaining a 20,000-pixel scrollable body. The check asserted the frame cap. The source review confirmed observer/listener cleanup when changing format, loading images and leaving the message.

In the final packaged macOS app, the real Djinni newsletter displayed its original table layout, typography, blue **Browse Jobs** button and named links. **Test 123** displayed **Mailspring** as its link label. Both retained their plain text body. Remote image controls remained visible; no live tracking URL or remote mailbox image was followed. Private screenshots stay in ignored `target/platform-verification/` and are not published in this report.

The scoped Impeccable mechanical scans returned an empty finding list. HTML controls wrap with the existing reader layout, the formatted frame has an accessible title, links retain keyboard focus styling, and the plain text fallback is available. The reader's entry animation keeps text opaque.

**Verdict:** the reported link/HTML gaps and the reproduced frame growth are fixed. Evidence is in `evidence/html-browser.json`, the HTML/reader logs and the workspace test logs.

## MIME confirmation — complete content

The mixed multipart fixture contains an HTML alternative for its first section and an additional plain text section. Selecting only its HTML would hide the additional section in the default reader. The regression was extended to reject that partial HTML result and failed before the repair. Such messages now fall back to their complete plain text body. Normal multipart alternatives retain formatted HTML, and attachment contents remain excluded. The workspace suite passes with this preservation rule.

**Not checked:** native Windows execution, mobile platforms, CID attachment images, HTML compose, screen-reader speech and arbitrary newsletter compatibility. Outlook was skipped at the user's request. Sanitization uses maintained DOMPurify; it is not a claim that every possible hostile HTML payload has been manually tested.
