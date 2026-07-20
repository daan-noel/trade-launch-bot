import { useEffect } from 'react';
import { useLocalStorage } from './useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';

/** Statuses the Settings "Notify on status" pills cover — position lifecycle
 *  plus arm/disarm (from `strategy_armed_changed`, not a position status). */
export const ALL_NOTIFY_STATUSES = [
  'Armed',
  'Disarmed',
  'BuySubmitted',
  'Holding',
  'ExitPending',
  'End',
  'ExitFailed',
  'ExitUnconfirmed',
] as const;

export type NotifyStatus = (typeof ALL_NOTIFY_STATUSES)[number];

/** Statuses that matter during live trading — failures + exit flow + open holds. */
export const CRITICAL_NOTIFY_STATUSES: readonly NotifyStatus[] = [
  'Holding',
  'ExitPending',
  'ExitFailed',
  'ExitUnconfirmed',
];

export type NotifyPreset = 'all' | 'critical' | 'custom';

function statusSetEqual(a: readonly string[], b: readonly string[]): boolean {
  if (a.length !== b.length) return false;
  const set = new Set(a);
  return b.every((s) => set.has(s));
}

/** Which preset pill matches the current status selection (else `custom`). */
export function notifyPresetOf(statuses: readonly string[]): NotifyPreset {
  if (statusSetEqual(statuses, ALL_NOTIFY_STATUSES)) return 'all';
  if (statusSetEqual(statuses, CRITICAL_NOTIFY_STATUSES)) return 'critical';
  return 'custom';
}

/** @deprecated Prefer {@link ALL_NOTIFY_STATUSES}. */
export const ALL_POSITION_STATUSES = ALL_NOTIFY_STATUSES;
/** @deprecated Prefer {@link NotifyStatus}. */
export type PositionStatus = NotifyStatus;

export interface NotificationPrefs {
  realEnabled: boolean;
  paperEnabled: boolean;
  desktopEnabled: boolean;
  statuses: string[];
}

const DEFAULT_PREFS: NotificationPrefs = {
  realEnabled: true,
  paperEnabled: false,
  desktopEnabled: false,
  statuses: [...ALL_NOTIFY_STATUSES],
};

const KNOWN = new Set<string>(ALL_NOTIFY_STATUSES);

/** Map legacy `Arming` (pre-redesign position status) → `Armed`. */
function migrateStatuses(statuses: string[]): string[] {
  return [...new Set(statuses.map((s) => (s === 'Arming' ? 'Armed' : s)))].filter((s) =>
    KNOWN.has(s),
  );
}

function statusesEqual(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((s, i) => s === b[i]);
}

export function useNotificationPrefs() {
  const [prefs, setPrefs] = useLocalStorage<NotificationPrefs>(
    STORAGE_KEYS.notificationPrefs,
    DEFAULT_PREFS,
  );

  // Persist one-shot cleanup of legacy `Arming` / unknown keys.
  useEffect(() => {
    const migrated = migrateStatuses(prefs.statuses);
    if (!statusesEqual(migrated, prefs.statuses)) {
      setPrefs((p) => ({ ...p, statuses: migrateStatuses(p.statuses) }));
    }
  }, [prefs.statuses, setPrefs]);

  return [prefs, setPrefs] as const;
}
