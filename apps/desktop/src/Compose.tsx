import { useEffect, useEffectEvent, useRef, useState } from 'react';
import { motion, useIsPresent } from 'motion/react';
import { ArrowUpRight, Check, Minus, Send, X } from 'lucide-react';
import { api, errorMessage, get, isNotFound, post, type Account, type Compose, type Contact, type Message } from './api';
import { backupDraft, readBackup, removeBackup } from './drafts';
import { easeOut, leave, rise, travel } from './motion';
import { ariaKeys, hint, matches, shortcuts } from './shortcuts';
import { t, useLocale } from './i18n';

export default function ComposeModal({ initial, accounts, persisted, onClose, onSaved, onDiscarded }: { initial: Compose; accounts: Account[]; persisted: boolean; onClose: () => void; onSaved: (sent: boolean, accountId: string) => Promise<void>; onDiscarded: () => Promise<void> }) {
  useLocale();
  // False while AnimatePresence plays the exit; the dialog stays modal until then, so ignore further input.
  const present = useIsPresent();
  const dialog = useRef<HTMLDialogElement>(null);
  const [draft, setDraft] = useState(() => readBackup(initial.id)?.draft ?? initial);
  const [to, setTo] = useState(() => readBackup(initial.id)?.to ?? initial.to.join(', '));
  const [stored, setStored] = useState(persisted);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ message: string; backup?: boolean } | null>(null);
  const [saveState, setSaveState] = useState<'saved' | 'unsaved' | 'attention' | 'backupSaving' | 'saving' | 'sending'>(persisted ? 'saved' : 'unsaved');
  const [backupError, setBackupError] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const revision = useRef(0);
  const mounted = useRef(true);
  const storedRef = useRef(persisted);
  const current = useRef({ draft, to });
  const focusStart = useEffectEvent(() => dialog.current?.querySelector<HTMLElement>(initial.in_reply_to ? 'textarea' : '#compose-to')?.focus());
  useEffect(() => {
    mounted.current = true;
    const previous = document.activeElement as HTMLElement | null;
    const element = dialog.current;
    element?.showModal(); focusStart();
    return () => { mounted.current = false; clearTimeout(timer.current ?? undefined); element?.close(); if (previous?.isConnected) previous.focus(); };
  }, []);
  const enqueue = (value: { draft: Compose; to: string }) => {
    const version = revision.current;
    const payload = { ...value.draft, to: value.to.split(/[,;]/).map(s => s.trim()).filter(Boolean) };
    setStored(true);
    queue.current = queue.current.catch(() => {}).then(async () => {
      const message = await post<Message>('/v1/messages', payload);
      storedRef.current = true;
      if (mounted.current) { setStored(true); if (version === revision.current) { setSaveState('saved'); setError(null); } }
      return message;
    });
    return queue.current.catch((e: unknown) => {
      if (mounted.current && version === revision.current) { setStored(storedRef.current); setSaveState('attention'); setError({ message: errorMessage(e), backup: true }); }
      throw e;
    });
  };
  // A draft left in the device backup (the app closed before it reached Alice) is saved again on open.
  const resumeBackup = useEffectEvent(() => { if (readBackup(initial.id)) void enqueue(current.current).catch(() => {}); });
  useEffect(() => resumeBackup(), []);
  const update = (next: Compose, recipients = to) => {
    current.current = { draft: next, to: recipients }; revision.current++;
    setDraft(next); setTo(recipients);
    try { backupDraft(next, recipients); setBackupError(false); setSaveState('backupSaving'); }
    catch { setBackupError(true); setSaveState('saving'); }
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => { void enqueue(current.current).catch(() => {}); }, 400);
  };
  const stopTimer = () => { if (timer.current) clearTimeout(timer.current); };
  const discard = async () => {
    stopTimer(); setBusy(true); setError(null);
    try {
      await queue.current.catch(() => {});
      try { await api('DELETE', `/v1/messages/${initial.id}`); }
      catch (e) { if (!isNotFound(e)) throw e; }
      removeBackup(initial.id); await onDiscarded();
    } catch (e) { setError({ message: errorMessage(e) }); }
    finally { setBusy(false); }
  };
  const save = async (send: boolean) => {
    stopTimer(); setBusy(true); setError(null); setSaveState('saving');
    try {
      await enqueue(current.current);
      if (send) { setSaveState('sending'); await post(`/v1/messages/${initial.id}/send`, {}); }
      removeBackup(initial.id); await onSaved(send, current.current.draft.account_id);
    } catch (e) { setError({ message: errorMessage(e) }); }
    finally { setBusy(false); }
  };
  const close = () => { if (busy || !present) return; if (draft.body || draft.subject || to || storedRef.current) void save(false); else { removeBackup(initial.id); onClose(); } };
  // A modal is not anchored to a trigger, so it scales from its centre. The ::backdrop fades in CSS (Motion can't reach pseudo-elements).
  return <motion.dialog ref={dialog} className="compose-dialog" aria-labelledby="compose-title" data-leaving={!present || undefined} inert={!present} initial={{ opacity: 0, transform: travel('translateY(12px) scale(0.97)', 'translateY(0px) scale(1)') }} animate={{ opacity: 1, transform: 'translateY(0px) scale(1)', transition: { duration: 0.26, ease: easeOut } }} exit={{ opacity: 0, transform: travel('translateY(6px) scale(0.98)', 'translateY(0px) scale(1)'), transition: leave }} onCancel={event => { event.preventDefault(); close(); }}>
    <form onSubmit={event => { event.preventDefault(); void save(true); }} onKeyDown={event => {
      if (matches(event.nativeEvent, shortcuts.send)) { event.preventDefault(); if (!busy && to.trim()) void save(true); }
      else if (matches(event.nativeEvent, shortcuts.saveDraft)) { event.preventDefault(); if (!busy) void save(false); }
    }}>
      <div className="dialog-top"><h2 id="compose-title"><span className="compose-dot" />{t(initial.in_reply_to ? 'compose.reply' : 'compose.newMessage')}</h2><div><button type="button" className="icon-button" aria-label={t('compose.saveAndCloseDraft')} aria-keyshortcuts={ariaKeys(shortcuts.closeCompose)} title={hint(t('compose.saveAndClose'), shortcuts.closeCompose)} disabled={busy} onClick={close}><Minus size={17} /></button><button type="button" className="icon-button" aria-label={t('compose.close')} disabled={busy} onClick={close}><X size={18} /></button></div></div>
      <div className="compose-field"><label htmlFor="compose-from">{t('compose.from')}</label><select id="compose-from" value={draft.account_id} disabled={stored || !!initial.in_reply_to || busy} onChange={event => update({ ...draft, account_id: event.target.value })}>{accounts.map(account => <option key={account.id} value={account.id}>{account.email}</option>)}</select><span><ArrowUpRight size={14} /></span></div>
      <div className="compose-field to-field"><label htmlFor="compose-to">{t('compose.to')}</label><RecipientInput value={to} accountId={draft.account_id} disabled={busy} onChange={value => update(draft, value)} /></div>
      <div className="compose-field subject-field"><label htmlFor="compose-subject">{t('compose.subject')}</label><input id="compose-subject" value={draft.subject} onChange={event => update({ ...draft, subject: event.target.value })} disabled={busy} /></div>
      <textarea className="compose-body" aria-label={t('compose.body')} value={draft.body} onChange={event => update({ ...draft, body: event.target.value })} disabled={busy} />
      <div className="compose-feedback"><p className="draft-status" role="status">{t(`compose.status.${saveState}`)}</p>
      {backupError && <motion.p className="form-error" role="alert" {...rise}>{t('compose.backupUnavailable')}</motion.p>}
      {error && <motion.p className="form-error" role="alert" {...rise}>{error.backup ? t('compose.saveFailed', { error: error.message }) : error.message}</motion.p>}</div>
      <div className="compose-bottom"><div><button type="submit" className="primary" aria-keyshortcuts={ariaKeys(shortcuts.send)} title={hint(t('compose.sendEmail'), shortcuts.send)} disabled={busy || !to.trim()}><Send size={15} />{t(busy ? saveState === 'sending' ? 'compose.status.sending' : 'compose.saving' : 'compose.sendEmail')}</button><button type="button" className="text-button" aria-keyshortcuts={ariaKeys(shortcuts.saveDraft)} title={hint(t('compose.saveDraft'), shortcuts.saveDraft)} disabled={busy} onClick={() => void save(false)}><Check size={14} />{t('compose.saveDraft')}</button></div><button type="button" className="text-button muted" disabled={busy} onClick={() => void discard()}>{t('compose.discardDraft')}</button></div>
    </form>
  </motion.dialog>;
}

/** The recipient being typed: the comma- or semicolon-separated part of `value` around the caret. */
function segmentAt(value: string, caret: number) {
  const start = Math.max(value.lastIndexOf(',', caret - 1), value.lastIndexOf(';', caret - 1)) + 1;
  const next = value.slice(caret).search(/[,;]/);
  const end = next < 0 ? value.length : caret + next;
  return { start, end, text: value.slice(start, end).trim() };
}

/**
 * The To field, suggesting people from mail history and address books as each recipient is typed.
 * ↑/↓ choose, Enter or Tab accepts, Esc closes the list (and only the list). Suggestions appear on
 * every keystroke, so the list shows instantly rather than animating.
 */
function RecipientInput({ value, accountId, disabled, onChange }: { value: string; accountId: string; disabled: boolean; onChange: (value: string) => void }) {
  const input = useRef<HTMLInputElement>(null);
  const [suggestions, setSuggestions] = useState<Contact[]>([]);
  const [active, setActive] = useState(0);
  const lookup = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const open = suggestions.length > 0 && !disabled;
  useEffect(() => () => { lookup.current++; if (timer.current) clearTimeout(timer.current); }, []);
  const close = () => { lookup.current++; if (timer.current) clearTimeout(timer.current); setSuggestions([]); };
  const suggest = (text: string, caret: number) => {
    const segment = segmentAt(text, caret);
    if (timer.current) clearTimeout(timer.current);
    const id = ++lookup.current;
    if (!segment.text) { setSuggestions([]); return; }
    // Everyone else already in the field is left out, so ask for enough to still fill the list.
    const present = new Set(text.split(/[,;]/).map(part => part.trim().toLowerCase()).filter(part => part && part !== segment.text.toLowerCase()));
    timer.current = setTimeout(() => {
      const params = new URLSearchParams({ q: segment.text, account_id: accountId, limit: String(Math.min(50, 6 + present.size)) });
      get<Contact[]>(`/v1/contacts?${params}`).then(found => {
        if (id !== lookup.current) return;
        setSuggestions(found.filter(contact => !present.has(contact.email.toLowerCase())).slice(0, 6)); setActive(0);
      }).catch(() => { if (id === lookup.current) setSuggestions([]); });
    }, 120);
  };
  const accept = (contact: Contact) => {
    const element = input.current;
    const segment = segmentAt(value, element?.selectionStart ?? value.length);
    const before = value.slice(0, segment.start).trimEnd();
    const head = `${before}${before ? ' ' : ''}${contact.email}, `;
    const rest = value.slice(segment.end).replace(/^[\s,;]+/, '');
    close(); onChange(head + rest);
    requestAnimationFrame(() => element?.setSelectionRange(head.length, head.length));
  };
  return <div className="recipient-input">
    <input ref={input} id="compose-to" role="combobox" aria-autocomplete="list" aria-expanded={open} aria-controls="compose-to-suggestions" aria-activedescendant={open ? `compose-to-option-${active}` : undefined} autoComplete="off" spellCheck={false} value={value} disabled={disabled}
      onChange={event => { onChange(event.target.value); suggest(event.target.value, event.target.selectionStart ?? event.target.value.length); }}
      onBlur={close}
      onKeyDown={event => {
        if (!open || event.nativeEvent.isComposing || event.metaKey || event.ctrlKey || event.altKey) return;
        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); setActive((active + (event.key === 'ArrowDown' ? 1 : suggestions.length - 1)) % suggestions.length); }
        else if ((event.key === 'Enter' || event.key === 'Tab') && !event.shiftKey) { event.preventDefault(); accept(suggestions[active]); }
        // Compose saves and closes on Esc; while suggestions show, Esc only closes them.
        else if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); }
      }} />
    {open && <ul id="compose-to-suggestions" className="recipient-suggestions" role="listbox" aria-label={t('compose.suggestedRecipients')}>{suggestions.map((contact, index) => <li key={contact.email} id={`compose-to-option-${index}`} role="option" aria-selected={index === active} className={index === active ? 'active' : ''} onMouseDown={event => event.preventDefault()} onMouseMove={() => { if (index !== active) setActive(index); }} onClick={() => accept(contact)}>
      <strong>{contact.name || contact.email}</strong>{contact.name && <span>{contact.email}</span>}
    </li>)}</ul>}
  </div>;
}
