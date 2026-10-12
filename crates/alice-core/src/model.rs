use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Gmail,
    Outlook,
    Imap,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub email: String,
    pub provider: Provider,
    pub is_demo: bool,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    #[serde(default)]
    pub folders: Mailboxes,
    pub last_synced: Option<String>,
    pub sync_error: Option<String>,
    /// Contacts and calendar access granted beyond mail. Only Gmail and Outlook OAuth accounts can
    /// grant them; other accounts use CardDAV and CalDAV sources.
    #[serde(default)]
    pub integrations: Integrations,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mailboxes {
    pub inbox: String,
    pub sent: String,
    pub archive: String,
    pub trash: String,
    /// Existing provider Junk mailbox; empty when the server has no discoverable spam folder.
    #[serde(default = "default_spam_mailbox")]
    pub spam: String,
}
impl Default for Mailboxes {
    fn default() -> Self {
        Self {
            inbox: "INBOX".into(),
            sent: "Sent".into(),
            archive: "Archive".into(),
            trash: "Trash".into(),
            spam: default_spam_mailbox(),
        }
    }
}
fn default_spam_mailbox() -> String {
    "Junk".into()
}

/// Provider APIs an OAuth account may use besides mail. Each needs its own consent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Integrations {
    /// Read the provider's address book: Google People, Microsoft Graph contacts.
    pub contacts: bool,
    /// Read busy times and add events: Google Calendar, Microsoft Graph calendars.
    pub calendar: bool,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrationsPatch {
    pub contacts: Option<bool>,
    pub calendar: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    /// Decoded size in bytes (estimated from the encoded size when it has not been downloaded).
    pub size: usize,
    /// IMAP body section that holds the attachment, such as `2` or `1.3`. Mail cached before
    /// sections were recorded has none; its attachments are found by position instead.
    #[serde(default)]
    pub part: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    /// `Content-ID` of an image the HTML body shows inline (`<img src="cid:…">`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub account_id: String,
    pub from_name: String,
    pub from_email: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub reply_to: Vec<String>,
    pub subject: String,
    pub preview: String,
    pub body: String,
    #[serde(default)]
    pub html_body: Option<String>,
    pub date: String,
    /// Logical folder; `spam` is quarantined from normal conversation and recipient-suggestion views.
    pub folder: String,
    pub is_read: bool,
    pub is_starred: bool,
    pub category: String,
    pub attachments: Vec<Attachment>,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub remote_mailbox: Option<String>,
    pub remote_uid: Option<u32>,
    pub uid_validity: Option<u32>,
    /// The calendar invitation this message carries, if any.
    #[serde(default)]
    pub invite: Option<Invite>,
}
impl Message {
    pub fn reply_recipients(&self) -> Vec<String> {
        if self.reply_to.is_empty() {
            vec![self.from_email.clone()]
        } else {
            self.reply_to.clone()
        }
    }
}

#[derive(Default, Clone, Deserialize, Serialize)]
pub struct MessageQuery {
    pub folder: Option<String>,
    pub account_id: Option<String>,
    pub q: Option<String>,
    pub unread: Option<bool>,
    pub category: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}
#[derive(Serialize, Deserialize)]
pub struct MessagePage {
    pub messages: Vec<Message>,
    pub total: usize,
    #[serde(default)]
    pub all_total: usize,
    #[serde(default)]
    pub unread_total: usize,
    /// The store revision these rows were read at; see `Store::revision`.
    #[serde(default)]
    pub revision: String,
}

/// One conversation in a list: messages of an account that share a subject once reply and forward
/// prefixes are removed. `message` is its newest message in the listed view.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThreadSummary {
    /// Stable while the conversation grows: account and normalized subject.
    pub id: String,
    pub message: Message,
    /// The conversation's messages in the listed view, newest first; moving the conversation moves these.
    pub ids: Vec<String>,
    /// Every unread message of the conversation: normal mail excludes Trash and Spam; Spam stays isolated.
    pub unread_ids: Vec<String>,
    /// Messages in the conversation, excluding Spam outside its own view and Trash outside Trash.
    pub count: usize,
    /// Senders from oldest to newest, without repeats; the account's own messages are "me".
    pub participants: Vec<String>,
    pub is_starred: bool,
}
#[derive(Serialize, Deserialize)]
pub struct ThreadPage {
    pub threads: Vec<ThreadSummary>,
    pub total: usize,
    pub all_total: usize,
    pub unread_total: usize,
    pub revision: String,
}

/// Disk use of one account's cached mail, by folder, with its sync progress.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountStorage {
    pub account_id: String,
    pub messages: usize,
    /// Bytes of cached mail (headers, text and HTML bodies). Attachments are never stored.
    pub bytes: u64,
    pub folders: Vec<FolderStorage>,
    pub sync: SyncStatus,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FolderStorage {
    pub folder: String,
    pub messages: usize,
    pub bytes: u64,
}
/// How much of an account's sync window is on this device.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SyncStatus {
    /// A sync is running for this account now.
    pub active: bool,
    /// Server messages in the sync window that are on this device, counted per mailbox copy.
    pub downloaded: usize,
    /// Server messages in the sync window, as of the last check.
    pub total: usize,
    pub last_synced: Option<String>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StorageReport {
    /// Size of Alice's mail database file, including its search index.
    pub database_bytes: u64,
    pub accounts: Vec<AccountStorage>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessagePatch {
    pub is_read: Option<bool>,
    pub is_starred: Option<bool>,
    pub folder: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compose {
    pub id: String,
    pub account_id: String,
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Light,
    Dark,
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailLayout {
    #[default]
    Single,
    Horizontal,
    Vertical,
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    #[default]
    Comfortable,
    Compact,
}
/// Which version of an email the desktop reader shows. Messages without HTML always show plain text.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageFormat {
    #[default]
    Html,
    Plain,
}
/// Where a conversation's newest message appears in the reader.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadOrder {
    #[default]
    NewestFirst,
    OldestFirst,
}
/// Whether the HTML reader loads remote images automatically or asks for each message.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteImages {
    #[default]
    Ask,
    Always,
}
/// What AI agents may do through `alice mcp`: nothing (the default), read, or read, change and send.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpAccess {
    #[default]
    Off,
    ReadOnly,
    Full,
}
/// How far back mail is kept on this device. Mail older than the window stays on the server.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncWindow {
    #[serde(rename = "30_days")]
    Days30,
    #[default]
    #[serde(rename = "90_days")]
    Days90,
    #[serde(rename = "1_year")]
    Year,
    #[serde(rename = "all")]
    All,
}
impl SyncWindow {
    /// The first day whose mail is kept, counting `today` as the last; `None` keeps everything.
    pub fn since(self, today: chrono::NaiveDate) -> Option<chrono::NaiveDate> {
        let days = match self {
            Self::Days30 => 30,
            Self::Days90 => 90,
            Self::Year => 365,
            Self::All => return None,
        };
        today.checked_sub_days(chrono::Days::new(days))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub language: crate::i18n::Language,
    pub theme: Theme,
    pub mail_layout: MailLayout,
    pub show_avatars: bool,
    pub density: Density,
    pub message_format: MessageFormat,
    pub remote_images: RemoteImages,
    pub thread_order: ThreadOrder,
    pub sync_window: SyncWindow,
    pub signature: String,
    pub default_account_id: String,
    /// Desktop notifications for new Inbox mail.
    pub notifications: bool,
    /// Extract verification codes locally and copy fresh deliveries in the desktop app.
    pub otp_autocopy: bool,
    /// The desktop app's menu bar (system tray) icon, which keeps the app running after its window closes.
    pub menu_bar: bool,
    /// Calendar that "Add to calendar" uses first: a `Calendar::id` from `/v1/calendars`, or empty.
    pub default_calendar: String,
    /// The desktop app imports the operating system's address book (macOS Contacts).
    pub system_contacts: bool,
    /// What AI agents may do with mail through `alice mcp`.
    pub mcp: McpAccess,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: crate::i18n::Language::default(),
            theme: Theme::Light,
            mail_layout: MailLayout::Single,
            show_avatars: true,
            density: Density::Comfortable,
            message_format: MessageFormat::Html,
            remote_images: RemoteImages::Ask,
            thread_order: ThreadOrder::NewestFirst,
            sync_window: SyncWindow::Days90,
            signature: String::new(),
            default_account_id: String::new(),
            notifications: true,
            otp_autocopy: false,
            menu_bar: true,
            default_calendar: String::new(),
            system_contacts: false,
            mcp: McpAccess::Off,
        }
    }
}
impl Preferences {
    pub fn draft_body(&self, body: &str) -> String {
        if self.signature.trim().is_empty() {
            return body.into();
        }
        let suffix = format!("\n\n-- \n{}", self.signature.trim());
        if body.ends_with(&suffix) {
            body.into()
        } else {
            format!("{}{suffix}", body.trim_end())
        }
    }
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferencesPatch {
    pub language: Option<crate::i18n::Language>,
    pub theme: Option<Theme>,
    pub mail_layout: Option<MailLayout>,
    pub show_avatars: Option<bool>,
    pub density: Option<Density>,
    pub message_format: Option<MessageFormat>,
    pub remote_images: Option<RemoteImages>,
    pub thread_order: Option<ThreadOrder>,
    pub sync_window: Option<SyncWindow>,
    pub signature: Option<String>,
    pub default_account_id: Option<String>,
    pub notifications: Option<bool>,
    pub otp_autocopy: Option<bool>,
    pub menu_bar: Option<bool>,
    pub default_calendar: Option<String>,
    pub system_contacts: Option<bool>,
    pub mcp: Option<McpAccess>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectAccount {
    pub name: String,
    pub email: String,
    pub provider: Provider,
    pub password: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub imap_host: Option<String>,
    pub imap_port: Option<u16>,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<u16>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct AiSettings {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub configured: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveAiSettings {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub api_key: Option<String>,
}
#[derive(Serialize, Deserialize)]
pub struct AiRequest {
    pub prompt: String,
    pub message_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedAction {
    pub message_id: String,
    pub action: InboxAction,
    pub subject: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboxAction {
    Archive,
    MarkRead,
    Star,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiResult {
    pub text: String,
    #[serde(default)]
    pub draft: Option<String>,
    #[serde(default)]
    pub actions: Vec<ProposedAction>,
    /// An event the user asked to put on a calendar, proposed for review.
    #[serde(default)]
    pub event: Option<EventDraft>,
}
/// Unread Inbox mail that a provider sync imported soon after the server received it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arrival {
    pub id: String,
    pub account_id: String,
    pub from_name: String,
    pub from_email: String,
    pub subject: String,
    pub preview: String,
}
/// Fresh locally extracted verification code, delivered independently of mail previews.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OtpArrival {
    pub id: String,
    /// Cached message to open; `id` above identifies the durable OTP delivery for claims.
    pub message_id: String,
    pub account_id: String,
    pub from_name: String,
    pub from_email: String,
    pub code: String,
    pub received_at: i64,
    pub expires_at: i64,
}
/// Arrivals after a cursor, and the cursor to pass next time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Arrivals {
    pub cursor: String,
    pub messages: Vec<Arrival>,
}

/// Where a contact suggestion came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactSource {
    /// People in this device's mail: those you wrote to and those who wrote to you.
    Mail,
    Google,
    Microsoft,
    Carddav,
    /// The operating system's address book, imported by the desktop app.
    System,
}
/// A suggested recipient, best first in `/v1/contacts`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contact {
    /// Display name; empty when no source knows one.
    pub name: String,
    pub email: String,
    pub sources: Vec<ContactSource>,
}
#[derive(Default, Clone, Deserialize, Serialize)]
pub struct ContactQuery {
    /// Text the name or address starts with (any word of the name, or the address); empty lists the
    /// people you write to most.
    pub q: Option<String>,
    /// The sending account: its own address books and correspondents rank first.
    pub account_id: Option<String>,
    /// 1–50, default 8.
    pub limit: Option<usize>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedContact {
    pub name: String,
    pub email: String,
}
/// Replaces the operating system address book's contacts.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContactImport {
    pub contacts: Vec<ImportedContact>,
}
/// One source that failed while others succeeded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceError {
    /// An account ID or a DAV source ID.
    pub source: String,
    pub message: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContactsSync {
    /// Contacts now stored from provider and CardDAV address books.
    pub contacts: usize,
    pub errors: Vec<SourceError>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DavKind {
    Carddav,
    Caldav,
}
/// A CardDAV address book server or CalDAV calendar server signed in with a password. The password
/// is in the OS credential store under `dav:{id}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DavSource {
    pub id: String,
    pub kind: DavKind,
    pub name: String,
    /// The URL the user entered; collections are discovered from it.
    pub url: String,
    pub username: String,
    #[serde(default)]
    pub last_synced: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AddDavSource {
    pub kind: DavKind,
    pub name: String,
    pub url: String,
    pub username: String,
    pub password: String,
}

/// An event to show or put on a calendar. Timed events have RFC 3339 UTC `start`/`end`; all-day
/// events have `YYYY-MM-DD` dates with an exclusive `end`, as in iCalendar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventDraft {
    pub title: String,
    pub start: String,
    #[serde(default)]
    pub end: Option<String>,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rsvp {
    Accepted,
    Tentative,
    Declined,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attendee {
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
    /// None while the attendee has not answered.
    #[serde(default)]
    pub status: Option<Rsvp>,
}
/// A calendar invitation (iMIP, RFC 6047) carried by a message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    /// iTIP method: `REQUEST`, `CANCEL`, `REPLY`, `PUBLISH`, …
    pub method: String,
    pub uid: String,
    #[serde(default)]
    pub sequence: i64,
    pub event: EventDraft,
    #[serde(default)]
    pub organizer: Option<Attendee>,
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    /// The `RRULE`, as written, when the event repeats.
    #[serde(default)]
    pub recurrence: Option<String>,
    /// The organizer cancelled the event (`METHOD:CANCEL` or `STATUS:CANCELLED`).
    #[serde(default)]
    pub cancelled: bool,
    /// This account's answer: its `PARTSTAT` in the invitation, then the reply Alice sent.
    #[serde(default)]
    pub response: Option<Rsvp>,
    /// The invitation's iCalendar text, for replying and for adding it to a calendar.
    pub ics: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RsvpRequest {
    pub response: Rsvp,
    #[serde(default)]
    pub comment: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalendarSource {
    Google,
    Microsoft,
    Caldav,
}
/// A calendar Alice can add events to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calendar {
    /// Opaque; pass back to `/v1/calendar/events`.
    pub id: String,
    pub name: String,
    pub source: CalendarSource,
    /// The account ID or DAV source ID it belongs to.
    pub owner: String,
    pub primary: bool,
}
/// The event to add: an invitation from `message_id`, or `event`. Exactly one is given.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AddEvent {
    pub calendar: String,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub event: Option<EventDraft>,
}
/// An iCalendar file for an invitation from `message_id`, or for `event`. Exactly one is given.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventFileRequest {
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub event: Option<EventDraft>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventFile {
    /// A safe file name ending in `.ics`.
    pub filename: String,
    pub ics: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreatedEvent {
    pub calendar: String,
    /// A link to the event in the provider's web calendar, when it gives one.
    #[serde(default)]
    pub url: Option<String>,
}
#[derive(Default, Clone, Deserialize, Serialize)]
pub struct BusyQuery {
    /// RFC 3339; default now.
    pub from: Option<String>,
    /// RFC 3339; default seven days after `from`; at most 62 days after it.
    pub to: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Busy {
    pub start: String,
    pub end: String,
}
/// Busy times across every connected calendar, merged and sorted. Titles are never included.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BusyReport {
    pub busy: Vec<Busy>,
    /// How many calendar sources answered.
    pub sources: usize,
    pub errors: Vec<SourceError>,
}
/// Every calendar Alice can add events to, and the sources that could not be listed.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CalendarList {
    pub calendars: Vec<Calendar>,
    pub errors: Vec<SourceError>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_saved_before_spam_support_keep_their_mailbox_configuration() -> anyhow::Result<()> {
        let mut saved = serde_json::json!({
            "id": "old-account", "name": "Mail", "email": "me@example.test",
            "provider": "imap", "is_demo": false,
            "imap_host": "imap.example.test", "imap_port": 993,
            "smtp_host": "smtp.example.test", "smtp_port": 587,
            "folders": {
                "inbox": "INBOX", "sent": "INBOX.Sent",
                "archive": "Saved", "trash": "Deleted"
            },
            "last_synced": null, "sync_error": null
        });
        let account: Account = serde_json::from_value(saved.clone())?;
        assert_eq!(account.folders.sent, "INBOX.Sent");
        assert_eq!(account.folders.archive, "Saved");
        assert_eq!(account.folders.trash, "Deleted");
        assert_eq!(account.folders.spam, "Junk");
        saved["folders"]["spam"] = "[Provider]/Unwanted".into();
        let account: Account = serde_json::from_value(saved.clone())?;
        let roundtrip: Account = serde_json::from_str(&serde_json::to_string(&account)?)?;
        assert_eq!(roundtrip.folders.spam, "[Provider]/Unwanted");
        saved.as_object_mut().unwrap().remove("folders");
        let account: Account = serde_json::from_value(saved)?;
        assert_eq!(account.folders.inbox, "INBOX");
        assert_eq!(account.folders.spam, "Junk");
        Ok(())
    }
}
