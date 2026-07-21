import {
  createContext,
  useCallback,
  useContext,
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

interface UnitContextValue {
  unit: PriceUnit;
  setUnit: (unit: PriceUnit) => void;
}

interface RateContextValue {
  usdRate: number | null;
  setUsdRate: (rate: number | null) => void;
}

const UnitContext = createContext<UnitContextValue | null>(null);
const RateContext = createContext<RateContextValue | null>(null);

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

/** Unit preference only — stable across SOL/USD rate ticks. */
export function usePriceUnitSetting() {
  const ctx = useContext(UnitContext);
  if (!ctx) throw new Error('usePriceUnitSetting must be used within PriceUnitProvider');
  return ctx;
}

/** Live SOL/USD rate only — subscribe when display actually depends on USD. */
export function useUsdRate() {
  const ctx = useContext(RateContext);
  if (!ctx) throw new Error('useUsdRate must be used within PriceUnitProvider');
  return ctx;
}

/** Combined unit + rate (toggle, charts that need both). Prefer the split hooks
 *  on hot paths so SOL mode skips rate-driven renders. */
export function usePriceUnit() {
  const unit = usePriceUnitSetting();
  const rate = useUsdRate();
  return { ...unit, ...rate };
}
