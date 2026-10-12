import { useEffect, useEffectEvent, useRef, useState } from 'react';
import { motion, useIsPresent } from 'motion/react';
import { ArrowUp, CalendarPlus, Check, FileText, LoaderCircle, Mail, Plus, Reply, Sparkles, X } from 'lucide-react';
import { errorMessage, post, type AiResult, type Message } from './api';
import { easeDrawer, easeOut, leave, rise, travel } from './motion';
import { EventCard } from './Invitation';
import { t, useLocale } from './i18n';

export default function Assistant({ selected, configured, defaultCalendar, onClose, onSettings, onDraft, onApply }: { selected?: Message; configured: boolean; defaultCalendar: string; onClose: () => void; onSettings: () => void; onDraft: (body: string, context?: Message) => void; onApply: (actions: AiResult['actions']) => Promise<void> }) {
  useLocale();
  const present = useIsPresent();
  const [prompt, setPrompt] = useState('');
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<AiResult | null>(null);
  const [error, setError] = useState<{ message: string; applying?: boolean } | null>(null);
  const [applied, setApplied] = useState(false);
  const [sentPrompt, setSentPrompt] = useState('');
  // The open email is in context unless the user removed its chip; opening a different email brings the chip back.
  const [detachedId, setDetachedId] = useState('');
  const [chipFor, setChipFor] = useState(selected?.id);
  if (selected?.id !== chipFor) { setChipFor(selected?.id); setDetachedId(''); }
  const context = selected && selected.id !== detachedId ? selected : undefined;
  const request = useRef(0);
  const panel = useRef<HTMLElement>(null);
  const focusStart = useEffectEvent(() => panel.current?.querySelector<HTMLElement>(configured ? 'textarea' : '.ai-connect-note button')?.focus());
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    focusStart();
    // Closing discards a request still in flight.
    const requests = request;
    return () => { requests.current++; if (previous?.isConnected) previous.focus(); };
  }, []);
  // A different email in context starts a new conversation and drops any answer still on its way; the request
  // counter is a ref, which render may not touch, so the reset runs here.
  // eslint-disable-next-line react-hooks/set-state-in-effect -- see above
  useEffect(() => { request.current++; setResult(null); setSentPrompt(''); setError(null); setBusy(false); setApplied(false); }, [context?.id]);
  const ask = async (text: string) => {
    if (!configured) { onSettings(); return; }
    const id = ++request.current;
    setBusy(true); setError(null); setApplied(false); setSentPrompt(text); setPrompt(''); setResult(null);
    try { const reply = await post<AiResult>('/v1/ai', { prompt: text, message_id: context?.id || null }); if (id === request.current) setResult(reply); }
    catch (e) { if (id === request.current) setError({ message: errorMessage(e) }); }
    finally { if (id === request.current) setBusy(false); }
  };
  const apply = async () => {
    if (!result) return; setBusy(true); setError(null);
    const remaining = [...result.actions];
    try { while (remaining.length) { await onApply([remaining[0]]); remaining.shift(); } setApplied(true); }
    catch (e) { setResult({ ...result, actions: remaining }); setError({ message: errorMessage(e), applying: true }); }
    finally { setBusy(false); }
  };
  // Slides in from the edge it docks to, on the iOS drawer curve; leaves faster than it arrived.
  return <motion.aside ref={panel} id="alice-assistant" className="assistant-panel" aria-label={t('compose.assistant.label')} inert={!present} initial={{ opacity: 0, transform: travel('translateX(24px)', 'translateX(0px)') }} animate={{ opacity: 1, transform: 'translateX(0px)', transition: { duration: 0.28, ease: easeDrawer } }} exit={{ opacity: 0, transform: travel('translateX(16px)', 'translateX(0px)'), transition: leave }} onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); onClose(); } }}><div className="assistant-header"><span><Sparkles size={18} />Alice</span><button className="icon-button" aria-label={t('compose.assistant.close')} onClick={onClose}><X size={18} /></button></div>
    <div className="assistant-scroll">
      {!configured && <div className="ai-connect-note"><p>{t('compose.assistant.providerNote')}</p><button className="primary" onClick={onSettings}>{t('compose.assistant.connectProvider')}</button><small>{t('compose.assistant.privacyNote')}</small></div>}
      <div className="assistant-suggestions"><button disabled={busy} onClick={() => void ask(t('compose.assistant.prompts.summarize'))}><FileText size={15} /><span>{t('compose.assistant.summarize')}</span></button><button disabled={busy || !context} onClick={() => void ask(t('compose.assistant.prompts.draftReply'))}><Reply size={15} /><span>{t('compose.assistant.draftReply')}</span></button><button disabled={busy || !context} onClick={() => void ask(t('compose.assistant.prompts.addToCalendar'))}><CalendarPlus size={15} /><span>{t('compose.assistant.addToCalendar')}</span></button><button disabled={busy} onClick={() => void ask(t('compose.assistant.prompts.attention'))}><Sparkles size={15} /><span>{t('compose.assistant.attention')}</span></button></div>
      {sentPrompt && <motion.div key={sentPrompt} className="user-prompt" {...rise}>{sentPrompt}</motion.div>}
      {busy && !result && <motion.div className="ai-thinking" role="status" initial={{ opacity: 0 }} animate={{ opacity: 1 }}><LoaderCircle size={15} />{t('compose.assistant.thinking')}</motion.div>}
      {result && <motion.div className="ai-result" {...rise} transition={{ duration: 0.28, ease: easeOut }}><div className="ai-result-label"><Sparkles size={13} />Alice</div><p>{result.text}</p>{result.draft && <div className="ai-draft"><p>{result.draft}</p><button className="text-button" disabled={busy} onClick={() => { if (result.draft) onDraft(result.draft, context); }}><Reply size={14} />{t('compose.assistant.useDraft')}</button></div>}{result.event && <EventCard event={result.event} defaultCalendar={defaultCalendar} />}{result.actions.length > 0 && <div className="ai-plan"><strong>{t('compose.assistant.proposedChanges', { count: result.actions.length })}</strong>{result.actions.map(action => <div key={`${action.message_id}:${action.action}`}><span>{t(`compose.assistant.actions.${action.action}`)}</span><small>{action.subject || t('compose.assistant.selectedMessage')}</small></div>)}<button className="primary" disabled={busy || applied} onClick={() => void apply()}>{applied ? <><Check size={14} />{t('compose.assistant.applied')}</> : t(busy ? 'compose.assistant.applying' : 'compose.assistant.applyChanges')}</button></div>}</motion.div>}
      {error && <motion.p className="form-error" role="alert" {...rise}>{error.applying ? t('compose.assistant.applyFailed', { error: error.message }) : error.message}</motion.p>}
    </div>
    <form className="assistant-input" onSubmit={event => { event.preventDefault(); if (prompt.trim()) void ask(prompt); }}><textarea aria-label={t('compose.assistant.ask')} placeholder={t(context ? 'compose.assistant.askEmail' : 'compose.assistant.askInbox')} value={prompt} disabled={busy} onChange={event => setPrompt(event.target.value)} onKeyDown={event => { if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); if (prompt.trim()) void ask(prompt); } }} />
      <div className="assistant-input-bar">{selected && <button type="button" className={`context-chip ${context ? 'attached' : ''}`} aria-pressed={!!context} aria-label={t('compose.assistant.includeEmail', { subject: selected.subject || t('compose.assistant.noSubject') })} title={t(context ? 'compose.assistant.removeContext' : 'compose.assistant.addContext')} disabled={busy} onClick={() => setDetachedId(context ? selected.id : '')}>{context ? <Mail size={13} /> : <Plus size={13} />}<span>{selected.subject || t('compose.assistant.noSubject')}</span>{context && <X size={13} />}</button>}<button className="icon-button" aria-label={t('compose.assistant.sendRequest')} disabled={busy || !prompt.trim()} type="submit"><ArrowUp size={17} /></button></div></form>
  </motion.aside>;
}
