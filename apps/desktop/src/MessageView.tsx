import { t } from './i18n';
import { useEffect, useMemo, useRef, useState } from 'react';
import { MoreHorizontal } from 'lucide-react';
import MessageBody from './MessageBody';
import { emailDocument, splitQuoted } from './emailHtml';
import type { Message, MessageFormat } from './api';
import { fromEvent, linkMenu, useMenus, wantsSystemMenu } from './ContextMenu';

/**
 * One message's body in the reader's chosen format (HTML in a sandboxed frame, or plain text).
 * Earlier messages a reply quotes stay folded behind a toggle, as in Gmail.
 */
export default function MessageView({ message, format, loadImages, onOpen }: { message: Message; format: MessageFormat; loadImages: boolean; onOpen: (url: string) => void }) {
  const menus = useMenus();
  const [images, setImages] = useState(loadImages);
  const [showQuoted, setShowQuoted] = useState(false);
  const [height, setHeight] = useState(150);
  const cleanup = useRef<(() => void) | null>(null);
  const html = format === 'html' ? message.html_body : null;
  const source = useMemo(() => html ? emailDocument(html, images, showQuoted) : null, [html, images, showQuoted]);
  const plain = useMemo(() => splitQuoted(message.body), [message.body]);
  useEffect(() => () => cleanup.current?.(), [source]);
  const quoted = source ? source.quoted : plain.quoted;
  return <div className="message-view">
    {source && !images && source.remoteImages > 0 && <div className="html-controls"><span>{t('app.hiddenImages')}</span><button className="text-button" onClick={() => setImages(true)}>{t('app.loadImages')}</button></div>}
    {source ? <iframe title={t('app.emailFrom', { name: message.from_name })} sandbox="allow-same-origin" referrerPolicy="no-referrer" srcDoc={source.document} style={{ height }} onLoad={event => {
      const frame = event.currentTarget;
      cleanup.current?.();
      const doc = frame.contentDocument;
      if (!doc?.body) return;
      // Keep untrusted viewport-based styles from growing the frame without bound; longer mail scrolls inside.
      const resize = () => setHeight(Math.min(12_000, Math.max(60, Math.ceil(doc.body.getBoundingClientRect().height))));
      const observer = new ResizeObserver(resize);
      observer.observe(doc.body);
      const open = (click: MouseEvent) => {
        const anchor = (click.target as Element).closest?.('a[href]');
        if (!anchor) return;
        click.preventDefault();
        const href = anchor.getAttribute('href');
        if (href) onOpen(href);
      };
      // The frame's right-clicks never reach the app: a link gets its own menu, anything else is passed
      // to the frame element so the message's menu answers, and selected text keeps the system menu.
      const menu = (click: MouseEvent) => {
        const target = click.target as Element;
        if (wantsSystemMenu(target)) return;
        click.preventDefault();
        const rect = frame.getBoundingClientRect(), x = click.clientX + rect.left, y = click.clientY + rect.top;
        const href = target.closest?.('a[href]')?.getAttribute('href');
        if (href) menus.open({ x, y, keyboard: false }, linkMenu(href, onOpen, menus), 'link');
        else frame.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: x, clientY: y }));
      };
      doc.addEventListener('click', open, true);
      doc.addEventListener('contextmenu', menu);
      resize();
      cleanup.current = () => { observer.disconnect(); doc.removeEventListener('click', open, true); doc.removeEventListener('contextmenu', menu); };
    }} /> : <MessageBody body={showQuoted ? message.body : plain.main} onOpen={onOpen} onLinkMenu={(event, url) => menus.open(fromEvent(event), linkMenu(url, onOpen, menus), 'link')} />}
    {quoted && <button className="quoted-toggle" aria-expanded={showQuoted} title={showQuoted ? t('app.hideEarlier') : t('app.showEarlier')} onClick={() => setShowQuoted(!showQuoted)}><MoreHorizontal size={16} /><span>{showQuoted ? t('app.hideEarlier') : t('app.showEarlier')}</span></button>}
  </div>;
}
