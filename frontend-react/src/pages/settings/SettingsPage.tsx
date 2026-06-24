import { useEffect, useState } from 'react';
import type { AppSettings } from 'services/api';
import { useGetSettingsQuery, useUpdateSettingsMutation } from 'store/apiSlice';
import { Switch } from 'components/ui/Switch';
import { Input } from 'components/ui/Input';

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
  const { data: settings, isLoading: loading } = useGetSettingsQuery();
  const [updateSettings, { isLoading: saving }] = useUpdateSettingsMutation();
  const [error, setError] = useState<string | null>(null);
  // Slippage edited as a percent string, committed on blur. Synced from the
  // persisted bps value whenever it changes.
  const [slipText, setSlipText] = useState('');
  // Watchdog timing edited as second strings, committed on blur. The server
  // clamps to its floors and returns the applied value, which re-syncs these.
  const [stallText, setStallText] = useState('');
  const [intervalText, setIntervalText] = useState('');
  const [maxSolText, setMaxSolText] = useState('');

  useEffect(() => {
    setSlipText(
      settings && settings.slippage_bps != null ? String(settings.slippage_bps / 100) : '',
    );
  }, [settings?.slippage_bps]);

  useEffect(() => {
    if (settings) setStallText(String(settings.watchdog_stall_timeout_secs));
  }, [settings?.watchdog_stall_timeout_secs]);

  useEffect(() => {
    if (settings) setIntervalText(String(settings.watchdog_check_interval_secs));
  }, [settings?.watchdog_check_interval_secs]);

  useEffect(() => {
    setMaxSolText(settings?.max_committed_sol != null ? String(settings.max_committed_sol) : '');
  }, [settings?.max_committed_sol]);

  function commitWatchdogSecs(
    text: string,
    current: number | undefined,
    field: 'watchdog_stall_timeout_secs' | 'watchdog_check_interval_secs',
  ) {
    const raw = text.trim();
    if (raw === '') return;
    const secs = parseInt(raw, 10);
    if (!Number.isInteger(secs) || secs <= 0) {
      setError('Watchdog times must be a positive number of seconds');
      return;
    }
    if (secs !== current) update({ [field]: secs });
  }

  function commitMaxCommittedSol() {
    if (!settings) return;
    const raw = maxSolText.trim();
    if (raw === '') {
      // Blank = clear the ceiling (set to null).
      if (settings.max_committed_sol != null) update({ max_committed_sol: null });
      return;
    }
    const sol = parseFloat(raw);
    if (!Number.isFinite(sol) || sol <= 0) {
      setError('Max committed SOL must be a positive number');
      return;
    }
    if (sol !== settings.max_committed_sol) update({ max_committed_sol: sol });
  }

  function commitSlippage() {
    if (!settings) return;
    const raw = slipText.trim();
    // Blank = leave the persisted default untouched.
    if (raw === '') return;
    const pct = parseFloat(raw);
    if (!Number.isFinite(pct) || pct < 0 || pct > 50) {
      setError('Slippage must be between 0 and 50%');
      return;
    }
    const bps = Math.round(pct * 100);
    if (bps !== settings.slippage_bps) {
      update({ slippage_bps: bps });
    }
  }

  // The shared cache is patched optimistically inside the mutation, so toggles
  // flip instantly and roll back centrally if the PUT fails.
  async function update(patch: Partial<AppSettings>) {
    setError(null);
    try {
      await updateSettings(patch).unwrap();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to save settings');
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
            <ToggleRow
              title="Persist raw transactions"
              description="When off, raw transaction blobs are no longer written to the database (curbs DB growth). Decoded trades and metrics are still recorded."
              checked={settings.persist_raw}
              disabled={saving}
              onChange={(persist_raw) => update({ persist_raw })}
            />
          </div>
        ) : null}

        {error && <p className="mt-3 text-xs text-danger">{error}</p>}
      </section>

      <section className="mt-4 max-w-2xl rounded-xl border border-white/8 bg-bg-panel p-4">
        <h3 className="text-sm font-semibold text-text">Trading</h3>
        <p className="mt-0.5 mb-3.5 text-xs text-text-dim">
          Default slippage tolerance applied to manual buys and sells that don't specify
          their own. Applies to both bonding-curve and AMM trades. Leave blank to use the
          server default (5%).
        </p>

        {loading ? (
          <p className="text-xs text-text-dim">Loading…</p>
        ) : settings ? (
          <div className="flex flex-wrap gap-4">
            <label className="flex max-w-[220px] flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                Default slippage %
              </span>
              <Input
                type="number"
                fieldSize="md"
                min={0}
                max={50}
                step={0.1}
                placeholder="5"
                value={slipText}
                disabled={saving}
                onChange={(e) => setSlipText(e.target.value)}
                onBlur={commitSlippage}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
                }}
              />
            </label>
            <label className="flex max-w-[220px] flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                Max committed SOL
              </span>
              <Input
                type="number"
                fieldSize="md"
                min={0}
                step={0.01}
                placeholder="no limit"
                value={maxSolText}
                disabled={saving}
                onChange={(e) => setMaxSolText(e.target.value)}
                onBlur={commitMaxCommittedSol}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
                }}
              />
              <p className="text-[11px] text-text-dim">
                Hard ceiling on open real positions (SOL). Blank = no limit.
              </p>
            </label>
          </div>
        ) : null}
      </section>

      <section className="mt-4 max-w-2xl rounded-xl border border-white/8 bg-bg-panel p-4">
        <h3 className="text-sm font-semibold text-text">Ingest watchdog</h3>
        <p className="mt-0.5 mb-3.5 text-xs text-text-dim">
          Restarts the process when live ingest makes no forward progress for the stall
          window — recovers a silently wedged stream the in-stream checks can't see. Only
          fires while live mode is on.
        </p>

        {loading ? (
          <p className="text-xs text-text-dim">Loading…</p>
        ) : settings ? (
          <div className="flex flex-col gap-2.5">
            <ToggleRow
              title="Enable watchdog"
              description="When off, a stalled ingest is never auto-restarted. Leave on in production; turn off only when debugging so a breakpoint-paused process isn't killed."
              checked={settings.watchdog_enabled}
              disabled={saving}
              onChange={(watchdog_enabled) => update({ watchdog_enabled })}
            />
            <div className="flex flex-wrap gap-4">
              <label className="flex max-w-[220px] flex-col gap-1.5">
                <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                  Stall timeout (s)
                </span>
                <Input
                  type="number"
                  fieldSize="md"
                  min={52}
                  step={1}
                  value={stallText}
                  disabled={saving || !settings.watchdog_enabled}
                  onChange={(e) => setStallText(e.target.value)}
                  onBlur={() =>
                    commitWatchdogSecs(
                      stallText,
                      settings.watchdog_stall_timeout_secs,
                      'watchdog_stall_timeout_secs',
                    )
                  }
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
                  }}
                />
              </label>
              <label className="flex max-w-[220px] flex-col gap-1.5">
                <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                  Check interval (s)
                </span>
                <Input
                  type="number"
                  fieldSize="md"
                  min={5}
                  step={1}
                  value={intervalText}
                  disabled={saving || !settings.watchdog_enabled}
                  onChange={(e) => setIntervalText(e.target.value)}
                  onBlur={() =>
                    commitWatchdogSecs(
                      intervalText,
                      settings.watchdog_check_interval_secs,
                      'watchdog_check_interval_secs',
                    )
                  }
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
                  }}
                />
              </label>
            </div>
            <p className="text-[11px] text-text-dim">
              Server floors: timeout ≥ 52s, interval ≥ 5s. Out-of-range values are
              clamped on save.
            </p>
          </div>
        ) : null}
      </section>
    </div>
  );
}
