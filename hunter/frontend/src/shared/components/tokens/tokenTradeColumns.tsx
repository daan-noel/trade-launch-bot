import type { ColumnDef } from 'components/table/types';
import type { TradeRecord } from 'types';
import { DateCell } from 'components/table/DateCell';
import { formatDecimal } from 'utils/format';
import { AmountCell, PriceCell } from 'components/tokens/priceCells';
import { cn } from 'lib/cn';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { Badge } from 'components/ui/Badge';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { formatIxLabelsText } from 'lib/ixLabels';

export interface TokenTradeColumnsOpts {
  /**
   * Fingerprint `volume_ix_patterns` keys (`JSON.stringify(labels)`). When
   * non-empty, prepends a read-only Vol/Non-vol badge (structural ix match
   * only — no creator/wallet contagion). Omit or empty → column hidden.
   */
  flowPatternKeys?: ReadonlySet<string> | null;
}

/** True when ordered `instruction_labels` exact-match a volume_ix_patterns row. */
export function isVolumeIxPattern(
  labels: readonly string[] | null | undefined,
  patternKeys: ReadonlySet<string>,
): boolean {
  return !!labels && labels.length > 0 && patternKeys.has(JSON.stringify(labels));
}

/**
 * Takes only the unit *label* (not the whole `usePriceDisplay` object) so the
 * column array stays referentially stable across USD-rate ticks — the rate
 * changes the price object's identity every tick, which would otherwise rebuild
 * every column and re-render the entire trades table. The two rate-dependent
 * value cells use the memoized `AmountCell`/`PriceCell`, which read the rate from
 * context themselves and re-render in isolation when it changes.
 */
export function tokenTradeColumns(
  unit: string,
  opts?: TokenTradeColumnsOpts,
): ColumnDef<TradeRecord>[] {
  const patternKeys = opts?.flowPatternKeys;
  const showVol = patternKeys != null && patternKeys.size > 0;

  const leading: ColumnDef<TradeRecord>[] = [];

  if (showVol) {
    const keys = patternKeys;
    leading.push({
      key: 'is_volume_ix_pattern',
      label: 'Vol',
      tooltip:
        'Structural volume ix-pattern match — this trade’s ordered instruction_labels ' +
        'exact-match a fingerprint volume_ix_patterns row (no creator/wallet contagion).',
      render: (t) => {
        const labels = t.instruction_labels;
        if (!labels || labels.length === 0) {
          return <span className="text-text-dim/40">—</span>;
        }
        const isVol = isVolumeIxPattern(labels, keys);
        return (
          <Badge variant={isVol ? 'danger' : 'neutral'} size="sm">
            {isVol ? 'Vol' : 'Non-vol'}
          </Badge>
        );
      },
      sortValue: (t) => (isVolumeIxPattern(t.instruction_labels, keys) ? 1 : 0),
      searchValue: (t) =>
        isVolumeIxPattern(t.instruction_labels, keys) ? 'vol' : 'non-vol',
    });
  }

  leading.push({
    key: 'ix_structure',
    label: 'ix_labels',
    tooltip: 'Ordered instruction-label structure of this trade — the flow-split matching key.',
    render: (t) => (
      <IxLabelsDisplay
        labels={t.instruction_labels ?? []}
        empty="—"
        copyJson
        maxHeight="4.5rem"
      />
    ),
    searchValue: (t) => formatIxLabelsText(t.instruction_labels ?? []),
  });

  return [
    ...leading,
    {
      key: 'side',
      label: 'Side',
      render: (t) => {
        const isBuy = t.trade_type === 'buy';
        return (
          <span
            className={cn(
              'inline-block rounded px-2 py-0.5 text-[11px] font-bold tracking-wide',
              isBuy
                ? 'border border-buy bg-buy/15 text-buy'
                : 'border border-sell bg-sell/15 text-sell',
            )}
          >
            {isBuy ? 'BUY' : 'SELL'}
          </span>
        );
      },
      sortValue: (t) => t.trade_type,
      searchValue: (t) => t.trade_type,
    },
    {
      key: 'wallet',
      label: 'Wallet',
      render: (t) => (
        <AddressDisplay address={t.wallet_address} kind="account" />
      ),
      sortValue: (t) => t.wallet_address,
      searchValue: (t) => t.wallet_address,
    },
    {
      key: 'sol',
      label: unit,
      render: (t) => {
        const isBuy = t.trade_type === 'buy';
        return (
          <span className={cn('font-semibold', isBuy ? 'text-buy' : 'text-sell')}>
            <AmountCell sol={t.amount_sol} />
          </span>
        );
      },
      sortValue: (t) => t.amount_sol,
      searchValue: (t) => String(t.amount_sol),
    },
    {
      key: 'tokens',
      label: 'Tokens',
      render: (t) => {
        const isBuy = t.trade_type === 'buy';
        return (
          <span className={cn('font-semibold', isBuy ? 'text-buy' : 'text-sell')}>
            {formatDecimal(t.token_amount, 0)}
          </span>
        );
      },
      sortValue: (t) => t.token_amount,
      searchValue: (t) => String(t.token_amount),
    },
    {
      key: 'price',
      label: `Price (${unit})`,
      render: (t) => {
        const isBuy = t.trade_type === 'buy';
        return (
          <span className={cn('font-semibold', isBuy ? 'text-buy' : 'text-sell')}>
            <PriceCell sol={t.price_per_token} />
          </span>
        );
      },
      sortValue: (t) => t.price_per_token,
      searchValue: (t) => String(t.price_per_token),
    },
    {
      key: 'signature',
      label: 'Signature',
      render: (t) => (
        <AddressDisplay address={t.tx_signature} kind="transaction" />
      ),
      sortValue: (t) => t.tx_signature,
      searchValue: (t) => t.tx_signature,
    },
    {
      key: 'slot',
      label: 'Slot',
      render: (t) => t.slot,
      sortValue: (t) => t.slot,
      searchValue: (t) => String(t.slot),
    },
    {
      key: 'time',
      label: 'Time (UTC)',
      width: '108px',
      render: (t) => <DateCell iso={t.received_at ?? t.block_time} />,
      sortValue: (t) => t.received_at ?? t.block_time,
      searchValue: (t) => t.received_at ?? t.block_time,
    },
  ];
}
