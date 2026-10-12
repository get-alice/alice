//! Mail search: an fsearch-style query language, matched against the store's search index.
//!
//! Terms separated by spaces must all match; `OR` (or `||`) offers alternatives, `NOT`, `!` or a
//! leading `-` excludes, and parentheses group. A quoted phrase matches as written. Field terms
//! narrow a word to one part of the message: `from:`, `to:`, `subject:` and `body:`. Filters:
//! `is:unread`, `is:read`, `is:starred`, `has:attachment`, and `after:`, `before:` or `date:`
//! with `today`, `yesterday`, `2026`, `2026-10`, `2026-10-11`, or an age like `7d`, `2w`, `3m`, `1y`.
//! An unknown `word:value` is searched as text, so URLs and times still work.
//!
//! Matching ignores case and accents. Sender, recipients and subject match any substring, and
//! unquoted words of four or more letters also match with a typo or two. Bodies match whole words
//! and word beginnings through a full-text index.
use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone};
use std::collections::HashSet;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Any,
    From,
    To,
    Subject,
    Body,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// Folded text; `phrase` when quoted.
    Text {
        field: Field,
        text: String,
        phrase: bool,
    },
    Read(bool),
    Starred,
    Attachment,
    /// A half-open `[from, to)` range in Julian days.
    Date {
        from: Option<f64>,
        to: Option<f64>,
    },
    Not(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

/// Lowercases and strips accents, so `Héllo` and `hello` compare equal.
pub fn fold(text: &str) -> String {
    if text.is_ascii() {
        return text.to_ascii_lowercase();
    }
    text.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Debug, PartialEq)]
enum Token {
    Open,
    Close,
    And,
    Or,
    Not,
    Term {
        key: Option<String>,
        value: String,
        quoted: bool,
    },
}

fn tokenize(query: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = query.chars().peekable();
    let quoted = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>| {
        chars.next();
        let mut value = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' if chars.peek() == Some(&'"') => value.push(chars.next().unwrap_or('"')),
                c => value.push(c),
            }
        }
        value
    };
    while let Some(&c) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                chars.next();
            }
            '(' => {
                chars.next();
                tokens.push(Token::Open);
            }
            ')' => {
                chars.next();
                tokens.push(Token::Close);
            }
            '"' => tokens.push(Token::Term {
                key: None,
                value: quoted(&mut chars),
                quoted: true,
            }),
            _ => {
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace()
                        || c == '('
                        || c == ')'
                        || (c == '"' && !word.ends_with(':'))
                    {
                        break;
                    }
                    if c == '"' {
                        let value = quoted(&mut chars);
                        word.pop();
                        tokens.push(Token::Term {
                            key: Some(word),
                            value,
                            quoted: true,
                        });
                        word = String::new();
                        break;
                    }
                    word.push(c);
                    chars.next();
                }
                if word.is_empty() {
                    continue;
                }
                match word.as_str() {
                    "OR" | "||" => tokens.push(Token::Or),
                    "AND" | "&&" => tokens.push(Token::And),
                    "NOT" | "!" | "-" => tokens.push(Token::Not),
                    _ if (word.starts_with('!') || word.starts_with('-')) && word.len() > 1 => {
                        tokens.push(Token::Not);
                        tokens.extend(tokenize(&word[1..]));
                    }
                    _ => match word.split_once(':') {
                        Some((key, value)) if !key.is_empty() && !value.is_empty() => {
                            tokens.push(Token::Term {
                                key: Some(key.into()),
                                value: value.into(),
                                quoted: false,
                            })
                        }
                        _ => tokens.push(Token::Term {
                            key: None,
                            value: word,
                            quoted: false,
                        }),
                    },
                }
            }
        }
    }
    tokens
}

struct Parser<'a> {
    tokens: Vec<Token>,
    at: usize,
    now: &'a DateTime<Local>,
}
impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }
    fn or(&mut self, depth: usize) -> Option<Expr> {
        let mut options = Vec::new();
        loop {
            options.extend(self.and(depth));
            match self.peek() {
                Some(Token::Or) => self.at += 1,
                // A stray `)` at the top level is ignored so half-typed queries still search.
                Some(Token::Close) if depth == 0 => self.at += 1,
                _ => break,
            }
        }
        collapse(options, Expr::Or)
    }
    fn and(&mut self, depth: usize) -> Option<Expr> {
        let mut terms = Vec::new();
        while let Some(token) = self.peek() {
            match token {
                Token::Or | Token::Close => break,
                Token::And => self.at += 1,
                _ => terms.extend(self.unary(depth)),
            }
        }
        collapse(terms, Expr::And)
    }
    fn unary(&mut self, depth: usize) -> Option<Expr> {
        let token = self.tokens.get(self.at)?;
        self.at += 1;
        match token {
            Token::Not => self.unary(depth).map(|e| Expr::Not(Box::new(e))),
            Token::Open => {
                let inner = self.or(depth + 1);
                if self.peek() == Some(&Token::Close) {
                    self.at += 1;
                }
                inner
            }
            Token::Term { key, value, quoted } => term(key.as_deref(), value, *quoted, self.now),
            Token::Close | Token::And | Token::Or => None,
        }
    }
}
fn collapse(mut items: Vec<Expr>, wrap: fn(Vec<Expr>) -> Expr) -> Option<Expr> {
    match items.len() {
        0 => None,
        1 => items.pop(),
        _ => Some(wrap(items)),
    }
}

fn term(key: Option<&str>, value: &str, quoted: bool, now: &DateTime<Local>) -> Option<Expr> {
    let text = |field| {
        let text = fold(value.trim());
        (!text.is_empty()).then_some(Expr::Text {
            field,
            text,
            phrase: quoted,
        })
    };
    let Some(key) = key else {
        return text(Field::Any);
    };
    let known = match (
        key.to_ascii_lowercase().as_str(),
        value.to_ascii_lowercase().as_str(),
    ) {
        ("from", _) => return text(Field::From),
        ("to", _) => return text(Field::To),
        ("subject", _) => return text(Field::Subject),
        ("body", _) => return text(Field::Body),
        ("is", "unread") => Some(Expr::Read(false)),
        ("is", "read") => Some(Expr::Read(true)),
        ("is", "starred" | "flagged") => Some(Expr::Starred),
        ("has", "attachment" | "attachments") => Some(Expr::Attachment),
        ("after", value) => period(value, now).map(|(from, _)| Expr::Date {
            from: Some(from),
            to: None,
        }),
        ("before", value) => period(value, now).map(|(from, _)| Expr::Date {
            from: None,
            to: Some(from),
        }),
        ("date", value) => period(value, now).map(|(from, to)| Expr::Date {
            from: Some(from),
            to: Some(to),
        }),
        _ => None,
    };
    known.or_else(|| {
        let text = fold(&format!("{key}:{value}"));
        Some(Expr::Text {
            field: Field::Any,
            text,
            phrase: quoted,
        })
    })
}

/// The `[start, end)` Julian-day range a date value names, in local time.
fn period(value: &str, now: &DateTime<Local>) -> Option<(f64, f64)> {
    let day = |date: NaiveDate| {
        Some(julian(
            &Local
                .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
                .earliest()?,
        ))
    };
    let today = now.date_naive();
    let days = |start: NaiveDate, end: NaiveDate| Some((day(start)?, day(end)?));
    match value {
        "today" => return days(today, today.succ_opt()?),
        "yesterday" => return days(today.pred_opt()?, today),
        _ => {}
    }
    if let Some(unit) = value.chars().last().filter(|c| "dwmy".contains(*c))
        && let Ok(count) = value[..value.len() - 1].parse::<i64>()
    {
        let span = Duration::days(
            count
                * match unit {
                    'd' => 1,
                    'w' => 7,
                    'm' => 30,
                    _ => 365,
                },
        );
        return Some((julian(&(*now - span)), julian(now) + 1.0 / 86_400.0));
    }
    let parts: Vec<_> = value.split(['-', '/']).collect();
    let numbers: Vec<u32> = parts
        .iter()
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    match numbers[..] {
        [year] if parts[0].len() == 4 => days(
            NaiveDate::from_ymd_opt(year as i32, 1, 1)?,
            NaiveDate::from_ymd_opt(year as i32 + 1, 1, 1)?,
        ),
        [year, month] => {
            let start = NaiveDate::from_ymd_opt(year as i32, month, 1)?;
            let end = if month == 12 {
                NaiveDate::from_ymd_opt(year as i32 + 1, 1, 1)?
            } else {
                NaiveDate::from_ymd_opt(year as i32, month + 1, 1)?
            };
            days(start, end)
        }
        [year, month, date] => {
            let start = NaiveDate::from_ymd_opt(year as i32, month, date)?;
            days(start, start.succ_opt()?)
        }
        _ => None,
    }
}
fn julian<Tz: TimeZone>(at: &DateTime<Tz>) -> f64 {
    at.timestamp_millis() as f64 / 86_400_000.0 + 2_440_587.5
}

/// Parses a query. `None` when it holds nothing to match.
pub fn parse(query: &str, now: &DateTime<Local>) -> Option<Expr> {
    Parser {
        tokens: tokenize(query),
        at: 0,
        now,
    }
    .or(0)
}

/// One message as the matcher sees it: folded header text and the filterable flags.
pub struct Row {
    pub rowid: i64,
    pub id: String,
    pub from_name: String,
    pub from_email: String,
    pub recipients: String,
    pub subject: String,
    pub is_read: bool,
    pub is_starred: bool,
    pub has_attachments: bool,
    pub julian: f64,
}

/// A parsed query with its full-text lookups done, ready to test rows.
pub struct Matcher {
    expr: Expr,
    /// Rows whose body matched each body term.
    bodies: Vec<((String, bool), HashSet<i64>)>,
    /// Terms allowed to match with typos; see [`Matcher::allow_typos`].
    typos: HashSet<String>,
}
impl Matcher {
    /// `lookup` returns the rows whose body matches an FTS5 query (see [`body_match`]).
    pub fn new<E>(
        expr: Expr,
        mut lookup: impl FnMut(&str) -> Result<HashSet<i64>, E>,
    ) -> Result<Self, E> {
        let mut terms = Vec::new();
        body_terms(&expr, &mut terms);
        let bodies = terms
            .into_iter()
            .map(|(text, phrase)| {
                let rows = match body_match(&text, phrase) {
                    Some(query) => lookup(&query)?,
                    None => HashSet::new(),
                };
                Ok(((text, phrase), rows))
            })
            .collect::<Result<_, E>>()?;
        Ok(Self {
            expr,
            bodies,
            typos: HashSet::new(),
        })
    }
    /// Lets words that match none of `rows` exactly match with a typo, so a misspelled name still
    /// finds its mail while correctly spelled common words do not pull in near misses.
    pub fn allow_typos(&mut self, rows: &[Row]) {
        let mut leaves = Vec::new();
        text_leaves(&self.expr, &mut leaves);
        for (field, text) in leaves {
            if text.chars().count() >= 4
                && text.chars().all(char::is_alphanumeric)
                && !rows
                    .iter()
                    .any(|row| self.text_score(field, text, false, row, false).is_some())
            {
                self.typos.insert(text.clone());
            }
        }
    }
    /// Whether `row` matches, and how well: sender hits rank above subject, recipients, then body;
    /// typo matches rank below exact ones. `None` when it does not match.
    pub fn score(&self, row: &Row) -> Option<f64> {
        self.expr_score(&self.expr, row)
    }
    fn expr_score(&self, expr: &Expr, row: &Row) -> Option<f64> {
        match expr {
            Expr::Text {
                field,
                text,
                phrase,
            } => self.text_score(*field, text, *phrase, row, true),
            Expr::Read(read) => (row.is_read == *read).then_some(0.0),
            Expr::Starred => row.is_starred.then_some(0.0),
            Expr::Attachment => row.has_attachments.then_some(0.0),
            Expr::Date { from, to } => (from.is_none_or(|from| row.julian >= from)
                && to.is_none_or(|to| row.julian < to))
            .then_some(0.0),
            // Exclusions are exact: a typo-tolerant `-word` would hide too much.
            Expr::Not(inner) => (!self.excluded(inner, row)).then_some(0.0),
            Expr::And(items) => items
                .iter()
                .try_fold(0.0, |total, e| Some(total + self.expr_score(e, row)?)),
            Expr::Or(items) => items
                .iter()
                .filter_map(|e| self.expr_score(e, row))
                .reduce(f64::max),
        }
    }
    fn excluded(&self, expr: &Expr, row: &Row) -> bool {
        match expr {
            Expr::Text {
                field,
                text,
                phrase,
            } => self.text_score(*field, text, *phrase, row, false).is_some(),
            Expr::Not(inner) => !self.excluded(inner, row),
            Expr::And(items) => items.iter().all(|e| self.excluded(e, row)),
            Expr::Or(items) => items.iter().any(|e| self.excluded(e, row)),
            other => self.expr_score(other, row).is_some(),
        }
    }
    fn text_score(
        &self,
        field: Field,
        text: &str,
        phrase: bool,
        row: &Row,
        typos: bool,
    ) -> Option<f64> {
        let typos = typos && !phrase && self.typos.contains(text);
        let heads = [
            (row.from_name.as_str(), 8.0),
            (row.from_email.as_str(), 8.0),
            (row.subject.as_str(), 6.0),
            (row.recipients.as_str(), 3.0),
        ];
        let wanted = |index: usize| match field {
            Field::Any => true,
            Field::From => index < 2,
            Field::Subject => index == 2,
            Field::To => index == 3,
            Field::Body => false,
        };
        let mut best: Option<f64> = None;
        let mut keep = |value: f64| best = Some(best.map_or(value, |b| b.max(value)));
        for (_, (haystack, weight)) in heads.iter().enumerate().filter(|(i, _)| wanted(*i)) {
            if let Some(at) = haystack.find(text) {
                let word_start = haystack[..at]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !c.is_alphanumeric());
                keep(weight + if word_start { 1.0 } else { 0.0 });
            } else if typos && typo_match(text, haystack) {
                keep(weight / 2.0);
            }
        }
        if matches!(field, Field::Any | Field::Body)
            && self
                .bodies
                .iter()
                .find(|((t, p), _)| t == text && *p == phrase)
                .is_some_and(|(_, rows)| rows.contains(&row.rowid))
        {
            keep(2.0);
        }
        best
    }
}
/// Unquoted text terms that could match with typos, with the field they search.
fn text_leaves<'a>(expr: &'a Expr, out: &mut Vec<(Field, &'a String)>) {
    match expr {
        Expr::Text {
            field,
            text,
            phrase: false,
        } => out.push((*field, text)),
        Expr::And(items) | Expr::Or(items) => items.iter().for_each(|e| text_leaves(e, out)),
        _ => {}
    }
}
/// Body words to look up in the full-text index: `(folded text, phrase)` per text term that can
/// match a body.
fn body_terms(expr: &Expr, out: &mut Vec<(String, bool)>) {
    match expr {
        Expr::Text {
            field: Field::Any | Field::Body,
            text,
            phrase,
        } if !out.iter().any(|(t, p)| t == text && p == phrase) => {
            out.push((text.clone(), *phrase));
        }
        Expr::Not(inner) => body_terms(inner, out),
        Expr::And(items) | Expr::Or(items) => items.iter().for_each(|e| body_terms(e, out)),
        _ => {}
    }
}
/// The FTS5 query for a body term: a quoted string, so user text is never FTS syntax. Unquoted
/// words also match word beginnings, so results appear while typing.
pub fn body_match(text: &str, phrase: bool) -> Option<String> {
    text.chars().any(char::is_alphanumeric).then(|| {
        let quoted = format!("\"{}\"", text.replace('"', "\"\""));
        if phrase { quoted } else { quoted + "*" }
    })
}
/// Whether a word of `haystack`, or its beginning, is within one edit of `term` (two for terms of
/// eight or more letters). Transposed letters count as one edit.
fn typo_match(term: &str, haystack: &str) -> bool {
    let length = term.chars().count();
    let allowed = if length >= 8 { 2 } else { 1 };
    haystack.split(|c: char| !c.is_alphanumeric()).any(|word| {
        let words = word.chars().count();
        if words + allowed < length {
            return false;
        }
        let prefix: String = word.chars().take(length).collect();
        strsim::osa_distance(term, &prefix) <= allowed
            || (words <= length + allowed && strsim::osa_distance(term, word) <= allowed)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, 11, 12, 0, 0).unwrap()
    }
    fn row(from: &str, subject: &str) -> Row {
        let (name, email) = from.split_once(' ').unwrap_or((from, ""));
        Row {
            rowid: 1,
            id: "m".into(),
            from_name: fold(name),
            from_email: fold(email),
            recipients: "alex@form.studio".into(),
            subject: fold(subject),
            is_read: false,
            is_starred: false,
            has_attachments: false,
            julian: julian(&now()),
        }
    }
    fn matches(query: &str, row: &Row, body_rows: &[(&str, i64)]) -> Option<f64> {
        let expr = parse(query, &now())?;
        let mut matcher = Matcher::new(expr, |fts| {
            Ok::<_, ()>(
                body_rows
                    .iter()
                    .filter(|(t, _)| body_match(t, false).as_deref() == Some(fts))
                    .map(|(_, r)| *r)
                    .collect(),
            )
        })
        .unwrap();
        matcher.allow_typos(std::slice::from_ref(row));
        matcher.score(row)
    }
    #[test]
    fn query_language_combines_fields_filters_and_operators() {
        let mail = row("Sophie sophie@form.studio", "Studio notes — October");
        assert!(matches("from:sophie subject:october", &mail, &[]).is_some());
        assert!(
            matches("from:october", &mail, &[]).is_none(),
            "Fields narrow where a word may match"
        );
        assert!(matches("sophie -october", &mail, &[]).is_none());
        assert!(matches("sophie (invoice OR notes)", &mail, &[]).is_some());
        assert!(matches("\"studio notes\" is:unread", &mail, &[]).is_some());
        assert!(matches("is:starred", &mail, &[]).is_none());
        assert!(matches("NOT is:read date:today", &mail, &[]).is_some());
        assert!(matches("before:2026-10-11", &mail, &[]).is_none());
        assert!(matches("after:7d", &mail, &[]).is_some());
        assert!(
            matches("https://form.studio", &mail, &[]).is_none(),
            "Unknown keys are searched as text"
        );
    }
    #[test]
    fn typos_match_headers_but_not_exclusions_or_phrases() {
        let mail = row("Sophie sophie@form.studio", "Quarterly invoice");
        assert!(
            matches("sohpie", &mail, &[]).is_some(),
            "A transposition is one edit"
        );
        assert!(matches("qaurterly", &mail, &[]).is_some());
        assert!(
            matches("invo", &mail, &[]).is_some(),
            "Substrings match while typing"
        );
        assert!(
            matches("sohp", &mail, &[]).is_some(),
            "A typo in a word's beginning matches"
        );
        assert!(matches("\"sohpie\"", &mail, &[]).is_none());
        assert!(
            matches("sophie -invoise", &mail, &[]).is_some(),
            "Exclusions need the exact word"
        );
        assert!(matches("zebra", &mail, &[]).is_none());
        let exact = matches("sophie", &mail, &[]).unwrap();
        let typo = matches("sohpie", &mail, &[]).unwrap();
        let body = matches("contract", &row("Noah", "Lunch"), &[("contract", 1)]).unwrap();
        assert!(
            exact > typo && typo > body,
            "Sender hits outrank typos, which outrank body-only hits"
        );
        let (things, this) = (row("Ana", "Things to do"), row("Ben", "This week"));
        let mut matcher = Matcher::new(parse("this", &now()).unwrap(), |_| {
            Ok::<_, ()>(HashSet::new())
        })
        .unwrap();
        matcher.allow_typos(&[things, this]);
        assert!(
            matcher.score(&row("Ana", "Things to do")).is_none(),
            "A word with exact matches is not fuzzed"
        );
    }
    #[test]
    fn accents_and_case_fold_and_body_terms_are_quoted_for_fts() {
        assert_eq!(fold("HÉLLO Zoë"), "hello zoe");
        assert!(matches("ZOE", &row("Zoë", "hi"), &[]).is_some());
        assert_eq!(
            body_match("say \"hi\"", true).as_deref(),
            Some("\"say \"\"hi\"\"\"")
        );
        assert_eq!(
            body_match("invoice", false).as_deref(),
            Some("\"invoice\"*")
        );
        assert_eq!(body_match("@.", false), None);
        assert!(parse("  ( ) OR ", &now()).is_none());
        assert!(parse("from:\"sophie chen\" (unclosed", &now()).is_some());
    }
}
