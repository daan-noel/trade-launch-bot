import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { getTablePins, setTablePins } from 'lib/storage';

/** What {@link usePinnedRows} hands to a DataTable's `pinning` prop. */
export interface PinnedRows<R> {
  /** True when this row is currently pinned. */
  isPinned: (row: R) => boolean;
  /** O(1) key lookup — prefer this when the row key is already in hand. */
  isPinnedKey: (key: string) => boolean;
  /** Flip the pinned state of a row (pins if unpinned, unpins if pinned). */
  onToggle: (row: R) => void;
  /** The pinned rows to render in the sticky top section, in pin order. */
  pinnedRows: R[];
}

const PINS_PERSIST_MS = 150;

/**
 * Row-pinning state for a {@link DataTable}, scoped per `tableId` and persisted via
 * the shared storage layer so pins survive reloads. `DataTable` calls this itself
 * when given `pinnable` + a `tableId`; a caller rarely needs it directly.
 *
 * Pass an **undefined** `tableId` to make the hook fully inert (no storage, no
 * state churn) — this is how `DataTable` keeps a single unconditional hook call
 * while pinning stays opt-in per table.
 *
 * Hunter tables are frequently **server-paged** (the token universe alone is 100K+),
 * so a pinned row is usually not in the page the table currently holds. We therefore
 * snapshot the full row object at pin time (it's on-screen when you click) and keep
 * it — the snapshot persists across reloads and is refreshed from whichever page
 * later contains it, so a pinned row stays live while you're near it and shows
 * last-known data otherwise. No extra query.
 */
export function usePinnedRows<R>(
  tableId: string | undefined,
  rowKey: (row: R) => string,
  pageRows: R[],
): PinnedRows<R> {
  const enabled = !!tableId;
  const initial = useRef<{ keys: string[]; rows: Record<string, R> } | undefined>(undefined);
  if (!initial.current) {
    const stored = tableId ? getTablePins(tableId) : { keys: [], rows: {} };
    initial.current = { keys: stored.keys, rows: stored.rows as Record<string, R> };
  }

  const [keys, setKeys] = useState<string[]>(initial.current.keys);
  // key -> last-seen full row object; state (not a ref) so the pinned section
  // re-renders when a poll hands back a fresher copy.
  const [snapshots, setSnapshots] = useState<Record<string, R>>(initial.current.rows);

  // Stable rowKey identity so the callbacks/effect below don't churn when the
  // caller passes an inline `(r) => r.mint_address`.
  const keyRef = useRef(rowKey);
  keyRef.current = rowKey;

  const keySet = useMemo(() => new Set(keys), [keys]);

  // Refresh snapshots from the freshest page data for any pinned key present.
  useEffect(() => {
    if (!enabled || keys.length === 0) return;
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
  // Debounced: rapid pin/unpin or snapshot refresh from polling shouldn't hit
  // localStorage on every tick.
  useEffect(() => {
    if (!tableId) return;
    const id = window.setTimeout(() => {
      const rows: Record<string, R> = {};
      for (const k of keys) if (snapshots[k] !== undefined) rows[k] = snapshots[k];
      setTablePins(tableId, { keys, rows });
    }, PINS_PERSIST_MS);
    return () => window.clearTimeout(id);
  }, [tableId, keys, snapshots]);

  const onToggle = useCallback((row: R) => {
    const k = keyRef.current(row);
    setKeys((prev) => (prev.includes(k) ? prev.filter((x) => x !== k) : [...prev, k]));
    setSnapshots((prev) => ({ ...prev, [k]: row })); // capture at pin time
  }, []);

  const isPinnedKey = useCallback((key: string) => keySet.has(key), [keySet]);
  const isPinned = useCallback(
    (row: R) => keySet.has(keyRef.current(row)),
    [keySet],
  );

  const pinnedRows = useMemo(
    () => keys.map((k) => snapshots[k]).filter((r): r is R => r !== undefined),
    [keys, snapshots],
  );

  return { isPinned, isPinnedKey, onToggle, pinnedRows };
}
