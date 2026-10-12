/*
 * The OS the app runs on, for platform words (Finder, menu bar) and shortcut modifiers. `navigator.platform`
 * is deprecated, but its replacement, `navigator.userAgentData`, is missing from WebKit, which Tauri uses
 * on macOS and Linux.
 */
// eslint-disable-next-line @typescript-eslint/no-deprecated
const name = navigator.platform;
export const isMac = /Mac|iPhone|iPad/.test(name);
export const isWindows = name.startsWith('Win');
