import { t } from './i18n';
import { createContext, use, useEffect, useEffectEvent, useLayoutEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent } from 'react';
import { motion, useIsPresent } from 'motion/react';
import { Copy, ExternalLink, type LucideIcon } from 'lucide-react';
import { ariaKeys, label, matches, type Shortcut } from './shortcuts';
import { easeOut, leave, travel } from './motion';

export interface MenuItem { label: string; icon?: LucideIcon; shortcut?: Shortcut; disabled?: boolean; run: () => void }
export type MenuEntry = MenuItem | 'separator';
/**
 * Where a menu opens. Keyboard-opened menus appear without animation and focus their first item;
 * `anchor` is the button that toggles the menu, so pressing it again closes rather than reopens it.
 */
export interface MenuRequest { x: number; y: number; keyboard: boolean; anchor?: Element }
/** An open menu. `key` names what it acts on, so that item can show it is the menu's target. */
export interface OpenMenu { id: number; request: MenuRequest; entries: MenuEntry[]; key: string; restore: HTMLElement | null }
export interface Menus {
  open: (request: MenuRequest | null, entries: MenuEntry[], key?: string) => void;
  /** Copies text and reports it, as `Copied sender address`. */
  copy: (text: string, what: string) => void;
}
export const MenuContext = createContext<Menus>({ open: () => {}, copy: () => {} });
export const useMenus = () => use(MenuContext);

/** Text fields keep the system's cut and paste, and selected text keeps Copy and Look Up. */
export function wantsSystemMenu(target: Element) {
  if (target.closest('input, textarea, select, [contenteditable]')) return true;
  const selection = target.ownerDocument.getSelection();
  return !!selection && !selection.isCollapsed && selection.toString().trim() !== '' && selection.containsNode(target, true);
}

let keyboardRequest = false;

/** A right-click as a menu request, or null to leave the system menu. A request stops the event, so the innermost menu wins. */
export function fromEvent(event: ReactMouseEvent | MouseEvent): MenuRequest | null {
  if (wantsSystemMenu(event.target as Element)) return null;
  event.preventDefault(); event.stopPropagation();
  return { x: event.clientX, y: event.clientY, keyboard: keyboardRequest };
}

/** A menu dropping from a button, opened from the keyboard when the click had no pointer. */
export function fromButton(button: HTMLElement, keyboard: boolean): MenuRequest {
  const rect = button.getBoundingClientRect();
  return { x: rect.left, y: rect.bottom + 6, keyboard, anchor: button };
}

/**
 * Opens the focused element's menu from the keyboard (Shift F10 or the Menu key) by sending it the
 * right-click a pointer would. A container names its current item with `data-menu-default`.
 */
export function openFocusedMenu() {
  const focused = document.activeElement;
  if (!(focused instanceof HTMLElement) || focused === document.body) return;
  const target = focused.querySelector<HTMLElement>('[data-menu-default]') ?? focused;
  const rect = target.getBoundingClientRect();
  keyboardRequest = true;
  try { target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: rect.left + 24, clientY: Math.min(rect.bottom, Math.max(rect.top, 0) + 40) })); }
  finally { keyboardRequest = false; }
}

/** Copies through the async clipboard, or a selection copy where the WebView lacks it. */
export async function copyText(text: string) {
  try { await navigator.clipboard.writeText(text); return; } catch { /* Fall back to copying a selection. */ }
  const focused = document.activeElement as HTMLElement | null;
  const field = Object.assign(document.createElement('textarea'), { value: text, readOnly: true });
  field.style.cssText = 'position:fixed;opacity:0;pointer-events:none';
  document.body.append(field); field.select();
  // Deprecated, but the only copy left when the WebView refuses the async clipboard.
  // eslint-disable-next-line @typescript-eslint/no-deprecated
  const copied = document.execCommand('copy');
  field.remove(); focused?.focus({ preventScroll: true });
  if (!copied) throw new Error(t('app.clipboardUnavailable'));
}

export function linkMenu(url: string, onOpen: (url: string) => void, menus: Menus): MenuEntry[] {
  return [{ label: t('app.openLink'), icon: ExternalLink, run: () => onOpen(url) }, { label: t('app.copyLink'), icon: Copy, run: () => menus.copy(url, t('app.linkAddress')) }];
}

/**
 * A menu at the pointer (or below a button), kept inside the window. Arrow keys, Home and End move
 * through it; an item's own shortcut also chooses it. Escape and Tab close it and return focus.
 */
export function ContextMenu({ menu, onClose }: { menu: OpenMenu; onClose: () => void }) {
  const element = useRef<HTMLDivElement>(null);
  const { x, y, keyboard, anchor } = menu.request;
  const [place, setPlace] = useState({ left: x, top: y, origin: 'top left' });
  // Separators only between groups: none leading, trailing or doubled.
  const entries = menu.entries.filter((entry, index, all) => entry !== 'separator' || (all[index - 1] !== 'separator' && all.slice(0, index).some(item => item !== 'separator') && all.slice(index + 1).some(item => item !== 'separator')));
  const close = (restore: boolean) => {
    onClose();
    if (restore && menu.restore?.isConnected) menu.restore.focus({ preventScroll: true });
  };
  const choose = (item: MenuItem) => { if (item.disabled) return; close(true); item.run(); };
  // A menu playing its exit must not close the one that replaced it.
  const present = useIsPresent();
  const dismiss = useEffectEvent(() => { if (present) onClose(); });
  useLayoutEffect(() => {
    const box = element.current;
    if (!box) return;
    // Opens toward the room it has, like a native menu: leftward near the right edge, upward near the bottom.
    const left = x + box.offsetWidth > innerWidth - 8, up = y + box.offsetHeight > innerHeight - 8;
    setPlace({ left: left ? Math.max(8, x - box.offsetWidth) : x, top: up ? Math.max(8, y - box.offsetHeight) : y, origin: `${up ? 'bottom' : 'top'} ${left ? 'right' : 'left'}` });
    (keyboard && box.querySelector<HTMLElement>('[role="menuitem"]:not(:disabled)') || box).focus({ preventScroll: true });
  }, [x, y, keyboard]);
  useEffect(() => {
    const outside = (event: PointerEvent) => { const target = event.target as Node; if (!element.current?.contains(target) && !anchor?.contains(target)) dismiss(); };
    const away = () => dismiss();
    window.addEventListener('pointerdown', outside, true);
    window.addEventListener('scroll', away, true);
    window.addEventListener('resize', away);
    // Also fires when a click lands in an email's frame, which the window's pointer events never see.
    window.addEventListener('blur', away);
    return () => { window.removeEventListener('pointerdown', outside, true); window.removeEventListener('scroll', away, true); window.removeEventListener('resize', away); window.removeEventListener('blur', away); };
  }, [anchor]);
  const keydown = (event: ReactKeyboardEvent) => {
    event.stopPropagation();
    const items = [...element.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)') ?? []];
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    const move = ({ ArrowDown: items[(index + 1) % items.length], ArrowUp: items[(index <= 0 ? items.length : index) - 1], Home: items[0], End: items[items.length - 1] } as Record<string, HTMLButtonElement | undefined>)[event.key];
    if (move) { event.preventDefault(); move.focus(); return; }
    if (event.key === 'Escape' || event.key === 'Tab') { event.preventDefault(); close(true); return; }
    if (event.key === 'Enter' || event.key === ' ') return;
    const item = entries.find((entry): entry is MenuItem => entry !== 'separator' && !!entry.shortcut && matches(event.nativeEvent, entry.shortcut));
    if (item) { event.preventDefault(); choose(item); }
  };
  return <motion.div ref={element} role="menu" aria-label={t('app.actions')} tabIndex={-1} className="context-menu" inert={!present} style={{ left: place.left, top: place.top, transformOrigin: place.origin }} onKeyDown={keydown} onContextMenu={event => event.preventDefault()}
    initial={keyboard ? false : { opacity: 0, transform: travel('scale(0.96)', 'scale(1)') }} animate={{ opacity: 1, transform: 'scale(1)', transition: { duration: 0.14, ease: easeOut } }} exit={{ opacity: 0, transition: leave }}>
    {/* eslint-disable-next-line @eslint-react/no-array-index-key -- entries never reorder while a menu is open */}
    {entries.map((entry, index) => entry === 'separator' ? <div key={index} role="separator" /> : <button key={index} type="button" role="menuitem" disabled={entry.disabled} aria-keyshortcuts={entry.shortcut && ariaKeys(entry.shortcut)}
      onPointerMove={event => { if (document.activeElement !== event.currentTarget) event.currentTarget.focus({ preventScroll: true }); }} onPointerLeave={() => element.current?.focus({ preventScroll: true })} onClick={() => choose(entry)}>
      {entry.icon ? <entry.icon size={16} aria-hidden="true" /> : <span className="menu-icon" />}<span>{entry.label}</span>{entry.shortcut && <kbd>{label(entry.shortcut)}</kbd>}
    </button>)}
  </motion.div>;
}
