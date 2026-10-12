# Platform integration, security, contacts and calendar

Research behind the matching items in [`todo.md`](../../todo.md). Each entry covers why it matters, how Mailspring and other clients do it, the relevant specs, what Alice has today, a build plan, pitfalls, acceptance criteria and effort. Code citations point at the Alice working tree as of 2026-10-13; Mailspring paths are relative to [Foundry376/Mailspring](https://github.com/Foundry376/Mailspring) (`mailspring/`) and [Foundry376/Mailspring-Sync](https://github.com/Foundry376/Mailspring-Sync) (`mailsync/`).

Sections covered:
- P1 → Platform integration
- P1 → Security
- P2 → Contacts and calendar
- New P2 notification and settings items requested by the lead

Sources:
- Alice HEAD (read-only).
- Mailspring source on main as of 2026-10-12 (`repos/mailspring`, `repos/mailsync`), cross-checked against the installed 1.26.0 bundle.
- The `ms.md` changelog and Mailspring forum and GitHub demand lists.
- Tauri plugin sources and documentation.
- RFCs and provider documentation, linked inline.

## Section findings

- **Three items are stale or mostly done.**
  - **The Google OAuth secret item.** HEAD has no bundled secret: it is bring-your-own, stored per account in the keychain. The real work is shipping a built-in client safely. The long pole is Google's restricted-scope verification for `mail.google.com` (yearly security assessment), not code.
  - **"Block remote CSS and web fonts."** The reader frame's CSP already blocks `@import`, external sheets and web fonts at all times. Only a regression test is missing.
  - **"Respect Do Not Disturb".** Alice posts native OS notifications with the OS sound, which every desktop already mutes under DND or Focus. Mailspring needed detection only because it plays its own audio, and that detection caused two outages (1.10.3, 1.15.1).
- **Build one desktop entry-point layer first.**
  - Components:
    - `tauri-plugin-single-instance`.
    - argv parsing plus macOS `RunEvent::Opened`.
    - A generalised version of the existing `Desktop.opened` / `take_opened` handoff (`apps/desktop/src-tauri/src/desktop.rs:50-63,446-454`).
  - It serves `mailto:` links, notification actions (including Windows protocol activation), start-hidden at login, and relaunch after updates.
  - It also fixes a current bug: a second launch starts a second GUI process that polls arrivals and posts duplicate notifications.
  - Keep platform code native per OS, as `notification.rs` already does. Avoid `tauri-plugin-deep-link`: its Windows installer template rewrites `HKLM\Software\Classes\mailto`, and on Linux `register()` silently makes the app the default.
  - Plugins worth adding:
    - single-instance
    - autostart (Windows and Linux)
    - updater + process
  - Collect default mail app, open at login, start hidden and updates under a new Settings › System section.
- **Release engineering gates half of Platform integration.**
  - The updater needs a minisign key, updater artifacts and a `latest.json` in CI.
  - `mailto` registration needs NSIS hooks and a custom `.desktop` template (`Exec=… %u`).
  - Flatpak needs portals for autostart, and the Linux badge needs a stable `.desktop` ID.
  - Mac App Store and Flatpak builds must compile out self-update and "set as default". Decide these alongside P3 "Linux packaging".
- **Security: the reader is sound; the risk moves to new surfaces.** The HTML composer, forward, print, show original and the clip popout are all still to be built.
  - Make `emailDocument` (`apps/desktop/src/emailHtml.ts:34`) the only route by which message HTML reaches a document.
  - Add a `quoteHtml` for the composer that parks remote URLs in `data-` attributes.
  - Consider serving the frame from a custom protocol, so the main window CSP can drop `img-src https:`.
  - New finding: DAV Basic credentials follow redirects to any HTTPS host (`crates/alice-core/src/dav.rs:229-260`).
- **Contacts and calendar: Alice's "no local copy" design is the blocker.**
  - Contacts are flattened to `(owner, source, email, name)` (`crates/alice-core/src/store.rs:26`), and calendars are pass-through (`crates/alice-core/src/calendar.rs:1-3`).
  - The editor, vCard export, richer sidebar and calendar views all need a local cache that keeps provider identity: href or resourceName, etag, sync token and the raw card or event.
  - Recommended order:
    1. CalDAV presets (S)
    2. DAV redirect fix (S)
    3. Address-book cache with incremental sync (L)
    4. Contact sidebar (M)
    5. Contact write-back (L)
    6. vCard import and export (M)
    7. Calendar cache and read-only views as the MVP (L + L)
    8. Calendar editing (L)
    9. Invitations and reminders (M)
- **Calendar is Mailspring's most requested feature ever** (GitHub #199, 497 👍). Mailspring still labels it "Preview" in 1.26.0, behind the Developer menu, and it has no Microsoft Graph or Exchange support at all. Alice's existing Google, Graph and CalDAV plumbing is a real head start.
- **Scopes.**
  - Writing contacts needs Google `contacts` and Graph `Contacts.ReadWrite`. Both are sensitive, not restricted, and can go through the existing re-consent endpoint.
  - The calendar write scopes are already granted.
- **Crates.**
  - Already in the tree; reuse them:
    - `zbus`, through notify-rust, for Linux badges.
    - `percent-encoding`, through `url`, for mailto parsing.
    - `objc2-*` and `windows-sys`, adding features.
    - `open`, for `ms-settings:` links.
  - New:
    - `tauri-plugin-single-instance`, `-autostart`, `-updater` and `-process`.
    - `objc2-service-management`.
    - Optionally `ashpd` (Flatpak).
    - Optionally `calcard` (Stalwart; Apache-2.0/MIT; vCard and iCalendar parse/write plus RRULE expansion).

## P1 → Platform integration

<a id="register-as-mailto-handler-and-offer-make-default-mail-client"></a>

### Register as `mailto:` handler and offer "make default mail client"

*Roadmap: P1 · Platform integration*

- **Why:** Clicking an email link in a browser or a PDF should open Alice's composer. Until that works, people switching to Alice keep their old client installed just for this.
- **Competitors:**
  - **Mailspring** registers its handler per OS:
    - **Linux:** a `.desktop` file with `MimeType=x-scheme-handler/mailto;` and a "New Message" action (`repos/mailspring/app/build/resources/linux/Mailspring.desktop.in`).
    - **Windows:** a registry file writes the ProgID `Mailspring.Url.mailto`, `Clients\Mail\Mailspring\Capabilities\URLAssociations`, and `RegisteredApplications` (`app/build/resources/win/mailspring-mailto-registration.reg`). These keys are written on first launch (`app/src/browser/main.js:317-325`).
    - **macOS:** Electron's `setAsDefaultProtocolClient`.
  - **Mailspring's "make default" offer.** A one-time banner offers to make Mailspring the default (`app/internal_packages/notifications/lib/items/default-client-notif.tsx`, config key `mailto.prompted-about-default`). Preferences › General has a "Use Mailspring as default mail client" checkbox (`internal_packages/preferences/lib/tabs/workspace-section.tsx:30-83`). Per OS (`app/src/default-client-helper.ts`):
    - **macOS:** `app.setAsDefaultProtocolClient('mailto')`.
    - **Linux:** `xdg-mime default <desktop> x-scheme-handler/mailto`, checked with `xdg-mime query default`. Disabled under Snap. If `xdg-utils` is missing it shows an error.
    - **Windows:** reads the `UserChoice` keys to check. Since 1.17.0 it opens `ms-settings:defaultapps?registeredAppUser=Mailspring` (`:46-55`).
  - **Mailspring parsing** (`app/src/flux/stores/draft-factory.ts:93-160`):
    - Splits on `subject|body|cc|to|from|bcc` and tolerates malformed links that contain unescaped `?` and `&`.
    - 1.10.0 fixed multiple `;`-separated Cc/Bcc addresses.
    - 1.17.0 fixed Windows argv parsing.
    - 1.21.0 removed `?attach=` (#2703). It let a web page attach local files.
    - 1.21.0 also limited path traversal in the protocol handler (#2700).
    - 1.26.0 ignores malformed launch URLs (#2913).
  - **Thunderbird and Apple Mail** each have a "default email reader" setting. In Apple Mail it is the only supported way to change the default on macOS.
- **Spec:**
  - RFC 6068:
    - §2: `to` is a comma-separated list in the path and/or `?to=`; header-field names are case-insensitive; `body` is text/plain with `%0D%0A` line breaks.
    - §3–4: only Subject, Keywords and Body are "safe". Originator, routing, trace and MIME fields "MUST be ignored".
    - §7: "SHOULD NOT send … without first disclosing and showing to the user the full message".
  - [Windows Default Programs](https://learn.microsoft.com/en-us/windows/win32/shell/default-programs) and [launching Default Apps settings](https://learn.microsoft.com/en-us/windows/apps/develop/launch/launch-default-apps-settings).
  - macOS: `NSWorkspace setDefaultApplicationAtURL:toOpenURLsWithScheme:completionHandler:` (12+).
  - freedesktop mime-apps spec (`mimeapps.list`).
- **Alice today:** Nothing.
  - No URL scheme in `apps/desktop/src-tauri/Info.plist`.
  - No plugins in `apps/desktop/src-tauri/Cargo.toml`.
  - `main()` only checks for `--daemon` (`apps/desktop/src-tauri/src/main.rs:163-174`).
  - There is no single-instance guard, so a second launch starts a second GUI process. That second process also polls `/v1/arrivals` and would post duplicate notifications (`apps/desktop/src-tauri/src/desktop.rs:78-121`).
  - The compose model has only `to` (`crates/alice-core/src/model.rs:222-229`).
  - There is a reusable pattern for handing things to the page: `Desktop.opened` plus `take_opened` (`desktop.rs:50-63`, `446-454`).
- **Plan:**
  1. **Parser.** Add `crates/alice-core/src/mailto.rs` with `parse(&str) -> MailtoDraft { to, cc, bcc, subject, body }`.
     - Percent-decode with the `percent-encoding` crate. It is already in `Cargo.lock` through `url`. Do not use form decoding: `+` is literal in RFC 6068.
     - Split addresses on `,` and also on `;` (Mailspring 1.10 compatibility). Merge path addresses with `?to=`.
     - Keep Mailspring's tolerance for an unescaped `&` inside `body`.
     - Ignore every other header field, including `attach`, `from` and `in-reply-to`.
     - Cap lengths, strip control characters except CRLF in the body, and validate addresses.
     - Because it lives in `alice-core`, the CLI can offer `alice compose 'mailto:…'` with the same parser.
  2. **Delivery into the app.**
     - Add `tauri-plugin-single-instance` 2.5. Register it first, and only in the GUI path (never under `--daemon`). Its callback receives argv on Windows and Linux.
     - On macOS, add `CFBundleURLTypes` (`mailto`) to `apps/desktop/src-tauri/Info.plist` and handle `RunEvent::Opened { urls }` in `desktop::on_run_event` (`desktop.rs:476-487`).
     - Parse `std::env::args()` on cold start as well.
     - Generalise `Opened` into `enum Opened { Message(Option<String>), Compose(MailtoDraft) }`, so a link that launches Alice is not lost before the page loads. The page then opens Compose through `createCompose` (`apps/desktop/src/App.tsx:207`).
     - Skip `tauri-plugin-deep-link`. Its NSIS/WiX templates write `HKLM\Software\Classes\mailto`, which overwrites the system's own mailto class and is deleted on uninstall. Its Linux `register()` silently runs `xdg-mime default`.
  3. **Registration at install.**
     - **Windows:** use NSIS hooks (`bundle.windows.nsis.installerHooks`) to write the ProgID `Alice.mailto` with `shell\open\command = "<exe>" "%1"`, plus `Software\Alice\Capabilities` (`ApplicationName`, `ApplicationDescription` — without this Alice does not appear in the list), `URLAssociations\mailto=Alice.mailto` and `RegisteredApplications\Alice`. Then call `SHChangeNotify(SHCNE_ASSOCCHANGED)`.
     - **Linux:** use a custom `desktopTemplate` with `MimeType=x-scheme-handler/mailto;` and `Exec=alice %u`. The default template has no `%u`.
  4. **"Make Alice the default" button** in a new Settings › System section (`apps/desktop/src/Settings.tsx:16-18`), backed by a Tauri command `default_mail_app(get|set)` in a new `src-tauri/src/default_app.rs`:
     - **macOS:** read with `NSWorkspace.URLForApplicationToOpenURL(mailto:)` and set with `setDefaultApplicationAtURL…`. Needs objc2-app-kit features `NSWorkspace`, `block2` and objc2-foundation `NSURL`.
     - **Windows:** read with `AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_PROGID, "mailto")`, which needs windows-sys feature `Win32_UI_Shell`. Set by opening `ms-settings:defaultapps?registeredAppUser=Alice` with `open::that`, which is already a dependency, and fall back to `ms-settings:defaultapps`.
     - **Linux:** `xdg-mime query/default <app>.desktop x-scheme-handler/mailto`. Under Flatpak or Snap, show instructions instead.
     - Show a one-time prompt after the first account connects (stored in `Preferences`), mirroring Mailspring's banner.
  5. Never auto-send from a link. The composer always opens for review.
- **Pitfalls:**
  - A web page controls the whole URL. Treat it as hostile input: no attachments, no From, no arbitrary headers, and bounded sizes.
  - Bcc in a URL is visible to anyone who can see the link (RFC 6068 §7).
  - Windows passes the URL as one argument. Quoting bugs broke Mailspring until 1.17.0.
  - On macOS, a cold launch delivers `Opened` before the webview is ready. The `take_opened` handoff handles this.
  - Apple DTS reports that setting the default fails from a sandboxed (Mac App Store) build. Hide the button there.
  - Whether macOS asks the user for consent when the mailto default changes is unverified. Test it.
  - On Linux, `xdg-mime` may be missing (Mailspring shows a dedicated error).
  - Cc/Bcc from a link have nowhere to go until the P0 Cc/Bcc fields exist. Until then, show "Cc/Bcc from link not supported yet" rather than dropping them silently.
- **Done when:**
  - Unit tests in `mailto.rs` pass for:
    - `mailto:a@x,b@x?cc=c@x;d@x&subject=Hi%20there&body=line1%0D%0Aline2`
    - an encoded `+`
    - a malformed `body=https://x/?a=1&b=2&subject=S`
    - `?attach=/etc/passwd` and `?from=` being ignored
    - an IDN domain
    - a 1 MB body being capped
  - A second launch with a `mailto:` argument focuses the running window with Compose prefilled, on all three OSes.
  - The Settings button reports the correct state after you change the default by hand.
- **Effort:** M. Depends on "Cc and Bcc fields". Shares the single-instance work with "Notification actions" and "Launch at login".

<a id="dock-taskbar-unread-badge-macos-windows-kdeunity"></a>

### Dock / taskbar unread badge (macOS, Windows, KDE/Unity)

*Roadmap: P1 · Platform integration*

- **Why:** It shows at a glance whether there is new mail, without opening the window. People coming from Mailspring, Apple Mail or Outlook expect it.
- **Competitors:**
  - **Mailspring `BadgeStore`** (`repos/mailspring/app/src/flux/stores/badge-store.ts`):
    - Counts unread or total Inbox threads. The setting `core.notifications.countBadge` is `hide` / `unread` (default) / `total`.
    - By default it counts only the focused account. `countBadgeAllAccounts` widens it (`app/src/config-schema.ts:370-386`).
    - Counts are capped at "999+".
  - **Mailspring per platform:**
    - macOS uses `app.dock.setBadge`.
    - Linux uses `app.setBadgeCount` (`app/src/browser/application.ts:609-614`). 1.23.0 extended this to KDE and other non-Unity desktops (#2756) and sends `org.freedesktop.DBus.Hello` first (#2778).
    - Windows has taskbar overlay icons since 1.17.4 (#2594). A 16×16 blue badge is drawn in BGRA (`app/src/browser/windows-taskbar-manager.ts:77-170`).
  - **User demand:**
    - Forum #831: "Badge only shows unread count for 1 email account". The default is confusing.
    - Forum #762: Windows dock badge.
    - Forum #1119: a more readable Windows tray badge.
- **Spec:**
  - Tauri 2.2+: `Window::set_badge_count` (macOS dock; Linux only via libunity when Unity is running), `set_badge_label` (macOS) and `set_overlay_icon` (Windows). Permissions: `core:window:allow-set-badge-count` and `core:window:allow-set-overlay-icon`. Alice calls these from Rust, so no capability change is needed.
  - The Unity LauncherEntry D-Bus signal `com.canonical.Unity.LauncherEntry.Update(app_uri "application://<desktop-id>", {"count": x, "count-visible": b})` is honoured by KDE Plasma, Ubuntu Dock / dash-to-dock and Plank.
- **Alice today:**
  - Nothing on the dock or taskbar.
  - `/v1/state` already returns `unread_count`, counted in Inbox conversations across all accounts (`crates/alice-core/src/daemon.rs:366-383`).
  - The desktop polls every 5 seconds but reads only `/v1/preferences` and `/v1/arrivals` (`apps/desktop/src-tauri/src/desktop.rs:78-121`).
  - Notification permission is requested without the badge option (`apps/desktop/src-tauri/src/notification.rs:159-162`).
  - Tauri 2.12.3 is already locked, so the badge APIs are available.
- **Plan:**
  1. Add `Preferences.badge: Badge { Off, Unread, Total }` (default `Unread`, counted across all accounts) in `model.rs:313-360` plus `PreferencesPatch`, `store.rs` save, and a toggle in Settings › Notifications (`Settings.tsx:144-147`).
  2. In `desktop::watch`, read the count each tick. Prefer a cheap `GET /v1/counts` that runs a `COUNT` on the indexed `message_index`. `/v1/state` calls `store.threads()`, which is heavy until the P0 SQL paging lands.
  3. Add `desktop::set_badge(app, n)`:
     - **macOS:** `window.set_badge_count(Some(n))`. Also add `UNAuthorizationOptions::Badge` to the permission request at `notification.rs:160`.
     - **Windows:** `window.set_overlay_icon(Some(img))`, using 10 pre-rendered 16×16 PNGs (1–9, "9+") through `include_image!`, with the count in the tooltip. This avoids a font rasterizer.
     - **Linux:** emit the LauncherEntry signal with `zbus`, which is already in the lock file via notify-rust. Make it a direct dependency with the `tokio` feature. The desktop id is `<productName>.desktop`. Don't rely on Tauri's libunity path, which does nothing on KDE, Fedora or Arch.
  4. Only update when the value changes, and clear it with `None` / `count-visible=false` at 0 or when the badge is turned off.
  5. Optionally show the count in the tray tooltip (`desktop.rs:336-338`).
- **Pitfalls:**
  - Thread-based and message-based counts disagree. Use conversations, matching the Unread tab (`daemon.rs:373`).
  - The count must update after reads elsewhere, so the 5-second polling is fine.
  - Flatpak needs `--talk-name=com.canonical.Unity` only if the dock filters by name. The signal is a broadcast, so it usually works.
  - The Windows overlay belongs to a window's taskbar button. While Alice is hidden to the tray there is no button, so put the count in the tray tooltip too.
  - Electron documents that the macOS badge needs notification permission.
- **Done when:**
  - Unit tests cover the badge formatting function (0 → none, 1000 → "999+") and the Windows image selection.
  - A daemon test shows `/v1/counts` matches the Unread tab count.
  - Manual checks pass on macOS Dock, Windows 11 taskbar, KDE Plasma and Ubuntu Dock.
- **Effort:** M. Benefits from "Page the thread list in SQL" and "Unread counts on every folder".

<a id="launch-at-login-launchagent-windows-run-key-xdg-autostart-flatpak-portal"></a>

### Launch at login (LaunchAgent, Windows Run key, XDG autostart / Flatpak portal)

*Roadmap: P1 · Platform integration*

- **Why:** Alice only notifies, copies OTP codes and syncs in the background while it is running. Without autostart, a reboot silently stops new-mail notifications.
- **Competitors:**
  - **Mailspring** has "Launch on system start" in Preferences › General (`internal_packages/preferences/lib/tabs/workspace-section.tsx:89-141`, `app/src/system-start-service.ts`):
    - **macOS:** Electron `setLoginItemSettings({openAtLogin})`. It deletes the legacy `~/Library/LaunchAgents/com.mailspring.plist` (`:26-61`; 1.17.0 #2509 fixed creating and deleting it). Login launches are detected with `wasOpenedAtLogin` and start in the background (`app/src/browser/main.js:347-351`).
    - **Windows:** a Squirrel `--processStart … --process-start-args "--background"` Run entry.
    - **Linux:** copies the `.desktop` file into `~/.config/autostart` with `--background %U`. Under Flatpak it uses `org.freedesktop.portal.Background.RequestBackground` (1.24.0 #2838).
  - **Mailspring fixes:**
    - 1.9.0 made the toggle state persistent (#2265).
    - 1.9.2 fixed background start on macOS.
    - 1.18.0 made Wayland `--background` show the window briefly and then hide it (#2623).
    - GitHub #848: Snap autostart was broken.
- **Spec:**
  - `tauri-plugin-autostart` 2.7 (auto-launch 0.6). Its `MacosLauncher` is `LaunchAgent` or `AppleScript`. The plugin does not yet expose SMAppService (plugins-workspace#2720).
  - `SMAppService.mainAppService` (macOS 13+, `objc2-service-management` 0.3).
  - [`org.freedesktop.portal.Background`](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Background.html) `RequestBackground {autostart, commandline}`. The `ashpd` crate wraps it.
- **Alice today:**
  - Not implemented. README "Current boundaries" says "starting Alice at login [is] not included yet" (`README.md:297`).
  - The tray and close-to-hide behaviour already exist (`desktop.rs:285-363`, `464-474`).
  - The daemon is started on demand by the app or CLI (`crates/alice-core/src/client.rs:99-150`).
- **Plan:**
  1. Add `tauri-plugin-autostart` with `Builder::new().args(["--hidden"]).app_name("Alice")` and `MacosLauncher::LaunchAgent`, the plugin default. Note that LaunchAgent shows up under "Allow in the Background" on macOS 13+, with a "Background Items Added" notice.
     - Better macOS UX: call `SMAppService::mainAppService().registerAndReturnError()` directly through `objc2-service-management` (features `SMAppService`). It appears as a proper Login Item and offers `openSystemSettingsLoginItems()` when status is `RequiresApproval`. On macOS, skip the plugin and use SMAppService. The plugin is still needed on Windows and Linux.
  2. **Flatpak:** detect `FLATPAK_ID` and use `ashpd` `Background::request().auto_start(true).command(["alice","--hidden"])`. This is a new dependency, so feature-gate it for the Flatpak build from the P3 "Linux packaging" item.
  3. Store the choice in the OS, not in `Preferences`. The OS state is the truth, because the person can remove it in System Settings or Task Manager. Read `is_enabled()` when Settings opens.
  4. UI: Settings › System gets "Open Alice at login", plus the start-hidden option from the (NEW) item below.
  5. **Daemon-only option (later):** a LaunchAgent, Run key or systemd user unit for `alice --daemon` alone, for CLI/TUI-only users. Defer it, and say so in the README.
- **Pitfalls:**
  - Windows' `StartupApproved\Run` disable toggle overrides the Run key. `is_enabled` must check both, which auto-launch does.
  - AppImage paths change on update. auto-launch uses `$APPIMAGE`.
  - Snap needs `autostart:` in `snapcraft.yaml` (GitHub #848).
  - Never enable autostart without consent; Mac App Store guideline 2.4.5(iii) forbids it.
  - Login launches must not steal focus (see the next item).
- **Done when:**
  - Toggling on writes the plist / Run value / `.desktop` file (or registers through the SMAppService or portal), and toggling off removes it.
  - Settings shows the OS state after an external change.
  - A manual reboot test on each OS confirms Alice starts and posts notifications.
- **Effort:** S–M. Pairs with "(NEW) Start hidden at login".

<a id="real-auto-update-download-verify-signature-install-tauri-updater"></a>

### Real auto-update: download, verify signature, install (Tauri updater)

*Roadmap: P1 · Platform integration*

- **Why:** Security fixes, such as the ones in this section, only matter once they are installed. Today Alice can only say that an update exists.
- **Competitors:**
  - **Mailspring** (`repos/mailspring/app/src/browser/autoupdate-manager.ts`):
    - Feed: `https://updates.getmailspring.com/check/{platform}/{arch}/{version}/{id}/{channel}` checked at startup and every 30 minutes. It sends the Mailspring ID, a privacy cost.
    - It moves Intel-on-Apple-Silicon users to arm64.
    - Windows uses Squirrel (`autoupdate-impl-win32.ts`) and macOS uses Electron's `autoUpdater`.
    - Linux only links to the download page (`autoupdate-impl-base.ts`), and is off under Snap.
  - **Mailspring fixes:**
    - 1.0.12: auto-update for non-admin users on macOS.
    - 1.6.1: a crash on "Check for Updates".
    - 1.19.0: Squirrel restart (#2631).
    - 1.20.0: error handling (#2670).
    - 1.21.0: URL validation in the update handler (#2697).
  - **Forum #542:** the update banner is annoying on distro builds that lag behind.
- **Spec:** [tauri-plugin-updater 2.13](https://v2.tauri.app/plugin/updater/):
  - minisign signatures: `pubkey` goes in config; `TAURI_SIGNING_PRIVATE_KEY` (and its password) are set in CI.
  - `bundle.createUpdaterArtifacts: true`.
  - Static `latest.json` with `{version, notes, pub_date, platforms{"darwin-aarch64"|"windows-x86_64-nsis"|"linux-x86_64-appimage"…: {url, signature}}}`.
  - Updatable formats: macOS `.app.tar.gz`, Windows NSIS/MSI (`installMode` passive/basicUi/quiet), and Linux AppImage. deb and rpm are installed through `pkexec` since 2.10.
  - `tauri-plugin-process` handles relaunch.
- **Alice today:**
  - The daemon only *checks*. `ALICE_RELEASE_FEED` must be set or it returns "unavailable".
  - The feed is `{version,url}`: HTTPS only, no redirects, a 64 KB cap, and a semver compare (`crates/alice-core/src/updates.rs:8-66`), served at `GET /v1/updates` (`daemon.rs:260`).
  - Settings › About shows the result (`apps/desktop/src/Settings.tsx:88`, `api.ts:58`).
  - CI signs and notarises macOS builds (`.github/workflows/ci.yml:90-136`), but there are no updater artifacts or signing key.
  - The daemon is replaced automatically when its build differs from the app's (`crates/alice-core/src/client.rs:66-121`).
- **Plan:**
  1. Generate a minisign key pair with `tauri signer generate`. Put the private key and password in CI secrets and the public key in `tauri.conf.json` `plugins.updater.pubkey`. Set `createUpdaterArtifacts: true`.
  2. **Release CI** (`ci.yml`) builds macOS universal or per-arch `.app.tar.gz`, Windows NSIS, and Linux AppImage plus deb. It signs them and publishes `latest.json` to GitHub Releases. The endpoint is `https://github.com/<org>/alice/releases/latest/download/latest.json`.
  3. In the desktop shell, add `tauri-plugin-updater` and `tauri-plugin-process`, and drive them from Rust (no JS plugin needed):
     - **Check** at startup plus every 6 hours. Show a non-modal "Alice X is ready — Restart" row in Settings › About, plus a tray menu item.
     - **Download** in the background with progress.
     - **Install**, using an `on_before_exit` hook that calls `POST /v1/shutdown` (`daemon.rs:261`) and waits on `daemon.lock`, exactly as `client.rs:108-118` does. That avoids NSIS force-killing the daemon in the middle of a send.
     - **Relaunch.** The new app's `Client::connect` then starts the matching daemon.
  4. Point `updates.rs` (used by the CLI/TUI and the web UI) at the same `latest.json`. It reads `version` plus a new top-level `url` (release page) that Tauri ignores. Ship a default feed URL instead of requiring `ALICE_RELEASE_FEED`.
  5. Disable self-update when `FLATPAK_ID` is set, under Snap, for Mac App Store builds (Cargo feature `self-update`, off for MAS), and for distro packages (`__TAURI_BUNDLE_TYPE` unset). Show "Updates come from your package manager" instead.
  6. Add channels later (stable/beta) through the endpoint path, without the user ID Mailspring sends.
- **Pitfalls:**
  - **Windows:**
    - NSIS passive mode force-closes every process running `alice.exe`, including the daemon. That is why the graceful shutdown step is mandatory.
    - Per-user versus per-machine installs need the matching `installMode`.
  - **macOS:**
    - Replacing the bundle needs write access to `/Applications/Alice.app` (Mailspring 1.0.12 non-admin issue). The updater falls back to asking for permission.
    - A daemon left running from the old bundle keeps its old code until it is replaced. The BUILD handshake covers this.
  - **CLI/daemon version fights:** a Homebrew or cargo CLI at an older build will replace the desktop's daemon and vice versa (`client.rs:105-121`). Document this, or make the CLI defer to a newer daemon.
  - **Signing key:** losing it strands every installed copy. Back it up offline.
  - **Downgrades:** the semver compare must not accept them; keep `allowDowngrades` false.
  - **Linux:** don't offer the update banner on distro builds (forum #542).
- **Done when:**
  - CI produces signed artifacts and a `latest.json` that `tauri signer verify` accepts.
  - An end-to-end test with a local HTTPS feed (as in the `updates.rs` tests) updates from 0.1.0 to 0.1.1 on each OS without losing an in-progress send. Check the `deliveries` table status after restart.
  - The updater is compiled out of the MAS and Flatpak builds.
- **Effort:** L (mostly CI and release engineering). Depends on "Linux packaging: Flatpak, Snap, deb, rpm, AppStream metadata; Wayland and tray fixes" for channel decisions, and on code signing for Windows.

<a id="follow-system-lightdark-theme"></a>

### Follow system light/dark theme

*Roadmap: P1 · Platform integration*

- **Why:** Most people run "Auto" appearance. An app stuck in light mode at night is the first thing they notice.
- **Competitors:**
  - Mailspring 1.20.0 added an "Automatic" theme (`app/internal_packages/ui-automatic`) and made it the default for new users (`app/src/theme-manager.ts:14,98`). It follows `nativeTheme.shouldUseDarkColors`. 1.20.0 also added the OS accent colour (#2682).
  - Mailspring 1.22.0 lets you pick which light and dark themes "Automatic" uses (#2732).
  - Mailspring 1.24.0 and 1.24.1 fixed dark-mode body rendering (#2850).
  - GitHub #1748 "Automatic Dark theme".
- **Spec:**
  - CSS `prefers-color-scheme` and the `color-scheme` property.
  - Tauri `Window::set_theme(Option<Theme>)` / `AppHandle::set_theme` (`None` = follow the system), `WindowEvent::ThemeChanged`, and window config `theme` omitted.
  - On Linux, Tauri ≥ 2.11 reads the XDG portal `color-scheme` through its default `dbus` feature, which Alice keeps because it doesn't disable default features.
- **Alice today:**
  - `Theme` is `Light | Dark`, default Light (`crates/alice-core/src/model.rs:231-237`, `342`).
  - The page sets `data-theme` from the preference (`apps/desktop/src/App.tsx:93`, `Settings.tsx:72-103`).
  - CSS defines both palettes with `color-scheme` (`apps/desktop/src/styles.css:9`, `36-37`).
  - The window background is hard-coded light, `#f7f7f9` (`tauri.conf.json:21`, `index.html:6`).
  - Settings offers two buttons (`Settings.tsx:136`) in a two-column grid (`styles.css:535`).
- **Plan:**
  1. Add `Theme::System` (`#[serde(rename_all="snake_case")]` → `"system"`) and make it the default for new installs. Existing saved `light`/`dark` values are kept.
  2. Frontend: create `resolvedTheme()` with `matchMedia('(prefers-color-scheme: dark)')` and a `change` listener, and set `data-theme` to the resolved value in `App.tsx:93` and `Settings.tsx:103`. This works in the browser build too.
  3. Desktop: in `desktop::apply` (`desktop.rs:256-274`), call `app.set_theme(None)` for System, or `Some(Light|Dark)` for an explicit choice. Native chrome (title bar, scrollbars, context menus, `<select>` popups) then matches.
  4. Avoid a white flash at startup:
     - Add `<style>@media (prefers-color-scheme: dark){html{background:#…}}</style>` and a dark `theme-color` media variant to `index.html`.
     - Drop the fixed `backgroundColor` or set it in `setup` from the window's `theme()`.
  5. Settings: three choices (System, Light, Dark) with a Monitor icon, `grid-template-columns: repeat(3, …)` and new strings in `apps/desktop/src/locales/{en-US,de}/settings.json`.
  6. Keep the reader's light message card in dark mode (`styles.css:270-271`). Smarter inversion is the separate P1 Reading item.
- **Pitfalls:**
  - **Linux:**
    - `set_theme(None)` reports Light until the next portal change.
    - `prefers-color-scheme` is not propagated on some GNOME 46 setups (tauri#9255).
    - Without the portal, it falls back to light.
    - Rely on `matchMedia` in the page and treat `ThemeChanged` as a hint.
  - The View Transition theme cross-fade (`Settings.tsx:71-76`) should not run on automatic OS switches, only on clicks.
  - The TUI follows the terminal and is unaffected (`crates/alice/src/tui.rs:163`).
- **Done when:**
  - A unit test shows that deserializing old preferences keeps `light`/`dark`, and that new preferences default to `system`.
  - Manual: toggling OS appearance updates Alice live on macOS, Windows 11 and GNOME/KDE, with no restart and no flash at launch in dark mode.
- **Effort:** S. No dependencies.

<a id="respect-do-not-disturb-focus"></a>

### Respect Do Not Disturb / Focus

*Roadmap: P1 · Platform integration · done*

- **Note:** Mostly satisfied by design. Alice posts every banner through the OS notification service and uses the OS sound: `UNNotificationSound::defaultSound()` on macOS (`apps/desktop/src-tauri/src/notification.rs:172`), and notify-rust toasts and XDG notifications elsewhere (`notification.rs:229-267`). macOS Focus, Windows Do Not Disturb / Focus Assist, and GNOME and KDE DND already silence and queue these. Mailspring needed detection only because it plays its own HTML5 "new mail" sound (`repos/mailspring/app/src/registries/sound-registry.ts:7-13`). The remaining work is to keep it that way, add a test matrix, and make a deliberate choice about OTP notices. Re-scope to S.
- **Why:** A notification or chime during a presentation or a Focus session is the classic reason people turn notifications off entirely.
- **Competitors:**
  - **Mailspring:**
    - 1.5.3 honoured macOS DND and Windows Quiet Time.
    - 1.10.3 and 1.15.1 fixed Windows quiet-hours module failures that blocked *all* notifications.
    - 1.17.0 (#2525) and 1.18.0 (macOS Sequoia, Windows) improved detection.
    - 1.17.4 (#2595) added Linux detection.
    - 1.26.0: on macOS 27, notifications respect Focus and the system sound setting.
  - **How Mailspring detects DND today:**
    - Windows: the `windows-focus-assist` native module treats PRIORITY_ONLY and ALARMS_ONLY as DND (`app/src/dnd-utils-windows.ts`).
    - Linux: `dbus-send … org.freedesktop.Notifications Inhibited`, then gsettings `show-banners`, Cinnamon and Xfce (`app/src/dnd-utils-linux.ts`), with a 5-second cache.
    - macOS: always `false`, because "macOS … applies Focus to notifications itself" (`app/src/native-notifications.ts:116-125`).
- **Spec:**
  - macOS `UNNotificationInterruptionLevel`. `.timeSensitive` breaks through Focus and needs the `com.apple.developer.usernotifications.time-sensitive` entitlement.
  - `INFocusStatusCenter` needs user authorization and is meant for communication apps.
  - Windows `SHQueryUserNotificationState` (`QUNS_BUSY`, `QUNS_PRESENTATION_MODE`, etc.). Focus Assist state has no documented API.
  - freedesktop Notifications: KDE's `Inhibited` property.
- **Alice today:**
  - No detection, and none needed for banners or sounds.
  - Banners are suppressed while the window is focused (`desktop.rs:383-390`).
  - Linux notifications use urgency Normal, the notify-rust default, which DND respects. `Critical` would bypass it.
- **Plan:**
  1. Document the invariant in `notification.rs`: never play in-app audio and never set `Urgency::Critical` / `.timeSensitive` for mail.
  2. Decide on OTP notices (`desktop.rs:124-185`). Option: mark them `.timeSensitive` on macOS behind a setting, with the entitlement, because codes expire within minutes. Default off.
  3. If custom sounds arrive (see "(NEW) Notification rules per account and folder"), keep them inside the notification payload (UN sound name, toast `ms-winsoundevent`, XDG `sound-file`). The OS then mutes them under DND.
  4. Manual QA matrix in `docs/` covering macOS Focus, Windows 11 DND, GNOME and KDE DND: a banner is queued silently, nothing pops or chimes, and it is visible in the notification centre afterwards.
- **Pitfalls:**
  - Detecting DND in-process (Mailspring's approach) caused outages twice (1.10.3, 1.15.1). Avoid it unless Alice plays its own audio.
  - macOS `will_present` returns Banner|List|Sound even when frontmost (`notification.rs:96-107`). That is fine, because Focus still applies.
- **Done when:** The QA matrix passes on all four environments, and a code comment and test guard against `Urgency::Critical` in `notification.rs`.
- **Effort:** S. No dependencies.

<a id="notification-actions-archive-mark-read-reply"></a>

### Notification actions: archive, mark read, reply

*Roadmap: P1 · Platform integration*

- **Why:** Most new mail needs only a glance and a dismissal. Acting from the banner saves opening the app.
- **Competitors:**
  - **Mailspring** (`repos/mailspring/app/internal_packages/unread-notifications/lib/main.ts:144-222`):
    - Each notification has "Mark as Read" and "Archive" buttons, plus inline reply with "Reply to %@…", which calls `Actions.sendQuickReply`.
    - The actions act on the *thread*.
    - Notifications close when the thread is read elsewhere (`:107-113`).
    - More than a few arrivals produce one summary notification.
  - **Mailspring by platform:**
    - 1.10.0 temporarily removed macOS inline reply.
    - 1.17.0 added Windows toasts with inline actions.
    - 1.17.4 fixed broken summary toast URLs (#2597).
    - 1.19.1 added Windows action buttons and inline reply inputs, and fixed click handling (#2648).
    - 1.20.0 fixed a race that made Archive and Mark as Read unreliable on Windows.
    - Windows buttons route through `mailspring://notification-…` protocol URLs, "since Windows toast XML with activationType="background" doesn't work reliably" (`app/src/browser/application.ts:1057-1062`).
  - **Forum #716** asks for more default actions (6 votes).
- **Spec:**
  - **macOS:** `UNNotificationCategory`, `UNNotificationAction` and `UNTextInputNotificationAction` (objc2-user-notifications 0.3, features `UNNotificationCategory`, `UNNotificationAction`), registered with `setNotificationCategories`. Responses arrive in `didReceiveNotificationResponse` with `actionIdentifier` and `UNTextInputNotificationResponse.userText`.
  - **Linux:** freedesktop actions are buttons only. Inline reply is KDE-only (`inline-reply`) or portal v2 `im.reply-with-text`, which only applies to the `im.received` category.
  - **Windows:** notify-rust / tauri-winrt-notification buttons work in-process only, with no input box and no cold-start activation. Reply and cold activation need a COM `INotificationActivationCallback` server plus `ToastActivatorCLSID` on the Start-menu shortcut. Protocol activation (`activationType="protocol"`) works cold but carries no text.
  - tauri-plugin-notification 2.6.0 now supports desktop action buttons, but uses the deprecated NSUserNotification on macOS.
- **Alice today:**
  - Click-to-open only. The macOS delegate handles the default response (`apps/desktop/src-tauri/src/notification.rs:109-119`). Linux registers a single `"default"` action (`notification.rs:260-263`). Windows waits for `Default` (`notification.rs:310-316`).
  - Notices are per message, or one grouped Inbox summary above 3 (`desktop.rs:392-423`).
  - The daemon API already supports the actions: `PATCH /v1/messages/{id}` with `{is_read}` or `{folder:"archive"}` (`daemon.rs:277-280`, `daemon.rs:410`, `model.rs:214-218`). Sending is `POST /v1/messages` + `/send`.
- **Plan:**
  1. Change `Target::Message(id)` to carry the action, e.g. `clicked(Target, Action::{Open, MarkRead, Archive, Reply(String)})` in `notification.rs:44-51`. In `desktop::open` (`desktop.rs:433-444`), run non-open actions against the daemon without showing the window.
  2. **macOS:**
     - At `platform::init` (`notification.rs:134-147`), register a category `mail` with actions `mark-read`, `archive` and a `UNTextInputNotificationAction` `reply`.
     - Set `setCategoryIdentifier("mail")` on per-message notices, but not on the grouped summary or OTP notices.
     - Branch on `actionIdentifier` in `did_receive`.
  3. **Linux:** add `.action("mark-read", …)` and `.action("archive", …)` buttons. Use inline reply only when the server's `GetCapabilities` includes `inline-reply` (KDE). Otherwise "Reply" opens Compose.
  4. **Windows:**
     - Phase 1: buttons through notify-rust (works while Alice runs, which is always the case with the tray). "Reply" opens Compose.
     - Phase 2 (optional): use protocol activation (`alice://notification/archive?id=…`) through the single-instance and argv path from the mailto item, so buttons work after a toast moves to Action Center.
     - Treat a COM activator for inline reply as a stretch goal.
  5. Act on the conversation, not just the one message, using the thread ID from `/v1/threads`. Withdraw delivered notifications when their message becomes read elsewhere: on macOS `removeDeliveredNotificationsWithIdentifiers`, as `notification.rs:189-199` already does for OTPs.
  6. Add localized labels to `locales/*/…` for desktop strings.
- **Pitfalls:**
  - **Quick reply:**
    - Quick reply sends mail without showing the composer. Keep it plain text, with signature, Re: subject and `In-Reply-To`/References as in the reader's reply (`App.tsx:207-213`), plus the "Correct MIME for replies" P0 fix.
    - Respect the at-most-once delivery path.
    - It needs the Reply-all and References work so threads aren't broken.
  - **Platform behaviour:**
    - Under macOS Focus, actions still work from Notification Center.
    - On Windows, a click is reported only while the toast is on screen. The protocol route fixes this.
    - Two or more actions collapse into an "Options" menu on some macOS styles. Order them Archive, Mark read, Reply.
  - **Failures:** if the daemon is unreachable, the action must fail visibly, with a follow-up notification "Couldn't archive — open Alice".
- **Done when:**
  - A unit test round-trips the target and action through the identifier format (extending `notification.rs:331-340`).
  - A daemon test shows archive and mark-read from a notification target hit the right messages.
  - Manual: macOS reply sends a threaded reply, and the Linux (KDE) and Windows buttons work.
- **Effort:** M (macOS + Linux + Windows buttons). L with Windows cold activation and inline reply. Depends on "Correct MIME for replies: `References` header chain", "Reply-all", and the single-instance work in the mailto item.

<a id="start-hidden-at-login-open-to-the-tray-dock-without-showing-the-window"></a>

### Start hidden at login: open to the tray / Dock without showing the window

*Roadmap: P1 · Platform integration · new*

- **Why:** Most people want notifications and sync after login, not a mail window over their desktop. GitHub #1953 "Run as minimized on startup" (7 👍) asks for exactly this.
- **Competitors:**
  - Mailspring's `--background` flag (`app/src/browser/main.js:144-173`). Every autostart entry passes it, and on macOS `wasOpenedAtLogin` implies it (`:347-351`).
    - 1.9.0: background start in maximized mode (#2259).
    - 1.9.2: background start on macOS.
    - 1.18.0: on Wayland the window shows briefly, then hides (#2623).
  - Mailspring has no setting for this: login launches always start hidden.
  - Thunderbird has "Start minimized" through extensions. Outlook (new) has "Start minimized" via its Run key argument.
- **Spec:** `tauri-plugin-autostart` `args`. With SMAppService no arguments are passed, so detect a login launch through the Apple Event `keyAELaunchedAsLogInItem` on the `kAEOpenApplication` event (`NSAppleEventManager.currentAppleEvent`). This is what Electron's `wasOpenedAtLogin` uses. Verify it with SMAppService main-app items.
- **Alice today:**
  - The window is created visible from `tauri.conf.json` (`apps/desktop/src-tauri/tauri.conf.json:13-22`).
  - Closing hides it when the tray exists (`desktop.rs:466-474`).
  - On Windows and Linux the window is forced visible if the tray is turned off while hidden (`desktop.rs:310-319`).
- **Plan:**
  1. Set `"visible": false` on the main window in `tauri.conf.json`. In `desktop::setup` (`desktop.rs:65-74`), call `show_window` unless the launch is a login launch: `--hidden` is in argv (Windows/Linux/LaunchAgent), or `keyAELaunchedAsLogInItem` is set (SMAppService).
  2. Add the preference `start_hidden: bool` (default true when autostart is on). A login launch with `start_hidden=false` shows the window.
  3. Guard: on Windows and Linux, start hidden only when `menu_bar` (the tray) is on. Otherwise there is no way back to the window; show it instead. On macOS the Dock icon is always available (`desktop.rs:476-484`).
  4. A `mailto:` or notification-click launch always shows the window, even if `--hidden` is also present.
- **Pitfalls:**
  - Wayland compositors may refuse to keep a mapped window hidden. Never map the window at all (visible=false at creation) rather than show-then-hide as Mailspring does.
  - A hidden first run before any account exists would look like a crash. Always show the window when there are no accounts.
  - The webview of a never-shown window still loads. That is fine, and it keeps notifications working.
- **Done when:**
  - Unit test `should_show(args, accounts, menu_bar, os)` covers the combinations.
  - Manual: reboot with autostart on, then check the tray or Dock icon is present, no window appears and a new-mail notification arrives.
- **Effort:** S. Depends on "Launch at login".

## P1 → Security

<a id="ship-a-built-in-google-oauth-client-safely-the-bundled-secret-shortcut-is-alread"></a>

### Ship a built-in Google OAuth client safely (the bundled-secret shortcut is already gone)

*Roadmap: P1 · Security*

- **Note:** The premise is stale. HEAD has no bundled Google secret. `oauth.rs:43` is the `Api` enum, and `git log -S GOCSPX --all` finds nothing in history. Today the client ID and secret are bring-your-own: they come from the Add Account form (`apps/desktop/src/AddAccount.tsx:35,49`) or from the `ALICE_GOOGLE_CLIENT_ID` / `ALICE_GOOGLE_CLIENT_SECRET` environment variables (`crates/alice-core/src/daemon.rs:769-783`). They are stored per account in the keychain (`crates/alice-core/src/vault.rs:10-11`) and sent at code exchange and refresh (`oauth.rs:295-297`, `oauth.rs:366-368`). The real decision is how Alice ships a turnkey Gmail sign-in without making the secret a liability. The item should be retitled: "Ship a built-in Google OAuth client: keep its secret out of git and out of per-account storage, with a rotation plan."
- **Why:** Switchers expect "Sign in with Google" to just work. Asking them to create a Google Cloud project, as Alice does today, is a setup wall. But a client ID that ships inside the app becomes a shared asset. If someone abuses it, or Google suspends it, every user's Gmail breaks at once.
- **Competitors:**
  - **Mailspring** embeds its Gmail client ID and secret in source. The secret is only lightly obfuscated: AES-decrypted at startup, and the comment admits it is "more an extension of the Client ID than a proper Client Secret" (`repos/mailspring/app/internal_packages/onboarding/lib/onboarding-constants.ts:5-28`). Either value can be overridden with `MS_GMAIL_CLIENT_ID` / `MS_GMAIL_CLIENT_SECRET`. Sign-in uses a loopback server on port 12141 with PKCE (`onboarding-helpers.ts:232-235`), and Office 365 uses a public client ID with no secret (`:38-39`, `:293-294`).
  - **Thunderbird** also ships Google client credentials in its public source (`OAuth2Providers.sys.mjs`).
  - **MailMate**, a desktop-only client, had to pass Google's CASA Tier 2 assessment for the Gmail scope ([MailMate list, April 2025](https://lists.freron.com/mailmate/2025-April/018270.html)).
- **Spec:**
  - [Google OAuth for installed apps](https://developers.google.com/identity/protocols/oauth2/native-app): installed apps "cannot keep secrets", PKCE with S256 is recommended, a loopback IP redirect on a port you choose is supported, and custom URI schemes are no longer supported.
  - In practice Google's token endpoint still rejects Desktop-type exchanges that omit `client_secret`, even with PKCE ([forum thread](https://discuss.google.dev/t/desktop-oauth-pkce-exchange-returns-invalid-request-after-successful-loopback-callback-with-no-client-secret/390526)). The secret has to ship.
  - Since June 2025 the Cloud Console shows a client secret only at creation and supports rotation ([Google blog](https://developers.googleblog.com/usability-and-safety-updates-to-google-auth-platform/)).
  - `https://mail.google.com/` is a restricted scope ([restricted-scope verification](https://developers.google.com/identity/protocols/oauth2/production-readiness/restricted-scope-verification)). Apps that use it need verification, plus a security assessment that is renewed "at least every 12 months". Until then Google applies a user cap and a short refresh-token lifetime, and Testing apps show a warning screen.
  - Microsoft public clients (mobile and desktop platform) need no secret.
- **Alice today:** Bring-your-own client only, as listed under **Note**. PKCE S256 and a loopback redirect are already in place (`oauth.rs:26`, `oauth.rs:219-240`). The fixed port 4816 (`oauth.rs:220-222`) fails with `mail.oauth.port_busy` if the port is taken.
- **Plan:**
  1. Register an Alice-owned Google "Desktop app" client. Inject the ID and secret at release build time with `option_env!("ALICE_GOOGLE_CLIENT_ID")` / `option_env!("ALICE_GOOGLE_CLIENT_SECRET")` from CI secrets in `.github/workflows/ci.yml`, so neither value is committed. Local and fork builds keep bring-your-own.
  2. Add `Secret::client: ClientKind { Builtin, Own }` in `vault.rs`. For built-in accounts, store only the kind and look the ID and secret up from the build constants at refresh time (`oauth.rs:347-368`). A rotated secret then reaches every account through the next app update, with nothing per account to migrate. Bring-your-own accounts keep their stored ID and secret.
  3. Write the rotation runbook: add a second secret in the Cloud Console, ship it, wait for adoption, then disable the old one. Refresh tokens are bound to the client ID, so users should not need to re-consent. Verify this once against a test account before relying on it.
  4. Bind the loopback listener to `127.0.0.1:0` and put the chosen port in `redirect_uri`. Google allows any loopback port for Desktop clients. This removes the fixed-port failure.
  5. Start Google verification for `mail.google.com` plus the sensitive contacts and calendar scopes, and budget for the yearly CASA assessment. Keep the bring-your-own path documented in `README.md` for people who don't want a shared client.
  6. In Add Account, hide the client ID and secret fields behind "Use my own Google Cloud client" once a built-in client exists.
- **Pitfalls:**
  - Anyone can pull the secret out of the binary. The real risk is impersonation: a phishing app that reuses Alice's client would show "Alice" on Google's consent screen. Google may then suspend the client, and that hits every user. Keep monitoring in the Cloud Console and keep bring-your-own as an escape hatch.
  - Never log token-exchange bodies.
  - Apps in Testing status get refresh tokens that expire after 7 days, so all accounts drop every week. Ship builds with the built-in client only after verification.
  - Mailspring 1.26 (#2907) had to force the provider's servers for OAuth accounts so Workspace and O365 sign-ins work. Keep Alice's endpoints fixed per provider (`oauth.rs:50-62`).
- **Done when:**
  - `strings` on a release binary finds the build-time client ID, and the git tree contains no secret.
  - A unit test shows that a `Builtin` account's refresh request uses the compiled-in secret, and that a stored per-account secret is ignored for it.
  - The OAuth listener test binds an ephemeral port and the redirect URI matches it.
  - The rotation runbook is in `docs/`.
- **Effort:** M for code, plus weeks to months of external lead time for Google verification and CASA. Depends on "Verify Outlook/O365 OAuth against a live account; ship a built-in Microsoft client id".

<a id="never-fetch-remote-content-in-replies-forwards-previews-or-show-all-when-images"></a>

### Never fetch remote content in replies, forwards, previews or "show all" when images are blocked

*Roadmap: P1 · Security*

- **Why:** Remote-image blocking is only as strong as its weakest path. One fetch from a reply or forward, a print preview or an expanded clip tells the sender that the message was opened, and from where.
- **Competitors:**
  - **Mailspring 1.26.0** fixed four separate leaks:
    - "Show All" on a clipped message rendered unsanitized HTML with remote images.
    - Replying, forwarding and sending fetched remote images in the quoted message.
    - Event notes fetched images from the description (#2901).
    - Print, quick-preview and "show original" windows loaded remote pages when a link was clicked.
  - Mailspring 1.21.0's audit also removed `file://` URIs from mail content (#2699).
  - Mailspring's blocker rewrites `src`, `background`, `@import` and `url()` with a regex and strips `<link>` (`repos/mailspring/app/internal_packages/message-autoload-images/lib/autoload-images-store.ts:24-37`, `autoload-images-extension.ts`). Its quoting path re-sanitizes and inlines CSS (`app/src/flux/stores/draft-factory.ts:35-55`). Remote `src` values survive into the contenteditable composer, which is where the 1.26 leak came from.
  - Apple Mail's "Protect Mail Activity" proxies all remote content instead.
- **Spec:** CSP Level 3: a `srcdoc` / `about:srcdoc` document inherits the embedder's policy container, so the frame's `<meta>` policy and the window's policy are both enforced.
- **Alice today:** Safe for now, mostly because the risky surfaces don't exist yet.
  - The HTML reader blocks remote images by stripping `src` (`apps/desktop/src/emailHtml.ts:64-71`). It also puts a frame CSP in place, `default-src 'none'; img-src data:` plus `https:` only after "Load images" (`emailHtml.ts:79`), inside `sandbox="allow-same-origin"` with no scripts and `referrerPolicy="no-referrer"` (`apps/desktop/src/MessageView.tsx:105`).
  - "Show earlier messages" re-renders through the same function with the same image state (`MessageView.tsx:93-98`).
  - Replies are plain text with no quote at all: `createCompose` starts with an empty body (`apps/desktop/src/App.tsx:207-213`).
  - There is no forward, print, show-original or clip popout yet.
  - Notifications and list previews are plain text (`desktop.rs:405-421`).
  - Event descriptions render as text (`apps/desktop/src/Invitation.tsx:113`).
  - The window CSP allows `img-src https:` (`apps/desktop/src-tauri/tauri.conf.json:24`). The frame needs that, but it means any remote `<img>` placed in the main DOM, such as a future rich-text composer, will load.
- **Plan:**
  1. Make `emailDocument` (`emailHtml.ts:34`) the only way message HTML reaches any document. Print (P1 Reading), the "show full message" popout and forward preview must call it with the message's current `images` state, not a fresh `false`/`true`.
  2. For the HTML composer (P1 Composer), add `quoteHtml(html, loadImages)` in `emailHtml.ts`.
     - It sanitizes with the same DOMPurify config.
     - It moves every remote `src` into `data-alice-src`, with `background` and `srcset` already forbidden.
     - It drops `<style>` `url()`s unless images are loaded.
     - The send path (`crates/alice-core/src/mime.rs`) restores `data-alice-src` to `src`, so recipients still see the images and Alice never fetches them.
  3. Defence in depth: serve the reader frame from a dedicated Tauri custom protocol (`register_uri_scheme_protocol("alice-mail")`) with its own CSP response header. The main window CSP can then drop `https:` from `img-src` (`tauri.conf.json:24`), and nothing outside the frame can ever load a remote image.
  4. When "Show original" lands, show raw source in a `<pre>` / textarea and never as HTML. Links from print and popout windows must go through `open_email_url` (`apps/desktop/src-tauri/src/main.rs:149-162`) and only on a real click, not on navigation. Mailspring 1.26 found a meta-refresh in a message opening a page on its own; `meta` is already forbidden at `emailHtml.ts:51`.
  5. The AI and MCP paths already get text only. Keep `body` (text) as their only input, and add a comment where `html_body` is excluded.
- **Pitfalls:**
  - Juice-style CSS inliners fetch `@import`ed sheets. Never run an inliner over message HTML in the app process.
  - `cid:` images are local, so they may stay.
  - `http:` images never load even after "Load images" (`emailHtml.ts:69`). That is deliberate (mixed content), but say so in the UI.
  - JSDOM in `scripts/test-html.mjs` does not enforce CSP, so CSP regressions need a real browser test or an assertion on the policy string.
  - DNS prefetch for links: add `<meta http-equiv="x-dns-prefetch-control" content="off">` to the frame head built at `emailHtml.ts:80`. It costs nothing.
- **Done when:**
  - `scripts/test-html.mjs` asserts that `quoteHtml(fixture, false)` leaves no `https:` URL in any `src`, `style` or `url()`, and that restoring it before send gives back the original `src` values.
  - A test asserts the frame policy string contains `default-src 'none'`, with `https:` in `img-src` only when `loadImages` is true.
  - A manual or Playwright check of print and popout against a local HTTP server that counts requests sees zero requests while images are blocked.
- **Effort:** S now for the policy and tests. M when the HTML composer and print land, and the work should be built into those items. Depends on "Rich text / HTML compose", "Forward, with original attachments carried over", "Print and Save as PDF" and "Clip very long messages with a 'show full message' popout".

<a id="block-remote-css-and-web-fonts-along-with-images"></a>

### Block remote CSS and web fonts along with images

*Roadmap: P1 · Security · done*

- **Note:** Already done in the reader, and stricter than Mailspring. The frame CSP is `default-src 'none'; style-src 'unsafe-inline'; img-src data:[ https:]` (`apps/desktop/src/emailHtml.ts:79`). With that policy:
  - `@import url(…)` and external sheets are always blocked, because `style-src` allows no URLs and `<link>` is forbidden (`emailHtml.ts:51`).
  - `@font-face` downloads fall back to `default-src 'none'` and are always blocked, even after "Load images".
  - CSS `background-image`, `list-style-image`, `cursor`, `border-image`, `mask` and `content: url()` all fall under `img-src`, so they load only together with images.

  What remains is a regression test, plus a decision on whether web fonts should load when the person chooses "Load images". Apple Mail loads them with remote content; Alice currently never does.
- **Why:** CSS `@import`, `url()` backgrounds and web fonts are tracking pixels by another name. Mailspring 1.21.0 (#2706) found `@import` "reach[ing] the network even when the user has remote-content blocking enabled".
- **Competitors:**
  - Mailspring 1.7.5: "Disabling image autoloading … also prevents requests for external stylesheets and web fonts."
  - Mailspring 1.21.0 strips `@import` with `CSSStyleSheet.replaceSync` (`repos/mailspring/app/src/services/sanitize-transformer.ts:238-272`) and has `FORCE_BODY` so head `<style>` blocks reach the sanitizer (`:286-299`).
  - Mailspring's own Electron CSP adds `object-src none; media-src mailspring:` (1.11.0).
- **Spec:** CSP Level 3 directive fallback: `font-src` falls back to `default-src`. CSS `url()` in properties is fetched as an image request and governed by `img-src`.
- **Alice today:** As described in **Note**. `<style>` is kept via `ADD_TAGS: ['style']`, and its contents are not rewritten (`emailHtml.ts:49-53`).
- **Plan:**
  1. Add assertions to `scripts/test-html.mjs`: the policy built at `emailHtml.ts:79` must have no `font-src`, no URL source in `style-src`, and must contain `default-src 'none'`.
  2. Optionally strip `@import` rules in `emailDocument` as Mailspring does. This is belt and braces in case the CSP is ever loosened. Run each sheet through `new CSSStyleSheet().replaceSync()`, which drops `@import` (this needs a real DOM, so guard it in JSDOM tests).
  3. Product decision: either keep fonts blocked always (current behaviour), or add ` https:` to `font-src` when `loadImages`. Recommend keeping them blocked. Fonts are rarely essential, and Alice's own font stack applies.
- **Pitfalls:**
  - If the frame is ever moved to a custom protocol (previous item), the CSP must move into the response header with the same directives.
  - `style-src 'unsafe-inline'` is required for email layout. Do not add `'unsafe-eval'` or any `https:` source.
  - SVG `data:` images in CSS are inert, because they can't run script or fetch as images.
- **Done when:** `npm run test:html` fails if the frame policy gains a font or stylesheet URL source, and a fixture with `@import url(https://t.invalid/a.css)` and `@font-face{src:url(https://t.invalid/f.woff2)}` produces a document whose policy blocks both.
- **Effort:** S. No dependencies.

<a id="never-auto-load-images-in-spam"></a>

### Never auto-load images in Spam

*Roadmap: P1 · Security*

- **Why:** Spam is where tracking pixels and "is this address live" probes are densest. Someone who chose "Always load images" did not mean it for spam.
- **Competitors:**
  - Mailspring 1.2.0: "never shows images by default when you're viewing your spam folder" (#784).
  - The current Mailspring logic is `if (autoloadImages && !inSpam) return false` (`repos/mailspring/app/internal_packages/message-autoload-images/lib/autoload-images-store.ts:43-56`). A per-sender "always load" allowlist entry still loads images in Spam, which is a hole Alice should not copy.
  - Gmail and Outlook.com also disable images and links in Spam.
- **Alice today:** No Spam exception. `loadImages={preferences.remote_images === 'always'}` applies to every folder (`apps/desktop/src/App.tsx:399`), and `MessageView` starts from it (`apps/desktop/src/MessageView.tsx:92`). Messages carry `folder: 'spam'` (`apps/desktop/src/api.ts:17`; the README says Spam copies win when a message is in several mailboxes).
- **Plan:**
  1. In `MessageView.tsx:92`, initialise with `useState(loadImages && message.folder !== 'spam')`. Doing it per message, rather than at `App.tsx:399`, also covers a mixed conversation, should one ever exist.
  2. In Spam, label the "Load images" bar (`MessageView.tsx:104`) as a warning, with new strings in `apps/desktop/src/locales/*/app.json`.
  3. In Spam, also strip link `href`s until "Not spam", as Gmail does. Optional; keep "Copy link" working.
  4. When the P1 "Multi-folder message model" lands, check the label set (any Spam label), not just `folder`.
- **Pitfalls:**
  - Messages moved out of Spam should pick up the normal setting. The `Conversation` key includes `remote_images` but not folder (`App.tsx:399`), so make sure `MessageView` remounts or re-derives when `message.folder` changes.
  - Gmail's Spam is `[Gmail]/Spam`. Make sure the folder-role mapping covers localized names.
- **Done when:** A reader test shows a `folder: 'spam'` message renders with `src` stripped even when `loadImages` is true, and the same message with `folder: 'inbox'` loads.
- **Effort:** S. Should be revisited after "Multi-folder message model".

<a id="dont-send-dav-basic-credentials-across-hosts-on-redirect"></a>

### Don't send DAV Basic credentials across hosts on redirect

*Roadmap: P1 · Security · new*

- **Why:** `dav::Session::request` follows up to five redirects by hand. On every hop it re-sends `basic_auth(username, password)`, to whatever HTTPS host the `Location` names (`crates/alice-core/src/dav.rs:217-273`). A compromised or misconfigured `/.well-known/caldav` on a vanity domain can then collect the app password for iCloud or Fastmail.
- **Competitors:**
  - Mailspring returns the well-known redirect target without following further (`repos/mailsync/MailSync/NetworkRequestUtils.cpp:139-160`). It fixed 303/307/308 handling in 1.26.0.
  - Browsers and curl drop `Authorization` on cross-origin redirects (curl needs `--location-trusted` to keep it).
- **Spec:** RFC 9110 §15.4 (redirects) and §11.6.2 (Authorization is origin-scoped). RFC 6764 §5: a well-known URI may redirect to another host.
- **Alice today:** Credentials follow every redirect (`dav.rs:229-260`). `check_url` enforces HTTPS only (`dav.rs:176-187`).
- **Plan:**
  1. Keep credentials only while the redirect stays on the original host, or on a host under the same registrable domain (iCloud partitions `pNN-caldav.icloud.com`).
  2. On any other host, stop and show "This server redirects to `<host>`. Use that address?". If the person accepts, save that URL on the `DavSource`.
  3. Use a small suffix check, not a new public-suffix crate: the same last two labels, or three for a known two-label suffix list. Leave a `ponytail:` note on that ceiling.
- **Pitfalls:** Fastmail and iCloud redirect within their own domain, and those redirects must keep working.
- **Done when:** A unit test with a fake server shows the `Authorization` header is not sent after a cross-domain 301, and is sent after a same-domain 301.
- **Effort:** S. No dependencies.

## P2 → Contacts and calendar

<a id="contact-editor-create-edit-delete-groups-notes-sync-back-to-google-graph-carddav"></a>

### Contact editor: create, edit, delete; groups; notes; sync back to Google, Graph, CardDAV

*Roadmap: P2 · Contacts and calendar*

- **Why:** People switching clients expect to fix a misspelled name, add a phone number or build a group from the address book they already use. Alice's address books are read-only copies today, so every correction means opening Google Contacts or Outlook.
- **Competitors:**
  - **Mailspring layout and editing.**
    - The editor is under Window > Contacts (shipped 1.7.0). Google was read-only until 1.7.4. The NOTE field arrived in 1.18.0 (#2621).
    - The sidebar has a unified "All Contacts" view, plus per-account "All Contacts", one entry per group, and "Found in Mail" (`repos/mailspring/app/internal_packages/contacts/lib/ContactPerspectivesList.tsx`).
    - Groups are created with "+", renamed inline, deleted, and filled by dragging contacts onto them (cross-account drags are refused, `:84-158`). There is also a "Remove from Group" button (`ContactDetailToolbar.tsx:108`).
    - Editable fields: names, nicknames, company and title, typed emails, phones and URLs, addresses, birthday, relations and notes. Photos are display-only (`ContactInfoMapping.ts`).
    - "Found in Mail" contacts are read-only. Deleting one only hides it.
    - Delete has no confirmation and no undo. Undoing a group delete recreates it empty (`app/src/flux/tasks/destroy-contactgroup-task.ts:30`).
  - **Mailspring write-back.** Edits are tasks run by mailsync (`repos/mailsync/MailSync/TaskProcessor.cpp:1446-1705`): `SyncbackContactTask`, `DestroyContactTask`, `SyncbackContactGroupTask`, `ChangeContactGroupMembershipTask` and `DestroyContactGroupTask`.
    - **Google** (`GoogleContactsWorker.cpp`): `people:createContact`, `PATCH …:updateContact?updatePersonFields=…` with the stored etag, `…:deleteContact`, and for groups `contactGroups` CRUD plus `members:modify` (`:201-269`). A stale etag fails the task with no merge.
    - **CardDAV** (`DAVWorker.cpp`):
      - Create POSTs to the collection, which is non-standard (`:800-802`).
      - Update PUTs with `If-Match`, then reads back via multiget (`:806-820`).
      - Delete sends `If-Match` (`:873`).
      - A 412 surfaces as a generic error.
      - Groups are separate group vCards (`KIND:group` + `MEMBER` in 4.0, `X-ADDRESSBOOKSERVER-KIND/MEMBER` for iCloud and Fastmail), not CATEGORIES. Nextcloud and Google CardDAV groups are therefore invisible.
      - Only one address book per account is synced ("TODO: Pick the primary one somehow!", `:673`).
    - **Sync.** Google uses a `syncToken` with a 2-second sleep per page. Its deletion check reads `conn["deleted"]` instead of `metadata.deleted`, so remote deletes probably never apply (`:69`). CardDAV uses ctag, then RFC 6578 `sync-collection` with 507 truncation handling, then an etag-diff fallback.
    - **No Exchange or Graph support at all.**
  - **Mailspring bugs to avoid:** date padding in `VCFHelpers.ts:46`, duplicate TITLE on edit (`ContactInfoMapping.ts:164`), relation edits silently dropped on CardDAV, and 403 from read-only books swallowed (`DAVWorker.cpp:805-813`). Changelog fixes: 1.7.5 (crash deleting a found-in-mail contact), 1.17.2 (Yahoo 406), 1.18.0 (etag on first sync), 1.25.0 (duplicate IDs), 1.26.0 (#2912 birthday without a date).
  - **Thunderbird** writes back to CardDAV, but its mailing lists stay local.
  - **Apple Mail** delegates to Contacts.app.
- **Spec:**
  - vCard 4.0, RFC 6350: KIND §6.1.4, MEMBER §6.6.5, UID §6.7.6, REV §6.7.4, CATEGORIES §6.7.1, NOTE §6.7.2.
  - vCard 3.0, RFC 2426.
  - CardDAV, RFC 6352: §6.3.2 creating resources, with preconditions `no-uid-conflict` and `max-resource-size` in §6.3.2.1; §9.2 avoiding lost updates (`If-Match`); §8.7 multiget.
  - RFC 6578 `sync-collection`.
  - [People API contacts guide](https://developers.google.com/people/v1/contacts): `updateContact` needs `updatePersonFields` and the source etag; send mutations for one user sequentially; sync writes can take several minutes to show; an expired `syncToken` returns 410; batch methods exist.
  - Microsoft Graph `contact` resource: `/me/contacts`, `/me/contactFolders/{id}/contacts/delta`, scope `Contacts.ReadWrite`.
- **Alice today:** Read-only by design ("nothing is ever written back", `crates/alice-core/src/contacts.rs:1-4`).
  - Storage flattens every card to `(owner, source, email, name)` (`crates/alice-core/src/store.rs:26`).
  - What each source reads:
    - Google: only `names,emailAddresses`, from connections and otherContacts (`contacts.rs:557-585`).
    - Graph: `$select=displayName,emailAddresses` (`contacts.rs:637`).
    - CardDAV: an `addressbook-query` limited to cards that have EMAIL (`contacts.rs:667-689`), parsed for FN, N and EMAIL only (`contacts.rs:747-789`).
  - Each refresh replaces everything, every 6 hours (`contacts.rs:41`, `contacts.rs:705-724`).
  - Scopes are `contacts.readonly` / `contacts.other.readonly` (`crates/alice-core/src/oauth.rs:28-31`) and `Contacts.Read` (`oauth.rs:37`).
  - Mac Contacts are imported read-only by the desktop app (`apps/desktop/src-tauri/src/contacts.rs`).
- **Plan:**
  1. **Local address book cache** (prerequisite).
     - New tables in `store.rs`:
       - `address_books(id, source, owner, url_or_folder, name, read_only, sync_token, ctag)`.
       - `cards(id, book_id, href_or_resource, etag, uid, raw TEXT, kind, display_name, emails_json, rev, dirty, deleted)`.
       - `card_members(group_id, card_id)`.
     - Keep `raw` as the provider's own representation: vCard text for CardDAV, Person JSON for Google, contact JSON for Graph. Edits then never lose fields Alice doesn't understand.
     - Rebuild the existing `contacts` suggestion table from `cards`, so autocomplete is unchanged.
  2. **Incremental sync** in `contacts.rs`:
     - CardDAV: `sync-collection` with `address-data` (RFC 6578), falling back to ctag plus an etag diff. Read every card, not just ones with EMAIL, and every address book, not just the first.
     - Google: `connections.list` with `requestSyncToken`/`syncToken`, full `personFields`. On 410, do a full resync. Honour `metadata.deleted`.
     - Graph: `contactFolders/{id}/contacts/delta`.
     - Extend `dav.rs` with `put(url, body, if_match)` and `delete(url, if_match)` next to `create` (`crates/alice-core/src/dav.rs:343-361`).
  3. **Write API** in `crates/alice-core/src/daemon.rs`:
     - Endpoints: `POST /v1/contacts`, `PATCH /v1/contacts/{id}`, `DELETE /v1/contacts/{id}`, `POST|PATCH|DELETE /v1/contact-groups[/{id}]` and `POST /v1/contact-groups/{id}/members`.
     - Writes go into a per-account outbox processed one at a time. People API requires sequential mutations.
     - On success, store the returned etag. On 412 or a stale-etag error, refetch, apply the person's change property by property when it doesn't overlap the server change, and otherwise show "Changed elsewhere — keep mine / keep theirs".
  4. **vCard editing without loss:**
     - Edit the raw text in place. Replace only touched properties, keep `item1.` groups, `X-` properties and PHOTO, bump REV and keep UID.
     - Write the version the server already holds: 3.0 for iCloud and Google CardDAV, 4.0 otherwise.
     - Groups: read all three forms (`KIND:group`+`MEMBER`, `X-ADDRESSBOOKSERVER-*`, `CATEGORIES`). Write the form that book already uses.
     - New cards are PUT to `{book}/{uuid}.vcf` with `If-None-Match: *`, using the existing `create`.
     - Crate option: `calcard` 0.3 (Stalwart, Apache-2.0/MIT) parses and writes vCard and iCalendar losslessly. Otherwise extend Alice's own line parser (`contacts.rs:747`), which is enough if edits stay line-based.
  5. **Scopes and consent:**
     - Editing needs `https://www.googleapis.com/auth/contacts` (sensitive, not restricted) and `Contacts.ReadWrite`. Ask for them only when the person turns on "Edit contacts" for an account, through the existing `POST /v1/accounts/{id}/integrations` re-consent flow (`daemon.rs:264`, `apps/desktop/src/ContactsSettings.tsx:60-61`).
     - Google "other contacts" stay read-only. Offer "Add to My Contacts" via `otherContacts:copyOtherContactToMyContactsGroup`.
     - Mac Contacts stay read-only (writing needs `CNSaveRequest`; leave for later).
  6. **UI:**
     - A Contacts view in `apps/desktop/src/App.tsx`: a book and group list, a contact list, and a detail editor for names, typed emails, phones, addresses, company and title, birthday (with year optional), notes and groups.
     - Read-only badges for found-in-mail, other-contacts and Mac sources.
     - Delete asks for confirmation and offers undo by re-PUTting the saved `raw`.
     - "Add to contacts" / "Edit contact" in the sender context menu (`App.tsx:400`).
  7. **CLI/MCP:** `alice contacts list|show` first. Add write commands only behind MCP `Full` access (`model.rs:280-284`).
- **Pitfalls:**
  - **Google:**
    - People API mutations must be sequential per user.
    - Writes take minutes to appear in sync results. Don't "resync to confirm"; use the response body.
    - `updateContact` without the current etag fails.
    - Birthdays can have no year, or no date at all (Mailspring 1.26.0 #2912).
  - **Provider quirks:**
    - Fastmail reassigns the UID on create (`DAVWorker.cpp:835-839`), so read back the stored card.
    - Yahoo returned 406 during discovery (1.17.2) and never returns sync tokens.
    - iCloud photos need authentication.
    - Some servers reject a vCard whose `VERSION` isn't the second line (Mailspring `ContactInfoMapping.ts:51-62`).
    - Graph has no contact groups in v1.0. Show Outlook categories as read-only tags rather than inventing groups.
  - **vCard format:** fold lines at 75 octets (RFC 6350 §3.2), escape `,`, `;` and `\`, and remember vCard 3.0 requires N and FN.
  - **Privacy:** a vCard PHOTO can be a remote URI. Don't fetch it.
  - **Scale:** `SOURCE_LIMIT` is 20,000 per book (`contacts.rs:30`). Keep it, and page the list view.
- **Done when:**
  - A Radicale-backed integration test (the setup the README already cites) covers create, edit, delete and group membership. It checks `If-None-Match` on create and `If-Match` on edit.
  - A fake server returning 412 triggers the conflict path.
  - Unit tests show that an edited iCloud-style vCard keeps its `X-ABLabel`, `item1.` groups and PHOTO byte for byte.
  - JSON fixture tests cover Google `updateContact` bodies (etag, `updatePersonFields`) and Graph PATCH bodies.
  - Autocomplete results don't change after migration.
- **Effort:** XL. **Note:** Split this into "Local address book cache with incremental sync" (L) and "Contact editor with write-back" (L). Depends on "Offline action queue" (share the outbox), and enables "Contact groups expand in recipient fields" and "vCard import and export".

<a id="vcard-import-and-export"></a>

### vCard import and export

*Roadmap: P2 · Contacts and calendar*

- **Why:** It is the way people move address books between apps and back them up. "Editable Contacts from IMAP Email Addresses" (forum #752) also wants found-in-mail people saved somewhere real.
- **Competitors:**
  - **Mailspring** (1.19.0 #2626, with tests in 1.21.0; `repos/mailspring/app/internal_packages/contacts/lib/VCFImportExport.ts`):
    - **Export** is reachable from "Export All vCards…" / "Export Selected…" in the menu, the context menu, a toolbar button, and drag to desktop. Multiple contacts go into one CRLF-joined file. CardDAV cards are written raw. Google and mail contacts are synthesised as vCard 3.0 but lose NOTE and PHOTO, and RELATED, a 4.0-only property, appears in 3.0 output. Groups are not exported.
    - **Import** uses a file dialog or drag-drop and accepts `.vcf` and `.vcard`. Cards are split with a regex, `VERSION:3.0` is injected if missing, and the UID is **always replaced**, so re-importing duplicates. One task runs per card. Import is blocked for Gmail accounts.
  - **Thunderbird** imports and exports vCard and CSV, as well as LDIF.
  - **Outlook desktop** imports only the first card of a multi-card `.vcf`.
- **Spec:**
  - RFC 6350 (vCard 4.0; §3.2 line folding; §10.1 media type `text/vcard`).
  - RFC 2426 (3.0). vCard 2.1 is not an RFC, but old Outlook and Android still emit it with `QUOTED-PRINTABLE` and `CHARSET=` parameters.
  - People API `people:batchCreateContacts` (up to 200 per call).
- **Alice today:** Only an internal parser for CardDAV responses that reads FN, N and EMAIL (`crates/alice-core/src/contacts.rs:747-789`, with a test fixture at `contacts.rs:1068`). There is no import or export endpoint and no UI.
- **Plan:**
  1. Export: `GET /v1/contacts/export?book=…&ids=…` returns `text/vcard`.
     - CardDAV cards are sent raw.
     - Google and Graph cards are synthesised as vCard 3.0, the most widely importable version. Include NOTE, BDAY (`--MMDD` when there is no year), and a PHOTO only if it is already cached.
     - The desktop saves through the same Downloads-and-reveal flow as attachments (`apps/desktop/src-tauri/src/downloads.rs`). That avoids adding `tauri-plugin-dialog` for one button.
     - Add a CLI `alice contacts export > contacts.vcf`.
  2. Import: `POST /v1/contacts/import` with the file text and a target book.
     - Parse with the lossless parser from the editor item. Unfold lines and decode 2.1 QUOTED-PRINTABLE.
     - Keep the UID when it isn't already in the book, so re-import updates instead of duplicating. Otherwise ask whether to replace or skip.
     - Generate a UID only when one is missing.
     - Queue the writes through the contact outbox. For Google, use `batchCreateContacts` in chunks of 200.
  3. Show a preview step ("142 contacts, 3 already exist") before anything is written.
- **Pitfalls:**
  - Apple exports 3.0 with large base64 PHOTOs and `item1.EMAIL` + `X-ABLabel`.
  - Files may have a BOM or LF-only line endings, and may contain several cards.
  - `max-resource-size` limits on servers.
  - Groups exported as `KIND:group` cards break importers that don't understand them. Make "include groups" an option, off by default.
  - Never write a found-in-mail-only contact back as a server card without the person choosing a book.
- **Done when:**
  - Exporting a Radicale book and importing it into an empty book gives identical cards, ignoring REV.
  - Fixture tests parse Apple (3.0), Google (3.0) and Outlook 2.1 QUOTED-PRINTABLE exports correctly.
  - Re-importing the same file creates no duplicates.
- **Effort:** M after the cache from "Contact editor" exists. Without the cache, export of synthesised cards alone is S.

<a id="contact-sidebar-in-the-reader-avatar-recent-threads-other-addresses"></a>

### Contact sidebar in the reader: avatar, recent threads, other addresses

*Roadmap: P2 · Contacts and calendar*

- **Why:** It answers "who is this and what have we talked about" without leaving the message, which is the context people use to triage.
- **Competitors:**
  - **Mailspring** (`repos/mailspring/app/internal_packages/participant-profile/`):
    - Profiles come from Mailspring's server: `GET /api/info-for-email-v2/{email}?phrase={name}`, cached in localStorage, up to 200 entries.
    - Shown: bio, location, live local time, social handles, company logo, size and funding. Company profiles arrived in 1.0.4. Mailspring switched to its own logo API in 1.5.3.
    - Related threads are a local `from:` search showing three, with "Show more" (`sidebar-related-threads.tsx`).
    - Avatars are initials overlaid with Gravatar, `sha256(email)`, with **no opt-out** (`app/src/components/contact-profile-photo.tsx:18-22`; Gravatar since 1.5.0).
    - `github-contact-card` queries `api.github.com/search/users?q={email}`. It is on by default with a hidden toggle.
    - Free users must click "Try it now", which counts against a quota.
  - **Superhuman** shows a similar social sidebar.
  - **Apple Mail** shows only the contact card.
- **Alice today:**
  - Initials avatars with a colour from the name (`apps/desktop/src/App.tsx:27`, `apps/desktop/src/Conversation.tsx:34,42`), behind the `show_avatars` preference (`crates/alice-core/src/model.rs:317`).
  - No sidebar.
  - The contacts directory already ranks people by sent and received counts and recency (`crates/alice-core/src/contacts.rs:161-226`).
- **Plan:**
  1. Daemon `GET /v1/people/{email}`:
     - Names and other addresses: from cards sharing a UID or person, once the address book cache exists. Before that, from all sources that know the email.
     - Phone, company and title from the address book.
     - Message counts, plus first and last exchange dates, from the existing correspondent index.
     - Five recent conversations from the existing search (`crates/alice-core/src/search.rs`, `from:x OR to:x`).
  2. Desktop: a collapsible right panel in the conversation view, shown on clicking the sender avatar or name.
     - The open/closed state is remembered in localStorage.
     - Width-aware: hidden automatically in the narrowest layout.
     - Actions: "New message", "Search all mail", "Add to/Edit contact", "Copy address".
  3. Avatars are local-first: address book PHOTO (`data:` only) and the Mac Contacts thumbnail.
  4. Gravatar is a new opt-in preference, off by default. Its description says it sends a hash of each address to Gravatar. Requests go through the daemon with no referrer, and results are cached on disk with a TTL. The window CSP already allows `https:` images.
  5. No social or company enrichment. It needs a third-party data broker, which conflicts with Alice's local-first stance.
- **Pitfalls:**
  - Skip the sidebar for automated senders (`contacts.rs:325`, `automated()`) and mailing lists.
  - Gravatar is a privacy leak: an Automattic-owned service learns who mails you. That is why it must be opt-in.
  - Keep the panel from stealing width from the reader frame's 12,000 px height cap logic (`MessageView.tsx:111`).
- **Done when:**
  - A daemon test seeds messages and cards and asserts `/v1/people/{email}` returns counts, other addresses and five threads.
  - A UI check confirms no network request happens when the panel opens with Gravatar off.
- **Effort:** M. Benefits from the cache in "Contact editor" for other addresses and photos; works without it.

<a id="calendar-view-day-week-month-agenda-create-edit-and-drag-events-recurrence-editi"></a>

### Calendar view: day, week, month, agenda; create, edit and drag events; recurrence editing; time zones

*Roadmap: P2 · Contacts and calendar*

- **Why:** This is the most requested Mailspring feature ever: GitHub #199 (497 👍) and forum #85 (68 votes). Mailspring still ships its calendar as a "Preview" under the Developer menu in 1.26.0, after nine years. Alice already reads invitations, RSVPs, lists calendars and adds events (`crates/alice-core/src/calendar.rs`, `apps/desktop/src/Invitation.tsx`). Showing them is the missing half.
- **Competitors:**
  - **Mailspring.**
    - **Status:** still a preview in 1.26.0, reached from Developer > "Calendar Preview" (`mailspring-app/menus/darwin.js:226`). Main moves it to Window > Calendar.
    - **History:** read-only Google preview in 1.5.6; month view, search, colours and recurrence exceptions in 1.17.0; day view and editing in 1.17.2 (#2574); agenda in 1.17.4; CalDAV discovery for Fastmail and Yahoo plus VTIMEZONE in created events in 1.18.0; double-click create, shortcuts and ICS recurrence editing in 1.19.0 (#2637); about 40 fixes in 1.24–1.26.
    - **Views and editing** (`repos/mailspring/app/internal_packages/main-calendar/lib/core`): Day, Week, Month and Agenda. Drag to create, move and resize, including all-day ↔ timed. Keyboard: arrows move, shift+arrows resize, `mod+t` today, `mod+left/right` page.
    - **Event fields:** title, time, all-day, invitees, notes, location, alert, show-as and time zone. Repeat options are only None/Day/Week/Month/Year.
    - **Recurring edits:** "This occurrence" or "All occurrences". There is **no "this and following"**.
    - **No calendar management** (no create, rename or recolour).
  - **Mailspring engine.**
    - Each `Event` row stores the whole ICS resource, plus a start/end range and UID/RECURRENCE-ID (`mailspring-schema.sql:3,57-67`). A COUNT-bounded series gets a 2037 end date, flagged as a TODO.
    - Expansion happens in the client with `ical-expander`.
    - Sync is CalDAV-only, Google included through `apidata.googleusercontent.com/caldav/v2`. It tries `sync-collection`, then `calendar-query` from −12 to +18 months plus multiget (`DAVWorker.cpp:78-79,1769,1978`).
    - Writes are PUT `{UID}.ics` with `If-Match`. Creates have **no** `If-None-Match`, and a 412 becomes "modified by another client".
    - Refresh every 15 minutes (1.26.0). There is no Exchange support at all.
  - **Mailspring's 1.24–1.26 calendar fixes:**
    - DST rules in VTIMEZONE (#2885, #2900) and Windows zone names (#2897).
    - Server read-back after save (#2915) and an error when the server rejects a save (#2914).
    - No RSVP onto other people's calendars (#2916) and per-occurrence RSVP fixes (#2917).
    - Deleted or unshared calendars are pruned, and event-UID path traversal is fixed.
  - **Other clients.**
    - Thunderbird has day, week, multiweek and month views plus tasks, with an offline cache.
    - eM Client and Outlook have full calendars over Exchange, Google and CalDAV.
    - Apple Mail has none; it delegates to Calendar.app.
- **Spec:**
  - **iCalendar, RFC 5545:** VEVENT §3.6.1, VTIMEZONE §3.6.5, VALARM §3.6.6, RRULE §3.3.10/§3.8.5.3, RDATE/EXDATE §3.8.5.1–2, RECURRENCE-ID and RANGE §3.8.4.4.
  - **CalDAV, RFC 4791:** §4.1 (one UID per resource), §5.3.2 PUT preconditions, §7.8 `calendar-query`, §7.9 `calendar-multiget`, §9.9 time-range.
  - **Related RFCs:** RFC 6578 `sync-collection`; RFC 6638 scheduling (`SCHEDULE-AGENT` §7.1); RFC 5546 iTIP and RFC 6047 iMIP; RFC 7809 time zones by reference; RFC 7986 `COLOR`.
  - **Google Calendar API:** `events.list` with `syncToken` (410 Gone means full resync), `singleEvents`, `showDeleted`; `events.insert/patch/delete` with `If-Match`; `events.instances`; `sendUpdates`.
  - **[Graph `calendarView/delta`](https://learn.microsoft.com/en-us/graph/api/event-delta?view=graph-rest-1.0):** v1.0 tracks **only the primary calendar** and returns expanded instances. Other calendars need `calendarView` polling. Recurrence is `patternedRecurrence` JSON, not RRULE. `Prefer: outlook.timezone`.
- **Alice today:**
  - Calendars are a pass-through: "Alice keeps no events of its own; each request asks the provider" (`crates/alice-core/src/calendar.rs:1-3`).
  - What exists:
    - Listing calendars (`calendar.rs:180-262`). The first CalDAV calendar is assumed primary (`calendar.rs:256`).
    - Adding one event (`calendar.rs:331-383`). It PUTs to a UUIDv5-of-UID resource name with `If-None-Match: *` (`calendar.rs:367-372`, `dav.rs:343-361`), which avoids Mailspring's UID path-traversal bug by construction.
    - Busy times (`calendar.rs:497-560`).
    - ICS parsing with IANA, Windows and embedded-VTIMEZONE zone resolution (`crates/alice-core/src/ical.rs:1-7,388-470,1102`).
  - What doesn't: recurrence is copied (`ical.rs:959`) but never expanded. Microsoft invitations lose their recurrence (README "Current boundaries").
  - Scopes: `calendar.readonly` + `calendar.events` (`oauth.rs:32-35`) and `Calendars.ReadWrite` (`oauth.rs:38`), which is already enough to write.
- **Plan:** (phased; each phase ships on its own)
  1. **Event cache and sync (L).**
     - Tables:
       - `calendars(id, source, owner, remote_id, name, color, read_only, owned, sync_token, ctag, visible)`.
       - `events(id, calendar_id, remote_id_or_href, etag, uid, recurrence_id, raw, start_utc, end_utc, series_end_utc, all_day, tzid, summary, location, status, transp)`, plus FTS on summary, location and description for search.
     - A calendar sync task in the daemon, separate from mail sync: every 15 minutes, on demand and after writes.
       - CalDAV: `sync-collection`, else ctag + `calendar-query` time-range (−12/+18 months) + multiget.
       - Google: `events.list` per calendar with `syncToken`, storing masters and exceptions rather than `singleEvents`.
       - Graph: `calendarView/delta` for the default calendar and `calendarView` for others, plus `seriesMaster` fetched on edit.
     - `GET /v1/calendar/events?from=&to=&calendars=` expands recurrences into the range: RRULE, RDATE, EXDATE and RECURRENCE-ID overrides. Use `calcard` (Stalwart, recurrence expansion included) or the `rrule` crate, keeping Alice's zone resolver in `ical.rs`.
  2. **Read-only views (L).**
     - New `apps/desktop/src/Calendar.tsx`: day, week, month and agenda, an all-day lane, overlap layout, a calendar list with colours and visibility, and the "now" line.
     - Keyboard parity with Mailspring, `g c` to open it, and the existing invite card linking to its event.
     - Recommend FullCalendar's MIT packages (`@fullcalendar/react`, `daygrid`, `timegrid`, `list`, `interaction`) over a hand-rolled grid. Drag, resize and DST-correct layout are the expensive parts. Theme it with Alice's CSS variables.
     - Add CLI `alice calendar agenda` and a TUI agenda pane from the same endpoint.
  3. **Editing (L).**
     - Endpoints: `POST /v1/calendar/events`, `PATCH /v1/calendar/events/{id}?scope=this|following|all`, `DELETE …` with the same scopes.
     - CalDAV: PUT with `If-Match` (or `If-None-Match: *` for creates).
     - Google: `events.patch` with `If-Match` etag. Graph: PATCH an occurrence or the `seriesMaster`.
     - "This occurrence" writes a RECURRENCE-ID override (Google: patch the instance).
     - "This and following" splits the series: UNTIL on the old master and a new master with a new UID. `RANGE=THISANDFUTURE` is poorly supported.
     - "All" edits the master and shifts overrides.
     - Every write is read back (Mailspring 1.26 #2915). On 412, refetch and offer to reapply.
     - Event time zone selector. Write VTIMEZONE with full DST rules, or omit it only where the server advertises RFC 7809.
  4. **Invitations and reminders (M).**
     - Attendees on new events. Google (`sendUpdates=all`) and Graph send invites themselves.
     - For CalDAV, send iMIP `REQUEST`/`CANCEL` from Alice only when the server lacks RFC 6638 auto-scheduling (no `calendar-auto-schedule` in the DAV header). Otherwise let the server do it, to avoid double mails.
     - RSVP from the invite card also writes PARTSTAT to the person's own copy when it is in the cache and owned (Mailspring #2916).
     - VALARM reminders through the existing desktop notifications (`apps/desktop/src-tauri/src/desktop.rs`).
- **Pitfalls:**
  - **Time and dates:**
    - DST and zones caused most of Mailspring's 1.24–1.26 fixes: series created in winter shown an hour off in summer, Windows zone names, invitations redefining UTC.
    - All-day DTEND is exclusive.
    - A COUNT-bounded series has no end date until expanded; store a computed `series_end_utc`, not 2037.
    - Moved instances (RECURRENCE-ID with a different DTSTART) must hide the original slot.
  - **Graph:**
    - v1.0 delta only covers the primary calendar.
    - Mapping `patternedRecurrence` to RRULE is lossy (e.g. `relativeMonthly` with `index: last`).
    - Graph returns times in UTC unless `Prefer: outlook.timezone` is set.
  - **Google:**
    - 429 / `rateLimitExceeded` needs backoff. Mailspring pauses 4 hours on `dailyLimitExceeded`.
    - Sync tokens are invalidated (410) after long gaps.
  - **CalDAV servers:**
    - Nextcloud lists deleted calendars for 30 days.
    - Fastmail exposes scheduling inbox and outbox collections. Alice's `supported-calendar-component-set` filter (`dav.rs:443-458`) already drops non-VEVENT collections; keep resourcetype checks too.
    - Yahoo has no sync token.
    - Read-only and shared calendars: honour `current-user-privilege-set` and never write RSVPs onto calendars you don't own.
  - **Privacy:** event descriptions must stay text, or go through `emailDocument` with images blocked (Mailspring 1.26 #2901).
  - **Performance:** expand only the visible range, and keep the expansion out of SQLite transactions.
- **Done when:**
  - Expansion fixtures pass: a weekly series across a DST change in America/New_York and Europe/Berlin, EXDATE, a moved RECURRENCE-ID, COUNT and UNTIL, all-day, and an Outlook Windows-zone series. Use the existing Google and Exchange fixtures in `ical.rs:1256,1305`.
  - A Radicale integration test covers create → drag (PATCH) → this-occurrence edit → this-and-following split → delete, with etags. A fake-server test covers 412.
  - JSON mapping tests cover Google `events.patch` and Graph `patternedRecurrence`.
  - A desktop smoke test (`scripts/smoke.mjs`) opens week view and finds a seeded event.
- **Effort:** XL in total (phases L + L + L + M). **Note:** Ship phases 1 and 2 (cached, read-only week/agenda) as the MVP. That alone beats Mailspring's "Preview" for Google and Microsoft users, since Mailspring has no Graph support. Depends on "CalDAV discovery for Fastmail, Yahoo, iCloud" and "Local address book cache" (which shares the sync-loop design). Phase 4 benefits from "Respect Do Not Disturb / Focus".

<a id="caldav-discovery-for-fastmail-yahoo-icloud"></a>

### CalDAV discovery for Fastmail, Yahoo, iCloud

*Roadmap: P2 · Contacts and calendar*

- **Why:** Typing `https://caldav.icloud.com` and knowing to make an app-specific password is where people give up. Mailspring forum #531 "Specify custom CardDAV service" (11 votes) and #441 show the demand.
- **Competitors:**
  - **Mailspring** has **no host table in the client or engine**. mailsync POSTs `{domain, imapHost}` to Mailspring's unauthenticated `/api/resolve-dav-hosts` and gets `caldavHost`/`carddavHost` back (`repos/mailsync/MailSync/DAVWorker.cpp:700-714`, `NetworkRequestUtils.cpp:211`). The user's domain therefore leaks to Mailspring.
  - From there it tries `https://{host}/.well-known/caldav` (then `http://`, then the server root), the `current-user-principal`, then `calendar-home-set` at Depth 0. Gmail is hardcoded to Google's CalDAV.
  - Non-OAuth accounts reuse the **IMAP username and app password** for DAV. That makes it one step for iCloud, Yahoo and Fastmail users.
  - Quirks Mailspring hit:
    - Yahoo: 406 errors (1.17.2), a second redirect landing on mail.yahoo.com (`DAVWorker.cpp:650`), and no sync tokens.
    - Fastmail/Cyrus: inbox/outbox collections and principal discovery (1.26.0).
    - Generic: `.well-known` 303/307/308 handling (1.26.0).
  - Thunderbird uses DNS SRV/TXT and `.well-known`, and finds CalDAV from the email domain when adding a calendar.
- **Spec:**
  - RFC 6764: §3 SRV `_caldavs._tcp` / `_carddavs._tcp`, §4 TXT `path=`, §5 `/.well-known/caldav|carddav`, §6 bootstrapping via `current-user-principal`.
  - RFC 5397 (`current-user-principal`).
  - RFC 4791 §6.2.1 (`calendar-home-set`) and RFC 6352 §7.1.1 (`addressbook-home-set`).
  - Providers:
    - iCloud: `https://caldav.icloud.com`, `https://contacts.icloud.com`, with an app-specific password. It redirects to `pNN-caldav.icloud.com`.
    - Fastmail: `https://caldav.fastmail.com`, `https://carddav.fastmail.com`, with an app password; the Basic plan has no DAV ([Fastmail help](https://fastmail.help/hc/en-us/articles/1500000278342)).
    - Yahoo: `https://caldav.calendar.yahoo.com`, `https://carddav.address.yahoo.com`, with an app password.
- **Alice today:**
  - Discovery starts from whatever URL the person types. It probes `/.well-known` only for a bare host, then the principal and home set, and keeps only VEVENT-capable calendars (`crates/alice-core/src/dav.rs:364-480`).
  - The form has free-text Name, URL, Username and Password. The hint mentions iCloud and Fastmail but fills nothing in (`apps/desktop/src/ContactsSettings.tsx:97-107`; string `settings.for_icloud_fastmail_nextcloud_and_other`).
  - DAV sources are separate from mail accounts, with their own password (`crates/alice-core/src/daemon.rs:981-1024`). The IMAP account's app password is never reused.
  - There is no SRV lookup, and the first calendar is "primary" (`calendar.rs:256`).
- **Plan:**
  1. Add a provider table in `alice-core`, shared with the P1 "Presets with setup tips" item:
     - Keys are email domains and IMAP hosts: `icloud.com`/`me.com`/`mac.com` → iCloud; `fastmail.com`/`fastmail.fm`/`messagingengine.com` and MX on `*.messagingengine.com` → Fastmail; `yahoo.*`/`ymail.com`/`rocketmail.com` → Yahoo.
     - Each entry carries both DAV URLs and an app-password help URL.
  2. In `ContactsSettings.tsx`, add "Add iCloud / Fastmail / Yahoo" buttons that prefill the URL and show the app-password steps. Allow the help hosts (`appleid.apple.com`, `fastmail.help`, `help.yahoo.com`) in `open_help_url` (`apps/desktop/src-tauri/src/main.rs:128-147`).
  3. For an IMAP account on a known provider, offer "Also use contacts and calendar" in the account's integration row (`ContactsSettings.tsx:91`). It creates CardDAV and CalDAV sources that reference the account's stored secret instead of asking again.
     - Model: `DavSource.credential: Own | Account(id)`.
     - `contacts::dav_session` (`contacts.rs:659-666`) and `calendar::dav_session` (`calendar.rs:163`) load it accordingly.
  4. For unknown domains:
     - Try `https://{domain}/.well-known/caldav`, then SRV `_caldavs._tcp.{domain}` and its TXT `path`.
     - SRV needs a resolver. `hickory-resolver` would be a new dependency, so ship well-known first and add SRV only if real domains need it. Leave a `ponytail:` note.
  5. Let people pick which discovered calendars are visible and which is the default. That replaces `primary: index == 0`.
- **Pitfalls:**
  - iCloud redirects across hosts within `icloud.com`. Keep credentials on same-domain redirects only (see "(NEW) Don't send DAV Basic credentials across hosts on redirect").
  - Yahoo needs an `Accept` header and a resilient redirect chain (1.17.2).
  - Fastmail's Basic plan returns 401 with a correct password. Say so in the error.
  - Google Workspace and Gmail must keep using the Calendar API, not CalDAV.
  - Microsoft accounts have no CalDAV; route them to Graph.
- **Done when:**
  - A unit test maps `x@me.com`, `x@fastmail.fm`, `x@yahoo.co.uk` and an IMAP host `imap.mail.me.com` to the right preset.
  - A fake-server test follows a `.well-known` redirect to a same-domain partition host with credentials and refuses a cross-domain one.
  - The "Also use contacts and calendar" flow creates two sources with no password prompt.
- **Effort:** S–M. Depends on "Presets with setup tips: iCloud, Yahoo, …" (shares the table) and "(NEW) Don't send DAV Basic credentials across hosts on redirect".

## P2 → Notifications and settings (NEW)

<a id="notification-rules-per-account-and-folder-custom-sound-per-account-onoff-and-onl"></a>

### Notification rules per account and folder: custom sound, per-account on/off and "only people I know", notify for folders other than Inbox

*Roadmap: P2 · Notifications and settings · new*

- **Why:** All-or-nothing notifications fail people with a work and a personal account, and people whose server filters skip the Inbox. Mailspring has none of these. Demand:
  - Forum #530 custom sound (15 votes) and GitHub #415 (11 👍).
  - Forum #133 per-account settings (14 votes) and GitHub #360.
  - Forum #323 and GitHub #667 (18 👍) for notifications on non-Inbox folders.
- **Competitors:**
  - **Mailspring** has only global settings: `core.notifications.enabled` and `sounds`, the badge settings, and Inbox-only notifications (`repos/mailspring/app/src/config-schema.ts:346-387`, `internal_packages/unread-notifications/lib/main.ts:282-285`). Its "custom sounds" package just swaps the bundled `.ogg` files (`internal_packages/custom-sounds/lib/main.ts`). It plays one sound per burst with a 5-second quiet period (`unread-notifications/lib/main.ts:15,248-253`).
  - **Apple Mail:** "New message notifications: Inbox only / VIPs / Contacts / All mailboxes / <smart mailbox>", plus a "New messages sound" picker.
  - **Thunderbird:** per-account and per-folder "check for new messages", and a custom sound file.
  - **Spark:** per account All / Smart / Off.
- **Spec:**
  - **macOS:** `UNNotificationSound soundNamed:`. The file must be in the app bundle or `~/Library/Sounds` (non-sandboxed), in aiff/wav/caf format and under 30 seconds.
  - **Windows:** toasts take `ms-winsoundevent:Notification.*` sounds. [Custom files only via `ms-appx`/`ms-resource`](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-custom-audio); absolute paths are unsupported, so custom sounds need packaged (MSIX) apps.
  - **Linux:** freedesktop hints `sound-file`, `sound-name` (sound naming spec, e.g. `message-new-email`) and `suppress-sound`. notify-rust exposes them as `Hint::SoundFile`, `SoundName` and `SuppressSound`. Some notification servers ignore them.
- **Alice today:**
  - Arrivals are Inbox-only and unread-only (`crates/alice-core/src/daemon.rs:125-147`).
  - `Preferences.notifications` is one global bool (`crates/alice-core/src/model.rs:325-326`).
  - Notices are grouped above 3 (`apps/desktop/src-tauri/src/desktop.rs:26,392-402`).
  - The sound is always the OS default (`notification.rs:172`).
  - Only Inbox, Sent, Archive, Trash and Spam are synced (README "Current boundaries"), so other folders cannot notify yet.
- **Plan:**
  1. Data model:
     - Add `Account.notify: NotifyRule { mode: All | Contacts | Off, folders: Vec<String> /* default ["inbox"] */, sound: Sound }` with `Sound = System | None | Named(String)`.
     - Keep the global `notifications` as the master switch.
     - "Contacts" means the sender is in any address book or you have written to them before. Reuse `contacts::Directory` (`crates/alice-core/src/contacts.rs:50-90`).
  2. In `daemon::arrivals` (`daemon.rs:125`), filter per account by folder and mode. Never notify for Spam or Trash. Add `folder` and `sound` to `Arrival`.
  3. Sounds stay inside the notification, so DND keeps working:
     - **macOS:** bundle 4–6 short `.caf` files in `Contents/Resources`. A user-chosen file is copied into `~/Library/Sounds/Alice-<name>.caf`, accepting aiff/wav/caf under 30 seconds.
     - **Windows:** offer the system `ms-winsoundevent` list only (Default, Mail, IM, Reminder, SMS) through tauri-winrt-notification's `Sound`.
     - **Linux:** use `Hint::SoundFile` for a custom file, or `SoundName("message-new-email")`, and `SuppressSound` for None.
     - Play one sound per check: in a grouped batch, only the first notice carries a sound.
  4. UI: in Settings › Notifications (`apps/desktop/src/Settings.tsx:144-147`), add one row per account with on/off, "Notify for: Everyone / People I know", a folder multi-select and a sound picker with preview. Preview through the page's `Audio`; that is a click, so DND doesn't apply.
- **Pitfalls:**
  - **Folders:** non-Inbox folders need "Sync every folder" first. Gmail labels need the multi-folder model so one message doesn't notify twice. Arrivals are keyed by message ID, which keeps that safe.
  - **Sounds:** Windows custom sound files would mean in-app playback and therefore DND detection, which caused Mailspring's outages (1.10.3, 1.15.1). Don't. Mac App Store builds cannot write to `~/Library/Sounds`, so ship bundled sounds only there.
  - **"People I know"** must not leak network lookups. It is local data only.
- **Done when:**
  - Daemon unit tests cover `arrivals()` with per-account modes, folder lists and Spam exclusion.
  - A desktop unit test maps `Sound` to per-platform payloads.
  - Manual: a custom sound on macOS and Linux and a system sound on Windows play, and are silent under DND.
- **Effort:** M. Depends on "Sync every folder, not only the five special-use roles", "Multi-folder message model" and "Respect Do Not Disturb / Focus".

<a id="settings-export-import-and-sync-across-devices"></a>

### Settings export / import, and sync across devices

*Roadmap: P2 · Notifications and settings · new*

- **Why:** Setting up a second machine, or reinstalling, means redoing accounts, signatures, the AI provider and every preference. Demand:
  - Forum #265 "Sync templates, signatures, accounts etc across multiple devices" (40 votes, Mailspring's 4th most-voted request).
  - GitHub #44 (47 👍), #1043 "Sync / Export / Import Settings" (22) and #755 "Sync Signatures" (11).
- **Competitors:**
  - **Mailspring** has neither export nor sync. Settings live in `config.json` in the config directory (`repos/mailspring/app/src/browser/config-persistence-manager.ts:25`), and people copy that file by hand. Its Mailspring ID syncs only plugin metadata (snooze, send-later, reminders) and signature images (1.24.1).
  - **Apple Mail** syncs rules, smart mailboxes and signatures through iCloud.
  - **New Outlook** roams signatures in the mailbox.
  - **Spark** syncs everything through its own server.
  - **eM Client** has backup/restore and settings export.
- **Spec:**
  - [RFC 5464 IMAP METADATA](https://www.rfc-editor.org/rfc/rfc5464): private server annotations such as `/private/vendor/getalice/settings`, subject to `MAXSIZE`. Cyrus and Fastmail support it, and so does Dovecot when `imap_metadata` is enabled. Gmail, Outlook.com, iCloud and Yahoo do not advertise it.
  - IMAP APPEND (RFC 9051 §6.3.12), for the folder fallback.
- **Alice today:**
  - Preferences are one JSON row (`settings.id='preferences'`, `crates/alice-core/src/store.rs:642-746`).
  - AI settings are row `'ai'` (`store.rs:471-485`).
  - Accounts and DAV sources are JSON rows (`store.rs:19,25`). Secrets live in the OS keychain (`crates/alice-core/src/vault.rs`).
  - The signature is one global string (`model.rs:323`).
  - There is no export, import or sync.
- **Plan:**
  1. **Export/import file (ship first).**
     - `GET /v1/settings/export` returns `{format:"alice-settings", version:1, exported_at, preferences, ai:{provider,model,base_url}, accounts:[{name,email,provider,imap/smtp host+port,integrations}], dav:[{kind,name,url,username}]}`.
     - Later add signatures, templates and rules as those items land.
     - **No secrets, tokens, client secrets or API keys.**
     - `POST /v1/settings/import` previews, then merges. Accounts and DAV sources are created in a "sign in to finish" state, keyed by email + provider so they don't duplicate.
     - Add a CLI `alice settings export|import`, and a desktop Settings › About › "Export settings…" that saves to Downloads like attachments do.
  2. **Device-local keys never travel:** `menu_bar`, `system_contacts`, `otp_autocopy`, `mcp` access level (a security boundary), and `default_calendar` IDs that don't resolve.
  3. **Optional sync (later).**
     - Pick one "sync account". Store the same JSON, without device-local keys, as a single message in an `Alice Settings` folder: APPEND the new copy, then delete the old one, marked by `X-Alice-Settings: 1`. Exclude that folder from mail sync, search and counts. This works on every IMAP server, including Gmail.
     - Use METADATA instead when the server advertises it.
     - Merge per section with `updated_at` timestamps, last writer wins per section.
- **Pitfalls:**
  - Account IDs are per-device UUIDs, so map by email + provider.
  - Gmail shows the folder as a label. Name it clearly, and let people opt out of sync.
  - JSON settings include hostnames and email addresses, which is fine in the person's own mailbox but should be stated.
  - Never sync MCP access or OTP auto-copy: enabling agent access on one machine must not enable it everywhere.
  - A schema version is needed for forward compatibility. Unknown fields should be ignored on import, not rejected (`PreferencesPatch` uses `deny_unknown_fields` for the API; keep the export reader lenient).
- **Done when:**
  - A round-trip unit test (export → wipe → import) restores preferences and recreates accounts as "needs sign-in".
  - A test asserts the export contains no `password`, `token`, `secret` or `api_key` keys.
  - A Dovecot fixture test (`scripts/test-mail.py`) syncs settings between two data dirs through the IMAP folder.
- **Effort:** M (file), L (IMAP sync). Depends on "Per-account signatures", "Templates with subject…" and "Mail rules / filters" (for more payload), and "Sync every folder…" (to hide the settings folder).
