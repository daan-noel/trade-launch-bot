import { lensItemId } from 'components/token-price-chart/lensTint';
import type { TokenHighlightItem } from 'components/tokens/useTokenHighlight';
import { serializeIxPatternRow } from 'lib/strategy/ixPatternRows';
import type { FlowDiscoveryTokenGross, FlowLensHitItem, FlowLensHits } from 'types';

/** Per coin with a hit: lens item id -> trades it matches (`FlowLensHits.by_mint`). */
export type LensHitsByMint = FlowLensHits['by_mint'];

/** The armed highlight as the `lens-hits` request names it: each item keyed by its
 *  `lensItemId`, a structure as the `ix_shape` entry its row saves as. */
export function lensHitItems(items: readonly TokenHighlightItem[]): FlowLensHitItem[] {
  const out: FlowLensHitItem[] = [];
  for (const i of items) {
    const id = lensItemId(i);
    if (i.kind === 'wallet') out.push({ kind: 'wallet', id, wallet: i.key });
    else if (i.row) out.push({ kind: 'structure', id, row: serializeIxPatternRow(i.row) });
  }
  return out;
}

/** The armed items `mint` carries, arming order, each with its trade count. */
export function tokenLensHits(
  mint: string,
  items: readonly TokenHighlightItem[],
  hits: LensHitsByMint | null,
): { item: TokenHighlightItem; trades: number }[] {
  const coin = hits?.[mint];
  if (!coin) return [];
  return items.flatMap((item) => {
    const trades = coin[lensItemId(item)] ?? 0;
    return trades > 0 ? [{ item, trades }] : [];
  });
}

/**
 * The token list under the highlight. Without hits it is `tokens` as given. With
 * hits it is ordered by how many armed items a coin carries (ties keep the gross
 * order), and with `matchingOnly` it keeps only coins carrying every id in
 * `required` - or, when `required` is empty, at least one armed item.
 */
export function lensRoster(
  tokens: readonly FlowDiscoveryTokenGross[],
  items: readonly TokenHighlightItem[],
  hits: LensHitsByMint | null,
  required: ReadonlySet<string>,
  matchingOnly: boolean,
): readonly FlowDiscoveryTokenGross[] {
  if (!hits || items.length === 0) return tokens;
  const ids = items.map(lensItemId);
  const carried = (t: FlowDiscoveryTokenGross) => {
    const coin = hits[t.mint_address];
    return coin ? ids.filter((id) => (coin[id] ?? 0) > 0).length : 0;
  };
  const has = (t: FlowDiscoveryTokenGross, id: string) => (hits[t.mint_address]?.[id] ?? 0) > 0;
  const kept = !matchingOnly
    ? tokens
    : required.size > 0
      ? tokens.filter((t) => [...required].every((id) => has(t, id)))
      : tokens.filter((t) => carried(t) > 0);
  return kept
    .map((t, i) => ({ t, i, n: carried(t) }))
    .sort((a, b) => b.n - a.n || a.i - b.i)
    .map((x) => x.t);
}
