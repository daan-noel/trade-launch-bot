import type { ColumnDef } from 'components/table/types';
import type { TokenRecord } from 'types';
import type { SwingChainStats } from './swingChains';

/**
 * The three "Chain of Swings" columns appended to the tokens table after a
 * global swing-detection run. `statsByMint` holds per-token results; tokens not
 * yet analysed render a dash and sort last.
 *
 * `sortable` defaults to true (client-side tables). Pass `false` when the host
 * table is server-side paged: these columns are derived in the browser, so the
 * backend can't order by them — a clickable header would emit a sort the server
 * silently ignores, leaving a misleading arrow.
 */
export function swingChainColumns(
  statsByMint: Map<string, SwingChainStats>,
  sortable = true,
): ColumnDef<TokenRecord>[] {
  const stat = (r: TokenRecord) => statsByMint.get(r.mint_address);

  const numColumn = (
    key: string,
    label: string,
    width: string,
    pick: (s: SwingChainStats) => number,
  ): ColumnDef<TokenRecord> => ({
    key,
    label,
    width,
    sortable,
    render: (r) => {
      const s = stat(r);
      return s ? pick(s) : '—';
    },
    sortValue: (r) => {
      const s = stat(r);
      return s ? pick(s) : null;
    },
    searchValue: (r) => {
      const s = stat(r);
      return s ? String(pick(s)) : '';
    },
  });

  return [
    numColumn('swing_pairs', 'Swing Pairs', '94px', (s) => s.totalPairCount),
    numColumn('max_seq_pairs', 'Max Seq Pairs', '112px', (s) => s.maxSequentialPairCount),
    numColumn('chain_count', 'Chains', '76px', (s) => s.chainCount),
  ];
}
