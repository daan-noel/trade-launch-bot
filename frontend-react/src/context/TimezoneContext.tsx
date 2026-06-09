import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import {
  getDefaultChartTimezone,
  isValidTimezone,
} from 'components/token-price-chart/chartTimezone';
import { LS_CHART_PREFS_KEY } from 'components/token-price-chart/constants';
import { fetchSettings, updateSettings } from 'services/api';

export const LS_TIMEZONE_KEY = 'app_timezone';

function loadTimezone(): string {
  try {
    const saved = localStorage.getItem(LS_TIMEZONE_KEY);
    if (saved && isValidTimezone(saved)) return saved;
  } catch {
    /* ignore */
  }
  try {
    const raw = localStorage.getItem(LS_CHART_PREFS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as { chartTimezone?: string };
      const tz = parsed.chartTimezone;
      if (typeof tz === 'string' && isValidTimezone(tz)) return tz;
    }
  } catch {
    /* ignore */
  }
  return getDefaultChartTimezone();
}

function saveTimezone(timezone: string) {
  try {
    localStorage.setItem(LS_TIMEZONE_KEY, timezone);
  } catch {
    /* ignore */
  }
}

interface TimezoneContextValue {
  timezone: string;
  setTimezone: (timezone: string) => void;
}

const TimezoneContext = createContext<TimezoneContextValue | null>(null);

export function TimezoneProvider({ children }: { children: ReactNode }) {
  const [timezone, setTimezoneState] = useState(() => loadTimezone());

  useEffect(() => {
    saveTimezone(timezone);
  }, [timezone]);

  // Hydrate from server-side settings (localStorage above is the instant cache).
  // If the server has no stored timezone yet, upload the local one so the
  // existing per-browser choice isn't lost on first load.
  useEffect(() => {
    let cancelled = false;
    fetchSettings()
      .then((s) => {
        if (cancelled) return;
        if (s.timezone && isValidTimezone(s.timezone)) {
          setTimezoneState(s.timezone);
        } else {
          updateSettings({ timezone: loadTimezone() }).catch(() => {});
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  const setTimezone = useCallback((next: string) => {
    if (!isValidTimezone(next)) return;
    setTimezoneState(next);
    updateSettings({ timezone: next }).catch(() => {});
  }, []);

  const value = useMemo(() => ({ timezone, setTimezone }), [timezone, setTimezone]);

  return (
    <TimezoneContext.Provider value={value}>{children}</TimezoneContext.Provider>
  );
}

export function useTimezone() {
  const ctx = useContext(TimezoneContext);
  if (!ctx) throw new Error('useTimezone must be used within TimezoneProvider');
  return ctx;
}
