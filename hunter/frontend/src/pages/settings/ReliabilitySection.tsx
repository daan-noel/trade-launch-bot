import { useEffect, useState } from 'react';
import type { AppSettings } from 'services/api';
import {
  NumberField,
  SettingsPanel,
  SettingsPanelIntro,
  SettingsRows,
  ToggleRow,
} from './SettingsPrimitives';

interface ReliabilitySectionProps {
  settings: AppSettings;
  saving: boolean;
  update: (patch: Partial<AppSettings>) => void;
  setError: (msg: string) => void;
}

export function ReliabilitySection({
  settings,
  saving,
  update,
  setError,
}: ReliabilitySectionProps) {
  const [stallText, setStallText] = useState('');
  const [intervalText, setIntervalText] = useState('');
  const [gapWindowText, setGapWindowText] = useState('');

  useEffect(() => {
    setStallText(String(settings.watchdog_stall_timeout_secs));
  }, [settings.watchdog_stall_timeout_secs]);

  useEffect(() => {
    setIntervalText(String(settings.watchdog_check_interval_secs));
  }, [settings.watchdog_check_interval_secs]);

  useEffect(() => {
    setGapWindowText(String(settings.gap_replay_max_window_secs));
  }, [settings.gap_replay_max_window_secs]);

  function commitWatchdogSecs(
    text: string,
    current: number,
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

  function commitGapWindow() {
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

  return (
    <SettingsPanel>
      <SettingsPanelIntro
        title="Reliability"
        description="Ingest watchdog and gap-replay after reconnect. Leave defaults unless you are debugging."
        tip={{
          title: 'Watchdog & gap-replay',
          body: 'Watchdog restarts the process when live ingest stalls (live mode only). Gap-replay backfills missed LaserStream txs for data coverage — it does not change what the bot buys (30 s freshness gate still applies).',
        }}
      />
      <SettingsRows>
        <ToggleRow
          title="Enable watchdog"
          description="Auto-restart when ingest makes no forward progress for the stall window."
          tip="Turn off only while debugging. Server floors: timeout >= 52s, interval >= 5s (clamped on save)."
          checked={settings.watchdog_enabled}
          disabled={saving}
          onChange={(watchdog_enabled) => update({ watchdog_enabled })}
        >
          <NumberField
            label="Stall timeout (s)"
            min={52}
            step={1}
            value={stallText}
            disabled={saving || !settings.watchdog_enabled}
            onChange={setStallText}
            onCommit={() =>
              commitWatchdogSecs(
                stallText,
                settings.watchdog_stall_timeout_secs,
                'watchdog_stall_timeout_secs',
              )
            }
          />
          <NumberField
            label="Check interval (s)"
            min={5}
            step={1}
            value={intervalText}
            disabled={saving || !settings.watchdog_enabled}
            onChange={setIntervalText}
            onCommit={() =>
              commitWatchdogSecs(
                intervalText,
                settings.watchdog_check_interval_secs,
                'watchdog_check_interval_secs',
              )
            }
          />
        </ToggleRow>
        <ToggleRow
          title="Enable gap-replay"
          description="On reconnect, replay from the last seen slot to fill the data gap."
          tip="Default off. Affects data coverage only — the 30 s freshness gate still decides what the sniper buys. Gaps beyond the max window re-subscribe live."
          checked={settings.gap_replay_on_reconnect}
          disabled={saving}
          onChange={(gap_replay_on_reconnect) => update({ gap_replay_on_reconnect })}
        >
          <NumberField
            label="Max window (s)"
            hint="Minimum 60 s. Larger gaps fall back to a live re-subscribe."
            min={60}
            step={30}
            value={gapWindowText}
            disabled={saving || !settings.gap_replay_on_reconnect}
            onChange={setGapWindowText}
            onCommit={commitGapWindow}
          />
        </ToggleRow>
      </SettingsRows>
    </SettingsPanel>
  );
}
