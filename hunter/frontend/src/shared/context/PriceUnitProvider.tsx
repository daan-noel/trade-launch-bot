import {
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  type ReactNode,
} from 'react';
import type { PriceUnit, PriceUnitState } from 'types';
import { STORAGE_KEYS, getJSON, setJSON } from 'lib/storage';
import { setPriceUnitSnapshot } from 'lib/priceUnitSnapshot';
import { useGetSettingsQuery, useUpdateSettingsMutation } from 'store/apiSlice';
import { RateContext, UnitContext } from './PriceUnitContext';

// Component-only module: it exports nothing but `PriceUnitProvider`, so React
// Fast Refresh treats it as a valid boundary and hot-swaps it in place. The
// context objects + hooks live in `PriceUnitContext.tsx` on purpose — see the
// note there for why the two must stay in separate modules.

const LS_PRICE_UNIT_KEY = STORAGE_KEYS.priceUnit;

type PriceUnitAction =
  | { type: 'SET_UNIT'; unit: PriceUnit }
  | { type: 'SET_USD_RATE'; rate: number | null };

function loadPriceUnit(): PriceUnitState {
  const s = getJSON<PriceUnitState>(LS_PRICE_UNIT_KEY, { unit: 'SOL', usdRate: null });
  setPriceUnitSnapshot(s);
  return s;
}

function savePriceUnit(state: PriceUnitState) {
  setJSON(LS_PRICE_UNIT_KEY, state);
}

function reducer(state: PriceUnitState, action: PriceUnitAction): PriceUnitState {
  const next =
    action.type === 'SET_UNIT'
      ? { ...state, unit: action.unit }
      : { ...state, usdRate: action.rate };
  savePriceUnit(next);
  return next;
}

export function PriceUnitProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, undefined, loadPriceUnit);
  const { data: settings } = useGetSettingsQuery();
  const [updateSettings] = useUpdateSettingsMutation();
  const hydratedRef = useRef(false);

  // Keep the non-React snapshot in lockstep so column filter accessors and
  // `toTableRequest` convert against the same unit/rate the cells render.
  useEffect(() => {
    setPriceUnitSnapshot({ unit: state.unit, usdRate: state.usdRate });
  }, [state.unit, state.usdRate]);

  // Hydrate the unit once server-side settings arrive (localStorage is the
  // instant cache). If the server has no stored preference yet, upload the
  // local one so the existing per-browser choice isn't lost on first load.
  // `usdRate` is a fetched value, not a setting, so it stays local. The
  // settings fetch itself is owned (and deduped) by the shared RTK Query cache.
  useEffect(() => {
    if (!settings || hydratedRef.current) return;
    hydratedRef.current = true;
    if (settings.price_unit === 'SOL' || settings.price_unit === 'USD') {
      dispatch({ type: 'SET_UNIT', unit: settings.price_unit });
    } else {
      updateSettings({ price_unit: loadPriceUnit().unit });
    }
  }, [settings, updateSettings]);

  const setUnit = useCallback(
    (unit: PriceUnit) => {
      dispatch({ type: 'SET_UNIT', unit });
      updateSettings({ price_unit: unit });
    },
    [updateSettings],
  );

  const setUsdRate = useCallback((rate: number | null) => {
    dispatch({ type: 'SET_USD_RATE', rate });
  }, []);

  // Split providers so SOL-mode cells that only read `unit` do not re-render
  // on every SOL/USD poll tick.
  const unitValue = useMemo(
    () => ({ unit: state.unit, setUnit }),
    [state.unit, setUnit],
  );
  const rateValue = useMemo(
    () => ({ usdRate: state.usdRate, setUsdRate }),
    [state.usdRate, setUsdRate],
  );

  return (
    <UnitContext.Provider value={unitValue}>
      <RateContext.Provider value={rateValue}>{children}</RateContext.Provider>
    </UnitContext.Provider>
  );
}
