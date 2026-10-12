import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import ts from 'typescript';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

mkdirSync('target', { recursive: true });
const directory = mkdtempSync(resolve('target/reader-test-'));
try {
  const source = readFileSync('apps/desktop/src/MessageBody.tsx', 'utf8');
  const compiled = ts.transpileModule(source, { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  const path = join(directory, 'reader.mjs');
  writeFileSync(path, compiled.outputText);
  const { default: MessageBody } = await import(pathToFileURL(path).href);
  const render = body => renderToStaticMarkup(createElement(MessageBody, { body, onOpen() {} }));
  const html = render('Sent from Mailspring (https://example.test/link?redirect=https%3A%2F%2Fexample.org%2F&recipient=fixture), for work.\n\nSee https://example.org/wiki/Email_(software).');
  assert.equal((html.match(/<a /g) || []).length, 2, 'Email URLs must render as links');
  assert.match(html, /href="https:\/\/example.test\/link\?redirect=https%3A%2F%2Fexample.org%2F&amp;recipient=fixture"/);
  assert.match(html, /href="https:\/\/example.org\/wiki\/Email_\(software\)"/);
  assert.match(html, /rel="noopener noreferrer"/);
  assert.equal((html.match(/<p>/g) || []).length, 2);
  const unsafe = render('<script>alert(1)</script> javascript:alert(1) file:///etc/passwd https://user:password@example.org');
  assert.doesNotMatch(unsafe, /<a |<script>/);
  assert.match(unsafe, /&lt;script&gt;/);
  console.log('Reader: clickable URLs, query strings, punctuation, paragraph layout and inert unsafe content passed');
} finally { rmSync(directory, { recursive: true, force: true }); }
