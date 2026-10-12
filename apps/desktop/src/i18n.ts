import { createElement, Fragment, type ReactNode, useSyncExternalStore } from 'react';
import common from './locales/en-US/common.json';
import app from './locales/en-US/app.json';
import settings from './locales/en-US/settings.json';
import compose from './locales/en-US/compose.json';
import invite from './locales/en-US/invite.json';

export const languages = [{ tag: 'en-US', name: 'English (US)' }, { tag: 'de', name: 'Deutsch' }] as const;
export type Locale = typeof languages[number]['tag'];
export type LanguagePreference = 'system' | Locale;
type IsPlural<T> = T extends { other: string } ? Exclude<keyof T, Intl.LDMLPluralRule> extends never ? true : false : false;
type Paths<T> = T extends string ? never : {
  [K in keyof T & string]: T[K] extends string ? K : IsPlural<T[K]> extends true ? K : `${K}.${Paths<T[K]>}`
}[keyof T & string];
export type Key = Paths<{ common: typeof common; app: typeof app; settings: typeof settings; compose: typeof compose; invite: typeof invite }>;
type Plural = Partial<Record<Intl.LDMLPluralRule, string>> & { other: string };
type Entry = string | Plural;
type Catalog = Record<string, Entry>;
const categories = new Set(['zero', 'one', 'two', 'few', 'many', 'other']);
const catalogs = new Map<Locale, Catalog>();
function flatten(value: unknown, prefix: string, target: Catalog) {
  if (typeof value === 'string') { target[prefix] = value; return; }
  if (!value || typeof value !== 'object') return;
  const entries = Object.entries(value);
  if (entries.some(([key]) => key === 'other') && entries.every(([key, text]) => categories.has(key) && typeof text === 'string')) {
    target[prefix] = value as Plural;
  } else for (const [key, child] of entries) flatten(child, `${prefix}.${key}`, target);
}
const modules = import.meta.glob<unknown>('./locales/*/*.json', { eager: true, import: 'default' });
for (const [path, data] of Object.entries(modules)) {
  const match = /\/locales\/([^/]+)\/([^/]+)\.json$/.exec(path);
  if (!match || !languages.some(l => l.tag === match[1])) continue;
  const tag = match[1] as Locale;
  let catalog = catalogs.get(tag);
  if (!catalog) { catalog = {}; catalogs.set(tag, catalog); }
  flatten(data, match[2], catalog);
}
export function matchLocale(names: readonly string[]): Locale {
  for (const name of names) {
    const base = name.split(/[-_.@]/)[0].toLowerCase();
    const supported = languages.find(l => l.tag.split('-')[0] === base);
    if (supported) return supported.tag;
  }
  return 'en-US';
}
let systemLocales: readonly string[] = typeof navigator === 'undefined' ? ['en-US'] : navigator.languages;
function cachedLanguage(): LanguagePreference {
  try {
    const saved = localStorage.getItem('alice.language');
    if (saved === 'system' || languages.some(l => l.tag === saved)) return saved as LanguagePreference;
  } catch { /* Storage is optional (private windows and restricted WebViews). */ }
  return 'system';
}
let preference = cachedLanguage();
let current = preference === 'system' ? matchLocale(systemLocales) : preference;
const subscribers = new Set<() => void>();
const subscribe = (callback: () => void) => { subscribers.add(callback); return () => { subscribers.delete(callback); }; };
export function locale(): Locale { return current; }
export function useLocale(): Locale { return useSyncExternalStore(subscribe, locale); }
function updateLocale() {
  const next = preference === 'system' ? matchLocale(systemLocales) : preference;
  if (typeof document !== 'undefined') document.documentElement.lang = next;
  if (next === current) return;
  current = next;
  for (const callback of subscribers) callback();
}
export function setSystemLocales(names: readonly string[]) { systemLocales = names; updateLocale(); }
/** Apply the persisted preference before publishing a new daemon snapshot. No component is remounted. */
export function applyLanguage(value: LanguagePreference) {
  if (value === preference) return;
  preference = value;
  try { localStorage.setItem('alice.language', value); } catch { /* See cachedLanguage. */ }
  updateLocale();
}
const numberFormats = new Map<Locale, Intl.NumberFormat>();
const pluralRules = new Map<Locale, Intl.PluralRules>();
const listFormats = new Map<Locale, Intl.ListFormat>();
const dateFormats = new Map<string, Intl.DateTimeFormat>();
export function number(value: number): string {
  let formatter = numberFormats.get(current);
  if (!formatter) { formatter = new Intl.NumberFormat(current); numberFormats.set(current, formatter); }
  return formatter.format(value);
}
export function dateFormat(options?: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const key = `${current}:${JSON.stringify(options ?? {})}`;
  let formatter = dateFormats.get(key);
  if (!formatter) { formatter = new Intl.DateTimeFormat(current, options); dateFormats.set(key, formatter); }
  return formatter;
}
export function date(value: Date | string, options?: Intl.DateTimeFormatOptions): string { return dateFormat(options).format(typeof value === 'string' ? new Date(value) : value); }
export function list(values: string[]): string {
  let formatter = listFormats.get(current);
  if (!formatter) { formatter = new Intl.ListFormat(current, { style: 'long', type: 'conjunction' }); listFormats.set(current, formatter); }
  return formatter.format(values);
}
type Params = Record<string, string | number>;
function pattern(key: Key, count?: number): string {
  let foundLocale = current;
  let entry = catalogs.get(current)?.[key];
  if (!entry) { foundLocale = 'en-US'; entry = catalogs.get('en-US')?.[key]; }
  if (!entry) return key;
  if (typeof entry === 'string') return entry;
  let rules = pluralRules.get(foundLocale);
  if (!rules) { rules = new Intl.PluralRules(foundLocale); pluralRules.set(foundLocale, rules); }
  return entry[rules.select(count ?? 0)] ?? entry.other;
}
export function t(key: Key, params: Params = {}): string {
  return pattern(key, typeof params.count === 'number' ? params.count : undefined).replace(/\{([A-Za-z0-9_]+)\}/g, (placeholder: string, name: string) => {
    const value = params[name];
    return value === undefined ? placeholder : typeof value === 'number' ? number(value) : value;
  });
}
/** Preserve semantic markup without embedding HTML in catalogs. */
export function rich(key: Key, nodes: Record<string, ReactNode>): ReactNode {
  const occurrences = new Map<string, number>();
  return pattern(key, typeof nodes.count === 'number' ? nodes.count : undefined).split(/(\{[A-Za-z0-9_]+\})/g).map(part => {
    const name = /^\{(.+)\}$/.exec(part)?.[1];
    const value = name && name in nodes ? nodes[name] : part;
    const identity = name ? `slot:${name}` : `text:${part}`;
    const occurrence = occurrences.get(identity) ?? 0;
    occurrences.set(identity, occurrence + 1);
    return createElement(Fragment, { key: `${identity}:${occurrence}` }, typeof value === 'number' ? number(value) : value);
  });
}
updateLocale();
