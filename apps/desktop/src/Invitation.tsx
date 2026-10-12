import { useEffect, useState, type ReactNode } from 'react';
import { motion } from 'motion/react';
import { ArrowUpRight, Check, CalendarArrowUp, CalendarPlus, CircleHelp, Download, LoaderCircle, MapPin, Repeat, Users, X } from 'lucide-react';
import { errorMessage, get, isDesktop, openEmailLink, openEventFile, post, type CalendarList, type CreatedEvent, type EventDraft, type Message, type Rsvp } from './api';
import { Glide, rise } from './motion';
import { date, dateFormat, number, t, useLocale } from './i18n';

/*
 * Calendar events in mail: an invitation's card above the message, and an event Alice proposes. Alice
 * hands events to a calendar (a connected provider or CalDAV calendar, or the system calendar app);
 * it never stores or shows a calendar itself.
 */

let calendars: Promise<CalendarList> | null = null;
/** The calendars events can be added to, listed once per session; `fresh` after access changed in Settings. */
export function loadCalendars(fresh = false) {
  if (fresh || !calendars) calendars = get<CalendarList>('/v1/calendars').catch((error: unknown) => { calendars = null; throw error; });
  return calendars;
}

const dateOnly = (value: string) => { const [year, month, day] = value.slice(0, 10).split('-').map(Number); return new Date(year, month - 1, day); };
const valid = (date: Date) => !Number.isNaN(date.getTime());

/** When an event happens, in the viewer's time zone: its first day, a date line and a time line. */
export function eventWhen(event: EventDraft): { day: Date | null; date: string; time: string } {
  const year = (date: Date) => date.getFullYear() !== new Date().getFullYear() ? 'numeric' as const : undefined;
  if (event.all_day) {
    const start = dateOnly(event.start);
    if (!valid(start)) return { day: null, date: event.start, time: t('invite.allDay') };
    // All-day ends are exclusive: an event on the 14th ends on the 15th.
    const last = event.end ? dateOnly(event.end) : start;
    if (event.end && valid(last)) last.setDate(last.getDate() - 1);
    if (valid(last) && last > start) return { day: start, date: dateFormat({ weekday: 'short', month: 'short', day: 'numeric', year: year(last) }).formatRange(start, last), time: t('invite.allDay') };
    return { day: start, date: date(start, { weekday: 'long', month: 'long', day: 'numeric', year: year(start) }), time: t('invite.allDay') };
  }
  const start = new Date(event.start);
  if (!valid(start)) return { day: null, date: event.start, time: '' };
  const end = event.end ? new Date(event.end) : null;
  const ends = end && valid(end) && end > start ? end : null;
  if (!ends || ends.toDateString() === start.toDateString()) {
    const time = dateFormat({ hour: 'numeric', minute: '2-digit', timeZoneName: 'short' });
    return { day: start, date: date(start, { weekday: 'long', month: 'long', day: 'numeric', year: year(start) }), time: ends ? time.formatRange(start, ends) : time.format(start) };
  }
  return { day: start, date: dateFormat({ weekday: 'short', month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit', year: year(ends), timeZoneName: 'short' }).formatRange(start, ends), time: '' };
}

const frequencies: Record<string, 'daily' | 'weekly' | 'monthly' | 'yearly'> = { DAILY: 'daily', WEEKLY: 'weekly', MONTHLY: 'monthly', YEARLY: 'yearly' };
/** `RRULE:FREQ=WEEKLY;INTERVAL=2` → `Repeats every 2 weeks`. */
export function repeats(rule: string) {
  const parts = Object.fromEntries(rule.replace(/^RRULE:/i, '').split(';').map(part => part.split('=') as [string, string]).map(([key, value]) => [key?.toUpperCase(), value?.toUpperCase()]));
  const frequency = frequencies[parts.FREQ];
  if (!frequency) return t('invite.repeat.unknown');
  const interval = Number(parts.INTERVAL);
  const count = parts.COUNT ? Number(parts.COUNT) : undefined;
  const variant = count === undefined ? interval > 1 ? 'interval' : 'simple' : interval > 1 ? 'intervalCount' : 'count';
  return t(`invite.repeat.${frequency}.${variant}`, { interval, ...(count === undefined ? {} : { count }) });
}

/** A page from a desk calendar: the event's month and day. */
function DateTile({ day, cancelled }: { day: Date | null; cancelled?: boolean }) {
  return <div className={`event-tile ${cancelled ? 'cancelled' : ''}`} aria-hidden="true">{day ? <><span>{date(day, { month: 'short' })}</span><strong>{number(day.getDate())}</strong></> : <CalendarPlus size={20} />}</div>;
}

function EventSummary({ event, cancelled, children }: { event: EventDraft; cancelled?: boolean; children?: ReactNode }) {
  const when = eventWhen(event);
  return <div className="event-summary"><DateTile day={when.day} cancelled={cancelled} /><div className="event-details">
    <strong className="event-title">{event.title || t('invite.untitledEvent')}</strong>
    <p className="event-when">{when.date}{when.time && <span>{when.time}</span>}</p>
    {event.location && <p className="event-meta"><MapPin size={14} aria-hidden="true" /><span>{event.location}</span></p>}
    {children}
  </div></div>;
}

const answers: { id: Rsvp; icon: typeof Check }[] = [
  { id: 'accepted', icon: Check },
  { id: 'tentative', icon: CircleHelp },
  { id: 'declined', icon: X },
];

/** The invitation `message` carries, with the reply and add-to-calendar controls. `onUpdated` receives the message after a reply. */
export function InviteCard({ message, invite, defaultCalendar, onUpdated }: { message: Message; invite: NonNullable<Message['invite']>; defaultCalendar: string; onUpdated: (message: Message) => void }) {
  useLocale();
  const [sending, setSending] = useState<Rsvp | null>(null);
  const [error, setError] = useState('');
  const canReply = invite.method.toUpperCase() === 'REQUEST' && !invite.cancelled;
  const organizer = invite.organizer ? invite.organizer.name || invite.organizer.email : '';
  const guests = invite.attendees.length;
  const reply = async (response: Rsvp) => {
    setSending(response); setError('');
    try { onUpdated(await post<Message>(`/v1/messages/${encodeURIComponent(message.id)}/rsvp`, { response })); }
    catch (e) { setError(errorMessage(e)); }
    finally { setSending(null); }
  };
  const answered = answers.find(answer => answer.id === invite.response);
  return <section className={`event-card invite-card ${invite.cancelled ? 'cancelled' : ''}`} aria-label={t('invite.invitation', { title: invite.event.title || t('invite.untitledEventLabel') })}>
    <EventSummary event={invite.event} cancelled={invite.cancelled}>
      {(organizer || guests > 0 || invite.recurrence) && <p className="event-meta">{guests > 0 ? <Users size={14} aria-hidden="true" /> : invite.recurrence ? <Repeat size={14} aria-hidden="true" /> : null}<span>{[organizer && t('invite.organizedBy', { organizer }), guests > 0 && t('invite.guests', { count: guests }), invite.recurrence && repeats(invite.recurrence)].filter(Boolean).join(' · ')}</span></p>}
    </EventSummary>
    {invite.cancelled ? <p className="event-status cancelled" role="status"><X size={14} />{organizer ? t('invite.cancelledBy', { organizer }) : t('invite.cancelled')}</p>
      : <>
        {canReply && <div className="event-reply"><span className="event-reply-label" id={`rsvp-${message.id}`} role="status">{sending ? t('invite.sendingReply') : answered ? t(`invite.answers.${answered.id}.done`) : t('invite.going')}</span><div className="segmented-control rsvp-options" role="group" aria-labelledby={`rsvp-${message.id}`}>{answers.map(answer => <button key={answer.id} aria-pressed={invite.response === answer.id} className={invite.response === answer.id ? 'selected' : ''} disabled={!!sending} onClick={() => { if (invite.response !== answer.id) void reply(answer.id); }}>{invite.response === answer.id && <Glide group={`rsvp-${message.id}`} />}{sending === answer.id ? <LoaderCircle size={14} className="spinning-icon" /> : <answer.icon size={14} />}{t(`invite.answers.${answer.id}.label`)}</button>)}</div></div>}
        {!canReply && answered && <p className="event-status" role="status"><answered.icon size={14} />{t(`invite.answers.${answered.id}.done`)}</p>}
        <AddToCalendar source={{ message_id: message.id }} defaultCalendar={defaultCalendar} />
      </>}
    {error && <motion.p className="form-error" role="alert" {...rise}>{error}</motion.p>}
  </section>;
}

/** An event Alice proposes from an email, to add after review. */
export function EventCard({ event, defaultCalendar }: { event: EventDraft; defaultCalendar: string }) {
  useLocale();
  return <section className="event-card proposed" aria-label={t('invite.proposedEvent', { title: event.title || t('invite.untitledEventLabel') })}>
    <EventSummary event={event}>{event.description && <p className="event-description">{event.description}</p>}</EventSummary>
    <AddToCalendar source={{ event }} defaultCalendar={defaultCalendar} />
  </section>;
}

/**
 * Adds an event to a connected calendar (one click, the default calendar preselected), or hands it to
 * the system calendar app as an .ics file.
 */
export function AddToCalendar({ source, defaultCalendar }: { source: { message_id: string } | { event: EventDraft }; defaultCalendar: string }) {
  useLocale();
  const [list, setList] = useState<CalendarList | null>(null);
  const [calendar, setCalendar] = useState('');
  const [busy, setBusy] = useState<'' | 'add' | 'open'>('');
  const [added, setAdded] = useState<{ name: string; url?: string | null } | null>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    loadCalendars().then(result => {
      if (!active) return;
      setList(result);
      const preferred = result.calendars.find(c => c.id === defaultCalendar) ?? result.calendars.find(c => c.primary) ?? result.calendars[0];
      setCalendar(preferred?.id || '');
    }).catch(() => { if (active) setList({ calendars: [], errors: [] }); });
    return () => { active = false; };
  }, [defaultCalendar]);
  const chosen = list?.calendars.find(c => c.id === calendar);
  const add = async () => {
    if (!chosen) return;
    setBusy('add'); setError('');
    try { const created = await post<CreatedEvent>('/v1/calendar/events', { calendar: chosen.id, ...source }); setAdded({ name: chosen.name, url: created.url }); }
    catch (e) { setError(errorMessage(e)); }
    finally { setBusy(''); }
  };
  const open = async () => {
    setBusy('open'); setError('');
    try { await openEventFile(source); }
    catch (e) { setError(errorMessage(e)); }
    finally { setBusy(''); }
  };
  const many = (list?.calendars.length || 0) > 1;
  return <div className="event-actions">
    {added ? <motion.p className="event-added" role="status" {...rise}><Check size={15} />{t('invite.addedTo', { calendar: added.name })}{added.url && <button className="text-button" onClick={() => { if (added.url) void openEmailLink(added.url).catch((e: unknown) => setError(errorMessage(e))); }}>{t('invite.viewEvent')}<ArrowUpRight size={13} /></button>}</motion.p>
      : list && list.calendars.length > 0 && <div className="event-add">
        {many && <select aria-label={t('invite.calendar')} value={calendar} disabled={!!busy} onChange={event => setCalendar(event.target.value)}>{list.calendars.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}</select>}
        <button className="secondary-button" disabled={!!busy || !chosen} title={chosen && !many ? t('invite.addTo', { calendar: chosen.name }) : undefined} onClick={() => void add()}>{busy === 'add' ? <LoaderCircle size={15} className="spinning-icon" /> : <CalendarPlus size={15} />}{t(busy === 'add' ? 'invite.adding' : many ? 'invite.add' : 'invite.addToCalendar')}</button>
      </div>}
    <button className="text-button" disabled={!!busy} onClick={() => void open()}>{busy === 'open' ? <LoaderCircle size={14} className="spinning-icon" /> : isDesktop ? <CalendarArrowUp size={14} /> : <Download size={14} />}{t(isDesktop ? 'invite.openCalendarApp' : 'invite.downloadIcs')}</button>
    {error && <motion.p className="form-error" role="alert" {...rise}>{error}</motion.p>}
  </div>;
}
