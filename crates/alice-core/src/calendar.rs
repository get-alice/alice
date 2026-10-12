//! The calendars Alice hands events to and reads busy times from: Google Calendar and Microsoft
//! Graph for accounts that allowed calendar access, and CalDAV servers. Alice keeps no events of its
//! own; each request asks the provider.

use crate::{
    client::http_client,
    dav::{self, CALDAV},
    i18n::Locale,
    ical,
    model::*,
    oauth::{self, Api},
    store::Store,
    vault,
};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, NaiveDateTime, SecondsFormat, Utc};
use reqwest::Url;
use serde_json::{Value, json};
use uuid::Uuid;

const GOOGLE: &str = "https://www.googleapis.com/calendar/v3";
const GRAPH: &str = "https://graph.microsoft.com/v1.0";
const GOOGLE_NAME: &str = "Google Calendar";
const GRAPH_NAME: &str = "Microsoft Graph";
/// The longest range `/v1/calendar/busy` answers for.
pub const MAX_BUSY_DAYS: i64 = 62;

/// A connected calendar service.
enum Source {
    Google(Account),
    Microsoft(Account),
    Caldav(DavSource),
}
impl Source {
    fn id(&self) -> &str {
        match self {
            Source::Google(account) | Source::Microsoft(account) => &account.id,
            Source::Caldav(source) => &source.id,
        }
    }
}

fn sources(store: &Store) -> Result<Vec<Source>> {
    let mut sources: Vec<Source> = store
        .accounts()?
        .into_iter()
        .filter(|account| account.integrations.calendar && !account.is_demo)
        .filter_map(|account| match account.provider {
            Provider::Gmail => Some(Source::Google(account)),
            Provider::Outlook => Some(Source::Microsoft(account)),
            Provider::Imap => None,
        })
        .collect();
    sources.extend(
        store
            .dav_sources()?
            .into_iter()
            .filter(|s| s.kind == DavKind::Caldav)
            .map(Source::Caldav),
    );
    Ok(sources)
}

/// Whether any calendar is connected.
pub fn connected(store: &Store) -> Result<bool> {
    Ok(!sources(store)?.is_empty())
}

fn source_name(source: CalendarSource) -> &'static str {
    match source {
        CalendarSource::Google => "google",
        CalendarSource::Microsoft => "microsoft",
        CalendarSource::Caldav => "caldav",
    }
}

/// A `Calendar::id`: the service, the owning account or DAV source, and the service's calendar ID
/// (a CalDAV collection URL), URL-safe.
pub fn calendar_id(source: CalendarSource, owner: &str, calendar: &str) -> String {
    format!(
        "{}.{}.{}",
        source_name(source),
        URL_SAFE_NO_PAD.encode(owner),
        URL_SAFE_NO_PAD.encode(calendar)
    )
}

struct Target {
    source: CalendarSource,
    owner: String,
    calendar: String,
}
fn target(id: &str) -> Result<Target> {
    let read = || -> Option<Target> {
        let mut parts = id.split('.');
        let source = match parts.next()? {
            "google" => CalendarSource::Google,
            "microsoft" => CalendarSource::Microsoft,
            "caldav" => CalendarSource::Caldav,
            _ => return None,
        };
        let mut text = || String::from_utf8(URL_SAFE_NO_PAD.decode(parts.next()?).ok()?).ok();
        let (owner, calendar) = (text()?, text()?);
        (parts.next().is_none() && !owner.is_empty() && !calendar.is_empty()).then_some(Target {
            source,
            owner,
            calendar,
        })
    };
    read().context(crate::text!("mail.calendar.choose"))
}

/// The account or DAV source ID a `Calendar::id` belongs to.
pub fn calendar_owner(id: &str) -> Option<String> {
    target(id).ok().map(|t| t.owner)
}

/// A service URL with `segments` appended, each percent-encoded.
fn endpoint(base: &str, segments: &[&str], query: &[(&str, &str)]) -> Result<Url> {
    let mut url = Url::parse(base)?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!(crate::text!("mail.calendar.invalid_service_url")))?
        .extend(segments);
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query);
    }
    Ok(url)
}

/// Sends a request to Google or Microsoft and reads the JSON answer.
async fn call(request: reqwest::RequestBuilder, service: &str) -> Result<Value> {
    let response = request
        .send()
        .await
        .with_context(|| crate::text!("mail.service.unreachable", service = service))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        let detail = body
            .pointer("/error/message")
            .and_then(Value::as_str)
            .map(|m| format!(": {}", m.trim().trim_end_matches('.')))
            .unwrap_or_default();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            bail!(crate::text!(
                "mail.calendar.access_refused",
                service = service,
                detail = detail
            ));
        }
        bail!(crate::text!(
            "mail.service.http",
            service = service,
            status = status.as_u16(),
            detail = detail
        ));
    }
    Ok(body)
}

async fn dav_session(source: &DavSource) -> Result<dav::Session> {
    let password = vault::load(&format!("dav:{}", source.id))
        .await?
        .password
        .context(crate::text!("mail.dav.password_missing"))?;
    dav::Session::new(&source.username, &password)
}

async fn dav_calendars(source: &DavSource) -> Result<(dav::Session, Vec<dav::Collection>)> {
    let session = dav_session(source).await?;
    let collections = session
        .discover(DavKind::Caldav, &dav::server_url(&source.url)?)
        .await?;
    Ok((session, collections))
}

/// Every calendar Alice can add events to, from every connected service.
pub async fn calendars(store: &Store, locale: Locale) -> Result<CalendarList> {
    let mut list = CalendarList::default();
    for source in sources(store)? {
        match source_calendars(&source).await {
            Ok(found) => list.calendars.extend(found),
            Err(error) => list.errors.push(SourceError {
                source: source.id().into(),
                message: crate::i18n::error_text(&error, locale),
            }),
        }
    }
    Ok(list)
}

async fn source_calendars(source: &Source) -> Result<Vec<Calendar>> {
    match source {
        Source::Google(account) => {
            let token = oauth::api_token(account, Api::Google).await?;
            let url = endpoint(
                GOOGLE,
                &["users", "me", "calendarList"],
                &[("minAccessRole", "writer"), ("maxResults", "250")],
            )?;
            let body = call(http_client()?.get(url).bearer_auth(token), GOOGLE_NAME).await?;
            Ok(items(&body, "items")
                .filter_map(|item| {
                    let id = item["id"].as_str()?;
                    let name = item["summaryOverride"]
                        .as_str()
                        .or(item["summary"].as_str())
                        .unwrap_or(id);
                    Some(Calendar {
                        id: calendar_id(CalendarSource::Google, &account.id, id),
                        name: name.into(),
                        source: CalendarSource::Google,
                        owner: account.id.clone(),
                        primary: item["primary"].as_bool().unwrap_or(false),
                    })
                })
                .collect())
        }
        Source::Microsoft(account) => {
            let token = oauth::api_token(account, Api::Graph).await?;
            let url = endpoint(
                GRAPH,
                &["me", "calendars"],
                &[
                    ("$select", "id,name,canEdit,isDefaultCalendar"),
                    ("$top", "100"),
                ],
            )?;
            let body = call(http_client()?.get(url).bearer_auth(token), GRAPH_NAME).await?;
            Ok(items(&body, "value")
                .filter(|item| item["canEdit"].as_bool().unwrap_or(false))
                .filter_map(|item| {
                    let id = item["id"].as_str()?;
                    Some(Calendar {
                        id: calendar_id(CalendarSource::Microsoft, &account.id, id),
                        name: item["name"].as_str().unwrap_or("Calendar").into(),
                        source: CalendarSource::Microsoft,
                        owner: account.id.clone(),
                        primary: item["isDefaultCalendar"].as_bool().unwrap_or(false),
                    })
                })
                .collect())
        }
        Source::Caldav(source) => {
            let (_, collections) = dav_calendars(source).await?;
            Ok(collections
                .into_iter()
                .enumerate()
                .map(|(index, collection)| Calendar {
                    id: calendar_id(CalendarSource::Caldav, &source.id, collection.url.as_str()),
                    name: collection.name,
                    source: CalendarSource::Caldav,
                    owner: source.id.clone(),
                    primary: index == 0,
                })
                .collect())
        }
    }
}

fn items<'a>(body: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> + 'a {
    body[key].as_array().into_iter().flatten()
}

/// An event to hand to a calendar: an invitation from a message, or a checked draft.
enum Event {
    Invite(Invite),
    Draft(EventDraft),
}
impl Event {
    fn draft(&self) -> &EventDraft {
        match self {
            Event::Invite(invite) => &invite.event,
            Event::Draft(draft) => draft,
        }
    }
    /// The calendar file that imports the event, and the event's UID.
    fn file(&self) -> Result<(String, String)> {
        match self {
            Event::Invite(invite) => Ok((invite.uid.clone(), ical::import_file(invite)?)),
            Event::Draft(draft) => ical::draft_file(draft),
        }
    }
}

fn event(store: &Store, message_id: Option<String>, draft: Option<EventDraft>) -> Result<Event> {
    match (message_id, draft) {
        (Some(id), None) => {
            let invite = store
                .message(&id)?
                .invite
                .context(crate::text!("mail.calendar.no_invitation"))?;
            if invite.cancelled {
                bail!(crate::text!("mail.calendar.cancelled"));
            }
            Ok(Event::Invite(invite))
        }
        (None, Some(draft)) => Ok(Event::Draft(ical::normalize(draft)?)),
        _ => bail!(crate::text!("mail.calendar.invitation_or_event")),
    }
}

/// A calendar file to open in the operating system's calendar app.
pub fn event_file(store: &Store, request: EventFileRequest) -> Result<EventFile> {
    let event = event(store, request.message_id, request.event)?;
    let (_, ics) = event.file()?;
    Ok(EventFile {
        filename: ical::filename(&event.draft().title),
        ics,
    })
}

/// The account a Google or Microsoft calendar belongs to, if it still allows calendar access.
fn calendar_account(store: &Store, id: &str, provider: Provider) -> Result<Account> {
    let account = store.account(id)?;
    if account.provider != provider || account.is_demo {
        bail!(crate::text!("mail.calendar.choose"));
    }
    if !account.integrations.calendar {
        bail!(crate::text!(
            "mail.calendar.allow_access",
            email = &account.email
        ));
    }
    Ok(account)
}

/// Puts an invitation or a new event on a calendar.
pub async fn add_event(store: &Store, input: AddEvent) -> Result<CreatedEvent> {
    let target = target(&input.calendar)?;
    let event = event(store, input.message_id, input.event)?;
    let url = match target.source {
        CalendarSource::Google => {
            let account = calendar_account(store, &target.owner, Provider::Gmail)?;
            google_add(
                &oauth::api_token(&account, Api::Google).await?,
                &target.calendar,
                &event,
            )
            .await?
        }
        CalendarSource::Microsoft => {
            let account = calendar_account(store, &target.owner, Provider::Outlook)?;
            graph_add(
                &oauth::api_token(&account, Api::Graph).await?,
                &target.calendar,
                event.draft(),
            )
            .await?
        }
        CalendarSource::Caldav => {
            let source = store.dav_source(&target.owner)?;
            if source.kind != DavKind::Caldav {
                bail!(crate::text!("mail.calendar.choose"));
            }
            // The ID came from a client: only a calendar the server still lists gets the password.
            let (session, collections) = dav_calendars(&source).await?;
            let collection = collections
                .into_iter()
                .find(|c| c.url.as_str() == target.calendar)
                .context(crate::text!("mail.calendar.no_longer_available"))?;
            let (uid, ics) = event.file()?;
            let mut base = collection.url;
            if !base.path().ends_with('/') {
                base.set_path(&format!("{}/", base.path()));
            }
            let url = base.join(&format!(
                "{}.ics",
                Uuid::new_v5(&Uuid::NAMESPACE_URL, uid.as_bytes())
            ))?;
            session
                .create(&url, "text/calendar; charset=utf-8", &ics)
                .await?;
            None
        }
    };
    Ok(CreatedEvent {
        calendar: input.calendar,
        url,
    })
}

fn google_time(draft: &EventDraft, value: &str, zone: Option<&str>) -> Value {
    match (draft.all_day, zone) {
        (true, _) => json!({ "date": value }),
        (false, Some(zone)) => json!({ "dateTime": value, "timeZone": zone }),
        (false, None) => json!({ "dateTime": value }),
    }
}

async fn google_add(token: &str, calendar: &str, event: &Event) -> Result<Option<String>> {
    let draft = event.draft();
    let end = draft.end.as_deref().unwrap_or(&draft.start);
    let (url, zone) = match event {
        // Import keeps the invitation's iCalUID, so the organizer's updates find the event.
        Event::Invite(invite) => (
            endpoint(GOOGLE, &["calendars", calendar, "events", "import"], &[])?,
            ical::start_zone(invite),
        ),
        Event::Draft(_) => (
            endpoint(GOOGLE, &["calendars", calendar, "events"], &[])?,
            None,
        ),
    };
    let mut body = json!({
        "summary": draft.title,
        "start": google_time(draft, &draft.start, zone.as_deref()),
        "end": google_time(draft, end, zone.as_deref()),
    });
    if let Some(location) = &draft.location {
        body["location"] = json!(location);
    }
    if let Some(description) = &draft.description {
        body["description"] = json!(description);
    }
    if let Event::Invite(invite) = event {
        body["iCalUID"] = json!(invite.uid);
        body["sequence"] = json!(invite.sequence);
        if let Some(organizer) = &invite.organizer {
            body["organizer"] = json!({ "email": organizer.email, "displayName": organizer.name });
        }
        body["attendees"] = invite
            .attendees
            .iter()
            .map(|a| {
                let status = match a.status {
                    Some(Rsvp::Accepted) => "accepted",
                    Some(Rsvp::Tentative) => "tentative",
                    Some(Rsvp::Declined) => "declined",
                    None => "needsAction",
                };
                json!({ "email": a.email, "displayName": a.name, "responseStatus": status })
            })
            .collect();
        let recurrence = ical::recurrence_lines(invite);
        if !recurrence.is_empty() {
            body["recurrence"] = json!(recurrence);
            // Google repeats an event in a named zone; without one, in UTC.
            if zone.is_none() && !draft.all_day {
                body["start"]["timeZone"] = json!("UTC");
                body["end"]["timeZone"] = json!("UTC");
            }
        }
    }
    let created = call(
        http_client()?.post(url).bearer_auth(token).json(&body),
        GOOGLE_NAME,
    )
    .await?;
    Ok(created["htmlLink"].as_str().map(String::from))
}

/// A Graph `dateTimeTimeZone` in UTC; all-day events start and end at midnight.
fn graph_time(draft: &EventDraft, value: &str) -> Result<Value> {
    let time = if draft.all_day {
        format!("{value}T00:00:00")
    } else {
        DateTime::parse_from_rfc3339(value)?
            .with_timezone(&Utc)
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string()
    };
    Ok(json!({ "dateTime": time, "timeZone": "UTC" }))
}

async fn graph_add(token: &str, calendar: &str, draft: &EventDraft) -> Result<Option<String>> {
    let end = draft.end.as_deref().unwrap_or(&draft.start);
    let body = json!({
        "subject": draft.title,
        "body": { "contentType": "text", "content": draft.description.clone().unwrap_or_default() },
        "start": graph_time(draft, &draft.start)?,
        "end": graph_time(draft, end)?,
        "isAllDay": draft.all_day,
        "location": { "displayName": draft.location.clone().unwrap_or_default() },
    });
    let url = endpoint(GRAPH, &["me", "calendars", calendar, "events"], &[])?;
    let created = call(
        http_client()?.post(url).bearer_auth(token).json(&body),
        GRAPH_NAME,
    )
    .await?;
    Ok(created["webLink"].as_str().map(String::from))
}

type Interval = (DateTime<Utc>, DateTime<Utc>);

fn instant(value: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value.trim())
        .context(crate::text!("mail.calendar.rfc3339_example"))?
        .with_timezone(&Utc))
}

/// Busy times across every connected calendar between `from` (default now) and `to` (default a
/// week later, at most [`MAX_BUSY_DAYS`] later). Only times; never titles.
pub async fn busy(store: &Store, query: &BusyQuery, locale: Locale) -> Result<BusyReport> {
    let from = query
        .from
        .as_deref()
        .map(instant)
        .transpose()?
        .unwrap_or_else(Utc::now);
    let to = query
        .to
        .as_deref()
        .map(instant)
        .transpose()?
        .unwrap_or(from + Duration::days(7));
    if to <= from {
        bail!(crate::text!("mail.calendar.range_end"));
    }
    if to - from > Duration::days(MAX_BUSY_DAYS) {
        bail!(crate::text!(
            "mail.calendar.busy_limit",
            days = MAX_BUSY_DAYS
        ));
    }
    let mut report = BusyReport::default();
    let mut intervals = Vec::new();
    for source in sources(store)? {
        match source_busy(&source, from, to).await {
            Ok(found) => {
                report.sources += 1;
                intervals.extend(found);
            }
            Err(error) => report.errors.push(SourceError {
                source: source.id().into(),
                message: crate::i18n::error_text(&error, locale),
            }),
        }
    }
    report.busy = merge(intervals, from, to);
    Ok(report)
}

/// Clips intervals to `from`–`to`, sorts them, and joins those that overlap or touch.
pub fn merge(mut intervals: Vec<Interval>, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Busy> {
    intervals = intervals
        .into_iter()
        .map(|(start, end)| (start.max(from), end.min(to)))
        .filter(|(start, end)| start < end)
        .collect();
    intervals.sort();
    let mut merged: Vec<Interval> = Vec::new();
    for (start, end) in intervals {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
        .into_iter()
        .map(|(start, end)| Busy {
            start: start.to_rfc3339_opts(SecondsFormat::Secs, true),
            end: end.to_rfc3339_opts(SecondsFormat::Secs, true),
        })
        .collect()
}

async fn source_busy(
    source: &Source,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<Interval>> {
    let rfc3339 = |time: DateTime<Utc>| time.to_rfc3339_opts(SecondsFormat::Secs, true);
    match source {
        Source::Google(account) => {
            let token = oauth::api_token(account, Api::Google).await?;
            let http = http_client()?;
            // The person's own calendars that their calendar shows; not other people's.
            let url = endpoint(
                GOOGLE,
                &["users", "me", "calendarList"],
                &[("maxResults", "250")],
            )?;
            let list = call(http.get(url).bearer_auth(&token), GOOGLE_NAME).await?;
            let ids: Vec<Value> = items(&list, "items")
                .filter(|c| {
                    c["primary"].as_bool().unwrap_or(false)
                        || (c["accessRole"] == "owner" && c["selected"].as_bool().unwrap_or(false))
                })
                .filter_map(|c| c["id"].as_str())
                .take(50)
                .map(|id| json!({ "id": id }))
                .collect();
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            let body = json!({ "timeMin": rfc3339(from), "timeMax": rfc3339(to), "items": ids });
            let answer = call(
                http.post(endpoint(GOOGLE, &["freeBusy"], &[])?)
                    .bearer_auth(&token)
                    .json(&body),
                GOOGLE_NAME,
            )
            .await?;
            Ok(answer["calendars"]
                .as_object()
                .into_iter()
                .flat_map(|calendars| calendars.values())
                .flat_map(|calendar| items(calendar, "busy"))
                .filter_map(|b| {
                    Some((
                        instant(b["start"].as_str()?).ok()?,
                        instant(b["end"].as_str()?).ok()?,
                    ))
                })
                .collect())
        }
        Source::Microsoft(account) => {
            let token = oauth::api_token(account, Api::Graph).await?;
            let http = http_client()?;
            let (start, end) = (rfc3339(from), rfc3339(to));
            let mut url = endpoint(
                GRAPH,
                &["me", "calendarView"],
                &[
                    ("startDateTime", start.as_str()),
                    ("endDateTime", end.as_str()),
                    ("$select", "showAs,start,end,isCancelled"),
                    ("$top", "250"),
                ],
            )?;
            let mut found = Vec::new();
            for _ in 0..20 {
                let page = call(
                    http.get(url.clone())
                        .bearer_auth(&token)
                        .header("Prefer", "outlook.timezone=\"UTC\""),
                    GRAPH_NAME,
                )
                .await?;
                found.extend(
                    items(&page, "value")
                        .filter(|e| {
                            e["showAs"].as_str().is_some_and(|s| s != "free")
                                && !e["isCancelled"].as_bool().unwrap_or(false)
                        })
                        .filter_map(|e| {
                            Some((graph_instant(&e["start"])?, graph_instant(&e["end"])?))
                        }),
                );
                // The next page link carries the token only back to Graph.
                match page["@odata.nextLink"]
                    .as_str()
                    .and_then(|next| Url::parse(next).ok())
                {
                    Some(next)
                        if next.scheme() == "https"
                            && next.host_str() == Some("graph.microsoft.com") =>
                    {
                        url = next
                    }
                    _ => break,
                }
            }
            Ok(found)
        }
        Source::Caldav(source) => {
            let (session, collections) = dav_calendars(source).await?;
            let range = |time: DateTime<Utc>| time.format("%Y%m%dT%H%M%SZ").to_string();
            let (start, end) = (range(from), range(to));
            // `expand` asks the server to list each occurrence of a repeating event, in UTC.
            let query = format!(
                r#"<?xml version="1.0" encoding="utf-8"?><cal:calendar-query {ns}><d:prop><cal:calendar-data><cal:expand start="{start}" end="{end}"/></cal:calendar-data></d:prop><cal:filter><cal:comp-filter name="VCALENDAR"><cal:comp-filter name="VEVENT"><cal:time-range start="{start}" end="{end}"/></cal:comp-filter></cal:comp-filter></cal:filter></cal:calendar-query>"#,
                ns = dav::NAMESPACES
            );
            let mut found = Vec::new();
            for collection in collections {
                for data in session
                    .report(&collection.url, &query, CALDAV, "calendar-data")
                    .await?
                {
                    found.extend(ical::busy_times(&data));
                }
            }
            Ok(found)
        }
    }
}

/// A Graph `dateTimeTimeZone` read in UTC, as requested with `Prefer: outlook.timezone="UTC"`.
fn graph_instant(value: &Value) -> Option<DateTime<Utc>> {
    let time = value["dateTime"].as_str()?;
    if let Some(zone) = value["timeZone"].as_str()
        && !zone.eq_ignore_ascii_case("UTC")
    {
        return None;
    }
    NaiveDateTime::parse_from_str(time, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|t| t.and_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_times_are_clipped_sorted_and_merged() {
        let at = |s: &str| s.parse::<DateTime<Utc>>().unwrap();
        let busy = merge(
            vec![
                (at("2026-10-14T15:00:00Z"), at("2026-10-14T16:00:00Z")),
                (at("2026-10-14T09:00:00Z"), at("2026-10-14T10:00:00Z")),
                // Overlaps the first, and touches the next.
                (at("2026-10-14T09:30:00Z"), at("2026-10-14T11:00:00Z")),
                (at("2026-10-14T11:00:00Z"), at("2026-10-14T11:30:00Z")),
                // Starts before the range; one entirely after it.
                (at("2026-10-13T22:00:00Z"), at("2026-10-14T01:00:00Z")),
                (at("2026-10-16T09:00:00Z"), at("2026-10-16T10:00:00Z")),
                (at("2026-10-14T12:00:00Z"), at("2026-10-14T12:00:00Z")),
            ],
            at("2026-10-14T00:00:00Z"),
            at("2026-10-15T00:00:00Z"),
        );
        let pairs: Vec<(&str, &str)> = busy
            .iter()
            .map(|b| (b.start.as_str(), b.end.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("2026-10-14T00:00:00Z", "2026-10-14T01:00:00Z"),
                ("2026-10-14T09:00:00Z", "2026-10-14T11:30:00Z"),
                ("2026-10-14T15:00:00Z", "2026-10-14T16:00:00Z"),
            ]
        );
    }

    #[test]
    fn calendar_ids_are_url_safe_and_name_their_owner() -> Result<()> {
        let id = calendar_id(
            CalendarSource::Caldav,
            "dav-1",
            "https://dav.example.test/cal/home/?a=b#c",
        );
        assert!(
            id.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        );
        let read = target(&id)?;
        assert_eq!(
            (read.source, read.owner.as_str(), read.calendar.as_str()),
            (
                CalendarSource::Caldav,
                "dav-1",
                "https://dav.example.test/cal/home/?a=b#c"
            )
        );
        assert_eq!(
            calendar_owner(&calendar_id(CalendarSource::Google, "acct", "primary")).as_deref(),
            Some("acct")
        );
        assert!(
            target("google.YQ").is_err() && target("other.YQ.YQ").is_err() && target("").is_err()
        );
        Ok(())
    }

    #[test]
    fn graph_times_are_read_only_in_utc() {
        assert_eq!(
            graph_instant(&json!({"dateTime":"2026-10-14T17:00:00.0000000","timeZone":"UTC"})),
            Some("2026-10-14T17:00:00Z".parse().unwrap())
        );
        assert_eq!(
            graph_instant(
                &json!({"dateTime":"2026-10-14T17:00:00","timeZone":"Pacific Standard Time"})
            ),
            None
        );
    }
}
