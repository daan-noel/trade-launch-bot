import { useEffect, useState } from 'react';
import {
  fetchTrackingSettings,
  setTrackingSettings,
  type TrackingSettings,
} from 'services/api';
import { Switch } from 'components/ui/Switch';

interface ToggleRowProps {
  title: string;
  description: string;
  checked: boolean;
  disabled: boolean;
  onChange: (next: boolean) => void;
}

function ToggleRow({ title, description, checked, disabled, onChange }: ToggleRowProps) {
  return (
    <div className="flex items-start justify-between gap-4 rounded-lg border border-white/6 bg-white/2 px-3.5 py-3">
      <div className="min-w-0">
        <div className="text-sm font-medium text-text">{title}</div>
        <p className="mt-0.5 text-xs leading-relaxed text-text-dim">{description}</p>
      </div>
      <div className="pt-0.5">
        <Switch checked={checked} disabled={disabled} onChange={onChange} label={title} />
      </div>
    </div>
  );
}

export function SettingsPage() {
  const [settings, setSettings] = useState<TrackingSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    fetchTrackingSettings()
      .then((s) => {
        if (!cancelled) setSettings(s);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : 'Failed to load settings');
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function update(patch: Partial<TrackingSettings>) {
    if (!settings) return;
    const previous = settings;
    // Optimistic: reflect the change immediately, roll back if the PUT fails.
    setSettings({ ...settings, ...patch });
    setSaving(true);
    setError(null);
    try {
      const next = await setTrackingSettings(patch);
      setSettings(next);
    } catch (e) {
      setSettings(previous);
      setError(e instanceof Error ? e.message : 'Failed to save settings');
    } finally {
      setSaving(false);
    }
  }

  return (
    <div>
      <h2 className="mb-3 text-base font-bold text-primary">Settings</h2>

      <section className="max-w-2xl rounded-xl border border-white/8 bg-bg-panel p-4">
        <h3 className="text-sm font-semibold text-text">Token tracking</h3>
        <p className="mt-0.5 mb-3.5 text-xs text-text-dim">
          Controls what the live ingest pipeline records. Changes apply immediately and
          persist across restarts.
        </p>

        {loading ? (
          <p className="text-xs text-text-dim">Loading…</p>
        ) : settings ? (
          <div className="flex flex-col gap-2.5">
            <ToggleRow
              title="Track Mayhem-mode tokens"
              description="When off, new Mayhem-mode tokens are not ingested, and any already-tracked Mayhem tokens are dropped from live tracking. Historical data is kept."
              checked={settings.track_mayhem}
              disabled={saving}
              onChange={(track_mayhem) => update({ track_mayhem })}
            />
            <ToggleRow
              title="Track trade history after migration"
              description="When off, migrated tokens' AMM trades stop being recorded and their pools are unsubscribed. The migration itself is still tracked."
              checked={settings.track_post_migration}
              disabled={saving}
              onChange={(track_post_migration) => update({ track_post_migration })}
            />
          </div>
        ) : null}

        {error && <p className="mt-3 text-xs text-danger">{error}</p>}
      </section>
    </div>
  );
}
