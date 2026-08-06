import { useCallback, useEffect, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';

import { STORAGE_KEYS, getString, remove, setString } from 'lib/storage';
import {
  isDefaultModeFilter,
  MODE_PARAM,
  parseModeFilter,
  type ModeFilter,
} from 'lib/strategy/mode';

/**
 * Trade-mode scope for a rule board, synced to the URL (`?mode=`) and mirrored
 * to `localStorage` — the same contract as {@link useTagFilter}, with which it
 * composes (a mode-scoped board keeps its `?tags=` and `?rule=` params).
 *
 * Precedence: **the URL wins when it carries the param**, so a pasted link
 * shows exactly what the sender saw. Otherwise the last-used scope is restored
 * from storage, which is what makes "I'm iterating on paper today" stick across
 * a reload instead of snapping back to `all` every visit.
 *
 * Pass a per-page `pageId` (Rules, Rules Control and Simulate each keep their
 * own scope — they are different jobs and should not drag each other); the `mt:`
 * key is built here, so no call site owns a raw key.
 */
export function useModeFilter(
  pageId: string,
): [ModeFilter, (next: ModeFilter) => void] {
  const storageKey = `${STORAGE_KEYS.filterMode}.${pageId}`;
  const [searchParams, setSearchParams] = useSearchParams();
  const urlHasFilter = searchParams.has(MODE_PARAM);

  const [filter, setFilter] = useState<ModeFilter>(() => {
    if (urlHasFilter) return parseModeFilter(searchParams.get(MODE_PARAM));
    return parseModeFilter(getString(storageKey));
  });

  // Seed the URL from a restored scope once, so the address bar always
  // describes what is on screen (and the board stays shareable).
  const seeded = useRef(false);

  const apply = useCallback(
    (next: ModeFilter) => {
      setFilter(next);
      // Storage is a convenience mirror; the URL is the real state.
      if (isDefaultModeFilter(next)) remove(storageKey);
      else setString(storageKey, next);
      setSearchParams(
        (prev) => {
          const params = new URLSearchParams(prev);
          // `all` leaves no param behind, so a cleared scope produces a clean URL.
          if (isDefaultModeFilter(next)) params.delete(MODE_PARAM);
          else params.set(MODE_PARAM, next);
          return params;
        },
        { replace: true },
      );
    },
    [storageKey, setSearchParams],
  );

  useEffect(() => {
    if (seeded.current) return;
    seeded.current = true;
    if (!urlHasFilter && !isDefaultModeFilter(filter)) apply(filter);
  }, [urlHasFilter, filter, apply]);

  // Browser back/forward rewrites the params — follow them.
  useEffect(() => {
    if (!urlHasFilter) return;
    const fromUrl = parseModeFilter(searchParams.get(MODE_PARAM));
    setFilter((prev) => (prev === fromUrl ? prev : fromUrl));
  }, [searchParams, urlHasFilter]);

  return [filter, apply];
}
