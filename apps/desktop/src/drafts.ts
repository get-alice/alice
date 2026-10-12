import type { Compose } from './api';

const prefix = 'alice:draft:';
export interface DraftBackup { draft: Compose; to: string; updatedAt: number }
export function backupDraft(draft: Compose, to: string) {
  localStorage.setItem(prefix + draft.id, JSON.stringify({ draft, to, updatedAt: Date.now() }));
}
export function removeBackup(id: string) { localStorage.removeItem(prefix + id); }
/** Device storage can hold anything (an older format, a hand edit), so a backup is checked field by field. */
function isBackup(entry: unknown, id: string): entry is DraftBackup {
  if (typeof entry !== 'object' || entry === null) return false;
  const { draft, to, updatedAt } = entry as Partial<Record<keyof DraftBackup, unknown>>;
  if (typeof draft !== 'object' || draft === null) return false;
  const fields = draft as Partial<Record<keyof Compose, unknown>>;
  return fields.id === id && typeof fields.account_id === 'string' && Array.isArray(fields.to) && fields.to.every((address: unknown) => typeof address === 'string') && typeof fields.subject === 'string' && typeof fields.body === 'string' && (fields.in_reply_to === null || typeof fields.in_reply_to === 'string') && typeof to === 'string' && typeof updatedAt === 'number';
}
export function readBackup(id: string): DraftBackup | null {
  try {
    const entry: unknown = JSON.parse(localStorage.getItem(prefix + id) || 'null');
    return isBackup(entry, id) ? entry : null;
  } catch { return null; }
}
export function latestBackup(accountIds: string[]): DraftBackup | null {
  try {
    return Object.keys(localStorage).filter(key => key.startsWith(prefix)).map(key => readBackup(key.slice(prefix.length)))
      .filter((entry): entry is DraftBackup => !!entry && accountIds.includes(entry.draft.account_id))
      .sort((a, b) => b.updatedAt - a.updatedAt)[0] || null;
  } catch { return null; }
}
