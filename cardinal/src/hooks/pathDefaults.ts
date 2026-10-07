// Default ignore/include paths shared by useIgnorePaths and useIncludePaths.
//
// Matching is longest-prefix (see fswalk::should_ignore_path), so broad system
// trees are ignored wholesale and the few user-relevant subfolders are rescued
// through the include list.
//
// Ignored:
// - `/Volumes` can include external drives and network mounts that are large or slow to traverse.
// - `/System` is the sealed OS volume; `/System/Volumes/Data` also mirrors the whole data volume.
// - `/Library` holds system-wide frameworks, extensions, receipts, caches, and app support data.
// - `/private` (and the `/etc`, `/tmp`, `/var` symlinks into it) is system runtime state.
// - `/usr`, `/bin`, `/sbin`, `/cores`, `/dev`, `/opt` are Unix system and package-manager trees.
// - `~/Library` is per-user app data (containers, caches, mail, messages, WebKit, preferences).
// - `~/.cargo`, `~/.rustup`, `~/.npm`, `~/.cache`, `~/.local`, `~/.vscode/extensions` are
//   toolchain and package caches with hundreds of thousands of files.
//
// Included (rescued from the ignored trees above):
// - `~/Library/CloudStorage` holds OneDrive/Dropbox/Google Drive; home-folder shortcuts point here.
// - `~/Library/Mobile Documents` is iCloud Drive.
// - `~/Library/Fonts` and `/Library/Fonts` are user-installed fonts.
export const DEFAULT_IGNORE_PATHS = [
  '/Volumes',
  '/System',
  '/Library',
  '/private',
  '/usr',
  '/bin',
  '/sbin',
  '/cores',
  '/dev',
  '/opt',
  '~/Library',
  '~/.cargo',
  '~/.rustup',
  '~/.npm',
  '~/.cache',
  '~/.local',
  '~/.vscode/extensions',
];

export const DEFAULT_INCLUDE_PATHS = [
  '~/Library/CloudStorage',
  '~/Library/Mobile Documents',
  '~/Library/Fonts',
  '/Library/Fonts',
];

// Upstream's original ignore defaults. Saved lists equal to these entries are
// treated as untouched defaults during migration.
const LEGACY_DEFAULT_IGNORE_PATHS = [
  '/Volumes',
  '~/Library/CloudStorage',
  '~/Library/Biome',
  '~/Library/Caches',
  '~/Library/Logs',
  '~/Library/Metadata',
  '/Library/Caches',
  '/System/Library/Caches',
  '/private/var',
  '/private/tmp',
];

const IGNORE_KEY = 'cardinal.ignorePaths';
const INCLUDE_KEY = 'cardinal.includePaths';
const VERSION_KEY = 'cardinal.pathDefaultsVersion';
const CURRENT_VERSION = '2';

let migrated = false;

const readList = (key: string): string[] | null => {
  const raw = window.localStorage.getItem(key);
  if (raw == null) return null;
  const parsed = JSON.parse(raw);
  return Array.isArray(parsed)
    ? parsed.filter((item): item is string => typeof item === 'string')
    : null;
};

const union = (base: string[], extra: string[]): string[] => [
  ...base,
  ...extra.filter((item) => !base.includes(item)),
];

// One-time upgrade of saved path lists to the current defaults, keeping any
// paths the user added themselves.
export function migratePathDefaults(): void {
  if (migrated || typeof window === 'undefined') return;
  migrated = true;

  try {
    if (window.localStorage.getItem(VERSION_KEY) === CURRENT_VERSION) return;

    const savedIgnore = readList(IGNORE_KEY);
    if (savedIgnore != null) {
      const userAdded = savedIgnore.filter((item) => !LEGACY_DEFAULT_IGNORE_PATHS.includes(item));
      window.localStorage.setItem(
        IGNORE_KEY,
        JSON.stringify(union(DEFAULT_IGNORE_PATHS, userAdded)),
      );
    }

    const savedInclude = readList(INCLUDE_KEY);
    if (savedInclude != null) {
      window.localStorage.setItem(
        INCLUDE_KEY,
        JSON.stringify(union(DEFAULT_INCLUDE_PATHS, savedInclude)),
      );
    }

    window.localStorage.setItem(VERSION_KEY, CURRENT_VERSION);
  } catch (error) {
    console.warn('Unable to migrate default path lists', error);
  }
}

// Test hook: allow the migration to run again after localStorage is cleared.
export function resetPathDefaultsMigration(): void {
  migrated = false;
}
