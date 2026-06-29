import { useEffect, useState, useCallback } from 'react';
import type { AppSettings } from 'services/api';
import { useGetSettingsQuery, useUpdateSettingsMutation } from 'store/apiSlice';
import { Switch } from 'components/ui/Switch';
import { Checkbox } from 'components/ui/Checkbox';
import { Input } from 'components/ui/Input';
import { cn } from 'lib/cn';
import {
  useNotificationPrefs,
  ALL_POSITION_STATUSES,
  FP_PARAM_GROUPS,
  type PositionStatus,
} from 'hooks/useNotificationPrefs';

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

const STATUS_PILL: Record<
  PositionStatus,
  { active: string; inactive: string; label: string }
> = {
  Arming: {
    active: 'border-white/20 bg-white/8 text-text',
    inactive: 'border-white/6 bg-transparent text-text-dim',
    label: 'Arming',
  },
  BuySubmitted: {
    active: 'border-info/40 bg-info/12 text-info',
    inactive: 'border-white/6 bg-transparent text-text-dim',
    label: 'Buy submitted',
  },
  Holding: {
    active: 'border-green/40 bg-green/10 text-green',
    inactive: 'border-white/6 bg-transparent text-text-dim',
    label: 'Holding',
  },
  ExitPending: {
    active: 'border-warning/40 bg-warning/10 text-warning',
    inactive: 'border-white/6 bg-transparent text-text-dim',
    label: 'Exit pending',
  },
  End: {
    active: 'border-primary/40 bg-primary/12 text-primary',
    inactive: 'border-white/6 bg-transparent text-text-dim',
    label: 'End',
  },
  ExitFailed: {
    active: 'border-red/40 bg-red/10 text-red',
    inactive: 'border-white/6 bg-transparent text-text-dim',
    label: 'Exit failed',
  },
};

function NotificationSection() {
  const [prefs, setPrefs] = useNotificationPrefs();
  const notifSupported = typeof Notification !== 'undefined';
  const [permState, setPermState] = useState<NotificationPermission>(
    notifSupported ? Notification.permission : 'denied',
  );

  const handleDesktopToggle = useCallback(
    async (next: boolean) => {
      if (!next) {
        setPrefs((p) => ({ ...p, desktopEnabled: false }));
        return;
      }
      if (permState === 'denied') return;
      if (permState === 'granted') {
        setPrefs((p) => ({ ...p, desktopEnabled: true }));
        return;
      }
      const result = await Notification.requestPermission();
      setPermState(result);
      if (result === 'granted') setPrefs((p) => ({ ...p, desktopEnabled: true }));
    },
    [permState, setPrefs],
  );

  function toggleStatus(s: string) {
    setPrefs((prev) => ({
      ...prev,
      statuses: prev.statuses.includes(s)
        ? prev.statuses.filter((x) => x !== s)
        : [...prev.statuses, s],
    }));
  }

  function toggleFp(key: string) {
    setPrefs((prev) => ({
      ...prev,
      fpParams: prev.fpParams.includes(key)
        ? prev.fpParams.filter((x) => x !== key)
        : [...prev.fpParams, key],
    }));
  }

  function setAllStatuses(all: boolean) {
    setPrefs((prev) => ({
      ...prev,
      statuses: all ? [...ALL_POSITION_STATUSES] : [],
    }));
  }

  const allSelected = ALL_POSITION_STATUSES.every((s) => prefs.statuses.includes(s));

  return (
    <section className="mt-4 max-w-2xl rounded-xl border border-white/8 bg-bg-panel p-4">
      <h3 className="text-sm font-semibold text-text">Position notifications</h3>
      <p className="mt-0.5 mb-3.5 text-xs text-text-dim">
        Toast alerts on position status changes. Shown on any page, regardless of which
        strategy tab is open. All preferences are stored locally.
      </p>

      <div className="flex flex-col gap-2.5">
        <ToggleRow
          title="Real trading notifications"
          description="Notify when a real (on-chain) position changes status."
          checked={prefs.realEnabled}
          disabled={false}
          onChange={(realEnabled) => setPrefs((p) => ({ ...p, realEnabled }))}
        />
        <ToggleRow
          title="Paper trading notifications"
          description="Notify when a paper-test position changes status."
          checked={prefs.paperEnabled}
          disabled={false}
          onChange={(paperEnabled) => setPrefs((p) => ({ ...p, paperEnabled }))}
        />
        <ToggleRow
          title="Desktop notifications (Windows Action Center)"
          description={
            permState === 'denied'
              ? 'Blocked by browser — click the lock icon in the address bar to allow notifications for this site.'
              : 'Send OS-level notifications so you get alerted even when this tab is in the background.'
          }
          checked={prefs.desktopEnabled && permState === 'granted'}
          disabled={!notifSupported || permState === 'denied'}
          onChange={handleDesktopToggle}
        />
      </div>

      <div className="mt-4">
        <div className="mb-1.5 flex items-center gap-3">
          <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
            Notify on status
          </span>
          <button
            type="button"
            onClick={() => setAllStatuses(!allSelected)}
            className="text-[11px] text-text-dim underline underline-offset-2 hover:text-text"
          >
            {allSelected ? 'Deselect all' : 'Select all'}
          </button>
        </div>
        <div className="flex flex-wrap gap-1.5">
          {ALL_POSITION_STATUSES.map((s) => {
            const on = prefs.statuses.includes(s);
            const cls = STATUS_PILL[s];
            return (
              <button
                key={s}
                type="button"
                onClick={() => toggleStatus(s)}
                className={cn(
                  'rounded-md border px-2.5 py-1 text-[11px] font-medium transition',
                  on ? cls.active : cls.inactive,
                )}
              >
                {cls.label}
              </button>
            );
          })}
        </div>
      </div>

      <div className="mt-4">
        <span className="mb-1.5 block text-[10px] font-bold uppercase tracking-wider text-text-dim">
          Show in notification
        </span>
        <div className="grid grid-cols-2 gap-x-6 gap-y-1.5">
          {FP_PARAM_GROUPS.map((g) => (
            <label key={g.key} className="flex cursor-pointer items-center gap-2">
              <Checkbox
                boxSize="sm"
                checked={prefs.fpParams.includes(g.key)}
                onChange={() => toggleFp(g.key)}
              />
              <span className="text-xs text-text">
                {g.label}
                <span className="ml-1.5 font-mono text-[10px] text-text-dim">{g.hint}</span>
              </span>
            </label>
          ))}
        </div>
      </div>
    </section>
  );
}

export function SettingsPage() {
  const { data: settings, isLoading: loading } = useGetSettingsQuery();
  const [updateSettings, { isLoading: saving }] = useUpdateSettingsMutation();
  const [error, setError] = useState<string | null>(null);
  // Buy/sell slippage edited as percent strings, committed on blur.
  const [buySlipText, setBuySlipText] = useState('');
  const [sellSlipText, setSellSlipText] = useState('');
  // Watchdog timing edited as second strings, committed on blur. The server
  // clamps to its floors and returns the applied value, which re-syncs these.
  const [stallText, setStallText] = useState('');
  const [intervalText, setIntervalText] = useState('');
  const [maxSolText, setMaxSolText] = useState('');
  const [gapWindowText, setGapWindowText] = useState('');

  useEffect(() => {
    setBuySlipText(
      settings?.buy_slippage_bps != null ? String(settings.buy_slippage_bps / 100) : '',
    );
  }, [settings?.buy_slippage_bps]);

  useEffect(() => {
    setSellSlipText(
      settings?.sell_slippage_bps != null ? String(settings.sell_slippage_bps / 100) : '',
    );
  }, [settings?.sell_slippage_bps]);

  useEffect(() => {
    if (settings) setStallText(String(settings.watchdog_stall_timeout_secs));
  }, [settings?.watchdog_stall_timeout_secs]);

  useEffect(() => {
    if (settings) setIntervalText(String(settings.watchdog_check_interval_secs));
  }, [settings?.watchdog_check_interval_secs]);

  useEffect(() => {
    setMaxSolText(settings?.max_committed_sol != null ? String(settings.max_committed_sol) : '');
  }, [settings?.max_committed_sol]);

  useEffect(() => {
    if (settings) setGapWindowText(String(settings.gap_replay_max_window_secs));
  }, [settings?.gap_replay_max_window_secs]);

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

  function commitBuySlippage() {
    if (!settings) return;
    const raw = buySlipText.trim();
    if (raw === '') {
      if (settings.buy_slippage_bps !== null) update({ buy_slippage_bps: null });
      return;
    }
    const pct = parseFloat(raw);
    if (!Number.isFinite(pct) || pct < 0 || pct > 50) {
      setError('Buy slippage must be between 0 and 50% (0 = no limit)');
      return;
    }
    const bps = Math.round(pct * 100);
    if (bps !== settings.buy_slippage_bps) update({ buy_slippage_bps: bps });
  }

  function commitSellSlippage() {
    if (!settings) return;
    const raw = sellSlipText.trim();
    if (raw === '') {
      if (settings.sell_slippage_bps !== null) update({ sell_slippage_bps: null });
      return;
    }
    const pct = parseFloat(raw);
    if (!Number.isFinite(pct) || pct < 0 || pct > 50) {
      setError('Sell slippage must be between 0 and 50% (0 or blank = no limit)');
      return;
    }
    const bps = Math.round(pct * 100);
    if (bps !== settings.sell_slippage_bps) update({ sell_slippage_bps: bps });
  }

  function commitGapWindow() {
    if (!settings) return;
    const raw = gapWindowText.trim();
    if (raw === '') return;
    const secs = parseInt(raw, 10);
    if (!Number.isInteger(secs) || secs < 60) {
      setError('Gap-replay window must be at least 60 seconds');
      return;
    }
    if (secs !== settings.gap_replay_max_window_secs)
      update({ gap_replay_max_window_secs: secs });
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
          Separate slippage tolerances for buys and sells. Applies to both manual trades
          and the bot. Buy default is 5% when blank. Sell default is no limit — bot exits
          always clear even during a rapid dump.
        </p>

        {loading ? (
          <p className="text-xs text-text-dim">Loading…</p>
        ) : settings ? (
          <div className="flex flex-wrap gap-4">
            <label className="flex max-w-[220px] flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                Buy slippage %
              </span>
              <Input
                type="number"
                fieldSize="md"
                min={0}
                max={50}
                step={0.1}
                placeholder="5"
                value={buySlipText}
                disabled={saving}
                onChange={(e) => setBuySlipText(e.target.value)}
                onBlur={commitBuySlippage}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
                }}
              />
              <p className="text-[11px] text-text-dim">Blank = server default (5%). 0 = no limit.</p>
            </label>
            <label className="flex max-w-[220px] flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                Sell slippage %
              </span>
              <Input
                type="number"
                fieldSize="md"
                min={0}
                max={50}
                step={0.1}
                placeholder="no limit"
                value={sellSlipText}
                disabled={saving}
                onChange={(e) => setSellSlipText(e.target.value)}
                onBlur={commitSellSlippage}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
                }}
              />
              <p className="text-[11px] text-text-dim">Blank or 0 = no limit (always fills).</p>
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

      <section className="max-w-2xl rounded-xl border border-white/8 bg-bg-panel p-4">
        <h3 className="text-sm font-semibold text-text">Gap-replay on reconnect</h3>
        <p className="mt-0.5 mb-3.5 text-xs text-text-dim">
          When enabled, LaserStream replays missed transactions after a reconnect by sending
          the last seen slot as <code>from_slot</code>. Default off — replayed creates carry
          stale block_time and are filtered by the 30 s freshness gate anyway.
        </p>

        {loading ? (
          <p className="text-xs text-text-dim">Loading…</p>
        ) : settings ? (
          <div className="flex flex-col gap-2.5">
            <ToggleRow
              title="Enable gap-replay"
              description="When on, reconnects replay missed transactions from the last seen slot. Filtered by freshness gate (30 s). Keep off unless you need gap coverage."
              checked={settings.gap_replay_on_reconnect}
              disabled={saving}
              onChange={(gap_replay_on_reconnect) => update({ gap_replay_on_reconnect })}
            />
            <label className="flex max-w-[220px] flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                Max window (s)
              </span>
              <Input
                type="number"
                fieldSize="md"
                min={60}
                step={30}
                value={gapWindowText}
                disabled={saving || !settings.gap_replay_on_reconnect}
                onChange={(e) => setGapWindowText(e.target.value)}
                onBlur={commitGapWindow}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
                }}
              />
            </label>
            <p className="text-[11px] text-text-dim">
              Gaps larger than this fall back to a live re-subscribe (no replay). Minimum 60 s.
            </p>
          </div>
        ) : null}
      </section>

      <NotificationSection />
    </div>
  );
}
