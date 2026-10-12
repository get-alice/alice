export const repo = 'https://github.com/get-alice/alice';

export type Platform = 'mac' | 'windows' | 'linux';
export interface Download { label: string; url: string }
export interface Release { version: string | null; platforms: Record<Platform, Download[]> }

// Order inside each list is preference: the first match is the one the big button offers.
const patterns: Record<Platform, [RegExp, string][]> = {
  mac: [[/universal\.dmg$/i, 'Universal (.dmg)'], [/(aarch64|arm64)\.dmg$/i, 'Apple silicon (.dmg)'], [/(x64|x86_64)\.dmg$/i, 'Intel (.dmg)']],
  windows: [[/setup\.exe$/i, 'Installer (.exe)'], [/\.msi$/i, 'Installer (.msi)']],
  linux: [[/\.AppImage$/, 'AppImage'], [/\.deb$/i, 'Debian, Ubuntu (.deb)'], [/\.rpm$/i, 'Fedora (.rpm)']],
};

const fallback = (): Release => ({
  version: null,
  platforms: { mac: [], windows: [], linux: [] },
});

/** Only verified release assets are offered as installers. */
export function platformsFrom(assets: { name: string; browser_download_url: string }[]): Release['platforms'] {
  const platforms = fallback().platforms;
  for (const platform of Object.keys(patterns) as Platform[]) {
    const found = patterns[platform].flatMap(([pattern, label]) => {
      const asset = assets.find(a => pattern.test(a.name));
      return asset ? [{ label, url: asset.browser_download_url }] : [];
    });
    if (found.length) platforms[platform] = found;
  }
  return platforms;
}

/** Reads the latest GitHub release at build time; unavailable metadata leaves installer choices empty. */
export async function latestRelease(): Promise<Release> {
  try {
    const response = await fetch('https://api.github.com/repos/get-alice/alice/releases/latest', {
      headers: { Accept: 'application/vnd.github+json', ...(process.env.GITHUB_TOKEN ? { Authorization: `Bearer ${process.env.GITHUB_TOKEN}` } : {}) },
      signal: AbortSignal.timeout(8000),
    });
    if (!response.ok) return fallback();
    const data = await response.json() as { tag_name: string; assets: { name: string; browser_download_url: string }[] };
    return { version: data.tag_name.replace(/^v/, ''), platforms: platformsFrom(data.assets) };
  } catch {
    return fallback();
  }
}
