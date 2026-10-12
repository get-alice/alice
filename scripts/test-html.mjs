import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
import { JSDOM } from 'jsdom';
import createDOMPurify from 'dompurify';

const { window } = new JSDOM('', { runScripts: 'outside-only' });
try {
  const source = readFileSync('apps/desktop/src/emailHtml.ts', 'utf8');
  const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } });
  const exports = {};
  window.eval(`(function(require,exports){${compiled.outputText}\n})`)(name => {
    assert.equal(name, 'dompurify'); return { default: createDOMPurify(window) };
  }, exports);
  const fixture = `<html><head><style>.cta{background:#036be8;color:white;padding:12px}</style><meta http-equiv="refresh" content="0;url=https://tracker.invalid"></head><body>
    <table><tr><td><a class="cta" href="https://example.test/jobs?utm_source=email&amp;id=1">Browse Jobs</a></td></tr></table>
    <img alt="Logo" src="https://tracker.invalid/logo.png" srcset="https://tracker.invalid/other.png 2x" onerror="alert(1)">
    <img alt="Inline" src="data:image/png;base64,iVBORw0KGgo=">
    <img alt="Unsafe" src="data:image/svg+xml,<svg onload='alert(1)'></svg>">
    <script>parent.document.body.innerHTML='compromised'</script><iframe src="https://tracker.invalid"></iframe>
    <form action="https://tracker.invalid"><input autofocus name="secret"></form>
    <a href="javascript:alert(1)" onclick="alert(1)">Unsafe link</a><a href="file:///etc/passwd">Local file</a>
    <a href="https://user:password@example.test">Credentials</a><svg onload="alert(1)"></svg></body></html>`;
  const parse = html => new JSDOM(html).window.document;
  const blockedResult = exports.emailDocument(fixture, false, false);
  // Only the https image can load; inline data images never need asking.
  assert.equal(blockedResult.remoteImages, 1);
  assert.equal(exports.emailDocument('<p>No pictures <img src="data:image/png;base64,iVBORw0KGgo="></p>', false, false).remoteImages, 0);
  const blocked = parse(blockedResult.document);
  assert.equal(blocked.querySelector('.cta').textContent, 'Browse Jobs');
  assert.equal(blocked.querySelector('.cta').getAttribute('href'), 'https://example.test/jobs?utm_source=email&id=1');
  assert.match(blocked.querySelectorAll('style')[1].textContent, /background:#036be8/);
  assert.equal(blocked.querySelectorAll('table td').length, 1);
  assert.equal(blocked.querySelector('img[alt="Logo"]').hasAttribute('src'), false);
  assert.match(blocked.querySelector('img[alt="Inline"]').getAttribute('src'), /^data:image\/png/);
  assert.equal(blocked.querySelector('img[alt="Unsafe"]').hasAttribute('src'), false);
  assert.equal(blocked.querySelectorAll('script,iframe,form,input,svg,[onerror],[onclick],[srcset],meta[http-equiv="refresh"]').length, 0);
  assert.equal(blocked.querySelectorAll('a[href]').length, 1);
  const policy = blocked.querySelector('meta[http-equiv="Content-Security-Policy"]').content;
  assert.match(policy, /script-src 'none'/); assert.match(policy, /img-src data:;/);
  const allowed = parse(exports.emailDocument(fixture, true, false).document);
  assert.equal(allowed.querySelector('img[alt="Logo"]').getAttribute('src'), 'https://tracker.invalid/logo.png');
  assert.match(allowed.querySelector('meta[http-equiv="Content-Security-Policy"]').content, /img-src data: https:;/);
  assert.equal(blockedResult.quoted, false);
  // The declared size, not the file's own proportion, shapes an image: a 396×1 divider cut from a square tile stays a
  // line under the frame's `img{height:auto}`, while the sender's inline style still wins. Partial sizes add nothing.
  const sized = parse(exports.emailDocument('<img id="line" src="https://x.test/tile.png" width="396" height="1"><img id="styled" width="87" height="60" style="height:60px"><img id="wide" width="100%" height="20"><img id="free" width="40">', true, false).document);
  assert.equal(sized.getElementById('line').getAttribute('style'), 'aspect-ratio:396/1;');
  assert.equal(sized.getElementById('styled').getAttribute('style'), 'aspect-ratio:87/60;height:60px');
  assert.equal(sized.getElementById('wide').hasAttribute('style'), false);
  assert.equal(sized.getElementById('free').hasAttribute('style'), false);

  // Replies fold the history they quote: Gmail's wrapper, Apple Mail's attribution and cite, Outlook's header block.
  const gmail = '<div dir="ltr">Sounds good, talk next week.<div class="gmail_signature">Joana</div></div><br><div class="gmail_quote"><div class="gmail_attr">On Thu, Sep 24, 2026 at 3:34 PM Amruth &lt;a@x.test&gt; wrote:<br></div><blockquote class="gmail_quote">Earlier <img src="https://tracker.invalid/old.png"></blockquote></div>';
  const folded = exports.emailDocument(gmail, false, false);
  assert.equal(folded.quoted, true);
  assert.equal(folded.remoteImages, 0, 'Images in folded history are not offered for loading');
  const foldedBody = parse(folded.document).body.textContent;
  assert.match(foldedBody, /Sounds good, talk next week\.\s*Joana/);
  assert.doesNotMatch(foldedBody, /wrote:|Earlier/);
  const unfolded = exports.emailDocument(gmail, false, true);
  assert.equal(unfolded.quoted, true);
  assert.match(parse(unfolded.document).body.textContent, /wrote:\s*Earlier/);
  const apple = '<div>Works for me.</div><div><br><div>On Sep 24, 2026, at 15:34, Ana &lt;ana@x.test&gt; wrote:</div><br><blockquote type="cite"><div>Shall we meet?</div></blockquote></div>';
  assert.equal(parse(exports.emailDocument(apple, false, false).document).body.textContent.trim(), 'Works for me.');
  const outlook = '<div>See attached.</div><hr><div id="divRplyFwdMsg"><b>From:</b> Ana<br><b>Sent:</b> Monday</div><div>Original text</div>';
  assert.equal(parse(exports.emailDocument(outlook, false, false).document).body.textContent.trim(), 'See attached.');
  // A forward with nothing of its own stays whole.
  const forward = '<div class="gmail_quote">---------- Forwarded message ---------<br>Body</div>';
  assert.equal(exports.emailDocument(forward, false, false).quoted, false);
  assert.match(parse(exports.emailDocument(forward, false, false).document).body.textContent, /Body/);

  const plain = exports.splitQuoted('Sounds good!\n\nBest,\nJoana\n\nOn Thu, Sep 24, 2026 at 3:34 PM Amruth Pillai <im.amruth@example.test>\nwrote:\n\n> Earlier message\n> > Older still');
  assert.equal(plain.main, 'Sounds good!\n\nBest,\nJoana');
  assert.equal(plain.quoted, true);
  // Results come from the JSDOM realm, so compare them by value.
  const split = body => ({ ...exports.splitQuoted(body) });
  assert.deepEqual(split('Thanks!\n\n-----Original Message-----\nFrom: Ana'), { main: 'Thanks!', quoted: true });
  assert.deepEqual(split('Inline reply\n\n> quoted line\n> another\n'), { main: 'Inline reply', quoted: true });
  assert.deepEqual(split('> only a quote'), { main: '> only a quote', quoted: false });
  assert.deepEqual(split('No history here.\nOn Monday we ship.'), { main: 'No history here.\nOn Monday we ship.', quoted: false });
  console.log('HTML reader: layout and link labels preserved; active content and unsafe links removed; images require opt-in and are counted; restrictive frame policy passed; quoted history folds in HTML and plain text replies');
} finally { window.close(); }
