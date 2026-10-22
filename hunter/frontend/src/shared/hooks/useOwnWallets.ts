import { useMemo } from 'react';
import { useGetOwnWalletsQuery, useGetProfilesQuery } from 'store/apiSlice';

const EMPTY: ReadonlySet<string> = new Set();

/**
 * Every address that is ours: the trader wallet registry (`trader_wallets`, archived
 * too - the set the engine keeps off every tag) plus the wallets of the `mine`
 * profile. The one home for "is this trade ours": the `mine` badge and the tag
 * exclusion of the chart's trade panel both read it, so neither can miss a wallet.
 */
export function useOwnWallets(): ReadonlySet<string> {
  const { data: traders } = useGetOwnWalletsQuery();
  const { data: profiles } = useGetProfilesQuery();
  return useMemo(() => {
    if (!traders?.length && !profiles?.length) return EMPTY;
    const out = new Set<string>(traders ?? []);
    for (const p of profiles ?? []) {
      if (p.profile_type === 'mine') for (const w of p.wallets) out.add(w.address);
    }
    return out;
  }, [traders, profiles]);
}

/** `exclude` plus our own wallets; the same set object when we have none to add. */
export function withOwnWallets(
  exclude: ReadonlySet<string> | null | undefined,
  own: ReadonlySet<string>,
): ReadonlySet<string> | null {
  if (own.size === 0) return exclude ?? null;
  if (!exclude || exclude.size === 0) return own;
  return new Set([...exclude, ...own]);
}
