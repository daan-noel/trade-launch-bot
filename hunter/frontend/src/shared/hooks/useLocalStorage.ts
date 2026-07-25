import { useCallback, useEffect, useState } from 'react';
import { getJSON, setJSON } from 'lib/storage';

/**
 * `useState` that persists to `localStorage` under `key` (via the shared
 * `lib/storage` layer — JSON, try/catch, namespaced `mt:` keys).
 *
 * Lazy-reads the stored value once on mount (falling back to `initial` when
 * absent or unparseable) and writes back on every change. Pass a key from
 * `STORAGE_KEYS` so it stays in the central registry.
 *
 * `initial` is read only on first mount; pass a stable default (or memoise a
 * computed one) since later changes to it are ignored.
 *
 * **All instances of the same `key` stay in sync.** A write broadcasts to every
 * other hook bound to that key in the same tab (a `CustomEvent`, since the
 * `storage` event only fires cross-tab), and a `storage` listener picks up
 * changes from other tabs. Without this, a component that reads the value once
 * (e.g. the always-mounted notification listener) would keep firing on a stale
 * copy after Settings changed it — until a full page reload.
 */
export function useLocalStorage<T>(
  key: string,
  initial: T,
): [T, (value: T | ((prev: T) => T)) => void] {
  const [value, setValue] = useState<T>(() => getJSON(key, initial));

  const set = useCallback(
    (next: T | ((prev: T) => T)) => {
      setValue((prev) => {
        const resolved = next instanceof Function ? next(prev) : next;
        setJSON(key, resolved);
        // Broadcast to sibling hooks in this tab (storage events are cross-tab only).
        window.dispatchEvent(
          new CustomEvent<T>(`${LS_SYNC_EVENT}:${key}`, { detail: resolved }),
        );
        return resolved;
      });
    },
    [key],
  );

  useEffect(() => {
    const onLocal = (e: Event) => {
      setValue((e as CustomEvent<T>).detail);
    };
    const onCrossTab = (e: StorageEvent) => {
      if (e.key === key) setValue(getJSON(key, initial));
    };
    window.addEventListener(`${LS_SYNC_EVENT}:${key}`, onLocal);
    window.addEventListener('storage', onCrossTab);
    return () => {
      window.removeEventListener(`${LS_SYNC_EVENT}:${key}`, onLocal);
      window.removeEventListener('storage', onCrossTab);
    };
    // `initial` is intentionally excluded — it's the first-mount default only
    // (documented above), so a changing default must not resubscribe/reset.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  return [value, set];
}

/** Same-tab sync channel prefix; per-key event name is `${LS_SYNC_EVENT}:${key}`. */
const LS_SYNC_EVENT = 'mt:ls-sync';
