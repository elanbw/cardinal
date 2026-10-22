import { beforeEach, describe, expect, it } from 'vitest';

import {
  DEFAULT_IGNORE_PATHS,
  DEFAULT_INCLUDE_PATHS,
  migratePathDefaults,
  resetPathDefaultsMigration,
} from '../pathDefaults';

const IGNORE_KEY = 'cardinal.ignorePaths';
const INCLUDE_KEY = 'cardinal.includePaths';
const VERSION_KEY = 'cardinal.pathDefaultsVersion';

const LEGACY_IGNORE = [
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

const readJson = (key: string) => JSON.parse(window.localStorage.getItem(key) ?? 'null');

describe('migratePathDefaults', () => {
  beforeEach(() => {
    window.localStorage.clear();
    resetPathDefaultsMigration();
  });

  it('replaces legacy defaults and keeps user-added paths', () => {
    window.localStorage.setItem(IGNORE_KEY, JSON.stringify([...LEGACY_IGNORE, '/Users/me/junk']));
    window.localStorage.setItem(INCLUDE_KEY, JSON.stringify(['/Volumes/media']));

    migratePathDefaults();

    expect(readJson(IGNORE_KEY)).toEqual([...DEFAULT_IGNORE_PATHS, '/Users/me/junk']);
    expect(readJson(INCLUDE_KEY)).toEqual([...DEFAULT_INCLUDE_PATHS, '/Volumes/media']);
    expect(window.localStorage.getItem(VERSION_KEY)).toBe('3');
  });

  it('leaves fresh installs to the hooks and marks the version', () => {
    migratePathDefaults();

    expect(window.localStorage.getItem(IGNORE_KEY)).toBeNull();
    expect(window.localStorage.getItem(INCLUDE_KEY)).toBeNull();
    expect(window.localStorage.getItem(VERSION_KEY)).toBe('3');
  });

  it('runs only once per version', () => {
    window.localStorage.setItem(VERSION_KEY, '3');
    window.localStorage.setItem(IGNORE_KEY, JSON.stringify(['/only']));

    migratePathDefaults();

    expect(readJson(IGNORE_KEY)).toEqual(['/only']);
  });
});
