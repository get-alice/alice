import { t, date, list } from './i18n';
import { useState, type MouseEvent } from 'react';
import { ChevronDown, ChevronUp } from 'lucide-react';
import MessageView from './MessageView';
import Attachments from './Attachments';
import { InviteCard } from './Invitation';
import { splitQuoted } from './emailHtml';
import { avatarColor, initials, relative } from './App';
import type { Message, Preferences } from './api';
import { fromEvent, useMenus, type MenuEntry } from './ContextMenu';

const replyPrefix = /^(?:\s*(?:re|fwd?|aw|wg|sv|vs|antw|tr|rif)\s*(?:\[\d+\])?\s*[:：])+\s*/i;

/**
 * A conversation in the reader. The newest message and any that were unread when it was opened start
 * expanded; the others fold to one line and open on click. `messages` is oldest first. Right-clicking a
 * message opens `menuFor` it, with folding added when there is more than one. An invitation's card sits
 * above its message; replying to it passes the updated message to `onUpdated`.
 */
export default function Conversation({ messages, order, format, loadImages, unreadIds, defaultCalendar, onOpen, onUpdated, menuFor }: { messages: Message[]; order: Preferences['thread_order']; format: Preferences['message_format']; loadImages: boolean; unreadIds: string[]; defaultCalendar: string; onOpen: (url: string) => void; onUpdated: (message: Message) => void; menuFor: (mail: Message, extra: MenuEntry[]) => MenuEntry[] }) {
  const menus = useMenus();
  const [toggled, setToggled] = useState<Record<string, boolean>>({});
  const newest = messages[messages.length - 1];
  const single = messages.length === 1;
  const shown = order === 'newest_first' ? [...messages].reverse() : messages;
  const isOpen = (mail: Message) => single || (toggled[mail.id] ?? (mail.id === newest.id || unreadIds.includes(mail.id)));
  const toggle = (mail: Message) => setToggled(current => ({ ...current, [mail.id]: !isOpen(mail) }));
  const menu = (event: MouseEvent, mail: Message) => menus.open(fromEvent(event), menuFor(mail, single ? [] : [{ label: isOpen(mail) ? t('app.collapseMessage') : t('app.expandMessage'), icon: isOpen(mail) ? ChevronUp : ChevronDown, run: () => toggle(mail) }]), `message:${mail.id}`);
  return <>
    <h2>{messages[0].subject.replace(replyPrefix, '') || messages[0].subject}</h2>
    {!single && <p className="thread-count">{t(order === 'newest_first' ? 'app.threadNewest' : 'app.threadOldest', { count: messages.length })}</p>}
    <div className={`thread ${single ? 'single' : ''}`}>{shown.map(mail => isOpen(mail) ? <section key={mail.id} className="thread-message open" aria-label={t('app.fromName', { name: mail.from_name })} onContextMenu={event => menu(event, mail)}>
      <div className="message-sender" onClick={event => { if (!single && !(event.target as Element).closest('details, a, button')) toggle(mail); }}>
        <div className={`avatar large ${avatarColor(mail.from_name)}`}>{initials(mail.from_name)}</div>
        <div><strong>{single ? mail.from_name : <button className="sender-toggle" aria-expanded="true" title={t('app.collapseMessage')} onClick={() => toggle(mail)}>{mail.from_name}</button>}</strong><details><summary>{t('app.toRecipients', { recipients: list(mail.to) })} <ChevronDown size={14} /></summary><div className="sender-detail">{t('app.fromDetail', { sender: mail.from_email })}<br />{t('app.toDetail', { recipients: list(mail.to) })}<br />{date(mail.date, { dateStyle: 'medium', timeStyle: 'medium' })}</div></details></div>
        <div className="message-time"><time dateTime={mail.date}>{relative(mail.date)}</time><span>{date(mail.date, { weekday: 'short', month: 'short', day: 'numeric' })}</span></div>
      </div>
      {mail.invite && <InviteCard message={mail} invite={mail.invite} defaultCalendar={defaultCalendar} onUpdated={onUpdated} />}
      <MessageView message={mail} format={format} loadImages={loadImages} onOpen={onOpen} />
      <Attachments message={mail} />
    </section> : <button key={mail.id} className={`thread-message collapsed ${mail.is_read ? '' : 'unread'}`} aria-expanded="false" onClick={() => toggle(mail)} onContextMenu={event => menu(event, mail)}>
      <div className={`avatar ${avatarColor(mail.from_name)}`}>{initials(mail.from_name)}</div>
      <span className="collapsed-summary"><strong>{mail.from_name}</strong><span>{splitQuoted(mail.body).main.replace(/\s+/g, ' ').slice(0, 200)}</span></span>
      <time dateTime={mail.date}>{relative(mail.date)}</time>
    </button>)}</div>
  </>;
}
