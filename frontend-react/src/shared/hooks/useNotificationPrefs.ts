import { useLocalStorage } from './useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';

export const ALL_POSITION_STATUSES = [
  'Arming',
  'BuySubmitted',
  'Holding',
  'ExitPending',
  'End',
  'ExitFailed',
] as const;

export type PositionStatus = (typeof ALL_POSITION_STATUSES)[number];

/** Each group is one checkbox in the Settings UI and one formatted segment in the toast body. */
export const FP_PARAM_GROUPS = [
  { key: 'buy_tol',  label: 'Initial buy + Tolerance',  hint: 'ib:0.1◎ tol:2%'      },
  { key: 'cu',       label: 'CU limit + price',          hint: 'lim:200K cu:1M'       },
  { key: 'cost',     label: 'Max cost + Spendable SOL',  hint: 'max:0.15◎ spnd:0.1◎'  },
  { key: 'ix',       label: 'IX labels',                 hint: '[pump,amm]'            },
  { key: 'tp_sl',    label: 'TP + SL',                   hint: 'TP:50% SL:-20%'        },
] as const;

export type FpGroupKey = (typeof FP_PARAM_GROUPS)[number]['key'];

export interface NotificationPrefs {
  realEnabled: boolean;
  paperEnabled: boolean;
  desktopEnabled: boolean;
  statuses: string[];
  fpParams: string[];
}

const DEFAULT_PREFS: NotificationPrefs = {
  realEnabled: true,
  paperEnabled: false,
  desktopEnabled: false,
  statuses: [...ALL_POSITION_STATUSES],
  fpParams: ['buy_tol', 'cu', 'tp_sl'],
};

export function useNotificationPrefs() {
  return useLocalStorage<NotificationPrefs>(STORAGE_KEYS.notificationPrefs, DEFAULT_PREFS);
}
