import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useReducer,
  type ReactNode,
} from 'react';
import type { PriceUnit, PriceUnitState } from '../types';

const LS_PRICE_UNIT_KEY = 'price_unit';

type PriceUnitAction =
  | { type: 'SET_UNIT'; unit: PriceUnit }
  | { type: 'SET_USD_RATE'; rate: number | null };

function loadPriceUnit(): PriceUnitState {
  try {
    const raw = localStorage.getItem(LS_PRICE_UNIT_KEY);
    if (raw) return JSON.parse(raw) as PriceUnitState;
  } catch {
    /* ignore */
  }
  return { unit: 'SOL', usdRate: null };
}

function savePriceUnit(state: PriceUnitState) {
  try {
    localStorage.setItem(LS_PRICE_UNIT_KEY, JSON.stringify(state));
  } catch {
    /* ignore */
  }
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

  const setUnit = useCallback((unit: PriceUnit) => {
    dispatch({ type: 'SET_UNIT', unit });
  }, []);

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
