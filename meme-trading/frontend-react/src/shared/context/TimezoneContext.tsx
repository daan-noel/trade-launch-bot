import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import {
  getDefaultChartTimezone,
  isValidTimezone,
} from 'components/token-price-chart/chartTimezone';
import { LS_CHART_PREFS_KEY } from 'components/token-price-chart/constants';
import { STORAGE_KEYS, getString, setString } from 'lib/storage';
import { useGetSettingsQuery, useUpdateSettingsMutation } from 'store/apiSlice';

export const LS_TIMEZONE_KEY = STORAGE_KEYS.timezone;

function loadTimezone(): string {
  const saved = getString(LS_TIMEZONE_KEY);
  if (saved && isValidTimezone(saved)) return saved;
  try {
    const raw = getString(LS_CHART_PREFS_KEY);
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
  setString(LS_TIMEZONE_KEY, timezone);
}

interface TimezoneContextValue {
  timezone: string;
  setTimezone: (timezone: string) => void;
}

const TimezoneContext = createContext<TimezoneContextValue | null>(null);

export function TimezoneProvider({ children }: { children: ReactNode }) {
  const [timezone, setTimezoneState] = useState(() => loadTimezone());
  const { data: settings } = useGetSettingsQuery();
  const [updateSettings] = useUpdateSettingsMutation();
  const hydratedRef = useRef(false);

  useEffect(() => {
    saveTimezone(timezone);
  }, [timezone]);

  // Hydrate once server-side settings arrive (localStorage above is the instant
  // cache). If the server has no stored timezone yet, upload the local one so
  // the existing per-browser choice isn't lost on first load. The settings
  // fetch is owned (and deduped) by the shared RTK Query cache.
  useEffect(() => {
    if (!settings || hydratedRef.current) return;
    hydratedRef.current = true;
    if (settings.timezone && isValidTimezone(settings.timezone)) {
      setTimezoneState(settings.timezone);
    } else {
      updateSettings({ timezone: loadTimezone() });
    }
  }, [settings, updateSettings]);

  const setTimezone = useCallback(
    (next: string) => {
      if (!isValidTimezone(next)) return;
      setTimezoneState(next);
      updateSettings({ timezone: next });
    },
    [updateSettings],
  );

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
