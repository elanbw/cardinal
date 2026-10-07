import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { DEFAULT_INCLUDE_PATHS, resetPathDefaultsMigration } from '../pathDefaults';
import { useIncludePaths } from '../useIncludePaths';

const STORAGE_KEY = 'cardinal.includePaths';

const flushEffects = async () => {
  await act(async () => {});
};

describe('useIncludePaths', () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.localStorage.setItem('cardinal.pathDefaultsVersion', '2');
    resetPathDefaultsMigration();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('uses cloud, iCloud, and font folders by default and persists them once', async () => {
    const setItemSpy = vi.spyOn(Storage.prototype, 'setItem');

    const { result } = renderHook(() => useIncludePaths());

    expect(result.current.includePaths).toEqual(DEFAULT_INCLUDE_PATHS);
    expect(result.current.defaultIncludePaths).toContain('~/Library/CloudStorage');

    await flushEffects();

    // Mirrors useIgnorePaths: on first run with no stored value the hook writes
    // the default to localStorage so subsequent reads short-circuit.
    expect(setItemSpy).toHaveBeenCalledWith(STORAGE_KEY, JSON.stringify(DEFAULT_INCLUDE_PATHS));
  });

  it('hydrates from stored values and filters invalid entries', async () => {
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify([' /Volumes/media ', '', 42, '   ', '/Volumes/work']),
    );
    const setItemSpy = vi.spyOn(Storage.prototype, 'setItem');

    const { result } = renderHook(() => useIncludePaths());

    expect(result.current.includePaths).toEqual(['/Volumes/media', '/Volumes/work']);

    await flushEffects();

    expect(setItemSpy).not.toHaveBeenCalled();
  });

  it('cleans and persists updates', async () => {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify([]));
    const setItemSpy = vi.spyOn(Storage.prototype, 'setItem');

    const { result } = renderHook(() => useIncludePaths());

    await flushEffects();

    act(() => {
      result.current.setIncludePaths([' /Volumes/media ', '', '/Volumes/work', '   ']);
    });

    expect(result.current.includePaths).toEqual(['/Volumes/media', '/Volumes/work']);
    expect(setItemSpy).toHaveBeenCalledWith(
      STORAGE_KEY,
      JSON.stringify(['/Volumes/media', '/Volumes/work']),
    );
  });

  it('falls back to defaults when stored JSON is invalid', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    window.localStorage.setItem(STORAGE_KEY, '{');

    const { result } = renderHook(() => useIncludePaths());

    expect(result.current.includePaths).toEqual(DEFAULT_INCLUDE_PATHS);

    await flushEffects();

    warnSpy.mockRestore();
  });
});
