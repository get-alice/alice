//! Saving attachments to the user's Downloads folder and showing them there, and handing calendar
//! events to the system calendar app.

use crate::{Daemon, desktop};
use alice_core::{
    client::needs_reconnect,
    i18n::{self, Locale},
};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};
use tauri::Manager;

/// Fetches attachment `index` of a message from the mail server (through the daemon) and saves it
/// in Downloads under its own name, numbered when that name is taken. Returns `{path, name}`.
#[tauri::command]
pub async fn save_attachment(
    app: tauri::AppHandle,
    daemon: tauri::State<'_, Daemon>,
    message_id: String,
    index: usize,
    locale: Option<String>,
) -> Result<Value, String> {
    let locale = locale
        .as_deref()
        .and_then(Locale::from_tag)
        .unwrap_or_else(|| desktop::locale(&app));
    if message_id.is_empty()
        || !message_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(alice_core::t!(locale, "desktop.error.message_missing"));
    }
    let path = format!("/v1/messages/{message_id}/attachments/{index}");
    let client = daemon
        .client(None)
        .await
        .map_err(|e| i18n::error_text(&e, locale))?;
    let (name, bytes) = match client.download_in(locale, &path).await {
        Err(error) if needs_reconnect(&error) => {
            daemon
                .client(Some(&client))
                .await
                .map_err(|e| i18n::error_text(&e, locale))?
                .download_in(locale, &path)
                .await
        }
        result => result,
    }
    .map_err(|e| i18n::error_text(&e, locale))?;
    let folder = app
        .path()
        .download_dir()
        .map_err(|_| alice_core::t!(locale, "desktop.error.downloads_folder"))?;
    let saved = tauri::async_runtime::spawn_blocking(move || {
        write_new(&folder, &file_name(&name, locale), &bytes, locale)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| {
        alice_core::t!(
            locale,
            "desktop.error.attachment_save",
            error = e.to_string()
        )
    })?;
    Ok(json!({
        "path": saved.display().to_string(),
        "name": saved.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
    }))
}

/// Shows a saved attachment in the system file manager. Only files in Downloads are accepted.
#[tauri::command]
pub fn reveal_download(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let locale = desktop::locale(&app);
    let folder = app
        .path()
        .download_dir()
        .map_err(|e| e.to_string())?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let file = PathBuf::from(path)
        .canonicalize()
        .map_err(|_| alice_core::t!(locale, "desktop.error.download_missing"))?;
    if file.parent() != Some(folder.as_path()) || !file.is_file() {
        return Err(alice_core::t!(locale, "desktop.error.saved_only"));
    }
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open")
        .arg("-R")
        .arg(&file)
        .status();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer")
        .arg(format!("/select,{}", file.display()))
        .status()
        .map(|_| std::process::ExitStatus::default());
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let status = open::that(&folder).map(|_| std::process::ExitStatus::default());
    status.map(|_| ()).map_err(|e| e.to_string())
}

/// Opens an iCalendar event with the default calendar app, which offers to add it. The file is
/// written to Alice's cache folder, replacing an earlier copy of the same event.
#[tauri::command]
pub async fn open_calendar_file(
    app: tauri::AppHandle,
    filename: String,
    ics: String,
) -> Result<(), String> {
    let locale = desktop::locale(&app);
    let folder = app
        .path()
        .app_cache_dir()
        .map_err(|_| alice_core::t!(locale, "desktop.error.cache_folder"))?
        .join("events");
    let path = folder.join(ics_name(&filename, locale));
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::create_dir_all(&folder)
            .and_then(|()| std::fs::write(&path, ics))
            .map_err(|e| {
                alice_core::t!(locale, "desktop.error.event_save", error = e.to_string())
            })?;
        open::that(&path).map_err(|e| {
            alice_core::t!(locale, "desktop.error.calendar_open", error = e.to_string())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A safe file name ending in `.ics`, so the system picks the calendar app.
fn ics_name(suggested: &str, locale: Locale) -> String {
    let name = file_name(suggested, locale);
    if name.to_ascii_lowercase().ends_with(".ics") {
        name
    } else {
        file_name(&format!("{name}.ics"), locale)
    }
}

/// A safe file name for an attachment: no folders, no characters file systems reject, no hidden
/// or reserved names, at most 180 bytes with the extension kept.
fn file_name(suggested: &str, locale: Locale) -> String {
    let cleaned: String = suggested
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned
        .trim()
        .trim_start_matches('.')
        .trim_end_matches(['.', ' '])
        .to_string();
    let stem = Path::new(&cleaned)
        .file_stem()
        .map(|s| s.to_string_lossy().to_uppercase())
        .unwrap_or_default();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.ends_with(|c: char| c.is_ascii_digit()));
    let name = if cleaned.is_empty() {
        alice_core::t!(locale, "desktop.download.attachment")
    } else if reserved {
        alice_core::t!(locale, "desktop.download.reserved_name", name = cleaned)
    } else {
        cleaned
    };
    if name.len() <= 180 {
        return name;
    }
    let extension = Path::new(&name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .filter(|e| e.len() <= 20)
        .unwrap_or_default();
    let mut stem: String = name[..name.len() - extension.len()].to_string();
    while stem.len() + extension.len() > 180 {
        stem.pop();
    }
    format!("{}{extension}", stem.trim_end())
}

/// Writes `bytes` to a new file in `folder` named `name`, or `name (2)`, `name (3)`… when taken.
/// Never replaces an existing file.
fn write_new(folder: &Path, name: &str, bytes: &[u8], locale: Locale) -> std::io::Result<PathBuf> {
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem.to_string(), format!(".{extension}")),
        _ => (name.to_string(), String::new()),
    };
    for attempt in 1..10_000 {
        let path = folder.join(if attempt == 1 {
            name.to_string()
        } else {
            format!("{stem} ({attempt}){extension}")
        });
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(bytes)?;
                file.sync_all()?;
                return Ok(path);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        ErrorKind::AlreadyExists,
        alice_core::t!(locale, "desktop.error.too_many_files"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_names_cannot_leave_downloads_or_hide() {
        assert_eq!(
            file_name("../../etc/passwd", Locale::EnUs),
            "_.._etc_passwd"
        );
        assert_eq!(file_name("..\\..\\boot.ini", Locale::EnUs), "_.._boot.ini");
        assert_eq!(file_name(".bashrc", Locale::EnUs), "bashrc");
        assert_eq!(file_name("  ", Locale::EnUs), "attachment");
        assert_eq!(file_name("CON.txt", Locale::EnUs), "attachment CON.txt");
        assert_eq!(file_name("Rechnung €.pdf", Locale::EnUs), "Rechnung €.pdf");
        let long = file_name(&format!("{}.pdf", "a".repeat(300)), Locale::EnUs);
        assert!(long.len() <= 180 && Path::new(&long).extension().is_some_and(|e| e == "pdf"));
    }

    #[test]
    fn saving_never_replaces_an_existing_file() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("alice-downloads-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let first = write_new(&dir, "plan.pdf", b"one", Locale::EnUs)?;
        let second = write_new(&dir, "plan.pdf", b"two", Locale::EnUs)?;
        assert_eq!(second.file_name().unwrap(), "plan (2).pdf");
        assert_eq!(std::fs::read(&first)?, b"one");
        std::fs::remove_dir_all(&dir)
    }

    #[test]
    fn calendar_files_stay_in_their_folder_and_open_as_events() {
        assert_eq!(
            ics_name("Design review.ics", Locale::EnUs),
            "Design review.ics"
        );
        assert_eq!(ics_name("../invite", Locale::EnUs), "_invite.ics");
        assert!(
            Path::new(&ics_name(&"a".repeat(300), Locale::EnUs))
                .extension()
                .is_some_and(|e| e == "ics")
        );
    }
}
