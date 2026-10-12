import { t, applyLanguage, languages, type LanguagePreference } from './i18n';
import { useEffect, useEffectEvent, useRef, useState } from 'react';
import { motion } from 'motion/react';
import { Check, LoaderCircle } from 'lucide-react';
import { api, defaultPreferences, errorMessage, type Preferences, type Snapshot } from './api';
import AddAccount from './AddAccount';
import Welcome from './Welcome';
import { easeOut, rise } from './motion';

/** First-run screen shown until a real mail account is connected. */
export default function Setup({ snapshot, onReady, onPreferences }: { snapshot: Snapshot; onReady: () => void; onPreferences: (preferences: Preferences) => void }) {
  const heading = useRef<HTMLHeadingElement>(null);
  // The welcome plays once per install; storage is optional, so without it the welcome plays each launch until setup finishes.
  const [welcomed, setWelcomed] = useState(() => { try { return localStorage.getItem('alice.welcomed') === '1'; } catch { return false; } });
  const welcome = () => { try { localStorage.setItem('alice.welcomed', '1'); } catch { /* See above. */ } setWelcomed(true); };
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
  const finishing = useRef(false);
  useEffect(() => { heading.current?.focus(); }, [welcomed]);
  const finish = () => {
    if (finishing.current) return;
    finishing.current = true;
    setConnected(true);
    onReady();
  };
  // A real account can also arrive outside this form: an OAuth sign-in noticed by the app's refresh first, or the CLI.
  const finishWhenConnected = useEffectEvent(() => { if (snapshot.accounts.length) finish(); });
  useEffect(() => finishWhenConnected(), [snapshot]);
  if (!welcomed) return <Welcome onContinue={welcome} />;
  return <div className="setup-shell">
    <header className="setup-header" data-tauri-drag-region />
    <main className="setup-scroll"><div className="setup-column">
      {/* First run: the welcome and the form arrive in sequence. A once-per-install moment, so a little more time is allowed. */}
      <motion.div className="setup-intro" {...rise} transition={{ duration: 0.28, ease: easeOut }}><img src="/alice.svg" alt="" /><h1 ref={heading} tabIndex={-1}>{t('settings.connect_an_email_account')}</h1><p>{t('settings.gmail_outlook_or_any_imap_account')}</p></motion.div>
      <motion.div className="settings-content setup-form" {...rise} transition={{ duration: 0.28, ease: easeOut, delay: 0.06 }}>
        <label className="preference-field" htmlFor="setup-language">{t('settings.language')}<select id="setup-language" value={language} disabled={savingLanguage} onChange={event => void chooseLanguage(event.target.value as LanguagePreference)}><option value="system">{t('settings.system_default')}</option>{languages.map(option => <option key={option.tag} value={option.tag}>{option.name}</option>)}</select></label>
        {languageError && <p className="form-error" role="alert">{languageError}</p>}
        {connected ? <div className="setup-done"><motion.p className="form-success" role="status" {...rise}><Check size={16} />{t('settings.your_account_is_connected')}</motion.p><p className="setup-status" role="status"><LoaderCircle size={15} className="spinning-icon" />{t('settings.opening_your_inbox')}</p></div>
        : <AddAccount onConnected={finish} />}</motion.div>
    </div></main>
  </div>;
}
