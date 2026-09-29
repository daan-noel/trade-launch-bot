import { useMemo, type ReactNode } from 'react';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { useProfileWallets } from 'hooks/useProfileWallets';
import {
  clampInt,
  CUSTOM_PRESET,
  DAY_MS,
  DEFAULT_DAYS,
  groupByProfile,
  MAX_DAYS,
  msToWallClock,
  shortAddr,
  TRADER_LOOKBACK_PRESETS,
} from './traderQuery';

export const FIELD_LABEL =
  'flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim';

/**
 * The wallet-study input row: wallet address, tracked-wallet picker, look-back
 * range. Shared by Trader Analysis and Entry Context so both ask for a wallet
 * and a window the same way; each page appends its own knobs as `children`.
 */
export function TraderQueryInputs({
  wallet,
  onWallet,
  onPickWallet,
  days,
  from,
  to,
  onRange,
  onEnter,
  timezone,
  children,
}: {
  wallet: string;
  onWallet: (w: string) => void;
  /** Picking a tracked wallet fills the input AND analyzes it. */
  onPickWallet: (w: string) => void;
  days: string;
  from: string;
  to: string;
  onRange: (r: { days: string; from: string; to: string }) => void;
  onEnter: () => void;
  timezone: string;
  children?: ReactNode;
}) {
  const profileWallets = useProfileWallets();
  const profileGroups = useMemo(() => groupByProfile(profileWallets), [profileWallets]);
  // Reflect the picker's selection only while the input still holds a known
  // tracked address; typing a custom address falls back to the placeholder.
  const picked = profileWallets.some((w) => w.address === wallet) ? wallet : '';
  const isCustom = days === CUSTOM_PRESET;
  // Seed instant for the day presets. Recomputed only when the preset (or zone)
  // changes — it feeds a draft and a trigger hint, not the query.
  const presetFrom = useMemo(
    () =>
      isCustom ? '' : msToWallClock(Date.now() - clampInt(days, DEFAULT_DAYS, 1, MAX_DAYS) * DAY_MS, timezone),
    [isCustom, days, timezone],
  );

  return (
    <>
      <label className={FIELD_LABEL}>
        Wallet address
        <Input
          value={wallet}
          onChange={(e) => onWallet(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') onEnter();
          }}
          placeholder="Solana base58 address"
          className="min-w-[420px] font-mono font-normal normal-case tracking-normal"
        />
      </label>
      {profileWallets.length > 0 && (
        <label className={FIELD_LABEL}>
          Tracked wallet
          <Select
            value={picked}
            onChange={(e) => {
              if (e.target.value) onPickWallet(e.target.value);
            }}
            className="min-w-[200px] font-normal normal-case tracking-normal"
          >
            <option value="">Pick a profile wallet…</option>
            {profileGroups.map((group) => (
              <optgroup key={group.profileName} label={group.profileName}>
                {group.wallets.map((w) => (
                  <option key={w.address} value={w.address}>
                    {shortAddr(w.address)}
                  </option>
                ))}
              </optgroup>
            ))}
          </Select>
        </label>
      )}
      <label className={FIELD_LABEL}>
        Look-back
        <DateTimeRangePicker
          aria-label="Look-back window"
          size="sm"
          timeZone={timezone}
          emptyLabel="Pick a range"
          customPreset={CUSTOM_PRESET}
          presets={[...TRADER_LOOKBACK_PRESETS]}
          // A day preset still hands the picker its resolved bounds, so the
          // trigger reads "7 days · 08/18 → now" and switching to Custom opens
          // on that window instead of a blank calendar.
          value={{ preset: days, from: isCustom ? from : presetFrom, to: isCustom ? to : '' }}
          onChange={({ preset, from: f, to: t }) =>
            preset === CUSTOM_PRESET
              ? onRange({ days: CUSTOM_PRESET, from: f, to: t })
              : onRange({ days: preset, from: '', to: '' })
          }
        />
      </label>
      {children}
    </>
  );
}
