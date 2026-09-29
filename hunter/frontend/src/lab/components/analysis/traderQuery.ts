/**
 * The query knobs every wallet-study page shares (Trader Analysis, Entry Context):
 * the look-back presets, the wall-clock ⇄ UTC conversions the range picker needs,
 * and the tracked-wallet grouping for the picker. One copy, so the two pages ask
 * for a window the same way.
 */

import type { ProfileWalletInfo } from 'components/token-price-chart/types';
import { datetimeLocalToUtcWallClock, utcIsoToDatetimeLocal } from 'utils/date';

// Look-back clamp mirrors the backend (`MAX_WINDOW_DAYS` in
// `lab/src/api/handlers/wallets.rs`), and bounds the custom range's SPAN too.
export const DEFAULT_DAYS = 7;
export const MAX_DAYS = 90;
export const DAY_MS = 86_400_000;

/** The picker's custom-range sentinel — `days` holds this instead of a day count
 *  while the window is an explicit `from`/`to` pair. */
export const CUSTOM_PRESET = 'custom';

export const TRADER_LOOKBACK_PRESETS = [
  { value: '1', label: '1 day' },
  { value: '3', label: '3 days' },
  { value: '7', label: '7 days' },
  { value: '14', label: '14 days' },
  { value: '30', label: '30 days' },
  { value: '60', label: '60 days' },
  { value: '90', label: '90 days' },
  {
    value: CUSTOM_PRESET,
    label: 'Custom',
    description: `Exact from → to, max ${MAX_DAYS}d span`,
  },
] as const;

/** A wall-clock `YYYY-MM-DDTHH:mm` in `tz` for an instant — the picker's wire
 *  shape. Seeds the popover draft from whatever window is active, so switching a
 *  day preset to Custom starts from that preset's bounds instead of blank. */
export const msToWallClock = (ms: number, tz: string) =>
  utcIsoToDatetimeLocal(new Date(ms).toISOString(), tz);

/** The picker's wall-clock (project zone) → the UTC RFC3339 instant the API
 *  takes. `bound` keeps a DST-ambiguous hour inside the range (see
 *  `datetimeLocalToUtcWallClock`). Blank in ⇒ blank out (no bound). */
export const wallClockToUtcIso = (wall: string, tz: string, bound: 'lower' | 'upper') => {
  const utc = datetimeLocalToUtcWallClock(wall, tz, bound);
  return utc ? `${utc}Z` : '';
};

export const clampInt = (raw: string, fallback: number, min: number, max: number) => {
  const n = parseInt(raw, 10);
  return Math.min(max, Math.max(min, Number.isFinite(n) ? n : fallback));
};

export const shortAddr = (a: string) => `${a.slice(0, 4)}…${a.slice(-4)}`;

/** The tracked wallets grouped by their profile name, for the picker's optgroups.
 *  `mine`-profile wallets sort first so your own wallet is easy to reach. */
export function groupByProfile(
  wallets: ProfileWalletInfo[],
): { profileName: string; wallets: ProfileWalletInfo[] }[] {
  const order: string[] = [];
  const byName = new Map<string, ProfileWalletInfo[]>();
  for (const w of wallets) {
    const name = w.profileName ?? 'Untitled';
    let bucket = byName.get(name);
    if (!bucket) {
      bucket = [];
      byName.set(name, bucket);
      order.push(name);
    }
    bucket.push(w);
  }
  return order
    .map((profileName) => ({ profileName, wallets: byName.get(profileName)! }))
    .sort((a, b) => Number(b.wallets[0]?.isMine) - Number(a.wallets[0]?.isMine));
}
