import { t } from './i18n';
import { useEffect, useMemo, useRef, useState } from 'react';
import { Check, Copy, MoreHorizontal } from 'lucide-react';
import MessageBody from './MessageBody';
import { contentId, emailDocument, splitQuoted } from './emailHtml';
import { attachmentBytes, copyVerificationCode, errorMessage, get, isDesktop, type Message, type MessageFormat } from './api';
import { fromEvent, linkMenu, useMenus, wantsSystemMenu } from './ContextMenu';

const embeddable = /^image\/(png|jpeg|gif|webp)$/;
/** Embedded images already fetched, by message and attachment index, so reopening a message does not fetch them again. */
const fetched = new Map<string, Promise<string>>();
function dataUrl(message: Message, index: number, type: string) {
  const key = `${message.id}:${index}`;
  let url = fetched.get(key);
  if (!url) {
    url = attachmentBytes(message, index).then(bytes => new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(reader.result as string);
      reader.onerror = () => reject(reader.error ?? new Error('read'));
      reader.readAsDataURL(new Blob([bytes], { type }));
    }));
    url.catch(() => fetched.delete(key));
    fetched.set(key, url);
  }
  return url;
}

/**
 * The images an HTML message embeds (`<img src="cid:…">`) as `data:` URLs by Content-ID, fetched from the
 * mail server. They are part of the message, so unlike remote images they load without asking.
 */
function useEmbedded(message: Message, html: string | null) {
  const [embedded, setEmbedded] = useState<Record<string, string>>({});
  useEffect(() => {
    if (!html || !/["']\s*cid:/i.test(html)) return;
    const template = document.createElement('template');
    template.innerHTML = html;
    const wanted = new Set([...template.content.querySelectorAll('img[src]')].map(img => img.getAttribute('src') || '').filter(src => /^cid:/i.test(src)).map(contentId));
    let live = true;
    message.attachments.forEach((file, index) => {
      const type = file.content_type || '';
      if (!file.content_id || !wanted.has(file.content_id) || !embeddable.test(type)) return;
      const id = file.content_id;
      dataUrl(message, index, type).then(url => { if (live) setEmbedded(current => ({ ...current, [id]: url })); }, () => { /* The image stays blank. */ });
    });
    return () => { live = false; };
  }, [message, html]);
  return embedded;
}

/** A reader action, independent of automatic arrival delivery and its freshness window. */
function VerificationCode({ message }: { message: Message }) {
  const [detected, setDetected] = useState<{ id: string; code: string | null; error: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [copyError, setCopyError] = useState('');
  useEffect(() => {
    let live = true;
    get<string | null>(`/v1/messages/${encodeURIComponent(message.id)}/otp`).then(
      code => { if (live) setDetected({ id: message.id, code, error: '' }); },
      (error: unknown) => { if (live) setDetected({ id: message.id, code: null, error: errorMessage(error) }); },
    );
    return () => { live = false; };
  }, [message.id, message.body, message.subject, message.html_body]);
  useEffect(() => {
    if (copied === null) return;
    const timer = setTimeout(() => setCopied(null), 2000);
    return () => clearTimeout(timer);
  }, [copied]);
  if (detected?.id !== message.id) return null;
  if (detected.error) return <p className="form-error" role="alert">{detected.error}</p>;
  if (!detected.code) return null;
  const copy = async () => {
    setBusy(true); setCopyError('');
    try { setCopied(await copyVerificationCode(message.id)); }
    catch (error) { setCopyError(errorMessage(error)); }
    finally { setBusy(false); }
  };
  return <div className="html-controls otp-controls">
    <span>{t('app.verificationCode')}<code>{detected.code}</code></span>
    <button className="text-button" disabled={busy} onClick={() => void copy()} aria-live="polite">{copied === detected.code ? <Check size={16} /> : <Copy size={16} />}{busy ? t('app.copyingCode') : copied === detected.code ? t('app.codeCopied') : t('app.copyCode')}</button>
    {copyError && <span className="form-error" role="alert">{copyError}</span>}
  </div>;
}

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
  const html = format === 'html' ? message.html_body ?? null : null;
  const embedded = useEmbedded(message, html);
  const source = useMemo(() => html ? emailDocument(html, images, showQuoted, embedded) : null, [html, images, showQuoted, embedded]);
  const plain = useMemo(() => splitQuoted(message.body), [message.body]);
  useEffect(() => () => cleanup.current?.(), [source]);
  const quoted = source ? source.quoted : plain.quoted;
  return <div className="message-view">
    {isDesktop && <VerificationCode message={message} />}
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
