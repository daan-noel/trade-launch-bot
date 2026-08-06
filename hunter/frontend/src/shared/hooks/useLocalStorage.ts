import { useCallback, useEffect, useRef, useState } from 'react';
import { getField, getJSON, setField, setJSON } from 'lib/storage';

export interface UseLocalStorageOptions {
  /**
   * Debounce persistence (localStorage write + same-tab broadcast) while keeping
   * React state updates immediate. Use for high-churn text (filter boxes) so
   * every keystroke doesn't hit disk / wake sibling hooks. Flushes on unmount.
   */
  debounceMs?: number;
}

/** Same-tab sync channel prefix; per-channel event name is `${LS_SYNC_EVENT}:${channel}`. */
const LS_SYNC_EVENT = 'mt:ls-sync';

type Setter<T> = (value: T | ((prev: T) => T)) => void;

/**
 * The shared `useState`-that-persists machine: immediate React state, optionally
 * debounced writes, same-tab broadcast on `channel`, and a re-read when another
 * tab writes `watchKey`. `read`/`write` are held in refs, so a caller may pass
 * fresh closures every render.
 */
function usePersisted<T>(
  channel: string,
  watchKey: string,
  read: () => T,
  write: (value: T) => void,
  debounceMs?: number,
): [T, Setter<T>] {
  const readRef = useRef(read);
  readRef.current = read;
  const writeRef = useRef(write);
  writeRef.current = write;

  const [value, setValue] = useState<T>(read);
  const pendingRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const latestRef = useRef(value);
  latestRef.current = value;

  const persist = useCallback(
    (resolved: T) => {
      writeRef.current(resolved);
      window.dispatchEvent(
        new CustomEvent<T>(`${LS_SYNC_EVENT}:${channel}`, { detail: resolved }),
      );
    },
    [channel],
  );

  const flush = useCallback(() => {
    if (pendingRef.current != null) {
      clearTimeout(pendingRef.current);
      pendingRef.current = null;
      persist(latestRef.current);
    }
  }, [persist]);

  const set = useCallback<Setter<T>>(
    (next) => {
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
      if (e.key !== watchKey) return;
      if (pendingRef.current != null) {
        clearTimeout(pendingRef.current);
        pendingRef.current = null;
      }
      const next = readRef.current();
      latestRef.current = next;
      setValue(next);
    };
    window.addEventListener(`${LS_SYNC_EVENT}:${channel}`, onLocal);
    window.addEventListener('storage', onCrossTab);
    return () => {
      window.removeEventListener(`${LS_SYNC_EVENT}:${channel}`, onLocal);
      window.removeEventListener('storage', onCrossTab);
    };
  }, [channel, watchKey]);

  return [value, set];
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
): [T, Setter<T>] {
  return usePersisted<T>(
    key,
    key,
    () => getJSON(key, initial),
    (v) => setJSON(key, v),
    options?.debounceMs,
  );
}

/**
 * {@link useLocalStorage} for ONE field of a shared blob (`mt:ui.toggles`,
 * `mt:page.creationStats`, …). Same contract, one extra guarantee: the write is
 * read-modify-write against storage, so two fields of the same blob never
 * clobber each other — including when one of them is debounced.
 *
 * Prefer this over a flat key per preference; the registry stays short and a new
 * pref costs a field, not a key.
 */
export function useStoredField<T>(
  key: string,
  field: string,
  initial: T,
  options?: UseLocalStorageOptions,
): [T, Setter<T>] {
  return usePersisted<T>(
    `${key}#${field}`,
    key,
    () => getField(key, field, initial),
    (v) => setField(key, field, v),
    options?.debounceMs,
  );
}
