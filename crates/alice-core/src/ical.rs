//! iCalendar (RFC 5545) as Alice meets it: invitations in mail (iMIP, RFC 6047) from Google,
//! Outlook, Apple and others, the iTIP `REPLY` that answers one, and calendar files that hand an
//! event to a calendar app or server. Alice stores no events; it only reads and writes these texts.
//!
//! Times come out in the [`EventDraft`] convention: timed events in UTC, all-day events as dates.
//! A `TZID` is resolved as an IANA name, then as a Windows zone name (Outlook), then from the
//! invitation's own `VTIMEZONE`; times without a zone are read in this device's zone.

use crate::model::{Attendee, EventDraft, Invite, Rsvp};
use anyhow::{Context, Result, bail};
use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, SecondsFormat,
    TimeZone, Utc, Weekday,
};
use chrono_tz::Tz;
use uuid::Uuid;

pub const PRODID: &str = "-//Alice//Alice Mail//EN";
/// Components nest a few levels deep (`VCALENDAR` › `VTIMEZONE` › `STANDARD`); deeper input is refused.
const MAX_DEPTH: usize = 8;

/// A content line: `NAME;PARAM=value:value`. Names are uppercase; the value is as written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Property {
    pub name: String,
    /// Parameter names uppercase; values unquoted, several values joined by `,`.
    pub params: Vec<(String, String)>,
    pub value: String,
}
impl Property {
    fn new(name: &str, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            params: Vec::new(),
            value: value.into(),
        }
    }
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
    /// The value as text, with `\n`, `\,`, `\;` and `\\` unescaped.
    pub fn text(&self) -> String {
        unescape(&self.value)
    }
    /// The content line, unfolded.
    fn line(&self) -> String {
        let mut line = self.name.clone();
        for (key, value) in &self.params {
            let value: String = value
                .chars()
                .filter(|c| *c != '"' && !c.is_control())
                .collect();
            line.push(';');
            line.push_str(key);
            line.push('=');
            if value.contains([':', ';', ',']) {
                line.push('"');
                line.push_str(&value);
                line.push('"');
            } else {
                line.push_str(&value);
            }
        }
        line.push(':');
        line.push_str(&self.value);
        line
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Component {
    /// Uppercase, such as `VEVENT`.
    pub name: String,
    pub properties: Vec<Property>,
    pub components: Vec<Component>,
}
impl Component {
    fn named(name: &str) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }
    pub fn property(&self, name: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.name == name)
    }
    pub fn properties<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Property> + 'a {
        self.properties.iter().filter(move |p| p.name == name)
    }
    pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Component> + 'a {
        self.components.iter().filter(move |c| c.name == name)
    }
    fn value(&self, name: &str) -> Option<&str> {
        self.property(name)
            .map(|p| p.value.trim())
            .filter(|v| !v.is_empty())
    }
    fn text(&self, name: &str) -> Option<String> {
        self.property(name)
            .map(Property::text)
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
    }
    fn write(&self, out: &mut String) {
        fold(&format!("BEGIN:{}", self.name), out);
        for property in &self.properties {
            fold(&property.line(), out);
        }
        for component in &self.components {
            component.write(out);
        }
        fold(&format!("END:{}", self.name), out);
    }
}

/// Parses an iCalendar text and returns its `VCALENDAR`.
pub fn parse(text: &str) -> Result<Component> {
    let mut stack = vec![Component::default()];
    for line in unfold(text.trim_start_matches('\u{feff}')) {
        let Some(property) = parse_line(&line) else {
            continue;
        };
        match property.name.as_str() {
            "BEGIN" => {
                if stack.len() > MAX_DEPTH {
                    bail!(crate::text!("mail.ical.depth"));
                }
                stack.push(Component::named(
                    &property.value.trim().to_ascii_uppercase(),
                ));
            }
            "END" if stack.len() > 1 => {
                let done = stack.pop().expect("stack has a component");
                stack
                    .last_mut()
                    .expect("stack has a root")
                    .components
                    .push(done);
            }
            "END" => {}
            _ => stack
                .last_mut()
                .expect("stack has a root")
                .properties
                .push(property),
        }
    }
    // A truncated file still yields what it has.
    while stack.len() > 1 {
        let done = stack.pop().expect("stack has a component");
        stack
            .last_mut()
            .expect("stack has a root")
            .components
            .push(done);
    }
    stack
        .pop()
        .and_then(|root| root.components.into_iter().find(|c| c.name == "VCALENDAR"))
        .context(crate::text!("mail.ical.invalid_file"))
}

/// Joins folded lines: a line starting with a space or tab continues the previous one.
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        match (raw.strip_prefix([' ', '\t']), lines.last_mut()) {
            (Some(rest), Some(last)) => last.push_str(rest),
            _ if raw.trim().is_empty() => {}
            _ => lines.push(raw.to_string()),
        }
    }
    lines
}

fn parse_line(line: &str) -> Option<Property> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = line.find([';', ':'])?;
    let name = line[..i].trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }
    let mut params = Vec::new();
    while i < len && bytes[i] == b';' {
        i += 1;
        let start = i;
        while i < len && !matches!(bytes[i], b'=' | b':' | b';') {
            i += 1;
        }
        if i == len {
            return None;
        }
        let key = line[start..i].trim().to_ascii_uppercase();
        if bytes[i] != b'=' {
            params.push((key, String::new()));
            continue;
        }
        i += 1;
        let mut value = String::new();
        loop {
            if i < len && bytes[i] == b'"' {
                let close = i + 1 + line[i + 1..].find('"')?;
                value.push_str(&line[i + 1..close]);
                i = close + 1;
            } else {
                let start = i;
                while i < len && !matches!(bytes[i], b';' | b':' | b',') {
                    i += 1;
                }
                value.push_str(&line[start..i]);
            }
            if i < len && bytes[i] == b',' {
                value.push(',');
                i += 1;
                continue;
            }
            break;
        }
        params.push((key, decode_caret(&value)));
    }
    if bytes.get(i) != Some(&b':') {
        return None;
    }
    Some(Property {
        name,
        params,
        value: line[i + 1..].to_string(),
    })
}

/// RFC 6868 parameter value escapes: `^n` newline, `^'` quote, `^^` caret.
fn decode_caret(value: &str) -> String {
    if !value.contains('^') {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '^' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('\'') => out.push('"'),
            Some('^') => out.push('^'),
            Some(other) => {
                out.push('^');
                out.push(other);
            }
            None => out.push('^'),
        }
    }
    out
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Escapes text for a TEXT value.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.replace("\r\n", "\n").chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' | '\r' => out.push_str("\\n"),
            c if c.is_control() && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// Writes a content line folded to at most 75 octets per line (continuations start with a space),
/// never splitting a UTF-8 character, and ends it with CRLF.
fn fold(line: &str, out: &mut String) {
    let mut width = 0;
    for c in line.chars() {
        let size = c.len_utf8();
        if width + size > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += size;
    }
    out.push_str("\r\n");
}

/// The address in a `CAL-ADDRESS` value such as `mailto:Ana@Example.com` (scheme case-insensitive).
fn address(value: &str) -> String {
    let value = value.trim();
    match value.get(..7) {
        Some(scheme) if scheme.eq_ignore_ascii_case("mailto:") => value[7..].trim().to_string(),
        _ => value.to_string(),
    }
}

/// The event an invitation is about: the series or single event (no `RECURRENCE-ID`), else the
/// first one, as when a single occurrence of a series is rescheduled.
fn master(calendar: &Component) -> Option<&Component> {
    calendar
        .children("VEVENT")
        .find(|e| e.property("RECURRENCE-ID").is_none())
        .or_else(|| calendar.children("VEVENT").next())
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum When {
    Date(NaiveDate),
    Time(DateTime<Utc>),
}
impl When {
    fn format(self) -> String {
        match self {
            When::Date(date) => date.format("%Y-%m-%d").to_string(),
            When::Time(time) => time.to_rfc3339_opts(SecondsFormat::Secs, true),
        }
    }
    fn date(self) -> NaiveDate {
        match self {
            When::Date(date) => date,
            When::Time(time) => time.with_timezone(&Local).date_naive(),
        }
    }
    /// The instant: all-day dates start at midnight in this device's zone.
    fn instant(self) -> DateTime<Utc> {
        match self {
            When::Date(date) => {
                localize(&Local, date.and_hms_opt(0, 0, 0).expect("midnight exists"))
            }
            When::Time(time) => time,
        }
    }
}

fn when(property: &Property, calendar: &Component) -> Result<When> {
    let value = property.value.trim();
    let is_date = property
        .param("VALUE")
        .is_some_and(|v| v.eq_ignore_ascii_case("DATE"))
        || value.len() == 8;
    if is_date {
        let date = value
            .get(..8)
            .and_then(|v| NaiveDate::parse_from_str(v, "%Y%m%d").ok());
        return date
            .map(When::Date)
            .with_context(|| crate::text!("mail.ical.invalid_date", value = value));
    }
    let (local, utc) = match value.strip_suffix(['Z', 'z']) {
        Some(local) => (local, true),
        None => (value, false),
    };
    let naive = NaiveDateTime::parse_from_str(local, "%Y%m%dT%H%M%S")
        .with_context(|| crate::text!("mail.ical.invalid_time", value = value))?;
    if utc {
        return Ok(When::Time(naive.and_utc()));
    }
    Ok(When::Time(match property.param("TZID") {
        Some(tzid) => in_zone(naive, tzid, calendar),
        None => localize(&Local, naive),
    }))
}

/// The UTC instant of a wall-clock time in zone `tzid`.
fn in_zone(naive: NaiveDateTime, tzid: &str, calendar: &Component) -> DateTime<Utc> {
    if let Some(zone) = named_zone(tzid) {
        return localize(&zone, naive);
    }
    let defined = calendar
        .children("VTIMEZONE")
        .find(|z| z.value("TZID") == Some(tzid.trim()))
        .and_then(|z| defined_offset(z, naive));
    match defined {
        Some(offset) => (naive - Duration::seconds(offset.into())).and_utc(),
        None => localize(&Local, naive),
    }
}

/// An IANA zone for a `TZID`: an IANA name, a Windows zone name, or an IANA name at the end of a
/// prefixed ID such as `/mozilla.org/20050126_1/America/New_York`.
pub fn named_zone(tzid: &str) -> Option<Tz> {
    let tzid = tzid.trim().trim_matches('"');
    if let Ok(zone) = tzid.parse::<Tz>() {
        return Some(zone);
    }
    if let Some((_, iana)) = WINDOWS_ZONES
        .iter()
        .find(|(windows, _)| windows.eq_ignore_ascii_case(tzid))
    {
        return iana.parse().ok();
    }
    tzid.match_indices('/')
        .find_map(|(i, _)| tzid[i + 1..].parse::<Tz>().ok())
}

/// Reads a wall-clock time in `zone`. A time skipped by a forward change is read with the offset
/// before the change, and a repeated time as its first occurrence (RFC 5545 §3.3.5).
fn localize<Z: TimeZone>(zone: &Z, naive: NaiveDateTime) -> DateTime<Utc> {
    match zone.from_local_datetime(&naive) {
        LocalResult::Single(time) | LocalResult::Ambiguous(time, _) => time.with_timezone(&Utc),
        LocalResult::None => zone
            .from_local_datetime(&(naive + Duration::hours(1)))
            .earliest()
            .map(|time| time.with_timezone(&Utc))
            .unwrap_or_else(|| naive.and_utc()),
    }
}

/// One `STANDARD` or `DAYLIGHT` block of a `VTIMEZONE`.
struct Observance {
    start: NaiveDateTime,
    from: i32,
    to: i32,
    rule: Option<YearlyRule>,
    dates: Vec<NaiveDateTime>,
}
/// The yearly `RRULE` of an observance: `BYMONTH` with `BYDAY` (`2SU`, `-1SU`) or `BYMONTHDAY`.
struct YearlyRule {
    month: u32,
    day: RuleDay,
    until: Option<NaiveDateTime>,
}
enum RuleDay {
    Weekday(i32, Weekday),
    MonthDay(i32),
}

/// The UTC offset in seconds that a `VTIMEZONE` gives a wall-clock time: that of the observance
/// that started most recently, or before any observance, the first one's offset before it.
fn defined_offset(zone: &Component, local: NaiveDateTime) -> Option<i32> {
    let observances: Vec<Observance> = zone
        .components
        .iter()
        .filter(|c| c.name == "STANDARD" || c.name == "DAYLIGHT")
        .filter_map(observance)
        .collect();
    observances
        .iter()
        .filter_map(|o| o.latest_onset(local).map(|onset| (onset, o.to)))
        .max_by_key(|(onset, _)| *onset)
        .map(|(_, offset)| offset)
        .or_else(|| observances.iter().min_by_key(|o| o.start).map(|o| o.from))
}

fn observance(component: &Component) -> Option<Observance> {
    let start = NaiveDateTime::parse_from_str(
        component.value("DTSTART")?.trim_end_matches(['Z', 'z']),
        "%Y%m%dT%H%M%S",
    )
    .ok()?;
    let to = utc_offset(component.value("TZOFFSETTO")?)?;
    let from = component
        .value("TZOFFSETFROM")
        .and_then(utc_offset)
        .unwrap_or(to);
    let rule = component
        .value("RRULE")
        .and_then(|rule| yearly_rule(rule, start));
    let dates = component
        .properties("RDATE")
        .flat_map(|p| p.value.split(','))
        .filter_map(|v| {
            NaiveDateTime::parse_from_str(v.trim().trim_end_matches(['Z', 'z']), "%Y%m%dT%H%M%S")
                .ok()
        })
        .collect();
    Some(Observance {
        start,
        from,
        to,
        rule,
        dates,
    })
}

impl Observance {
    fn latest_onset(&self, local: NaiveDateTime) -> Option<NaiveDateTime> {
        if local < self.start {
            return None;
        }
        let mut latest = self.start;
        for date in &self.dates {
            if *date <= local && *date > latest {
                latest = *date;
            }
        }
        if let Some(rule) = &self.rule {
            for year in [local.year() - 1, local.year()] {
                let Some(onset) = rule.onset(year, self.start) else {
                    continue;
                };
                if onset >= self.start
                    && onset <= local
                    && rule.until.is_none_or(|until| onset <= until)
                    && onset > latest
                {
                    latest = onset;
                }
            }
        }
        Some(latest)
    }
}

impl YearlyRule {
    fn onset(&self, year: i32, start: NaiveDateTime) -> Option<NaiveDateTime> {
        let date = match self.day {
            RuleDay::Weekday(n, weekday) => nth_weekday(year, self.month, n, weekday)?,
            RuleDay::MonthDay(day) if day > 0 => {
                NaiveDate::from_ymd_opt(year, self.month, day as u32)?
            }
            RuleDay::MonthDay(day) => {
                last_of_month(year, self.month)? + Duration::days((day + 1).into())
            }
        };
        Some(date.and_time(start.time()))
    }
}

fn yearly_rule(rule: &str, start: NaiveDateTime) -> Option<YearlyRule> {
    let parts: Vec<(String, &str)> = rule
        .split(';')
        .filter_map(|part| part.split_once('='))
        .map(|(key, value)| (key.trim().to_ascii_uppercase(), value.trim()))
        .collect();
    let get = |key: &str| parts.iter().find(|(k, _)| k == key).map(|(_, v)| *v);
    if !get("FREQ")?.eq_ignore_ascii_case("YEARLY") {
        return None;
    }
    let month = get("BYMONTH")
        .and_then(|m| m.split(',').next()?.parse().ok())
        .unwrap_or(start.month());
    let day = if let Some(byday) = get("BYDAY") {
        let byday = byday.split(',').next()?.trim();
        let (count, weekday) = byday.split_at(
            byday
                .len()
                .checked_sub(2)
                .filter(|i| byday.is_char_boundary(*i))?,
        );
        let weekday = match weekday.to_ascii_uppercase().as_str() {
            "MO" => Weekday::Mon,
            "TU" => Weekday::Tue,
            "WE" => Weekday::Wed,
            "TH" => Weekday::Thu,
            "FR" => Weekday::Fri,
            "SA" => Weekday::Sat,
            "SU" => Weekday::Sun,
            _ => return None,
        };
        RuleDay::Weekday(
            if count.is_empty() {
                1
            } else {
                count.trim_start_matches('+').parse().ok()?
            },
            weekday,
        )
    } else {
        RuleDay::MonthDay(
            get("BYMONTHDAY")
                .and_then(|d| d.parse().ok())
                .unwrap_or(start.day() as i32),
        )
    };
    let until = get("UNTIL").and_then(|u| {
        let u = u.trim_end_matches(['Z', 'z']);
        NaiveDateTime::parse_from_str(u, "%Y%m%dT%H%M%S")
            .ok()
            .or_else(|| {
                NaiveDate::parse_from_str(u, "%Y%m%d")
                    .ok()
                    .and_then(|d| d.and_hms_opt(23, 59, 59))
            })
    });
    Some(YearlyRule { month, day, until })
}

fn last_of_month(year: i32, month: u32) -> Option<NaiveDate> {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, 1)?.pred_opt()
}

/// The `n`th `weekday` of a month; negative `n` counts from the end (`-1` is the last).
fn nth_weekday(year: i32, month: u32, n: i32, weekday: Weekday) -> Option<NaiveDate> {
    let date = if n > 0 {
        let first = NaiveDate::from_ymd_opt(year, month, 1)?;
        let shift =
            (7 + weekday.num_days_from_monday() - first.weekday().num_days_from_monday()) % 7;
        first + Duration::days(i64::from(shift) + 7 * i64::from(n - 1))
    } else if n < 0 {
        let last = last_of_month(year, month)?;
        let shift =
            (7 + last.weekday().num_days_from_monday() - weekday.num_days_from_monday()) % 7;
        last - Duration::days(i64::from(shift) + 7 * i64::from(-n - 1))
    } else {
        return None;
    };
    (date.month() == month).then_some(date)
}

/// `+0100`, `-0800` or `+053000` in seconds.
fn utc_offset(value: &str) -> Option<i32> {
    let value = value.trim();
    let sign = match value.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits = &value[1..];
    if !(digits.len() == 4 || digits.len() == 6) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i32 = digits[..2].parse().ok()?;
    let minutes: i32 = digits[2..4].parse().ok()?;
    let seconds: i32 = digits.get(4..6).map_or(Some(0), |s| s.parse().ok())?;
    Some(sign * (hours * 3600 + minutes * 60 + seconds))
}

/// An RFC 5545 `DURATION` such as `PT1H30M`, `P1D` or `P2W`.
fn duration(value: &str) -> Option<Duration> {
    let value = value.trim();
    let (sign, rest) = match value.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, value.strip_prefix('+').unwrap_or(value)),
    };
    let rest = rest.strip_prefix(['P', 'p'])?;
    let mut total = 0i64;
    let mut number = String::new();
    let mut in_time = false;
    for c in rest.chars() {
        match c.to_ascii_uppercase() {
            '0'..='9' => number.push(c),
            'T' => in_time = true,
            unit @ ('W' | 'D' | 'H' | 'M' | 'S') => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                total += n * match (unit, in_time) {
                    ('W', false) => 604_800,
                    ('D', false) => 86_400,
                    ('H', true) => 3_600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
            }
            _ => return None,
        }
    }
    number.is_empty().then(|| Duration::seconds(sign * total))
}

/// An event's start and end. Without `DTEND`, the end is `DTSTART` plus `DURATION`, else one day
/// for an all-day event and the start itself for a timed one (RFC 5545 §3.6.1).
fn span(event: &Component, calendar: &Component) -> Result<(When, When)> {
    let start = when(
        event
            .property("DTSTART")
            .context(crate::text!("mail.ical.no_start"))?,
        calendar,
    )?;
    let end = match (
        event.property("DTEND"),
        event.value("DURATION").and_then(duration),
    ) {
        (Some(end), _) => when(end, calendar)?,
        (None, Some(length)) => match start {
            When::Date(date) => When::Date(date + Duration::days(length.num_days().max(1))),
            When::Time(time) => When::Time(time + length),
        },
        (None, None) => match start {
            When::Date(date) => When::Date(date + Duration::days(1)),
            time @ When::Time(_) => time,
        },
    };
    // An all-day start keeps the end a date; an end before the start is ignored.
    let end = match (start, end) {
        (When::Date(start), end) => When::Date(end.date().max(start + Duration::days(1))),
        (When::Time(start), end) => When::Time(end.instant().max(start)),
    };
    Ok((start, end))
}

fn partstat(value: &str) -> Option<Rsvp> {
    match value.trim().to_ascii_uppercase().as_str() {
        "ACCEPTED" => Some(Rsvp::Accepted),
        "TENTATIVE" => Some(Rsvp::Tentative),
        "DECLINED" => Some(Rsvp::Declined),
        _ => None,
    }
}
fn partstat_value(response: Rsvp) -> &'static str {
    match response {
        Rsvp::Accepted => "ACCEPTED",
        Rsvp::Tentative => "TENTATIVE",
        Rsvp::Declined => "DECLINED",
    }
}

fn attendee(property: &Property) -> Attendee {
    Attendee {
        email: address(&property.value),
        name: property
            .param("CN")
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(String::from),
        status: property.param("PARTSTAT").and_then(partstat),
    }
}

/// The invitation in `ics`, with `response` read from the `PARTSTAT` of the attendee whose address
/// is `email` (the account it arrived in).
pub fn invite(ics: &str, email: &str) -> Result<Invite> {
    let calendar = parse(ics)?;
    let event = master(&calendar).context(crate::text!("mail.ical.no_event"))?;
    let method = calendar
        .value("METHOD")
        .map(str::to_ascii_uppercase)
        .unwrap_or_else(|| "PUBLISH".into());
    let uid = event
        .value("UID")
        .context(crate::text!("mail.ical.no_uid"))?
        .to_string();
    let (start, end) = span(event, &calendar)?;
    let attendees: Vec<Attendee> = event.properties("ATTENDEE").map(attendee).collect();
    let response = attendees
        .iter()
        .find(|a| a.email.eq_ignore_ascii_case(email.trim()))
        .and_then(|a| a.status);
    Ok(Invite {
        cancelled: method == "CANCEL"
            || event
                .value("STATUS")
                .is_some_and(|s| s.eq_ignore_ascii_case("CANCELLED")),
        method,
        uid,
        sequence: event
            .value("SEQUENCE")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        event: EventDraft {
            title: event.text("SUMMARY").unwrap_or_else(|| "(No title)".into()),
            all_day: matches!(start, When::Date(_)),
            start: start.format(),
            end: Some(end.format()),
            location: event.text("LOCATION"),
            description: event.text("DESCRIPTION"),
        },
        organizer: event.property("ORGANIZER").map(attendee),
        attendees,
        recurrence: event.value("RRULE").map(String::from),
        response,
        ics: ics.to_string(),
    })
}

fn header(method: Option<&str>) -> String {
    let mut out = String::new();
    for line in [
        "BEGIN:VCALENDAR",
        &format!("PRODID:{PRODID}"),
        "VERSION:2.0",
        "CALSCALE:GREGORIAN",
    ] {
        fold(line, &mut out);
    }
    if let Some(method) = method {
        fold(&format!("METHOD:{method}"), &mut out);
    }
    out
}

fn stamp() -> Property {
    Property::new("DTSTAMP", Utc::now().format("%Y%m%dT%H%M%SZ").to_string())
}

/// The iTIP `REPLY` (RFC 5546 §3.2.3) that answers `invite` as attendee `email`: the same event
/// (UID, SEQUENCE, RECURRENCE-ID, times), its organizer, and only this attendee with `response`.
pub fn reply(
    invite: &Invite,
    email: &str,
    name: &str,
    response: Rsvp,
    comment: Option<&str>,
) -> Result<String> {
    let calendar = parse(&invite.ics)?;
    let event = master(&calendar).context(crate::text!("mail.ical.no_event"))?;
    let mut answer = Component::named("VEVENT");
    for key in [
        "UID",
        "SEQUENCE",
        "RECURRENCE-ID",
        "DTSTART",
        "DTEND",
        "DURATION",
        "ORGANIZER",
        "SUMMARY",
    ] {
        answer.properties.extend(event.property(key).cloned());
    }
    answer.properties.push(stamp());
    let mut me = event
        .properties("ATTENDEE")
        .find(|p| address(&p.value).eq_ignore_ascii_case(email.trim()))
        .cloned()
        .unwrap_or_else(|| {
            let mut property = Property::new("ATTENDEE", format!("mailto:{}", email.trim()));
            if !name.trim().is_empty() {
                property.params.push(("CN".into(), name.trim().into()));
            }
            property
        });
    me.params
        .retain(|(key, _)| key != "PARTSTAT" && key != "RSVP");
    me.params
        .push(("PARTSTAT".into(), partstat_value(response).into()));
    answer.properties.push(me);
    if let Some(comment) = comment.map(str::trim).filter(|c| !c.is_empty()) {
        answer
            .properties
            .push(Property::new("COMMENT", escape(comment)));
    }
    let mut out = header(Some("REPLY"));
    for zone in calendar.children("VTIMEZONE") {
        zone.write(&mut out);
    }
    answer.write(&mut out);
    fold("END:VCALENDAR", &mut out);
    Ok(out)
}

/// A calendar file that imports `invite` as an event, rather than presenting it as an invitation:
/// no `METHOD` (as CalDAV stores require), its time zones, and its events with the same UID.
pub fn import_file(invite: &Invite) -> Result<String> {
    let calendar = parse(&invite.ics)?;
    let mut out = header(None);
    for zone in calendar.children("VTIMEZONE") {
        zone.write(&mut out);
    }
    let mut events = 0;
    for event in calendar
        .children("VEVENT")
        .filter(|e| e.value("UID") == Some(invite.uid.as_str()))
    {
        event.write(&mut out);
        events += 1;
    }
    if events == 0 {
        bail!(crate::text!("mail.ical.no_event"));
    }
    fold("END:VCALENDAR", &mut out);
    Ok(out)
}

/// A calendar file for a new event, and the new event's UID. `draft` must be [`normalize`]d.
pub fn draft_file(draft: &EventDraft) -> Result<(String, String)> {
    let uid = format!("{}@alice.local", Uuid::new_v4());
    let mut event = Component::named("VEVENT");
    event.properties.push(Property::new("UID", uid.clone()));
    event.properties.push(stamp());
    let end = draft
        .end
        .as_deref()
        .context(crate::text!("mail.ical.no_end"))?;
    for (name, value) in [("DTSTART", draft.start.as_str()), ("DTEND", end)] {
        let property = if draft.all_day {
            let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .context(crate::text!("mail.ical.date_format"))?;
            let mut property = Property::new(name, date.format("%Y%m%d").to_string());
            property.params.push(("VALUE".into(), "DATE".into()));
            property
        } else {
            let time = DateTime::parse_from_rfc3339(value)
                .context(crate::text!("mail.ical.time_format"))?;
            Property::new(
                name,
                time.with_timezone(&Utc)
                    .format("%Y%m%dT%H%M%SZ")
                    .to_string(),
            )
        };
        event.properties.push(property);
    }
    event
        .properties
        .push(Property::new("SUMMARY", escape(&draft.title)));
    if let Some(location) = &draft.location {
        event
            .properties
            .push(Property::new("LOCATION", escape(location)));
    }
    if let Some(description) = &draft.description {
        event
            .properties
            .push(Property::new("DESCRIPTION", escape(description)));
    }
    let mut out = header(None);
    event.write(&mut out);
    fold("END:VCALENDAR", &mut out);
    Ok((uid, out))
}

/// A safe file name for an event titled `title`, ending in `.ics`.
pub fn filename(title: &str) -> String {
    let mut name = String::new();
    for c in title.chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            name.push(c);
        } else if !name.is_empty() && !name.ends_with(' ') {
            name.push(' ');
        }
        if name.chars().count() >= 60 {
            break;
        }
    }
    let name = name.trim();
    format!("{}.ics", if name.is_empty() { "Event" } else { name })
}

/// The time zone `invite` starts in, when it has an IANA name (Google needs one to repeat an event).
pub fn start_zone(invite: &Invite) -> Option<String> {
    let calendar = parse(&invite.ics).ok()?;
    let tzid = master(&calendar)?
        .property("DTSTART")?
        .param("TZID")?
        .to_string();
    named_zone(&tzid).map(|zone| zone.name().to_string())
}

/// The series' `RRULE`, `RDATE` and `EXDATE` lines, dates in UTC so that any service can read them.
pub fn recurrence_lines(invite: &Invite) -> Vec<String> {
    let Ok(calendar) = parse(&invite.ics) else {
        return Vec::new();
    };
    let Some(event) = master(&calendar) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for property in &event.properties {
        match property.name.as_str() {
            "RRULE" => lines.push(format!("RRULE:{}", property.value.trim())),
            "RDATE" | "EXDATE" => {
                let values: Vec<When> = property
                    .value
                    .split(',')
                    .filter_map(|v| {
                        when(
                            &Property {
                                value: v.trim().into(),
                                ..property.clone()
                            },
                            &calendar,
                        )
                        .ok()
                    })
                    .collect();
                if values.iter().all(|v| matches!(v, When::Date(_))) && !values.is_empty() {
                    let dates: Vec<String> = values
                        .iter()
                        .map(|v| v.date().format("%Y%m%d").to_string())
                        .collect();
                    lines.push(format!("{};VALUE=DATE:{}", property.name, dates.join(",")));
                } else if !values.is_empty() {
                    let times: Vec<String> = values
                        .iter()
                        .map(|v| v.instant().format("%Y%m%dT%H%M%SZ").to_string())
                        .collect();
                    lines.push(format!("{}:{}", property.name, times.join(",")));
                }
            }
            _ => {}
        }
    }
    lines
}

/// The busy intervals in a calendar server's answer: its events that are neither cancelled nor
/// marked free. Expects recurrences already expanded (CalDAV `expand`); all-day events cover this
/// device's local days.
pub fn busy_times(ics: &str) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    let Ok(calendar) = parse(ics) else {
        return Vec::new();
    };
    calendar
        .children("VEVENT")
        .filter(|e| {
            !e.value("STATUS")
                .is_some_and(|s| s.eq_ignore_ascii_case("CANCELLED"))
        })
        .filter(|e| {
            !e.value("TRANSP")
                .is_some_and(|t| t.eq_ignore_ascii_case("TRANSPARENT"))
        })
        .filter_map(|e| span(e, &calendar).ok())
        .map(|(start, end)| (start.instant(), end.instant()))
        .filter(|(start, end)| end > start)
        .collect()
}

/// Checks an event proposed by a person or by Ask Alice and puts it in the [`EventDraft`]
/// convention: timed events in UTC with an end (one hour when none is given), all-day events as
/// dates with an exclusive end.
pub fn normalize(draft: EventDraft) -> Result<EventDraft> {
    let title = draft.title.trim().to_string();
    if title.is_empty() || title.chars().count() > 500 {
        bail!(crate::text!("mail.ical.title_limit"));
    }
    let optional =
        |value: Option<String>, limit: usize, error: crate::i18n::Text| -> Result<Option<String>> {
            let value = value
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
            if value.as_ref().is_some_and(|v| v.chars().count() > limit) {
                bail!(error);
            }
            Ok(value)
        };
    let location = optional(
        draft.location,
        1_000,
        crate::text!("mail.ical.location_limit"),
    )?;
    let description = optional(
        draft.description,
        20_000,
        crate::text!("mail.ical.description_limit"),
    )?;
    let (start, end) = if draft.all_day {
        let date = |value: &str| {
            NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
                .context(crate::text!("mail.ical.all_day_format"))
        };
        let start = date(&draft.start)?;
        let end = draft.end.as_deref().map(date).transpose()?.unwrap_or(start);
        if end < start {
            bail!(crate::text!("mail.ical.end_before_start"));
        }
        // The end is exclusive; a one-day event may be given ending on its own day.
        let end = end.max(start + Duration::days(1));
        (
            start.format("%Y-%m-%d").to_string(),
            end.format("%Y-%m-%d").to_string(),
        )
    } else {
        let time = |value: &str| {
            DateTime::parse_from_rfc3339(value.trim())
                .map(|t| t.with_timezone(&Utc))
                .context(crate::text!("mail.ical.event_time_format"))
        };
        let start = time(&draft.start)?;
        let end = draft
            .end
            .as_deref()
            .map(time)
            .transpose()?
            .unwrap_or(start + Duration::hours(1));
        if end < start {
            bail!(crate::text!("mail.ical.end_before_start"));
        }
        (When::Time(start).format(), When::Time(end).format())
    };
    Ok(EventDraft {
        title,
        start,
        end: Some(end),
        all_day: draft.all_day,
        location,
        description,
    })
}

/// Windows time zone names, as Outlook and Exchange write them in `TZID`, and the IANA zone each
/// stands for (CLDR `windowsZones.xml`, territory 001), plus a few legacy names still in use.
const WINDOWS_ZONES: &[(&str, &str)] = &[
    ("Dateline Standard Time", "Etc/GMT+12"),
    ("UTC-11", "Etc/GMT+11"),
    ("Aleutian Standard Time", "America/Adak"),
    ("Hawaiian Standard Time", "Pacific/Honolulu"),
    ("Marquesas Standard Time", "Pacific/Marquesas"),
    ("Alaskan Standard Time", "America/Anchorage"),
    ("UTC-09", "Etc/GMT+9"),
    ("Pacific Standard Time (Mexico)", "America/Tijuana"),
    ("UTC-08", "Etc/GMT+8"),
    ("Pacific Standard Time", "America/Los_Angeles"),
    ("US Mountain Standard Time", "America/Phoenix"),
    ("Mountain Standard Time (Mexico)", "America/Mazatlan"),
    ("Mountain Standard Time", "America/Denver"),
    ("Yukon Standard Time", "America/Whitehorse"),
    ("Central America Standard Time", "America/Guatemala"),
    ("Central Standard Time", "America/Chicago"),
    ("Easter Island Standard Time", "Pacific/Easter"),
    ("Central Standard Time (Mexico)", "America/Mexico_City"),
    ("Mexico Standard Time", "America/Mexico_City"),
    ("Mexico Standard Time 2", "America/Chihuahua"),
    ("Canada Central Standard Time", "America/Regina"),
    ("SA Pacific Standard Time", "America/Bogota"),
    ("Eastern Standard Time (Mexico)", "America/Cancun"),
    ("Eastern Standard Time", "America/New_York"),
    ("Haiti Standard Time", "America/Port-au-Prince"),
    ("Cuba Standard Time", "America/Havana"),
    ("US Eastern Standard Time", "America/Indiana/Indianapolis"),
    ("Turks And Caicos Standard Time", "America/Grand_Turk"),
    ("Paraguay Standard Time", "America/Asuncion"),
    ("Atlantic Standard Time", "America/Halifax"),
    ("Venezuela Standard Time", "America/Caracas"),
    ("Central Brazilian Standard Time", "America/Cuiaba"),
    ("SA Western Standard Time", "America/La_Paz"),
    ("Pacific SA Standard Time", "America/Santiago"),
    ("Newfoundland Standard Time", "America/St_Johns"),
    ("Tocantins Standard Time", "America/Araguaina"),
    ("E. South America Standard Time", "America/Sao_Paulo"),
    ("SA Eastern Standard Time", "America/Cayenne"),
    ("Argentina Standard Time", "America/Argentina/Buenos_Aires"),
    ("Greenland Standard Time", "America/Nuuk"),
    ("Montevideo Standard Time", "America/Montevideo"),
    ("Magallanes Standard Time", "America/Punta_Arenas"),
    ("Saint Pierre Standard Time", "America/Miquelon"),
    ("Bahia Standard Time", "America/Bahia"),
    ("UTC-02", "Etc/GMT+2"),
    ("Mid-Atlantic Standard Time", "Etc/GMT+2"),
    ("Azores Standard Time", "Atlantic/Azores"),
    ("Cape Verde Standard Time", "Atlantic/Cape_Verde"),
    ("UTC", "Etc/UTC"),
    ("tzone://Microsoft/Utc", "Etc/UTC"),
    ("GMT Standard Time", "Europe/London"),
    ("Greenwich Standard Time", "Atlantic/Reykjavik"),
    ("Sao Tome Standard Time", "Africa/Sao_Tome"),
    ("Morocco Standard Time", "Africa/Casablanca"),
    ("W. Europe Standard Time", "Europe/Berlin"),
    ("Central Europe Standard Time", "Europe/Budapest"),
    ("Romance Standard Time", "Europe/Paris"),
    ("Central European Standard Time", "Europe/Warsaw"),
    ("W. Central Africa Standard Time", "Africa/Lagos"),
    ("Jordan Standard Time", "Asia/Amman"),
    ("GTB Standard Time", "Europe/Bucharest"),
    ("Middle East Standard Time", "Asia/Beirut"),
    ("Egypt Standard Time", "Africa/Cairo"),
    ("E. Europe Standard Time", "Europe/Chisinau"),
    ("Syria Standard Time", "Asia/Damascus"),
    ("West Bank Standard Time", "Asia/Hebron"),
    ("South Africa Standard Time", "Africa/Johannesburg"),
    ("FLE Standard Time", "Europe/Kyiv"),
    ("Israel Standard Time", "Asia/Jerusalem"),
    ("South Sudan Standard Time", "Africa/Juba"),
    ("Kaliningrad Standard Time", "Europe/Kaliningrad"),
    ("Sudan Standard Time", "Africa/Khartoum"),
    ("Libya Standard Time", "Africa/Tripoli"),
    ("Namibia Standard Time", "Africa/Windhoek"),
    ("Arabic Standard Time", "Asia/Baghdad"),
    ("Turkey Standard Time", "Europe/Istanbul"),
    ("Arab Standard Time", "Asia/Riyadh"),
    ("Belarus Standard Time", "Europe/Minsk"),
    ("Russian Standard Time", "Europe/Moscow"),
    ("E. Africa Standard Time", "Africa/Nairobi"),
    ("Volgograd Standard Time", "Europe/Volgograd"),
    ("Iran Standard Time", "Asia/Tehran"),
    ("Arabian Standard Time", "Asia/Dubai"),
    ("Astrakhan Standard Time", "Europe/Astrakhan"),
    ("Azerbaijan Standard Time", "Asia/Baku"),
    ("Russia Time Zone 3", "Europe/Samara"),
    ("Mauritius Standard Time", "Indian/Mauritius"),
    ("Saratov Standard Time", "Europe/Saratov"),
    ("Georgian Standard Time", "Asia/Tbilisi"),
    ("Caucasus Standard Time", "Asia/Yerevan"),
    ("Armenian Standard Time", "Asia/Yerevan"),
    ("Afghanistan Standard Time", "Asia/Kabul"),
    ("West Asia Standard Time", "Asia/Tashkent"),
    ("Ekaterinburg Standard Time", "Asia/Yekaterinburg"),
    ("Pakistan Standard Time", "Asia/Karachi"),
    ("Qyzylorda Standard Time", "Asia/Qyzylorda"),
    ("India Standard Time", "Asia/Kolkata"),
    ("Sri Lanka Standard Time", "Asia/Colombo"),
    ("Nepal Standard Time", "Asia/Kathmandu"),
    ("Central Asia Standard Time", "Asia/Bishkek"),
    ("Bangladesh Standard Time", "Asia/Dhaka"),
    ("Omsk Standard Time", "Asia/Omsk"),
    ("Myanmar Standard Time", "Asia/Yangon"),
    ("SE Asia Standard Time", "Asia/Bangkok"),
    ("Altai Standard Time", "Asia/Barnaul"),
    ("W. Mongolia Standard Time", "Asia/Hovd"),
    ("North Asia Standard Time", "Asia/Krasnoyarsk"),
    ("N. Central Asia Standard Time", "Asia/Novosibirsk"),
    ("Tomsk Standard Time", "Asia/Tomsk"),
    ("China Standard Time", "Asia/Shanghai"),
    ("North Asia East Standard Time", "Asia/Irkutsk"),
    ("Singapore Standard Time", "Asia/Singapore"),
    ("Malay Peninsula Standard Time", "Asia/Kuala_Lumpur"),
    ("W. Australia Standard Time", "Australia/Perth"),
    ("Taipei Standard Time", "Asia/Taipei"),
    ("Ulaanbaatar Standard Time", "Asia/Ulaanbaatar"),
    ("Aus Central W. Standard Time", "Australia/Eucla"),
    ("Transbaikal Standard Time", "Asia/Chita"),
    ("Tokyo Standard Time", "Asia/Tokyo"),
    ("North Korea Standard Time", "Asia/Pyongyang"),
    ("Korea Standard Time", "Asia/Seoul"),
    ("Yakutsk Standard Time", "Asia/Yakutsk"),
    ("Cen. Australia Standard Time", "Australia/Adelaide"),
    ("AUS Central Standard Time", "Australia/Darwin"),
    ("E. Australia Standard Time", "Australia/Brisbane"),
    ("AUS Eastern Standard Time", "Australia/Sydney"),
    ("West Pacific Standard Time", "Pacific/Port_Moresby"),
    ("Tasmania Standard Time", "Australia/Hobart"),
    ("Vladivostok Standard Time", "Asia/Vladivostok"),
    ("Lord Howe Standard Time", "Australia/Lord_Howe"),
    ("Bougainville Standard Time", "Pacific/Bougainville"),
    ("Russia Time Zone 10", "Asia/Srednekolymsk"),
    ("Magadan Standard Time", "Asia/Magadan"),
    ("Norfolk Standard Time", "Pacific/Norfolk"),
    ("Sakhalin Standard Time", "Asia/Sakhalin"),
    ("Central Pacific Standard Time", "Pacific/Guadalcanal"),
    ("Russia Time Zone 11", "Asia/Kamchatka"),
    ("Kamchatka Standard Time", "Asia/Kamchatka"),
    ("New Zealand Standard Time", "Pacific/Auckland"),
    ("UTC+12", "Etc/GMT-12"),
    ("Fiji Standard Time", "Pacific/Fiji"),
    ("Chatham Islands Standard Time", "Pacific/Chatham"),
    ("UTC+13", "Etc/GMT-13"),
    ("Tonga Standard Time", "Pacific/Tongatapu"),
    ("Samoa Standard Time", "Pacific/Apia"),
    ("Line Islands Standard Time", "Pacific/Kiritimati"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Google Calendar's invitation layout: IANA TZID with a VTIMEZONE, folded lines, escapes.
    const GOOGLE: &str = "BEGIN:VCALENDAR\r\nPRODID:-//Google Inc//Google Calendar 70.9054//EN\r\nVERSION:2.0\r\nCALSCALE:GREGORIAN\r\nMETHOD:REQUEST\r\nBEGIN:VTIMEZONE\r\nTZID:America/New_York\r\nX-LIC-LOCATION:America/New_York\r\nBEGIN:DAYLIGHT\r\nTZOFFSETFROM:-0500\r\nTZOFFSETTO:-0400\r\nTZNAME:EDT\r\nDTSTART:19700308T020000\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU\r\nEND:DAYLIGHT\r\nBEGIN:STANDARD\r\nTZOFFSETFROM:-0400\r\nTZOFFSETTO:-0500\r\nTZNAME:EST\r\nDTSTART:19701101T020000\r\nRRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nDTSTART;TZID=America/New_York:20261104T093000\r\nDTEND;TZID=America/New_York:20261104T101500\r\nDTSTAMP:20261011T120000Z\r\nORGANIZER;CN=Ana Ruiz:mailto:ana@example.com\r\nUID:7kq2c9@google.com\r\nATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;CN=Ana Ruiz;X-NUM-GUESTS=0:mailto:ana@example.com\r\nATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=\r\n TRUE;CN=\"Lee, Sam\";X-NUM-GUESTS=0:MAILTO:Sam.Lee@Example.com\r\nDESCRIPTION:Agenda:\\n1. Budget\\, timeline\\; risks\\nJoin: https://meet.goo\r\n gle.com/abc-defg-hij\r\nLOCATION:Room 4\\, Floor 2\r\nSEQUENCE:2\r\nSTATUS:CONFIRMED\r\nSUMMARY:Q4 planning\r\nRRULE:FREQ=WEEKLY;BYDAY=WE;COUNT=4\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;TZID=America/New_York:20261111T110000\r\nDTEND;TZID=America/New_York:20261111T120000\r\nRECURRENCE-ID;TZID=America/New_York:20261111T093000\r\nUID:7kq2c9@google.com\r\nSUMMARY:Q4 planning (moved)\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn google_invitation_reads_in_utc_with_the_accounts_answer() -> Result<()> {
        let invite = invite(GOOGLE, "sam.lee@example.com")?;
        assert_eq!(invite.method, "REQUEST");
        assert_eq!(invite.uid, "7kq2c9@google.com");
        assert_eq!(invite.sequence, 2);
        // The series, not the moved occurrence; 4 November is after US DST ended (EST, UTC-5).
        assert_eq!(invite.event.title, "Q4 planning");
        assert_eq!(invite.event.start, "2026-11-04T14:30:00Z");
        assert_eq!(invite.event.end.as_deref(), Some("2026-11-04T15:15:00Z"));
        assert!(!invite.event.all_day);
        assert_eq!(invite.event.location.as_deref(), Some("Room 4, Floor 2"));
        assert_eq!(
            invite.event.description.as_deref(),
            Some("Agenda:\n1. Budget, timeline; risks\nJoin: https://meet.google.com/abc-defg-hij")
        );
        assert_eq!(
            invite
                .organizer
                .as_ref()
                .map(|o| (o.email.as_str(), o.name.as_deref())),
            Some(("ana@example.com", Some("Ana Ruiz")))
        );
        assert_eq!(invite.attendees.len(), 2);
        assert_eq!(invite.attendees[0].status, Some(Rsvp::Accepted));
        assert_eq!(invite.attendees[1].name.as_deref(), Some("Lee, Sam"));
        assert_eq!(invite.attendees[1].email, "Sam.Lee@Example.com");
        assert_eq!(
            invite.recurrence.as_deref(),
            Some("FREQ=WEEKLY;BYDAY=WE;COUNT=4")
        );
        assert_eq!(invite.response, None);
        assert!(!invite.cancelled);
        assert_eq!(
            super::invite(GOOGLE, "ana@example.com")?.response,
            Some(Rsvp::Accepted)
        );
        assert_eq!(start_zone(&invite).as_deref(), Some("America/New_York"));
        Ok(())
    }

    /// Outlook's layout: a Windows zone name with its VTIMEZONE, and a zone only the VTIMEZONE
    /// describes (the name is neither IANA nor Windows).
    #[test]
    fn outlook_windows_zones_and_custom_vtimezones_read_in_utc() -> Result<()> {
        let outlook = |tzid: &str| {
            format!(
                "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nPRODID:Microsoft Exchange Server 2010\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:{tzid}\r\nBEGIN:STANDARD\r\nDTSTART:16010101T030000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nRRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=10\r\nEND:STANDARD\r\nBEGIN:DAYLIGHT\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\nRRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=3\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nORGANIZER;CN=Jonas Weber:mailto:jonas@contoso.example\r\nATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;CN=Alex:mailto:alex@form.studio\r\nSUMMARY;LANGUAGE=en-US:Design sync\r\nDTSTART;TZID={tzid}:20261023T143000\r\nDTEND;TZID={tzid}:20261023T153000\r\nUID:040000008200E00074C5B7101A82E008\r\nSEQUENCE:0\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:040000008200E00074C5B7101A82E008\r\nRECURRENCE-ID;TZID={tzid}:20261030T143000\r\nDTSTART;TZID={tzid}:20261030T143000\r\nDTEND;TZID={tzid}:20261030T153000\r\nSUMMARY:Design sync\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
            )
        };
        // 23 October is summer time in Berlin (UTC+2).
        let windows = invite(&outlook("W. Europe Standard Time"), "Alex@Form.Studio")?;
        assert_eq!(windows.event.start, "2026-10-23T12:30:00Z");
        assert_eq!(windows.event.end.as_deref(), Some("2026-10-23T13:30:00Z"));
        assert_eq!(windows.response, None);
        assert_eq!(windows.event.title, "Design sync");
        assert_eq!(start_zone(&windows).as_deref(), Some("Europe/Berlin"));
        let custom = outlook("Customized Time Zone");
        assert_eq!(invite(&custom, "")?.event.start, "2026-10-23T12:30:00Z");
        // After the last Sunday of October (25th) the custom zone is back to UTC+1.
        let calendar = parse(&custom)?;
        let moved = calendar.children("VEVENT").nth(1).unwrap();
        assert_eq!(
            span(moved, &calendar)?.0,
            When::Time("2026-10-30T13:30:00Z".parse()?)
        );
        Ok(())
    }

    #[test]
    fn all_day_events_cancellations_and_durations() -> Result<()> {
        let event = |extra: &str| {
            format!(
                "BEGIN:VCALENDAR\nVERSION:2.0\n{extra}BEGIN:VEVENT\nUID:a@b\nDTSTART;VALUE=DATE:20261224\nDTEND;VALUE=DATE:20261226\nSUMMARY:Holidays\nEND:VEVENT\nEND:VCALENDAR\n"
            )
        };
        let holidays = invite(&event(""), "me@example.com")?;
        assert!(holidays.event.all_day);
        assert_eq!(
            (holidays.event.start.as_str(), holidays.event.end.as_deref()),
            ("2026-12-24", Some("2026-12-26"))
        );
        assert_eq!(holidays.method, "PUBLISH");
        assert!(holidays.organizer.is_none());
        assert!(invite(&event("METHOD:CANCEL\n"), "me@example.com")?.cancelled);
        let cancelled = GOOGLE.replace("STATUS:CONFIRMED", "STATUS:CANCELLED");
        assert!(invite(&cancelled, "me@example.com")?.cancelled);
        let timed = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:d\nDTSTART:20261020T160000Z\nDURATION:PT1H30M\nEND:VEVENT\nEND:VCALENDAR";
        assert_eq!(
            invite(timed, "")?.event.end.as_deref(),
            Some("2026-10-20T17:30:00Z")
        );
        let day = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:d\nDTSTART;VALUE=DATE:20261020\nEND:VEVENT\nEND:VCALENDAR";
        assert_eq!(invite(day, "")?.event.end.as_deref(), Some("2026-10-21"));
        assert!(invite("BEGIN:VCALENDAR\nEND:VCALENDAR", "").is_err());
        Ok(())
    }

    #[test]
    fn reply_round_trips_with_only_this_attendee_and_folds_lines() -> Result<()> {
        let original = invite(GOOGLE, "sam.lee@example.com")?;
        let reply = reply(
            &original,
            "sam.lee@example.com",
            "Sam",
            Rsvp::Tentative,
            Some("Running late, see you at 10"),
        )?;
        assert!(reply.ends_with("END:VCALENDAR\r\n"));
        assert!(!reply.replace("\r\n", "").contains('\n'));
        for line in reply.split("\r\n") {
            assert!(line.len() <= 75, "{line}");
        }
        let answer = invite(&reply, "sam.lee@example.com")?;
        assert_eq!(answer.method, "REPLY");
        assert_eq!(
            (answer.uid.as_str(), answer.sequence),
            ("7kq2c9@google.com", 2)
        );
        assert_eq!(answer.event.start, original.event.start);
        assert_eq!(answer.response, Some(Rsvp::Tentative));
        assert_eq!(answer.attendees.len(), 1);
        assert_eq!(answer.attendees[0].name.as_deref(), Some("Lee, Sam"));
        assert_eq!(answer.organizer, original.organizer);
        let calendar = parse(&reply)?;
        assert_eq!(calendar.value("PRODID"), Some(PRODID));
        let event = calendar.children("VEVENT").next().unwrap();
        assert!(event.property("ATTENDEE").unwrap().param("RSVP").is_none());
        assert_eq!(
            event.text("COMMENT").as_deref(),
            Some("Running late, see you at 10")
        );
        // An address the invitation did not list still answers as itself.
        let other = super::reply(&original, "alias@example.com", "Sam", Rsvp::Declined, None)?;
        let other = invite(&other, "alias@example.com")?;
        assert_eq!(
            (other.response, other.attendees.len()),
            (Some(Rsvp::Declined), 1)
        );
        // A long title folds without splitting characters.
        let long = GOOGLE.replace(
            "SUMMARY:Q4 planning",
            &format!("SUMMARY:{}", "Planificación é ".repeat(10)),
        );
        let folded = super::reply(
            &invite(&long, "")?,
            "sam.lee@example.com",
            "",
            Rsvp::Accepted,
            None,
        )?;
        assert!(folded.split("\r\n").all(|line| line.len() <= 75));
        assert_eq!(
            invite(&folded, "")?.event.title,
            "Planificación é ".repeat(10).trim()
        );
        Ok(())
    }

    #[test]
    fn calendar_files_import_instead_of_inviting() -> Result<()> {
        let file = import_file(&invite(GOOGLE, "")?)?;
        let calendar = parse(&file)?;
        assert!(calendar.property("METHOD").is_none());
        assert_eq!(calendar.children("VEVENT").count(), 2);
        assert_eq!(invite(&file, "")?.uid, "7kq2c9@google.com");
        let draft = normalize(EventDraft {
            title: "  Lunch, with Sam ".into(),
            start: "2026-10-14T12:00:00-07:00".into(),
            end: None,
            all_day: false,
            location: Some(" ".into()),
            description: Some("Bring the samples".into()),
        })?;
        assert_eq!(draft.start, "2026-10-14T19:00:00Z");
        assert_eq!(draft.end.as_deref(), Some("2026-10-14T20:00:00Z"));
        assert_eq!(draft.location, None);
        let (uid, ics) = draft_file(&draft)?;
        let read = invite(&ics, "")?;
        assert_eq!((read.uid, read.event), (uid, draft));
        assert_eq!(filename("Lunch, with Sam / 2pm?"), "Lunch with Sam 2pm.ics");
        assert_eq!(filename("../.."), "Event.ics");
        Ok(())
    }

    #[test]
    fn proposed_events_are_validated_and_normalized() {
        let draft = |start: &str, end: Option<&str>, all_day: bool| EventDraft {
            title: "Dinner".into(),
            start: start.into(),
            end: end.map(String::from),
            all_day,
            location: None,
            description: None,
        };
        assert!(
            normalize(draft(
                "2026-10-14T19:00:00+02:00",
                Some("2026-10-14T18:00:00+02:00"),
                false
            ))
            .is_err()
        );
        assert!(normalize(draft("2026-10-14 19:00", None, false)).is_err());
        assert!(normalize(draft("2026-10-14T19:00:00", None, false)).is_err());
        assert!(normalize(draft("2026-10-14T19:00:00Z", None, true)).is_err());
        assert!(
            normalize(EventDraft {
                title: "x".repeat(501),
                ..draft("2026-10-14", None, true)
            })
            .is_err()
        );
        let day = normalize(draft("2026-10-14", Some("2026-10-14"), true)).unwrap();
        assert_eq!(
            (day.start.as_str(), day.end.as_deref()),
            ("2026-10-14", Some("2026-10-15"))
        );
        let timed = normalize(draft(
            "2026-10-14T19:00:00+02:00",
            Some("2026-10-14T21:30:00+02:00"),
            false,
        ))
        .unwrap();
        assert_eq!(
            (timed.start.as_str(), timed.end.as_deref()),
            ("2026-10-14T17:00:00Z", Some("2026-10-14T19:30:00Z"))
        );
    }

    #[test]
    fn busy_times_skip_free_and_cancelled_events() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:1\r\nDTSTART:20261014T170000Z\r\nDTEND:20261014T180000Z\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:2\r\nDTSTART:20261014T190000Z\r\nDTEND:20261014T200000Z\r\nTRANSP:TRANSPARENT\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:3\r\nDTSTART:20261015T170000Z\r\nDTEND:20261015T180000Z\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        assert_eq!(
            busy_times(ics),
            vec![(
                "2026-10-14T17:00:00Z".parse().unwrap(),
                "2026-10-14T18:00:00Z".parse().unwrap()
            )]
        );
    }

    #[test]
    fn every_windows_zone_maps_to_a_known_iana_zone() {
        for (windows, iana) in WINDOWS_ZONES {
            assert!(iana.parse::<Tz>().is_ok(), "{windows} → {iana}");
        }
        assert_eq!(
            named_zone("/mozilla.org/20050126_1/America/New_York"),
            Some(chrono_tz::America::New_York)
        );
        assert_eq!(
            named_zone("pacific standard time"),
            Some(chrono_tz::America::Los_Angeles)
        );
    }
}
