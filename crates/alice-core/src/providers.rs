use crate::{i18n::Locale, mime::Part, model::*, store::preview, vault::Secret};
use anyhow::{Context, Result, bail};
use imap::Authenticator;
use lettre::{
    Message as Mail, SmtpTransport, Transport,
    transport::smtp::authentication::{Credentials, Mechanism},
};
use mailparse::{MailHeader, MailHeaderMap};
use native_tls::{TlsConnector, TlsStream};
use std::{
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};
use uuid::Uuid;

pub(crate) type Session = imap::Session<TlsStream<TcpStream>>;
struct Xoauth<'a> {
    email: &'a str,
    token: &'a str,
}
impl Authenticator for Xoauth<'_> {
    type Response = String;
    fn process(&self, challenge: &[u8]) -> String {
        if challenge.is_empty() {
            format!("user={}\x01auth=Bearer {}\x01\x01", self.email, self.token)
        } else {
            String::new()
        }
    }
}
pub(crate) fn session(account: &Account, secret: &Secret) -> Result<Session> {
    let mut connection = Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "IMAP host could not be resolved",
    ));
    for address in (account.imap_host.as_str(), account.imap_port).to_socket_addrs()? {
        connection = TcpStream::connect_timeout(&address, Duration::from_secs(20));
        if connection.is_ok() {
            break;
        }
    }
    let tcp = connection.context(crate::text!("mail.imap.connect"))?;
    tcp.set_read_timeout(Some(Duration::from_secs(30)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(30)))?;
    let tls = TlsConnector::new()?.connect(&account.imap_host, tcp)?;
    let mut client = imap::Client::new(tls);
    client.read_greeting()?;
    if let Some(token) = &secret.access_token {
        client
            .authenticate(
                "XOAUTH2",
                &Xoauth {
                    email: &account.email,
                    token,
                },
            )
            .map_err(|(e, _)| {
                anyhow::anyhow!(crate::text!("mail.imap.oauth", error = e.to_string()))
            })
    } else {
        client
            .login(
                &account.email,
                secret
                    .password
                    .as_deref()
                    .context(crate::text!("mail.account.password_missing"))?,
            )
            .map_err(|(e, _)| {
                anyhow::anyhow!(crate::text!(
                    "mail.imap.authentication",
                    error = e.to_string()
                ))
            })
    }
}

pub fn modify(
    account: &Account,
    secret: &Secret,
    message: &Message,
    patch: &MessagePatch,
) -> Result<Message> {
    let uid = message
        .remote_uid
        .context(crate::text!("mail.message.sync_first"))?;
    let mut session = session(account, secret)?;
    let selected = session.select(
        message
            .remote_mailbox
            .as_deref()
            .context(crate::text!("mail.imap.mailbox_missing"))?,
    )?;
    if selected.uid_validity != message.uid_validity {
        bail!(crate::text!("mail.imap.identity_changed"));
    }
    if let Some(read) = patch.is_read {
        session.uid_store(
            uid.to_string(),
            if read {
                "+FLAGS.SILENT (\\Seen)"
            } else {
                "-FLAGS.SILENT (\\Seen)"
            },
        )?;
    }
    if let Some(starred) = patch.is_starred {
        session.uid_store(
            uid.to_string(),
            if starred {
                "+FLAGS.SILENT (\\Flagged)"
            } else {
                "-FLAGS.SILENT (\\Flagged)"
            },
        )?;
    }
    let mut updated = message.clone();
    if let Some(read) = patch.is_read {
        updated.is_read = read;
    }
    if let Some(starred) = patch.is_starred {
        updated.is_starred = starred;
    }
    if let Some(folder) = &patch.folder {
        let destination = match folder.as_str() {
            "inbox" => &account.folders.inbox,
            "archive" => &account.folders.archive,
            "trash" => &account.folders.trash,
            "spam" => &account.folders.spam,
            _ => bail!(crate::text!("mail.message.unsupported_folder")),
        };
        if destination.is_empty() {
            bail!(crate::text!("mail.message.unsupported_folder"));
        }
        if !session.capabilities()?.has_str("MOVE") {
            bail!(crate::text!("mail.imap.no_safe_move"));
        }
        let response = session.run_command_and_read_response(format!(
            "UID MOVE {uid} {}",
            imap_quote(destination)?
        ))?;
        updated.folder = folder.clone();
        updated.remote_mailbox = Some(destination.clone());
        if let Some((validity, new_uid)) = moved_uid(&response, uid) {
            updated.uid_validity = Some(validity);
            updated.remote_uid = Some(new_uid);
        } else {
            let destination_mailbox = session.select(destination)?;
            updated.uid_validity = destination_mailbox.uid_validity;
            updated.remote_uid = None;
            if let Some(identity) = &message.message_id {
                let candidates =
                    session.uid_search(format!("HEADER Message-ID {}", imap_quote(identity)?))?;
                for candidate in candidates {
                    let headers = session.uid_fetch(candidate.to_string(), "BODY.PEEK[HEADER]")?;
                    if headers.iter().any(|f| {
                        f.header()
                            .and_then(|h| mailparse::parse_headers(h).ok())
                            .and_then(|(h, _)| h.get_first_value("Message-ID"))
                            .as_ref()
                            == Some(identity)
                    }) {
                        updated.remote_uid = Some(candidate);
                        break;
                    }
                }
            }
        }
    }
    session.logout()?;
    Ok(updated)
}

pub(crate) fn imap_quote(value: &str) -> Result<String> {
    if value.chars().any(char::is_control) {
        bail!(crate::text!("mail.imap.invalid_string"));
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}
fn moved_uid(response: &[u8], source: u32) -> Option<(u32, u32)> {
    let response = String::from_utf8_lossy(response).to_ascii_uppercase();
    let code = response.split("[COPYUID ").nth(1)?.split(']').next()?;
    let mut fields = code.split_whitespace();
    let validity = fields.next()?.parse().ok()?;
    let from: u32 = fields.next()?.parse().ok()?;
    let to: u32 = fields.next()?.parse().ok()?;
    (from == source && to > 0).then_some((validity, to))
}

pub fn build_mail(account: &Account, message: &Message) -> Result<Mail> {
    let mut builder = Mail::builder()
        .from(account.email.parse()?)
        .header(lettre::message::header::ContentType::TEXT_PLAIN)
        .subject(&message.subject)
        .message_id(message.message_id.clone());
    if message.to.is_empty() {
        bail!(crate::text!("mail.message.recipient_required"));
    }
    for to in &message.to {
        builder = builder.to(to
            .parse()
            .context(crate::text!("mail.message.invalid_recipient"))?);
    }
    if let Some(reply) = &message.in_reply_to {
        builder = builder.in_reply_to(reply.clone()).references(reply.clone());
    }
    Ok(builder.body(message.body.clone())?)
}
/// Sends `mail` through the account's SMTP server.
pub fn deliver(account: &Account, secret: &Secret, mail: &Mail) -> Result<()> {
    let mut transport = if account.smtp_port == 465 {
        SmtpTransport::relay(&account.smtp_host)?
    } else {
        SmtpTransport::starttls_relay(&account.smtp_host)?
    };
    let credential = secret
        .access_token
        .as_ref()
        .or(secret.password.as_ref())
        .context(crate::text!("mail.smtp.credentials_missing"))?;
    transport = transport
        .port(account.smtp_port)
        .timeout(Some(Duration::from_secs(40)))
        .credentials(Credentials::new(account.email.clone(), credential.clone()));
    if secret.access_token.is_some() {
        transport = transport.authentication(vec![Mechanism::Xoauth2]);
    }
    transport
        .build()
        .send(mail)
        .context(crate::text!("mail.smtp.delivery_failed"))?;
    Ok(())
}
/// The iMIP answer (RFC 6047) to the invitation in `message`: a short note for people, and the
/// iTIP `REPLY` for the organizer's calendar, in one `multipart/alternative`.
pub fn rsvp_mail(
    account: &Account,
    message: &Message,
    invite: &Invite,
    response: Rsvp,
    comment: Option<&str>,
    locale: Locale,
) -> Result<Mail> {
    use lettre::message::{MultiPart, SinglePart, header::ContentType};
    let organizer = invite
        .organizer
        .as_ref()
        .map(|o| o.email.trim())
        .filter(|e| !e.is_empty())
        .context(crate::text!("mail.rsvp.no_organizer"))?;
    let subject = match response {
        Rsvp::Accepted => crate::t!(
            locale,
            "mail.rsvp.accepted_subject",
            title = &invite.event.title
        ),
        Rsvp::Tentative => crate::t!(
            locale,
            "mail.rsvp.tentative_subject",
            title = &invite.event.title
        ),
        Rsvp::Declined => crate::t!(
            locale,
            "mail.rsvp.declined_subject",
            title = &invite.event.title
        ),
    };
    let name = if account.name.trim().is_empty() {
        &account.email
    } else {
        &account.name
    };
    let mut text = match response {
        Rsvp::Accepted => crate::t!(
            locale,
            "mail.rsvp.accepted_body",
            name = name,
            title = &invite.event.title
        ),
        Rsvp::Tentative => crate::t!(
            locale,
            "mail.rsvp.tentative_body",
            name = name,
            title = &invite.event.title
        ),
        Rsvp::Declined => crate::t!(
            locale,
            "mail.rsvp.declined_body",
            name = name,
            title = &invite.event.title
        ),
    };
    if let Some(comment) = comment.map(str::trim).filter(|c| !c.is_empty()) {
        text.push_str("\n\n");
        text.push_str(comment);
    }
    let ics = crate::ical::reply(invite, &account.email, &account.name, response, comment)?;
    let calendar = ContentType::parse("text/calendar; method=REPLY; charset=UTF-8")
        .context(crate::text!("mail.rsvp.invalid_content_type"))?;
    let mut builder = Mail::builder()
        .from(account.email.parse()?)
        .to(organizer
            .parse()
            .context(crate::text!("mail.rsvp.invalid_organizer"))?)
        .subject(subject);
    if let Some(reply) = &message.message_id {
        builder = builder.in_reply_to(reply.clone()).references(reply.clone());
    }
    Ok(builder.multipart(
        MultiPart::alternative()
            .singlepart(SinglePart::plain(text))
            .singlepart(SinglePart::builder().header(calendar).body(ics)),
    )?)
}

/// Parses a fully downloaded message.
pub fn parse_message(
    account: &Account,
    folder: &str,
    mailbox: &str,
    validity: u32,
    uid: u32,
    raw: &[u8],
) -> Result<Message> {
    let parsed = mailparse::parse_mail(raw)?;
    Ok(message_from(
        account,
        folder,
        mailbox,
        validity,
        uid,
        &parsed.headers,
        &Part::from_parsed(&parsed),
    ))
}
/// The message with these headers and structure, whose text parts have been downloaded.
pub fn message_from(
    account: &Account,
    folder: &str,
    mailbox: &str,
    validity: u32,
    uid: u32,
    headers: &[MailHeader<'_>],
    root: &Part,
) -> Message {
    let sender = headers.get_first_value("From").unwrap_or_default();
    let addresses = mailparse::addrparse(&sender).ok();
    let info = addresses.and_then(|a| a.extract_single_info());
    let from_email = info
        .as_ref()
        .map(|i| i.addr.clone())
        .unwrap_or(sender.clone());
    let from_name = info.and_then(|i| i.display_name).unwrap_or_else(|| {
        from_email
            .split('@')
            .next()
            .unwrap_or("Unknown")
            .to_string()
    });
    let addresses = |header| {
        mailparse::addrparse(&headers.get_first_value(header).unwrap_or_default())
            .map(|a| {
                a.iter()
                    .flat_map(|entry| match entry {
                        mailparse::MailAddr::Single(i) => vec![i.addr.clone()],
                        mailparse::MailAddr::Group(g) => {
                            g.addrs.iter().map(|i| i.addr.clone()).collect()
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let message_id = headers.get_first_value("Message-ID");
    let mut body = root.plain_body().unwrap_or_default();
    if root.has_oversized_text() {
        body.push_str(&format!("{}This message's text is larger than {} MB. Open it in your provider to read all of it.", if body.is_empty() { "" } else { "\n\n" }, crate::mime::TEXT_LIMIT / 1_000_000));
    }
    let date = headers
        .get_first_value("Date")
        .and_then(|s| chrono::DateTime::parse_from_rfc2822(&s).ok())
        .map(|d| d.with_timezone(&chrono::Utc).to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    Message {
        id: message_row_id(
            account,
            folder,
            mailbox,
            validity,
            uid,
            message_id.as_deref(),
        ),
        account_id: account.id.clone(),
        from_name,
        from_email,
        to: addresses("To"),
        reply_to: addresses("Reply-To"),
        subject: headers
            .get_first_value("Subject")
            .unwrap_or_else(|| "(No subject)".into()),
        preview: preview(&body),
        html_body: root.html_body(),
        body,
        date,
        folder: folder.into(),
        is_read: false,
        is_starred: false,
        category: "personal".into(),
        attachments: root.attachments(),
        in_reply_to: headers.get_first_value("In-Reply-To"),
        message_id,
        remote_mailbox: Some(mailbox.into()),
        remote_uid: Some(uid),
        uid_validity: Some(validity),
        // A calendar part that is not a readable event (a task, a broken file) is left as an attachment.
        invite: root
            .calendar()
            .and_then(|ics| crate::ical::invite(ics, &account.email).ok()),
    }
}
/// A stable row id: Sent copies and other copies of a message are separate rows (mail you send
/// yourself appears in both Inbox and Sent); messages without a Message-ID are keyed by location.
pub fn message_row_id(
    account: &Account,
    folder: &str,
    mailbox: &str,
    validity: u32,
    uid: u32,
    message_id: Option<&str>,
) -> String {
    let identity = message_id.map_or_else(|| format!("{mailbox}:{validity}:{uid}"), str::to_string);
    let key = if folder == "sent" {
        format!("{}:sent:{identity}", account.id)
    } else {
        format!("{}:{identity}", account.id)
    };
    Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn move_response_tracks_destination_identity_and_quotes_untrusted_names() -> Result<()> {
        assert_eq!(
            moved_uid(
                b"* OK [COPYUID 2 10 44] moved\r\n* 1 EXPUNGE\r\na1 OK done\r\n",
                10
            ),
            Some((2, 44))
        );
        assert_eq!(moved_uid(b"a1 OK [COPYUID 2 11 44] done", 10), None);
        assert_eq!(moved_uid(b"a1 OK no mapping", 10), None);
        assert!(imap_quote("Archive\r\nLOGOUT").is_err());
        assert_eq!(imap_quote("A\\B\"C")?, "\"A\\\\B\\\"C\"");
        Ok(())
    }
    #[test]
    fn mixed_body_preserves_all_inline_parts_and_reply_to() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let account = {
            let store = crate::store::Store::open(&dir.path().join("mail.db"))?;
            store.add_demo()?
        };
        let raw = b"From: alerts@example.com\r\nReply-To: Support <support@example.com>, Helpers: helper@example.com;\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: multipart/alternative; boundary=y\r\n\r\n--y\r\nContent-Type: text/plain\r\n\r\nFirst section\r\n--y\r\nContent-Type: text/html\r\n\r\n<p>Duplicate HTML</p>\r\n--y--\r\n--x\r\nContent-Type: text/plain\r\n\r\nSecond section\r\n--x\r\nContent-Type: text/plain\r\nContent-Disposition: attachment; filename=private.txt\r\n\r\nAttachment only\r\n--x--\r\n";
        let message = parse_message(&account, "inbox", "INBOX", 1, 1, raw)?;
        assert!(message.body.contains("First section"));
        assert!(message.body.contains("Second section"));
        assert!(!message.body.contains("Duplicate HTML"));
        assert!(!message.body.contains("Attachment only"));
        assert!(
            message.html_body.is_none(),
            "A partial HTML body must not hide other inline sections"
        );
        assert_eq!(
            serde_json::to_value(&message)?["reply_to"],
            serde_json::json!(["support@example.com", "helper@example.com"])
        );
        Ok(())
    }
    #[test]
    fn multipart_mail_uses_text_and_preserves_reply_identity() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let account = {
            let store = crate::store::Store::open(&dir.path().join("mail.db"))?;
            store.add_demo()?
        };
        let raw=b"From: =?UTF-8?Q?Sophie_Chen?= <sophie@example.com>\r\nTo: alex@example.com\r\nSubject: Ideas\r\nMessage-ID: <same@example.com>\r\nContent-Type: multipart/alternative; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nA quiet idea.\r\n--x\r\nContent-Type: text/html\r\n\r\n<img src='https://tracker.invalid'><p>HTML version</p>\r\n--x--\r\n";
        let inbox = parse_message(&account, "inbox", "INBOX", 5, 10, raw)?;
        let archive = parse_message(&account, "archive", "Archive", 6, 11, raw)?;
        assert_eq!(inbox.from_name, "Sophie Chen");
        assert!(inbox.body.contains("A quiet idea."));
        assert!(!inbox.body.contains("tracker.invalid"));
        assert_eq!(
            serde_json::to_value(&inbox)?["html_body"]
                .as_str()
                .map(str::trim),
            Some("<img src='https://tracker.invalid'><p>HTML version</p>")
        );
        assert_eq!(inbox.id, archive.id);
        Ok(())
    }
    #[test]
    fn rsvp_mail_carries_an_itip_reply_that_sync_reads_back() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = crate::store::Store::open(&dir.path().join("mail.db"))?;
        let account = store.add_demo()?;
        let message = store.message("demo-13")?;
        let invite = message.invite.clone().context("demo invitation")?;
        for (locale, subject, body) in [
            (
                Locale::EnUs,
                "Accepted: Identity review",
                "accepted: Identity review",
            ),
            (
                Locale::De,
                "Zugesagt: Identity review",
                "hat zugesagt: Identity review",
            ),
        ] {
            let raw = rsvp_mail(
                &account,
                &message,
                &invite,
                Rsvp::Accepted,
                Some("See you there"),
                locale,
            )?
            .formatted();
            let parsed = mailparse::parse_mail(&raw)?;
            assert_eq!(
                parsed.headers.get_first_value("To").as_deref(),
                Some("maya@studio-north.co")
            );
            assert_eq!(
                parsed.headers.get_first_value("In-Reply-To").as_deref(),
                Some("<demo-13@alice.local>")
            );
            assert!(
                parsed
                    .headers
                    .get_first_value("Subject")
                    .is_some_and(|s| s.starts_with(subject))
            );
            assert_eq!(parsed.ctype.mimetype, "multipart/alternative");
            let calendar = &parsed.subparts[1];
            assert_eq!(
                (
                    calendar.ctype.mimetype.as_str(),
                    calendar.ctype.params.get("method").map(String::as_str)
                ),
                ("text/calendar", Some("REPLY"))
            );
            // The organizer's copy of the answer, as a sync would read it.
            let answer = parse_message(&account, "inbox", "INBOX", 1, 1, &raw)?;
            assert!(answer.body.contains(body));
            assert!(answer.body.contains("See you there"));
            let reply = answer.invite.context("reply parsed")?;
            assert_eq!(
                (reply.method.as_str(), reply.uid.as_str(), reply.response),
                ("REPLY", invite.uid.as_str(), Some(Rsvp::Accepted))
            );
            assert_eq!(reply.event.start, invite.event.start);
        }
        Ok(())
    }
}
