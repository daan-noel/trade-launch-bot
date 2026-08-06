import { useCallback, useState } from 'react';
import {
  PNL_DIST_DENSITIES,
  type PnlDistDensity,
} from 'components/analytics/pnlSeries';

const STORAGE_KEY = 'hunter.pnlDistDensity';

function readStored(): PnlDistDensity {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v && (PNL_DIST_DENSITIES as readonly string[]).includes(v)) {
      return v as PnlDistDensity;
    }
  } catch {
    /* private mode / blocked storage */
  }
  // 'dense' — the distribution is read to find the tail, and the coarser bins
  // hide exactly that. Both apps opted into it, so it is the shared default.
  return 'dense';
}

/** Persist Sparse / Default / Dense across reloads (view preference, not cohort). */
export function usePnlDistDensity(): [PnlDistDensity, (next: PnlDistDensity) => void] {
  const [density, setDensityState] = useState<PnlDistDensity>(readStored);
  const setDensity = useCallback((next: PnlDistDensity) => {
    setDensityState(next);
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      /* ignore */
    }
  }, []);
  return [density, setDensity];
}
