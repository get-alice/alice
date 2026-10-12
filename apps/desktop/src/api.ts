import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { t, locale, number, type LanguagePreference } from './i18n';
import { isMac } from './platform';

/** Which provider APIs Alice may use for this account (Gmail and Outlook sign-ins only). */
export interface Integrations { contacts: boolean; calendar: boolean }
export interface Account {
  id: string; name: string; email: string; provider: 'gmail' | 'outlook' | 'imap';
  last_synced: string | null; sync_error: string | null; integrations?: Integrations;
}
/** `part` is the attachment's section on the mail server; mail cached before sections were recorded has none. */
/** `content_id` names an image the HTML body shows inline as `cid:…`. */
export interface Attachment { name: string; size: number; part?: string | null; content_type?: string | null; content_id?: string | null }
export interface Message {
  id: string; account_id: string; from_name: string; from_email: string; to: string[]; reply_to: string[];
  subject: string; preview: string; body: string; html_body?: string | null; date: string; folder: string;
  is_read: boolean; is_starred: boolean; category: string;
  attachments: Attachment[]; message_id: string | null; in_reply_to: string | null; invite?: Invite | null;
}
export type Rsvp = 'accepted' | 'tentative' | 'declined';
/** Timed events have RFC 3339 UTC `start`/`end`; all-day events have `YYYY-MM-DD` dates with an exclusive `end`. */
export interface EventDraft { title: string; start: string; end?: string | null; all_day: boolean; location?: string | null; description?: string | null }
export interface Attendee { email: string; name?: string | null; status?: Rsvp | null }
/** A calendar invitation a message carries. `response` is this account's answer. */
export interface Invite { method: string; uid: string; sequence: number; event: EventDraft; organizer?: Attendee | null; attendees: Attendee[]; recurrence?: string | null; cancelled: boolean; response?: Rsvp | null; ics: string }
export type ContactSource = 'mail' | 'google' | 'microsoft' | 'carddav' | 'system';
/** A suggested recipient; `name` is empty when no source knows one. */
export interface Contact { name: string; email: string; sources: ContactSource[] }
export interface SourceError { source: string; message: string }
export interface ContactsSync { contacts: number; errors: SourceError[] }
export type DavKind = 'carddav' | 'caldav';
export interface DavSource { id: string; kind: DavKind; name: string; url: string; username: string; last_synced?: string | null; error?: string | null }
export interface Calendar { id: string; name: string; source: 'google' | 'microsoft' | 'caldav'; owner: string; primary: boolean }
export interface CalendarList { calendars: Calendar[]; errors: SourceError[] }
export interface CreatedEvent { calendar: string; url?: string | null }
export interface EventFile { filename: string; ics: string }
export interface Compose { id: string; account_id: string; to: string[]; subject: string; body: string; in_reply_to: string | null }
export interface AiSettings { provider: string; model: string; base_url: string; configured: boolean }
/** A conversation in a list; see `ThreadSummary` in the daemon. `message` is its newest message in the view. */
export interface Thread { id: string; message: Message; ids: string[]; unread_ids: string[]; count: number; participants: string[]; is_starred: boolean }
export interface ThreadPage { threads: Thread[]; total: number; all_total: number; unread_total: number; revision: string }
export type MessageFormat = 'html' | 'plain';
/** How far back mail is kept on this device. */
export type SyncWindow = '30_days' | '90_days' | '1_year' | 'all';
export interface Preferences { language: LanguagePreference; theme: 'light' | 'dark'; mail_layout: 'single' | 'horizontal' | 'vertical'; show_avatars: boolean; density: 'comfortable' | 'compact'; message_format: MessageFormat; remote_images: 'ask' | 'always'; thread_order: 'newest_first' | 'oldest_first'; sync_window: SyncWindow; signature: string; default_account_id: string; notifications: boolean; otp_autocopy: boolean; menu_bar: boolean; default_calendar: string; system_contacts: boolean; mcp: 'off' | 'read_only' | 'full' }
export const defaultPreferences: Preferences = { language: 'system', theme: 'light', mail_layout: 'single', show_avatars: true, density: 'comfortable', message_format: 'html', remote_images: 'ask', thread_order: 'newest_first', sync_window: '90_days', signature: '', default_account_id: '', notifications: true, otp_autocopy: false, menu_bar: true, default_calendar: '', system_contacts: false, mcp: 'off' };
export interface Health { status: string; version: string }
/** `revision` changes whenever the daemon's stored data may have changed; `syncing` while any account is syncing; `unread_count` is unread Inbox conversations. */
export interface Snapshot { accounts: Account[]; unread_count: number; ai: AiSettings; preferences: Preferences; syncing: boolean; revision: string }
/** Disk use of an account's cached mail by folder, and how much of its sync window is on this device. */
export interface AccountStorage {
  account_id: string; messages: number; bytes: number;
  folders: { folder: string; messages: number; bytes: number }[];
  sync: { active: boolean; downloaded: number; total: number; last_synced: string | null; error: string | null };
}
export interface StorageReport { database_bytes: number; accounts: AccountStorage[] }
export interface UpdateCheck { status: 'unavailable' | 'error' | 'available' | 'current'; message?: string; version?: string; url?: string }
export interface AiResult { text: string; draft: string | null; actions: { message_id: string; action: 'archive' | 'mark_read' | 'star'; subject?: string }[]; event?: EventDraft | null }
export type Patch = Partial<Pick<Message, 'folder' | 'is_read' | 'is_starred'>>;

/** The daemon's error body: `{"error": {"message": "…"}}`. */
type ErrorBody = { error?: { message?: string } } | null;

export async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
  if (isTauri()) return invoke<T>('daemon_request', { method, path, body: body ?? null, locale: locale() });
  const response = await fetch(`/api${path}`, { method, headers: { 'Accept-Language': locale(), ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) }, body: body === undefined ? undefined : JSON.stringify(body) });
  const result = await response.json().catch(() => null) as unknown;
  if (!response.ok) throw Object.assign(new Error((result as ErrorBody)?.error?.message || t('app.starting')), { status: response.status });
  return result as T;
}
/** What failed, for people: Tauri commands reject with a string, everything else with an `Error`. */
export function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : typeof error === 'object' && error !== null && 'message' in error && typeof error.message === 'string' ? error.message : String(error);
}
export function isNotFound(error: unknown) { return typeof error === 'object' && error !== null && 'status' in error && error.status === 404; }
export const get = <T,>(path: string) => api<T>('GET', path);
export const post = <T,>(path: string, body: unknown) => api<T>('POST', path, body);
export const patch = (id: string, body: Patch) => api<Message>('PATCH', `/v1/messages/${encodeURIComponent(id)}`, body);
/** Uses the native, same-device clipboard path; never the WebView's ordinary clipboard. */
export const copyVerificationCode = (messageId: string) => invoke<string>('copy_message_otp', { messageId });
export async function openAuth(url: string) {
  if (isTauri()) await invoke('open_auth_url', { url });
  else window.open(url, '_blank', 'noopener,noreferrer');
}
export async function openHelp(url: string) {
  if (isTauri()) await invoke('open_help_url', { url });
  else window.open(url, '_blank', 'noopener,noreferrer');
}
export async function openEmailLink(url: string) {
  const parsed = new URL(url);
  if (!['http:', 'https:'].includes(parsed.protocol) || !parsed.hostname || parsed.username || parsed.password) throw new Error(t('app.unsafeLink'));
  if (isTauri()) await invoke('open_email_url', { url });
  else window.open(url, '_blank', 'noopener,noreferrer');
}
/**
 * Opens an attachment. The desktop app saves it in Downloads the first time, opens that file with
 * its default app (programs and scripts are `shown` in the file manager instead) and returns where
 * it is; the browser starts a normal download and returns null.
 */
export async function openAttachment(message: Message, index: number): Promise<{ path: string; name: string; shown: boolean } | null> {
  if (isTauri()) return invoke<{ path: string; name: string; shown: boolean }>('open_attachment', { messageId: message.id, index, locale: locale() });
  const response = await fetch(`/api/v1/messages/${encodeURIComponent(message.id)}/attachments/${index}`, { headers: { 'Accept-Language': locale() } });
  if (!response.ok) throw new Error((await response.json().catch(() => null) as ErrorBody)?.error?.message || t('app.downloadFailed'));
  const name = decodeURIComponent(response.headers.get('X-Alice-Filename') || '') || message.attachments[index]?.name || t('app.attachmentFilename');
  const url = URL.createObjectURL(await response.blob());
  const link = Object.assign(document.createElement('a'), { href: url, download: name });
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
  return null;
}
/** The bytes of an attachment, fetched from the mail server; for images embedded in HTML mail. */
export async function attachmentBytes(message: Message, index: number): Promise<ArrayBuffer> {
  if (isTauri()) return invoke<ArrayBuffer>('read_attachment', { messageId: message.id, index });
  const response = await fetch(`/api/v1/messages/${encodeURIComponent(message.id)}/attachments/${index}`, { headers: { 'Accept-Language': locale() } });
  if (!response.ok) throw new Error((await response.json().catch(() => null) as ErrorBody)?.error?.message || t('app.downloadFailed'));
  return response.arrayBuffer();
}
/**
 * Hands an event to the system calendar app as an .ics file: an invitation by `message_id`, or `event`.
 * The desktop app opens it with the default calendar app; the browser downloads it.
 */
export async function openEventFile(source: { message_id: string } | { event: EventDraft }) {
  const file = await post<EventFile>('/v1/calendar/ics', source);
  if (isTauri()) { await invoke('open_calendar_file', { filename: file.filename, ics: file.ics }); return; }
  const url = URL.createObjectURL(new Blob([file.ics], { type: 'text/calendar' }));
  Object.assign(document.createElement('a'), { href: url, download: file.filename }).click();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}
/**
 * Reads the Mac address book (asking for access the first time) and replaces the daemon's copy of it.
 * Returns how many addresses were imported.
 */
export async function importSystemContacts() {
  const contacts = await invoke<{ name: string; email: string }[]>('read_system_contacts');
  const imported = (await api<{ contacts: number }>('PUT', '/v1/contacts/system', { contacts })).contacts;
  systemContactsImportedAt = Date.now();
  return imported;
}
/** When Mac Contacts were last imported in this session (ms since the epoch), so turning them on does not import twice. */
export let systemContactsImportedAt = 0;
/** Where the desktop app saved a message's attachments in Downloads, by index, for files still there. */
export async function savedAttachments(messageId: string) {
  return invoke<Record<number, string>>('saved_attachments', { messageId });
}
/** Shows a saved attachment in Finder or Explorer. */
export async function revealDownload(path: string) {
  await invoke('reveal_download', { path });
}
/** A byte count for people: `840 KB`, `12.4 MB`. */
export function formatBytes(bytes: number) {
  if (bytes < 1000) return t('app.bytes', { value: bytes });
  const units = ['app.kilobytes', 'app.megabytes', 'app.gigabytes', 'app.terabytes'] as const;
  let value = bytes / 1000, unit = 0;
  while (value >= 1000 && unit < units.length - 1) { value /= 1000; unit++; }
  return t(units[unit], { value: value < 10 ? new Intl.NumberFormat(locale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(value) : number(Math.round(value)) });
}
/** Running in the native app, which has notifications and a menu bar icon. */
export const isDesktop = isTauri();
/** The native macOS app, which can read Mac Contacts. */
export const isMacDesktop = isDesktop && isMac;
/** What a clicked new-mail notification asks to show: a message, or the Inbox when `message_id` is null. */
export interface Opened { message_id: string | null }
/** Calls `open` for each clicked notification, including one that launched the app. Returns a cleanup. */
export function onNotificationOpen(open: (opened: Opened) => void) {
  if (!isDesktop) return () => {};
  const take = () => { void invoke<Opened | null>('take_opened').then(opened => { if (opened) open(opened); }).catch(() => {}); };
  const stop = listen('open-message', take);
  take();
  return () => { void stop.then(unlisten => unlisten()); };
}
