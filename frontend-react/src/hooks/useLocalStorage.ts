import { useCallback, useState } from 'react';
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
        return resolved;
      });
    },
    [key],
  );

  return [value, set];
}
