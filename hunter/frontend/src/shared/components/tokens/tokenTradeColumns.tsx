import type { ColumnDef } from 'components/table/types';
import type { FlowReason } from 'lib/flow/classifyFlow';
import type { TradeRecord } from 'types';
import { DateCell } from 'components/table/DateCell';
import { formatDecimal } from 'utils/format';
import { AmountCell, FeeCell, PriceCell } from 'components/tokens/priceCells';
import { cn } from 'lib/cn';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { Badge } from 'components/ui/Badge';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { formatIxLabelsText } from 'lib/ixLabels';

export interface TokenTradeColumnsOpts {
  /**
   * `volume_ix_patterns` keys (`JSON.stringify(labels)`) to test each row's
   * structure against. When non-empty, prepends the Vol/Non-vol badge column.
   * Omit or empty → column hidden, unless {@link onTogglePattern} is set.
   */
  flowPatternKeys?: ReadonlySet<string> | null;
  /**
   * Makes the badge a click-to-stage control: adds/removes that row's ordered
   * `instruction_labels` in the app-wide pattern draft. Set ⇒ the column always
   * renders, because authoring starts from an EMPTY set and the rows you click
   * into it are the whole point.
   */
  onTogglePattern?: ((labels: readonly string[]) => void) | null;
  /**
   * Effective (contagion-aware) classification per trade id — what the chart's
   * lines actually did with the row. The badge tests structure alone, so without
   * this a row reading "Non-vol" whose SOL sits on the vol line looks like a bug.
   */
  flowReasons?: ReadonlyMap<string, FlowReason> | null;
}

/** True when ordered `instruction_labels` exact-match a volume_ix_patterns row. */
export function isVolumeIxPattern(
  labels: readonly string[] | null | undefined,
  patternKeys: ReadonlySet<string>,
): boolean {
  return !!labels && labels.length > 0 && patternKeys.has(JSON.stringify(labels));
}

/** Stable empty set so an unconfigured column doesn't allocate per render. */
const EMPTY_PATTERN_KEYS: ReadonlySet<string> = new Set<string>();

/** Why the chart counted a row as volume when its own structure didn't. */
const CONTAGION_NOTE: Record<Exclude<FlowReason, 'structural'>, string> = {
  creator: 'via creator',
  wallet: 'via wallet',
};

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
  const keys = opts?.flowPatternKeys ?? EMPTY_PATTERN_KEYS;
  const onToggle = opts?.onTogglePattern ?? null;
  const reasons = opts?.flowReasons ?? null;
  const showVol = keys.size > 0 || onToggle != null;

  const leading: ColumnDef<TradeRecord>[] = [];

  if (showVol) {
    leading.push({
      key: 'is_volume_ix_pattern',
      label: 'Vol',
      tooltip: onToggle
        ? 'Structural volume ix-pattern match. Click to stage/unstage this trade’s ordered ' +
          'instruction_labels as a volume_ix_pattern — the chart’s vol/non-vol lines redraw ' +
          'immediately. “via creator/wallet” = the lines already count this row through ' +
          'contagion, whatever its own structure is.'
        : 'Structural volume ix-pattern match — this trade’s ordered instruction_labels ' +
          'exact-match a volume_ix_patterns row (no creator/wallet contagion).',
      render: (t) => {
        const labels = t.instruction_labels;
        if (!labels || labels.length === 0) {
          return <span className="text-text-dim/40">—</span>;
        }
        const isVol = isVolumeIxPattern(labels, keys);
        const reason = reasons?.get(t.id) ?? null;
        const note = reason && reason !== 'structural' ? CONTAGION_NOTE[reason] : null;
        const badge = (
          <Badge
            variant={isVol ? 'danger' : 'neutral'}
            size="sm"
            className={onToggle ? 'cursor-pointer' : undefined}
          >
            {isVol ? 'Vol' : 'Non-vol'}
          </Badge>
        );
        const cell = (
          <span className="inline-flex items-center gap-1">
            {onToggle ? (
              <button
                type="button"
                aria-pressed={isVol}
                title={
                  isVol
                    ? 'Staged as a volume_ix_pattern — click to remove'
                    : 'Click to stage this structure as a volume_ix_pattern'
                }
                onClick={(e) => {
                  // The row itself is selectable on several hosts; a stage click
                  // must not also change the table's selection.
                  e.stopPropagation();
                  onToggle(labels);
                }}
                className="rounded-md focus:outline-none focus-visible:ring-1 focus-visible:ring-primary"
              >
                {badge}
              </button>
            ) : (
              badge
            )}
            {note && (
              <span className="text-[9px] uppercase tracking-wide text-text-dim/70">
                {note}
              </span>
            )}
          </span>
        );
        return cell;
      },
      // Structure outranks contagion: sorting this column is for finding the rows
      // whose pattern you can actually toggle.
      sortValue: (t) =>
        isVolumeIxPattern(t.instruction_labels, keys)
          ? 2
          : reasons?.get(t.id)
            ? 1
            : 0,
      searchValue: (t) => {
        const structural = isVolumeIxPattern(t.instruction_labels, keys);
        const reason = reasons?.get(t.id) ?? null;
        const note = reason && reason !== 'structural' ? CONTAGION_NOTE[reason] : '';
        return `${structural ? 'vol' : 'non-vol'}${note ? ` ${note}` : ''}`;
      },
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
      filterNumber: (t) => t.amount_sol,
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
      filterNumber: (t) => t.token_amount,
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
      filterNumber: (t) => t.price_per_token,
    },
    {
      key: 'fee',
      label: 'Fee',
      tooltip:
        'Network fee paid to land this trade’s transaction — base signature fee + priority ' +
        'fee, as reported on-chain. Charged once per transaction, so the legs of a multi-leg ' +
        'tx all show the same value. Excludes the Jito tip (a transfer, not a fee) and the ' +
        'venue’s own swap fee (already inside the SOL amount). “—” = not captured (trades ' +
        'ingested before the fee column existed; it cannot be backfilled).',
      render: (t) => (
        <span className="text-text-dim">
          <FeeCell sol={t.fee_sol} />
        </span>
      ),
      // Unknown sorts below every real fee instead of tying with a genuine
      // minimum — `null` is "not captured", not "cheapest".
      sortValue: (t) => t.fee_sol ?? -1,
      searchValue: (t) => (t.fee_sol != null ? String(t.fee_sol) : ''),
      filterNumber: (t) => t.fee_sol ?? null,
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
      filterNumber: (t) => t.slot,
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
