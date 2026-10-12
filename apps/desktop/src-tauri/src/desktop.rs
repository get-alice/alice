//! Alice beyond its window: the menu bar icon, new-mail notifications, and showing the window again.

use crate::{
    Daemon, clipboard,
    notification::{self, Notice, Target},
};
use alice_core::{Arrival, Arrivals, OtpArrival, Preferences, i18n::Locale};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::{
    AppHandle, Emitter, Manager, RunEvent, Window, WindowEvent,
    menu::{Menu, MenuBuilder, MenuEvent},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

const WINDOW: &str = "main";
const TRAY: &str = "alice";
/// How often the app checks the daemon for new mail and changed preferences.
const CHECK_INTERVAL: Duration = Duration::from_secs(5);
/// More arrivals than this in one check are announced together.
const MAX_SEPARATE_NOTICES: usize = 3;

#[derive(Clone, Copy)]
enum CopyOutcome {
    Copied,
    Failed(clipboard::CopyError),
    Superseded,
}

/// Only ordering metadata is retained across checks, never the previous code.
struct LastCopied {
    received_at: i64,
    id: String,
    expires_at: i64,
}

impl LastCopied {
    fn supersedes(&self, event: &OtpArrival, now: i64) -> bool {
        self.expires_at > now
            && (self.received_at > event.received_at
                || (self.received_at == event.received_at && self.id <= event.id))
    }
}

/// What a clicked notification asked the window to show; `message_id` is `None` for the Inbox.
#[derive(Clone, serde::Serialize)]
pub struct Opened {
    message_id: Option<String>,
}

#[derive(Default)]
pub struct Desktop {
    /// Held by the page until it takes it, so a click that launched Alice is not lost.
    opened: Mutex<Option<Opened>>,
    asked_permission: AtomicBool,
    locale: Mutex<Option<Locale>>,
    tray_locale: Mutex<Option<Locale>>,
}

pub fn setup(app: &AppHandle) {
    let handle = app.clone();
    notification::init(&app.config().identifier, move |target| {
        open(&handle, target)
    });
    // Registered once: a tray icon's own menu handler outlives the icon, so re-adding it would
    // run each action again.
    app.on_menu_event(on_tray_menu);
    tauri::async_runtime::spawn(watch(app.clone()));
}

/// Follows the daemon while Alice runs: applies the desktop preferences and announces new mail.
/// Each request reconnects, starting the daemon again if it stopped.
async fn watch(app: AppHandle) {
    let daemon = app.state::<Daemon>();
    let mut cursor: Option<String> = None;
    let mut last_copied = None;
    let mut checks = tokio::time::interval(CHECK_INTERVAL);
    checks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        checks.tick().await;
        let Ok(preferences) = daemon.get::<Preferences>("/v1/preferences").await else {
            continue;
        };
        apply(&app, &preferences);
        // OTP delivery is opt-in, independent of mail banners, focus, and the arrival cursor.
        let otp_ids = if preferences.otp_autocopy {
            deliver_otps(
                &app,
                &daemon,
                preferences.language.resolve(),
                &mut last_copied,
            )
            .await
        } else {
            Vec::new()
        };
        let path = match &cursor {
            Some(cursor) => format!("/v1/arrivals?after={cursor}"),
            None => "/v1/arrivals".into(),
        };
        let Ok(arrivals) = daemon.get::<Arrivals>(&path).await else {
            continue;
        };
        // The first check only learns where to start.
        if cursor.replace(arrivals.cursor).is_some() && preferences.notifications && !in_use(&app) {
            let messages = arrivals
                .messages
                .into_iter()
                .filter(|mail| !otp_ids.iter().any(|id| id == &mail.id))
                .collect();
            notices(messages, preferences.language.resolve())
                .into_iter()
                .for_each(notification::show);
        }
    }
}

/// Claims the batch before writing once. A claimed event is never retried after a copy failure.
async fn deliver_otps(
    app: &AppHandle,
    daemon: &Daemon,
    locale: Locale,
    last_copied: &mut Option<LastCopied>,
) -> Vec<String> {
    let Ok(pending) = daemon.get::<Vec<OtpArrival>>("/v1/otp").await else {
        return Vec::new();
    };
    let pending = fresh_otps(pending, clipboard::now());
    let ids = pending
        .iter()
        .map(|event| event.message_id.clone())
        .collect();
    let mut claimed = Vec::with_capacity(pending.len());
    for event in pending {
        let path = format!("/v1/otp/{}/claim", event.id);
        if let Ok(response) = daemon.request("POST", &path, None).await
            && serde_json::from_str::<bool>(response.get()).unwrap_or(false)
        {
            claimed.push(event);
        }
    }
    let Some(newest) = claimed.first() else {
        return ids;
    };
    // Opting out during a request must not leave an outstanding batch copying afterwards.
    if !daemon
        .get::<Preferences>("/v1/preferences")
        .await
        .is_ok_and(|preferences| preferences.otp_autocopy)
    {
        return ids;
    }
    let result = if last_copied
        .as_ref()
        .is_some_and(|previous| previous.supersedes(newest, clipboard::now()))
    {
        CopyOutcome::Superseded
    } else {
        match clipboard::copy(app, &newest.code, newest.expires_at).await {
            Ok(()) => {
                *last_copied = Some(LastCopied {
                    received_at: newest.received_at,
                    id: newest.id.clone(),
                    expires_at: newest.expires_at,
                });
                CopyOutcome::Copied
            }
            Err(error) => CopyOutcome::Failed(error),
        }
    };
    let expires_at = claimed
        .iter()
        .map(|event| event.expires_at)
        .min()
        .unwrap_or(newest.expires_at);
    otp_notices(claimed, result, locale)
        .into_iter()
        .for_each(|notice| notification::show_sensitive(notice, expires_at));
    ids
}

fn fresh_otps(mut events: Vec<OtpArrival>, now: i64) -> Vec<OtpArrival> {
    events.retain(|event| {
        event.expires_at > now
            && event.received_at <= now.saturating_add(30)
            && event.received_at >= now.saturating_sub(300)
    });
    events.sort_unstable_by(|left, right| {
        right
            .received_at
            .cmp(&left.received_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    events
}

/// No grouped summary: every OTP keeps its code and its actual clipboard outcome visible.
fn otp_notices(events: Vec<OtpArrival>, result: CopyOutcome, locale: Locale) -> Vec<Notice> {
    events
        .into_iter()
        .enumerate()
        .map(|(index, event)| {
            let status = match (index == 0, result) {
                (_, CopyOutcome::Superseded) => alice_core::t!(locale, "desktop.otp.superseded"),
                (true, CopyOutcome::Copied) => alice_core::t!(locale, "desktop.otp.copied"),
                (true, CopyOutcome::Failed(clipboard::CopyError::Unavailable)) => {
                    alice_core::t!(locale, "desktop.otp.copy_failed")
                }
                (true, CopyOutcome::Failed(clipboard::CopyError::Expired)) => {
                    alice_core::t!(locale, "desktop.otp.expired")
                }
                (false, CopyOutcome::Copied) => alice_core::t!(locale, "desktop.otp.superseded"),
                (false, CopyOutcome::Failed(_)) => {
                    alice_core::t!(locale, "desktop.otp.older_not_copied")
                }
            };
            let name = sender_text(&event.from_name);
            let email = sender_text(&event.from_email);
            Notice {
                locale,
                target: Target::Message(event.message_id),
                title: if !name.is_empty() {
                    name
                } else if !email.is_empty() {
                    email
                } else {
                    alice_core::t!(locale, "desktop.notice.unknown_sender")
                },
                subtitle: alice_core::t!(locale, "desktop.otp.code", code = event.code),
                body: status,
            }
        })
        .collect()
}

fn sender_text(text: &str) -> String {
    let clean: String = text
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(*c, '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}' | '\u{feff}')
        })
        .take(120)
        .collect();
    clean.trim().to_owned()
}

/// Applies the desktop preferences: adds or removes the menu bar icon, and asks once for permission
/// to notify.
pub fn apply(app: &AppHandle, preferences: &Preferences) {
    let locale = preferences.language.resolve();
    *app.state::<Desktop>()
        .locale
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(locale);
    if (preferences.notifications || preferences.otp_autocopy)
        && !app
            .state::<Desktop>()
            .asked_permission
            .swap(true, Ordering::Relaxed)
    {
        notification::request_permission(locale);
    }
    // AppKit adds and removes status items only on the main thread, which also orders the changes.
    let handle = app.clone();
    let shown = preferences.menu_bar;
    let _ = app.run_on_main_thread(move || show_tray(&handle, shown, locale));
}

/// The last applied preference, including system default before the daemon's first snapshot.
pub fn locale(app: &AppHandle) -> Locale {
    app.state::<Desktop>()
        .locale
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .unwrap_or_else(Locale::system)
}

fn show_tray(app: &AppHandle, shown: bool, locale: Locale) {
    let desktop = app.state::<Desktop>();
    let mut tray_locale = desktop
        .tray_locale
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    match (shown, app.tray_by_id(TRAY).is_some()) {
        (true, false) => match add_tray(app, locale) {
            Ok(()) => *tray_locale = Some(locale),
            Err(error) => eprintln!(
                "{}",
                alice_core::t!(locale, "desktop.error.tray", error = error.to_string())
            ),
        },
        (true, true) if *tray_locale != Some(locale) => {
            let updated = tray_menu(app, locale)
                .and_then(|menu| app.tray_by_id(TRAY).unwrap().set_menu(Some(menu)));
            match updated {
                Ok(()) => *tray_locale = Some(locale),
                Err(error) => eprintln!(
                    "{}",
                    alice_core::t!(locale, "desktop.error.tray", error = error.to_string())
                ),
            }
        }
        (false, true) => {
            app.remove_tray_by_id(TRAY);
            *tray_locale = None;
            // Without the icon, a hidden window on Windows and Linux could not be reopened.
            if !cfg!(target_os = "macos")
                && let Some(window) = app.get_webview_window(WINDOW)
                && !window.is_visible().unwrap_or(true)
            {
                show_window(app);
            }
        }
        _ => {}
    }
}

fn tray_menu(app: &AppHandle, locale: Locale) -> tauri::Result<Menu<tauri::Wry>> {
    MenuBuilder::new(app)
        .text("tray-open", alice_core::t!(locale, "desktop.tray.open"))
        .text("tray-sync", alice_core::t!(locale, "desktop.tray.sync"))
        .separator()
        .text("tray-quit", alice_core::t!(locale, "desktop.tray.quit"))
        .build()
}

fn add_tray(app: &AppHandle, locale: Locale) -> tauri::Result<()> {
    let menu = tray_menu(app, locale)?;
    let tray = TrayIconBuilder::with_id(TRAY)
        .tooltip("Alice")
        .menu(&menu)
        // Windows opens the app on a left click and shows the menu on a right click.
        .show_menu_on_left_click(!cfg!(windows))
        .on_tray_icon_event(|tray, event| {
            if cfg!(windows)
                && let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
            {
                show_window(tray.app_handle());
            }
        });
    #[cfg(target_os = "macos")]
    let tray = tray
        .icon(tauri::include_image!("icons/tray.png"))
        .icon_as_template(true);
    #[cfg(not(target_os = "macos"))]
    let tray = match app.default_window_icon() {
        Some(icon) => tray.icon(icon.clone()),
        None => tray,
    };
    tray.build(app)?;
    Ok(())
}

fn on_tray_menu(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "tray-open" => show_window(app),
        "tray-sync" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                // New mail is announced by the next check.
                let _ = app
                    .state::<Daemon>()
                    .request("POST", "/v1/sync", None)
                    .await;
            });
        }
        "tray-quit" => app.exit(0),
        _ => {}
    }
}

/// Whether the window is open and focused, where new mail already appears.
fn in_use(app: &AppHandle) -> bool {
    app.get_webview_window(WINDOW).is_some_and(|window| {
        window.is_visible().unwrap_or(false)
            && window.is_focused().unwrap_or(false)
            && !window.is_minimized().unwrap_or(false)
    })
}

/// One notice per arrival, or one for all of them when many arrive at once.
fn notices(arrivals: Vec<Arrival>, locale: Locale) -> Vec<Notice> {
    if arrivals.len() > MAX_SEPARATE_NOTICES {
        return vec![Notice {
            locale,
            target: Target::Inbox,
            title: "Alice".into(),
            subtitle: String::new(),
            body: alice_core::t!(locale, "desktop.notice.grouped", count = arrivals.len()),
        }];
    }
    arrivals
        .into_iter()
        .map(|mail| Notice {
            locale,
            target: Target::Message(mail.id),
            title: if !mail.from_name.trim().is_empty() {
                mail.from_name
            } else if !mail.from_email.trim().is_empty() {
                mail.from_email
            } else {
                alice_core::t!(locale, "desktop.notice.unknown_sender")
            },
            subtitle: if mail.subject.trim().is_empty() {
                alice_core::t!(locale, "desktop.notice.no_subject")
            } else {
                mail.subject
            },
            body: excerpt(&mail.preview, 200),
        })
        .collect()
}

fn excerpt(text: &str, chars: usize) -> String {
    match text.char_indices().nth(chars) {
        Some((end, _)) => format!("{}…", text[..end].trim_end()),
        None => text.into(),
    }
}

/// Shows the window with what a clicked notification announced.
fn open(app: &AppHandle, target: Target) {
    let message_id = match target {
        Target::Message(id) => Some(id),
        Target::Inbox => None,
    };
    *app.state::<Desktop>()
        .opened
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(Opened { message_id });
    show_window(app);
    let _ = app.emit("open-message", ());
}

/// The page takes what a clicked notification asked it to show, once.
#[tauri::command]
pub fn take_opened(desktop: tauri::State<'_, Desktop>) -> Option<Opened> {
    desktop
        .opened
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Closing the window hides it while Alice keeps running: always on macOS, where the Dock icon
/// reopens it, and on other systems while the menu bar icon can.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event
        && window.label() == WINDOW
        && (cfg!(target_os = "macos") || window.app_handle().tray_by_id(TRAY).is_some())
    {
        api.prevent_close();
        let _ = window.hide();
    }
}

pub fn on_run_event(app: &AppHandle, event: RunEvent) {
    #[cfg(target_os = "macos")]
    if let RunEvent::Reopen {
        has_visible_windows: false,
        ..
    } = event
    {
        show_window(app);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, event);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrival(id: &str, from_name: &str, subject: &str) -> Arrival {
        Arrival {
            id: id.into(),
            account_id: "account".into(),
            from_name: from_name.into(),
            from_email: "sophie@example.com".into(),
            subject: subject.into(),
            preview: "Let's talk".into(),
        }
    }

    #[test]
    fn each_arrival_is_announced_by_its_sender_and_subject() {
        let shown = notices(
            vec![arrival("1", "Sophie", "Ideas"), arrival("2", " ", "")],
            Locale::EnUs,
        );
        assert_eq!(shown.len(), 2);
        assert_eq!(shown[0].target, Target::Message("1".into()));
        assert_eq!(
            (shown[0].title.as_str(), shown[0].subtitle.as_str()),
            ("Sophie", "Ideas")
        );
        assert_eq!(
            (shown[1].title.as_str(), shown[1].subtitle.as_str()),
            ("sophie@example.com", "(no subject)")
        );
    }

    #[test]
    fn many_arrivals_at_once_are_announced_together() {
        let shown = notices(
            (0..4)
                .map(|i| arrival(&i.to_string(), "Sophie", "Ideas"))
                .collect(),
            Locale::EnUs,
        );
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].target, Target::Inbox);
        assert_eq!(shown[0].body, "4 new messages in your Inbox");
    }

    #[test]
    fn long_previews_are_cut_on_a_character_boundary() {
        assert_eq!(excerpt("héllo wörld", 6), "héllo…");
        assert_eq!(excerpt("short", 6), "short");
    }

    fn otp(id: &str, received_at: i64, expires_at: i64) -> OtpArrival {
        OtpArrival {
            id: id.into(),
            message_id: format!("message-{id}"),
            account_id: "account".into(),
            from_name: "Sophie".into(),
            from_email: "sophie@example.com".into(),
            code: "123456".into(),
            received_at,
            expires_at,
        }
    }

    #[test]
    fn only_fresh_codes_are_selected_newest_first_with_stable_ties() {
        let fresh = fresh_otps(
            vec![
                otp("old", 900, 1100),
                otp("b", 990, 1290),
                otp("a", 990, 1290),
                otp("expired", 999, 1000),
                otp("imported", 699, 1100),
                otp("future", 1031, 1331),
            ],
            1000,
        );
        assert_eq!(
            fresh
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "old"],
        );
    }

    #[test]
    fn every_code_keeps_its_click_target_and_accurate_copy_outcome() {
        let cases = [
            (
                CopyOutcome::Copied,
                "Copied to clipboard",
                "Not copied — a newer code was copied instead",
            ),
            (
                CopyOutcome::Failed(clipboard::CopyError::Unavailable),
                "Not copied — private clipboard access failed. Copy the code manually",
                "Not copied — a newer code arrived, but could not be copied",
            ),
            (
                CopyOutcome::Failed(clipboard::CopyError::Expired),
                "Not copied — the code expired before it could be copied",
                "Not copied — a newer code arrived, but could not be copied",
            ),
            (
                CopyOutcome::Superseded,
                "Not copied — a newer code was copied instead",
                "Not copied — a newer code was copied instead",
            ),
        ];
        for (result, newest_status, older_status) in cases {
            let events = (0..4)
                .map(|id| otp(&id.to_string(), 990 - id, 1290))
                .collect();
            let shown = otp_notices(events, result, Locale::EnUs);
            assert_eq!(shown.len(), 4, "OTP codes must never be hidden by grouping");
            assert_eq!(shown[0].body, newest_status);
            for (index, notice) in shown.iter().enumerate() {
                assert_eq!(notice.target, Target::Message(format!("message-{index}")));
                assert_eq!(notice.subtitle, "Verification code: 123456");
                if index != 0 {
                    assert_eq!(notice.body, older_status);
                }
            }
        }
    }

    #[test]
    fn otp_sender_cannot_insert_controls_or_bidi_overrides_into_the_banner() {
        let mut event = otp("message", 990, 1290);
        event.from_name = "  Soph\u{202e}ie\r\n  ".into();
        let shown = otp_notices(vec![event], CopyOutcome::Copied, Locale::EnUs);
        assert_eq!(shown[0].title, "Sophie");
    }

    #[test]
    fn delayed_older_batches_never_replace_a_newer_unexpired_copy() {
        let previous = LastCopied {
            received_at: 1000,
            id: "b".into(),
            expires_at: 1300,
        };
        assert!(previous.supersedes(&otp("old", 999, 1299), 1100));
        assert!(previous.supersedes(&otp("c", 1000, 1300), 1100));
        assert!(!previous.supersedes(&otp("a", 1000, 1300), 1100));
        assert!(!previous.supersedes(&otp("new", 1001, 1301), 1100));
        assert!(!previous.supersedes(&otp("old", 999, 1299), 1300));
    }
}
