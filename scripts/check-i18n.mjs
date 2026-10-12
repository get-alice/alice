import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

// A translation must preserve the source message's arguments, and every supported language must
// cover every message. Treat plural objects as one entry, not separate dotted keys.
const roots = ['crates/alice-core/locales', 'apps/desktop/src/locales'];
const categories = new Set(['zero', 'one', 'two', 'few', 'many', 'other']);
const failures = [];
const fail = message => failures.push(message);
const placeholders = text => [...new Set([...text.matchAll(/\{([A-Za-z0-9_]+)\}/g)].map(match => match[1]))].sort().join(',');
function flatten(value, prefix, output, file) {
  if (typeof value === 'string') {
    if (!value.trim()) fail(`${file}: ${prefix} is empty`);
    output.set(prefix, value);
    return;
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    fail(`${file}: ${prefix} must be text or an object`);
    return;
  }
  const entries = Object.entries(value);
  if ('other' in value && entries.every(([key]) => categories.has(key))) {
    for (const [category, text] of entries) {
      if (typeof text !== 'string' || !text.trim()) fail(`${file}: ${prefix}.${category} must be nonempty text`);
    }
    output.set(prefix, value);
    return;
  }
  for (const [key, child] of entries) flatten(child, `${prefix}.${key}`, output, file);
}
function load(root, tag) {
  const catalog = new Map();
  for (const filename of readdirSync(join(root, tag)).filter(name => name.endsWith('.json')).sort()) {
    const file = join(root, tag, filename);
    flatten(JSON.parse(readFileSync(file, 'utf8')), filename.slice(0, -5), catalog, file);
  }
  return catalog;
}
const loaded = new Map();
for (const root of roots) {
  const tags = readdirSync(root, { withFileTypes: true }).filter(entry => entry.isDirectory()).map(entry => entry.name).sort();
  const source = load(root, 'en-US');
  const locales = new Map();
  for (const tag of tags) {
    const catalog = load(root, tag);
    locales.set(tag, catalog);
    const plurals = new Intl.PluralRules(tag).resolvedOptions().pluralCategories;
    for (const key of source.keys()) if (!catalog.has(key)) fail(`${root}/${tag}: missing ${key}`);
    for (const [key, translated] of catalog) {
      if (!source.has(key)) { fail(`${root}/${tag}: unknown ${key}`); continue; }
      const original = source.get(key);
      if (typeof original === 'string' && typeof translated === 'string') {
        if (placeholders(original) !== placeholders(translated)) fail(`${root}/${tag}: ${key} placeholders differ`);
      } else if (typeof original === 'object' && typeof translated === 'object') {
        for (const category of plurals) if (!(category in translated)) fail(`${root}/${tag}: ${key} missing plural ${category}`);
        for (const [category, text] of Object.entries(translated)) {
          if (!plurals.includes(category)) fail(`${root}/${tag}: ${key} unexpected plural ${category}`);
          const reference = original[category] ?? original.other;
          if (typeof text === 'string' && typeof reference === 'string' && placeholders(text) !== placeholders(reference)) fail(`${root}/${tag}: ${key}.${category} placeholders differ`);
        }
      } else fail(`${root}/${tag}: ${key} must preserve text/plural kind`);
    }
  }
  loaded.set(root, locales);
  console.log(`${root}: ${source.size} keys, ${tags.join(', ')}`);
}
const rust = loaded.get(roots[0]);
const frontend = loaded.get(roots[1]);
if ([...rust.keys()].join(',') !== [...frontend.keys()].join(',')) fail('Rust and frontend must support the same language tags');
// Shared folder terminology must not drift between desktop and terminal.
for (const [tag, catalog] of rust) for (const [key, value] of catalog) {
  if (key.startsWith('common.') && JSON.stringify(value) !== JSON.stringify(frontend.get(tag)?.get(key))) fail(`${tag}: shared ${key} differs between Rust and frontend`);
}
function sourceFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap(entry => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return ['target', 'node_modules', 'gen'].includes(entry.name) ? [] : sourceFiles(path);
    return path.endsWith('.rs') ? [path] : [];
  });
}
const english = rust.get('en-US');
for (const dir of ['crates/alice-core/src', 'crates/alice/src', 'apps/desktop/src-tauri/src']) {
  for (const file of sourceFiles(dir)) {
    const source = readFileSync(file, 'utf8').replace(/^\s*\/\/.*$/gm, '');
    // Rust macros have a literal key, with an optional first locale expression. Dynamic clap help
    // keys are validated when its Command tree is localized, not guessed from a source regexp.
    for (const match of source.matchAll(/\b(?:t|text|tr)!\s*\(\s*(?:[^";]*?\s*,\s*)?"([a-z][a-z0-9_.-]*)"/g)) {
      if (!english.has(match[1])) fail(`${file}: unknown translation key ${match[1]}`);
    }
  }
}
if (failures.length) {
  console.error(failures.join('\n'));
  process.exitCode = 1;
} else console.log('Catalog keys, placeholders, plural forms and shared terminology match.');
