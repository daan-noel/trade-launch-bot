import { useCallback, useState } from 'react';
import { cn } from 'lib/cn';
import {
  useNotificationPrefs,
  ALL_NOTIFY_STATUSES,
  CRITICAL_NOTIFY_STATUSES,
  notifyPresetOf,
  type NotifyPreset,
  type NotifyStatus,
} from 'hooks/useNotificationPrefs';
import {
  SettingsPanel,
  SettingsPanelIntro,
  SettingsRows,
  ToggleRow,
} from './SettingsPrimitives';

const STATUS_PILL: Record<
  NotifyStatus,
  { active: string; inactive: string; label: string }
> = {
  Armed: {
    active: 'border-white/20 bg-white/8 text-text',
    inactive: 'border-white/6 bg-transparent text-text-dim',
    label: 'Armed',
  },
  Disarmed: {
    active: 'border-white/20 bg-white/8 text-text',
    inactive: 'border-white/6 bg-transparent text-text-dim',
    label: 'Disarmed',
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
  ExitUnconfirmed: {
    active: 'border-red/40 bg-red/10 text-red',
    inactive: 'border-white/6 bg-transparent text-text-dim',
    label: 'Exit unconfirmed',
  },
};

const PRESET_LABEL: Record<NotifyPreset, string> = {
  all: 'All',
  critical: 'Critical',
  custom: 'Custom',
};

interface NotificationsSectionProps {
  /** Flash the page-level "Saved" indicator after a local pref write. */
  onSaved?: () => void;
}

export function NotificationsSection({ onSaved }: NotificationsSectionProps) {
  const [prefs, setPrefs] = useNotificationPrefs();
  const notifSupported = typeof Notification !== 'undefined';
  const [permState, setPermState] = useState<NotificationPermission>(
    notifSupported ? Notification.permission : 'denied',
  );

  const writePrefs = useCallback(
    (updater: Parameters<typeof setPrefs>[0]) => {
      setPrefs(updater);
      onSaved?.();
    },
    [setPrefs, onSaved],
  );

  const handleDesktopToggle = useCallback(
    async (next: boolean) => {
      if (!next) {
        writePrefs((p) => ({ ...p, desktopEnabled: false }));
        return;
      }
      if (permState === 'denied') return;
      if (permState === 'granted') {
        writePrefs((p) => ({ ...p, desktopEnabled: true }));
        return;
      }
      const result = await Notification.requestPermission();
      setPermState(result);
      if (result === 'granted') writePrefs((p) => ({ ...p, desktopEnabled: true }));
    },
    [permState, writePrefs],
  );

  function toggleStatus(s: string) {
    writePrefs((prev) => ({
      ...prev,
      statuses: prev.statuses.includes(s)
        ? prev.statuses.filter((x) => x !== s)
        : [...prev.statuses, s],
    }));
  }

  function applyPreset(preset: NotifyPreset) {
    if (preset === 'custom') return;
    writePrefs((prev) => ({
      ...prev,
      statuses:
        preset === 'all' ? [...ALL_NOTIFY_STATUSES] : [...CRITICAL_NOTIFY_STATUSES],
    }));
  }

  const preset = notifyPresetOf(prefs.statuses);

  return (
    <SettingsPanel>
      <SettingsPanelIntro
        title="Position notifications"
        description="Toast alerts on arm/disarm and position status changes. Stored locally in this browser."
        tip={{
          body: 'Shown on any page, regardless of which strategy tab is open. Desktop alerts use the OS Action Center (Chrome/Edge) with Open Ops / Trade actions.',
        }}
      />
      <SettingsRows>
        <ToggleRow
          title="Real trading notifications"
          description="Notify when a real (on-chain) position changes status."
          checked={prefs.realEnabled}
          disabled={false}
          onChange={(realEnabled) => writePrefs((p) => ({ ...p, realEnabled }))}
        />
        <ToggleRow
          title="Paper trading notifications"
          description="Notify when a paper-test position changes status."
          checked={prefs.paperEnabled}
          disabled={false}
          onChange={(paperEnabled) => writePrefs((p) => ({ ...p, paperEnabled }))}
        />
        <ToggleRow
          title="Desktop notifications"
          description={
            permState === 'denied'
              ? 'Blocked by browser — allow notifications via the lock icon in the address bar.'
              : 'OS alerts with status icon + Open Ops / Trade actions.'
          }
          tip="Same position updates replace the prior toast. Holding + failures play a sound; failures stay until dismissed. Chrome/Edge on Windows Action Center."
          checked={prefs.desktopEnabled && permState === 'granted'}
          disabled={!notifSupported || permState === 'denied'}
          onChange={handleDesktopToggle}
        />

        <div className="py-3">
          <div className="mb-2 flex flex-wrap items-center gap-2">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
              Notify on status
            </span>
            <div className="flex flex-wrap gap-1">
              {(['critical', 'all', 'custom'] as const).map((key) => {
                const active = preset === key;
                const locked = key === 'custom';
                return (
                  <button
                    key={key}
                    type="button"
                    disabled={locked && !active}
                    onClick={() => applyPreset(key)}
                    className={cn(
                      'rounded-md border px-2 py-0.5 text-[11px] font-medium transition',
                      active
                        ? 'border-primary/40 bg-primary/12 text-primary'
                        : 'border-white/6 bg-transparent text-text-dim hover:border-white/15 hover:text-text',
                      locked && !active && 'cursor-default opacity-40 hover:border-white/6 hover:text-text-dim',
                    )}
                  >
                    {PRESET_LABEL[key]}
                  </button>
                );
              })}
            </div>
          </div>
          <div className="flex flex-wrap gap-1.5">
            {ALL_NOTIFY_STATUSES.map((s) => {
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
          <p className="mt-2 text-[11px] text-text-dim">
            Critical = holding, exit pending, and exit failures.
          </p>
        </div>
      </SettingsRows>
    </SettingsPanel>
  );
}
