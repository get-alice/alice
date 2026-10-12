/*
 * Every keyboard shortcut in the mail view, defined once: the same entry matches key presses, labels
 * menu items and tooltips, and fills Settings → Keyboard shortcuts.
 *
 * A key is a `KeyboardEvent.key` value with optional `Mod+` (⌘ on macOS, Ctrl elsewhere) and `Shift+`
 * prefixes. Letters and named keys match Shift exactly; symbols ignore it, as layouts put `#` and `?`
 * on different shifted keys.
 */
import { t, type Key } from './i18n';
import { isMac } from './platform';

export interface Shortcut {
  /** Alternatives; the first is shown in menus and tooltips. */
  keys: readonly string[];
  description: string;
}

const shortcut = (description: Key, ...keys: string[]): Shortcut => ({ keys, get description() { return t(description); } });

export const shortcuts = {
  compose: shortcut('app.shortcut0', 'Mod+N'),
  search: shortcut('app.shortcut1', 'Mod+K', '/'),
  sync: shortcut('app.shortcut2', 'Mod+Shift+N'),
  assistant: shortcut('app.shortcut3', 'Mod+J'),
  settings: shortcut('app.shortcut4', 'Mod+,'),
  help: shortcut('app.shortcut5', '?'),
  menu: shortcut('app.shortcut6', 'Shift+F10', 'ContextMenu'),
  next: shortcut('app.shortcut7', 'j', 'ArrowDown'),
  previous: shortcut('app.shortcut8', 'k', 'ArrowUp'),
  open: shortcut('app.shortcut9', 'Enter', 'o'),
  reply: shortcut('app.shortcut10', 'r'),
  archive: shortcut('app.shortcut11', 'e'),
  trash: shortcut('app.shortcut12', '#', 'Delete', 'Backspace'),
  inbox: shortcut('app.shortcut13', 'i'),
  read: shortcut('app.shortcut14', 'u'),
  star: shortcut('app.shortcut15', 's'),
  back: shortcut('app.shortcut16', 'Escape'),
  send: shortcut('app.shortcut17', 'Mod+Enter'),
  saveDraft: shortcut('app.shortcut18', 'Mod+S'),
  closeCompose: shortcut('app.shortcut19', 'Escape'),
};

/** Folders in sidebar order, each a ⌘/Ctrl digit away. */
export const goTo = {
  inbox: shortcut('app.shortcut20', 'Mod+1'), starred: shortcut('app.shortcut21', 'Mod+2'), sent: shortcut('app.shortcut22', 'Mod+3'),
  drafts: shortcut('app.shortcut23', 'Mod+4'), archive: shortcut('app.shortcut24', 'Mod+5'), trash: shortcut('app.shortcut25', 'Mod+6'),
};

function parse(key: string) {
  const parts = key.split('+');
  // `Mod+,` and similar: the key itself is never empty, so a trailing `+` would be the key.
  const name = parts.pop() || '+';
  return { name, mod: parts.includes('Mod'), shift: parts.includes('Shift') };
}

export function matches(event: KeyboardEvent, shortcut: Shortcut) {
  if (event.altKey) return false;
  return shortcut.keys.some(key => {
    const { name, mod, shift } = parse(key);
    if ((event.metaKey || event.ctrlKey) !== mod) return false;
    if (/^[a-z]$/i.test(name)) return event.key.toLowerCase() === name.toLowerCase() && event.shiftKey === shift;
    if (name.length === 1) return event.key === name;
    return event.key === name && event.shiftKey === shift;
  });
}

const names: Record<string, string> = isMac
  ? { ArrowDown: '↓', ArrowUp: '↑', Enter: '↩', Escape: 'app.keyEscape', Delete: '⌦', Backspace: '⌫', ContextMenu: 'app.keyMenu' }
  : { ArrowDown: '↓', ArrowUp: '↑', Escape: 'app.keyEscape', Delete: 'app.keyDelete', ContextMenu: 'app.keyMenu' };

/** One key as people read it: `⌘N` on macOS, `Ctrl+N` elsewhere. */
export function keyLabel(key: string) {
  const { name, mod, shift } = parse(key);
  const named = names[name];
  const shown = named?.startsWith('app.') ? t(named as Key) : named || (name === 'Enter' ? t('app.keyEnter') : name === 'Backspace' ? t('app.keyBackspace') : name.length === 1 ? name.toUpperCase() : name);
  if (isMac) return `${shift ? '⇧' : ''}${mod ? '⌘' : ''}${shown}`;
  return [mod && t('app.keyCtrl'), shift && t('app.keyShift'), shown].filter(Boolean).join('+');
}

/** The primary key, for menus and tooltips. */
export function label(shortcut: Shortcut) {
  return keyLabel(shortcut.keys[0]);
}

/** A tooltip naming the control and its shortcut: `Archive · E`. */
export function hint(text: string, shortcut: Shortcut) {
  return `${text} · ${label(shortcut)}`;
}

/** The `aria-keyshortcuts` value, which names modifiers as the platform reports them. */
export function ariaKeys(shortcut: Shortcut) {
  return shortcut.keys.map(key => {
    const { name, mod, shift } = parse(key);
    return [mod && (isMac ? 'Meta' : 'Control'), shift && 'Shift', name.length === 1 ? name.toUpperCase() : name].filter(Boolean).join('+');
  }).join(' ');
}
