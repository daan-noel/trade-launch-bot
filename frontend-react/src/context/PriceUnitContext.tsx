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
import { useGetSettingsQuery, useUpdateSettingsMutation } from 'store/apiSlice';

const LS_PRICE_UNIT_KEY = STORAGE_KEYS.priceUnit;

type PriceUnitAction =
  | { type: 'SET_UNIT'; unit: PriceUnit }
  | { type: 'SET_USD_RATE'; rate: number | null };

function loadPriceUnit(): PriceUnitState {
  return getJSON<PriceUnitState>(LS_PRICE_UNIT_KEY, { unit: 'SOL', usdRate: null });
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

interface PriceUnitContextValue extends PriceUnitState {
  setUnit: (unit: PriceUnit) => void;
  setUsdRate: (rate: number | null) => void;
}

const PriceUnitContext = createContext<PriceUnitContextValue | null>(null);

export function PriceUnitProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, undefined, loadPriceUnit);
  const { data: settings } = useGetSettingsQuery();
  const [updateSettings] = useUpdateSettingsMutation();
  const hydratedRef = useRef(false);

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

  const value = useMemo(
    () => ({ ...state, setUnit, setUsdRate }),
    [state, setUnit, setUsdRate],
  );

  return (
    <PriceUnitContext.Provider value={value}>{children}</PriceUnitContext.Provider>
  );
}

export function usePriceUnit() {
  const ctx = useContext(PriceUnitContext);
  if (!ctx) throw new Error('usePriceUnit must be used within PriceUnitProvider');
  return ctx;
}
