//! A small WebDAV client for CardDAV (RFC 6352) and CalDAV (RFC 4791): finding someone's address
//! books or calendars from a server URL, reading their items with a `REPORT`, and storing one.
//!
//! Requests use HTTP Basic authentication, so every URL must be HTTPS (plain HTTP only to this
//! device). Redirects are followed by hand, keeping the method and body, and checked the same way.

use crate::{client::http_client, model::DavKind};
use anyhow::{Context, Result, bail};
use quick_xml::{NsReader, escape::resolve_predefined_entity, events::Event, name::ResolveResult};
use reqwest::{Method, StatusCode, Url, header};

pub const DAV: &str = "DAV:";
pub const CARDDAV: &str = "urn:ietf:params:xml:ns:carddav";
pub const CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
/// Namespace prefixes for request bodies: `d:` DAV, `card:` CardDAV, `cal:` CalDAV.
pub const NAMESPACES: &str = r#"xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav" xmlns:cal="urn:ietf:params:xml:ns:caldav""#;
/// Responses larger than this are refused.
const RESPONSE_LIMIT: usize = 64 * 1024 * 1024;
/// An XML element with its resolved namespace, attributes (by local name), the text directly
/// inside it, and its child elements.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    pub ns: String,
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Element>,
}
impl Element {
    /// Parses a whole document and returns its root element.
    pub fn parse(xml: &str) -> Result<Element> {
        let mut reader = NsReader::from_str(xml);
        let mut stack: Vec<Element> = vec![Element::default()];
        loop {
            let (ns, event) = reader
                .read_resolved_event()
                .context(crate::text!("mail.dav.invalid_xml"))?;
            let namespace = match ns {
                ResolveResult::Bound(ns) => ns.as_ref().to_string(),
                _ => String::new(),
            };
            match event {
                Event::Start(_) | Event::Empty(_) if stack.len() > 64 => {
                    bail!(crate::text!("mail.dav.xml_depth"));
                }
                Event::Start(start) => stack.push(Element {
                    ns: namespace,
                    name: start.local_name().as_ref().to_string(),
                    attributes: attributes(&start),
                    ..Default::default()
                }),
                Event::Empty(start) => {
                    let element = Element {
                        ns: namespace,
                        name: start.local_name().as_ref().to_string(),
                        attributes: attributes(&start),
                        ..Default::default()
                    };
                    stack.last_mut().unwrap().children.push(element);
                }
                Event::End(_) => {
                    let element = stack.pop().unwrap();
                    stack
                        .last_mut()
                        .context(crate::text!("mail.dav.invalid_xml"))?
                        .children
                        .push(element);
                }
                Event::Text(text) => stack
                    .last_mut()
                    .unwrap()
                    .text
                    .push_str(&text.xml10_content()),
                Event::CData(data) => stack.last_mut().unwrap().text.push_str(data.as_ref()),
                Event::GeneralRef(reference) => {
                    let resolved = match reference.resolve_char_ref() {
                        Ok(Some(c)) => c.to_string(),
                        _ => resolve_predefined_entity(&reference.xml10_content())
                            .unwrap_or_default()
                            .to_string(),
                    };
                    stack.last_mut().unwrap().text.push_str(&resolved);
                }
                Event::Eof => break,
                _ => {}
            }
        }
        let document = stack
            .pop()
            .filter(|_| stack.is_empty())
            .context(crate::text!("mail.dav.invalid_xml"))?;
        document
            .children
            .into_iter()
            .next()
            .context(crate::text!("mail.dav.empty_xml"))
    }
    pub fn is(&self, ns: &str, name: &str) -> bool {
        self.ns == ns && self.name.eq_ignore_ascii_case(name)
    }
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
    pub fn child(&self, ns: &str, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.is(ns, name))
    }
    pub fn children_named<'a>(
        &'a self,
        ns: &'a str,
        name: &'a str,
    ) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |c| c.is(ns, name))
    }
}

/// One `response` of a `207 Multi-Status`: the resource and its properties that were found.
#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub url: Url,
    /// The `prop` elements of every successful `propstat`, merged.
    pub props: Element,
}
impl Resource {
    pub fn prop(&self, ns: &str, name: &str) -> Option<&Element> {
        self.props.child(ns, name)
    }
    /// Whether `resourcetype` includes `ns:name`.
    pub fn has_type(&self, ns: &str, name: &str) -> bool {
        self.prop(DAV, "resourcetype")
            .is_some_and(|t| t.child(ns, name).is_some())
    }
}
/// Reads a `multistatus` body. Relative hrefs are resolved against `base`.
pub fn multistatus(base: &Url, xml: &str) -> Result<Vec<Resource>> {
    let root = Element::parse(xml)?;
    if !root.is(DAV, "multistatus") {
        bail!(crate::text!("mail.dav.not_multistatus"));
    }
    let mut resources = Vec::new();
    for response in root.children_named(DAV, "response") {
        let Some(href) = response.child(DAV, "href") else {
            continue;
        };
        let Ok(url) = base.join(href.text.trim()) else {
            continue;
        };
        let mut props = Element {
            ns: DAV.into(),
            name: "prop".into(),
            ..Default::default()
        };
        for propstat in response.children_named(DAV, "propstat") {
            let ok = propstat
                .child(DAV, "status")
                .is_none_or(|s| s.text.split_whitespace().nth(1) == Some("200"));
            if let (true, Some(prop)) = (ok, propstat.child(DAV, "prop")) {
                props.children.extend(prop.children.iter().cloned());
            }
        }
        resources.push(Resource { url, props });
    }
    Ok(resources)
}

/// An address book or calendar.
#[derive(Clone, Debug, PartialEq)]
pub struct Collection {
    pub url: Url,
    pub name: String,
}

/// Accepts HTTPS URLs, and HTTP only to this device. Credentials, queries and fragments are refused.
pub fn check_url(url: &Url) -> Result<()> {
    let local = url
        .host_str()
        .is_some_and(|h| ["localhost", "127.0.0.1", "[::1]"].contains(&h));
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        bail!(crate::text!("mail.dav.https_required"));
    }
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        bail!(crate::text!("mail.dav.url_credentials"));
    }
    Ok(())
}
/// Parses a server URL someone typed, adding `https://` when no scheme is given.
pub fn server_url(input: &str) -> Result<Url> {
    let input = input.trim();
    let url = if input.contains("://") {
        Url::parse(input)
    } else {
        Url::parse(&format!("https://{input}"))
    }
    .context(crate::text!("mail.dav.invalid_url"))?;
    check_url(&url)?;
    Ok(url)
}

/// A signed-in connection to one DAV server.
pub struct Session {
    http: reqwest::Client,
    username: String,
    password: String,
}
impl Session {
    pub fn new(username: &str, password: &str) -> Result<Self> {
        Ok(Self {
            http: http_client()?,
            username: username.into(),
            password: password.into(),
        })
    }
    /// Sends a request, following up to five redirects with the same method and body. Returns the
    /// final URL, status and body.
    pub async fn request(
        &self,
        method: &str,
        url: &Url,
        depth: Option<&str>,
        content_type: Option<&str>,
        body: Option<&str>,
        extra: &[(&str, &str)],
    ) -> Result<(Url, StatusCode, String)> {
        let method = Method::from_bytes(method.as_bytes())?;
        let mut url = url.clone();
        for _ in 0..6 {
            check_url(&url)?;
            let mut request = self
                .http
                .request(method.clone(), url.clone())
                .basic_auth(&self.username, Some(&self.password));
            if let Some(depth) = depth {
                request = request.header("Depth", depth);
            }
            if let Some(content_type) = content_type {
                request = request.header(header::CONTENT_TYPE, content_type);
            }
            for (name, value) in extra {
                request = request.header(*name, *value);
            }
            if let Some(body) = body {
                request = request.body(body.to_string());
            }
            let response = request.send().await.with_context(|| match url.host_str() {
                Some(host) => crate::text!("mail.service.unreachable", service = host),
                None => crate::text!("mail.dav.unreachable"),
            })?;
            let status = response.status();
            if status.is_redirection() && status != StatusCode::NOT_MODIFIED {
                let location = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .context(crate::text!("mail.dav.redirect_missing"))?;
                url = url
                    .join(location)
                    .context(crate::text!("mail.dav.redirect_invalid"))?;
                continue;
            }
            if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                bail!(crate::text!("mail.dav.credentials_rejected"));
            }
            if response
                .content_length()
                .is_some_and(|n| n as usize > RESPONSE_LIMIT)
            {
                bail!(crate::text!("mail.dav.response_too_large"));
            }
            let bytes = response.bytes().await?;
            if bytes.len() > RESPONSE_LIMIT {
                bail!(crate::text!("mail.dav.response_too_large"));
            }
            return Ok((url, status, String::from_utf8_lossy(&bytes).into_owned()));
        }
        bail!(crate::text!("mail.dav.redirect_limit"))
    }
    /// `PROPFIND` for the properties in `props` (inner XML of `d:prop`, using [`NAMESPACES`] prefixes).
    pub async fn propfind(&self, url: &Url, depth: &str, props: &str) -> Result<Vec<Resource>> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><d:propfind {NAMESPACES}><d:prop>{props}</d:prop></d:propfind>"#
        );
        let (url, status, text) = self
            .request(
                "PROPFIND",
                url,
                Some(depth),
                Some("application/xml; charset=utf-8"),
                Some(&body),
                &[],
            )
            .await?;
        if status != StatusCode::MULTI_STATUS {
            bail!(crate::text!(
                "mail.dav.properties_http",
                status = status.as_u16()
            ));
        }
        multistatus(&url, &text)
    }
    /// A `REPORT` whose body is `query` (a complete XML document). Returns nonempty
    /// `data_prop` payloads (`card:address-data` or `cal:calendar-data`).
    pub async fn report(
        &self,
        url: &Url,
        query: &str,
        data_ns: &str,
        data_prop: &str,
    ) -> Result<Vec<String>> {
        let (url, status, text) = self
            .request(
                "REPORT",
                url,
                Some("1"),
                Some("application/xml; charset=utf-8"),
                Some(query),
                &[],
            )
            .await?;
        if status != StatusCode::MULTI_STATUS {
            let error = if data_ns == CARDDAV {
                crate::text!("mail.dav.contacts_http", status = status.as_u16())
            } else {
                crate::text!("mail.dav.calendar_http", status = status.as_u16())
            };
            bail!(error);
        }
        Ok(multistatus(&url, &text)?
            .into_iter()
            .filter_map(|resource| {
                let data = resource
                    .props
                    .children
                    .into_iter()
                    .find(|prop| prop.is(data_ns, data_prop))?
                    .text;
                (!data.trim().is_empty()).then_some(data)
            })
            .collect())
    }
    /// Creates a new resource at `url`, refusing to replace one that exists.
    pub async fn create(&self, url: &Url, content_type: &str, body: &str) -> Result<()> {
        let (_, status, _) = self
            .request(
                "PUT",
                url,
                None,
                Some(content_type),
                Some(body),
                &[("If-None-Match", "*")],
            )
            .await?;
        if status == StatusCode::PRECONDITION_FAILED {
            bail!(crate::text!("mail.dav.event_exists"));
        }
        if !status.is_success() {
            bail!(crate::text!("mail.dav.save_http", status = status.as_u16()));
        }
        Ok(())
    }
    /// The address books (`DavKind::Carddav`) or event calendars (`DavKind::Caldav`) reachable from
    /// `url`: a collection itself, an account's home, its principal, or the server's root.
    pub async fn discover(&self, kind: DavKind, url: &Url) -> Result<Vec<Collection>> {
        let (ns, home_set, collection_type, well_known) = match kind {
            DavKind::Carddav => (
                CARDDAV,
                "addressbook-home-set",
                "addressbook",
                "/.well-known/carddav",
            ),
            DavKind::Caldav => (
                CALDAV,
                "calendar-home-set",
                "calendar",
                "/.well-known/caldav",
            ),
        };
        let probe = format!(
            "<d:resourcetype/><d:displayname/><d:current-user-principal/><{p}:{home_set}/>",
            p = if kind == DavKind::Carddav {
                "card"
            } else {
                "cal"
            }
        );
        // A bare server URL starts from its well-known location, when it has one.
        let mut start = url.clone();
        let mut found = None;
        if matches!(url.path(), "" | "/") {
            let known = url.join(well_known)?;
            if let Ok(resources) = self.propfind(&known, "0", &probe).await {
                found = resources.into_iter().next();
                start = known;
            }
        }
        let resource = match found {
            Some(resource) => resource,
            None => self
                .propfind(&start, "0", &probe)
                .await?
                .into_iter()
                .next()
                .context("The server returned no properties")?,
        };
        if resource.has_type(ns, collection_type) {
            return Ok(vec![collection(&resource)]);
        }
        let home = match resource
            .prop(ns, home_set)
            .and_then(|h| h.child(DAV, "href"))
        {
            Some(href) => resource.url.join(href.text.trim())?,
            None => {
                let principal = resource
                    .prop(DAV, "current-user-principal")
                    .and_then(|p| p.child(DAV, "href"))
                    .map(|href| resource.url.join(href.text.trim()))
                    .transpose()?
                    .unwrap_or_else(|| resource.url.clone());
                let props = format!(
                    "<{p}:{home_set}/>",
                    p = if kind == DavKind::Carddav {
                        "card"
                    } else {
                        "cal"
                    }
                );
                let principal = self
                    .propfind(&principal, "0", &props)
                    .await?
                    .into_iter()
                    .next();
                match principal
                    .as_ref()
                    .and_then(|p| p.prop(ns, home_set))
                    .and_then(|h| h.child(DAV, "href"))
                {
                    Some(href) => principal.as_ref().unwrap().url.join(href.text.trim())?,
                    None => resource.url.clone(),
                }
            }
        };
        let listing = self
            .propfind(
                &home,
                "1",
                "<d:resourcetype/><d:displayname/><cal:supported-calendar-component-set/>",
            )
            .await?;
        let collections: Vec<Collection> = listing
            .iter()
            .filter(|r| r.has_type(ns, collection_type))
            .filter(|r| {
                // Calendars that hold only tasks or journals cannot take events.
                kind == DavKind::Carddav
                    || r.prop(CALDAV, "supported-calendar-component-set")
                        .is_none_or(|set| {
                            set.children.is_empty()
                                || set.children.iter().any(|comp| {
                                    comp.is(CALDAV, "comp")
                                        && comp
                                            .attribute("name")
                                            .is_some_and(|n| n.eq_ignore_ascii_case("VEVENT"))
                                })
                        })
            })
            .map(collection)
            .collect();
        if collections.is_empty() {
            bail!(
                "No {} were found at this server",
                if kind == DavKind::Carddav {
                    "address books"
                } else {
                    "calendars"
                }
            );
        }
        Ok(collections)
    }
}
fn collection(resource: &Resource) -> Collection {
    let name = resource
        .prop(DAV, "displayname")
        .map(|n| n.text.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            resource
                .url
                .path_segments()
                .and_then(|mut s| s.rfind(|s| !s.is_empty()))
                .unwrap_or("Default")
                .to_string()
        });
    Collection {
        url: resource.url.clone(),
        name,
    }
}
/// An element's attributes by local name, unescaped.
fn attributes(start: &quick_xml::events::BytesStart<'_>) -> Vec<(String, String)> {
    start
        .attributes()
        .flatten()
        .map(|attribute| {
            (
                attribute.key.local_name().as_ref().to_string(),
                attribute
                    .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .map(|v| v.into_owned())
                    .unwrap_or_default(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multistatus_keeps_successful_props_and_resolves_hrefs() -> Result<()> {
        let base = Url::parse("https://dav.example.test/home/")?;
        let xml = r#"<?xml version="1.0"?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:carddav">
 <D:response><D:href>/home/contacts/</D:href>
  <D:propstat><D:prop><D:resourcetype><D:collection/><C:addressbook/></D:resourcetype><D:displayname>Friends &amp; family</D:displayname></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
  <D:propstat><D:prop><C:supported-address-data/></D:prop><D:status>HTTP/1.1 404 Not Found</D:status></D:propstat>
 </D:response>
 <D:response><D:href>card.vcf</D:href><D:propstat><D:prop><D:getetag>"1"</D:getetag><C:address-data><![CDATA[BEGIN:VCARD]]></C:address-data></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
</D:multistatus>"#;
        let resources = multistatus(&base, xml)?;
        assert_eq!(
            resources[0].url.as_str(),
            "https://dav.example.test/home/contacts/"
        );
        assert!(resources[0].has_type(CARDDAV, "addressbook"));
        assert_eq!(collection(&resources[0]).name, "Friends & family");
        assert!(
            resources[0]
                .prop(CARDDAV, "supported-address-data")
                .is_none()
        );
        assert_eq!(
            resources[1].url.as_str(),
            "https://dav.example.test/home/card.vcf"
        );
        assert_eq!(
            resources[1].prop(CARDDAV, "address-data").unwrap().text,
            "BEGIN:VCARD"
        );
        Ok(())
    }
    #[test]
    fn urls_need_https_except_on_this_device() {
        assert!(server_url("contacts.icloud.com").is_ok_and(|u| u.scheme() == "https"));
        assert!(server_url("http://dav.example.test").is_err());
        assert!(server_url("http://127.0.0.1:5232/").is_ok());
        assert!(server_url("https://me:pw@dav.example.test").is_err());
    }
}
