import { useLocalStorage } from './useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';

export const ALL_POSITION_STATUSES = [
  'BuySubmitted',
  'Holding',
  'ExitPending',
  'End',
  'ExitFailed',
  'ExitUnconfirmed',
] as const;

export type PositionStatus = (typeof ALL_POSITION_STATUSES)[number];

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
  statuses: [...ALL_POSITION_STATUSES],
};

export function useNotificationPrefs() {
  return useLocalStorage<NotificationPrefs>(STORAGE_KEYS.notificationPrefs, DEFAULT_PREFS);
}
