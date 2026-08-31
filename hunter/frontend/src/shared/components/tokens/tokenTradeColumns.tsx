import type { ColumnDef } from 'components/table/types';
import type { FlowReason } from 'lib/flow/classifyFlow';
import type { TradeRecord } from 'types';
import { DateCell } from 'components/table/DateCell';
import { formatDecimal, formatWithCommas } from 'utils/format';
import { AmountCell, FeeCell, PriceCell } from 'components/tokens/priceCells';
import { cn } from 'lib/cn';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { Badge } from 'components/ui/Badge';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { formatIxLabelsText } from 'lib/ixLabels';
import { tradePriorityLamports, tradePrioritySol, tradeTipSol } from 'lib/tradeFees';
import { patternKey } from 'lib/flow/volumePatterns';
import { templateGrain } from 'lib/strategy/templateGrain';
import {
  anyRowMatchesTrade,
  feeFromTrade,
  feeMaskActive,
  formatFeePins,
  patternRowKey,
  rowFromTrade,
  type IxPatternFee,
  type IxPatternFeeMask,
  type IxPatternRow,
} from 'lib/strategy/ixPatternRows';
// Deep import: `constants` is type-only w.r.t. lightweight-charts, so the wash
// colors come along without dragging the charting library into this chunk.
import { CHART_COLORS } from 'components/token-price-chart/constants';

export interface TokenTradeColumnsOpts {
  /**
   * `ix_patterns` keys (`JSON.stringify(labels)`) to test each row's
   * structure against. When non-empty, prepends the Tagged/Untagged badge column.
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
   *
   * The optional `fee` is the pin copied from this tx under the strip's fee-field
   * checkboxes. Omitted / empty ⇒ an ix-only row, which is the default.
   */
  onTogglePattern?: ((labels: readonly string[], fee?: IxPatternFee) => void) | null;
  /**
   * Stored rows a click writes. Pressed / Tagged follow engine matching: an
   * unpinned row is a fee wildcard (this tx stays selected whether or not it
   * carries a budget, and whether or not the pin strip is on). A pin-only list
   * lights only the trades that satisfy that pin. The click still toggles the
   * exact row the mask would write.
   */
  patternRows?: readonly IxPatternRow[] | null;
  /** Sticky fee-field modifiers — which of this tx's budget fields a click copies. */
  feePinMask?: IxPatternFeeMask | null;
  /** Name of the fingerprint {@link onTogglePattern} writes to — named in the
   *  badge tooltip, since the click is an immediate save and not a local edit. */
  toggleTargetName?: string | null;
  /**
   * Effective (contagion-aware) classification per trade id — what the chart's
   * lines actually did with the row. The badge tests structure alone, so without
   * this a row reading "Untagged" whose SOL sits on the tagged line looks like a bug.
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
  /**
   * WHICH list {@link flowPatternKeys} is and {@link onTogglePattern} writes into.
   * `'tagged'` (the default) is `m_flow_ix.ix_patterns`; `'dump'` is
   * `m_dump_ix.ix_patterns`; `'working'` is `m_burst_slot.working_templates`
   * grain ids (a different vocabulary — membership is `templateGrain`, not an
   * exact `ix_labels` sequence).
   *
   * The tagged and dump columns are otherwise identical and that is the danger: a
   * badge reading "Tagged" while the click files the build under `m_dump_ix` names
   * the wrong metric for the row, so the label, the tone and the tooltip all follow
   * this. Contagion notes are suppressed under `'dump'` and `'working'` - the
   * reasons map is the flow split's verdict.
   */
  patternList?: 'tagged' | 'dump' | 'working';
  /**
   * Keys of the list this column is NOT writing into. A build may sit on BOTH -
   * that is the normal case and nothing rejects it - so the mark is INFORMATION,
   * not a conflict: it says this sell is already counted by the other group's
   * metrics, which is what a reader comparing `tagged_sell` and `dump_sell` needs
   * to know before treating them as disjoint.
   */
  otherListKeys?: ReadonlySet<string> | null;
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

/** Why the chart counted a row as tagged when its own structure didn't. */
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
  const list = opts?.patternList ?? 'tagged';
  const isDump = list === 'dump';
  const isWorking = list === 'working';
  const otherKeys = opts?.otherListKeys ?? null;
  const pinMask = opts?.feePinMask ?? null;
  const patternRows = isWorking ? null : (opts?.patternRows ?? null);
  const pinning = !isWorking && feeMaskActive(pinMask);
  const listField = isWorking
    ? 'm_burst_slot.working_templates'
    : isDump
      ? 'm_dump_ix.ix_patterns'
      : 'm_flow_ix.ix_patterns';
  const inWord = isWorking ? 'Working' : isDump ? 'Dump' : 'Tagged';
  const outWord = isWorking ? 'Other' : isDump ? 'Not dump' : 'Untagged';

  const leading: ColumnDef<TradeRecord>[] = [];

  if (showTagged) {
    leading.push({
      key: 'is_tagged_ix_pattern',
      label: inWord,
      tooltip: onToggle
        ? isWorking
          ? `Template grain match against ${listField}. Clicking SAVES this trade's grain ` +
            `(program|CU|ATA|N|S|F) to ${targetLabel} — harvest working list, not a full ` +
            `ix_labels sequence. Active rules bound to it change meaning on the next reload.`
          : `Structural ${listField} match. Clicking SAVES this trade’s ordered ` +
            `instruction_labels to ${targetLabel} under ${listField} — there is no staging ` +
            `step, and every active rule bound to it changes meaning from the ` +
            `engine’s next rules reload on.` +
            (isDump
              ? ` The same build may also sit under m_flow_ix - the two groups ask` +
                ` different questions, so a sell can be tagged flow AND a dump.`
              : ` “via creator/wallet” = the lines already count this row through` +
                ` contagion, whatever its own structure is.`)
        : isWorking
          ? `Template grain on ${listField} — this trade's program|CU|ATA|N|S|F grain.`
          : `Structural ${listField} match — this trade’s ordered instruction_labels ` +
            `match a row of that list (an ix-only row is a fee wildcard)` +
            (isDump ? '.' : ' (no creator/wallet contagion).'),
      render: (t) => {
        const labels = t.instruction_labels;
        if (!labels || labels.length === 0) {
          return <span className="text-text-dim/40">—</span>;
        }
        const isTagged = isWorking
          ? keys.has(templateGrain(labels))
          : patternRows != null
            ? anyRowMatchesTrade(patternRows, labels, t)
            : isIxPattern(labels, keys);
        const clickRow = pinning ? rowFromTrade(labels, t, pinMask) : { labels: [...labels] };
        const clickFee = feeFromTrade(t, pinMask);
        const exactClickSaved =
          patternRows != null &&
          patternRows.some((r) => patternRowKey(r) === patternRowKey(clickRow));
        const pinNote = pinning ? formatFeePins(clickFee) : '';
        // The reasons map is the FLOW split's verdict (structure + contagion), so it
        // says nothing about a dump build or a working grain and must not decorate either.
        const reason = isDump || isWorking ? null : (reasons?.get(t.id) ?? null);
        const note = reason && reason !== 'structural' ? CONTAGION_NOTE[reason] : null;
        const inOther =
          !isTagged && !isWorking && otherKeys != null && isIxPattern(labels, otherKeys);
        const badge = (
          <Badge
            variant={isTagged ? (isWorking ? 'success' : isDump ? 'warning' : 'danger') : 'neutral'}
            size="sm"
            className={onToggle ? 'cursor-pointer' : undefined}
          >
            {isTagged ? inWord : outWord}
          </Badge>
        );
        const pinClickHint = pinning
          ? pinNote
            ? ` this structure + ${pinNote}`
            : ' this structure only (this tx has none of the checked fee fields)'
          : ' this structure';
        const clickTitle = exactClickSaved
          ? `Saved under ${listField} on ${targetLabel} — click to remove${pinClickHint}`
          : isTagged && pinning
            ? `Covered by the ix-only row (any budget) on ${targetLabel}. Click to also save${pinClickHint}`
            : isWorking
              ? `Click to save this grain under ${listField} on ${targetLabel}`
              : `Click to save${pinClickHint} under ${listField} on ${targetLabel}`;
        const cell = (
          <span className="inline-flex items-center gap-1">
            {onToggle ? (
              <button
                type="button"
                aria-pressed={isTagged}
                title={clickTitle}
                onClick={(e) => {
                  // The row itself is selectable on several hosts; an edit click
                  // must not also change the table's selection.
                  e.stopPropagation();
                  onToggle(labels, pinning ? clickFee : undefined);
                }}
                className="rounded-md focus:outline-none focus-visible:ring-1 focus-visible:ring-primary"
              >
                {badge}
              </button>
            ) : (
              badge
            )}
            {pinning && exactClickSaved && pinNote && (
              <span className="font-mono text-[9px] text-accent" title={`this exact pin is saved: ${pinNote}`}>
                {pinNote}
              </span>
            )}
            {note && (
              <span className="text-[9px] uppercase tracking-wide text-text-dim/70">
                {note}
              </span>
            )}
            {inOther && (
              <span
                className="text-[9px] uppercase tracking-wide text-text-dim/70"
                title={`This build is also in the ${isDump ? 'tagged' : 'dump'} list, which is allowed. The same sell is counted by ${isDump ? 'm_flow_ix' : 'm_dump_ix'} too - two independent answers, so do not read the two groups' numbers as parts of a whole.`}
              >
                also {isDump ? 'tagged' : 'dump'}
              </span>
            )}
          </span>
        );
        return cell;
      },
      // Structure outranks contagion: sorting this column is for finding the rows
      // whose pattern you can actually toggle.
      sortValue: (t) => {
        const labels = t.instruction_labels;
        const structural = isWorking
          ? !!labels && keys.has(templateGrain(labels))
          : patternRows != null
            ? !!labels && anyRowMatchesTrade(patternRows, labels, t)
            : isIxPattern(labels, keys);
        return structural ? 2 : !isDump && reasons?.get(t.id) ? 1 : 0;
      },
      searchValue: (t) => {
        const labels = t.instruction_labels;
        const structural = isWorking
          ? !!labels && keys.has(templateGrain(labels))
          : patternRows != null
            ? !!labels && anyRowMatchesTrade(patternRows, labels, t)
            : isIxPattern(labels, keys);
        const reason = isDump ? null : (reasons?.get(t.id) ?? null);
        const note = reason && reason !== 'structural' ? CONTAGION_NOTE[reason] : '';
        const word = structural ? list : isDump ? 'not dump' : 'untagged';
        return `${word}${note ? ` ${note}` : ''}`;
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
          'View-only: unlike the Tagged badge, it saves nothing and no rule reads it.'
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
      key: 'priority',
      label: 'Priority',
      tooltip:
        'What the sender spent to land EARLY, across both rails: the compute rail ' +
        '(CU limit × CU price ÷ 1e6) plus any tip. This is the comparable number — ' +
        'the raw parts are not, because CU price is charged per compute unit, so the ' +
        'same spend at half the limit reads as double the price. “—” = neither rail ' +
        'captured (trades ingested before the columns existed; unbackfillable).',
      render: (t) => (
        <span className="text-text-dim">
          <FeeCell sol={tradePrioritySol(t)} />
        </span>
      ),
      // Same convention as Fee: unknown sorts below every real spend rather than
      // tying with a genuine zero.
      sortValue: (t) => tradePrioritySol(t) ?? -1,
      searchValue: (t) => {
        const v = tradePriorityLamports(t);
        return v != null ? String(v) : '';
      },
      filterNumber: (t) => tradePrioritySol(t),
    },
    {
      key: 'tip',
      label: 'Tip',
      tooltip:
        'Lamports transferred to a known tip account (Jito block engine, Helius ' +
        'Sender) — the priority rail the Fee column structurally cannot see, because ' +
        'a tip is a transfer instruction, not a fee. Paid ONCE per transaction even ' +
        'when it sells four wallets’ bags. “—” = the tx carries no top-level ' +
        'transfer; “◎0” = it carries one but none reached a recognised tip account ' +
        '(a router paying its own rake, or a tip rail the decoder does not know yet).',
      render: (t) => (
        <span className="text-text-dim">
          <FeeCell sol={tradeTipSol(t)} />
        </span>
      ),
      // `?? -1` for unknown only — a real 0 keeps its own rank, because "transfers,
      // none to a tip account" is a reading and belongs beside the other readings.
      sortValue: (t) => tradeTipSol(t) ?? -1,
      searchValue: (t) => (t.tip_lamports != null ? String(t.tip_lamports) : ''),
      filterNumber: (t) => tradeTipSol(t),
    },
    {
      key: 'cu_limit',
      label: 'CU Limit',
      tooltip:
        'Compute units this transaction requested (SetComputeUnitLimit). “—” = it ' +
        'set none and took the runtime default. Heavily modal — 300k / 400k / 500k ' +
        'are hardcoded client presets — with a long simulation-derived tail, which ' +
        'makes it a property of the sender’s SOFTWARE rather than of the moment.',
      render: (t) => (
        <span className="text-text-dim tabular-nums">
          {t.cu_limit != null ? formatWithCommas(t.cu_limit) : '—'}
        </span>
      ),
      sortValue: (t) => t.cu_limit ?? -1,
      searchValue: (t) => (t.cu_limit != null ? String(t.cu_limit) : ''),
      filterNumber: (t) => t.cu_limit ?? null,
    },
    {
      key: 'cu_price',
      label: 'CU Price',
      tooltip:
        'SetComputeUnitPrice, in MICRO-LAMPORTS PER COMPUTE UNIT — not a lamport ' +
        'amount, and not a number anyone picks directly: 3,333,333 is what “0.001 ' +
        'SOL at a 300k limit” looks like from this side. Rank and compare on the ' +
        'Priority column instead; this one is here to explain it, not to sort by. ' +
        '“—” = no price set, i.e. no compute-rail priority fee at all.',
      render: (t) => (
        <span className="text-text-dim tabular-nums">
          {t.cu_price != null ? formatWithCommas(t.cu_price) : '—'}
        </span>
      ),
      sortValue: (t) => t.cu_price ?? -1,
      searchValue: (t) => (t.cu_price != null ? String(t.cu_price) : ''),
      filterNumber: (t) => t.cu_price ?? null,
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
