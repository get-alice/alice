import { t, rich } from './i18n';
import { useEffect, useEffectEvent, useState, type ReactNode } from 'react';
import { motion } from 'motion/react';
import { ArrowUpRight, ChevronRight, LoaderCircle } from 'lucide-react';
import { errorMessage, get, openAuth, openHelp, post } from './api';
import { ProviderIcon } from './App';
import { rise } from './motion';

/** The provider choice, setup guide, and credential fields for connecting a mail account. `footer` renders just above the connect button. */
export default function AddAccount({ onConnected, onCancel, heading, footer }: { onConnected: () => void; onCancel?: () => void; heading?: ReactNode; footer?: ReactNode }) {
  const [provider, setProvider] = useState<'gmail' | 'outlook' | 'imap'>('gmail');
  const [passwordMode, setPasswordMode] = useState(false);
  const [name, setName] = useState(''); const [email, setEmail] = useState('');
  const [password, setPassword] = useState(''); const [clientId, setClientId] = useState(''); const [clientSecret, setClientSecret] = useState('');
  const [imapHost, setImapHost] = useState(''); const [smtpHost, setSmtpHost] = useState('');
  const [imapPort, setImapPort] = useState(993); const [smtpPort, setSmtpPort] = useState(587);
  const [busy, setBusy] = useState(false); const [error, setError] = useState<string | (() => string)>('');
  const [flowId, setFlowId] = useState(''); const [authUrl, setAuthUrl] = useState('');
  const connected = useEffectEvent(() => onConnected());
  useEffect(() => {
    if (!flowId) return; let cancelled = false;
    const timer = window.setInterval(() => {
      get<{ status: string; message?: string }>(`/v1/connections/${flowId}`).then(result => {
        if (cancelled || result.status === 'pending') return;
        setFlowId(''); setBusy(false);
        if (result.status === 'connected') { setPassword(''); setClientSecret(''); connected(); }
        else setError(() => result.message || (() => t('settings.sign_in_could_not_be_completed')));
      }).catch((e: unknown) => { if (!cancelled) { setError(errorMessage(e)); setBusy(false); setFlowId(''); } });
    }, 2000);
    return () => { cancelled = true; clearInterval(timer); };
  }, [flowId]);
  const connect = async () => {
    setBusy(true); setError('');
    try {
      const result = await post<{ url?: string; flow_id?: string }>('/v1/accounts', { name: name || email.split('@')[0], email, provider, password: provider === 'imap' || passwordMode ? password : null, client_id: clientId || null, client_secret: clientSecret || null, imap_host: imapHost || null, smtp_host: smtpHost || null, imap_port: imapPort, smtp_port: smtpPort });
      if (result.url && result.flow_id) { setAuthUrl(result.url); setFlowId(result.flow_id); await openAuth(result.url); }
      else { setPassword(''); setClientSecret(''); setBusy(false); onConnected(); }
    } catch (e) { setError(errorMessage(e)); setBusy(false); }
  };
  const help = (url: string, label: string) => <a href={url} target="_blank" rel="noreferrer" onClick={event => { event.preventDefault(); void openHelp(url).catch((e: unknown) => setError(errorMessage(e))); }}>{label} <ArrowUpRight size={12} aria-hidden="true" /></a>;
  return <><form onSubmit={event => { event.preventDefault(); void connect(); }}>
    {onCancel && <button className="text-button muted back-settings" type="button" onClick={onCancel}>{t('settings.back_to_accounts')}</button>}{heading}<div className="provider-options">{(['gmail', 'outlook', 'imap'] as const).map(value => <button type="button" disabled={busy} aria-pressed={provider === value} className={provider === value ? 'selected' : ''} key={value} onClick={() => { setProvider(value); setPasswordMode(false); }}><ProviderIcon provider={value} />{value === 'gmail' ? 'Gmail' : value === 'outlook' ? 'Outlook' : 'IMAP / SMTP'}</button>)}</div>
    <details open className="setup-guide"><summary>{provider === 'imap' ? t('settings.what_you_need_to_connect') : passwordMode ? t('settings.set_up_a_gmail_app_password') : t('settings.set_up_sign_in_first')}</summary>
      {provider === 'imap' ? <p>{t('settings.use_the_imap_and_smtp_server')}</p> : passwordMode ? <><p>{t('settings.turn_on_google_2_step_verification')}</p>{help('https://support.google.com/accounts/answer/185833', t('settings.google_app_password_instructions'))}</> : provider === 'gmail' ? <><p>{t('settings.alice_currently_needs_your_own_google')}</p><ol><li>{t('settings.create_a_google_cloud_project_and')}</li><li>{rich('settings.google_desktop_client', { type: <strong>{t('settings.desktop_app_type')}</strong> })}</li><li>{rich('settings.google_callback', { url: <code>http://127.0.0.1:4816/callback</code> })}</li></ol>{help('https://developers.google.com/identity/protocols/oauth2/native-app', t('settings.google_desktop_sign_in_guide'))}<p>{t('settings.for_a_personal_account_select_use')}</p></> : <><p>{t('settings.alice_currently_needs_your_own_microsoft')}</p><ol><li>{t('settings.register_a_public_client_app_in')}</li><li>{rich('settings.microsoft_callback', { url: <code>http://127.0.0.1:4816/callback</code> })}</li><li>{rich('settings.microsoft_permissions', { imap: <code>IMAP.AccessAsUser.All</code>, smtp: <code>SMTP.Send</code>, offline: <code>offline_access</code> })}</li><li>{t('settings.copy_the_application_client_id_below')}</li></ol>{help('https://learn.microsoft.com/en-us/exchange/client-developer/legacy-protocols/how-to-authenticate-an-imap-pop-smtp-application-by-using-oauth', t('settings.microsoft_mail_sign_in_guide'))}</>}
    </details>
    <div className="form-grid"><label>{t('settings.account_name')}<input value={name} onChange={event => setName(event.target.value)} placeholder={t('settings.work_personal_something_else')} disabled={busy} /></label><label>{t('settings.email_address')}<input type="email" value={email} onChange={event => setEmail(event.target.value)} placeholder={t('settings.email_placeholder')} required disabled={busy} /></label></div>
    {provider === 'gmail' && <label className="checkbox-field"><input type="checkbox" checked={passwordMode} onChange={event => setPasswordMode(event.target.checked)} disabled={busy} />{t('settings.use_a_gmail_app_password')}</label>}
    {provider === 'imap' && <div className="form-grid server-grid"><label>{t('settings.imap_host')}<input value={imapHost} onChange={event => setImapHost(event.target.value)} placeholder={t('settings.imap_placeholder')} required disabled={busy} /></label><label>{t('settings.imap_tls_port')}<input type="number" min="1" max="65535" value={imapPort} onChange={event => setImapPort(Number(event.target.value))} required disabled={busy} /></label><label>{t('settings.smtp_host')}<input value={smtpHost} onChange={event => setSmtpHost(event.target.value)} placeholder={t('settings.smtp_placeholder')} required disabled={busy} /></label><label>{t('settings.smtp_tls_port')}<select value={smtpPort} onChange={event => setSmtpPort(Number(event.target.value))} disabled={busy}><option value={587}>587 — STARTTLS</option><option value={465}>465 — TLS</option></select></label></div>}
    {provider === 'imap' || passwordMode ? <label>{t('settings.app_password')}<input type="password" autoComplete="new-password" value={password} onChange={event => setPassword(event.target.value)} required disabled={busy} /></label> : <><label>{t('settings.oauth_client_id')}<input value={clientId} onChange={event => setClientId(event.target.value)} placeholder={t('settings.your_app_registration_s_client_id')} disabled={busy} /></label>{provider === 'gmail' && <label>{t('settings.oauth_client_secret')} <span className="optional">{t('settings.if_your_registration_requires_it')}</span><input type="password" value={clientSecret} onChange={event => setClientSecret(event.target.value)} autoComplete="new-password" disabled={busy} /></label>}<p className="field-note">{rich(provider === 'gmail' ? 'settings.google_oauth_note' : 'settings.microsoft_oauth_note', { url: <code>http://127.0.0.1:4816/callback</code> })}</p></>}
    {footer}<button className="primary connect-button" type="submit" disabled={busy}>{busy ? <><LoaderCircle size={15} />{flowId ? t('settings.waiting_for_browser_sign_in') : t('settings.connecting')}</> : <>{t('settings.connect_account')}<ChevronRight size={15} /></>}</button>{flowId && authUrl && <p className="field-note">{rich('settings.finish_browser', { link: <a href={authUrl} target="_blank" rel="noreferrer">{t("settings.open_sign_in_again")}</a> })}</p>}
  </form>{error && <motion.p className="form-error" role="alert" {...rise}>{typeof error === 'function' ? error() : error}</motion.p>}</>;
}
