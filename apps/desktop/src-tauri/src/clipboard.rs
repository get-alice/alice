//! Sensitive clipboard writes. Never retry through an ordinary, potentially syncing clipboard.
//! System exclusion flags do not prevent other local applications from reading clipboard text.

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyError {
    Unavailable,
    Expired,
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|time| i64::try_from(time.as_secs()).ok())
        .unwrap_or(i64::MAX)
}

pub(crate) fn lifetime(expires_at: i64) -> Result<Duration, CopyError> {
    let expires = u64::try_from(expires_at)
        .ok()
        .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
        .ok_or(CopyError::Expired)?;
    let remaining = expires
        .duration_since(SystemTime::now())
        .map_err(|_| CopyError::Expired)?;
    if remaining.is_zero() {
        return Err(CopyError::Expired);
    }
    Ok(remaining.min(Duration::from_secs(60)))
}
/// An explicit reader action may copy cached/older mail even with automatic copying disabled.
#[tauri::command]
pub async fn copy_message_otp(
    app: AppHandle,
    daemon: tauri::State<'_, crate::Daemon>,
    message_id: String,
) -> Result<String, String> {
    let locale = crate::desktop::locale(&app);
    let code = daemon
        .get::<Option<String>>(&format!("/v1/messages/{message_id}/otp"))
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| alice_core::t!(locale, "desktop.otp.not_found"))?;
    let expires_at = now()
        .checked_add(60)
        .ok_or_else(|| alice_core::t!(locale, "desktop.otp.copy_failed"))?;
    copy(&app, &code, expires_at)
        .await
        .map_err(|_| alice_core::t!(locale, "desktop.otp.copy_failed"))?;
    Ok(code)
}

/// Reports success only after the OS accepts the private write. The expiry is checked on the
/// writing thread, not just when the daemon batch was fetched. No earlier clipboard is read.
pub async fn copy(app: &AppHandle, code: &str, expires_at: i64) -> Result<(), CopyError> {
    if code.is_empty() || code.chars().any(char::is_control) {
        return Err(CopyError::Unavailable);
    }
    let code = code.to_owned();
    #[cfg(any(target_os = "macos", windows))]
    {
        let (send, receive) = tokio::sync::oneshot::channel();
        let handle = app.clone();
        app.run_on_main_thread(move || {
            let result = lifetime(expires_at).and_then(|_| {
                platform::write(&handle, &code).map(|generation| (generation, SystemTime::now()))
            });
            let _ = send.send(result);
        })
        .map_err(|_| CopyError::Unavailable)?;
        let (generation, copied_at) = receive.await.map_err(|_| CopyError::Unavailable)??;
        // Schedule against the actual write time, including time waiting for the main thread.
        let delay = lifetime(expires_at).unwrap_or(Duration::ZERO).min(
            Duration::from_secs(60).saturating_sub(copied_at.elapsed().unwrap_or(Duration::ZERO)),
        );
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(delay).await;
            let clear_handle = handle.clone();
            let _ = handle.run_on_main_thread(move || platform::clear(&clear_handle, generation));
        });
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        tokio::task::spawn_blocking(move || {
            lifetime(expires_at)?;
            platform::write(&code)
        })
        .await
        .map_err(|_| CopyError::Unavailable)?
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = (app, code, expires_at);
        Err(CopyError::Unavailable)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::CopyError;
    use objc2::{ClassType, msg_send, rc::Retained};
    use objc2_app_kit::{NSPasteboard, NSPasteboardContentsOptions};
    use objc2_foundation::NSString;
    use tauri::AppHandle;

    fn pasteboard() -> Result<Retained<NSPasteboard>, CopyError> {
        // AppKit can return nil in non-GUI sessions despite the nonnull API annotation.
        let board: Option<Retained<NSPasteboard>> =
            unsafe { msg_send![NSPasteboard::class(), generalPasteboard] };
        board.ok_or(CopyError::Unavailable)
    }

    pub fn write(_app: &AppHandle, code: &str) -> Result<isize, CopyError> {
        let board = pasteboard()?;
        // Apple's currentHostOnly contract explicitly disables Universal Clipboard propagation.
        // https://developer.apple.com/documentation/appkit/nspasteboard/contentsoptions/currenthostonly
        let generation =
            board.prepareForNewContentsWithOptions(NSPasteboardContentsOptions::CurrentHostOnly);
        // Conventional password-manager hints: advisory to third-party clipboard managers.
        for marker in [
            "org.nspasteboard.TransientType",
            "org.nspasteboard.ConcealedType",
        ] {
            if !board.setString_forType(&NSString::from_str(""), &NSString::from_str(marker)) {
                return Err(CopyError::Unavailable);
            }
        }
        if board.changeCount() != generation
            || !board.setString_forType(
                &NSString::from_str(code),
                &NSString::from_str("public.utf8-plain-text"),
            )
        {
            return Err(CopyError::Unavailable);
        }
        Ok(generation)
    }

    pub fn clear(_app: &AppHandle, generation: isize) {
        if let Ok(board) = pasteboard()
            && board.changeCount() == generation
        {
            board.prepareForNewContentsWithOptions(NSPasteboardContentsOptions::CurrentHostOnly);
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::CopyError;
    use std::ptr;
    use tauri::{AppHandle, Manager};
    use windows_sys::Win32::{
        Foundation::{GlobalFree, HGLOBAL, HWND},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
                RegisterClipboardFormatW, SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        },
    };

    struct Open;
    impl Open {
        fn acquire(app: &AppHandle) -> Result<Self, CopyError> {
            let window = app
                .get_webview_window("main")
                .ok_or(CopyError::Unavailable)?;
            let hwnd = window.hwnd().map_err(|_| CopyError::Unavailable)?;
            // A real HWND is required: OpenClipboard(NULL) + EmptyClipboard makes writes fail.
            if unsafe { OpenClipboard(hwnd.0 as HWND) } == 0 {
                return Err(CopyError::Unavailable);
            }
            Ok(Self)
        }
    }
    impl Drop for Open {
        fn drop(&mut self) {
            unsafe { CloseClipboard() };
        }
    }

    struct Memory(HGLOBAL);
    impl Memory {
        fn new<T: Copy>(data: &[T]) -> Result<Self, CopyError> {
            let memory = Self(unsafe { GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of_val(data)) });
            if memory.0.is_null() {
                return Err(CopyError::Unavailable);
            }
            let destination = unsafe { GlobalLock(memory.0) };
            if destination.is_null() {
                return Err(CopyError::Unavailable);
            }
            unsafe {
                ptr::copy_nonoverlapping(data.as_ptr(), destination.cast::<T>(), data.len());
                GlobalUnlock(memory.0);
            }
            Ok(memory)
        }
        fn transfer(mut self, format: u32) -> Result<(), CopyError> {
            if unsafe { SetClipboardData(format, self.0) }.is_null() {
                return Err(CopyError::Unavailable);
            }
            self.0 = ptr::null_mut(); // The OS owns this allocation after SetClipboardData succeeds.
            Ok(())
        }
    }
    impl Drop for Memory {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { GlobalFree(self.0) };
            }
        }
    }

    pub fn write(app: &AppHandle, code: &str) -> Result<u32, CopyError> {
        // Register and allocate everything before touching the user's clipboard. Exclusion data
        // must be installed before text, and failure never falls back to an unprotected write.
        // https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats
        let name: Vec<u16> = "ExcludeClipboardContentFromMonitorProcessing"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let exclusion = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
        if exclusion == 0 {
            return Err(CopyError::Unavailable);
        }
        let marker = Memory::new(&[0u32])?;
        let text: Vec<u16> = code.encode_utf16().chain(Some(0)).collect();
        let text = Memory::new(&text)?;
        let _open = Open::acquire(app)?;
        if unsafe { EmptyClipboard() } == 0 {
            return Err(CopyError::Unavailable);
        }
        marker.transfer(exclusion)?;
        text.transfer(13)?; // CF_UNICODETEXT
        // Capture while holding the OS clipboard lock, never after another writer can take it.
        Ok(unsafe { GetClipboardSequenceNumber() })
    }

    pub fn clear(app: &AppHandle, generation: u32) {
        if let Ok(_open) = Open::acquire(app)
            && generation != 0
            && unsafe { GetClipboardSequenceNumber() } == generation
        {
            unsafe { EmptyClipboard() };
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::CopyError;
    use arboard::{Clipboard, SetExtLinux};
    use std::sync::{LazyLock, Mutex};

    // X11 selection ownership disappears when the owner is dropped. Keep it alive while Alice
    // runs; wayland-data-control permits writes without focusing a surface on supported compositors.
    static CLIPBOARD: LazyLock<Mutex<Option<Clipboard>>> = LazyLock::new(|| Mutex::new(None));

    pub fn write(code: &str) -> Result<(), CopyError> {
        // Arboard normally falls back to X11 when Wayland data-control is unavailable. Reject an
        // unsupported compositor explicitly rather than reporting success on another selection.
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            wl_clipboard_rs::utils::is_primary_selection_supported()
                .map_err(|_| CopyError::Unavailable)?;
        }
        let mut current = CLIPBOARD.lock().map_err(|_| CopyError::Unavailable)?;
        if current.is_none() {
            *current = Some(Clipboard::new().map_err(|_| CopyError::Unavailable)?);
        }
        current
            .as_mut()
            .ok_or(CopyError::Unavailable)?
            .set()
            .exclude_from_history()
            .text(code)
            .map_err(|_| CopyError::Unavailable)
    }
    // Do not call arboard::clear: both backends clear unconditionally, and a text comparison
    // followed by clear can destroy a later owner's content. Linux has no safe automatic clear
    // through this API. The KDE secret hint is advisory; other managers may ignore it.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_deadline_never_outlives_the_code_or_sixty_seconds() {
        assert_eq!(lifetime(now() + 600).unwrap(), Duration::from_secs(60));
        let short = lifetime(now() + 10).unwrap();
        assert!(short <= Duration::from_secs(10) && !short.is_zero());
        assert_eq!(lifetime(now() - 1), Err(CopyError::Expired));
    }
}
