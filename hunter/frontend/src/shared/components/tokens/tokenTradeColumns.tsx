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
import { patternKey } from 'lib/flow/volumePatterns';
// Deep import: `constants` is type-only w.r.t. lightweight-charts, so the wash
// colors come along without dragging the charting library into this chunk.
import { CHART_COLORS } from 'components/token-price-chart/constants';

export interface TokenTradeColumnsOpts {
  /**
   * `ix_patterns` keys (`JSON.stringify(labels)`) to test each row's
   * structure against. When non-empty, prepends the Vol/Non-vol badge column.
   * Omit or empty → column hidden, unless {@link onTogglePattern} is set.
   */
  flowPatternKeys?: ReadonlySet<string> | null;
  /**
   * Makes the badge an edit control: adds/removes that row's ordered
   * `instruction_labels` in the target fingerprint's saved `ix_patterns`.
   * Set ⇒ the column always renders, because authoring starts from an EMPTY set
   * and the rows you click into it are the whole point.
   *
   * There is no staging step — a click PERSISTS, and every active rule bound to
   * that fingerprint classifies flow differently from the engine's next rules
   * reload on. Pass {@link toggleTargetName} so the row says which one.
   */
  onTogglePattern?: ((labels: readonly string[]) => void) | null;
  /** Name of the fingerprint {@link onTogglePattern} writes to — named in the
   *  badge tooltip, since the click is an immediate save and not a local edit. */
  toggleTargetName?: string | null;
  /**
   * Effective (contagion-aware) classification per trade id — what the chart's
   * lines actually did with the row. The badge tests structure alone, so without
   * this a row reading "Non-vol" whose SOL sits on the vol line looks like a bug.
   */
  flowReasons?: ReadonlyMap<string, FlowReason> | null;
  /**
   * Arms the ephemeral WALLET highlight lens from a row — adds a target button to
   * the Wallet cell. Nothing is persisted: this only washes candles and rows.
   */
  onLensWallet?: ((address: string) => void) | null;
  /** The armed wallet, so its own rows render the button lit. */
  lensWallet?: string | null;
  /**
   * Arms the ephemeral IX-STRUCTURE lens from a row. Deliberately separate from
   * {@link onTogglePattern}, which lives one column over and SAVES to the
   * fingerprint the engine reads — asking "where else did this shape appear" must
   * never change how a live rule classifies flow.
   */
  onLensStructure?: ((labels: readonly string[]) => void) | null;
  /** `patternKey` of the armed structure, so matching rows render the button lit. */
  lensStructureKey?: string | null;
}

/** Target glyph for a highlight-lens toggle — reads as "find this everywhere". */
function LensIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden className="size-3">
      <circle cx="8" cy="8" r="3.25" stroke="currentColor" strokeWidth="1.4" />
      <path
        d="M8 1.5v2.2M8 12.3v2.2M1.5 8h2.2M12.3 8h2.2"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
    </svg>
  );
}

/** The glyph's box. A button and its spacer MUST share it: a row that renders one
 *  and a row that renders neither would start their content at different x, which
 *  reads as a ragged column. */
const LENS_SLOT = 'block size-3 shrink-0 p-px';

/** Holds the slot open on a row that has nothing to arm (no labels captured). */
function LensSpacer() {
  return <span className={LENS_SLOT} aria-hidden />;
}

/**
 * The one control that arms a highlight lens. Lit while its target is the armed
 * one, so a row can say "this is what the chart is washing" without a legend.
 */
function LensButton({
  armed,
  color,
  title,
  onClick,
}: {
  armed: boolean;
  color: string;
  title: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={armed}
      title={title}
      onClick={(e) => {
        // Several hosts make the row itself selectable; arming a lens must not
        // also move the table's selection.
        e.stopPropagation();
        onClick();
      }}
      className={cn(
        LENS_SLOT,
        'rounded transition focus:outline-none focus-visible:ring-1 focus-visible:ring-primary',
        armed ? 'opacity-100' : 'opacity-30 hover:opacity-90',
      )}
      style={{ color: armed ? color : undefined }}
    >
      <LensIcon />
    </button>
  );
}

/** True when ordered `instruction_labels` exact-match a ix_patterns row. */
export function isIxPattern(
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
  const showTagged = keys.size > 0 || onToggle != null;
  const onLensWallet = opts?.onLensWallet ?? null;
  const lensWallet = opts?.lensWallet ?? null;
  const onLensStructure = opts?.onLensStructure ?? null;
  const lensStructureKey = opts?.lensStructureKey ?? null;
  const targetLabel = opts?.toggleTargetName ? `“${opts.toggleTargetName}”` : 'the fingerprint';

  const leading: ColumnDef<TradeRecord>[] = [];

  if (showTagged) {
    leading.push({
      key: 'is_volume_ix_pattern',
      label: 'Vol',
      tooltip: onToggle
        ? `Structural volume ix-pattern match. Clicking SAVES this trade’s ordered ` +
          `instruction_labels to ${targetLabel} as a volume_ix_pattern — there is no staging ` +
          `step, and every active rule bound to it classifies flow differently from the ` +
          `engine’s next rules reload on. “via creator/wallet” = the lines already count this ` +
          `row through contagion, whatever its own structure is.`
        : 'Structural volume ix-pattern match — this trade’s ordered instruction_labels ' +
          'exact-match a ix_patterns row (no creator/wallet contagion).',
      render: (t) => {
        const labels = t.instruction_labels;
        if (!labels || labels.length === 0) {
          return <span className="text-text-dim/40">—</span>;
        }
        const isTagged = isIxPattern(labels, keys);
        const reason = reasons?.get(t.id) ?? null;
        const note = reason && reason !== 'structural' ? CONTAGION_NOTE[reason] : null;
        const badge = (
          <Badge
            variant={isTagged ? 'danger' : 'neutral'}
            size="sm"
            className={onToggle ? 'cursor-pointer' : undefined}
          >
            {isTagged ? 'Vol' : 'Non-vol'}
          </Badge>
        );
        const cell = (
          <span className="inline-flex items-center gap-1">
            {onToggle ? (
              <button
                type="button"
                aria-pressed={isTagged}
                title={
                  isTagged
                    ? `Saved as a volume_ix_pattern on ${targetLabel} — click to remove it`
                    : `Click to save this structure as a volume_ix_pattern on ${targetLabel}`
                }
                onClick={(e) => {
                  // The row itself is selectable on several hosts; an edit click
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
        isIxPattern(t.instruction_labels, keys)
          ? 2
          : reasons?.get(t.id)
            ? 1
            : 0,
      searchValue: (t) => {
        const structural = isIxPattern(t.instruction_labels, keys);
        const reason = reasons?.get(t.id) ?? null;
        const note = reason && reason !== 'structural' ? CONTAGION_NOTE[reason] : '';
        return `${structural ? 'vol' : 'non-vol'}${note ? ` ${note}` : ''}`;
      },
    });
  }

  leading.push({
    key: 'ix_structure',
    label: 'ix_labels',
    tooltip:
      'Ordered instruction-label structure of this trade — the flow-split matching key. ' +
      (onLensStructure
        ? 'Click the target to wash every candle this exact ordered structure appeared in. ' +
          'View-only: unlike the Vol badge, it saves nothing and no rule reads it.'
        : ''),
    render: (t) => {
      const labels = t.instruction_labels ?? [];
      return (
        <span className="flex items-start gap-1">
          {onLensStructure &&
            (labels.length > 0 ? (
              <LensButton
                armed={lensStructureKey === patternKey(labels)}
                color={CHART_COLORS.lensStructure}
                title={
                  lensStructureKey === patternKey(labels)
                    ? 'Stop highlighting this ix structure'
                    : 'Highlight every candle and row with this exact ordered structure'
                }
                onClick={() => onLensStructure(labels)}
              />
            ) : (
              <LensSpacer />
            ))}
          {/* `flex-1` restores what the bare `<pre>` had as a direct cell child:
              it fills the rest of the column instead of shrinking to its own text,
              which is what keeps the scroll edge of a tall structure lined up with
              every other row's. */}
          <IxLabelsDisplay
            labels={labels}
            empty="—"
            copyJson
            maxHeight="4.5rem"
            className="flex-1"
          />
        </span>
      );
    },
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
      tooltip: onLensWallet
        ? 'Click the target to wash every candle this wallet traded in. View-only — ' +
          'nothing is saved, and it clears with the token.'
        : undefined,
      render: (t) => (
        <span className="flex items-start gap-1">
          {onLensWallet &&
            (t.wallet_address ? (
              <LensButton
                armed={lensWallet === t.wallet_address}
                color={CHART_COLORS.lensWallet}
                title={
                  lensWallet === t.wallet_address
                    ? 'Stop highlighting this wallet'
                    : 'Highlight every candle and row this wallet traded in'
                }
                onClick={() => onLensWallet(t.wallet_address)}
              />
            ) : (
              <LensSpacer />
            ))}
          <AddressDisplay address={t.wallet_address} kind="account" />
        </span>
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
