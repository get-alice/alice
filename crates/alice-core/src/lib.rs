// The daemon handles untrusted mail; only the desktop crate needs `unsafe`, for macOS APIs.
#![forbid(unsafe_code)]

pub mod ai;
pub mod calendar;
pub mod client;
pub mod contacts;
pub mod daemon;
pub mod dav;
pub mod i18n;
pub mod ical;
pub mod mime;
pub mod model;
pub mod oauth;
pub mod providers;
pub mod remote;
pub mod search;
pub mod store;
pub mod sync;
pub mod sync_store;
pub mod updates;
pub mod vault;

pub use client::Client;
pub use i18n::{Language, Locale};
pub use model::*;
