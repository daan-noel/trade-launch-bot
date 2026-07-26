import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

/** What {@link usePinnedRows} hands to a DataTable's `pinning` prop. */
export interface PinnedRows<T> {
  /** True when this row is currently pinned. */
  isPinned: (row: T) => boolean;
  /** Flip the pinned state of a row (pins if unpinned, unpins if pinned). */
  onToggle: (row: T) => void;
  /** The pinned rows to render in the sticky top section, in pin order. */
  pinnedRows: T[];
}

interface Persisted<T> {
  keys: string[];
  rows: Record<string, T>;
}

const STORAGE_PREFIX = 'dt-pins:';

function load<T>(tableId: string): Persisted<T> {
  try {
    const raw = localStorage.getItem(STORAGE_PREFIX + tableId);
    if (!raw) return { keys: [], rows: {} };
    const p = JSON.parse(raw) as Partial<Persisted<T>>;
    return { keys: Array.isArray(p.keys) ? p.keys : [], rows: p.rows ?? {} };
  } catch {
    return { keys: [], rows: {} };
  }
}

/**
 * Row-pinning state for a {@link DataTable}, scoped per `tableId` and persisted to
 * localStorage so pins survive reloads. Feed the result straight into DataTable's
 * `pinning` prop.
 *
 * forge tables are **server-paged**, so a pinned row is often not in the page the
 * table currently holds. We therefore snapshot the full row object at pin time (it's
 * on-screen when you click) and keep it — the snapshot both persists across reloads
 * and is refreshed from whichever page later contains it, so a pinned row stays live
 * while you're near it and shows last-known data otherwise. No extra server calls.
 */
export function usePinnedRows<T>(
  tableId: string,
  rowKey: (row: T) => string,
  pageRows: T[],
): PinnedRows<T> {
  const initial = useRef<Persisted<T>>();
  if (!initial.current) initial.current = load<T>(tableId);

  const [keys, setKeys] = useState<string[]>(initial.current.keys);
  // key -> last-seen full row object; state (not a ref) so the pinned section
  // re-renders when a poll hands back a fresher copy.
  const [snapshots, setSnapshots] = useState<Record<string, T>>(initial.current.rows);

  // Stable rowKey identity so the callbacks/effect below don't churn when the
  // caller passes an inline `(r) => r.id`.
  const keyRef = useRef(rowKey);
  keyRef.current = rowKey;

  const keySet = useMemo(() => new Set(keys), [keys]);

  // Refresh snapshots from the freshest page data for any pinned key present.
  useEffect(() => {
    if (keys.length === 0) return;
    setSnapshots((prev) => {
      let next = prev;
      for (const row of pageRows) {
        const k = keyRef.current(row);
        if (keySet.has(k) && prev[k] !== row) {
          if (next === prev) next = { ...prev };
          next[k] = row;
        }
      }
      return next;
    });
  }, [pageRows, keySet, keys.length]);

  // Persist keys + snapshots together so a reload can render pinned rows immediately.
  useEffect(() => {
    try {
      const rows: Record<string, T> = {};
      for (const k of keys) if (snapshots[k] !== undefined) rows[k] = snapshots[k];
      localStorage.setItem(STORAGE_PREFIX + tableId, JSON.stringify({ keys, rows }));
    } catch {
      /* storage full / disabled — pins just won't persist this session */
    }
  }, [tableId, keys, snapshots]);

  const onToggle = useCallback((row: T) => {
    const k = keyRef.current(row);
    setKeys((prev) => (prev.includes(k) ? prev.filter((x) => x !== k) : [...prev, k]));
    setSnapshots((prev) => ({ ...prev, [k]: row })); // capture at pin time
  }, []);

  const isPinned = useCallback((row: T) => keySet.has(keyRef.current(row)), [keySet]);

  const pinnedRows = useMemo(
    () => keys.map((k) => snapshots[k]).filter((r): r is T => r !== undefined),
    [keys, snapshots],
  );

  return { isPinned, onToggle, pinnedRows };
}
