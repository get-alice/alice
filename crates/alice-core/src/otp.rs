//! Conservative local verification-code recognition and durable, at-most-once delivery.
use crate::{
    model::{Message, OtpArrival, Preferences},
    store::Store,
};
use anyhow::Result;
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(crate) const MAX_AGE: i64 = 300;
const FUTURE_SKEW: i64 = 30;

#[derive(Debug, PartialEq, Eq)]
pub struct Code {
    pub code: String,
    /// An explicitly declared lifetime, capped at the delivery's five-minute ceiling.
    pub lifetime: i64,
}

fn authentication(text: &str) -> bool {
    [
        "verification code",
        "verify your",
        "verify code",
        "authentication code",
        "security code",
        "sign-in code",
        "sign in code",
        "signin code",
        "login code",
        "log-in code",
        "one-time",
        "one time",
        "passcode",
        "two-factor",
        "2fa",
        "bestätigungscode",
        "bestaetigungscode",
        "verifizierungscode",
        "bestätigungs-code",
        "sicherheitscode",
        "anmeldecode",
        "anmelde-code",
        "einmalcode",
        "einmal-code",
        "einmalpasswort",
        "verifizierung",
        "authentifizierungscode",
    ]
    .iter()
    .any(|term| text.contains(term))
        || text
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| word == "otp")
}

fn current_text(subject: &str, body: &str) -> Option<String> {
    let subject_lower = subject.trim().to_lowercase();
    if ["re:", "fw:", "fwd:", "aw:", "wg:"]
        .iter()
        .any(|p| subject_lower.starts_with(p))
    {
        return None;
    }
    let mut text = format!("{subject}\n");
    for line in body.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("-----original message")
            || lower.starts_with("-----ursprüngliche nachricht")
            || lower.contains("forwarded message")
            || lower.contains("weitergeleitete nachricht")
            || (lower.starts_with("on ") && lower.ends_with("wrote:"))
            || (lower.starts_with("am ") && lower.contains("schrieb") && lower.ends_with(':'))
            || ["from:", "von:", "sent:", "gesendet:"]
                .iter()
                .any(|p| lower.starts_with(p))
        {
            break;
        }
        if trimmed.starts_with('>') {
            continue;
        }
        // Remove URL tokens rather than interpreting their path, query or fragment as a code.
        for token in line.split_whitespace() {
            let lower = token.to_lowercase();
            if lower.contains("://") || lower.starts_with("www.") || lower.starts_with("mailto:") {
                text.push(' ');
            } else {
                text.push_str(token);
                text.push(' ');
            }
        }
        text.push('\n');
    }
    Some(text)
}

/// Uses the full decoded subject and body, never the truncated preview. False negatives are
/// intentional: ambiguous codes, numeric identifiers, quoted history and URL tokens are unsafe.
pub fn extract(subject: &str, body: &str) -> Option<Code> {
    let text = current_text(subject, body)?;
    let lower = text.to_lowercase();
    if !authentication(&lower) {
        return None;
    }
    let subject_auth = authentication(&subject.to_lowercase());
    let mut found: Option<&str> = None;
    let mut start = None;
    for (index, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if ch.is_alphanumeric() {
            start.get_or_insert(index);
            continue;
        }
        let Some(begin) = start.take() else {
            continue;
        };
        let token = &text[begin..index];
        let numeric = token.bytes().all(|b| b.is_ascii_digit());
        let mixed = token
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            && token.bytes().any(|b| b.is_ascii_uppercase())
            && token.bytes().any(|b| b.is_ascii_digit());
        if !(numeric && (4..=8).contains(&token.len()) || mixed && (6..=8).contains(&token.len())) {
            continue;
        }
        let before: String = text[..begin]
            .chars()
            .rev()
            .take(90)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let after: String = text[index..].chars().take(60).collect();
        let nearby = format!("{before}{after}").to_lowercase();
        if !authentication(&nearby)
            && !(subject_auth
                && (before.to_lowercase().contains("code") || token_is_alone(&text, begin, index)))
        {
            continue;
        }
        // A date, telephone, order, invoice or reference is not a verification code even in
        // a mail that also discusses authentication. Reject numeric date components as well.
        let identifier = [
            "order",
            "invoice",
            "reference",
            "tracking",
            "phone",
            "telephone",
            "tel:",
            "bestell",
            "rechnung",
            "referenz",
            "telefon",
            "date:",
            "datum:",
        ];
        let local_before = before
            .rsplit(['.', '\n'])
            .next()
            .unwrap_or(&before)
            .to_lowercase();
        let local_after = after
            .split(['.', '\n'])
            .next()
            .unwrap_or(&after)
            .to_lowercase();
        if identifier
            .iter()
            .any(|p| local_before.contains(p) || local_after.contains(p))
        {
            continue;
        }
        if numeric {
            if token.len() == 4
                && token
                    .parse::<u32>()
                    .is_ok_and(|n| (1900..=2100).contains(&n))
            {
                continue;
            }
            if token.len() == 8
                && ["%Y%m%d", "%d%m%Y", "%m%d%Y"]
                    .iter()
                    .any(|format| chrono::NaiveDate::parse_from_str(token, format).is_ok())
            {
                continue;
            }
            let left = text[..begin].chars().next_back();
            let right = text[index..].chars().next();
            let followed_by_number = text[index..]
                .chars()
                .nth(1)
                .is_some_and(|c| c.is_ascii_digit());
            let preceded_by_number = text[..begin]
                .chars()
                .rev()
                .nth(1)
                .is_some_and(|c| c.is_ascii_digit());
            if left.is_some_and(|c| "/-+$€£".contains(c))
                || (left == Some('.') && preceded_by_number)
                || (right.is_some_and(|c| "/-.".contains(c)) && followed_by_number)
            {
                continue;
            }
            if ["seconds", "minutes", "sekunden", "minuten"]
                .iter()
                .any(|unit| after.trim_start().to_lowercase().starts_with(unit))
            {
                continue;
            }
        }
        if found.is_some_and(|previous| previous != token) {
            return None;
        }
        found = Some(token);
    }
    Some(Code {
        code: found?.to_string(),
        lifetime: declared_lifetime(&lower).unwrap_or(MAX_AGE).min(MAX_AGE),
    })
}

fn token_is_alone(text: &str, begin: usize, end: usize) -> bool {
    text[..begin]
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .trim()
        .is_empty()
        && text[end..]
            .split('\n')
            .next()
            .unwrap_or("")
            .trim()
            .is_empty()
}

fn declared_lifetime(text: &str) -> Option<i64> {
    let mut window = [""; 4];
    let mut shortest: Option<i64> = None;
    for word in text.split_whitespace() {
        window.rotate_left(1);
        window[3] = word;
        let declared = (window[0] == "expires" && window[1] == "in")
            || (window[0] == "valid" && window[1] == "for")
            || (window[0] == "gültig" && window[1] == "für");
        if !declared {
            continue;
        }
        let Ok(count) = window[2].parse::<i64>() else {
            continue;
        };
        let unit = window[3].trim_matches(|c: char| !c.is_alphabetic());
        let seconds = match unit {
            "second" | "seconds" | "sekunde" | "sekunden" => 1,
            "minute" | "minutes" | "minuten" => 60,
            _ => continue,
        };
        if let Some(lifetime) = count.checked_mul(seconds).filter(|n| *n > 0) {
            shortest = Some(shortest.map_or(lifetime, |previous| previous.min(lifetime)));
        }
    }
    shortest
}

pub(crate) fn recognize(message: &Message) -> Option<Code> {
    // HTML-to-text can erase blockquote semantics. Never interpret a quoted HTML history as
    // a fresh code; plain-text replies are trimmed by current_text above.
    if message.html_body.as_ref().is_some_and(|html| {
        let lower = html.to_lowercase();
        lower.contains("<blockquote")
            || lower.contains("gmail_quote")
            || lower.contains("moz-cite-prefix")
            || lower.contains("divrplyfwdmsg")
            || lower.contains("yahoo_quoted")
    }) {
        return None;
    }
    extract(&message.subject, &message.body)
}

pub(crate) fn enabled(db: &Connection) -> Result<bool> {
    let data: Option<String> = db
        .query_row(
            "SELECT data FROM settings WHERE id='preferences'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(data
        .map(|s| serde_json::from_str::<Preferences>(&s))
        .transpose()?
        .unwrap_or_default()
        .otp_autocopy)
}

pub(crate) fn enqueue(
    tx: &Transaction<'_>,
    message: &Message,
    previous: Option<i64>,
    receipt: Option<&str>,
    now: i64,
) -> Result<()> {
    if previous.is_none() {
        return Ok(());
    }
    if message.folder != "inbox" {
        return Ok(());
    }
    let Some(received_at) = receipt
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.timestamp())
    else {
        return Ok(());
    };
    // UIDNEXT admission establishes novelty. A delivery may predate last_synced when it
    // arrived during the preceding sync, whose successful timestamp is recorded at completion.
    if received_at > now + FUTURE_SKEW || received_at + MAX_AGE <= now {
        return Ok(());
    }
    let Some(code) = recognize(message) else {
        return Ok(());
    };
    let expires_at = received_at + code.lifetime;
    if expires_at <= now {
        return Ok(());
    }
    let mut identity = Sha256::new();
    identity.update((message.account_id.len() as u64).to_le_bytes());
    identity.update(message.account_id.as_bytes());
    if let Some(id) = message
        .message_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
    {
        identity.update(b"message-id");
        identity.update(id.trim().as_bytes());
    } else {
        // Hash borrowed parts without allocating another full body. Receipt/content remain
        // stable when UIDVALIDITY and cache IDs change; lengths prevent field collisions.
        identity.update(b"receipt");
        identity.update(received_at.to_le_bytes());
        for part in [&message.from_email, &message.subject, &message.body] {
            identity.update((part.len() as u64).to_le_bytes());
            identity.update(part.as_bytes());
        }
    }
    let id = Uuid::new_v5(&Uuid::NAMESPACE_URL, &identity.finalize()).to_string();
    let arrival = OtpArrival {
        id,
        message_id: message.id.clone(),
        account_id: message.account_id.clone(),
        from_name: message.from_name.clone(),
        from_email: message.from_email.clone(),
        code: code.code,
        received_at,
        expires_at,
    };
    tx.execute(
        "INSERT OR IGNORE INTO otp_deliveries VALUES(?1,?2,?3,?4,?5,'pending')",
        params![
            arrival.id,
            arrival.account_id,
            received_at,
            expires_at,
            serde_json::to_string(&arrival)?
        ],
    )?;
    Ok(())
}

impl Store {
    /// Cursorless durable delivery: daemon and desktop restarts do not discard fresh events.
    pub fn pending_otp(&self) -> Result<Vec<OtpArrival>> {
        let db = self.db.lock();
        let now = Utc::now().timestamp();
        db.execute("UPDATE otp_deliveries SET status='expired',data=NULL WHERE status='pending' AND (expires_at<=?1 OR received_at>?2)", params![now, now + FUTURE_SKEW])?;
        if !enabled(&db)? {
            return Ok(Vec::new());
        }
        let mut stmt = db.prepare("SELECT data FROM otp_deliveries WHERE status='pending' AND expires_at>?1 AND received_at<=?2 ORDER BY received_at DESC,id ASC")?;
        stmt.query_map(params![now, now + FUTURE_SKEW], |r| r.get::<_, String>(0))?
            .map(|r| Ok(serde_json::from_str(&r?)?))
            .collect()
    }
    /// Claims before the native side effect. A crash may lose a copy, never replay one.
    pub fn claim_otp(&self, id: &str) -> Result<bool> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        if !enabled(&tx)? {
            return Ok(false);
        }
        let now = Utc::now().timestamp();
        let claimed = tx.execute("UPDATE otp_deliveries SET status='claimed',data=NULL WHERE id=?1 AND status='pending' AND expires_at>?2 AND received_at<=?3", params![id, now, now + FUTURE_SKEW])? == 1;
        tx.commit()?;
        Ok(claimed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_recognition_is_contextual_and_conservative() {
        for (subject, body, expected) in [
            (
                "Your verification code",
                "Your verification code is 001234.",
                Some("001234"),
            ),
            (
                "Sign-in code",
                "Use this sign-in code: A0BC12",
                Some("A0BC12"),
            ),
            (
                "Anmeldecode",
                "Ihr Anmeldecode lautet 009876",
                Some("009876"),
            ),
            (
                "Security code",
                "123456\n\nOn Monday, Ana wrote:\nSecurity code: 654321",
                Some("123456"),
            ),
            (
                "Security code",
                "Security code: 123456\n> Security code: 654321",
                Some("123456"),
            ),
            (
                "Verification code",
                "Verification code: 123456 and verification code: 654321",
                None,
            ),
            ("Order", "Your order number is 123456", None),
            (
                "Verification",
                "Verification code order reference 123456",
                None,
            ),
            (
                "Verification code",
                "Call phone 12345678 for a verification code",
                None,
            ),
            (
                "Verification code",
                "Your verification code arrives on 2026-10-12",
                None,
            ),
            (
                "Verification code",
                "Verification code generated on 12102026",
                None,
            ),
            (
                "Verification code",
                "Verification code phone: 1234.5678",
                None,
            ),
            (
                "Verification code",
                "Verification code reference: ABC123",
                None,
            ),
            (
                "Verification code",
                "Verification code: https://example.test/123456",
                None,
            ),
            ("Fwd: Verification code", "Verification code: 123456", None),
            (
                "Hello",
                "-----Original Message-----\nVerification code: 123456",
                None,
            ),
            ("Login code", "Login code: abc123", None),
        ] {
            assert_eq!(
                extract(subject, body).map(|c| c.code),
                expected.map(str::to_string),
                "{subject}: {body}"
            );
        }
        assert_eq!(
            extract(
                "Verification code",
                "Verification code: 123456. Valid for 1 minute."
            )
            .unwrap()
            .lifetime,
            60
        );
        assert_eq!(
            extract(
                "Verification code",
                "Verification code: 123456. Valid for 2 minutes. Expires in 30 seconds."
            )
            .unwrap()
            .lifetime,
            30
        );
    }

    #[test]
    fn html_only_mime_is_decoded_before_recognition() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = Store::open(&dir.path().join("mail.db"))?;
        let account = store.add_demo()?;
        let raw = format!(
            "Subject: =?UTF-8?Q?Your_verification_code?=\r\nContent-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n<p>{}</p><p>Your verification code is <b>001234</b>.</p>",
            "Introductory text. ".repeat(40)
        );
        let mut message =
            crate::providers::parse_message(&account, "inbox", "INBOX", 1, 99, raw.as_bytes())?;
        assert_eq!(message.subject, "Your verification code");
        assert!(!message.preview.contains("001234"));
        assert_eq!(recognize(&message).unwrap().code, "001234");
        message.html_body =
            Some("<blockquote>Your verification code is 001234</blockquote>".into());
        assert!(recognize(&message).is_none());
        Ok(())
    }

    fn imported(
        store: &Store,
        account: &crate::model::Account,
        (uid, validity): (u32, u32),
        identity: &str,
        receipt: i64,
        new_since: Option<u32>,
        body: &str,
    ) -> Result<()> {
        let identity_header = if identity.is_empty() {
            String::new()
        } else {
            format!("Message-ID: <{identity}@example.test>\r\n")
        };
        let raw = format!(
            "From: Security <security@example.test>\r\nSubject: Verification code\r\n{identity_header}\r\n{body}"
        );
        let message = crate::providers::parse_message(
            account,
            "inbox",
            "INBOX",
            validity,
            uid,
            raw.as_bytes(),
        )?;
        store.import_copies(
            &account.id,
            "inbox",
            "INBOX",
            validity,
            new_since,
            vec![crate::sync_store::Incoming {
                uid,
                message_id: message.message_id.clone(),
                is_read: true,
                is_starred: false,
                date: Some(
                    chrono::DateTime::from_timestamp(receipt, 0)
                        .unwrap()
                        .to_rfc3339(),
                ),
                message: Some(message),
            }],
        )?;
        Ok(())
    }

    #[test]
    fn imports_are_fresh_opt_in_deliveries_and_claims_survive_restart_and_reset() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("mail.db");
        let store = Store::open(&path)?;
        assert!(!store.preferences()?.otp_autocopy);
        let mut account = store.add_demo()?;
        account.id = "otp-account".into();
        account.is_demo = false;
        account.last_synced = None;
        store.save_account(&account)?;
        let now = Utc::now().timestamp();
        imported(
            &store,
            &account,
            (1, 1),
            "disabled",
            now - 5,
            Some(1),
            "Verification code: 001234",
        )?;
        assert!(store.pending_otp()?.is_empty());
        store.save_preferences(crate::model::PreferencesPatch {
            otp_autocopy: Some(true),
            ..Default::default()
        })?;
        imported(
            &store,
            &account,
            (2, 1),
            "first-sync",
            now - 5,
            Some(1),
            "Verification code: 001234",
        )?;
        assert!(store.pending_otp()?.is_empty());
        account.last_synced = Some(
            chrono::DateTime::from_timestamp(now - 600, 0)
                .unwrap()
                .to_rfc3339(),
        );
        store.save_account(&account)?;
        for (uid, id, receipt, floor, body) in [
            (3, "backfill", now - 5, None, "Verification code: 001234"),
            (
                4,
                "offline-old",
                now - 301,
                Some(1),
                "Verification code: 001234",
            ),
            (5, "future", now + 60, Some(1), "Verification code: 001234"),
            (
                6,
                "short-expired",
                now - 65,
                Some(1),
                "Verification code: 001234. Valid for 1 minute.",
            ),
        ] {
            imported(&store, &account, (uid, 1), id, receipt, floor, body)?;
        }
        assert!(store.pending_otp()?.is_empty());
        // Already-read mail is eligible; ordinary arrival filtering must not own OTP admission.
        // New UIDs received while a previous sync was running can be older than that
        // sync's completion timestamp; freshness, not last_synced ordering, governs admission.
        account.last_synced = Some(
            chrono::DateTime::from_timestamp(now - 1, 0)
                .unwrap()
                .to_rfc3339(),
        );
        store.save_account(&account)?;
        imported(
            &store,
            &account,
            (7, 1),
            "fresh",
            now - 5,
            Some(1),
            "Verification code: 001234. Valid for 1 minute.",
        )?;
        let event = store.pending_otp()?.pop().unwrap();
        assert_eq!(
            (event.code.as_str(), event.received_at, event.expires_at),
            ("001234", now - 5, now + 55)
        );
        assert_eq!(
            store.message(&event.message_id)?.body,
            "Verification code: 001234. Valid for 1 minute."
        );
        assert_ne!(event.id, event.message_id);
        drop(store);
        let store = std::sync::Arc::new(Store::open(&path)?);
        assert_eq!(
            store.pending_otp()?.as_slice(),
            std::slice::from_ref(&event)
        );
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let store = store.clone();
                let id = event.id.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.claim_otp(&id).unwrap()
                })
            })
            .collect();
        assert_eq!(
            threads
                .into_iter()
                .map(|t| usize::from(t.join().unwrap()))
                .sum::<usize>(),
            1
        );
        assert!(store.pending_otp()?.is_empty());
        drop(store);
        let store = Store::open(&path)?;
        assert!(!store.claim_otp(&event.id)?);
        store.forget_mailbox(&account.id, "INBOX")?;
        imported(
            &store,
            &account,
            (99, 2),
            "fresh",
            now - 5,
            Some(1),
            "Verification code: 001234. Valid for 1 minute.",
        )?;
        assert!(store.pending_otp()?.is_empty());
        imported(
            &store,
            &account,
            (100, 2),
            "next",
            now - 4,
            Some(1),
            "Verification code: 009876",
        )?;
        let pending = store.pending_otp()?.pop().unwrap();
        store.save_preferences(crate::model::PreferencesPatch {
            otp_autocopy: Some(false),
            ..Default::default()
        })?;
        assert!(!store.claim_otp(&pending.id)?);
        store.save_preferences(crate::model::PreferencesPatch {
            otp_autocopy: Some(true),
            ..Default::default()
        })?;
        assert!(store.pending_otp()?.is_empty());
        imported(
            &store,
            &account,
            (101, 2),
            "expires",
            now - 3,
            Some(1),
            "Verification code: 123456",
        )?;
        let expiring = store.pending_otp()?.pop().unwrap();
        store.db.lock().execute(
            "UPDATE otp_deliveries SET expires_at=?2 WHERE id=?1",
            params![expiring.id, Utc::now().timestamp() - 1],
        )?;
        assert!(!store.claim_otp(&expiring.id)?);
        assert!(store.pending_otp()?.is_empty());
        imported(
            &store,
            &account,
            (102, 2),
            "",
            now - 2,
            Some(1),
            "Verification code: 654321",
        )?;
        let fallback = store.pending_otp()?.pop().unwrap();
        assert!(store.claim_otp(&fallback.id)?);
        store.forget_mailbox(&account.id, "INBOX")?;
        imported(
            &store,
            &account,
            (1, 3),
            "",
            now - 2,
            Some(1),
            "Verification code: 654321",
        )?;
        assert!(store.pending_otp()?.is_empty());
        Ok(())
    }
}
