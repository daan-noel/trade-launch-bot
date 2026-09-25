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
import { tagLabel } from 'lib/flow/tapeClassify';
import { formatFeePins } from 'lib/strategy/ixPatternRows';
import { stageValueOf, stageValueText, type TagStage } from 'hooks/useIxPatternTarget';
// Deep import: `constants` is type-only w.r.t. lightweight-charts, so the wash
// colors come along without dragging the charting library into this chunk.
import { CHART_COLORS } from 'components/token-price-chart/constants';

export interface TokenTradeColumnsOpts {
  /**
   * The tag the chart classifies with (`@name`, no `@`). Set ⇒ the tag column
   * renders: which half each trade landed on, and - with {@link stage} - the click
   * that adds this trade's value to the tag.
   */
  flowTagName?: string | null;
  /**
   * The verdict per trade id over the coin's FULL history (`tradeFlowReasons`): the
   * same pass the chart's lines draw from, so a badge and the line above it cannot
   * disagree. Absent id = the rest (`@!tag`).
   */
  flowReasons?: ReadonlyMap<string, FlowReason> | null;
  /**
   * What a badge click writes (tag, matcher, owner). A fingerprint stage SAVES on
   * click - every active rule bound to that fingerprint reads the new tag from the
   * engine's next rules reload on - so the tooltip names the tag, the matcher and
   * the fingerprint. `null` ⇒ the badge is display only.
   */
  stage?: TagStage | null;
  /** A tag field's title from the registry (`reg.tags.fields`), by key: names the
   *  matcher a click writes and the matcher a trade carries the tag through. */
  tagFieldTitle?: (key: string) => string;
  /**
   * Arms the ephemeral WALLET highlight lens from a row — adds a target button to
   * the Wallet cell. Nothing is persisted: this only washes candles and rows.
   */
  onLensWallet?: ((address: string) => void) | null;
  /** The armed wallet, so its own rows render the button lit. */
  lensWallet?: string | null;
  /**
   * Arms the ephemeral IX-STRUCTURE lens from a row. Deliberately separate from the
   * tag badge, which lives one column over and SAVES to the fingerprint the engine
   * reads - asking "where else did this shape appear" must never change how a live
   * rule classifies.
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

/** What the Wallet column actually holds — stated on the column, because reading it
 *  as "the trader" is the mistake the badge below exists to stop. */
const WALLET_IS_VENUE_CREDIT_TIP =
  'This is who the VENUE credited (TradeEvent.user), not necessarily who signed. An ' +
  'aggregator routes its customers through a PDA of its own, so a PROXY row is one ' +
  'router carrying many unrelated people — see the Payer column for the real sender.';

/**
 * Says whether the Wallet cell names a trader at all.
 *
 * Three states, and the middle one is why this renders nothing rather than a green
 * tick: `true` = a router's proxy PDA (it signed nothing on this transaction),
 * `false` = it signed, `null`/absent = **not captured** — the trade predates
 * migration 0014 and `raw_txs` no longer holds the transaction to answer from. A
 * badge that showed "direct" for an uncaptured row would assert the one thing
 * nobody can know, so only the two known states are drawn.
 *
 * The stakes are per-wallet reads: one router address carries 462,483 rows here,
 * 92.5% of them behind a single address, so counting it as a trader tops every
 * leaderboard with a PDA and deflates unique-wallet breadth on the same tokens.
 */
function ProxyBadge({ isProxied }: { isProxied?: boolean | null }) {
  if (isProxied !== true) return null;
  return (
    <span
      title={
        'PROXY — this address signed nothing on this transaction, so it is a router PDA ' +
        'and NOT the trader. Every customer that router routed collapses onto this one ' +
        'address. The real sender is the Payer column.'
      }
      className="shrink-0 rounded border border-warning/40 bg-warning/12 px-1 py-px text-[9px] font-semibold uppercase leading-tight tracking-wide text-warning"
    >
      proxy
    </span>
  );
}

/** The verdict's half: tagged (a matcher held), excluded (a creation-slot buyer
 *  under `exclude_creation_slot`) or the rest. */
function halfOf(reason: FlowReason | null | undefined): 'tagged' | 'excluded' | 'rest' {
  return reason == null ? 'rest' : reason === 'creation_slot' ? 'excluded' : 'tagged';
}

/** The registry field that names a reason (`creation_slot` is the option's effect). */
const reasonField = (r: FlowReason) => (r === 'creation_slot' ? 'exclude_creation_slot' : r);

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
  const tagName = opts?.flowTagName ?? null;
  const reasons = opts?.flowReasons ?? null;
  const stage = opts?.stage ?? null;
  const title = opts?.tagFieldTitle ?? ((k: string) => k);
  const onLensWallet = opts?.onLensWallet ?? null;
  const lensWallet = opts?.lensWallet ?? null;
  const onLensStructure = opts?.onLensStructure ?? null;
  const lensStructureKey = opts?.lensStructureKey ?? null;

  const leading: ColumnDef<TradeRecord>[] = [];

  if (tagName) {
    const on = tagLabel(tagName);
    const off = tagLabel(tagName, true);
    const writes = stage ? `${tagLabel(stage.tagName)} (${title(stage.matcher)})` : null;
    const owner = stage?.ownerName ? ` on "${stage.ownerName}"` : '';
    leading.push({
      key: 'flow_tag',
      label: on,
      tooltip:
        `Which half of ${on} the chart put this trade on: ${on}, ${off} (the rest) or neither ` +
        `(a creation-slot buyer the tag ignores), classified over the coin's full history; ` +
        `"via" names the matcher that held.` +
        (stage && writes
          ? ` Clicking adds this trade's ${title(stage.matcher)} to ${writes}${owner}, or removes it when listed.`
          : ''),
      render: (t) => {
        const reason = reasons?.get(t.id) ?? null;
        const half = halfOf(reason);
        const badge = (
          <Badge
            variant={half === 'tagged' ? 'danger' : half === 'excluded' ? 'warning' : 'neutral'}
            size="sm"
            className={stage?.toggle ? 'cursor-pointer' : undefined}
          >
            {half === 'tagged' ? on : half === 'excluded' ? 'neither' : off}
          </Badge>
        );
        const note =
          reason && reason !== 'creation_slot' ? `via ${title(reasonField(reason)).toLowerCase()}` : null;
        const value = stage ? stageValueOf(stage.matcher, t, stage.feePins) : null;
        const listed = !!stage && value != null && stage.listed(value);
        const muted = !!stage && value != null && !listed && !!stage.muted?.(value);
        const pins = value?.matcher === 'ix_shape' ? formatFeePins(value.row) : '';
        const clickTitle = !value
          ? ''
          : muted
            ? `In ${writes}${owner} but muted by the lens chips: click to classify with it again.`
            : `${listed ? 'Remove' : 'Add'} ${stageValueText(value)} ${listed ? 'from' : 'to'} ${writes}${owner}.` +
              (stage?.ownerName ? ' Saves now.' : '');
        const toggle = stage?.toggle ?? null;
        return (
          <span className="inline-flex items-center gap-1">
            {toggle && value ? (
              <button
                type="button"
                aria-pressed={listed}
                title={clickTitle}
                onClick={(e) => {
                  // The row itself is selectable on several hosts; an edit click
                  // must not also change the table's selection.
                  e.stopPropagation();
                  toggle(value);
                }}
                // inline-flex, so the hit area is the badge itself — an inline
                // button leaves line-height slack the click falls through.
                className="inline-flex rounded-md focus:outline-none focus-visible:ring-1 focus-visible:ring-primary"
              >
                {badge}
              </button>
            ) : (
              badge
            )}
            {listed && (
              <span className="text-[9px] uppercase tracking-wide text-accent" title={`listed under ${writes}`}>
                listed{pins ? ` ${pins}` : ''}
              </span>
            )}
            {muted && <span className="text-[9px] uppercase tracking-wide text-text-dim/70">muted</span>}
            {note && <span className="text-[9px] uppercase tracking-wide text-text-dim/70">{note}</span>}
          </span>
        );
      },
      sortValue: (t) => {
        const half = halfOf(reasons?.get(t.id));
        return half === 'tagged' ? 2 : half === 'excluded' ? 1 : 0;
      },
      searchValue: (t) => {
        const reason = reasons?.get(t.id) ?? null;
        const half = halfOf(reason);
        const word = half === 'tagged' ? on : half === 'excluded' ? 'neither' : off;
        return reason && reason !== 'creation_slot' ? `${word} via ${reason}` : word;
      },
    });
  }

  leading.push({
    key: 'ix_structure',
    label: 'ix_labels',
    tooltip:
      'Ordered instruction-label structure of this trade - what the exact ix shape matcher reads. ' +
      (onLensStructure
        ? 'Click the target to wash every candle this exact ordered structure appeared in. ' +
          'View-only: unlike the tag badge, it saves nothing and no rule reads it.'
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
      tooltip:
        (onLensWallet
          ? 'Click the target to wash every candle this wallet traded in. View-only — ' +
            'nothing is saved, and it clears with the token.\n\n'
          : '') + WALLET_IS_VENUE_CREDIT_TIP,
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
                    : t.is_proxied === true
                      ? 'Highlight every row credited to this ROUTER PDA — that is every ' +
                        'customer it routed, not one trader'
                      : 'Highlight every candle and row this wallet traded in'
                }
                onClick={() => onLensWallet(t.wallet_address)}
              />
            ) : (
              <LensSpacer />
            ))}
          <AddressDisplay address={t.wallet_address} kind="account" />
          <ProxyBadge isProxied={t.is_proxied} />
        </span>
      ),
      sortValue: (t) => t.wallet_address,
      // The flag is searchable by word, so `proxy` selects the router rows and
      // `direct` the ones that signed — the split every per-wallet read depends on.
      searchValue: (t) =>
        `${t.wallet_address} ${t.is_proxied === true ? 'proxy router' : t.is_proxied === false ? 'direct' : ''}`,
    },
    {
      key: 'payer',
      label: 'Payer',
      tooltip:
        'The transaction fee payer (`account_keys[0]`) — the REAL sender when Wallet is ' +
        'a router proxy, and a candidate otherwise (a bot can pay from one keypair and ' +
        'trade from another).\n\n' +
        'Per-TRANSACTION, so every leg of a multi-leg tx repeats it: collapse by signature ' +
        'before counting payers. Blank on trades ingested before the column existed — ' +
        'unbackfillable, since raw_txs keeps only 3 days.',
      render: (t) =>
        t.payer_address ? (
          <AddressDisplay address={t.payer_address} kind="account" />
        ) : (
          <span
            className="text-text-dim"
            title="Not captured — this trade predates the payer column, and raw_txs no longer holds the transaction"
          >
            —
          </span>
        ),
      sortValue: (t) => t.payer_address ?? '',
      searchValue: (t) => t.payer_address ?? '',
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
