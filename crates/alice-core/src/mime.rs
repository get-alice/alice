//! A message's MIME structure, built either from a fully downloaded message or from the server's
//! IMAP `BODYSTRUCTURE`, and what Alice reads from it: the plain and HTML bodies, and attachments
//! with the IMAP body sections that hold them.
//!
//! Section numbers follow RFC 3501 §6.4.5: a multipart's parts are numbered from 1 below their
//! parent's number, and a single-part message's body is section `1`. Attached messages
//! (`message/rfc822`) are attachments; Alice does not read inside them.

use crate::model::Attachment;
use imap_proto::types::{BodyContentCommon, BodyContentSinglePart, BodyStructure, ContentEncoding};
use mailparse::{DispositionType, ParsedMail};

/// Text parts larger than this are not downloaded; the message shows a notice instead.
pub const TEXT_LIMIT: usize = 4_000_000;
/// Calendar parts (invitations) larger than this are not downloaded.
pub const CALENDAR_LIMIT: usize = 1_000_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Part {
    /// IMAP body section; empty for a multipart message root, which has no section of its own.
    pub section: String,
    /// Lowercase `type/subtype`.
    pub mimetype: String,
    pub charset: Option<String>,
    /// Lowercase `Content-Transfer-Encoding`.
    pub encoding: String,
    /// `Content-Disposition: attachment`.
    pub attachment: bool,
    pub filename: Option<String>,
    /// `Content-ID` without its angle brackets: how the HTML body refers to this part (`cid:…`).
    pub content_id: Option<String>,
    /// Decoded size in bytes; estimated from the encoded size for a server structure.
    pub size: usize,
    pub children: Vec<Part>,
    /// Decoded text of a text part, once downloaded.
    pub text: Option<String>,
}

fn child_section(parent: &str, index: usize) -> String {
    if parent.is_empty() {
        (index + 1).to_string()
    } else {
        format!("{parent}.{}", index + 1)
    }
}

/// Decodes RFC 2047 encoded words (`=?UTF-8?B?…?=`) that some mailers put in parameter values.
fn decode_words(value: &str) -> String {
    if !value.contains("=?") {
        return value.to_string();
    }
    mailparse::parse_header(format!("X: {value}").as_bytes())
        .map(|(header, _)| header.get_value())
        .unwrap_or_else(|_| value.to_string())
}

/// Decodes `Content-Type`/`Content-Disposition` parameters given as name/value pairs (as in a
/// `BODYSTRUCTURE`): RFC 2231 charsets (`name*=UTF-8''%E2%82%AC`) and continuations
/// (`name*0*=…; name*1*=…`), and RFC 2047 encoded words in plain values.
fn parameter(pairs: &[(&str, &str)], wanted: &str) -> Option<String> {
    let mut plain = None;
    let mut extended = None;
    let mut segments: Vec<(u32, bool, &str)> = Vec::new();
    for (key, value) in pairs {
        let key = key.to_ascii_lowercase();
        let Some(rest) = key.strip_prefix(wanted) else {
            continue;
        };
        if rest.is_empty() {
            plain = Some(decode_words(value));
        } else if rest == "*" {
            extended = Some(*value);
        } else if let Some(index) = rest.strip_prefix('*') {
            let (number, encoded) = index
                .strip_suffix('*')
                .map_or((index, false), |n| (n, true));
            if let Ok(number) = number.parse() {
                segments.push((number, encoded, value));
            }
        }
    }
    let decode = |charset: &str, encoded: &str| -> String {
        let bytes = percent_decode(encoded);
        charset::Charset::for_label(charset.as_bytes())
            .map(|charset| charset.decode_without_bom_handling(&bytes).0.into_owned())
            .unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned())
    };
    if let Some(value) = extended {
        let mut fields = value.splitn(3, '\'');
        let (charset, _language, text) = (fields.next()?, fields.next()?, fields.next()?);
        return Some(decode(charset, text));
    }
    if !segments.is_empty() {
        segments.sort_by_key(|(number, _, _)| *number);
        // The charset comes from the first encoded segment; every encoded segment is percent-encoded.
        let charset = segments
            .iter()
            .find(|(_, encoded, _)| *encoded)
            .and_then(|(_, _, value)| value.split('\'').next())
            .unwrap_or("utf-8")
            .to_string();
        let mut bytes = Vec::new();
        for (index, (_, encoded, value)) in segments.iter().enumerate() {
            if *encoded {
                let value = if index == 0 {
                    value.splitn(3, '\'').nth(2).unwrap_or("")
                } else {
                    value
                };
                bytes.extend(percent_decode(value));
            } else {
                bytes.extend(value.as_bytes());
            }
        }
        return Some(
            charset::Charset::for_label(charset.as_bytes())
                .map(|c| c.decode_without_bom_handling(&bytes).0.into_owned())
                .unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned()),
        );
    }
    plain
}

pub(crate) fn percent_decode(value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(byte) = bytes
                .get(index + 1..index + 3)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            output.push(byte);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    output
}

/// A `Content-ID` value without its angle brackets.
fn content_id(value: &str) -> Option<String> {
    let id = value
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim();
    (!id.is_empty()).then(|| id.to_string())
}

fn encoding_name(encoding: &ContentEncoding<'_>) -> String {
    match encoding {
        ContentEncoding::SevenBit => "7bit".into(),
        ContentEncoding::EightBit => "8bit".into(),
        ContentEncoding::Binary => "binary".into(),
        ContentEncoding::Base64 => "base64".into(),
        ContentEncoding::QuotedPrintable => "quoted-printable".into(),
        ContentEncoding::Other(other) => other.to_ascii_lowercase(),
    }
}

/// The decoded size of `octets` encoded bytes.
fn decoded_size(octets: usize, encoding: &str) -> usize {
    // Base64 carries 3 bytes in 4 characters, plus a line break every 76 characters.
    if encoding == "base64" {
        octets / 78 * 57 + (octets % 78) * 3 / 4
    } else {
        octets
    }
}

impl Part {
    /// The structure of a fully downloaded message, with every text part already decoded.
    pub fn from_parsed(mail: &ParsedMail<'_>) -> Self {
        Self::parsed(
            mail,
            if mail.subparts.is_empty() {
                "1".into()
            } else {
                String::new()
            },
        )
    }
    fn parsed(mail: &ParsedMail<'_>, section: String) -> Self {
        let disposition = mail.get_content_disposition();
        let attachment = disposition.disposition == DispositionType::Attachment;
        let filename = disposition
            .params
            .get("filename")
            .or_else(|| mail.ctype.params.get("name"))
            .map(|name| decode_words(name));
        let encoding = mail
            .headers
            .iter()
            .find(|h| {
                h.get_key_ref()
                    .eq_ignore_ascii_case("Content-Transfer-Encoding")
            })
            .map(|h| h.get_value().trim().to_ascii_lowercase())
            .unwrap_or_else(|| "7bit".into());
        let content_id = mail
            .headers
            .iter()
            .find(|h| h.get_key_ref().eq_ignore_ascii_case("Content-ID"))
            .and_then(|h| content_id(&h.get_value()));
        let mimetype = mail.ctype.mimetype.to_ascii_lowercase();
        let children: Vec<Part> = mail
            .subparts
            .iter()
            .enumerate()
            .map(|(index, part)| Self::parsed(part, child_section(&section, index)))
            .collect();
        let leaf = children.is_empty();
        let text = if leaf && !attachment && (mimetype == "text/plain" || mimetype == "text/html") {
            mail.get_body().ok()
        } else if leaf && is_calendar_type(&mimetype) {
            // iCalendar text is UTF-8 unless a charset says otherwise (RFC 5545 §3.1.4).
            if mail.ctype.params.contains_key("charset") {
                mail.get_body().ok()
            } else {
                mail.get_body_raw()
                    .ok()
                    .map(|raw| String::from_utf8_lossy(&raw).into_owned())
            }
            .filter(|text| text.len() <= CALENDAR_LIMIT)
        } else {
            None
        };
        Self {
            size: if leaf {
                mail.get_body_raw().map(|body| body.len()).unwrap_or(0)
            } else {
                0
            },
            section,
            charset: Some(mail.ctype.charset.clone()),
            encoding,
            attachment,
            filename,
            content_id,
            mimetype,
            children,
            text,
        }
    }
    /// The structure the server reports in `BODYSTRUCTURE`; text parts still need downloading
    /// (see [`Self::text_parts`] and [`Self::set_text`]).
    pub fn from_structure(structure: &BodyStructure<'_>) -> Self {
        let section = if matches!(structure, BodyStructure::Multipart { .. }) {
            String::new()
        } else {
            "1".into()
        };
        Self::structure(structure, section)
    }
    fn structure(structure: &BodyStructure<'_>, section: String) -> Self {
        let single = |common: &BodyContentCommon<'_>, other: &BodyContentSinglePart<'_>| {
            let mut part = Self::common(common, section.clone());
            part.encoding = encoding_name(&other.transfer_encoding);
            part.content_id = other.id.and_then(content_id);
            part.size = decoded_size(other.octets as usize, &part.encoding);
            part
        };
        match structure {
            BodyStructure::Basic { common, other, .. }
            | BodyStructure::Text { common, other, .. }
            | BodyStructure::Message { common, other, .. } => single(common, other),
            BodyStructure::Multipart { common, bodies, .. } => {
                let mut part = Self::common(common, section.clone());
                part.children = bodies
                    .iter()
                    .enumerate()
                    .map(|(index, body)| Self::structure(body, child_section(&section, index)))
                    .collect();
                part
            }
        }
    }
    fn common(common: &BodyContentCommon<'_>, section: String) -> Self {
        let params = common.ty.params.as_deref().unwrap_or(&[]);
        let disposition = common.disposition.as_ref();
        let disposition_params = disposition.and_then(|d| d.params.as_deref()).unwrap_or(&[]);
        Self {
            section,
            mimetype: format!("{}/{}", common.ty.ty, common.ty.subtype).to_ascii_lowercase(),
            charset: parameter(params, "charset"),
            encoding: "7bit".into(),
            attachment: disposition.is_some_and(|d| d.ty.eq_ignore_ascii_case("attachment")),
            filename: parameter(disposition_params, "filename")
                .or_else(|| parameter(params, "name")),
            ..Default::default()
        }
    }
    fn leaves(&self) -> Box<dyn Iterator<Item = &Part> + '_> {
        if self.children.is_empty() {
            Box::new(std::iter::once(self))
        } else {
            Box::new(self.children.iter().flat_map(|child| child.leaves()))
        }
    }
    fn is_text_body(&self) -> bool {
        self.children.is_empty()
            && !self.attachment
            && (self.mimetype == "text/plain" || self.mimetype == "text/html")
    }
    fn is_calendar(&self) -> bool {
        self.children.is_empty() && is_calendar_type(&self.mimetype)
    }
    /// The parts Alice reads and still needs to download: displayable text up to [`TEXT_LIMIT`]
    /// each, and calendar parts (inline or attached) up to [`CALENDAR_LIMIT`]. Calendar parts are
    /// never shown as the body.
    pub fn text_parts(&self) -> Vec<&Part> {
        self.leaves()
            .filter(|part| {
                part.text.is_none()
                    && ((part.is_text_body() && part.size <= TEXT_LIMIT)
                        || (part.is_calendar() && part.size <= CALENDAR_LIMIT))
            })
            .collect()
    }
    /// The charset to decode this part's text with: its own, else UTF-8 for iCalendar.
    pub fn text_charset(&self) -> Option<&str> {
        self.charset
            .as_deref()
            .or_else(|| self.is_calendar().then_some("utf-8"))
    }
    /// The text of the first downloaded calendar part: the invitation a message carries.
    pub fn calendar(&self) -> Option<&str> {
        self.leaves()
            .find(|part| part.is_calendar() && part.text.is_some())?
            .text
            .as_deref()
    }
    /// Whether a displayable text part was too large to download.
    pub fn has_oversized_text(&self) -> bool {
        self.leaves()
            .any(|part| part.is_text_body() && part.size > TEXT_LIMIT)
    }
    pub fn find(&self, section: &str) -> Option<&Part> {
        if self.section == section {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(section))
    }
    fn find_mut(&mut self, section: &str) -> Option<&mut Part> {
        if self.section == section {
            return Some(self);
        }
        self.children
            .iter_mut()
            .find_map(|child| child.find_mut(section))
    }
    /// Stores the decoded text of the text part at `section`.
    pub fn set_text(&mut self, section: &str, text: String) {
        if let Some(part) = self
            .find_mut(section)
            .filter(|part| part.children.is_empty())
        {
            part.text = Some(text);
        }
    }
    /// The plain text body: every inline text section, with one choice per `multipart/alternative`
    /// (plain preferred; HTML converted to text when it is the only choice).
    pub fn plain_body(&self) -> Option<String> {
        if self.attachment {
            return None;
        }
        if self.children.is_empty() {
            return match self.mimetype.as_str() {
                "text/plain" => self.text.clone(),
                "text/html" => self
                    .text
                    .as_ref()
                    .and_then(|html| html2text::from_read(html.as_bytes(), 80).ok()),
                _ => None,
            };
        }
        if self.mimetype == "multipart/alternative" {
            return self
                .children
                .iter()
                .filter(|part| part.mimetype != "text/html")
                .find_map(Part::plain_body)
                .or_else(|| self.children.iter().find_map(Part::plain_body));
        }
        let parts: Vec<_> = self.children.iter().filter_map(Part::plain_body).collect();
        (!parts.is_empty()).then(|| parts.join("\n\n"))
    }
    /// The HTML body, when HTML represents the whole message.
    pub fn html_body(&self) -> Option<String> {
        if self.attachment {
            return None;
        }
        if self.mimetype == "text/html" {
            return self.text.clone();
        }
        // A partial HTML body must not hide other inline sections of a mixed message.
        if self.mimetype != "multipart/alternative"
            && self.children.iter().filter_map(Part::plain_body).count() > 1
        {
            return None;
        }
        self.children.iter().find_map(Part::html_body)
    }
    /// Named parts, attached messages and unnamed parts marked as attachments, in message order.
    pub fn attachments(&self) -> Vec<Attachment> {
        let mut found = Vec::new();
        self.collect(&mut found);
        found
    }
    /// The name a part is offered under as an attachment: its file name, or for an attached
    /// message, an unnamed part marked as an attachment or an unnamed embedded image, a generic
    /// name by type.
    pub fn attachment_name(&self) -> Option<String> {
        if let Some(name) = &self.filename {
            return Some(name.clone());
        }
        if self.mimetype == "message/rfc822" {
            return Some("Attached message.eml".into());
        }
        let embedded = self.content_id.is_some() && self.mimetype.starts_with("image/");
        if !(self.attachment || embedded) || !self.children.is_empty() {
            return None;
        }
        let extension = match self.mimetype.as_str() {
            "application/pdf" => ".pdf",
            "application/zip" => ".zip",
            "text/plain" => ".txt",
            "text/html" => ".html",
            "text/calendar" => ".ics",
            "text/csv" => ".csv",
            "image/jpeg" => ".jpg",
            "image/png" => ".png",
            "image/gif" => ".gif",
            _ => "",
        };
        Some(format!("Attachment{extension}"))
    }
    fn collect(&self, output: &mut Vec<Attachment>) {
        if let Some(name) = self.attachment_name() {
            output.push(Attachment {
                name,
                size: self.size,
                part: (!self.section.is_empty()).then(|| self.section.clone()),
                content_type: Some(self.mimetype.clone()),
                content_id: self.content_id.clone(),
            });
        }
        for child in &self.children {
            child.collect(output);
        }
    }
}

/// iCalendar types: `text/calendar`, and `application/ics` that some mailers attach instead.
fn is_calendar_type(mimetype: &str) -> bool {
    mimetype == "text/calendar" || mimetype == "application/ics"
}

/// Undoes a part's `Content-Transfer-Encoding` (base64, quoted-printable; others pass through).
pub fn decode_bytes(raw: &[u8], encoding: &str) -> Vec<u8> {
    let mut wrapped = format!("Content-Transfer-Encoding: {encoding}\r\n\r\n").into_bytes();
    wrapped.extend_from_slice(raw);
    mailparse::parse_mail(&wrapped)
        .and_then(|mail| mail.get_body_raw())
        .unwrap_or_else(|_| raw.to_vec())
}

/// Decodes a text part's transfer encoding and charset.
pub fn decode_text(raw: &[u8], encoding: &str, charset: Option<&str>) -> String {
    let charset = charset
        .unwrap_or("us-ascii")
        .replace(['"', '\\', '\r', '\n'], "");
    let mut wrapped = format!("Content-Type: text/plain; charset=\"{charset}\"\r\nContent-Transfer-Encoding: {encoding}\r\n\r\n").into_bytes();
    wrapped.extend_from_slice(raw);
    mailparse::parse_mail(&wrapped)
        .and_then(|mail| mail.get_body())
        .unwrap_or_else(|_| String::from_utf8_lossy(raw).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use imap_proto::types::{ContentDisposition, ContentType};

    fn leaf<'a>(
        ty: &'a str,
        subtype: &'a str,
        params: Vec<(&'a str, &'a str)>,
        disposition: Option<ContentDisposition<'a>>,
        encoding: ContentEncoding<'a>,
        octets: u32,
    ) -> BodyStructure<'a> {
        let common = BodyContentCommon {
            ty: ContentType {
                ty,
                subtype,
                params: Some(params),
            },
            disposition,
            language: None,
            location: None,
        };
        let other = BodyContentSinglePart {
            id: None,
            md5: None,
            description: None,
            transfer_encoding: encoding,
            octets,
        };
        if ty.eq_ignore_ascii_case("text") {
            BodyStructure::Text {
                common,
                other,
                lines: 1,
                extension: None,
            }
        } else {
            BodyStructure::Basic {
                common,
                other,
                extension: None,
            }
        }
    }
    fn multipart<'a>(subtype: &'a str, bodies: Vec<BodyStructure<'a>>) -> BodyStructure<'a> {
        BodyStructure::Multipart {
            common: BodyContentCommon {
                ty: ContentType {
                    ty: "multipart",
                    subtype,
                    params: None,
                },
                disposition: None,
                language: None,
                location: None,
            },
            bodies,
            extension: None,
        }
    }

    #[test]
    fn server_structure_picks_text_sections_and_numbers_attachments_like_imap() {
        let attachment = |params| {
            Some(ContentDisposition {
                ty: "attachment",
                params: Some(params),
            })
        };
        let structure = multipart(
            "mixed",
            vec![
                multipart(
                    "alternative",
                    vec![
                        leaf(
                            "TEXT",
                            "PLAIN",
                            vec![("CHARSET", "utf-8")],
                            None,
                            ContentEncoding::QuotedPrintable,
                            120,
                        ),
                        leaf(
                            "TEXT",
                            "HTML",
                            vec![("CHARSET", "utf-8")],
                            None,
                            ContentEncoding::Base64,
                            400,
                        ),
                    ],
                ),
                leaf(
                    "APPLICATION",
                    "PDF",
                    vec![("NAME", "ignored.pdf")],
                    attachment(vec![("FILENAME*", "UTF-8''Rechnung%20%E2%82%AC.pdf")]),
                    ContentEncoding::Base64,
                    78_000,
                ),
                leaf(
                    "IMAGE",
                    "PNG",
                    vec![],
                    attachment(vec![
                        ("filename*0*", "UTF-8''very%20long"),
                        ("filename*1", " name.png"),
                    ]),
                    ContentEncoding::Base64,
                    780,
                ),
                leaf(
                    "TEXT",
                    "PLAIN",
                    vec![],
                    attachment(vec![("filename", "=?UTF-8?B?bm90ZXMudHh0?=")]),
                    ContentEncoding::SevenBit,
                    30,
                ),
                leaf(
                    "TEXT",
                    "HTML",
                    vec![],
                    None,
                    ContentEncoding::SevenBit,
                    TEXT_LIMIT as u32 + 1,
                ),
                // Marked as an attachment without any name.
                leaf(
                    "APPLICATION",
                    "PDF",
                    vec![],
                    attachment(vec![]),
                    ContentEncoding::Base64,
                    78,
                ),
            ],
        );
        let part = Part::from_structure(&structure);
        assert_eq!(
            part.text_parts()
                .iter()
                .map(|p| p.section.as_str())
                .collect::<Vec<_>>(),
            ["1.1", "1.2"]
        );
        assert!(part.has_oversized_text());
        let attachments = part.attachments();
        assert_eq!(
            attachments
                .iter()
                .map(|a| (a.name.as_str(), a.part.as_deref().unwrap()))
                .collect::<Vec<_>>(),
            [
                ("Rechnung €.pdf", "2"),
                ("very long name.png", "3"),
                ("notes.txt", "4"),
                ("Attachment.pdf", "6")
            ]
        );
        assert_eq!(attachments[0].size, 57_000);
        assert_eq!(
            attachments[0].content_type.as_deref(),
            Some("application/pdf")
        );
        // A single-part message's body is section 1.
        let single = Part::from_structure(&leaf(
            "TEXT",
            "PLAIN",
            vec![],
            None,
            ContentEncoding::SevenBit,
            5,
        ));
        assert_eq!(single.text_parts()[0].section, "1");
    }

    #[test]
    fn server_structure_and_full_parse_read_the_same_bodies_and_attachments() -> anyhow::Result<()>
    {
        let raw = b"Content-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: multipart/alternative; boundary=y\r\n\r\n--y\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nCaf=C3=A9 plans\r\n--y\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Caf\xc3\xa9 plans</p>\r\n--y--\r\n--x\r\nContent-Type: application/pdf; name=plan.pdf\r\nContent-Disposition: attachment; filename=plan.pdf\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0=\r\n--x--\r\n";
        let parsed = Part::from_parsed(&mailparse::parse_mail(raw)?);
        let mut structured = Part::from_structure(&multipart(
            "mixed",
            vec![
                multipart(
                    "alternative",
                    vec![
                        leaf(
                            "text",
                            "plain",
                            vec![("charset", "utf-8")],
                            None,
                            ContentEncoding::QuotedPrintable,
                            17,
                        ),
                        leaf(
                            "text",
                            "html",
                            vec![("charset", "utf-8")],
                            None,
                            ContentEncoding::SevenBit,
                            18,
                        ),
                    ],
                ),
                leaf(
                    "application",
                    "pdf",
                    vec![("name", "plan.pdf")],
                    Some(ContentDisposition {
                        ty: "attachment",
                        params: Some(vec![("filename", "plan.pdf")]),
                    }),
                    ContentEncoding::Base64,
                    8,
                ),
            ],
        ));
        structured.set_text(
            "1.1",
            decode_text(b"Caf=C3=A9 plans", "quoted-printable", Some("utf-8")),
        );
        structured.set_text(
            "1.2",
            decode_text(b"<p>Caf\xc3\xa9 plans</p>", "7bit", Some("utf-8")),
        );
        assert_eq!(
            parsed.plain_body().as_deref().map(str::trim),
            Some("Café plans")
        );
        assert_eq!(structured.plain_body(), parsed.plain_body());
        assert_eq!(structured.html_body(), parsed.html_body());
        let names = |part: &Part| {
            part.attachments()
                .into_iter()
                .map(|a| (a.name, a.part))
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&structured), names(&parsed));
        assert_eq!(
            names(&parsed),
            [("plan.pdf".to_string(), Some("2".to_string()))]
        );
        assert_eq!(decode_bytes(b"JVBERi0=", "base64"), b"%PDF-");
        Ok(())
    }

    #[test]
    fn calendar_parts_are_downloaded_but_never_shown_as_the_body() -> anyhow::Result<()> {
        // Google's layout: the invitation inline in the alternative, and again as an attachment.
        let raw = b"Content-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: multipart/alternative; boundary=y\r\n\r\n--y\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\nYou have been invited\r\n--y\r\nContent-Type: text/html; charset=UTF-8\r\n\r\n<p>You have been invited</p>\r\n--y\r\nContent-Type: text/calendar; charset=UTF-8; method=REQUEST\r\nContent-Transfer-Encoding: base64\r\n\r\nQkVHSU46VkNBTEVOREFSDQpTVU1NQVJZOkNhZsOpDQpFTkQ6VkNBTEVOREFSDQo=\r\n--y--\r\n--x\r\nContent-Type: application/ics; name=invite.ics\r\nContent-Disposition: attachment; filename=invite.ics\r\n\r\nBEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n--x--\r\n";
        let parsed = Part::from_parsed(&mailparse::parse_mail(raw)?);
        assert_eq!(
            parsed.calendar(),
            Some("BEGIN:VCALENDAR\r\nSUMMARY:Café\r\nEND:VCALENDAR\r\n")
        );
        assert_eq!(
            parsed.plain_body().as_deref().map(str::trim),
            Some("You have been invited")
        );
        assert_eq!(
            parsed.html_body().as_deref().map(str::trim),
            Some("<p>You have been invited</p>")
        );
        let attachment = |name| {
            Some(ContentDisposition {
                ty: "attachment",
                params: Some(vec![("filename", name)]),
            })
        };
        let mut structured = Part::from_structure(&multipart(
            "mixed",
            vec![
                multipart(
                    "alternative",
                    vec![
                        leaf(
                            "text",
                            "plain",
                            vec![("charset", "utf-8")],
                            None,
                            ContentEncoding::SevenBit,
                            21,
                        ),
                        leaf(
                            "text",
                            "calendar",
                            vec![("method", "REQUEST")],
                            None,
                            ContentEncoding::Base64,
                            60,
                        ),
                    ],
                ),
                leaf(
                    "application",
                    "ics",
                    vec![],
                    attachment("invite.ics"),
                    ContentEncoding::SevenBit,
                    40,
                ),
                leaf(
                    "text",
                    "calendar",
                    vec![],
                    attachment("huge.ics"),
                    ContentEncoding::SevenBit,
                    CALENDAR_LIMIT as u32 + 1,
                ),
            ],
        ));
        let wanted: Vec<&str> = structured
            .text_parts()
            .iter()
            .map(|p| p.section.as_str())
            .collect();
        assert_eq!(wanted, ["1.1", "1.2", "2"]);
        let calendar = structured.find("1.2").unwrap();
        let text = decode_text(
            b"QkVHSU46VkNBTEVOREFSDQpTVU1NQVJZOkNhZsOpDQpFTkQ6VkNBTEVOREFSDQo=",
            &calendar.encoding,
            calendar.text_charset(),
        );
        structured.set_text("1.2", text);
        structured.set_text("1.1", "You have been invited".into());
        assert_eq!(structured.calendar(), parsed.calendar());
        assert_eq!(
            structured.plain_body().as_deref(),
            Some("You have been invited")
        );
        assert_eq!(structured.html_body(), None);
        Ok(())
    }

    #[test]
    fn embedded_images_are_attachments_with_their_content_id() -> anyhow::Result<()> {
        let raw = b"Content-Type: multipart/related; boundary=b\r\n\r\n--b\r\nContent-Type: text/html\r\n\r\n<img src=\"cid:ii_1\">\r\n--b\r\nContent-Type: image/png\r\nContent-ID: <ii_1>\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0K\r\n--b--\r\n";
        let parsed = Part::from_parsed(&mailparse::parse_mail(raw)?);
        let attachments = parsed.attachments();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].name, "Attachment.png");
        assert_eq!(attachments[0].content_id.as_deref(), Some("ii_1"));
        assert_eq!(attachments[0].part.as_deref(), Some("2"));
        Ok(())
    }
}
