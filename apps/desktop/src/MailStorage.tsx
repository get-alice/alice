import { t, date, number } from './i18n';
import { useEffect, useState } from 'react';
import { HardDrive } from 'lucide-react';
import { errorMessage, formatBytes, get, type Account, type StorageReport, type SyncWindow } from './api';
import { Glide } from './motion';

const windows = (): [SyncWindow, string][] => [['30_days', t('settings.30_days')], ['90_days', t('settings.90_days')], ['1_year', t('settings.1_year')], ['all', t('settings.all_mail')]];
const folderNames = () => ({ inbox: t('common.folder.inbox'), starred: t('common.folder.starred'), sent: t('common.folder.sent'), drafts: t('common.folder.drafts'), archive: t('common.folder.archive'), trash: t('common.folder.trash'), spam: t('common.folder.spam') });

function since(value: string | null) {
  if (!value) return t('settings.not_synced_yet');
  const minutes = Math.round((Date.now() - new Date(value).getTime()) / 60_000);
  return minutes < 1 ? t('settings.synced_just_now') : minutes < 60 ? t('settings.storage_synced_minutes', { count: minutes }) : t('settings.storage_synced_at', { date: date(value, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' }) });
}

/** How far back mail is kept on this device, and the space each account's mail takes. */
export default function MailStorage({ accounts, window, onWindow }: { accounts: Account[]; window: SyncWindow; onWindow: (window: SyncWindow) => void }) {
  const [report, setReport] = useState<StorageReport | null>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    const load = () => get<StorageReport>('/v1/storage').then(next => { if (active) { setReport(next); setError(''); } }).catch((e: unknown) => { if (active) setError(errorMessage(e)); });
    void load();
    // Progress moves while older mail downloads.
    const timer = setInterval(() => { if (!document.hidden) void load(); }, 3000);
    return () => { active = false; clearInterval(timer); };
  }, []);
  const folderLabels = folderNames();
  return <section className="preference-section mail-storage">
    <h3>{t('settings.mail_on_this_device')}</h3>
    <p>{t('settings.alice_keeps_recent_mail_here_so')}</p>
    <div className="preference-row"><div><span className="preference-label">{t('settings.keep_mail_from_the_last')}</span><p>{t('settings.widening_downloads_older_mail_in_the')}</p></div>
      <div className="segmented-control" role="group" aria-label={t('settings.keep_mail_from_the_last')}>{windows().map(([id, label]) => <button key={id} aria-pressed={window === id} className={window === id ? 'selected' : ''} onClick={() => onWindow(id)}>{window === id && <Glide group="sync-window" />}{label}</button>)}</div>
    </div>
    {error && <p className="form-error" role="alert">{error}</p>}
    {report && <div className="storage-list">{accounts.map(account => {
      const usage = report.accounts.find(a => a.account_id === account.id);
      if (!usage) return null;
      const { sync } = usage;
      const downloading = sync.total > 0 && sync.downloaded < sync.total;
      return <div key={account.id} className="storage-account">
        <div className="storage-heading"><span><strong>{account.name}</strong><small>{account.email}</small></span><span className="storage-size">{formatBytes(usage.bytes)}</span></div>
        <p className="storage-status" role="status">{sync.error ? <span className="storage-error">{sync.error}</span>
          : downloading ? <>{sync.active ? t('settings.downloading_mail') : t('settings.waiting_to_download')} · {t('settings.download_progress', { downloaded: sync.downloaded, total: sync.total })}</>
          : <>{t('settings.message_count', { count: usage.messages })} · {sync.active ? t('settings.checking_for_new_mail') : since(sync.last_synced)}</>}</p>
        {downloading && <div className="storage-progress" role="progressbar" aria-label={t('settings.downloading_account', { email: account.email })} aria-valuemin={0} aria-valuemax={sync.total} aria-valuenow={sync.downloaded}><span style={{ width: `${Math.max(2, (sync.downloaded / sync.total) * 100)}%` }} /></div>}
        {usage.folders.length > 0 && <ul className="storage-folders">{usage.folders.map(folder => <li key={folder.folder}><span>{folderLabels[folder.folder as keyof typeof folderLabels] || folder.folder}</span><span>{number(folder.messages)} · {formatBytes(folder.bytes)}</span></li>)}</ul>}
      </div>;
    })}</div>}
    {report && <p className="preference-note storage-total"><HardDrive size={15} />{t('settings.database_size', { size: formatBytes(report.database_bytes) })}</p>}
  </section>;
}
