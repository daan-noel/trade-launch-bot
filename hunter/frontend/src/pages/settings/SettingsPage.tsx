import { useCallback, useEffect, useRef, useState } from 'react';
import type { AppSettings } from 'services/api';
import { useGetSettingsQuery, useUpdateSettingsMutation } from 'store/apiSlice';
import { LoadingState } from 'components/ui/LoadingState';
import { cn } from 'lib/cn';
import { TradingSection } from './TradingSection';
import { NotificationsSection } from './NotificationsSection';
import { TrackingSection } from './TrackingSection';
import { ReliabilitySection } from './ReliabilitySection';

const SAVED_FLASH_MS = 1600;

export function SettingsPage() {
  const { data: settings, isLoading: loading } = useGetSettingsQuery();
  const [updateSettings, { isLoading: saving }] = useUpdateSettingsMutation();
  const [error, setError] = useState<string | null>(null);
  const [savedFlash, setSavedFlash] = useState(false);
  const savedTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (savedTimer.current) clearTimeout(savedTimer.current);
    };
  }, []);

  const flashSaved = useCallback(() => {
    setSavedFlash(true);
    if (savedTimer.current) clearTimeout(savedTimer.current);
    savedTimer.current = setTimeout(() => setSavedFlash(false), SAVED_FLASH_MS);
  }, []);

  // Optimistic RTK patch — toggles flip instantly and roll back if the PUT fails.
  const update = useCallback(
    async (patch: Partial<AppSettings>) => {
      setError(null);
      try {
        await updateSettings(patch).unwrap();
        flashSaved();
      } catch (e) {
        setError(e instanceof Error ? e.message : 'Failed to save settings');
      }
    },
    [updateSettings, flashSaved],
  );

  const reportError = useCallback((msg: string) => {
    setError(msg);
  }, []);

  return (
    <div className="w-full">
      <div className="mb-3 flex items-baseline justify-between gap-3">
        <h2 className="text-base font-bold text-primary">Settings</h2>
        <span
          aria-live="polite"
          className={cn(
            'text-[11px] font-medium transition-opacity duration-300',
            savedFlash ? 'text-green opacity-100' : 'opacity-0',
          )}
        >
          Saved
        </span>
      </div>

      {error && (
        <div
          role="alert"
          className="mb-3 flex items-start justify-between gap-3 rounded-lg border border-red/30 bg-red/10 px-3 py-2 text-xs text-danger"
        >
          <span className="min-w-0 leading-relaxed">{error}</span>
          <button
            type="button"
            onClick={() => setError(null)}
            className="shrink-0 text-danger/80 underline-offset-2 hover:underline"
          >
            Dismiss
          </button>
        </div>
      )}

      {loading && !settings ? (
        <LoadingState label="Loading settings…" variant="panel" />
      ) : settings ? (
        <div className="grid grid-cols-1 items-start gap-4 lg:grid-cols-2">
          <TradingSection
            settings={settings}
            saving={saving}
            update={update}
            setError={reportError}
          />
          <NotificationsSection onSaved={flashSaved} />
          <TrackingSection settings={settings} saving={saving} update={update} />
          <ReliabilitySection
            settings={settings}
            saving={saving}
            update={update}
            setError={reportError}
          />
        </div>
      ) : null}
    </div>
  );
}
