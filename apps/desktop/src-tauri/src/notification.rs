//! System notifications whose click opens Alice. macOS uses `UNUserNotificationCenter` directly so a
//! click is delivered even when it launches Alice; Windows and Linux use `notify-rust`.

use alice_core::i18n::Locale;
use std::sync::OnceLock;

/// What a clicked notification shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Message(String),
    Inbox,
}
#[cfg(any(target_os = "macos", test))]
impl Target {
    /// The notification's identifier, which a newer notification with the same one replaces:
    /// one per message, and one Inbox summary at a time.
    fn identifier(&self) -> String {
        match self {
            Self::Message(id) => format!("message:{id}"),
            Self::Inbox => "inbox".into(),
        }
    }
    fn from_identifier(identifier: &str) -> Self {
        let identifier = identifier
            .strip_prefix("otp:")
            .and_then(|rest| rest.split_once(':').map(|(_, target)| target))
            .unwrap_or(identifier);
        identifier
            .strip_prefix("message:")
            .map_or(Self::Inbox, |id| Self::Message(id.into()))
    }
}

pub struct Notice {
    // Native macOS banners have no Alice-owned action label; other platforms use this for it.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub locale: Locale,
    pub target: Target,
    pub title: String,
    pub subtitle: String,
    pub body: String,
}

type Handler = Box<dyn Fn(Target) + Send + Sync>;
static ON_CLICK: OnceLock<Handler> = OnceLock::new();

fn clicked(target: Target) {
    if let Some(handler) = ON_CLICK.get() {
        handler(target);
    }
}

/// Registers what a click does. Call once, during app setup, so a click that launched Alice is handled.
pub fn init(identifier: &str, on_click: impl Fn(Target) + Send + Sync + 'static) {
    let _ = ON_CLICK.set(Box::new(on_click));
    platform::init(identifier);
}

/// Asks the system for permission to notify; macOS prompts the first time only.
pub fn request_permission(locale: Locale) {
    platform::request_permission(locale);
}

pub fn show(notice: Notice) {
    platform::show(notice, None);
}

/// OTP notification failures must never log a native error that might embed their payload.
pub fn show_sensitive(notice: Notice, expires_at: i64) {
    platform::show(notice, Some(expires_at));
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{Notice, Target, clicked};
    use alice_core::i18n::Locale;
    use block2::{DynBlock, RcBlock};
    use objc2::{
        AnyThread, define_class, msg_send, rc::Retained, runtime::Bool, runtime::ProtocolObject,
    };
    use objc2_foundation::{NSArray, NSBundle, NSError, NSObject, NSObjectProtocol, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
        UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
        UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "AliceNotificationDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            /// Alice is frontmost but its window may be closed, so still show the banner.
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                completion: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                completion.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List
                    | UNNotificationPresentationOptions::Sound,));
            }

            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                completion: &DynBlock<dyn Fn()>,
            ) {
                let identifier = response.notification().request().identifier().to_string();
                clicked(Target::from_identifier(&identifier));
                completion.call(());
            }
        }
    );

    /// `UNUserNotificationCenter` raises an exception outside an app bundle, e.g. under `tauri dev`.
    fn center() -> Option<Retained<UNUserNotificationCenter>> {
        let bundle = NSBundle::mainBundle();
        let path = bundle.bundlePath().to_string();
        let in_app = std::path::Path::new(&path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("app"));
        (bundle.bundleIdentifier().is_some() && in_app)
            .then(UNUserNotificationCenter::currentNotificationCenter)
    }

    pub fn init(_identifier: &str) {
        let Some(center) = center() else {
            eprintln!(
                "{}",
                alice_core::t!(Locale::system(), "desktop.notice.bundle_required")
            );
            return;
        };
        let delegate: Retained<Delegate> =
            unsafe { msg_send![super(Delegate::alloc().set_ivars(())), init] };
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        // The center keeps only a weak reference; the delegate lives as long as the app.
        std::mem::forget(delegate);
    }

    pub fn request_permission(locale: Locale) {
        if let Some(center) = center() {
            let done = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
                if !granted.as_bool() {
                    eprintln!(
                        "{}",
                        alice_core::t!(locale, "desktop.notice.permission_denied")
                    );
                }
            });
            center.requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
                &done,
            );
        }
    }

    pub fn show(notice: Notice, expires_at: Option<i64>) {
        let Some(center) = center() else { return };
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&notice.title));
        content.setSubtitle(&NSString::from_str(&notice.subtitle));
        content.setBody(&NSString::from_str(&notice.body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        static NEXT_OTP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let identifier = if expires_at.is_some() {
            format!(
                "otp:{}:{}",
                NEXT_OTP.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                notice.target.identifier()
            )
        } else {
            notice.target.identifier()
        };
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&identifier),
            &content,
            None,
        );
        center.addNotificationRequest_withCompletionHandler(&request, None);
        if let Some(expires_at) = expires_at {
            let delay = crate::clipboard::lifetime(expires_at).unwrap_or_default();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(delay).await;
                if let Some(center) = self::center() {
                    let id = NSString::from_str(&identifier);
                    let identifiers = NSArray::from_slice(&[&*id]);
                    center.removePendingNotificationRequestsWithIdentifiers(&identifiers);
                    center.removeDeliveredNotificationsWithIdentifiers(&identifiers);
                }
            });
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::{Notice, clicked};
    use alice_core::i18n::Locale;

    /// The installed app's AppUserModelID, which Windows shows as the toast's sender. It needs the
    /// runtime bundle identifier, so it is set in `init`.
    #[cfg(windows)]
    static APP_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();

    #[cfg_attr(not(windows), allow(unused_variables))]
    pub fn init(identifier: &str) {
        // Development builds are not registered with Windows, so their toasts use PowerShell's ID.
        #[cfg(windows)]
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
            && !dir.ends_with("target\\debug")
            && !dir.ends_with("target\\release")
        {
            let _ = APP_ID.set(identifier.into());
        }
    }

    pub fn request_permission(_locale: Locale) {}

    pub fn show(notice: Notice, expires_at: Option<i64>) {
        // Each notification waits for its click or dismissal on its own thread.
        std::thread::spawn(move || {
            let mut notification = notify_rust::Notification::new();
            notification.summary(&notice.title);
            let delay =
                expires_at.map(|expiry| crate::clipboard::lifetime(expiry).unwrap_or_default());
            if let Some(delay) = delay {
                notification.timeout(notify_rust::Timeout::Milliseconds(
                    delay.as_millis().max(1).min(60_000) as u32,
                ));
            }
            #[cfg(windows)]
            {
                notification.subtitle(&notice.subtitle).body(&notice.body);
                if let Some(app_id) = APP_ID.get() {
                    notification.app_id(app_id);
                }
            }
            #[cfg(not(windows))]
            {
                let body = [notice.subtitle.as_str(), notice.body.as_str()]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                notification
                    .appname("Alice")
                    .body(&body)
                    .auto_icon()
                    .hint(notify_rust::Hint::Category("email.arrived".into()))
                    .action(
                        "default",
                        &alice_core::t!(notice.locale, "desktop.notice.open"),
                    );
                if expires_at.is_some() {
                    notification.hint(notify_rust::Hint::Transient(true));
                }
            }
            let handle = match notification.show() {
                Ok(handle) => handle,
                Err(error) => {
                    if expires_at.is_some() {
                        eprintln!(
                            "{}",
                            alice_core::t!(notice.locale, "desktop.otp.notice_failed")
                        );
                    } else {
                        eprintln!(
                            "{}",
                            alice_core::t!(
                                notice.locale,
                                "desktop.notice.failed",
                                error = error.to_string()
                            )
                        );
                    }
                    return;
                }
            };
            let target = notice.target;
            #[cfg(not(windows))]
            if let Some(delay) = delay {
                tauri::async_runtime::spawn(async move {
                    tokio::select! {
                        () = tokio::time::sleep(delay) => {},
                        () = handle.wait_for_action_async(|response| {
                            match response {
                                notify_rust::NotificationResponse::Default => clicked(target.clone()),
                                notify_rust::NotificationResponse::Action(action) if action == "default" => {
                                    clicked(target.clone());
                                }
                                _ => {}
                            }
                        }) => {},
                    }
                    handle.close_async().await;
                });
                return;
            }
            #[cfg(windows)]
            {
                let _ = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
                    if matches!(response, notify_rust::NotificationResponse::Default) {
                        clicked(target.clone());
                    }
                });
            }
            #[cfg(not(windows))]
            handle.wait_for_action(|action| {
                if action == "default" {
                    clicked(target);
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::Target;

    #[test]
    fn a_clicked_notification_opens_what_it_announced() {
        for target in [Target::Message("a1:b2".into()), Target::Inbox] {
            assert_eq!(Target::from_identifier(&target.identifier()), target);
            assert_eq!(
                Target::from_identifier(&format!("otp:42:{}", target.identifier())),
                target,
            );
        }
    }
}
