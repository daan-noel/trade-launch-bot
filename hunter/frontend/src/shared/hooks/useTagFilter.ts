import { useCallback, useEffect, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';

import { STORAGE_KEYS, getString, remove, setString } from 'lib/storage';
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
 *
 * `pageId` scopes the mirror (Rules, Rules Control and Simulate each keep their
 * own filter); the `mt:` key is built here, so no call site owns a raw key.
 */
export function useTagFilter(
  pageId: string,
): [TagFilterState, (next: TagFilterState) => void] {
  const storageKey = `${STORAGE_KEYS.filterTags}.${pageId}`;
  const [searchParams, setSearchParams] = useSearchParams();
  const urlHasFilter =
    searchParams.has(TAG_PARAMS.include) || searchParams.has(TAG_PARAMS.exclude);

  const [filter, setFilter] = useState<TagFilterState>(() => {
    if (urlHasFilter) return parseTagFilter(searchParams);
    const raw = getString(storageKey);
    return raw ? parseTagFilter(new URLSearchParams(raw)) : EMPTY_TAG_FILTER;
  });

  // Seed the URL from a restored filter once, so the address bar always
  // describes what is on screen (and the board stays shareable).
  const seeded = useRef(false);

  const apply = useCallback(
    (next: TagFilterState) => {
      setFilter(next);
      const serialized = serializeTagFilter(next);
      // Storage is a convenience mirror; the URL is the real state.
      if (isEmptyTagFilter(next)) remove(storageKey);
      else setString(storageKey, new URLSearchParams(serialized).toString());
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
