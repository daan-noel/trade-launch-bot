import { useCallback, useEffect, useRef, useState } from 'react';
import { getJSON, setJSON } from 'lib/storage';

export interface UseLocalStorageOptions {
  /**
   * Debounce persistence (localStorage write + same-tab broadcast) while keeping
   * React state updates immediate. Use for high-churn text (filter boxes) so
   * every keystroke doesn't hit disk / wake sibling hooks. Flushes on unmount.
   */
  debounceMs?: number;
}

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
  options?: UseLocalStorageOptions,
): [T, (value: T | ((prev: T) => T)) => void] {
  const debounceMs = options?.debounceMs;
  const [value, setValue] = useState<T>(() => getJSON(key, initial));
  const pendingRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const latestRef = useRef(value);
  latestRef.current = value;

  const persist = useCallback(
    (resolved: T) => {
      setJSON(key, resolved);
      window.dispatchEvent(
        new CustomEvent<T>(`${LS_SYNC_EVENT}:${key}`, { detail: resolved }),
      );
    },
    [key],
  );

  const flush = useCallback(() => {
    if (pendingRef.current != null) {
      clearTimeout(pendingRef.current);
      pendingRef.current = null;
      persist(latestRef.current);
    }
  }, [persist]);

  const set = useCallback(
    (next: T | ((prev: T) => T)) => {
      setValue((prev) => {
        const resolved = next instanceof Function ? next(prev) : next;
        latestRef.current = resolved;
        if (debounceMs != null && debounceMs > 0) {
          if (pendingRef.current != null) clearTimeout(pendingRef.current);
          pendingRef.current = setTimeout(() => {
            pendingRef.current = null;
            persist(latestRef.current);
          }, debounceMs);
        } else {
          persist(resolved);
        }
        return resolved;
      });
    },
    [debounceMs, persist],
  );

  // Flush a pending debounced write on unmount so a fast navigate doesn't drop
  // the last keystrokes.
  useEffect(() => () => flush(), [flush]);

  useEffect(() => {
    const onLocal = (e: Event) => {
      // Sibling hook wrote — cancel our pending debounce so we don't overwrite
      // their fresher value a tick later.
      if (pendingRef.current != null) {
        clearTimeout(pendingRef.current);
        pendingRef.current = null;
      }
      const detail = (e as CustomEvent<T>).detail;
      latestRef.current = detail;
      setValue(detail);
    };
    const onCrossTab = (e: StorageEvent) => {
      if (e.key !== key) return;
      if (pendingRef.current != null) {
        clearTimeout(pendingRef.current);
        pendingRef.current = null;
      }
      const next = getJSON(key, initial);
      latestRef.current = next;
      setValue(next);
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
