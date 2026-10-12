//! The languages Alice speaks, and the text its Rust surfaces (daemon, CLI, TUI, desktop shell) show.
//!
//! Text lives in JSON catalogs, `crates/alice-core/locales/<tag>/<namespace>.json`. A key is the
//! namespace and the path to a string, joined by dots: `common.folder.inbox`. A string may contain
//! `{name}` placeholders. An object whose keys are CLDR plural categories (`one`, `other`, …) is a
//! plural message, chosen by the integer `count` argument. `scripts/check-i18n.mjs` checks that every
//! language has every key with the same placeholders.
//!
//! To add a language: add a [`Locale`] variant (its tag, names, plural rule and chrono locale), add
//! its catalogs to [`files`], and translate every namespace.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::{borrow::Cow, collections::HashMap, fmt, sync::OnceLock};

/// A language Alice is translated into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Locale {
    /// English (United States), the source language: every key exists in it.
    #[default]
    EnUs,
    De,
}
impl Locale {
    pub const ALL: [Self; 2] = [Self::EnUs, Self::De];
    /// The BCP 47 tag: what the API, the preference and the catalogs use.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::EnUs => "en-US",
            Self::De => "de",
        }
    }
    /// The language's own name, as a language picker shows it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::EnUs => "English (US)",
            Self::De => "Deutsch",
        }
    }
    /// The language's name in English, for instructions to an AI model.
    pub const fn english_name(self) -> &'static str {
        match self {
            Self::EnUs => "English",
            Self::De => "German",
        }
    }
    /// Month and weekday names for [`date`].
    pub const fn chrono(self) -> chrono::Locale {
        match self {
            Self::EnUs => chrono::Locale::en_US,
            Self::De => chrono::Locale::de_DE,
        }
    }
    /// The CLDR plural category of an integer.
    pub const fn plural(self, n: i64) -> &'static str {
        match self {
            Self::EnUs | Self::De => {
                if n == 1 {
                    "one"
                } else {
                    "other"
                }
            }
        }
    }
    /// The locale with exactly this tag, ignoring case.
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|l| l.tag().eq_ignore_ascii_case(tag))
    }
    /// The supported language of a BCP 47 tag or POSIX locale name (`de-AT`, `de_DE.UTF-8`, `en`).
    /// Every English variant gets US English.
    pub fn matching(name: &str) -> Option<Self> {
        let language = name.split(['-', '_', '.', '@']).next().unwrap_or_default();
        if language.eq_ignore_ascii_case("en") {
            Some(Self::EnUs)
        } else if language.eq_ignore_ascii_case("de") {
            Some(Self::De)
        } else {
            None
        }
    }
    /// The first supported language of a preference-ordered list.
    pub fn first_matching<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        names.into_iter().find_map(Self::matching)
    }
    /// The operating system's preferred language, or US English when it has none Alice speaks.
    pub fn system() -> Self {
        sys_locale::get_locales()
            .find_map(|name| Self::matching(&name))
            .unwrap_or_default()
    }
    /// The terminal's language, the way gettext finds it: `LANGUAGE` (a `:`-separated list), then
    /// `LC_ALL`, `LC_MESSAGES` and `LANG`. The `C` and `POSIX` locales are English. Without any of
    /// these (Windows, apps started outside a shell), the system's language.
    pub fn from_env() -> Self {
        Self::from_vars(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
            .unwrap_or_else(Self::system)
    }
    fn from_vars(var: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let locale = var("LC_ALL")
            .or_else(|| var("LC_MESSAGES"))
            .or_else(|| var("LANG"));
        let posix = locale.as_deref().is_some_and(|l| {
            let base = l.split('.').next().unwrap_or_default();
            base == "C" || base == "POSIX"
        });
        if posix {
            return Some(Self::EnUs);
        }
        let listed = var("LANGUAGE").and_then(|list| Self::first_matching(list.split(':')));
        listed.or_else(|| locale.map(|l| Self::matching(&l).unwrap_or_default()))
    }
    /// The best supported language of an `Accept-Language` header, by quality then order.
    pub fn from_accept_language(header: &str) -> Option<Self> {
        let mut best = None;
        let mut best_quality = 0.0;
        for range in header.split(',') {
            let mut parts = range.split(';');
            let tag = parts.next().unwrap_or_default().trim();
            let quality = parts
                .find_map(|p| p.trim().strip_prefix("q="))
                .map_or(Some(1.0), |q| q.trim().parse::<f32>().ok());
            let Some(quality) = quality.filter(|q| *q > 0.0 && *q <= 1.0) else {
                continue;
            };
            if quality > best_quality
                && let Some(locale) = Self::matching(tag)
            {
                best = Some(locale);
                best_quality = quality;
            }
        }
        best
    }
}
impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}
impl Serialize for Locale {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.tag())
    }
}
impl<'de> Deserialize<'de> for Locale {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let tag = Cow::<str>::deserialize(deserializer)?;
        Self::from_tag(&tag)
            .ok_or_else(|| serde::de::Error::custom(format!("unsupported language `{tag}`")))
    }
}

/// The language preference: one language, or the system's. Stored as `"system"` or a tag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Language(pub Option<Locale>);
impl Language {
    /// The language to use, with the system's resolved by [`Locale::system`].
    pub fn resolve(self) -> Locale {
        self.0.unwrap_or_else(Locale::system)
    }
}
impl Serialize for Language {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.map_or("system", Locale::tag))
    }
}
impl<'de> Deserialize<'de> for Language {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let tag = Cow::<str>::deserialize(deserializer)?;
        if tag == "system" {
            return Ok(Self(None));
        }
        Locale::from_tag(&tag)
            .map(|l| Self(Some(l)))
            .ok_or_else(|| serde::de::Error::custom(format!("unsupported language `{tag}`")))
    }
}

/// A value for a `{name}` placeholder. Integers are written without grouping, so ports and years
/// read right; the `count` argument also picks a plural message's form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arg<'a> {
    Str(Cow<'a, str>),
    Int(i64),
}
impl Arg<'_> {
    pub fn into_owned(self) -> Arg<'static> {
        match self {
            Self::Str(s) => Arg::Str(Cow::Owned(s.into_owned())),
            Self::Int(n) => Arg::Int(n),
        }
    }
}
impl<'a> From<&'a str> for Arg<'a> {
    fn from(value: &'a str) -> Self {
        Self::Str(Cow::Borrowed(value))
    }
}
impl<'a> From<&'a &str> for Arg<'a> {
    fn from(value: &'a &str) -> Self {
        Self::Str(Cow::Borrowed(value))
    }
}
impl<'a> From<&'a String> for Arg<'a> {
    fn from(value: &'a String) -> Self {
        Self::Str(Cow::Borrowed(value))
    }
}
impl From<String> for Arg<'_> {
    fn from(value: String) -> Self {
        Self::Str(Cow::Owned(value))
    }
}
impl<'a> From<Cow<'a, str>> for Arg<'a> {
    fn from(value: Cow<'a, str>) -> Self {
        Self::Str(value)
    }
}
macro_rules! int_args {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg<'_> {
            fn from(value: $t) -> Self {
                Self::Int(i64::try_from(value).unwrap_or(i64::MAX))
            }
        }
    )*};
}
int_args!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

/// Text in the language `$locale`: `t!(locale, "common.folder.inbox")`,
/// `t!(locale, "cli.messages_found", count = n)`. The key must be a string literal, so the catalog
/// check can find it.
#[macro_export]
macro_rules! t {
    ($locale:expr, $key:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::tr($locale, $key, &[$((stringify!($name), $crate::i18n::Arg::from($value))),*])
    };
}
/// A [`Text`](crate::i18n::Text) to show later in whichever language asks: for errors
/// (`bail!(text!("server.draft_only"))`) and other messages made before the language is known.
#[macro_export]
macro_rules! text {
    ($key:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::Text::new($key, vec![$((stringify!($name), $crate::i18n::Arg::from($value).into_owned())),*])
    };
}

/// Text that is shown later, in the language of whoever reads it. It displays in US English, the
/// language of logs; [`Text::render`] shows it in another. As an error, the daemon sends it in the
/// language the request asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    key: &'static str,
    args: Vec<(&'static str, Arg<'static>)>,
}
impl Text {
    pub fn new(key: &'static str, args: Vec<(&'static str, Arg<'static>)>) -> Self {
        Self { key, args }
    }
    pub fn render(&self, locale: Locale) -> String {
        tr(locale, self.key, &self.args)
    }
}
impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(Locale::EnUs))
    }
}
impl std::error::Error for Text {}

/// What an error tells people: Alice's own text, shown in the reader's language, or text from
/// elsewhere (a mail server, a library), shown as it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Text(Text),
    Plain(String),
}
impl Message {
    /// What `error` says: its outermost message, as `error.to_string()` shows it.
    pub fn of(error: &anyhow::Error) -> Self {
        let shown = error.to_string();
        match error.downcast_ref::<Text>() {
            // `downcast_ref` also finds a `Text` under outer context, which is not what is shown.
            Some(text) if text.to_string() == shown => Self::Text(text.clone()),
            _ => Self::Plain(shown),
        }
    }
    pub fn render(&self, locale: Locale) -> Cow<'_, str> {
        match self {
            Self::Text(text) => Cow::Owned(text.render(locale)),
            Self::Plain(text) => Cow::Borrowed(text),
        }
    }
}
impl From<Text> for Message {
    fn from(text: Text) -> Self {
        Self::Text(text)
    }
}
impl From<String> for Message {
    fn from(text: String) -> Self {
        Self::Plain(text)
    }
}
impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(Locale::EnUs))
    }
}

/// What `error` says, in `locale`: [`Message::of`] rendered.
pub fn error_text(error: &anyhow::Error, locale: Locale) -> String {
    Message::of(error).render(locale).into_owned()
}

/// `time` in `format` (chrono's `strftime` syntax, usually from the catalog) with `locale`'s month
/// and weekday names: `date(locale, &time, &t!(locale, "tui.date.day"))`.
pub fn date<Tz: chrono::TimeZone>(
    locale: Locale,
    time: &chrono::DateTime<Tz>,
    format: &str,
) -> String
where
    Tz::Offset: fmt::Display,
{
    time.format_localized(format, locale.chrono()).to_string()
}

/// `key` in `locale`, its placeholders filled from `args`. A key missing from `locale` falls back to
/// US English, and one missing there shows as the key itself. Use [`t!`](crate::t) unless the key is
/// built at run time.
pub fn tr(locale: Locale, key: &str, args: &[(&str, Arg<'_>)]) -> String {
    let Some((found, entry)) = lookup(locale, key) else {
        return key.to_owned();
    };
    let pattern = match entry {
        Entry::Text(text) => text.as_str(),
        Entry::Plural(forms) => {
            let count = args.iter().find_map(|(name, value)| match value {
                Arg::Int(n) if *name == "count" => Some(*n),
                _ => None,
            });
            let category = found.plural(count.unwrap_or(0));
            forms
                .iter()
                .find(|(c, _)| c == category)
                .or_else(|| forms.iter().find(|(c, _)| c == "other"))
                .map_or("", |(_, text)| text.as_str())
        }
    };
    interpolate(pattern, args)
}
/// Whether `key` exists in US English, for keys built at run time.
pub fn exists(key: &str) -> bool {
    catalog(Locale::EnUs).contains_key(key)
}
fn lookup(locale: Locale, key: &str) -> Option<(Locale, &'static Entry)> {
    catalog(locale)
        .get(key)
        .map(|entry| (locale, entry))
        .or_else(|| catalog(Locale::EnUs).get(key).map(|e| (Locale::EnUs, e)))
}
fn interpolate(pattern: &str, args: &[(&str, Arg<'_>)]) -> String {
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut rest = pattern;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let name_len = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        let value = (name_len > 0 && after[name_len..].starts_with('}'))
            .then(|| args.iter().find(|(n, _)| *n == &after[..name_len]))
            .flatten();
        match value {
            Some((_, Arg::Str(s))) => out.push_str(s),
            Some((_, Arg::Int(n))) => out.push_str(&n.to_string()),
            None => {
                out.push('{');
                rest = after;
                continue;
            }
        }
        rest = &after[name_len + 1..];
    }
    out.push_str(rest);
    out
}

enum Entry {
    Text(String),
    /// `(category, text)` pairs.
    Plural(Vec<(String, String)>),
}
type Catalog = HashMap<String, Entry>;
const PLURAL_CATEGORIES: [&str; 6] = ["zero", "one", "two", "few", "many", "other"];

macro_rules! catalog_files {
    ($tag:literal: $($namespace:literal),*) => {
        &[$(($namespace, include_str!(concat!("../locales/", $tag, "/", $namespace, ".json")))),*]
    };
}
/// Every catalog of `locale`, as `(namespace, JSON)`.
fn files(locale: Locale) -> &'static [(&'static str, &'static str)] {
    match locale {
        Locale::EnUs => {
            catalog_files!("en-US": "common", "server", "mail", "cli", "tui", "desktop")
        }
        Locale::De => catalog_files!("de": "common", "server", "mail", "cli", "tui", "desktop"),
    }
}
fn catalog(locale: Locale) -> &'static Catalog {
    static CATALOGS: [OnceLock<Catalog>; Locale::ALL.len()] =
        [const { OnceLock::new() }; Locale::ALL.len()];
    CATALOGS[locale as usize].get_or_init(|| {
        let mut catalog = Catalog::new();
        for (namespace, json) in files(locale) {
            // The catalogs are part of the program; `scripts/check-i18n.mjs` and the tests parse them.
            let value: Value = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("{}/{namespace}.json is invalid: {e}", locale.tag()));
            flatten(namespace, &value, &mut catalog);
        }
        catalog
    })
}
fn flatten(prefix: &str, value: &Value, catalog: &mut Catalog) {
    match value {
        Value::String(text) => {
            catalog.insert(prefix.to_owned(), Entry::Text(text.clone()));
        }
        Value::Object(map)
            if map.contains_key("other")
                && map.keys().all(|k| PLURAL_CATEGORIES.contains(&k.as_str())) =>
        {
            let forms = map
                .iter()
                .filter_map(|(category, text)| Some((category.clone(), text.as_str()?.to_owned())))
                .collect();
            catalog.insert(prefix.to_owned(), Entry::Plural(forms));
        }
        Value::Object(map) => {
            for (key, value) in map {
                flatten(&format!("{prefix}.{key}"), value, catalog);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiates_languages() {
        assert_eq!(Locale::matching("de_AT.UTF-8"), Some(Locale::De));
        assert_eq!(Locale::matching("en-GB"), Some(Locale::EnUs));
        assert_eq!(Locale::matching("fr-FR"), None);
        assert_eq!(
            Locale::from_accept_language("fr-CH, fr;q=0.9, de;q=0.8, en;q=0.7, *;q=0.5"),
            Some(Locale::De)
        );
        assert_eq!(
            Locale::from_accept_language("de;q=0, en-US"),
            Some(Locale::EnUs)
        );
        assert_eq!(Locale::from_accept_language("fr, ja"), None);
        assert_eq!(
            Locale::from_accept_language("en;q=0.5, de"),
            Some(Locale::De)
        );
        assert_eq!(
            Locale::from_accept_language("de;q=NaN, en;q=0.5"),
            Some(Locale::EnUs)
        );
        assert_eq!(
            Locale::from_accept_language("de;q=2, en"),
            Some(Locale::EnUs)
        );
        assert_eq!(
            Locale::from_accept_language("en;q=0.7, de;q=0.7"),
            Some(Locale::EnUs)
        );
    }

    #[test]
    fn terminal_language_follows_gettext_order() {
        let vars = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        assert_eq!(
            Locale::from_vars(vars(&[("LANG", "de_DE.UTF-8")])),
            Some(Locale::De)
        );
        // LC_ALL overrides LANG; LANGUAGE's list overrides both unless the locale is C.
        assert_eq!(
            Locale::from_vars(vars(&[("LANG", "de_DE.UTF-8"), ("LC_ALL", "en_US.UTF-8")])),
            Some(Locale::EnUs)
        );
        assert_eq!(
            Locale::from_vars(vars(&[("LANG", "en_US.UTF-8"), ("LANGUAGE", "fr:de")])),
            Some(Locale::De)
        );
        assert_eq!(
            Locale::from_vars(vars(&[("LC_ALL", "C.UTF-8"), ("LANGUAGE", "de")])),
            Some(Locale::EnUs)
        );
        // A language Alice does not speak is English, not the system's.
        assert_eq!(
            Locale::from_vars(vars(&[("LANG", "fr_FR.UTF-8")])),
            Some(Locale::EnUs)
        );
        assert_eq!(Locale::from_vars(vars(&[])), None);
    }

    #[test]
    fn language_preference_round_trips() {
        let system: Language = serde_json::from_str("\"system\"").unwrap();
        assert_eq!(system, Language(None));
        let german: Language = serde_json::from_str("\"de\"").unwrap();
        assert_eq!(german, Language(Some(Locale::De)));
        assert_eq!(serde_json::to_string(&german).unwrap(), "\"de\"");
        assert!(serde_json::from_str::<Language>("\"fr\"").is_err());
    }

    #[test]
    fn fills_placeholders_without_reinterpreting_values() {
        assert_eq!(
            interpolate(
                "{count} messages in {folder}",
                &[("count", Arg::Int(3)), ("folder", "{count}".into())]
            ),
            "3 messages in {count}"
        );
        assert_eq!(
            interpolate("{not closed {name}", &[("name", "Ana".into())]),
            "{not closed Ana"
        );
        assert_eq!(interpolate("{missing}", &[]), "{missing}");
    }

    #[test]
    fn errors_do_not_discard_outer_context() {
        let error = anyhow::Error::new(text!("common.folder.inbox"));
        assert!(matches!(Message::of(&error), Message::Text(_)));
        // A translatable root cause must not hide a more relevant outer error.
        let wrapped = error.context("Server said no");
        assert_eq!(error_text(&wrapped, Locale::De), "Server said no");
        let plain = anyhow::anyhow!("IMAP: BAD command");
        assert_eq!(
            Message::of(&plain),
            Message::Plain("IMAP: BAD command".into())
        );
    }
}
