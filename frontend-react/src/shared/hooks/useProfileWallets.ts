import { useMemo } from 'react';
import { CHART_COLORS, WALLET_MARKER_COLORS, type ProfileWalletInfo } from 'components/token-price-chart';
import { useGetProfilesQuery } from 'store/apiSlice';
import type { WalletProfile } from 'types';

/** Stable empty default so the memo below doesn't recompute while loading. */
const EMPTY_PROFILES: WalletProfile[] = [];

function buildProfileWallets(profiles: WalletProfile[]): ProfileWalletInfo[] {
  const result: ProfileWalletInfo[] = [];
  let colorIdx = 0;
  for (const profile of profiles) {
    const isMine = profile.profile_type === 'mine';
    for (const wallet of profile.wallets) {
      if (!wallet.is_tracked) continue;
      result.push({
        address: wallet.address,
        label: wallet.comment
          ? wallet.comment.slice(0, 12)
          : `${wallet.address.slice(0, 4)}…${wallet.address.slice(-4)}`,
        // `mine` gets a fixed color instead of the rotating palette, so it stays
        // recognizable regardless of how many other wallets are tracked.
        color: isMine ? CHART_COLORS.mine : WALLET_MARKER_COLORS[colorIdx % WALLET_MARKER_COLORS.length],
        profileName: profile.name,
        tags: profile.tags.map((t) => ({ name: t.name, color: t.color })),
        isMine,
      });
      if (!isMine) colorIdx++;
    }
  }
  return result;
}

/**
 * Tracked-wallet markers for the trade chart, sourced from the shared profiles
 * cache (same `getProfiles` query the Profiles page edits). Centralizes what
 * used to be duplicated per-page `buildProfileWallets` logic.
 */
export function useProfileWallets(): ProfileWalletInfo[] {
  const { data: profiles = EMPTY_PROFILES } = useGetProfilesQuery();
  return useMemo(() => buildProfileWallets(profiles), [profiles]);
}
