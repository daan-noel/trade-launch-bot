import type { ColumnDef } from 'components/table/types';
import { DateCell } from 'components/table/DateCell';
import { Input } from 'components/ui/Input';
import { Switch } from 'components/ui/Switch';
import { cn } from 'lib/cn';
import { entryKey } from '@lab/lib/entryContext/types';
import type { EntryRow, SlippageReading } from '@lab/lib/entryContext/types';
import {
  formatQuote,
  formatSlip,
  isDefinite,
  signalCounts,
  slippageOptions,
  type SignalFocus,
} from '@lab/lib/entryContext/signal';
import type { TraderTokenRow } from 'types';

export interface SignalPanelProps {
  entries: readonly EntryRow[];
  /** Counts stay blank until a read has landed. */
  ready: boolean;
  loading: boolean;
  truncated: boolean;
  readings: readonly SlippageReading[];
  focus: SignalFocus;
  onFocus: (next: SignalFocus) => void;
  /** A derived slippage to keep. `null` shows every buy. */
  slipPick: number | null;
  onSlipPick: (pct: number | null) => void;
  slotsBefore: number;
  /** Off shows each derived slippage as a button. On is the comma-separated box. */
  slippageOn: boolean;
  /** The box, for example `10, 20, 30`. Sent only while `slippageOn`. */
  slippage: string;
  onSlotsBefore: (n: number) => void;
  onSlippageOn: (on: boolean) => void;
  onSlippage: (raw: string) => void;
}

const KNOB = 'text-[9px] font-bold uppercase leading-none tracking-widest text-text-dim';
const FIELD = 'h-7 w-16 py-0 font-mono';

/**
 * Reserve match. It names the signal tx he priced. A count click keeps only those buys.
 */
export function SignalPanel(p: SignalPanelProps) {
  const c = signalCounts(p.entries);
  const pick = (next: SignalFocus) => p.onFocus(p.focus === next ? 'all' : next);
  const n = (v: number) => (p.ready ? v.toLocaleString() : '—');
  const slips = slippageOptions(p.readings);
  const slipOn = (pct: number) => p.slipPick != null && formatSlip(p.slipPick) === formatSlip(pct);

  return (
    <section className="@container min-w-0">
      <h3 className="mb-1.5 text-[11px] font-bold uppercase tracking-wider text-text-dim">
        Reserve match - signal tx
      </h3>
      <article className="rounded-md border border-info/30 bg-info/5 px-3 py-2">
        {/* One line when this section has the full page. In the side-by-side
            column the counts drop under the knobs so they stay inside the card. */}
        <div className="flex flex-col gap-2 @5xl:flex-row @5xl:flex-wrap @5xl:items-center @5xl:justify-between">
          <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1.5">
            <label
              className="inline-flex items-center gap-1.5"
              title="Slots before his buy. Reserve match reads this window for the signal tx."
            >
              <span className={KNOB}>Slots before</span>
              <Input
                type="number"
                min={0}
                max={50}
                value={p.slotsBefore}
                onChange={(e) => p.onSlotsBefore(Number(e.target.value))}
                aria-label="Slots before"
                className={FIELD}
              />
            </label>
            <span className="inline-flex items-center gap-1.5">
              <span className={KNOB}>Slippage</span>
              <Switch checked={p.slippageOn} onChange={p.onSlippageOn} label="Edit slippage" />
            </span>
            {p.slippageOn ? (
              <Input
                value={p.slippage}
                placeholder="10, 20, 30"
                title="Percents, comma-separated. These are the options the match tries."
                aria-label="Slippage percents"
                onChange={(e) => p.onSlippage(e.target.value)}
                className="h-7 w-36 py-0 font-normal normal-case tracking-normal"
              />
            ) : slips.length > 0 ? (
              <span className="inline-flex flex-wrap items-center gap-1">
                {slips.map((pct) => {
                  const on = slipOn(pct);
                  const label = `${formatSlip(pct)}%`;
                  return (
                    <button
                      key={label}
                      type="button"
                      onClick={() => p.onSlipPick(on ? null : pct)}
                      title={
                        on
                          ? `Showing buys at ${label}. Click again for every slippage.`
                          : `Show only buys whose signal slippage is ${label}.`
                      }
                      className={cn(
                        'inline-flex h-7 items-center rounded border px-2 text-[11px] font-normal leading-none',
                        on
                          ? 'border-info bg-info/25 font-bold text-info'
                          : 'border-white/20 text-text hover:border-info/50',
                      )}
                    >
                      {label}
                    </button>
                  );
                })}
              </span>
            ) : (
              <span className="text-[11px] leading-none text-text-dim">
                {p.ready ? 'no definite entry' : '—'}
              </span>
            )}
          </div>
          <div className="flex flex-wrap items-center gap-1.5">
            <CountTile
              tone="gold"
              active={p.focus === 'definite'}
              onClick={() => pick('definite')}
              value={n(c.definite)}
              label="Definite"
              detail="slippage read here"
            />
            <CountTile
              tone="teal"
              active={p.focus === 'crowded'}
              onClick={() => pick('crowded')}
              value={n(c.crowded)}
              label="Crowded"
              detail="quote or floor"
            />
            <CountTile
              tone="dim"
              active={p.focus === 'blank'}
              onClick={() => pick('blank')}
              value={n(c.noCeiling + c.noMatch + c.several)}
              label="Blank"
              detail={
                p.ready
                  ? `${c.noCeiling} no bound · ${c.noMatch} no match · ${c.several} several`
                  : 'no bound, no match, or several matches'
              }
            />
          </div>
        </div>
          {p.ready && c.single > 0 && (
            <button
              type="button"
              onClick={() => pick('single')}
              className={cn(
                'mt-2 text-[11px] underline-offset-2 hover:underline',
                p.focus === 'single' ? 'font-bold text-text' : 'text-text-dim',
              )}
            >
              {c.single.toLocaleString()} one print, slippage not read
            </button>
          )}
      </article>
      {(p.focus !== 'all' || p.slipPick != null) && (
        <p className="mt-1 text-[11px] text-text-dim">
          {p.slipPick != null ? `Showing ${formatSlip(p.slipPick)}% slippage. ` : 'Showing these buys. '}
          Click the selection again for every buy.
        </p>
      )}
      {(p.loading || p.truncated) && (
        <p className="mt-1 text-[11px] text-text-dim">
          {p.loading ? 'Reading…' : ''}
          {p.truncated ? ' Showing the most recent buys.' : ''}
        </p>
      )}
    </section>
  );
}

function CountTile({
  tone,
  active,
  onClick,
  value,
  label,
  detail,
}: {
  tone: 'gold' | 'teal' | 'dim';
  active: boolean;
  onClick: () => void;
  value: string;
  label: string;
  detail: string;
}) {
  const face = {
    gold: active
      ? 'border-warning bg-warning text-black'
      : 'border-warning/70 bg-warning/15 text-warning',
    teal: active ? 'border-info bg-info/25 text-info' : 'border-info/40 text-info',
    dim: active ? 'border-white/40 bg-white/10 text-text' : 'border-white/15 text-text-dim',
  }[tone];
  return (
    <button
      type="button"
      onClick={onClick}
      title={detail}
      className={cn(
        'inline-flex h-9 shrink-0 items-center gap-2 rounded-md border px-2.5 text-left',
        face,
      )}
    >
      <span className="inline-flex w-12 shrink-0 items-center justify-end gap-0.5 text-sm font-bold leading-none">
        {tone === 'gold' && <span aria-hidden>★</span>}
        <span className="tabular-nums">{value}</span>
      </span>
      <span className="flex flex-col justify-center gap-0.5 leading-none">
        <span className="text-[10px] font-bold uppercase tracking-wider">{label}</span>
        <span className="whitespace-nowrap text-[10px] font-normal normal-case tracking-normal opacity-80">
          {detail}
        </span>
      </span>
    </button>
  );
}

/** One buy's reserve outcome. Definite is the only filled gold. */
export function ReserveCell({ entry }: { entry: EntryRow }) {
  const call = entry.reserve_call;
  if (!call) {
    return <span className="text-text-dim">{entry.reserve ? 'signal tx' : '—'}</span>;
  }
  if (call.kind === 'definite') {
    return (
      <div className="leading-tight">
        <div className="font-bold text-warning">★ DEFINITE · {formatSlip(call.slippage_pct)}%</div>
        <div className="text-[10px] text-text-dim">
          one print in the slots before him
        </div>
        {call.quote_sol != null && (
          <div className="text-[10px] text-text-dim">quote {formatQuote(call.quote_sol)} SOL</div>
        )}
      </div>
    );
  }
  if (call.kind === 'crowded') {
    return (
      <div className="leading-tight">
        <div className="font-bold text-info">
          CROWDED{call.slippage_pct != null ? ` · matched ${formatSlip(call.slippage_pct)}%` : ''}
        </div>
        {call.quote_sol != null && (
          <div className="text-[10px] text-text-dim">quote {formatQuote(call.quote_sol)} SOL</div>
        )}
      </div>
    );
  }
  if (call.kind === 'single') {
    return (
      <div className="leading-tight">
        <div className="font-bold text-text">ONE PRINT</div>
        <div className="text-[10px] text-text-dim">the only print before him · slippage not read</div>
      </div>
    );
  }
  if (call.kind === 'no_ceiling') {
    return (
      <div className="leading-tight text-text-dim">
        <div>— no bound stored</div>
        <div className="text-[10px]">crowded, ceiling or floor missing</div>
      </div>
    );
  }
  if (call.kind === 'no_match') {
    return (
      <div className="leading-tight text-text-dim">
        <div>— no quote or floor matched</div>
        <div className="text-[10px]">no earlier tx equals his bound</div>
      </div>
    );
  }
  if (call.kind === 'several') {
    return (
      <div className="leading-tight text-text-dim">
        <div>— several matched</div>
        <div className="text-[10px]">more than one quote or floor</div>
      </div>
    );
  }
  return (
    <div className="leading-tight text-text-dim">
      <div>—</div>
      <div className="text-[10px]">no print in the slots before him</div>
    </div>
  );
}

const RESERVE_RANK: Record<string, number> = {
  definite: 5,
  crowded: 4,
  single: 3,
  several: 2,
  no_match: 2,
  no_ceiling: 1,
  empty: 0,
};

/** The reserve outcome on an entry row. */
export function signalBuyColumns(): ColumnDef<EntryRow>[] {
  return [
    {
      key: 'reserve_call',
      label: 'Signal tx',
      group: 'buy',
      tooltip:
        'Reserve match names the signal tx he priced.\n' +
        'DEFINITE = one print in the slots before him, slippage read from his bound.\n' +
        'CROWDED = several prints, and one quote equals his ceiling, or one floor equals his min_tokens_out.\n' +
        'A floor buy is BuyExactSolIn or an exact-quote buy. Its slippage is not a ceiling slippage.\n' +
        'Blank = crowded with no stored bound, no earlier tx equals his bound, or several prints matched.',
      render: (e) => <ReserveCell entry={e} />,
      sortValue: (e) => RESERVE_RANK[e.reserve_call?.kind ?? 'empty'] ?? 0,
      searchValue: (e) => e.reserve_call?.kind ?? '',
    },
  ];
}

function reserveRollup(buys: readonly EntryRow[]): string {
  let definite = 0;
  let crowded = 0;
  let blank = 0;
  let single = 0;
  for (const b of buys) {
    const k = b.reserve_call?.kind;
    if (k === 'definite') definite += 1;
    else if (k === 'crowded') crowded += 1;
    else if (k === 'no_ceiling' || k === 'no_match' || k === 'several') blank += 1;
    else if (k === 'single') single += 1;
  }
  const parts: string[] = [];
  if (definite) parts.push(`★ ${definite} definite`);
  if (crowded) parts.push(`${crowded} crowded`);
  if (blank) parts.push(`${blank} blank`);
  if (parts.length) return parts.join(' · ');
  if (single) return `${single} one print`;
  return '—';
}

/**
 * Trader Analysis, per token. The rollup is the token's buys. Opening it lists
 * each buy the way Entry Context does.
 */
export function signalTokenColumns(
  byMint: ReadonlyMap<string, readonly EntryRow[]>,
  open: ReadonlySet<string>,
  toggle: (mint: string) => void,
): ColumnDef<TraderTokenRow>[] {
  const buysOf = (r: TraderTokenRow) => byMint.get(r.mint_address) ?? [];
  return [
    {
      key: 'sig_reserve',
      label: 'Signal tx',
      group: 'reserve',
      tooltip: 'This token\'s buys, by the signal tx reserve match named. ★ is a definite entry.',
      render: (r) => {
        const buys = buysOf(r);
        const opened = open.has(r.mint_address);
        const gold = buys.some(isDefinite);
        return (
          <div className="min-w-[220px]">
            <button
              type="button"
              className={cn('text-left text-[11px] font-bold', gold ? 'text-warning' : 'text-text')}
              onClick={() => toggle(r.mint_address)}
            >
              {reserveRollup(buys)}
              {buys.length > 0 && (
                <span className="ml-1 font-normal text-text-dim">{opened ? '▾' : '▸'} {buys.length}</span>
              )}
            </button>
            {opened && (
              <ul className="mt-1 space-y-1.5 border-t border-white/10 pt-1">
                {buys.map((e) => (
                  <li
                    key={entryKey(e)}
                    className={cn('text-[11px]', isDefinite(e) && 'border-l-2 border-warning pl-1')}
                  >
                    <DateCell iso={e.at} />
                    <ReserveCell entry={e} />
                  </li>
                ))}
              </ul>
            )}
          </div>
        );
      },
      sortValue: (r) => buysOf(r).filter(isDefinite).length,
      searchValue: (r) => reserveRollup(buysOf(r)),
    },
  ];
}

export { isDefinite };
