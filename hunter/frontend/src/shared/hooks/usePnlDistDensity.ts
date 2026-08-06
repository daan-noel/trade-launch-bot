import { useCallback } from 'react';
import {
  PNL_DIST_DENSITIES,
  type PnlDistDensity,
} from 'components/analytics/pnlSeries';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';

// 'dense' — the distribution is read to find the tail, and the coarser bins
// hide exactly that. Both apps opted into it, so it is the shared default.
const DEFAULT_DENSITY: PnlDistDensity = 'dense';

function coerce(v: unknown): PnlDistDensity {
  return typeof v === 'string' && (PNL_DIST_DENSITIES as readonly string[]).includes(v)
    ? (v as PnlDistDensity)
    : DEFAULT_DENSITY;
}

/** Persist Sparse / Default / Dense across reloads (view preference, not cohort).
 *  Shared by every distribution chart, so a change on one deck reaches the rest
 *  of the tab immediately (`useLocalStorage` broadcasts). */
export function usePnlDistDensity(): [PnlDistDensity, (next: PnlDistDensity) => void] {
  const [stored, setStored] = useLocalStorage<PnlDistDensity>(
    STORAGE_KEYS.pnlDistDensity,
    DEFAULT_DENSITY,
  );
  const setDensity = useCallback(
    (next: PnlDistDensity) => setStored(coerce(next)),
    [setStored],
  );
  return [coerce(stored), setDensity];
}
