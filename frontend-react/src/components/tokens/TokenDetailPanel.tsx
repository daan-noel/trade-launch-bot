import { useState, type ReactNode } from 'react';
import { cn } from '../../lib/cn';
import { usePriceDisplay } from '../../hooks/usePriceDisplay';
import type { TokenDetailRecord } from '../../types';
import { useTimezone } from '../../context/TimezoneContext';
import { formatIso } from '../../utils/date';
import {
  formatDecimal,
  formatDecimalTrim,
  formatPrice,
  ratioVariant,
  truncate,
} from '../../utils/format';
import { AddrCard, StatCard } from '../ui/StatCard';
import { Badge } from '../ui/Badge';

function CopyIcon({ copied }: { copied: boolean }) {
  if (copied) {
    return (
      <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
        <path
          d="m5 10.5 3.5 3.5L15 7"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
    );
  }
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      <rect
        x="6.5"
        y="6.5"
        width="9"
        height="9"
        rx="1.5"
        stroke="currentColor"
        strokeWidth="1.5"
      />
      <path
        d="M5.5 13.5h-1a1.5 1.5 0 0 1-1.5-1.5v-8a1.5 1.5 0 0 1 1.5-1.5h8a1.5 1.5 0 0 1 1.5 1.5v1"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
    </svg>
  );
}

function parseInstructionItems(instr: unknown): string[] {
  const valueToString = (v: unknown) => (typeof v === 'string' ? v : JSON.stringify(v));

  if (Array.isArray(instr)) {
    return instr.map(valueToString);
  }
  if (instr && typeof instr === 'object' && 'instructions' in instr) {
    const arr = (instr as { instructions?: unknown[] }).instructions;
    if (Array.isArray(arr)) return arr.map(valueToString);
  }
  return [];
}

function buildInstructionList(
  instr: unknown,
  onCopyItem: (text: string, index: number) => void,
  copiedIndex: number | null,
): ReactNode {
  const items = parseInstructionItems(instr);

  if (items.length === 0) {
    return (
      <pre className="max-h-[260px] overflow-auto rounded-lg border border-white/3 bg-black/12 p-3 text-xs leading-snug text-text whitespace-pre-wrap break-words">
        {JSON.stringify(instr, null, 2)}
      </pre>
    );
  }

  return (
    <div className="flex max-h-[240px] flex-col gap-0.5 overflow-y-auto">
      {items.map((text, i) => (
        <button
          key={i}
          type="button"
          onClick={() => onCopyItem(text, i)}
          title="Click to copy"
          className={cn(
            'rounded border-l-2 px-2 py-1 text-left text-[10px] leading-snug text-text whitespace-pre-wrap break-words transition',
            copiedIndex === i
              ? 'border-primary/40 bg-primary/10'
              : 'border-white/8 bg-white/2 hover:border-white/15 hover:bg-white/4',
          )}
        >
          {text}
        </button>
      ))}
    </div>
  );
}

interface TokenDetailPanelProps {
  detail: TokenDetailRecord | null;
  loading: boolean;
  error: string | null;
}

export function TokenDetailPanel({ detail, loading, error }: TokenDetailPanelProps) {
  const price = usePriceDisplay();
  const { timezone } = useTimezone();
  const [copiedAll, setCopiedAll] = useState(false);
  const [copiedItemIndex, setCopiedItemIndex] = useState<number | null>(null);

  if (loading) {
    return <div className="py-5 text-center text-xs text-text-dim">Loading details…</div>;
  }
  if (error) {
    return <p className="p-3 text-xs text-red">{error}</p>;
  }
  if (!detail) {
    return (
      <div className="py-5 text-center text-xs text-text-dim">
        Select a row to load detailed info.
      </div>
    );
  }

  const entry =
    detail.initial_buy_sol != null &&
    detail.initial_supply_token != null &&
    detail.initial_supply_token > 0
      ? detail.initial_buy_sol / detail.initial_supply_token
      : null;
  const athMult =
    entry && detail.ath_price && entry !== 0 ? detail.ath_price / entry : null;
  const curMult =
    entry && detail.current_price && entry !== 0 ? detail.current_price / entry : null;

  const copyAllInstructions = async () => {
    try {
      await navigator.clipboard.writeText(JSON.stringify(detail.instruction_labels, null, 2));
      setCopiedAll(true);
      setCopiedItemIndex(null);
      setTimeout(() => setCopiedAll(false), 1500);
    } catch {
      /* ignore */
    }
  };

  const copyInstructionItem = async (text: string, index: number) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedItemIndex(index);
      setCopiedAll(false);
      setTimeout(() => setCopiedItemIndex(null), 1500);
    } catch {
      /* ignore */
    }
  };

  const labelCount = Array.isArray(detail.instruction_labels)
    ? detail.instruction_labels.length
    : ((detail.instruction_labels as { instructions?: unknown[] })?.instructions?.length ?? 0);

  const symbol = detail.symbol || truncate(detail.mint_address, 8);

  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center justify-between border-b border-white/7 pb-2">
        <div className="flex items-baseline gap-2.5">
          <span className="text-[15px] font-extrabold tracking-wide">{symbol}</span>
          <span className="text-xs text-text-mid">{detail.name}</span>
        </div>
        <Badge variant={detail.is_migrated ? 'primary' : 'info'}>
          {detail.is_migrated ? 'Migrated ✓' : 'Bonding Curve'}
        </Badge>
      </div>

      <div className="flex items-start gap-0">
        <div className="grid min-w-0 flex-1 grid-cols-2 gap-2">
          <div>
            <div className="mb-1 text-[9px] font-bold uppercase tracking-widest text-text-dim">
              Price Performance
            </div>
            <div className="grid grid-cols-3 gap-1">
              <StatCard label="First Entry Price" value={entry != null ? formatPrice(entry) : '-'} large />
              <StatCard label="ATH Price" value={detail.ath_price != null ? price.displayPrice(detail.ath_price) : '-'} variant="primary" large />
              <StatCard label="Current Price" value={detail.current_price != null ? price.displayPrice(detail.current_price) : '-'} large />
              <StatCard
                label="ATH / FEP"
                value={
                  athMult != null
                    ? `${formatDecimalTrim(athMult, 2)}x  (${formatDecimal((athMult ?? 0) * 100, 1)}%)`
                    : '-'
                }
                variant={ratioVariant(athMult)}
                large
              />
              <StatCard
                label="Current / FEP"
                value={
                  curMult != null
                    ? `${formatDecimalTrim(curMult, 2)}x  (${formatDecimal((curMult ?? 0) * 100, 1)}%)`
                    : '-'
                }
                variant={ratioVariant(curMult)}
                large
              />
              <StatCard label="ATH Timestamp" value={detail.ath_timestamp ? formatIso(detail.ath_timestamp, timezone) : '-'} variant="muted" />
            </div>
          </div>

          <div>
            <div className="mb-1 text-[9px] font-bold uppercase tracking-widest text-text-dim">
              Activity & Market
            </div>
            <div className="grid grid-cols-3 gap-1">
              <StatCard label={`Volume (${price.unitLabel})`} value={detail.volume_sol_total != null ? price.displayCompact(detail.volume_sol_total, 4) : '-'} variant="info" large />
              <StatCard label={`Market Cap (${price.unitLabel})`} value={detail.market_cap != null ? price.displayCompact(detail.market_cap, 4) : '-'} large />
              <StatCard label="Trade Count" value={detail.trade_count != null ? String(detail.trade_count) : '-'} large />
              <StatCard label="Unique Wallets" value={detail.unique_wallets != null ? String(detail.unique_wallets) : '-'} variant="info" large />
              <StatCard label="Last Trade" value={detail.last_trade_at ? formatIso(detail.last_trade_at, timezone) : '-'} variant="muted" />
              <StatCard label="Created" value={formatIso(detail.created_at, timezone)} variant="muted" />
            </div>
          </div>

          <div className="col-span-2">
            <div className="mb-1 text-[9px] font-bold uppercase tracking-widest text-text-dim">
              Creation Parameters
            </div>
            <div className="grid grid-cols-4 gap-1">
              <StatCard label={`Initial Buy (${price.unitLabel})`} value={detail.initial_buy_sol != null ? price.displayAmount(detail.initial_buy_sol) : '-'} />
              <StatCard label="Initial Supply" value={detail.initial_supply_token != null ? String(detail.initial_supply_token) : '-'} />
              <StatCard label="CU Limit" value={detail.cu_limit != null ? String(detail.cu_limit) : '-'} variant="muted" bold />
              <StatCard label="CU Price" value={detail.cu_price != null ? String(detail.cu_price) : '-'} variant="muted" bold />
            </div>
          </div>

          <div className="col-span-2">
            <div className="mb-1 text-[9px] font-bold uppercase tracking-widest text-text-dim">
              Addresses
            </div>
            <div className="grid grid-cols-4 gap-1">
              <AddrCard
                label="Creator"
                full={detail.creator_address}
                kind="account"
              />
              <AddrCard
                label="Mint"
                full={detail.mint_address}
                kind="token"
              />
              <AddrCard
                label="Create TX"
                full={detail.create_tx_address}
                kind="transaction"
              />
              {detail.bonding_curve_address ? (
                <AddrCard
                  label="Bonding Curve"
                  full={detail.bonding_curve_address}
                  kind="account"
                />
              ) : (
                <StatCard label="Bonding Curve" value="-" variant="muted" />
              )}
            </div>
          </div>
        </div>

        <div className="mx-2.5 w-px shrink-0 self-stretch bg-white/7" />

        <div className="flex w-[300px] shrink-0 flex-col gap-1 overflow-y-auto max-h-[300px]">
          <div className="flex items-center justify-between text-[9px] font-bold uppercase tracking-widest text-text-dim">
            <span>Instruction Labels ({labelCount})</span>
            <button
              type="button"
              onClick={copyAllInstructions}
              className={cn(
                'inline-flex size-7 shrink-0 items-center justify-center rounded border border-white/12 bg-white/4 text-text transition hover:bg-white/8',
                copiedAll && 'border-primary/30 bg-primary/12 text-primary',
              )}
              title={copiedAll ? 'Copied!' : 'Copy all instructions'}
              aria-label={copiedAll ? 'Copied' : 'Copy all instructions'}
            >
              <CopyIcon copied={copiedAll} />
            </button>
          </div>
          {buildInstructionList(detail.instruction_labels, copyInstructionItem, copiedItemIndex)}
        </div>
      </div>
    </section>
  );
}
