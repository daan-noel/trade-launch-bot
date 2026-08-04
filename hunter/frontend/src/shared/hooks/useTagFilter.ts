import { useCallback, useEffect, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';

import {
  EMPTY_TAG_FILTER,
  isEmptyTagFilter,
  parseTagFilter,
  serializeTagFilter,
  TAG_PARAMS,
  type TagFilterState,
} from 'lib/strategy/tags';

/**
 * Rule-tag filter state, synced to the URL (`?tags=` / `?notags=`) and mirrored
 * to `localStorage` — the same bidirectional shape as `useSelectionSearchParam`,
 * with which it composes (a filtered board keeps its `?rule=` selection).
 *
 * Precedence: **the URL wins when it carries either param**, so a pasted link
 * shows exactly what the sender saw. Otherwise the last-used filter is restored
 * from storage on mount, which is what makes a habitual "hide `stage:experiment`"
 * stick across sessions instead of needing a re-toggle every visit.
 */
export function useTagFilter(
  storageKey: string,
): [TagFilterState, (next: TagFilterState) => void] {
  const [searchParams, setSearchParams] = useSearchParams();
  const urlHasFilter =
    searchParams.has(TAG_PARAMS.include) || searchParams.has(TAG_PARAMS.exclude);

  const [filter, setFilter] = useState<TagFilterState>(() => {
    if (urlHasFilter) return parseTagFilter(searchParams);
    try {
      const raw = localStorage.getItem(storageKey);
      if (raw) return parseTagFilter(new URLSearchParams(raw));
    } catch {
      // Private mode / quota — a filter is not worth failing the page over.
    }
    return EMPTY_TAG_FILTER;
  });

  // Seed the URL from a restored filter once, so the address bar always
  // describes what is on screen (and the board stays shareable).
  const seeded = useRef(false);

  const apply = useCallback(
    (next: TagFilterState) => {
      setFilter(next);
      const serialized = serializeTagFilter(next);
      try {
        if (isEmptyTagFilter(next)) localStorage.removeItem(storageKey);
        else localStorage.setItem(storageKey, new URLSearchParams(serialized).toString());
      } catch {
        // Ignore — storage is a convenience, the URL is the real state.
      }
      setSearchParams(
        (prev) => {
          const params = new URLSearchParams(prev);
          for (const key of Object.values(TAG_PARAMS)) {
            const v = serialized[key];
            if (v) params.set(key, v);
            else params.delete(key);
          }
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
    if (!urlHasFilter && !isEmptyTagFilter(filter)) apply(filter);
  }, [urlHasFilter, filter, apply]);

  // Browser back/forward rewrites the params — follow them.
  useEffect(() => {
    if (!urlHasFilter) return;
    const fromUrl = parseTagFilter(searchParams);
    setFilter((prev) =>
      prev.include.join(',') === fromUrl.include.join(',') &&
      prev.exclude.join(',') === fromUrl.exclude.join(',')
        ? prev
        : fromUrl,
    );
  }, [searchParams, urlHasFilter]);

  return [filter, apply];
}
