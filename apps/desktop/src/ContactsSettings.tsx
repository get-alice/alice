import { t, date } from './i18n';
import { Fragment, useEffect, useEffectEvent, useState } from 'react';
import { motion } from 'motion/react';
import { BookUser, CalendarDays, Check, ChevronRight, LoaderCircle, Plus, RefreshCw, ShieldCheck } from 'lucide-react';
import { api, errorMessage, get, importSystemContacts, isMacDesktop, openAuth, post, type Account, type CalendarList, type ContactsSync, type DavKind, type DavSource, type Integrations, type Preferences } from './api';
import { ProviderIcon } from './App';
import { loadCalendars } from './Invitation';
import { Glide, rise } from './motion';

const kinds = (): [DavKind, string][] => [['carddav', t('settings.contacts_carddav')], ['caldav', t('settings.calendar_caldav')]];

function synced(value: string | null | undefined) {
  if (!value) return t('settings.contacts_not_synced');
  const minutes = Math.round((Date.now() - new Date(value).getTime()) / 60_000);
  return minutes < 1 ? t('settings.contacts_synced_now') : minutes < 60 ? t('settings.contacts_synced_minutes', { count: minutes }) : t('settings.contacts_synced_at', { date: date(value, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' }) });
}

/**
 * Settings → Contacts & calendars: provider access for Gmail and Outlook sign-ins, CardDAV and CalDAV
 * servers, the default calendar, and Mac Contacts. Alice reads address books and adds events; it never
 * changes contacts or keeps a calendar of its own.
 */
export default function ContactsSettings({ accounts, preferences, onPreferences, onChanged }: { accounts: Account[]; preferences: Preferences; onPreferences: (patch: Partial<Preferences>) => void; onChanged: () => void }) {
  const real = accounts.filter(a => !a.is_demo);
  const [sources, setSources] = useState<DavSource[] | null>(null);
  const [calendars, setCalendars] = useState<CalendarList | null>(null);
  const [busy, setBusy] = useState('');
  // Feedback appears in the section whose action it answers: `access`, `dav` or `suggestions`.
  const [notice, setNotice] = useState<{ at: string; text: string | (() => string); ok: boolean } | null>(null);
  const [flow, setFlow] = useState<{ account: string; id: string; url: string } | null>(null);
  const [adding, setAdding] = useState(false);
  const [form, setForm] = useState({ kind: 'carddav' as DavKind, name: '', url: '', username: '', password: '' });
  const [sync, setSync] = useState<ContactsSync | null>(null);
  const changed = useEffectEvent(() => onChanged());
  const refreshCalendars = (fresh = false) => loadCalendars(fresh).then(setCalendars).catch((e: unknown) => setCalendars({ calendars: [], errors: [{ source: '', message: errorMessage(e) }] }));
  useEffect(() => {
    get<DavSource[]>('/v1/dav').then(setSources).catch((e: unknown) => { setSources([]); setNotice({ at: 'dav', text: errorMessage(e), ok: false }); });
    void refreshCalendars();
  }, []);
  useEffect(() => {
    if (!flow) return; let cancelled = false;
    const timer = window.setInterval(() => {
      get<{ status: string; message?: string }>(`/v1/connections/${flow.id}`).then(result => {
        if (cancelled || result.status === 'pending') return;
        setFlow(null);
        if (result.status === 'connected') { setNotice({ at: 'access', text: () => t('settings.access_granted_alice_will_use_it_from_now_on'), ok: true }); changed(); void refreshCalendars(true); }
        else setNotice({ at: 'access', text: result.message || (() => t('settings.sign_in_could_not_be_completed')), ok: false });
      }).catch((e: unknown) => { if (!cancelled) { setNotice({ at: 'access', text: errorMessage(e), ok: false }); setFlow(null); } });
    }, 2000);
    return () => { cancelled = true; clearInterval(timer); };
  }, [flow]);
  // `action` resolves to a confirmation to show, or to nothing.
  const run = async (key: string, at: string, action: () => Promise<string | (() => string)> | Promise<void>) => {
    setBusy(key); setNotice(null);
    try { const done = await action(); if ((typeof done === 'string' && done) || typeof done === 'function') setNotice({ at, text: done, ok: true }); }
    catch (e) { setNotice({ at, text: errorMessage(e), ok: false }); }
    finally { setBusy(''); }
  };
  const feedback = (at: string) => notice?.at === at && <motion.p className={notice.ok ? 'form-success' : 'form-error'} role={notice.ok ? 'status' : 'alert'} {...rise}>{notice.ok && <Check size={16} />}{typeof notice.text === 'function' ? notice.text() : notice.text}</motion.p>;
  const allow = (account: Account, patch: Partial<Integrations>) => run(`access:${account.id}`, 'access', async () => {
    const result = await post<{ account?: Account; flow_id?: string; url?: string }>(`/v1/accounts/${encodeURIComponent(account.id)}/integrations`, patch);
    if (result.flow_id && result.url) { setFlow({ account: account.id, id: result.flow_id, url: result.url }); await openAuth(result.url); return; }
    onChanged(); void refreshCalendars(true);
  });
  const addSource = () => run('add', 'dav', async () => {
    const source = await post<DavSource>('/v1/dav', { ...form, name: form.name.trim() || new URL(form.url).hostname });
    setSources(current => [...(current ?? []), source]); setAdding(false);
    setForm({ kind: 'carddav', name: '', url: '', username: '', password: '' });
    if (source.kind === 'caldav') void refreshCalendars(true);
    return () => t('settings.source_connected', { name: source.name });
  });
  const removeSource = (source: DavSource) => run(`remove:${source.id}`, 'dav', async () => {
    const result = await api<{ credentials_removed: boolean; message: string | null }>('DELETE', `/v1/dav/${encodeURIComponent(source.id)}`);
    setSources(current => (current ?? []).filter(s => s.id !== source.id));
    // The password may outlive the server in the credential store; the daemon says where to delete it.
    if (source.kind === 'caldav') void refreshCalendars(true);
    return () => `${t(source.kind === 'carddav' ? 'settings.source_contacts_removed' : 'settings.source_removed', { name: source.name })}${result.credentials_removed || !result.message ? '' : ` ${result.message}`}`;
  });
  const refreshContacts = () => run('sync', 'suggestions', async () => { setSync(null); setSync(await post<ContactsSync>('/v1/contacts/sync', {})); });
  const systemContacts = (on: boolean) => run('system', 'suggestions', async () => {
    if (on) { const count = await importSystemContacts(); onPreferences({ system_contacts: true }); return () => t('settings.system_contacts_enabled', { count }); }
    else { const { removed } = await api<{ removed: number }>('DELETE', '/v1/contacts/system'); onPreferences({ system_contacts: false }); return () => t('settings.system_contacts_disabled', { count: removed }); }
  });
  // A failed source is named the way Settings shows it: an account's address or a server's name.
  const sourceName = (id: string) => accounts.find(a => a.id === id)?.email || sources?.find(s => s.id === id)?.name || id;
  const owners = [...new Set(calendars?.calendars.map(c => c.owner) ?? [])];
  return <>
    <div className="settings-section-heading"><h2>{t('settings.contacts_calendars')}</h2><p>{t('settings.alice_suggests_recipients_as_you_write')}</p></div>
    <section className="preference-section"><h3>{t('settings.google_and_microsoft')}</h3><p>{t('settings.let_alice_read_the_address_book')}</p>
      {real.length ? <div className="connected-accounts integration-list">{real.map(account => <Fragment key={account.id}><div>
        <ProviderIcon provider={account.provider} /><span><strong>{account.name}</strong><small>{account.email}</small></span>
        {account.provider === 'imap' ? <span className="integration-note">{t('settings.uses_carddav_and_caldav')}</span> : <div className="integration-switches">{(['contacts', 'calendar'] as const).map(kind => <span key={kind}><input id={`${kind}-${account.id}`} type="checkbox" role="switch" checked={!!account.integrations?.[kind]} disabled={!!busy || !!flow} onChange={event => void allow(account, { [kind]: event.target.checked })} /><label htmlFor={`${kind}-${account.id}`}>{kind === 'contacts' ? t('settings.contacts') : t('settings.calendar')}</label></span>)}</div>}
      </div>
        {flow?.account === account.id && <motion.div className="integration-waiting" role="status" {...rise}><LoaderCircle size={15} className="spinning-icon" /><span>{t('settings.waiting_for_you_to_allow_access_in_your_browser')}</span><button className="text-button" onClick={() => void openAuth(flow.url).catch((e: unknown) => setNotice({ at: 'access', text: errorMessage(e), ok: false }))}>{t('settings.open_sign_in_again')}</button><button className="text-button muted" onClick={() => setFlow(null)}>{t('settings.cancel')}</button></motion.div>}
      </Fragment>)}</div> : <p className="preference-note">{t('settings.add_a_gmail_or_outlook_account')}</p>}
      {feedback('access')}
    </section>
    <section className="preference-section"><h3>{t('settings.carddav_and_caldav_servers')}</h3><p>{t('settings.for_icloud_fastmail_nextcloud_and_other')}</p>
      {sources && sources.length > 0 && <div className="connected-accounts dav-list">{sources.map(source => <div key={source.id}>
        <span className="dav-icon">{source.kind === 'carddav' ? <BookUser size={17} /> : <CalendarDays size={17} />}</span>
        <span><strong>{source.name}</strong><small>{source.username} · {source.url}</small><small className={source.error ? 'storage-error' : ''}>{source.error || (source.kind === 'carddav' ? synced(source.last_synced) : t('settings.read_when_needed_events_are_not_copied'))}</small></span>
        <span className="account-status">{source.kind === 'carddav' ? t('settings.contacts') : t('settings.calendar')}</span>
        <button className="text-button muted" aria-label={t('settings.remove_named', { name: source.name })} disabled={!!busy} onClick={() => void removeSource(source)}>{busy === `remove:${source.id}` ? t('settings.removing') : t('settings.remove')}</button>
      </div>)}</div>}
      {sources?.length === 0 && !adding && <p className="preference-note">{t('settings.no_servers_yet')}</p>}
      {adding ? <motion.form className="dav-form" aria-label={t('settings.add_a_server')} {...rise} onSubmit={event => { event.preventDefault(); void addSource(); }}>
        <div className="segmented-control" role="group" aria-label={t('settings.server_type')}>{kinds().map(([id, label]) => <button type="button" key={id} aria-pressed={form.kind === id} className={form.kind === id ? 'selected' : ''} disabled={busy === 'add'} onClick={() => setForm({ ...form, kind: id })}>{form.kind === id && <Glide group="dav-kind" />}{label}</button>)}</div>
        <div className="form-grid"><label><span>{t('settings.name')} <span className="optional">{t('settings.optional')}</span></span><input value={form.name} placeholder={t('settings.dav_name_placeholder')} disabled={busy === 'add'} onChange={event => setForm({ ...form, name: event.target.value })} /></label><label>{t('settings.server_url')}<input type="url" required value={form.url} placeholder={form.kind === 'carddav' ? 'https://contacts.icloud.com' : 'https://caldav.icloud.com'} disabled={busy === 'add'} onChange={event => setForm({ ...form, url: event.target.value })} /></label></div>
        <div className="form-grid"><label>{t('settings.username')}<input required autoComplete="off" value={form.username} disabled={busy === 'add'} onChange={event => setForm({ ...form, username: event.target.value })} /></label><label>{t('settings.app_password')}<input type="password" required autoComplete="new-password" value={form.password} disabled={busy === 'add'} onChange={event => setForm({ ...form, password: event.target.value })} /></label></div>
        <div className="dav-form-actions"><button type="submit" className="primary" disabled={busy === 'add'}>{busy === 'add' ? <><LoaderCircle size={15} className="spinning-icon" />{t('settings.signing_in')}</> : t('settings.add_server')}</button><button type="button" className="text-button muted" disabled={busy === 'add'} onClick={() => setAdding(false)}>{t('settings.cancel')}</button></div>
      </motion.form> : <button className="add-account-button" onClick={() => { setAdding(true); setNotice(null); }}><Plus size={16} />{t('settings.add_a_server')}<ChevronRight size={15} /></button>}
      {feedback('dav')}
    </section>
    <section className="preference-section"><h3>{t('settings.recipient_suggestions')}</h3><p>{t('settings.people_you_write_to_and_hear')}</p>
      <div className="preference-row"><div><span className="preference-label">{t('settings.address_books')}</span><p>{t('settings.alice_refreshes_them_on_its_own')}</p></div><button className="secondary-button" disabled={!!busy} onClick={() => void refreshContacts()}><RefreshCw size={15} className={busy === 'sync' ? 'spinning-icon' : ''} />{busy === 'sync' ? t('settings.refreshing') : t('settings.refresh_contacts')}</button></div>
      {sync && <motion.div className={`${sync.errors.length ? 'form-error' : 'update-result'} sync-result`} role={sync.errors.length ? 'alert' : 'status'} {...rise}><p>{sync.contacts ? t('settings.synced_contacts', { count: sync.contacts }) : t('settings.no_address_books_are_connected_yet')}{sync.errors.map(e => <span key={e.source}>{sourceName(e.source)}: {e.message}</span>)}</p></motion.div>}
      {isMacDesktop && <div className="preference-row"><div><label htmlFor="system-contacts">{t('settings.mac_contacts')}</label><p>{t('settings.suggest_people_from_the_contacts_app')}</p></div>{busy === 'system' ? <LoaderCircle size={18} className="spinning-icon" aria-label={t('settings.updating_mac_contacts')} /> : <input id="system-contacts" type="checkbox" role="switch" checked={preferences.system_contacts} disabled={!!busy} onChange={event => void systemContacts(event.target.checked)} />}</div>}
      {feedback('suggestions')}
    </section>
    <section className="preference-section"><h3>{t('settings.calendar_for_new_events')}</h3><p>{t('settings.where_add_to_calendar_puts_invitations')}</p>
      {calendars === null ? <p className="preference-note">{t('settings.loading_calendars')}</p> : calendars.calendars.length ? <label className="preference-field">{t('settings.add_events_to')}<select value={calendars.calendars.some(c => c.id === preferences.default_calendar) ? preferences.default_calendar : ''} onChange={event => onPreferences({ default_calendar: event.target.value })}><option value="">{t('settings.primary_calendar')}</option>{owners.map(owner => <optgroup key={owner} label={sourceName(owner)}>{calendars.calendars.filter(c => c.owner === owner).map(c => <option key={c.id} value={c.id}>{c.primary ? t('settings.primary_calendar_name', { name: c.name }) : c.name}</option>)}</optgroup>)}</select></label>
        : <p className="preference-note">{t('settings.turn_on_calendar_for_an_account')}</p>}
      {calendars?.errors.map(e => <div key={e.source} className="form-error" role="alert">{e.source ? `${sourceName(e.source)}: ` : ''}{e.message}</div>)}
    </section>
    <div className="privacy-note"><ShieldCheck size={18} /><p>{t('settings.contacts_and_events_stay_with_their')}</p></div>
  </>;
}
