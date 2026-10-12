import { t, useLocale, date as localizedDate, number, list, applyLanguage } from './i18n';
import { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { Archive, ArrowDown, ArrowLeft, Check, ChevronLeft, ChevronRight, CircleHelp, Copy, Inbox, Mail, MailOpen, MoreHorizontal, PanelRight, Pencil, Plus, RefreshCw, Reply, Search, Send, Settings as SettingsIcon, Sparkles, Star, Tag, Trash2, X } from 'lucide-react';
import { get, patch, post, openEmailLink, onNotificationOpen, importSystemContacts, isMacDesktop, systemContactsImportedAt, errorMessage, type Account, type Compose, type Message, type Opened, type Patch, type Snapshot, type Thread, type ThreadPage, defaultPreferences, type Preferences } from './api';
import ComposeModal from './Compose';
import SettingsPage, { type SettingsSection } from './Settings';
import Setup from './Setup';
import Assistant from './Assistant';
import Conversation from './Conversation';
import { latestBackup, removeBackup } from './drafts';
import { easeOut, Glide, leave, prefersReducedMotion, rise, travel } from './motion';
import { ContextMenu, MenuContext, copyText, fromButton, fromEvent, openFocusedMenu, type MenuEntry, type MenuItem, type Menus, type OpenMenu } from './ContextMenu';
import { ariaKeys, goTo, hint, label, matches, shortcuts, type Shortcut } from './shortcuts';
// Matches the CSS breakpoint where every layout collapses to one pane.
const singlePaneMaxWidth = 1050;
// Keys each opened menu, so a menu replacing one that is still fading out mounts fresh.
let menuSerial = 0;

const folders = [
  { id: 'inbox', name: 'common.folder.inbox', icon: Inbox, shortcut: goTo.inbox }, { id: 'starred', name: 'common.folder.starred', icon: Star, shortcut: goTo.starred },
  { id: 'sent', name: 'common.folder.sent', icon: Send, shortcut: goTo.sent }, { id: 'drafts', name: 'common.folder.drafts', icon: Pencil, shortcut: goTo.drafts },
  { id: 'archive', name: 'common.folder.archive', icon: Archive, shortcut: goTo.archive }, { id: 'trash', name: 'common.folder.trash', icon: Trash2, shortcut: goTo.trash },
] as const;
export function initials(name: string) { return name.split(/[\s@]+/).filter(Boolean).slice(0, 2).map(n => n[0]).join('').toUpperCase(); }
export function avatarColor(name: string) { return ['rose', 'sage', 'violet', 'sand', 'blue'][name.charCodeAt(0) % 5]; }
export function relative(date: string) {
  const d = new Date(date), now = new Date();
  return d.toDateString() === now.toDateString() ? localizedDate(d, { hour: 'numeric', minute: '2-digit' }) : localizedDate(d, { month: 'short', day: 'numeric' });
}
/** The conversation's senders for a list row: "Joana, me", or the sender alone when only one wrote. */
function senders(row: Thread) {
  if (row.participants.length < 2) return row.participants[0] === 'me' ? t('app.me') : row.message.from_name;
  const names = row.participants.map(name => name === 'me' ? t('app.me') : name.split(/\s+/)[0]);
  return names.length > 3 ? `${names[0]} … ${list(names.slice(-2))}` : list(names);
}
function categoryName(id: string) { return id === 'work' ? t('app.work') : id === 'newsletters' ? t('app.reading') : id === 'updates' ? t('app.updates') : t('app.personal'); }
function dateGroup(date: string) {
  const day = new Date(date).toDateString(), yesterday = new Date(); yesterday.setDate(yesterday.getDate() - 1);
  return day === new Date().toDateString() ? t('app.today') : day === yesterday.toDateString() ? t('app.yesterday') : t('app.earlier');
}

export default function App() {
  useLocale();
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [threads, setThreads] = useState<Thread[]>([]);
  const [total, setTotal] = useState(0);
  const [unreadTotal, setUnreadTotal] = useState(0);
  const recovered = useRef(false);
  const [folder, setFolder] = useState('inbox');
  const [accountId, setAccountId] = useState('');
  const [category, setCategory] = useState('');
  const [unread, setUnread] = useState(false);
  const [search, setSearch] = useState('');
  const [query, setQuery] = useState('');
  const [page, setPage] = useState(0);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [compose, setCompose] = useState<Compose | null>(null);
  const [settings, setSettings] = useState<SettingsSection | null>(null);
  const [assistant, setAssistant] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [connectionError, setConnectionError] = useState('');
  const [toast, setToast] = useState<{ id: number; text: string; error?: boolean } | null>(null);
  const [toastHeld, setToastHeld] = useState(false);
  const [contextMenu, setContextMenu] = useState<OpenMenu | null>(null);
  // Which view the loaded rows belong to: rows cascade in only when this changes, not on every background refresh.
  const [listScope, setListScope] = useState('');
  // Rows that fold away when removed (pointer archive/trash), and whether the reader eases in (pointer) or swaps instantly (keyboard).
  const [leaving, setLeaving] = useState<ReadonlySet<string>>(() => new Set());
  const [readerEnter, setReaderEnter] = useState(false);
  const [starPop, setStarPop] = useState(0);
  // The assistant keeps its grid column until its exit animation finishes: docked when it opens, undocked on exit.
  const [assistantDocked, setAssistantDocked] = useState(false);
  if (assistant && !assistantDocked) setAssistantDocked(true);
  const request = useRef(0);
  const searchRef = useRef<HTMLInputElement>(null);
  const thread = threads.find(t => t.id === selectedId);
  // The conversation's newest message in this view: the toolbar, menu and assistant act on it.
  const selected = thread?.message;
  const visible = threads;
  // The open conversation, loaded separately from the list; the listed message stands in until it arrives.
  const [conversation, setConversation] = useState<{ thread: string; messages: Message[] } | null>(null);
  // Messages that were unread when the conversation was opened start expanded.
  const [openedUnread, setOpenedUnread] = useState<string[]>([]);
  const [listRevision, setListRevision] = useState('');
  const messages = thread && conversation?.thread === thread.id ? conversation.messages : selected ? [selected] : [];
  // Reply to the newest message someone else sent, or the newest when every message is yours.
  const replyTarget = [...messages].reverse().find(m => m.folder !== 'sent') ?? messages[messages.length - 1];
  const accounts = snapshot?.accounts ?? [];
  const preferences = snapshot?.preferences ?? defaultPreferences;
  useEffect(() => { document.documentElement.dataset.theme = preferences.theme; }, [preferences.theme]);
  // Density is app-wide (reader, settings, compose), so it lives on the root like the theme.
  useEffect(() => { document.documentElement.dataset.density = preferences.density; }, [preferences.density]);
  const openSettings = (section: SettingsSection) => { setAssistant(false); setSettings(section); };
  // Setup stays open from when it is first needed until it reports ready, so its demo import can finish after the account appears.
  const [setupOpen, setSetupOpen] = useState(false);
  const realAccount = accounts.some(a => !a.is_demo);
  if (snapshot && !realAccount && !setupOpen) setSetupOpen(true);
  const setup = !!snapshot && (setupOpen || !realAccount);

  // What the shown list was loaded for. A poll that finds the same daemon revision has nothing new: it skips the list request and every re-render.
  const loaded = useRef({ scope: '', revision: '' });
  const refresh = useCallback(async () => {
    const current = ++request.current;
    const params = new URLSearchParams({ folder, limit: '50', offset: String(page * 50) });
    if (accountId) params.set('account_id', accountId);
    if (query) params.set('q', query);
    if (unread) params.set('unread', 'true');
    if (category) params.set('category', category);
    const statePath = accountId ? `/v1/state?account_id=${encodeURIComponent(accountId)}` : '/v1/state';
    const listPath = `/v1/threads?${params}`;
    const scope = `${statePath} ${listPath}`;
    let state: Snapshot, mail: ThreadPage;
    if (scope === loaded.current.scope) {
      state = await get<Snapshot>(statePath);
      if (current !== request.current) return;
      if (state.revision && state.revision === loaded.current.revision) { setConnectionError(''); return; }
      mail = await get<ThreadPage>(listPath);
    } else [state, mail] = await Promise.all([get<Snapshot>(statePath), get<ThreadPage>(listPath)]);
    if (current !== request.current) return;
    // A daemon from an older build (e.g. an installed Alice.app) can own the port; its state lacks preferences.
    if (!state.preferences) throw new Error(t('app.oldDaemon'));
    if (accountId && !state.accounts.some(a => a.id === accountId)) { setAccountId(''); setPage(0); return; }
    // State and list read at different revisions may disagree, so the next poll loads both again.
    loaded.current = { scope, revision: state.revision && state.revision === mail.revision ? state.revision : '' };
    state.preferences = { ...state.preferences, language: state.preferences.language ?? 'system' };
    applyLanguage(state.preferences.language);
    setSnapshot(state); setThreads(mail.threads); setTotal(mail.total); setUnreadTotal(mail.unread_total); setListRevision(mail.revision); setConnectionError('');
    setListScope([folder, accountId, category, unread, page].join('|'));
    if (page > 0 && page * 50 >= mail.total) setPage(Math.max(0, Math.ceil(mail.total / 50) - 1));
    const shown = mail.threads;
    setSelectedId(id => shown.some(t => t.id === id) ? id : state.preferences.mail_layout !== 'single' && window.innerWidth > singlePaneMaxWidth ? shown[0]?.id || null : null);
  }, [folder, accountId, query, unread, page, category]);
  useEffect(() => {
    let active = true;
    // A hidden window skips polls and catches up as soon as it is shown.
    const update = () => { if (!document.hidden) refresh().catch((e: unknown) => { if (active) setConnectionError(errorMessage(e)); }); };
    update(); const timer = window.setInterval(update, 2000);
    document.addEventListener('visibilitychange', update);
    // Bumping the request counter discards a refresh still in flight for the old view.
    const requests = request;
    return () => { active = false; clearInterval(timer); document.removeEventListener('visibilitychange', update); requests.current++; };
  }, [refresh]);
  // Reload the open conversation when another one opens or stored mail changes (new replies, flags).
  const openThread = thread?.id, openMessage = thread?.message.id;
  useEffect(() => {
    if (!openThread || !openMessage) return;
    let active = true;
    get<Message[]>(`/v1/threads/${encodeURIComponent(openMessage)}`)
      .then(messages => { if (active && messages.length) setConversation({ thread: openThread, messages }); })
      .catch(() => { /* The listed message stays shown; the next change retries. */ });
    return () => { active = false; };
  }, [openThread, openMessage, listRevision]);
  useEffect(() => { const timer = setTimeout(() => { setQuery(search); setPage(0); }, 180); return () => clearTimeout(timer); }, [search]);
  // Like Sonner: a toast you are pointing at stays until you let go of it.
  useEffect(() => { if (!toast || toast.error || toastHeld) return; const timer = setTimeout(() => setToast(null), 4200); return () => clearTimeout(timer); }, [toast, toastHeld]);

  const notify = (text: string, error = false) => setToast({ id: Date.now(), text, error });
  // Mac Contacts are read at start, when turned on (Settings imports then), and every 6 hours while Alice runs.
  const systemContacts = isMacDesktop && preferences.system_contacts;
  useEffect(() => {
    if (!systemContacts) return;
    const run = () => { void importSystemContacts().catch((e: unknown) => notify(t('app.contactsError', { error: errorMessage(e) }), true)); };
    if (Date.now() - systemContactsImportedAt > 60_000) run();
    const timer = setInterval(run, 6 * 60 * 60_000);
    return () => clearInterval(timer);
  }, [systemContacts]);
  const change = async (ids: string[], changes: Patch) => {
    setBusy(true);
    try { for (const id of ids) await patch(id, changes); await refresh(); return true; }
    catch (e) { notify(errorMessage(e), true); return false; }
    finally { setBusy(false); }
  };
  // In a single pane the list and reader replace each other; move focus with them so keyboard navigation continues.
  const choose = (row: Thread, pointer = false) => {
    setSelectedId(row.id); setReaderEnter(pointer);
    if (row.id !== selectedId) setOpenedUnread(row.unread_ids);
    if ((preferences.mail_layout === 'single' || window.innerWidth <= singlePaneMaxWidth) && !document.activeElement?.closest('.reader')) requestAnimationFrame(() => document.querySelector<HTMLElement>('.reader')?.focus());
    if (row.unread_ids.length) void change(row.unread_ids, { is_read: true });
  };
  const backToList = () => { setSelectedId(null); requestAnimationFrame(() => document.querySelector<HTMLElement>('.email-list')?.focus()); };
  // A clicked new-mail notification shows the Inbox, then its message once a list loaded after the
  // click includes it: the list shown at the click may predate the message or its unread state.
  const [opening, setOpening] = useState<{ id: string; stale: Thread[] } | null>(null);
  const openNotified = useEffectEvent((opened: Opened) => {
    setSettings(null); setAssistant(false); setContextMenu(null);
    setFolder('inbox'); setAccountId(''); setCategory(''); setUnread(false); setSearch(''); setQuery(''); setPage(0); setSelectedId(null);
    loaded.current = { scope: '', revision: '' }; request.current++;
    setOpening(opened.message_id ? { id: opened.message_id, stale: threads } : null);
  });
  useEffect(() => onNotificationOpen(opened => openNotified(opened)), []);
  // Runs when a list arrives rather than on a click, so it reads the latest `choose` without re-running for it.
  const openWhenListed = useEffectEvent(() => {
    if (!opening || threads === opening.stale || query || listScope !== ['inbox', '', '', false, 0].join('|')) return;
    const row = threads.find(t => t.ids.includes(opening.id));
    if (row) { setOpening(null); choose(row); }
  });
  // A list arriving is outside React, so the conversation it brings opens from this effect.
  // eslint-disable-next-line react-hooks/set-state-in-effect -- reacts to loaded data, not to render state
  useEffect(() => openWhenListed(), [opening, threads, listScope, query]);
  useEffect(() => {
    if (!opening) return;
    const timer = setTimeout(() => { setOpening(null); notify(t('app.missingInbox')); }, 6000);
    return () => clearTimeout(timer);
  }, [opening]);
  const createCompose = (mail?: Message, body = '') => {
    const account = mail?.account_id || accounts.find(a => a.id === preferences.default_account_id)?.id || accounts.find(a => !a.is_demo)?.id;
    if (!account) { openSettings('accounts'); return; }
    setCompose(mail?.folder === 'drafts' ? { id: mail.id, account_id: mail.account_id, to: mail.to, subject: mail.subject, body: mail.body, in_reply_to: mail.in_reply_to } : {
      id: crypto.randomUUID(), account_id: account, to: mail ? (mail.reply_to.length ? mail.reply_to : [mail.from_email]) : [],
      subject: mail ? (/^re:/i.test(mail.subject) ? mail.subject : `Re: ${mail.subject}`) : '', body: preferences.signature.trim() && !body.endsWith(`\n\n-- \n${preferences.signature.trim()}`) ? `${body.trimEnd()}\n\n-- \n${preferences.signature.trim()}` : body, in_reply_to: mail?.message_id || null,
    });
  };
  const goFolder = (id: string) => { setAssistant(false); setFolder(id); setCategory(''); setPage(0); setSelectedId(null); setUnread(false); };
  const sync = async () => {
    setSyncing(true);
    try { const result = await post<{ synced: number; errors: { message: string }[] }>('/v1/sync', {}); await refresh(); notify(result.errors.length ? result.errors.map(e => e.message).join(' · ') : t('app.upToDate'), !!result.errors.length); }
    catch (e) { notify(errorMessage(e), true); }
    finally { setSyncing(false); }
  };
  // Archive and Trash move the conversation's messages in this view. A pointer move folds the row away
  // so the list doesn't jump; a keyboard shortcut removes it instantly.
  const move = async (row: Thread, destination: string, pointer: boolean) => {
    if (pointer) setLeaving(ids => new Set(ids).add(row.id));
    const ok = await change(row.ids, { folder: destination });
    setLeaving(ids => { const next = new Set(ids); next.delete(row.id); return next; });
    if (ok) notify(destination === 'archive' ? t('app.movedArchive') : destination === 'trash' ? t('app.movedTrash') : t('app.movedInbox'));
  };
  // Starring marks the newest message; unstarring clears every starred message, as the list shows the conversation starred if any is.
  // A conversation that isn't open has only its listed messages to clear.
  const toggleStar = (row: Thread) => {
    const open = row.id === thread?.id;
    if (open) setStarPop(n => n + 1);
    void (row.is_starred ? change(open ? messages.filter(m => m.is_starred).map(m => m.id) : row.ids, { is_starred: false }) : change([row.message.id], { is_starred: true }));
  };
  // Reply as the reader does, to the newest message someone else sent; a draft opens to continue writing.
  const replyTo = async (row: Thread) => {
    try {
      const conversation = row.id === thread?.id ? messages : await get<Message[]>(`/v1/threads/${encodeURIComponent(row.message.id)}`);
      const target = row.message.folder === 'drafts' ? conversation.find(m => m.id === row.message.id) ?? row.message : [...conversation].reverse().find(m => m.folder !== 'sent');
      if (target) createCompose(target);
    } catch (e) { notify(errorMessage(e), true); }
  };
  const menus = useMemo<Menus>(() => ({
    open: (request, entries, key = '') => { if (request) setContextMenu({ id: ++menuSerial, request, entries, key, restore: document.activeElement as HTMLElement | null }); },
    copy: (text, what) => { copyText(text).then(() => setToast({ id: Date.now(), text: t('app.copied', { what }) }), (e: unknown) => setToast({ id: Date.now(), text: errorMessage(e), error: true })); },
  }), []);
  /**
   * Everything that can be done to a conversation, for its menus, the reader toolbar and the keyboard.
   * `pointer` actions animate (a moved row folds away); keyboard ones apply instantly.
   */
  const threadActions = (row: Thread, pointer: boolean) => {
    const open = row.id === thread?.id;
    const draft = row.message.folder === 'drafts';
    const fixed = draft || row.message.folder === 'sent';
    // An open conversation knows its reply target; a row only knows whether anyone else wrote in it.
    const canReply = draft || (open ? replyTarget?.folder !== 'sent' : row.participants.some(name => name !== 'me'));
    const unread = row.unread_ids.length > 0;
    return {
      open: { label: draft ? t('app.openDraft') : t('app.openConversation'), icon: MailOpen, shortcut: shortcuts.open, run: () => choose(row, pointer) },
      reply: { label: draft ? t('app.continueWriting') : t('app.reply'), icon: draft ? Pencil : Reply, shortcut: shortcuts.reply, disabled: !canReply, run: () => void replyTo(row) },
      archive: { label: t('app.archive'), icon: Archive, shortcut: shortcuts.archive, disabled: busy || fixed || row.message.folder === 'archive', run: () => void move(row, 'archive', pointer) },
      trash: { label: t('app.moveTrash'), icon: Trash2, shortcut: shortcuts.trash, disabled: busy || fixed || row.message.folder === 'trash', run: () => void move(row, 'trash', pointer) },
      inbox: { label: t('app.moveInbox'), icon: Inbox, shortcut: shortcuts.inbox, disabled: busy || fixed || row.message.folder === 'inbox', run: () => void move(row, 'inbox', pointer) },
      read: { label: unread ? t('app.markRead') : t('app.markUnread'), icon: unread ? MailOpen : Mail, shortcut: shortcuts.read, disabled: busy, run: () => void (unread ? change(row.unread_ids, { is_read: true }) : change([row.message.id], { is_read: false })) },
      star: { label: row.is_starred ? t('app.unstar') : t('app.star'), icon: Star, shortcut: shortcuts.star, disabled: busy, run: () => toggleStar(row) },
      ask: { label: t('app.askAbout'), icon: Sparkles, shortcut: shortcuts.assistant, run: () => { if (!open) choose(row, pointer); setAssistant(true); } },
      copy: { label: t('app.copySender'), icon: Copy, run: () => menus.copy(row.message.from_email, t('app.senderAddress')) },
    } satisfies Record<string, MenuItem>;
  };
  /** A conversation's menu: `lead` items, then moving, flagging and asking Alice. */
  const threadMenu = (row: Thread, lead: MenuEntry[], tail: MenuEntry[] = []): MenuEntry[] => {
    const actions = threadActions(row, true);
    return [...lead, 'separator', actions.archive, actions.trash, actions.inbox, 'separator', actions.read, actions.star, 'separator', actions.ask, ...tail];
  };
  // One window listener for the app's lifetime; the event handler always reads the latest render.
  const onKeyDown = useEffectEvent((event: KeyboardEvent) => {
    if (settings || compose || setup || contextMenu || event.isComposing || event.defaultPrevented) return;
    if (matches(event, shortcuts.back)) { if (assistant) setAssistant(false); else if (selected && (preferences.mail_layout === 'single' || window.innerWidth <= singlePaneMaxWidth)) backToList(); return; }
    if (matches(event, shortcuts.menu)) { event.preventDefault(); openFocusedMenu(); return; }
    // ⌘/Ctrl shortcuts work anywhere in the mail view. Single keys work in the list or reader (or with nothing
    // focused), outside text fields and controls, where they would type or press instead; a list row is the exception.
    const target = event.target as HTMLElement;
    const plain = !event.metaKey && !event.ctrlKey;
    if (plain && (target.closest('input, textarea, select, button:not(.email-row), a, summary, [contenteditable]') || !(target === document.body || target.closest('.mailbox, .reader')))) return;
    const index = visible.findIndex(t => t.id === selectedId);
    const step = (offset: number) => { if (visible.length) choose(visible[Math.min(Math.max(index + offset, 0), visible.length - 1)]); };
    const commands: [Shortcut, () => void][] = [
      [shortcuts.compose, () => createCompose()],
      [shortcuts.search, () => searchRef.current?.focus()],
      [shortcuts.sync, () => { if (!syncing) void sync(); }],
      [shortcuts.assistant, () => setAssistant(!assistant)],
      [shortcuts.settings, () => openSettings('appearance')],
      [shortcuts.help, () => openSettings('shortcuts')],
      ...folders.map(item => [item.shortcut, () => goFolder(item.id)] as [Shortcut, () => void]),
      [shortcuts.next, () => step(1)],
      [shortcuts.previous, () => step(-1)],
    ];
    // Enter on a focused row presses it; on the list itself it opens the selected conversation.
    if (target.matches('.email-list')) commands.push([shortcuts.open, () => { const row = thread ?? visible[0]; if (row) choose(row); }]);
    if (thread) {
      const actions = threadActions(thread, false);
      for (const action of [actions.reply, actions.archive, actions.trash, actions.inbox, actions.read, actions.star]) commands.push([action.shortcut, () => { if (!action.disabled) action.run(); }]);
    }
    const command = commands.find(([shortcut]) => matches(event, shortcut));
    if (command) { event.preventDefault(); command[1](); }
  });
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => onKeyDown(event);
    window.addEventListener('keydown', keydown); return () => window.removeEventListener('keydown', keydown);
  }, []);

  useEffect(() => {
    if (!snapshot || setup || recovered.current) return;
    recovered.current = true;
    const backup = latestBackup(snapshot.accounts.map(a => a.id));
    if (!backup) return;
    void get<Message>(`/v1/messages/${backup.draft.id}`).then(mail => {
      if (mail.folder !== 'drafts') removeBackup(backup.draft.id);
      else setCompose(current => current ?? backup.draft);
    }).catch(() => setCompose(current => current ?? backup.draft));
  }, [snapshot, setup]);

  if (!snapshot) return <motion.div className="startup" initial={{ opacity: 0 }} animate={{ opacity: 1 }}><img src="/alice.svg" alt="" /><h1>Alice</h1><p>{connectionError || t('app.connecting')}</p>{connectionError && <button className="primary" onClick={() => void refresh().catch((e: unknown) => setConnectionError(errorMessage(e)))}>{t('app.retry')}</button>}</motion.div>;
  if (setup) return <Setup snapshot={snapshot} onPreferences={saved => { request.current++; applyLanguage(saved.language ?? 'system'); setSnapshot(current => current ? { ...current, preferences: { ...saved, language: saved.language ?? 'system' } } : current); }} onReady={() => { setSetupOpen(false); setSettings(null); void refresh().catch((e: unknown) => setConnectionError(errorMessage(e))); }} />;
  if (settings) return <SettingsPage initialTab={settings} snapshot={snapshot} onClose={() => { setSettings(null); request.current++; requestAnimationFrame(() => document.querySelector<HTMLButtonElement>('.sidebar-footer button')?.focus()); }} onChanged={() => void refresh().catch((e: unknown) => notify(errorMessage(e), true))} onPreferences={(saved: Preferences) => { request.current++; applyLanguage(saved.language ?? 'system'); setSnapshot(current => current ? { ...current, preferences: { ...saved, language: saved.language ?? 'system' } } : current); if (saved.mail_layout !== preferences.mail_layout) setSelectedId(null); }} />;
  const currentAccount = accounts.find(a => a.id === selected?.account_id);
  const selectedIndex = visible.findIndex(m => m.id === selectedId);
  const title = t(folders.find(f => f.id === folder)?.name ?? 'common.folder.inbox');
  // The React Compiler's ref check reads these actions as ref values because their handlers (not render) reach the
  // request counter through `refresh`; render only reads labels and disabled states.
  const actions = thread ? threadActions(thread, true) : null;
  // eslint-disable-next-line react-hooks/refs -- see above
  const readerMenu = thread && actions ? threadMenu(thread, [actions.reply], [actions.copy]) : [];
  const syncItem: MenuItem = { label: t('app.syncAccounts'), icon: RefreshCw, shortcut: shortcuts.sync, disabled: syncing, run: () => void sync() };
  // Outlines what an open menu acts on.
  const targeted = (key: string) => contextMenu?.key === key ? 'context-target' : '';

  return <MenuContext value={menus}><div className={`app-shell layout-${preferences.mail_layout} ${preferences.show_avatars ? '' : 'avatars-hidden'} ${selected ? 'has-selection' : ''} ${assistant || assistantDocked ? 'ai-open' : ''}`}>
    <aside className="sidebar">
      <div className="brand" data-tauri-drag-region><img src="/alice.svg" alt="" /><span>alice<span className="brand-dot">.</span></span></div>
      <button className="compose-button" aria-keyshortcuts={ariaKeys(shortcuts.compose)} onClick={() => createCompose()}><Pencil size={18} /><span>{t('app.compose')}</span><kbd>{label(shortcuts.compose)}</kbd></button>
      <div className="sidebar-scroll"><nav className="folder-nav" aria-label={t('app.mailFolders')}>{folders.map(item => <button key={item.id} className={`nav-item ${folder === item.id && !category ? 'active' : ''} ${targeted(`folder:${item.id}`)}`} aria-current={folder === item.id && !category ? 'page' : undefined} aria-keyshortcuts={ariaKeys(item.shortcut)} onClick={() => goFolder(item.id)} title={hint(t(item.name), item.shortcut)}
        onContextMenu={event => menus.open(fromEvent(event), [{ label: t('app.openFolder', { folder: t(item.name) }), icon: item.icon, shortcut: item.shortcut, run: () => goFolder(item.id) }, { label: t('app.unreadOnly'), icon: MailOpen, run: () => { goFolder(item.id); setUnread(true); } }], `folder:${item.id}`)}>
        {folder === item.id && !category && <Glide group="folder" />}<item.icon size={19} /><span>{t(item.name)}</span>{item.id === 'inbox' && snapshot.unread_count > 0 && <span className="nav-count" aria-label={t('app.unreadCount', { count: snapshot.unread_count })}>{number(snapshot.unread_count)}</span>}
      </button>)}</nav>
      <div className="sidebar-section"><span className="section-label">{t('app.collections')}</span><Tag size={14} />
        {[['work', 'violet'], ['personal', 'sage'], ['newsletters', 'sand']].map(([id, color]) => <button key={id} className={`collection ${category === id ? 'active' : ''}`} aria-pressed={category === id} onClick={() => { setAssistant(false); setFolder('inbox'); setCategory(category === id ? '' : id); setPage(0); setSelectedId(null); }}><span className={`collection-dot ${color}`} /><span>{categoryName(id)}</span></button>)}
      </div>
      <div className="sidebar-section accounts-section"><span className="section-label">{t('app.accounts')}</span><button className="icon-button" aria-label={t('app.addAccount')} onClick={() => openSettings('accounts')}><Plus size={14} /></button>
        {accounts.map(account => <button key={account.id} className={`account-item ${accountId === account.id ? 'active' : ''} ${targeted(`account:${account.id}`)}`} aria-pressed={accountId === account.id} onClick={() => { setAccountId(accountId === account.id ? '' : account.id); setPage(0); }} title={account.email}
          onContextMenu={event => menus.open(fromEvent(event), [{ label: accountId === account.id ? t('app.showAllInboxes') : t('app.onlyAccount', { name: account.name }), icon: Inbox, run: () => { setAccountId(accountId === account.id ? '' : account.id); setPage(0); } }, { label: t('app.copyEmail'), icon: Copy, run: () => menus.copy(account.email, t('app.emailAddress')) }, 'separator', syncItem, { label: t('app.manageAccounts'), icon: SettingsIcon, run: () => openSettings('accounts') }], `account:${account.id}`)}>
          <ProviderIcon provider={account.provider} /><span><strong>{account.name}</strong><small>{account.email}</small></span>{account.sync_error && <span className="error-dot" title={account.sync_error} />}
        </button>)}
      </div>
      </div>
      <button className={`alice-nav ${assistant ? 'active' : ''}`} aria-pressed={assistant} aria-keyshortcuts={ariaKeys(shortcuts.assistant)} title={hint(t('app.askAlice'), shortcuts.assistant)} onClick={() => setAssistant(!assistant)}><Sparkles size={19} /><span>{t('app.askAlice')}</span></button>
      <div className="sidebar-footer"><button className="nav-item" aria-label={t('app.settings')} aria-keyshortcuts={ariaKeys(shortcuts.settings)} title={hint(t('app.settings'), shortcuts.settings)} onClick={() => openSettings('appearance')}><SettingsIcon size={19} /><span>{t('app.settings')}</span></button></div>
    </aside>

    <header className="workspace-header" data-tauri-drag-region><div className="breadcrumb"><Mail size={16} /><span>{t('app.mail')}</span><ChevronRight size={14} /><span>{accountId ? accounts.find(a => a.id === accountId)?.name : t('app.allInboxes')}</span></div>
      <div className="header-right"><span className={`connection-status ${connectionError ? 'offline' : ''}`}><span />{connectionError ? t('app.reconnecting') : syncing || snapshot.syncing ? t('app.syncing') : busy ? t('app.updating') : t('app.connected')}</span><button className="icon-button" aria-label={t('app.keyboardShortcuts')} aria-keyshortcuts={ariaKeys(shortcuts.help)} title={hint(t('app.keyboardShortcuts'), shortcuts.help)} onClick={() => openSettings('shortcuts')}><CircleHelp size={19} /></button><button className={`icon-button ${assistant ? 'selected' : ''}`} aria-label={t('app.toggleAssistant')} aria-expanded={assistant} aria-controls="alice-assistant" aria-keyshortcuts={ariaKeys(shortcuts.assistant)} title={hint(t('app.askAlice'), shortcuts.assistant)} onClick={() => setAssistant(!assistant)}><PanelRight size={19} /></button></div>
    </header>

    <section className="mailbox" aria-label={t('app.messageList')}>
      <div className="mailbox-title"><h1>{category ? categoryName(category) : title}</h1><button className={`icon-button ${syncing ? 'spinning' : ''}`} aria-label={syncing ? t('app.syncingAccounts') : t('app.syncAccounts')} aria-keyshortcuts={ariaKeys(shortcuts.sync)} title={hint(t('app.syncAccounts'), shortcuts.sync)} disabled={syncing} onClick={() => void sync()}><RefreshCw size={18} /></button></div>
      <div className="search-box"><Search size={17} /><input ref={searchRef} aria-label={t('app.searchMail')} aria-keyshortcuts={ariaKeys(shortcuts.search)} placeholder={t('app.searchYourMail')} value={search} onChange={e => setSearch(e.target.value)} />{search ? <button className="icon-button" aria-label={t('app.clearSearch')} onClick={() => setSearch('')}><X size={15} /></button> : <kbd>{label(shortcuts.search)}</kbd>}</div>

      <div className="mail-tabs"><button aria-pressed={!unread} className={!unread ? 'active' : ''} onClick={() => { setUnread(false); setPage(0); }}>{!unread && <Glide group="mail-tab" className="tab-underline" />}{t('app.allMail')}</button><button aria-pressed={unread} className={unread ? 'active' : ''} onClick={() => { setUnread(true); setPage(0); }}>{unread && <Glide group="mail-tab" className="tab-underline" />}{t('app.unread')}{unreadTotal > 0 && <> <span>{number(unreadTotal)}</span></>}</button><button className="sort-label" onClick={() => notify(query ? t('app.ranked') : t('app.sorted'))} title={query ? t('app.bestFirst') : t('app.newestFirst')}><ArrowDown size={14} /><span>{query ? t('app.bestMatch') : t('app.newest')}</span></button></div>
      {/* Keyed by the loaded view: rows cascade in when the view changes, not on every background refresh. */}
      <div className="email-list" key={listScope} tabIndex={0} aria-label={t('app.listNavigation')}
        onContextMenu={event => menus.open(fromEvent(event), [{ label: t('app.compose'), icon: Pencil, shortcut: shortcuts.compose, run: () => createCompose() }, { label: t('app.searchYourMail'), icon: Search, shortcut: shortcuts.search, run: () => searchRef.current?.focus() }, syncItem, 'separator', { label: unread ? t('app.showAllMail') : t('app.unreadOnly'), icon: unread ? Inbox : MailOpen, run: () => { setUnread(!unread); setPage(0); } }], 'list')}>{visible.length === 0 && <motion.div className="empty-mail" {...rise}><Inbox size={32} /><h3>{search ? t('app.noResults') : t('app.noMessages')}</h3>{unread && <button className="text-button" onClick={() => setUnread(false)}>{t('app.showAllMail')}</button>}</motion.div>}<AnimatePresence>{visible.map((row, index) => { const mail = row.message, isUnread = row.unread_ids.length > 0; return <motion.div key={row.id} className="email-item" initial={rise.initial} animate={{ ...rise.animate, transition: { duration: 0.22, ease: easeOut, delay: Math.min(index, 12) * 0.028 } }} exit={leaving.has(row.id) ? { opacity: 0, height: 0, transition: { duration: 0.22, ease: easeOut } } : undefined}>
        {/* Search results are ranked, so date groups would repeat; they only head date-ordered lists. */}
        {!query && (index === 0 || dateGroup(mail.date) !== dateGroup(visible[index - 1].message.date)) && <div className="date-divider">{dateGroup(mail.date)}</div>}
        {/* Right-click acts on the row without opening it; the selected row answers the keyboard's menu key. */}
        <button className={`email-row ${row.id === selectedId ? 'selected' : ''} ${isUnread ? 'unread' : ''} ${targeted(`row:${row.id}`)}`} data-menu-default={row.id === selectedId || undefined} aria-pressed={row.id === selectedId} aria-label={t('app.rowLabel', { sender: senders(row), subject: mail.subject, count: row.count, status: row.is_starred ? (isUnread ? t('app.unreadStarred') : t('app.readStarred')) : (isUnread ? t('app.unreadStatus') : t('app.readStatus')) })} onClick={() => choose(row, true)}
          onContextMenu={event => { const rowActions = threadActions(row, true); menus.open(fromEvent(event), threadMenu(row, [rowActions.open, rowActions.reply], [rowActions.copy]), `row:${row.id}`); }}>
          {isUnread && <span className="unread-dot" />}<div className={`avatar ${avatarColor(mail.from_name)}`}>{initials(mail.from_name)}</div><div className="email-summary"><div className="email-from"><strong>{senders(row)}{row.count > 1 && <span className="thread-size">{number(row.count)}</span>}</strong>{row.is_starred && <Star size={14} className="starred" />}<time dateTime={mail.date}>{relative(mail.date)}</time></div><div className="email-subject"><span>{mail.subject}</span><span className="email-row-meta"><span className={`category-label ${mail.category}`}>{categoryName(mail.category)}</span></span></div><p>{mail.preview}</p></div>
        </button>
      </motion.div>; })}</AnimatePresence></div>
      <div className="mailbox-footer"><span>{total ? t('app.range', { start: page * 50 + 1, end: Math.min(page * 50 + threads.length, total), total }) : t('app.zeroConversations')}</span><div><button className="icon-button" aria-label={t('app.previousPage')} disabled={page === 0} onClick={() => setPage(page - 1)}><ChevronLeft size={16} /></button><button className="icon-button" aria-label={t('app.nextPage')} disabled={(page + 1) * 50 >= total} onClick={() => setPage(page + 1)}><ChevronRight size={16} /></button></div></div>
    </section>

    <main className="reader" aria-label={t('app.messageReader')} tabIndex={0}>
      {/* eslint-disable-next-line react-hooks/refs -- the conversation actions above; handlers run on click */}
      <div className="reader-toolbar"><button className="icon-button mobile-back" aria-label={t('app.backList')} onClick={backToList}><ArrowLeft size={19} /></button><div className="toolbar-actions"><button className="icon-button" aria-label={t('app.archiveConversation')} aria-keyshortcuts={ariaKeys(shortcuts.archive)} title={hint(t('app.archive'), shortcuts.archive)} disabled={!actions || actions.archive.disabled} onClick={actions?.archive.run}><Archive size={19} /></button><button className="icon-button" aria-label={t('app.trashConversation')} aria-keyshortcuts={ariaKeys(shortcuts.trash)} title={hint(t('app.moveTrash'), shortcuts.trash)} disabled={!actions || actions.trash.disabled} onClick={actions?.trash.run}><Trash2 size={19} /></button><span className="toolbar-divider" /><button className="icon-button" aria-label={actions?.read.label || t('app.markUnread')} aria-keyshortcuts={ariaKeys(shortcuts.read)} title={hint(actions?.read.label || t('app.markUnread'), shortcuts.read)} disabled={!actions || actions.read.disabled} onClick={actions?.read.run}><Mail size={19} /></button><button className={`icon-button ${thread?.is_starred ? 'starred' : ''}`} aria-pressed={thread?.is_starred || false} aria-label={thread?.is_starred ? t('app.unstarConversation') : t('app.starConversation')} aria-keyshortcuts={ariaKeys(shortcuts.star)} title={hint(thread?.is_starred ? t('app.unstar') : t('app.star'), shortcuts.star)} disabled={!actions || actions.star.disabled} onClick={actions?.star.run}>
        {/* Remounts on each click so the pop plays for that click only, never when j/k lands on a starred message. */}
        <motion.span key={starPop} className="icon-wrap" animate={starPop && !prefersReducedMotion() ? { transform: ['scale(1) rotate(0deg)', 'scale(1.3) rotate(-12deg)', 'scale(1) rotate(0deg)'] } : undefined} transition={{ duration: 0.36, ease: easeOut }}><Star size={19} /></motion.span></button>
        <button className={`icon-button ${contextMenu?.key === 'more' ? 'selected' : ''}`} aria-label={t('app.moreActions')} aria-haspopup="menu" aria-expanded={contextMenu?.key === 'more'} disabled={!thread} onClick={event => { if (contextMenu?.key === 'more') setContextMenu(null); else menus.open(fromButton(event.currentTarget, event.detail === 0), readerMenu, 'more'); }}><MoreHorizontal size={19} /></button></div><div className="reader-pagination"><span>{t('app.position', { position: selectedIndex >= 0 ? number(selectedIndex + 1) : '–', total: visible.length })}</span><button className="icon-button" aria-label={t('app.previousConversation')} aria-keyshortcuts={ariaKeys(shortcuts.previous)} title={hint(t('app.previousConversation'), shortcuts.previous)} disabled={selectedIndex <= 0} onClick={() => choose(visible[selectedIndex - 1], true)}><ChevronLeft size={18} /></button><button className="icon-button" aria-label={t('app.nextConversation')} aria-keyshortcuts={ariaKeys(shortcuts.next)} title={hint(t('app.nextConversation'), shortcuts.next)} disabled={selectedIndex < 0 || selectedIndex >= visible.length - 1} onClick={() => choose(visible[selectedIndex + 1], true)}><ChevronRight size={18} /></button></div></div>
      {/* Opening a conversation by pointer eases it in; keyboard navigation (j/k) swaps instantly. */}
      {thread && selected && replyTarget ? <div className="reader-scroll" key={thread.id}><motion.article className="message-content" data-menu-default initial={readerEnter ? rise.initial : false} animate={rise.animate} onContextMenu={event => menus.open(fromEvent(event), readerMenu, 'reader')}>
        <div className="message-labels"><span className={`category-label ${selected.category}`}>{categoryName(selected.category)}</span><span className="message-account">{currentAccount?.email}{currentAccount?.is_demo ? t('app.demoSuffix') : ''}</span></div>
        <Conversation key={`${thread.id}:${preferences.message_format}:${preferences.remote_images}`} messages={messages} order={preferences.thread_order} format={preferences.message_format} loadImages={preferences.remote_images === 'always'} unreadIds={openedUnread} defaultCalendar={preferences.default_calendar} onUpdated={updated => { setConversation(current => current && { ...current, messages: current.messages.map(m => m.id === updated.id ? updated : m) }); setThreads(current => current.map(t => t.message.id === updated.id ? { ...t, message: updated } : t)); }} onOpen={url => { void openEmailLink(url).catch((e: unknown) => notify(errorMessage(e), true)); }}
          menuFor={(mail, extra) => threadMenu(thread, [{ label: mail.folder === 'drafts' ? t('app.continueWriting') : t('app.replyTo', { name: mail.from_name.split(' ')[0] }), icon: mail.folder === 'drafts' ? Pencil : Reply, shortcut: mail.id === (selected.folder === 'drafts' ? selected : replyTarget).id ? shortcuts.reply : undefined, disabled: mail.folder === 'sent', run: () => createCompose(mail) }, { label: t('app.copySender'), icon: Copy, run: () => menus.copy(mail.from_email, t('app.senderAddress')) }, ...extra])} />
        <div className="reply-block"><div className="reply-buttons">{(selected.folder === 'drafts' || replyTarget.folder !== 'sent') && <button className="reply-button" onClick={() => createCompose(selected.folder === 'drafts' ? selected : replyTarget)}>{selected.folder === 'drafts' ? <Pencil size={16} /> : <Reply size={17} />}{selected.folder === 'drafts' ? t('app.continueWriting') : t('app.replyTo', { name: replyTarget.from_name.split(' ')[0] })}</button>}<button className="ai-reply-button" onClick={() => setAssistant(true)}><Sparkles size={15} />{replyTarget.folder === 'sent' ? t('app.askAlice') : t('app.draftAlice')}</button></div></div>
      </motion.article></div> : <motion.div className="reader-empty" {...rise}><img src="/alice.svg" alt="" /><h2>{t('app.noSelected')}</h2></motion.div>}
    </main>
    <AnimatePresence onExitComplete={() => { if (!assistant) setAssistantDocked(false); }}>{assistant && <Assistant key="assistant" selected={replyTarget} configured={snapshot.ai.configured} defaultCalendar={preferences.default_calendar} onClose={() => setAssistant(false)} onSettings={() => openSettings('ai')} onDraft={(body, context) => createCompose(context, body)} onApply={async actions => { for (const action of actions) await patch(action.message_id, action.action === 'archive' ? { folder: 'archive' } : action.action === 'star' ? { is_starred: true } : { is_read: true }); await refresh(); }} />}</AnimatePresence>
    <AnimatePresence>{compose && <ComposeModal key={compose.id} initial={compose} accounts={accounts} persisted={threads.some(t => t.ids.includes(compose.id))} onClose={() => setCompose(null)} onDiscarded={async () => { setCompose(null); await refresh(); notify(t('app.draftDiscarded')); }} onSaved={async (sent, accountId) => { const demo = accounts.find(a => a.id === accountId)?.is_demo; setCompose(null); await refresh(); notify(sent ? demo ? t('app.demoSent') : t('app.emailSent') : t('app.draftSaved')); }} />}</AnimatePresence>
    {/* Sonner-style: rises from the bottom edge and leaves the way it came; a replacement crossfades in place. */}
    <div className="toast-region"><AnimatePresence>{toast && <motion.div key={toast.id} className={`toast ${toast.error ? 'error' : ''}`} role={toast.error ? 'alert' : 'status'} onPointerEnter={() => setToastHeld(true)} onPointerLeave={() => setToastHeld(false)} initial={{ opacity: 0, transform: travel('translateY(100%) scale(0.96)', 'translateY(0%) scale(1)') }} animate={{ opacity: 1, transform: 'translateY(0%) scale(1)', transition: { duration: 0.36, ease: easeOut } }} exit={{ opacity: 0, transform: travel('translateY(8px) scale(0.97)', 'translateY(0%) scale(1)'), transition: leave }}>{toast.error ? <CircleHelp size={18} /> : <Check size={18} />}<span>{toast.text}</span><button aria-label={t('app.dismissNotification')} onClick={() => { setToastHeld(false); setToast(null); }}><X size={16} /></button></motion.div>}</AnimatePresence></div>
    <AnimatePresence>{contextMenu && <ContextMenu key={contextMenu.id} menu={contextMenu} onClose={() => setContextMenu(null)} />}</AnimatePresence>
  </div></MenuContext>;
}

export function ProviderIcon({ provider }: { provider: Account['provider'] }) {
  return <span className={`provider-icon ${provider}`} aria-label={provider === 'gmail' ? 'Gmail' : provider === 'outlook' ? 'Outlook' : 'IMAP'}>{provider === 'gmail' ? <svg viewBox="0 0 24 24" aria-hidden="true"><path fill="#4285f4" d="M3 6h3v13H3z" /><path fill="#34a853" d="M18 6h3v13h-3z" /><path fill="#ea4335" d="m3 5 9 7 9-7v4l-9 7-9-7z" /><path fill="#fbbc04" d="m18 7 3-2v5l-3 2z" /></svg> : provider === 'outlook' ? <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="8" y="4" width="14" height="16" rx="2" fill="#0364b8" /><path fill="#28a8ea" d="M8 9.5 15 14l7-4.5V18a2 2 0 0 1-2 2h-10a2 2 0 0 1-2-2z" /><rect x="2" y="6.5" width="11" height="11" rx="1.8" fill="#0078d4" /><ellipse cx="7.5" cy="12" rx="2.6" ry="3.1" fill="none" stroke="#fff" strokeWidth="1.7" /></svg> : <Mail size={15} />}</span>;
}
