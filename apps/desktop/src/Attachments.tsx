import { t } from './i18n';
import { useEffect, useState } from 'react';
import { Check, Download, ExternalLink, FileText, FolderOpen, Image, LoaderCircle, Mail } from 'lucide-react';
import { errorMessage, formatBytes, isDesktop, openAttachment, revealDownload, savedAttachments, type Attachment, type Message } from './api';
import { fromEvent, useMenus } from './ContextMenu';
import { isMac, isWindows } from './platform';

const reveal = () => isMac ? t('app.finder') : isWindows ? t('app.explorer') : t('app.folder');

function Icon({ file }: { file: Attachment }) {
  const type = file.content_type || '';
  if (type.startsWith('image/')) return <Image size={17} />;
  if (type === 'message/rfc822') return <Mail size={17} />;
  return <FileText size={17} />;
}

/**
 * A message's attachments. On the desktop, choosing one fetches it from the mail server into
 * Downloads the first time and opens that file from then on; the browser downloads it.
 */
export default function Attachments({ message }: { message: Message }) {
  const menus = useMenus();
  const [busy, setBusy] = useState<number | null>(null);
  const [saved, setSaved] = useState<Record<number, string>>({});
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const desktopFiles = isDesktop && message.attachments.length > 0;
  useEffect(() => {
    if (!desktopFiles) return;
    let live = true;
    savedAttachments(message.id).then(found => { if (live) setSaved(current => ({ ...found, ...current })); }, () => {});
    return () => { live = false; };
  }, [message.id, desktopFiles]);
  const open = async (index: number) => {
    setBusy(index); setError(''); setNotice('');
    try {
      const result = await openAttachment(message, index);
      if (result) {
        setSaved(current => ({ ...current, [index]: result.path }));
        if (result.shown) setNotice(t('app.programShown', { name: result.name }));
      }
    } catch (e) { setError(errorMessage(e)); }
    finally { setBusy(null); }
  };
  const show = (path: string) => void revealDownload(path).catch((e: unknown) => setError(errorMessage(e)));
  if (!message.attachments.length) return null;
  const action = isDesktop ? t('app.openAttachment') : t('app.download');
  return <div className="attachments">
    <span>{t('app.attachmentsDescription', { count: message.attachments.length })}</span>
    {/* An attachment is addressed by its position in the message, which never changes. */}
    {/* eslint-disable-next-line @eslint-react/no-array-index-key -- see above */}
    {message.attachments.map((file, index) => <div className="attachment-item" key={index}
      onContextMenu={event => menus.open(fromEvent(event), [{ label: action, icon: isDesktop ? ExternalLink : Download, disabled: busy !== null, run: () => void open(index) }, ...(saved[index] ? [{ label: reveal(), icon: FolderOpen, run: () => show(saved[index]) }] : [])], `attachment:${message.id}:${index}`)}>
      <button className="attachment-info" disabled={busy !== null} aria-label={t(isDesktop ? 'app.openFile' : 'app.downloadFile', { name: file.name, size: formatBytes(file.size) })} title={action} onClick={() => void open(index)}>
        <Icon file={file} />
        <span>{file.name}<small>{formatBytes(file.size)}{saved[index] ? t('app.savedDownloads') : ''}</small></span>
        {busy === index ? <LoaderCircle size={16} className="spinning-icon" aria-label={t('app.downloading')} /> : saved[index] ? <Check size={16} /> : <Download size={16} />}
      </button>
      {saved[index] && <button className="text-button" onClick={() => show(saved[index])}><FolderOpen size={15} />{reveal()}</button>}
    </div>)}
    {notice && <p className="attachment-notice" role="status">{notice}</p>}
    {error && <p className="form-error" role="alert">{error}</p>}
  </div>;
}
