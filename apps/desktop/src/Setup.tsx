import { t, applyLanguage, languages, type LanguagePreference } from './i18n';
import { useEffect, useEffectEvent, useRef, useState } from 'react';
import { motion } from 'motion/react';
import { Check, ChevronRight, LoaderCircle } from 'lucide-react';
import { api, defaultPreferences, errorMessage, post, type Account, type Preferences, type Snapshot } from './api';
import AddAccount from './AddAccount';
import { easeOut, rise } from './motion';

/** First-run screen shown until a real mail account is connected. */
export default function Setup({ snapshot, onReady, onPreferences }: { snapshot: Snapshot; onReady: () => void; onPreferences: (preferences: Preferences) => void }) {
  const heading = useRef<HTMLHeadingElement>(null);
  const [demo, setDemo] = useState(false);
  const [language, setLanguage] = useState(snapshot.preferences?.language ?? defaultPreferences.language);
  const [savingLanguage, setSavingLanguage] = useState(false);
  const [languageError, setLanguageError] = useState('');
  const chooseLanguage = async (next: LanguagePreference) => {
    const previous = language;
    setLanguage(next); applyLanguage(next); setSavingLanguage(true); setLanguageError('');
    try { const saved = await api<Preferences>('PATCH', '/v1/preferences', { language: next }); applyLanguage(saved.language); onPreferences(saved); }
    catch (error) { setLanguage(previous); applyLanguage(previous); setLanguageError(errorMessage(error)); }
    finally { setSavingLanguage(false); }
  };
  const [connected, setConnected] = useState(false);
  const [demoError, setDemoError] = useState('');
  const finishing = useRef(false);
  const hasDemo = snapshot.accounts.some(a => a.is_demo);
  useEffect(() => { heading.current?.focus(); }, []);
  const finish = async () => {
    if (finishing.current) return;
    finishing.current = true;
    setConnected(true);
    if (demo && !hasDemo) {
      try { await post<Account>('/v1/demo', {}); }
      catch (e) { setDemoError(errorMessage(e)); return; }
    }
    onReady();
  };
  // A real account can also arrive outside this form: an OAuth sign-in noticed by the app's refresh first, or the CLI.
  const finishWhenConnected = useEffectEvent(() => { if (snapshot.accounts.some(a => !a.is_demo)) void finish(); });
  // eslint-disable-next-line react-hooks/set-state-in-effect -- responds to an account the daemon reports, not to render state
  useEffect(() => finishWhenConnected(), [snapshot]);
  return <div className="setup-shell">
    <header className="setup-header" data-tauri-drag-region />
    <main className="setup-scroll"><div className="setup-column">
      {/* First run: the welcome and the form arrive in sequence. A once-per-install moment, so a little more time is allowed. */}
      <motion.div className="setup-intro" {...rise} transition={{ duration: 0.28, ease: easeOut }}><img src="/alice.svg" alt="" /><h1 ref={heading} tabIndex={-1}>{t('settings.connect_an_email_account')}</h1><p>{t('settings.gmail_outlook_or_any_imap_account')}</p></motion.div>
      <motion.div className="settings-content setup-form" {...rise} transition={{ duration: 0.28, ease: easeOut, delay: 0.06 }}>
        <label className="preference-field" htmlFor="setup-language">{t('settings.language')}<select id="setup-language" value={language} disabled={savingLanguage} onChange={event => void chooseLanguage(event.target.value as LanguagePreference)}><option value="system">{t('settings.system_default')}</option>{languages.map(option => <option key={option.tag} value={option.tag}>{option.name}</option>)}</select></label>
        {languageError && <p className="form-error" role="alert">{languageError}</p>}
        {connected ? <div className="setup-done"><motion.p className="form-success" role="status" {...rise}><Check size={16} />{t('settings.your_account_is_connected')}</motion.p>{demoError ? <><motion.p className="form-error" role="alert" {...rise}>{t('settings.demo_failed', { error: demoError })}</motion.p><button className="primary" onClick={onReady}>{t('settings.continue_to_your_inbox')}<ChevronRight size={15} /></button></> : <p className="setup-status" role="status"><LoaderCircle size={15} className="spinning-icon" />{demo && !hasDemo ? t('settings.adding_the_demo_inbox') : t('settings.opening_your_inbox')}</p>}</div>
        : <AddAccount onConnected={() => void finish()} footer={!hasDemo && <label className="setup-demo-option"><input type="checkbox" checked={demo} onChange={event => setDemo(event.target.checked)} /><span><strong>{t('settings.also_add_a_demo_inbox')}</strong><small>{t('settings.sample_messages_to_explore_alice_nothing')}</small></span></label>} />}</motion.div>
    </div></main>
  </div>;
}
