import { t } from './i18n';
import { useState } from 'react';
import { Check, Download, FileText, FolderOpen, Image, LoaderCircle, Mail } from 'lucide-react';
import { downloadAttachment, errorMessage, formatBytes, isDesktop, revealDownload, type Attachment, type Message } from './api';
import { fromEvent, useMenus } from './ContextMenu';
import { isMac, isWindows } from './platform';

const reveal = () => isMac ? t('app.finder') : isWindows ? t('app.explorer') : t('app.folder');

function Icon({ file }: { file: Attachment }) {
  const type = file.content_type || '';
  if (type.startsWith('image/')) return <Image size={17} />;
  if (type === 'message/rfc822') return <Mail size={17} />;
  return <FileText size={17} />;
}

/** A message's attachments, each fetched from the mail server when chosen and saved to Downloads. */
export default function Attachments({ message }: { message: Message }) {
  const menus = useMenus();
  const [busy, setBusy] = useState<number | null>(null);
  const [saved, setSaved] = useState<Record<number, string>>({});
  const [error, setError] = useState('');
  const download = async (index: number) => {
    setBusy(index); setError('');
    try {
      const result = await downloadAttachment(message, index);
      if (result) setSaved(current => ({ ...current, [index]: result.path }));
    } catch (e) { setError(errorMessage(e)); }
    finally { setBusy(null); }
  };
  if (!message.attachments.length) return null;
  return <div className="attachments">
    <span>{t('app.attachmentsDescription', { count: message.attachments.length })}</span>
    {/* An attachment is addressed by its position in the message, which never changes. */}
    {/* eslint-disable-next-line @eslint-react/no-array-index-key -- see above */}
    {message.attachments.map((file, index) => <div className="attachment-item" key={index}
      onContextMenu={event => menus.open(fromEvent(event), [{ label: isDesktop ? t('app.saveDownloads') : t('app.download'), icon: Download, disabled: busy !== null, run: () => void download(index) }, ...(saved[index] ? [{ label: reveal(), icon: FolderOpen, run: () => void revealDownload(saved[index]).catch((e: unknown) => setError(errorMessage(e))) }] : [])], `attachment:${message.id}:${index}`)}>
      <button className="attachment-info" disabled={busy !== null} aria-label={t('app.downloadFile', { name: file.name, size: formatBytes(file.size) })} title={isDesktop ? t('app.saveDownloads') : t('app.download')} onClick={() => void download(index)}>
        <Icon file={file} />
        <span>{file.name}<small>{formatBytes(file.size)}{saved[index] ? t('app.savedDownloads') : ''}</small></span>
        {busy === index ? <LoaderCircle size={16} className="spinning-icon" aria-label={t('app.downloading')} /> : saved[index] ? <Check size={16} /> : <Download size={16} />}
      </button>
      {saved[index] && <button className="text-button" onClick={() => void revealDownload(saved[index]).catch((e: unknown) => setError(errorMessage(e)))}><FolderOpen size={15} />{reveal()}</button>}
    </div>)}
    {error && <p className="form-error" role="alert">{error}</p>}
  </div>;
}
