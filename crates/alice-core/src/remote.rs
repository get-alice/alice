//! Read-only IMAP access for sync and attachment downloads. Mailboxes are opened with `EXAMINE`,
//! so nothing here changes server state; bodies are fetched with `BODY.PEEK`.
//!
//! Sync asks the server only for what changed: mailbox status (UIDVALIDITY, UIDNEXT, EXISTS and,
//! with CONDSTORE, HIGHESTMODSEQ), the UIDs in the sync window, flags changed since the last
//! modification sequence, and the headers, structure and text parts of messages it does not have.
//! Attachments are never downloaded during sync.

use crate::{
    mime::{self, Part},
    model::*,
    providers::{self, Session},
    vault::Secret,
};
use anyhow::{Context, Result, bail};
use imap::types::Flag;
use imap_proto::types::{MessageSection, SectionPath};
use std::collections::BTreeMap;

/// Messages fetched whole when the server's structure cannot be read; larger ones keep only headers.
const FULL_MESSAGE_LIMIT: usize = 30_000_000;
/// UIDs per command for flag and header requests, keeping command lines short.
const UID_CHUNK: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailboxStatus {
    pub validity: u32,
    pub uid_next: Option<u32>,
    pub exists: u32,
    /// `None` when the server lacks CONDSTORE or the mailbox has no modification sequences.
    pub highest_modseq: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteFlags {
    pub uid: u32,
    pub is_read: bool,
    pub is_starred: bool,
}
/// A message's flags, date, size and raw header block.
#[derive(Clone, Debug)]
pub struct Header {
    pub uid: u32,
    pub is_read: bool,
    pub is_starred: bool,
    /// The server's INTERNALDATE (RFC 3339), which the sync window is measured by.
    pub date: Option<String>,
    pub size: usize,
    pub message_id: Option<String>,
    pub raw: Vec<u8>,
}

pub struct Remote {
    session: Session,
    account: Account,
    secret: Secret,
    condstore: bool,
    selected: Option<String>,
    /// A command failed, possibly mid-response. The unread rest of that response would answer
    /// the next command, and the IMAP client panics on the mismatched tag, so the connection is
    /// closed without sending anything else, not even LOGOUT.
    poisoned: bool,
}

impl Remote {
    pub fn connect(account: &Account, secret: &Secret) -> Result<Self> {
        let mut session = providers::session(account, secret)?;
        let capabilities = session.capabilities()?;
        let condstore = capabilities.has_str("CONDSTORE") || capabilities.has_str("QRESYNC");
        drop(capabilities);
        Ok(Self {
            session,
            account: account.clone(),
            secret: secret.clone(),
            condstore,
            selected: None,
            poisoned: false,
        })
    }
    /// Runs one IMAP command, remembering a failure; see [`Self::poisoned`].
    fn command<T>(
        &mut self,
        run: impl FnOnce(&mut Session) -> imap::error::Result<T>,
    ) -> imap::error::Result<T> {
        let result = run(&mut self.session);
        self.poisoned |= result.is_err();
        result
    }
    /// Opens a fresh connection after a failure left this one in an unknown state. The old one
    /// is dropped, which closes it.
    fn reconnect(&mut self) -> Result<()> {
        let fresh = Self::connect(&self.account, &self.secret)?;
        self.session = fresh.session;
        self.poisoned = false;
        if let Some(mailbox) = self.selected.take() {
            self.examine(&mailbox)?;
        }
        Ok(())
    }
    pub fn logout(mut self) {
        if !self.poisoned {
            let _ = self.session.logout();
        }
    }
    /// The account's Inbox, Sent, Trash and Archive mailboxes, found by special-use attributes
    /// (Gmail's All Mail stands in for Archive), and those of them that exist, in sync order.
    pub fn folders(&mut self) -> Result<(Mailboxes, Vec<(String, String)>)> {
        let names = self.command(|s| s.list(Some(""), Some("*")))?;
        let find = |flag: &str, fallback: &str| -> String {
            names
                .iter()
                .find(|name| name.attributes().iter().any(|attr| matches!(attr, imap::types::NameAttribute::Custom(value) if value.eq_ignore_ascii_case(flag))))
                .map(|n| n.name().to_string())
                .unwrap_or_else(|| fallback.to_string())
        };
        let gmail = self.account.provider == Provider::Gmail;
        let folders = Mailboxes {
            inbox: "INBOX".into(),
            sent: find("\\Sent", &self.account.folders.sent),
            archive: find(
                if gmail { "\\All" } else { "\\Archive" },
                if gmail {
                    "[Gmail]/All Mail"
                } else {
                    &self.account.folders.archive
                },
            ),
            trash: find("\\Trash", &self.account.folders.trash),
        };
        let mut present: Vec<(String, String)> = Vec::new();
        for (folder, mailbox) in [
            ("inbox", &folders.inbox),
            ("sent", &folders.sent),
            ("trash", &folders.trash),
            ("archive", &folders.archive),
        ] {
            let exists = folder == "inbox" || names.iter().any(|n| n.name() == mailbox);
            if exists && !present.iter().any(|(_, m)| m == mailbox) {
                present.push((folder.to_string(), mailbox.clone()));
            }
        }
        drop(names);
        self.drain();
        Ok((folders, present))
    }
    /// Opens `mailbox` read-only and reports its status.
    pub fn examine(&mut self, mailbox: &str) -> Result<MailboxStatus> {
        let command = format!(
            "EXAMINE {}{}",
            providers::imap_quote(mailbox)?,
            if self.condstore { " (CONDSTORE)" } else { "" }
        );
        let response = self
            .command(|s| s.run_command_and_read_response(command))
            .with_context(|| crate::text!("mail.imap.open_mailbox", mailbox = mailbox))?;
        self.selected = Some(mailbox.to_string());
        let status = parse_status(&response)
            .with_context(|| crate::text!("mail.imap.identity_missing", mailbox = mailbox))?;
        Ok(MailboxStatus {
            highest_modseq: status.highest_modseq.filter(|_| self.condstore),
            ..status
        })
    }
    fn open(&mut self, mailbox: &str) -> Result<()> {
        if self.selected.as_deref() != Some(mailbox) {
            self.examine(mailbox)?;
        }
        Ok(())
    }
    /// UIDs of the open mailbox's messages that arrived on or after `since` (all when `None`).
    pub fn window(&mut self, since: Option<chrono::NaiveDate>) -> Result<Vec<u32>> {
        let query = since.map_or_else(
            || "ALL".to_string(),
            |day| format!("SINCE {}", day.format("%-d-%b-%Y")),
        );
        let mut uids: Vec<u32> = self.command(|s| s.uid_search(query))?.into_iter().collect();
        uids.sort_unstable();
        self.drain();
        Ok(uids)
    }
    /// Flags of messages changed since modification sequence `modseq` (CONDSTORE, RFC 7162).
    pub fn changed_flags(&mut self, modseq: u64) -> Result<Vec<RemoteFlags>> {
        let fetched =
            self.command(|s| s.uid_fetch("1:*", format!("(UID FLAGS) (CHANGEDSINCE {modseq})")))?;
        let flags = fetched.iter().filter_map(remote_flags).collect();
        drop(fetched);
        self.drain();
        Ok(flags)
    }
    /// Current flags of `uids` in the open mailbox.
    pub fn flags(&mut self, uids: &[u32]) -> Result<Vec<RemoteFlags>> {
        let mut flags = Vec::new();
        for chunk in uids.chunks(UID_CHUNK) {
            let fetched = self.command(|s| s.uid_fetch(uid_set(chunk), "(UID FLAGS)"))?;
            flags.extend(fetched.iter().filter_map(remote_flags));
        }
        self.drain();
        Ok(flags)
    }
    /// Flags, dates, sizes and headers of `uids` in `mailbox`. UIDs the server no longer has are absent.
    pub fn headers(&mut self, mailbox: &str, uids: &[u32]) -> Result<Vec<Header>> {
        self.open(mailbox)?;
        let mut headers = Vec::new();
        for chunk in uids.chunks(UID_CHUNK) {
            let fetched = self.command(|s| {
                s.uid_fetch(
                    uid_set(chunk),
                    "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER])",
                )
            })?;
            for fetch in fetched.iter() {
                let (Some(uid), Some(raw)) = (fetch.uid, fetch.header()) else {
                    continue;
                };
                let message_id = mailparse::parse_headers(raw).ok().and_then(|(headers, _)| {
                    use mailparse::MailHeaderMap;
                    headers.get_first_value("Message-ID")
                });
                headers.push(Header {
                    uid,
                    is_read: fetch.flags().contains(&Flag::Seen),
                    is_starred: fetch.flags().contains(&Flag::Flagged),
                    date: fetch
                        .internal_date()
                        .map(|date| date.with_timezone(&chrono::Utc).to_rfc3339()),
                    size: fetch.size.unwrap_or(0) as usize,
                    message_id,
                    raw: raw.to_vec(),
                });
            }
        }
        self.drain();
        Ok(headers)
    }
    /// The structure and displayable text of each message in `headers`, without attachments.
    /// A message whose structure the server cannot describe is downloaded whole instead (up to
    /// 30 MB; beyond that it keeps only its headers and an empty structure).
    pub fn contents(&mut self, mailbox: &str, headers: &[Header]) -> Result<Vec<(u32, Part)>> {
        self.open(mailbox)?;
        let uids: Vec<u32> = headers.iter().map(|h| h.uid).collect();
        let structures = match self.structures(&uids) {
            Ok(structures) => structures,
            Err(_) => {
                // One message the IMAP parser rejects fails the whole response; retry one at a time.
                self.reconnect()?;
                let mut structures = BTreeMap::new();
                for uid in &uids {
                    match self.structures(&[*uid]) {
                        Ok(found) => structures.extend(found),
                        Err(_) => self.reconnect()?,
                    }
                }
                structures
            }
        };
        let mut parts: Vec<(u32, Part)> = Vec::new();
        let mut whole = Vec::new();
        for header in headers {
            match structures.get(&header.uid) {
                Some(part) => parts.push((header.uid, part.clone())),
                None => whole.push(header),
            }
        }
        self.fill_texts(&mut parts)?;
        for header in whole {
            parts.push((header.uid, self.whole(header)?));
        }
        Ok(parts)
    }
    fn structures(&mut self, uids: &[u32]) -> Result<BTreeMap<u32, Part>> {
        let fetched = self.command(|s| s.uid_fetch(uid_set(uids), "(UID BODYSTRUCTURE)"))?;
        let structures = fetched
            .iter()
            .filter_map(|fetch| Some((fetch.uid?, Part::from_structure(fetch.bodystructure()?))))
            .collect();
        drop(fetched);
        self.drain();
        Ok(structures)
    }
    /// Downloads every message's text sections, one request per distinct set of sections.
    fn fill_texts(&mut self, parts: &mut [(u32, Part)]) -> Result<()> {
        let mut groups: BTreeMap<Vec<String>, Vec<u32>> = BTreeMap::new();
        for (uid, part) in parts.iter() {
            let sections: Vec<String> = part
                .text_parts()
                .iter()
                .map(|p| p.section.clone())
                .collect();
            if !sections.is_empty() {
                groups.entry(sections).or_default().push(*uid);
            }
        }
        for (sections, uids) in groups {
            let items: Vec<String> = sections.iter().map(|s| format!("BODY.PEEK[{s}]")).collect();
            let fetched = self
                .command(|s| s.uid_fetch(uid_set(&uids), format!("(UID {})", items.join(" "))))?;
            for fetch in fetched.iter() {
                let Some(uid) = fetch.uid else { continue };
                let Some((_, part)) = parts.iter_mut().find(|(u, _)| *u == uid) else {
                    continue;
                };
                for section in &sections {
                    let Some(raw) = fetch.section(&section_path(section, None)) else {
                        continue;
                    };
                    let Some(leaf) = part.find(section) else {
                        continue;
                    };
                    let text = mime::decode_text(raw, &leaf.encoding, leaf.text_charset());
                    part.set_text(section, text);
                }
            }
            drop(fetched);
            self.drain();
        }
        Ok(())
    }
    fn whole(&mut self, header: &Header) -> Result<Part> {
        if header.size > FULL_MESSAGE_LIMIT {
            return Ok(Part::default());
        }
        let fetched = self.command(|s| s.uid_fetch(header.uid.to_string(), "(UID BODY.PEEK[])"))?;
        let part = fetched
            .iter()
            .find(|fetch| fetch.uid == Some(header.uid))
            .and_then(|fetch| fetch.body())
            .and_then(|raw| {
                mailparse::parse_mail(raw)
                    .ok()
                    .map(|mail| Part::from_parsed(&mail))
            })
            .unwrap_or_default();
        drop(fetched);
        self.drain();
        Ok(part)
    }
    /// The decoded bytes of `attachment`, the `index`th attachment of the message at `uid`.
    /// Fetches only the attachment's section when its position is known and still matches;
    /// otherwise (mail cached before sections were recorded, or a structure the server cannot
    /// describe) the whole message.
    pub fn attachment(
        &mut self,
        mailbox: &str,
        validity: u32,
        uid: u32,
        index: usize,
        known: &[Attachment],
    ) -> Result<Vec<u8>> {
        let attachment = known
            .get(index)
            .context(crate::text!("mail.attachment.not_found"))?;
        if self.examine(mailbox)?.validity != validity {
            bail!(crate::text!("mail.attachment.mailbox_changed"));
        }
        if let Some(section) = &attachment.part {
            match self.attachment_section(uid, section, &attachment.name) {
                Ok(Some(bytes)) => return Ok(bytes),
                Ok(None) => {}
                // The IMAP parser rejected the response; the connection is in an unknown state.
                Err(_) => {
                    self.reconnect()?;
                    if self.examine(mailbox)?.validity != validity {
                        bail!(crate::text!("mail.attachment.mailbox_changed"));
                    }
                }
            }
        }
        // Find the same attachment by name and occurrence in the full message.
        let occurrence = known[..index]
            .iter()
            .filter(|a| a.name == attachment.name)
            .count();
        let fetched = self.command(|s| s.uid_fetch(uid.to_string(), "(UID BODY.PEEK[])"))?;
        let raw = fetched
            .iter()
            .find(|fetch| fetch.uid == Some(uid))
            .and_then(|fetch| fetch.body())
            .context(crate::text!("mail.message.no_longer_available"))?;
        let mail = mailparse::parse_mail(raw)?;
        let found = Part::from_parsed(&mail)
            .attachments()
            .into_iter()
            .filter(|a| a.name == attachment.name)
            .nth(occurrence)
            .context(crate::text!("mail.attachment.no_longer_available"))?;
        let part = parsed_section(&mail, found.part.as_deref().unwrap_or("1"))
            .context(crate::text!("mail.attachment.no_longer_available"))?;
        // An attached multipart entity is saved whole, with its own headers and every part.
        Ok(if part.subparts.is_empty() {
            part.get_body_raw()?
        } else {
            part.raw_bytes.to_vec()
        })
    }
    /// The attachment at `section` if the server's structure still names it `name`.
    fn attachment_section(
        &mut self,
        uid: u32,
        section: &str,
        name: &str,
    ) -> Result<Option<Vec<u8>>> {
        let fetched = self.command(|s| {
            s.uid_fetch(
                uid.to_string(),
                format!("(UID BODYSTRUCTURE BODY.PEEK[{section}])"),
            )
        })?;
        let fetch = fetched
            .iter()
            .find(|fetch| fetch.uid == Some(uid))
            .context(crate::text!("mail.message.no_longer_available"))?;
        let structure = fetch.bodystructure().map(Part::from_structure);
        let Some(part) = structure
            .as_ref()
            .and_then(|s| s.find(section))
            .filter(|part| part.attachment_name().as_deref() == Some(name))
        else {
            return Ok(None);
        };
        let Some(body) = fetch.section(&section_path(section, None)) else {
            return Ok(None);
        };
        let (body, container, encoding) = (
            body.to_vec(),
            !part.children.is_empty(),
            part.encoding.clone(),
        );
        drop(fetched);
        self.drain();
        if !container {
            return Ok(Some(mime::decode_bytes(&body, &encoding)));
        }
        // An attached multipart entity is saved whole: its MIME headers, then its body.
        let fetched = self.command(|s| {
            s.uid_fetch(uid.to_string(), format!("(UID BODY.PEEK[{section}.MIME])"))
        })?;
        let headers = fetched
            .iter()
            .find(|fetch| fetch.uid == Some(uid))
            .and_then(|fetch| fetch.section(&section_path(section, Some(MessageSection::Mime))))
            .map(<[u8]>::to_vec);
        drop(fetched);
        self.drain();
        Ok(headers.map(|headers| [headers, body].concat()))
    }
    /// Discards untagged responses the IMAP client queued, which sync does not use.
    fn drain(&mut self) {
        while self.session.unsolicited_responses.try_recv().is_ok() {}
    }
}

fn remote_flags(fetch: &imap::types::Fetch) -> Option<RemoteFlags> {
    Some(RemoteFlags {
        uid: fetch.uid?,
        is_read: fetch.flags().contains(&Flag::Seen),
        is_starred: fetch.flags().contains(&Flag::Flagged),
    })
}

fn section_path(section: &str, part: Option<MessageSection>) -> SectionPath {
    SectionPath::Part(
        section.split('.').filter_map(|n| n.parse().ok()).collect(),
        part,
    )
}

/// The part of a fully parsed message at an IMAP section.
fn parsed_section<'a>(
    mail: &'a mailparse::ParsedMail<'a>,
    section: &str,
) -> Option<&'a mailparse::ParsedMail<'a>> {
    if mail.subparts.is_empty() {
        return (section == "1").then_some(mail);
    }
    let mut part = mail;
    for index in section.split('.') {
        part = part
            .subparts
            .get(index.parse::<usize>().ok()?.checked_sub(1)?)?;
    }
    Some(part)
}

/// A compact IMAP UID set: `1:5,9,12:20`.
pub fn uid_set(uids: &[u32]) -> String {
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut ranges: Vec<String> = Vec::new();
    let mut index = 0;
    while index < sorted.len() {
        let start = sorted[index];
        let mut end = start;
        while index + 1 < sorted.len() && sorted[index + 1] == end + 1 {
            index += 1;
            end = sorted[index];
        }
        ranges.push(if start == end {
            start.to_string()
        } else {
            format!("{start}:{end}")
        });
        index += 1;
    }
    ranges.join(",")
}

/// Reads the untagged status lines of an `EXAMINE`/`SELECT` response.
fn parse_status(response: &[u8]) -> Option<MailboxStatus> {
    let text = String::from_utf8_lossy(response);
    let code = |name: &str| -> Option<u64> {
        text.lines().find_map(|line| {
            let start = line.to_ascii_uppercase().find(&format!("[{name} "))? + name.len() + 2;
            line[start..].split(']').next()?.trim().parse().ok()
        })
    };
    let exists = text.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        (words.next()? == "*").then_some(())?;
        let count = words.next()?.parse().ok()?;
        words
            .next()?
            .eq_ignore_ascii_case("EXISTS")
            .then_some(count)
    })?;
    Some(MailboxStatus {
        validity: u32::try_from(code("UIDVALIDITY")?).ok()?,
        uid_next: code("UIDNEXT").and_then(|n| u32::try_from(n).ok()),
        exists,
        highest_modseq: code("HIGHESTMODSEQ"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uid_sets_collapse_runs() {
        assert_eq!(uid_set(&[9, 1, 2, 3, 5, 12, 11, 10, 3]), "1:3,5,9:12");
        assert_eq!(uid_set(&[7]), "7");
    }

    #[test]
    fn examine_status_reads_identity_counts_and_modification_sequence() {
        let response = b"* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n* OK [PERMANENTFLAGS ()] Read-only mailbox.\r\n* 172 EXISTS\r\n* 0 RECENT\r\n* OK [UIDVALIDITY 1675] UIDs valid\r\n* OK [UIDNEXT 4392] Predicted next UID\r\n* OK [HIGHESTMODSEQ 90060115205545359] Highest\r\na2 OK [READ-ONLY] Examine completed\r\n";
        assert_eq!(
            parse_status(response),
            Some(MailboxStatus {
                validity: 1675,
                uid_next: Some(4392),
                exists: 172,
                highest_modseq: Some(90060115205545359)
            })
        );
        let without =
            b"* 0 EXISTS\r\n* OK [UIDVALIDITY 3] ok\r\n* OK [NOMODSEQ] none\r\na1 OK done\r\n";
        assert_eq!(
            parse_status(without),
            Some(MailboxStatus {
                validity: 3,
                uid_next: None,
                exists: 0,
                highest_modseq: None
            })
        );
        assert_eq!(parse_status(b"a1 OK done\r\n"), None);
    }

    #[test]
    fn sections_address_nested_parts_of_a_parsed_message() -> Result<()> {
        let raw = b"Content-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain\r\n\r\nBody\r\n--x\r\nContent-Type: multipart/mixed; boundary=y\r\n\r\n--y\r\nContent-Type: text/plain; name=a.txt\r\n\r\nFirst\r\n--y\r\nContent-Type: text/plain; name=b.txt\r\n\r\nSecond\r\n--y--\r\n--x--\r\n";
        let mail = mailparse::parse_mail(raw)?;
        assert_eq!(
            parsed_section(&mail, "2.2").map(|p| p.get_body().unwrap().trim().to_string()),
            Some("Second".into())
        );
        assert!(parsed_section(&mail, "3").is_none());
        let single = mailparse::parse_mail(b"Content-Type: text/plain\r\n\r\nOnly")?;
        assert!(parsed_section(&single, "1").is_some());
        Ok(())
    }
}
