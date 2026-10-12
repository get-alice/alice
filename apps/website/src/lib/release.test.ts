import assert from 'node:assert/strict';
import { test } from 'node:test';
import { platformsFrom } from './release.ts';

test('picks Tauri bundle names for each platform, best first', () => {
  const names = ['Alice_0.2.0_x64.dmg', 'Alice_0.2.0_aarch64.dmg', 'Alice_0.2.0_x64_en-US.msi', 'Alice_0.2.0_x64-setup.exe', 'Alice_0.2.0_amd64.deb', 'Alice_0.2.0_amd64.AppImage', 'Alice-0.2.0-1.x86_64.rpm', 'latest.json'];
  const platforms = platformsFrom(names.map(name => ({ name, browser_download_url: `https://example.org/${name}` })));
  assert.deepEqual(platforms.mac.map(f => f.label), ['Apple silicon (.dmg)', 'Intel (.dmg)']);
  assert.deepEqual(platforms.windows.map(f => f.label), ['Installer (.exe)', 'Installer (.msi)']);
  assert.deepEqual(platforms.linux.map(f => f.label), ['AppImage', 'Debian, Ubuntu (.deb)', 'Fedora (.rpm)']);
  assert.equal(platforms.windows[0]!.url, 'https://example.org/Alice_0.2.0_x64-setup.exe');
});

test('a platform without assets exposes no installer', () => {
  const platforms = platformsFrom([{ name: 'Alice_0.2.0_aarch64.dmg', browser_download_url: 'https://example.org/a.dmg' }]);
  assert.deepEqual(platforms.linux, []);
  assert.deepEqual(platforms.windows, []);
  assert.equal(platforms.mac[0]!.url, 'https://example.org/a.dmg');
});
