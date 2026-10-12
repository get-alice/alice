//! Saving attachments to the user's Downloads folder, opening them or showing them there, and
//! handing calendar events to the system calendar app.

use crate::{Daemon, desktop};
use alice_core::{
    client::needs_reconnect,
    i18n::{self, Locale},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::Manager;

/// Which file in Downloads holds each attachment Alice saved, by `<message id>:<index>`, so
/// opening an attachment again reuses its file instead of fetching it from the mail server. Kept
/// in `downloads.json` in Alice's data folder.
type Records = BTreeMap<String, Record>;

/// A saved attachment: its file name in Downloads and the size Alice wrote. A file that is gone,
/// or whose size differs, is no longer that attachment, and the next open saves it anew.
#[derive(Serialize, Deserialize)]
struct Record {
    name: String,
    size: u64,
}

/// Serializes reading and rewriting `downloads.json`.
static RECORDS: Mutex<()> = Mutex::new(());

/// Files that run a program, a script, an installer or a link when opened. Alice shows these in
/// the file manager instead of opening them.
const RUNNABLE: &str = "app appref-ms application bash bat cmd com command cpl csh desktop exe \
    fileloc gadget hta inetloc inf jar js jse ksh lnk mpkg msc msi msp pif pkg pl ps1 psm1 py pyw \
    rb reg run scf scr sh terminal tool url vbe vbs webloc ws wsc wsf wsh zsh";

/// Fetches attachment `index` of a message from the mail server (through the daemon): its name
/// and bytes.
async fn fetch(
    daemon: &Daemon,
    message_id: &str,
    index: usize,
    locale: Locale,
) -> Result<(String, Vec<u8>), String> {
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
    match client.download_in(locale, &path).await {
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
    .map_err(|e| i18n::error_text(&e, locale))
}

/// The bytes of attachment `index` of a message, for showing an image embedded in its HTML.
#[tauri::command]
pub async fn read_attachment(
    app: tauri::AppHandle,
    daemon: tauri::State<'_, Daemon>,
    message_id: String,
    index: usize,
) -> Result<tauri::ipc::Response, String> {
    let (_, bytes) = fetch(&daemon, &message_id, index, desktop::locale(&app)).await?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Opens attachment `index` of a message with its default app, first saving it in Downloads under
/// its own name (numbered when that name is taken) unless Alice already saved it there. Programs
/// and scripts are shown in the file manager instead. Returns `{path, name, shown}`, `shown` when
/// the file was shown rather than opened.
#[tauri::command]
pub async fn open_attachment(
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
    let folder = app
        .path()
        .download_dir()
        .map_err(|_| alice_core::t!(locale, "desktop.error.downloads_folder"))?;
    let records = records_file(&app, locale)?;
    let key = format!("{message_id}:{index}");
    let known = {
        let (records, folder, key) = (records.clone(), folder.clone(), key.clone());
        blocking(move || Ok(saved(&load(&records), &key, &folder))).await?
    };
    let path = match known {
        Some(path) => path,
        None => {
            let (name, bytes) = fetch(&daemon, &message_id, index, locale).await?;
            blocking(move || {
                record_new(
                    &records,
                    &folder,
                    &key,
                    &file_name(&name, locale),
                    &bytes,
                    locale,
                )
                .map_err(|e| {
                    alice_core::t!(
                        locale,
                        "desktop.error.attachment_save",
                        error = e.to_string()
                    )
                })
            })
            .await?
        }
    };
    let shown = runs_when_opened(&path);
    let target = path.clone();
    blocking(move || {
        if shown {
            show_in_folder(&target)
        } else {
            open::that(&target)
        }
        .map_err(|e| {
            alice_core::t!(
                locale,
                "desktop.error.attachment_open",
                error = e.to_string()
            )
        })
    })
    .await?;
    Ok(json!({
        "path": path.display().to_string(),
        "name": path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        "shown": shown,
    }))
}

/// Where the attachments of a message that Alice saved in Downloads are, by index, for those still
/// there.
#[tauri::command]
pub async fn saved_attachments(
    app: tauri::AppHandle,
    message_id: String,
) -> Result<BTreeMap<usize, String>, String> {
    let locale = desktop::locale(&app);
    let folder = app
        .path()
        .download_dir()
        .map_err(|_| alice_core::t!(locale, "desktop.error.downloads_folder"))?;
    let records = records_file(&app, locale)?;
    blocking(move || {
        let prefix = format!("{message_id}:");
        let records = load(&records);
        Ok(records
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&prefix))
            .filter_map(|(key, _)| {
                let index: usize = key[prefix.len()..].parse().ok()?;
                Some((index, saved(&records, key, &folder)?.display().to_string()))
            })
            .collect())
    })
    .await
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
    show_in_folder(&file).map_err(|e| e.to_string())
}

/// Selects `file` in Finder or Explorer; elsewhere opens its folder.
fn show_in_folder(file: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open")
        .arg("-R")
        .arg(file)
        .status();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer")
        .arg(format!("/select,{}", file.display()))
        .status();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let status =
        open::that(file.parent().unwrap_or(file)).map(|()| std::process::ExitStatus::default());
    status.map(|_| ())
}

/// Runs blocking file work off the async runtime.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

fn records_file(app: &tauri::AppHandle, locale: Locale) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|folder| folder.join("downloads.json"))
        .map_err(|_| alice_core::t!(locale, "desktop.error.data_folder"))
}

/// The saved attachments; none when the file is missing or unreadable, which only costs fetching
/// them again. It is replaced whole, so reading needs no lock.
fn load(records: &Path) -> Records {
    std::fs::read(records)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The file in `folder` that holds attachment `key`, if Alice saved it and it is unchanged.
fn saved(records: &Records, key: &str, folder: &Path) -> Option<PathBuf> {
    let record = records.get(key)?;
    // Only a plain name in Downloads, whatever the records file says.
    if Path::new(&record.name).file_name()? != record.name.as_str() {
        return None;
    }
    let path = folder.join(&record.name);
    let metadata = std::fs::metadata(&path).ok()?;
    (metadata.is_file() && metadata.len() == record.size).then_some(path)
}

/// Saves attachment `key` as a new file in `folder` and records it, unless another open saved it
/// meanwhile. Returns its path.
fn record_new(
    records: &Path,
    folder: &Path,
    key: &str,
    name: &str,
    bytes: &[u8],
    locale: Locale,
) -> std::io::Result<PathBuf> {
    let _lock = RECORDS.lock().unwrap_or_else(|e| e.into_inner());
    let mut known = load(records);
    if let Some(path) = saved(&known, key, folder) {
        return Ok(path);
    }
    let path = write_new(folder, name, bytes, locale)?;
    let record = Record {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        size: bytes.len() as u64,
    };
    known.insert(key.to_string(), record);
    if let Some(parent) = records.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Replaced whole, so readers never see half a file.
    let partial = records.with_extension("json.partial");
    std::fs::write(&partial, serde_json::to_vec(&known)?)?;
    std::fs::rename(&partial, records)?;
    Ok(path)
}

/// Whether opening the file would run something rather than show a document.
fn runs_when_opened(path: &Path) -> bool {
    let Some(extension) = path.extension() else {
        return false;
    };
    let extension = extension.to_string_lossy().to_ascii_lowercase();
    RUNNABLE
        .split_whitespace()
        .any(|runnable| runnable == extension)
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
    fn an_attachment_is_saved_once_until_its_file_changes() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("alice-saved-{}", std::process::id()));
        let records = dir.join("data").join("downloads.json");
        std::fs::create_dir_all(&dir)?;
        let first = record_new(&records, &dir, "m1:0", "plan.pdf", b"one", Locale::EnUs)?;
        let again = record_new(&records, &dir, "m1:0", "plan.pdf", b"one", Locale::EnUs)?;
        assert_eq!(again, first);
        assert_eq!(saved(&load(&records), "m1:0", &dir), Some(first.clone()));
        // Another attachment with the same name gets its own file.
        let other = record_new(&records, &dir, "m2:0", "plan.pdf", b"two", Locale::EnUs)?;
        assert_eq!(other.file_name().unwrap(), "plan (2).pdf");
        // Edited or removed, the file is no longer that attachment.
        std::fs::write(&first, b"edited")?;
        assert_eq!(saved(&load(&records), "m1:0", &dir), None);
        std::fs::remove_file(&other)?;
        assert_eq!(saved(&load(&records), "m2:0", &dir), None);
        let fresh = record_new(&records, &dir, "m1:0", "plan.pdf", b"one", Locale::EnUs)?;
        assert_eq!(fresh.file_name().unwrap(), "plan (2).pdf");
        assert_eq!(std::fs::read(&first)?, b"edited");
        std::fs::remove_dir_all(&dir)
    }

    #[test]
    fn records_cannot_point_outside_downloads() {
        let dir = std::env::temp_dir();
        let records = Records::from([(
            "m:0".to_string(),
            Record {
                name: "../etc/hosts".into(),
                size: 0,
            },
        )]);
        assert_eq!(saved(&records, "m:0", &dir.join("downloads")), None);
    }

    #[test]
    fn programs_are_shown_instead_of_opened() {
        assert!(runs_when_opened(Path::new("/d/invoice.EXE")));
        assert!(runs_when_opened(Path::new("/d/setup (2).command")));
        assert!(!runs_when_opened(Path::new("/d/invoice.pdf")));
        assert!(!runs_when_opened(Path::new("/d/README")));
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
