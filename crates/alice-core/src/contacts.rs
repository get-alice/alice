//! Recipient suggestions. People come from this device's mail, ranked by how often and how recently
//! you wrote to them, and from read-only copies of address books: the Google and Microsoft contacts
//! of accounts that allowed it, CardDAV servers, and the operating system's contacts, which the
//! desktop app imports. Address books are only read; nothing is ever written back to them.

use crate::{
    client::http_client,
    dav::{self, Collection, Session},
    model::*,
    oauth::{self, Api},
    search::fold,
    store::Store,
    vault,
};
use anyhow::{Context, Result, bail};
use parking_lot::Mutex;
use reqwest::{Url, header};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Most contacts kept from one address book.
pub const SOURCE_LIMIT: usize = 20_000;
/// Longest display name kept, in characters.
const NAME_LIMIT: usize = 200;
/// Each message you sent counts this much for each of its recipients; a message someone sent you
/// counts 1 for them. The people you write to are the ones you are likely to write to again.
const SENT_WEIGHT: f64 = 10.0;
/// A message counts half as much after this many days, so current correspondents outrank old ones.
const HALF_LIFE_DAYS: f64 = 90.0;
/// While mail keeps changing, the suggestion index is rebuilt at most this often.
const REBUILD_INTERVAL: Duration = Duration::from_secs(5);
/// How often address books are read again in the background.
const REFRESH_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const DEFAULT_LIMIT: usize = 8;
const MAX_LIMIT: usize = 50;
/// Provider address books are read at most this many pages deep.
const PAGE_LIMIT: usize = 100;

/// The suggestion index: everyone known, from one pass over stored mail plus the stored address
/// books, kept until the store changes.
#[derive(Default)]
pub struct Directory {
    index: Mutex<Option<Index>>,
    /// Set when an address book changed, to rebuild on next use without waiting out the interval.
    stale: AtomicBool,
}
struct Index {
    revision: String,
    built: Instant,
    people: Arc<Vec<Person>>,
}
impl Directory {
    /// The best people for `query`, best first; see [`ContactQuery`]. Walks stored mail when it
    /// changed since the index was built, so call this off the async runtime.
    pub fn suggest(&self, store: &Store, query: &ContactQuery) -> Result<Vec<Contact>> {
        Ok(rank(&self.people(store)?, query))
    }
    /// Rebuilds the index on next use, e.g. after an address book changed.
    pub fn invalidate(&self) {
        self.stale.store(true, Ordering::Relaxed);
    }
    fn people(&self, store: &Store) -> Result<Arc<Vec<Person>>> {
        // Concurrent lookups wait for one build instead of each walking the mail.
        let mut index = self.index.lock();
        let stale = self.stale.swap(false, Ordering::Relaxed);
        if let Some(current) = index.as_ref()
            && !stale
            && (current.built.elapsed() < REBUILD_INTERVAL || current.revision == store.revision())
        {
            return Ok(current.people.clone());
        }
        let (revision, people) =
            build(store).inspect_err(|_| self.stale.store(stale, Ordering::Relaxed))?;
        let people = Arc::new(people);
        *index = Some(Index {
            revision,
            built: Instant::now(),
            people: people.clone(),
        });
        Ok(people)
    }
}

/// Someone who can be suggested.
struct Person {
    /// Lowercased.
    email: String,
    name: String,
    sources: Vec<ContactSource>,
    /// Frecency of your mail with them; 0 when they are only in address books.
    score: f64,
    /// Accounts whose mail or provider address book has them.
    accounts: Vec<String>,
    folded_name: String,
    folded_email: String,
    /// Folded words a query may start: those of the name and the pieces of the address's local part.
    words: Vec<String>,
    folded_domain: String,
}
impl Person {
    /// How well the folded query `q` (split into `terms`) matches: 0 for the name or address, 1 for
    /// only the address's domain, `None` for no match.
    fn strength(&self, q: &str, terms: &[&str]) -> Option<u8> {
        if self.folded_name.starts_with(q)
            || self.folded_email.starts_with(q)
            || terms
                .iter()
                .all(|term| self.words.iter().any(|word| word.starts_with(term)))
        {
            Some(0)
        } else if self.folded_domain.starts_with(q) {
            Some(1)
        } else {
            None
        }
    }
}

/// The people of `people` (best first) matching `query`: those of the sending account first, then
/// name and address matches before domain-only ones. An empty query lists your correspondents.
fn rank(people: &[Person], query: &ContactQuery) -> Vec<Contact> {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let q = fold(query.q.as_deref().unwrap_or("").trim());
    let terms: Vec<&str> = q.split_whitespace().collect();
    let account = query.account_id.as_deref().filter(|id| !id.is_empty());
    let mut found: Vec<(bool, u8, &Person)> = people
        .iter()
        .filter_map(|person| {
            let strength = if q.is_empty() {
                person.sources.contains(&ContactSource::Mail).then_some(0)
            } else {
                person.strength(&q, &terms)
            }?;
            let elsewhere = account.is_some_and(|id| !person.accounts.iter().any(|a| a == id));
            Some((elsewhere, strength, person))
        })
        .collect();
    // `people` is in frecency order, which this stable sort keeps within each group.
    found.sort_by_key(|(elsewhere, strength, _)| (*elsewhere, *strength));
    found
        .into_iter()
        .take(limit)
        .map(|(_, _, person)| Contact {
            name: person.name.clone(),
            email: person.email.clone(),
            sources: person.sources.clone(),
        })
        .collect()
}

/// Someone in stored mail.
#[derive(Default)]
struct Correspondent {
    /// From the newest message they sent.
    name: String,
    named_at: f64,
    /// Whether you ever wrote to them.
    wrote: bool,
    score: f64,
    accounts: Vec<String>,
}

/// Everyone in stored mail but yourself, by lowercased address, with the store revision walked.
fn correspondents(store: &Store) -> Result<(String, HashMap<String, Correspondent>)> {
    let own: HashSet<String> = store
        .accounts()?
        .into_iter()
        .map(|a| a.email.trim().to_lowercase())
        .collect();
    let now = chrono::Utc::now().timestamp_millis() as f64 / 86_400_000.0 + 2_440_587.5;
    let mut people: HashMap<String, Correspondent> = HashMap::new();
    let mut counted = HashSet::new();
    let revision = store.each_correspondence(
        |account, folder, from_name, from_email, recipients, julian| {
            // Providers such as Gmail keep one message in several folders; count it once.
            let mut copy = DefaultHasher::new();
            (account, from_email, recipients, julian.to_bits()).hash(&mut copy);
            if !counted.insert(copy.finish()) {
                return;
            }
            let weight = 0.5f64.powf((now - julian).max(0.0) / HALF_LIFE_DAYS);
            let from = from_email.trim().to_lowercase();
            if folder == "sent" || own.contains(&from) {
                for to in recipients.split_whitespace().filter_map(address) {
                    if !own.contains(&to) {
                        note(&mut people, to, account, SENT_WEIGHT * weight).wrote = true;
                    }
                }
            } else if let Some(from) = address(&from) {
                let person = note(&mut people, from, account, weight);
                let name = display_name(from_name);
                if !name.is_empty() && julian >= person.named_at {
                    person.name = name;
                    person.named_at = julian;
                }
            }
        },
    )?;
    Ok((revision, people))
}
/// Adds a message's `score` to `email`'s correspondent, seen in `account`'s mail.
fn note<'a>(
    people: &'a mut HashMap<String, Correspondent>,
    email: String,
    account: &str,
    score: f64,
) -> &'a mut Correspondent {
    let person = people.entry(email).or_default();
    person.score += score;
    if !person.accounts.iter().any(|a| a == account) {
        person.accounts.push(account.to_string());
    }
    person
}

/// The suggestion index: correspondents (without automated senders you never wrote to) merged with
/// address book contacts by address, highest frecency first. Address book names win over the
/// names in mail.
fn build(store: &Store) -> Result<(String, Vec<Person>)> {
    let (revision, mail) = correspondents(store)?;
    let book = store.address_book()?;
    let mut people: HashMap<String, Person> = HashMap::with_capacity(mail.len() + book.len());
    let person = |email: String,
                  name: String,
                  sources: Vec<ContactSource>,
                  score: f64,
                  accounts: Vec<String>| Person {
        email,
        name,
        sources,
        score,
        accounts,
        folded_name: String::new(),
        folded_email: String::new(),
        words: Vec::new(),
        folded_domain: String::new(),
    };
    for (email, found) in mail {
        if (!found.wrote && automated(&email)) || email.parse::<lettre::Address>().is_err() {
            continue;
        }
        people.insert(
            email.clone(),
            person(
                email,
                found.name,
                vec![ContactSource::Mail],
                found.score,
                found.accounts,
            ),
        );
    }
    let mut named = HashSet::new();
    for contact in book {
        let entry = people.entry(contact.email.clone()).or_insert_with(|| {
            person(
                contact.email.clone(),
                String::new(),
                Vec::new(),
                0.0,
                Vec::new(),
            )
        });
        if !entry.sources.contains(&contact.source) {
            entry.sources.push(contact.source);
        }
        if matches!(
            contact.source,
            ContactSource::Google | ContactSource::Microsoft
        ) && !entry.accounts.contains(&contact.owner)
        {
            entry.accounts.push(contact.owner);
        }
        if !contact.name.is_empty() && named.insert(contact.email) {
            entry.name = contact.name;
        }
    }
    let mut people: Vec<Person> = people.into_values().collect();
    for person in &mut people {
        person.sources.sort();
        person.folded_name = fold(&person.name);
        person.folded_email = fold(&person.email);
        let (local, domain) = person
            .folded_email
            .rsplit_once('@')
            .unwrap_or((person.folded_email.as_str(), ""));
        person.folded_domain = domain.to_string();
        person.words = person
            .folded_name
            .split(|c: char| !c.is_alphanumeric() && c != '\'')
            .chain(local.split(['.', '_', '-', '+']))
            .filter(|word| !word.is_empty())
            .map(str::to_string)
            .collect();
    }
    people.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.name.is_empty().cmp(&b.name.is_empty()))
            .then_with(|| a.folded_name.cmp(&b.folded_name))
            .then_with(|| a.email.cmp(&b.email))
    });
    Ok((revision, people))
}

/// A lowercased address from a stored header field, or `None` when it does not look like one.
fn address(text: &str) -> Option<String> {
    let text = text
        .trim()
        .trim_matches(|c: char| matches!(c, '<' | '>' | ',' | ';' | '"' | '\''));
    let (local, domain) = text.rsplit_once('@')?;
    (!local.is_empty() && !domain.is_empty()).then(|| text.to_lowercase())
}

/// Addresses that send mail nobody answers: no-reply senders, bounces, delivery reports and
/// notifications.
fn automated(email: &str) -> bool {
    let (local, domain) = email.rsplit_once('@').unwrap_or((email, ""));
    let local: String = local
        .split('+')
        .next()
        .unwrap_or("")
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    ["noreply", "donotreply", "mailerdaemon", "bounce"]
        .iter()
        .any(|word| local.contains(word))
        || ["postmaster", "notification", "notify"]
            .iter()
            .any(|word| local.starts_with(word))
        || domain.split('.').any(|label| label.starts_with("bounce"))
}

/// A display name with whitespace collapsed and quotes removed; empty when it is an address.
fn display_name(raw: &str) -> String {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = name.trim_matches(|c: char| c == '"' || c == '\'').trim();
    if name.contains('@') {
        return String::new();
    }
    name.chars().take(NAME_LIMIT).collect()
}

/// Valid, distinct addresses (lowercased) with tidy names, at most [`SOURCE_LIMIT`]. When an address
/// is listed twice, the first name given wins.
pub fn tidy(contacts: impl IntoIterator<Item = ImportedContact>) -> Vec<ImportedContact> {
    let mut kept: Vec<ImportedContact> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for contact in contacts {
        let email = contact.email.trim();
        let email = match email.get(..7) {
            Some(scheme) if scheme.eq_ignore_ascii_case("mailto:") => &email[7..],
            _ => email,
        };
        let email = email.trim().trim_matches(|c: char| c == '<' || c == '>');
        if email.len() > 254 || email.parse::<lettre::Address>().is_err() {
            continue;
        }
        let email = email.to_lowercase();
        let name = display_name(&contact.name);
        match index.get(&email) {
            Some(&i) if kept[i].name.is_empty() => kept[i].name = name,
            Some(_) => {}
            None if kept.len() < SOURCE_LIMIT => {
                index.insert(email.clone(), kept.len());
                kept.push(ImportedContact { name, email });
            }
            None => {}
        }
    }
    kept
}

/// Serializes address book refreshes, so two never replace the same address book at once.
static REFRESHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Reads every address book again: the Google and Microsoft contacts of accounts that allowed it,
/// and every CardDAV server. A failing source keeps its previous contacts and does not stop the
/// others; its error is reported (and recorded on DAV sources).
pub async fn refresh_all(store: &Store, locale: crate::i18n::Locale) -> Result<ContactsSync> {
    let _running = REFRESHING.lock().await;
    let mut errors = Vec::new();
    for account in store
        .accounts()?
        .into_iter()
        .filter(|a| a.integrations.contacts)
    {
        if let Err(e) = refresh_account(store, &account).await {
            errors.push(SourceError {
                source: account.id,
                message: crate::i18n::error_text(&e, locale),
            });
        }
    }
    for source in store
        .dav_sources()?
        .into_iter()
        .filter(|s| s.kind == DavKind::Carddav)
    {
        if let Err(e) = refresh_carddav(store, &source).await {
            errors.push(SourceError {
                source: source.id,
                message: crate::i18n::error_text(&e, locale),
            });
        }
    }
    Ok(ContactsSync {
        contacts: store.synced_contact_count()?,
        errors,
    })
}

/// Refreshes address books when the daemon starts and every six hours after.
pub async fn run_background(store: Arc<Store>) {
    loop {
        // DAV sources record their errors; `POST /v1/contacts/sync` reports the accounts' ones.
        let _ = refresh_all(&store, crate::i18n::Locale::EnUs).await;
        tokio::time::sleep(REFRESH_INTERVAL).await;
    }
}

/// Reads an account's provider address book (Google People or Microsoft Graph) into the store.
/// Returns how many contacts are stored.
pub async fn refresh_account(store: &Store, account: &Account) -> Result<usize> {
    if !account.integrations.contacts {
        bail!(crate::text!("mail.contacts.not_shared"));
    }
    let (source, contacts) = match account.provider {
        Provider::Gmail => (
            ContactSource::Google,
            google_contacts(&oauth::api_token(account, Api::Google).await?).await?,
        ),
        Provider::Outlook => (
            ContactSource::Microsoft,
            graph_contacts(&oauth::api_token(account, Api::Graph).await?).await?,
        ),
        Provider::Imap => bail!(crate::text!("mail.contacts.provider_required")),
    };
    store.replace_contacts(source, &account.id, &tidy(contacts))
}

/// A provider API's JSON answer, or an error carrying the provider's own message.
async fn fetch(http: &reqwest::Client, url: Url, token: &str, service: &str) -> Result<String> {
    let response = http
        .get(url)
        .bearer_auth(token)
        .header(header::ACCEPT, "application/json")
        .send()
        .await
        .with_context(|| {
            if service == "Microsoft contacts" {
                crate::text!("mail.contacts.microsoft_unreachable")
            } else {
                crate::text!("mail.service.unreachable", service = service)
            }
        })?;
    let status = response.status();
    let body = response.text().await.with_context(|| {
        if service == "Microsoft contacts" {
            crate::text!("mail.contacts.microsoft_incomplete")
        } else {
            crate::text!("mail.service.incomplete", service = service)
        }
    })?;
    if !status.is_success() {
        let detail = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
            .map(|message| format!(": {message}"))
            .unwrap_or_default();
        if service == "Microsoft contacts" {
            bail!(crate::text!(
                "mail.contacts.microsoft_http",
                status = status.as_u16(),
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeoplePage {
    #[serde(default)]
    connections: Vec<PeoplePerson>,
    #[serde(default)]
    other_contacts: Vec<PeoplePerson>,
    next_page_token: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeoplePerson {
    #[serde(default)]
    names: Vec<PeopleName>,
    #[serde(default)]
    email_addresses: Vec<PeopleEmail>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeopleName {
    #[serde(default)]
    display_name: String,
}
#[derive(Deserialize)]
struct PeopleEmail {
    #[serde(default)]
    value: String,
}
/// One page of Google People `connections` or `otherContacts`: a contact per address, and the next
/// page's token.
fn people_page(body: &str) -> Result<(Vec<ImportedContact>, Option<String>)> {
    let page: PeoplePage =
        serde_json::from_str(body).context(crate::text!("mail.contacts.google_unreadable"))?;
    let contacts = page
        .connections
        .into_iter()
        .chain(page.other_contacts)
        .flat_map(|person| {
            let name = person
                .names
                .iter()
                .map(|n| n.display_name.trim())
                .find(|n| !n.is_empty())
                .unwrap_or("")
                .to_string();
            person
                .email_addresses
                .into_iter()
                .map(move |e| ImportedContact {
                    name: name.clone(),
                    email: e.value,
                })
        })
        .collect();
    Ok((
        contacts,
        page.next_page_token.filter(|token| !token.is_empty()),
    ))
}
/// The account's Google contacts, then the people Google keeps from its mail ("other contacts").
async fn google_contacts(token: &str) -> Result<Vec<ImportedContact>> {
    let http = http_client()?;
    let mut contacts = Vec::new();
    for (path, fields) in [
        ("people/me/connections", "personFields"),
        ("otherContacts", "readMask"),
    ] {
        let mut page_token: Option<String> = None;
        for _ in 0..PAGE_LIMIT {
            if contacts.len() >= SOURCE_LIMIT {
                break;
            }
            let mut url = Url::parse("https://people.googleapis.com/v1/")?.join(path)?;
            url.query_pairs_mut()
                .append_pair(fields, "names,emailAddresses")
                .append_pair("pageSize", "1000");
            if let Some(token) = &page_token {
                url.query_pairs_mut().append_pair("pageToken", token);
            }
            let (page, next) = people_page(&fetch(&http, url, token, "Google Contacts").await?)?;
            contacts.extend(page);
            match next {
                Some(next) if page_token.as_ref() != Some(&next) => page_token = Some(next),
                _ => break,
            }
        }
    }
    Ok(contacts)
}

#[derive(Deserialize)]
struct GraphPage {
    #[serde(default)]
    value: Vec<GraphContact>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphContact {
    display_name: Option<String>,
    #[serde(default)]
    email_addresses: Vec<GraphEmail>,
}
#[derive(Deserialize)]
struct GraphEmail {
    name: Option<String>,
    address: Option<String>,
}
/// One page of Microsoft Graph contacts: a contact per address, and the next page's link.
fn graph_page(body: &str) -> Result<(Vec<ImportedContact>, Option<String>)> {
    let page: GraphPage =
        serde_json::from_str(body).context(crate::text!("mail.contacts.microsoft_unreadable"))?;
    let contacts = page
        .value
        .into_iter()
        .flat_map(|contact| {
            let display = contact.display_name.unwrap_or_default();
            contact.email_addresses.into_iter().filter_map(move |e| {
                let email = e.address?;
                // The address's own name is often just the address again.
                let name = if display.trim().is_empty() {
                    e.name
                        .filter(|n| !n.eq_ignore_ascii_case(&email))
                        .unwrap_or_default()
                } else {
                    display.clone()
                };
                Some(ImportedContact { name, email })
            })
        })
        .collect();
    Ok((contacts, page.next_link))
}
/// The contacts in the Outlook account's default contacts folder.
async fn graph_contacts(token: &str) -> Result<Vec<ImportedContact>> {
    let http = http_client()?;
    let mut contacts = Vec::new();
    let mut next = Some(Url::parse(
        "https://graph.microsoft.com/v1.0/me/contacts?$select=displayName,emailAddresses&$top=999",
    )?);
    for _ in 0..PAGE_LIMIT {
        let Some(url) = next.take() else { break };
        let (page, link) = graph_page(&fetch(&http, url, token, "Microsoft contacts").await?)?;
        contacts.extend(page);
        if contacts.len() >= SOURCE_LIMIT {
            break;
        }
        // The token goes only to Graph itself.
        next = link
            .map(|link| {
                Url::parse(&link).ok().filter(|url| {
                    url.scheme() == "https" && url.host_str() == Some("graph.microsoft.com")
                })
            })
            .map(|url| url.context(crate::text!("mail.contacts.microsoft_unexpected_link")))
            .transpose()?;
    }
    Ok(contacts)
}

/// Signs in to a DAV source with its stored password.
pub async fn dav_session(source: &DavSource) -> Result<Session> {
    let secret = vault::load(&format!("dav:{}", source.id)).await?;
    let password = secret
        .password
        .context(crate::text!("mail.dav.password_missing"))?;
    Session::new(&source.username, &password)
}
/// The contacts with an address in `collections` (address books of one CardDAV server).
pub async fn read_carddav(
    session: &Session,
    collections: &[Collection],
) -> Result<Vec<ImportedContact>> {
    // RFC 6352 queries need a filter; this one asks only for cards that have an address.
    let query = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><card:addressbook-query {}><d:prop><card:address-data/></d:prop><card:filter><card:prop-filter name="EMAIL"/></card:filter></card:addressbook-query>"#,
        dav::NAMESPACES
    );
    let mut contacts = Vec::new();
    for collection in collections {
        if contacts.len() >= SOURCE_LIMIT {
            break;
        }
        for data in session
            .report(&collection.url, &query, dav::CARDDAV, "address-data")
            .await?
        {
            contacts.extend(parse_vcards(&data));
        }
    }
    Ok(tidy(contacts))
}
/// Reads a CardDAV source's address books again; see [`store_carddav`].
pub async fn refresh_carddav(store: &Store, source: &DavSource) -> Result<usize> {
    let contacts = async {
        let session = dav_session(source).await?;
        let collections = session
            .discover(DavKind::Carddav, &dav::server_url(&source.url)?)
            .await?;
        read_carddav(&session, &collections).await
    }
    .await;
    store_carddav(store, source, contacts)
}
/// Stores the contacts read from a CardDAV source, replacing its previous ones, and records the
/// outcome on the source: `last_synced` when they were stored, `error` otherwise (the previous
/// contacts then stay). Returns how many are stored.
pub fn store_carddav(
    store: &Store,
    source: &DavSource,
    contacts: Result<Vec<ImportedContact>>,
) -> Result<usize> {
    let stored = contacts
        .and_then(|contacts| store.replace_contacts(ContactSource::Carddav, &source.id, &contacts));
    let mut source = source.clone();
    match &stored {
        Ok(_) => {
            source.last_synced = Some(chrono::Utc::now().to_rfc3339());
            source.error = None;
        }
        Err(e) => source.error = Some(e.to_string()),
    }
    store.update_dav_source(&source)?;
    stored
}

/// A vCard being read.
#[derive(Default)]
struct Card {
    full_name: String,
    structured_name: String,
    emails: Vec<String>,
}
impl Card {
    fn finish(&mut self, contacts: &mut Vec<ImportedContact>) {
        let card = std::mem::take(self);
        let name = if card.full_name.trim().is_empty() {
            card.structured_name
        } else {
            card.full_name
        };
        contacts.extend(card.emails.into_iter().map(|email| ImportedContact {
            name: name.trim().to_string(),
            email,
        }));
    }
}
/// The people in vCard text, which may hold several cards: one contact per EMAIL, named by FN or
/// else by N. Folded lines are joined; groups (`item1.EMAIL`) and parameters are ignored.
pub fn parse_vcards(text: &str) -> Vec<ImportedContact> {
    let unfolded = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace("\n ", "")
        .replace("\n\t", "");
    let mut contacts = Vec::new();
    let mut card = Card::default();
    for line in unfolded.lines() {
        let Some((head, value)) = split_property(line) else {
            continue;
        };
        let property = head.split(';').next().unwrap_or("");
        let property = property
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        match property.as_str() {
            "BEGIN" if value.trim().eq_ignore_ascii_case("VCARD") => card = Card::default(),
            "END" if value.trim().eq_ignore_ascii_case("VCARD") => card.finish(&mut contacts),
            "FN" => card.full_name = unescape(value),
            "N" => {
                // Family; given; additional; prefixes; suffixes.
                let parts: Vec<String> = split_unescaped(value, ';')
                    .into_iter()
                    .map(|part| unescape(&part))
                    .collect();
                let part = |i: usize| parts.get(i).map(|p| p.trim()).unwrap_or("");
                card.structured_name = [part(1), part(2), part(0)]
                    .into_iter()
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
            }
            "EMAIL" => card.emails.push(value.trim().to_string()),
            _ => {}
        }
    }
    // A card missing its END line.
    card.finish(&mut contacts);
    contacts
}
/// A content line's name and parameters, and its value: split at the first colon outside quotes.
fn split_property(line: &str) -> Option<(&str, &str)> {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => return Some((&line[..i], &line[i + 1..])),
            _ => {}
        }
    }
    None
}
fn split_unescaped(value: &str, separator: char) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut escaped = false;
    for c in value.chars() {
        if escaped {
            parts.last_mut().unwrap().extend(['\\', c]);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == separator {
            parts.push(String::new());
        } else {
            parts.last_mut().unwrap().push(c);
        }
    }
    parts
}
/// A text value with vCard escapes resolved; line breaks become spaces.
fn unescape(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => text.push(' '),
            Some(other) => text.push(other),
            None => {}
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{FIXTURE_ACCOUNT_ID, SYSTEM_CONTACTS};
    use chrono::{TimeDelta, Utc};

    fn suggest(directory: &Directory, store: &Store, q: &str, limit: usize) -> Result<Vec<String>> {
        let query = ContactQuery {
            q: Some(q.into()),
            account_id: None,
            limit: Some(limit),
        };
        Ok(directory
            .suggest(store, &query)?
            .into_iter()
            .map(|c| c.email)
            .collect())
    }

    #[test]
    fn people_you_write_to_outrank_people_who_only_write_to_you() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = Store::open(&dir.path().join("mail.db"))?;
        let account = store.add_fixture()?;
        let me = account.email.as_str();
        let template = store.message("sample-1")?;
        let mut n: i64 = 0;
        let mut mail = |folder: &str, from: &str, to: &[&str], days_ago: i64| -> Result<()> {
            n += 1;
            store.save_message(&Message {
                id: format!("test-{n}"),
                folder: folder.into(),
                from_name: String::new(),
                from_email: from.into(),
                to: to.iter().map(|s| s.to_string()).collect(),
                // Distinct times, so no two messages look like copies of one.
                date: (Utc::now() - TimeDelta::days(days_ago) - TimeDelta::minutes(n)).to_rfc3339(),
                message_id: Some(format!("<test-{n}@example.com>")),
                ..template.clone()
            })
        };
        // Eight recent messages in, against one message out.
        for _ in 0..8 {
            mail("inbox", "busy@example.com", &[me], 1)?;
        }
        mail("sent", me, &["friend@example.com", me], 2)?;
        // Mail you sent long ago counts for little now.
        mail("sent", me, &["old@example.com"], 730)?;
        // A copy you sent from another folder is still yours.
        mail("archive", me, &["colleague@example.com"], 3)?;
        mail("inbox", "noreply@service.example", &[me], 1)?;
        mail("inbox", "notifications@github.com", &[me], 1)?;
        mail("inbox", "MAILER-DAEMON@mx.example.com", &[me], 1)?;
        // An automated-looking address you wrote to is a real correspondent.
        mail("sent", me, &["no-reply@bank.example"], 5)?;
        // Spam must not introduce correspondents, including a spoofed copy of mail from yourself.
        mail("spam", "unwanted@example.com", &[me], 1)?;
        mail("spam", me, &["spam-recipient@example.com"], 1)?;
        let people = suggest(&Directory::default(), &store, "", 50)?;
        let position = |email: &str| people.iter().position(|p| p == email);
        for kept in [
            "friend@example.com",
            "colleague@example.com",
            "busy@example.com",
            "old@example.com",
            "no-reply@bank.example",
        ] {
            assert!(position(kept).is_some(), "{kept}");
        }
        assert_eq!(people[0], "friend@example.com");
        assert!(position("friend@example.com") < position("busy@example.com"));
        assert!(position("colleague@example.com") < position("busy@example.com"));
        assert!(position("busy@example.com") < position("old@example.com"));
        assert!(position("no-reply@bank.example").is_some());
        for excluded in [
            "noreply@service.example",
            "notifications@github.com",
            "mailer-daemon@mx.example.com",
            "unwanted@example.com",
            "spam-recipient@example.com",
            me,
        ] {
            assert_eq!(position(excluded), None, "{excluded}");
        }
        // Received mail names its sender.
        let sophie = Directory::default().suggest(
            &store,
            &ContactQuery {
                q: Some("sophie".into()),
                ..Default::default()
            },
        )?;
        assert_eq!(
            (sophie[0].name.as_str(), sophie[0].sources.as_slice()),
            ("Sophie Chen", [ContactSource::Mail].as_slice())
        );
        Ok(())
    }

    #[test]
    fn matches_name_word_and_address_prefixes_ignoring_case_and_accents() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = Store::open(&dir.path().join("mail.db"))?;
        store.add_fixture()?;
        let imported = tidy([
            ImportedContact {
                name: "Zoë  Ångström".into(),
                email: "Zoe.Angstrom@Example.org".into(),
            },
            ImportedContact {
                name: "Studio Sophie".into(),
                email: "mailto:sophie@form.studio".into(),
            },
            ImportedContact {
                name: "Broken".into(),
                email: "not an address".into(),
            },
        ]);
        assert_eq!(imported.len(), 2);
        store.replace_contacts(ContactSource::System, SYSTEM_CONTACTS, &imported)?;
        let directory = Directory::default();
        // The fixture has people at example.com too.
        for q in [
            "zoe",
            "ZOË",
            "ång",
            "angstrom z",
            "zoë ång",
            "zoe.ang",
            "example.o",
        ] {
            assert_eq!(
                suggest(&directory, &store, q, 8)?,
                ["zoe.angstrom@example.org"],
                "{q}"
            );
        }
        for q in ["oe", "strom", "zoe@other", "org"] {
            assert!(suggest(&directory, &store, q, 8)?.is_empty(), "{q}");
        }
        // The address book's name wins over the one in mail, and both sources are listed.
        let sophie = directory.suggest(
            &store,
            &ContactQuery {
                q: Some("so".into()),
                ..Default::default()
            },
        )?;
        let sophie = sophie
            .iter()
            .find(|c| c.email == "sophie@form.studio")
            .context("Sophie is suggested")?;
        assert_eq!(sophie.name, "Studio Sophie");
        assert_eq!(sophie.sources, [ContactSource::Mail, ContactSource::System]);
        // Domain-only matches come after name matches, and the limit is clamped.
        store.replace_contacts(
            ContactSource::System,
            SYSTEM_CONTACTS,
            &tidy([
                ImportedContact {
                    name: "Ann".into(),
                    email: "ann@zoetrope.example".into(),
                },
                imported[0].clone(),
            ]),
        )?;
        directory.invalidate();
        assert_eq!(
            suggest(&directory, &store, "zoe", 8)?,
            ["zoe.angstrom@example.org", "ann@zoetrope.example"]
        );
        assert_eq!(suggest(&directory, &store, "zoe", 0)?.len(), 1);
        // The sending account's people come first.
        let mut account = store.account(FIXTURE_ACCOUNT_ID)?;
        account.id = "work".into();
        account.email = "me@work.example".into();
        account.integrations.contacts = true;
        store.save_account(&account)?;
        store.replace_contacts(
            ContactSource::Google,
            "work",
            &tidy([ImportedContact {
                name: "Zed".into(),
                email: "zed@work.example".into(),
            }]),
        )?;
        directory.invalidate();
        let query = ContactQuery {
            q: Some("z".into()),
            account_id: Some("work".into()),
            limit: None,
        };
        assert_eq!(
            directory.suggest(&store, &query)?[0].email,
            "zed@work.example"
        );
        Ok(())
    }

    #[test]
    fn provider_contacts_are_kept_only_while_the_account_shares_them() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = Store::open(&dir.path().join("mail.db"))?;
        let mut account = store.add_fixture()?;
        let contacts = tidy([ImportedContact {
            name: "Zed".into(),
            email: "zed@example.com".into(),
        }]);
        assert_eq!(
            store.replace_contacts(ContactSource::Google, &account.id, &contacts)?,
            0,
            "access was not granted"
        );
        account.integrations.contacts = true;
        store.save_account(&account)?;
        assert_eq!(
            store.replace_contacts(ContactSource::Google, &account.id, &contacts)?,
            1
        );
        assert_eq!(
            store.replace_contacts(ContactSource::Carddav, "removed-server", &contacts)?,
            0
        );
        assert_eq!(store.synced_contact_count()?, 1);
        assert_eq!(store.remove_contacts(&account.id)?, 1);
        Ok(())
    }

    #[test]
    fn reads_folded_grouped_and_repeated_vcard_emails() {
        let cards = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Zoë Ång\r\n ström\r\nN:Ångström;Zoë;;;\r\nitem1.EMAIL;type=INTERNET;type=pref:zoe@example.org\r\nitem1.X-ABLabel:_$!<Work>!$_\r\nEMAIL;TYPE=\"home,pref\":zoe@home.example\r\nEND:VCARD\r\n\
            BEGIN:VCARD\nVERSION:4.0\nN:Doe\\, Jr.;Jane;;;\nEMAIL;PREF=1:mailto:jane@example.com\nEND:VCARD\n\
            BEGIN:VCARD\nVERSION:3.0\nFN:No Address\nTEL:+1 555 0100\nEND:VCARD\n";
        let contacts = parse_vcards(cards);
        let pairs: Vec<(&str, &str)> = contacts
            .iter()
            .map(|c| (c.name.as_str(), c.email.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("Zoë Ångström", "zoe@example.org"),
                ("Zoë Ångström", "zoe@home.example"),
                ("Jane Doe, Jr.", "mailto:jane@example.com"),
            ]
        );
        assert_eq!(tidy(contacts)[2].email, "jane@example.com");
    }

    #[test]
    fn reads_google_people_pages() -> Result<()> {
        let (contacts, next) = people_page(
            r#"{"connections":[
                {"resourceName":"people/c1","names":[{"displayName":"","metadata":{}},{"displayName":"Ana Ruiz"}],"emailAddresses":[{"value":"ana@example.com"},{"value":"ana.ruiz@work.example","type":"work"}]},
                {"resourceName":"people/c2","emailAddresses":[{"value":"nameless@example.com"}]},
                {"resourceName":"people/c3","names":[{"displayName":"Phone Only"}]}
            ],"nextPageToken":"page-2","totalPeople":3}"#,
        )?;
        let pairs: Vec<(&str, &str)> = contacts
            .iter()
            .map(|c| (c.name.as_str(), c.email.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("Ana Ruiz", "ana@example.com"),
                ("Ana Ruiz", "ana.ruiz@work.example"),
                ("", "nameless@example.com")
            ]
        );
        assert_eq!(next.as_deref(), Some("page-2"));
        let (contacts, next) = people_page(
            r#"{"otherContacts":[{"names":[{"displayName":"Bo"}],"emailAddresses":[{"value":"bo@example.com"}]}]}"#,
        )?;
        assert_eq!(
            (contacts[0].name.as_str(), contacts[0].email.as_str(), next),
            ("Bo", "bo@example.com", None)
        );
        assert!(people_page("{}")?.0.is_empty());
        Ok(())
    }

    #[test]
    fn reads_microsoft_graph_pages() -> Result<()> {
        let (contacts, next) = graph_page(
            r#"{"@odata.context":"x","value":[
                {"id":"1","displayName":"Ana Ruiz","emailAddresses":[{"name":"ana@example.com","address":"ana@example.com"},{"name":"Ana","address":"ana@work.example"}]},
                {"id":"2","displayName":null,"emailAddresses":[{"name":"Bo Lind","address":"bo@example.com"},{"name":"cy@example.com","address":"cy@example.com"}]},
                {"id":"3","displayName":"No Address","emailAddresses":[]}
            ],"@odata.nextLink":"https://graph.microsoft.com/v1.0/me/contacts?$skip=999"}"#,
        )?;
        let pairs: Vec<(&str, &str)> = contacts
            .iter()
            .map(|c| (c.name.as_str(), c.email.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("Ana Ruiz", "ana@example.com"),
                ("Ana Ruiz", "ana@work.example"),
                ("Bo Lind", "bo@example.com"),
                ("", "cy@example.com")
            ]
        );
        assert_eq!(
            next.as_deref(),
            Some("https://graph.microsoft.com/v1.0/me/contacts?$skip=999")
        );
        assert_eq!(graph_page(r#"{"value":[]}"#)?.1, None);
        Ok(())
    }
}
