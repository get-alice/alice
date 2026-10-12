import DOMPurify from 'dompurify';

// Elements mail clients wrap around the earlier messages a reply quotes (Gmail, Apple Mail, Thunderbird,
// Outlook, Yahoo, Proton).
const quoteMarkers = '.gmail_quote, .gmail_quote_container, blockquote[type="cite"], .moz-cite-prefix, #divRplyFwdMsg, #appendonsend, [id^="mail-editor-reference-message-container"], .yahoo_quoted, .protonmail_quote, #reply-intro';
const attribution = /^(?:On\s[\s\S]{4,300}\bwrote:|Am\s[\s\S]{4,300}\bschrieb(?:\s[^\n]*)?:)$/i;

/** Where the quoted history of an HTML reply begins, if it has one. */
function quoteStart(root: DocumentFragment) {
  let start = root.querySelector(quoteMarkers);
  // An attribution line ("On …, Ana wrote:") outside the marker, or with no marker at all, starts the quote too.
  for (const element of root.querySelectorAll('div, p')) {
    if (start && !(start.compareDocumentPosition(element) & Node.DOCUMENT_POSITION_PRECEDING)) break;
    if (element.textContent && attribution.test(element.textContent.trim()) && !element.querySelector('div, p, blockquote')) { start = element; break; }
  }
  return start;
}

/** Removes `start` and everything after it in document order. */
function cutFrom(start: Element, root: DocumentFragment) {
  const doomed: Node[] = [start];
  for (let node: Node | null = start; node && node !== root; node = node.parentNode) {
    for (let sibling = node.nextSibling; sibling; sibling = sibling.nextSibling) doomed.push(sibling);
  }
  for (const node of doomed) node.parentNode?.removeChild(node);
}

/**
 * The original HTML in a sandboxed frame document. Remote images load only when `loadImages` is set;
 * `remoteImages` counts the shown ones that could, so the reader only asks when there is something to load.
 * `quoted` reports whether the email ends with earlier messages; they are left out unless `showQuoted`.
 */
export function emailDocument(html: string, loadImages: boolean, showQuoted: boolean) {
  // Template contents are inert, including images, until sanitized and placed in the frame.
  const template = document.createElement('template');
  template.innerHTML = html;
  const start = quoteStart(template.content);
  let quoted = false;
  if (start) {
    const reply = document.createElement('template');
    reply.innerHTML = html;
    const replyStart = quoteStart(reply.content);
    if (replyStart) cutFrom(replyStart, reply.content);
    // A forward or a bare quote has nothing of its own to show; keep it whole.
    quoted = !!reply.content.textContent?.trim() || !!reply.content.querySelector('img');
    if (quoted && !showQuoted) template.content.replaceChildren(reply.content);
  }
  const clean = DOMPurify.sanitize(template.content, {
    USE_PROFILES: { html: true }, RETURN_DOM_FRAGMENT: true, ADD_TAGS: ['style'],
    FORBID_TAGS: ['script', 'iframe', 'object', 'embed', 'form', 'input', 'textarea', 'select', 'button', 'meta', 'link', 'base', 'audio', 'video'],
    FORBID_ATTR: ['srcset', 'background', 'ping', 'autofocus'],
  });
  for (const anchor of clean.querySelectorAll('a')) {
    const href = anchor.getAttribute('href') || '';
    try {
      const url = new URL(href);
      if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new Error();
      anchor.setAttribute('rel', 'noopener noreferrer');
      anchor.setAttribute('title', url.href);
    } catch { anchor.removeAttribute('href'); }
  }
  let remoteImages = 0;
  for (const image of clean.querySelectorAll('img')) {
    const src = image.getAttribute('src') || '';
    const inline = /^data:image\/(png|jpeg|gif|webp);base64,/i.test(src);
    let remote = false;
    try { const url = new URL(src); remote = url.protocol === 'https:' && !url.username && !url.password; } catch { /* Inline or unsupported image. */ }
    if (remote) remoteImages++;
    if (!inline && !(remote && loadImages)) image.removeAttribute('src');
  }
  const container = document.createElement('div');
  container.append(clean);
  const policy = `default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data:${loadImages ? ' https:' : ''}; form-action 'none'; base-uri 'none'`;
  return { remoteImages, quoted, document: `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="${policy}"><meta name="referrer" content="no-referrer"><style>html,body{height:auto!important;min-height:0!important}body{margin:0;font:14px/1.6 Arial,sans-serif;color:#222;overflow-wrap:anywhere}img,table{max-width:100%}img{height:auto}a:focus-visible{outline:2px solid #68518e;outline-offset:3px}</style></head><body>${container.innerHTML}</body></html>` };
}

// "On Thu, Sep 24, 2026 at 3:34 PM Ana <ana@…> wrote:" (often wrapped), Outlook's original-message
// separators and header blocks.
const plainAttribution = /^[ \t]*(?:(?:On\s[^\n]*(?:\n[^\n]*){0,2}?\bwrote:|Am\s[^\n]*(?:\n[^\n]*){0,2}?\bschrieb(?:\s[^\n]*)?:)[ \t]*$|-{2,}\s*(?:Original Message|Ursprüngliche Nachricht|Originalnachricht)\s*-{2,}|_{10,}[ \t]*\n[ \t]*(?:From|Von):|(?:From|Von):[ \t][^\n]+\n(?:[^\n]*\n){0,3}?[ \t]*(?:Sent|Date|Gesendet|Datum):[ \t])/im;

/** Splits a plain-text reply into what the sender wrote and the earlier messages it quotes. */
export function splitQuoted(body: string) {
  let index = body.search(plainAttribution);
  // A trailing run of "> " lines quotes earlier mail even without an attribution.
  const lines = body.split('\n');
  let first = lines.length, quotes = false;
  while (first > 0 && (!lines[first - 1].trim() || lines[first - 1].startsWith('>'))) { first--; quotes ||= lines[first].startsWith('>'); }
  if (quotes) {
    const offset = lines.slice(0, first).reduce((sum, line) => sum + line.length + 1, 0);
    if (index < 0 || offset < index) index = offset;
  }
  const main = index < 0 ? body : body.slice(0, index).trimEnd();
  return main.trim() ? { main, quoted: index >= 0 } : { main: body, quoted: false };
}
