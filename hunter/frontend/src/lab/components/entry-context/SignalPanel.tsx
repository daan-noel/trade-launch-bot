import type { ColumnDef } from 'components/table/types';
import { DateCell } from 'components/table/DateCell';
import { Input } from 'components/ui/Input';
import { Switch } from 'components/ui/Switch';
import { cn } from 'lib/cn';
import { FIELD_LABEL } from '@lab/components/analysis/TraderQueryInputs';
import { entryKey } from '@lab/lib/entryContext/types';
import type { EntryRow, SlippageReading } from '@lab/lib/entryContext/types';
import {
  formatQuote,
  formatSlip,
  isDefinite,
  signalCounts,
  slippageText,
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
  slotsBefore: number;
  slackLamports: number;
  /** Off shows the derived options as text. On is the comma-separated box. */
  slippageOn: boolean;
  /** The box, for example `10, 20, 30`. Sent only while `slippageOn`. */
  slippage: string;
  onSlotsBefore: (n: number) => void;
  onSlack: (n: number) => void;
  onSlippageOn: (on: boolean) => void;
  onSlippage: (raw: string) => void;
}

const NUM = 'w-[72px] font-normal normal-case tracking-normal';

/**
 * Reserve match. It names the transaction. A count click keeps only those buys.
 */
export function SignalPanel(p: SignalPanelProps) {
  const c = signalCounts(p.entries);
  const pick = (next: SignalFocus) => p.onFocus(p.focus === next ? 'all' : next);
  const n = (v: number) => (p.ready ? v.toLocaleString() : '—');

  return (
    <section className="mb-3">
      <h3 className="mb-1.5 text-[11px] font-bold uppercase tracking-wider text-text-dim">Signal</h3>
      <div>
        <article className="rounded-md border border-info/30 bg-info/5 p-3">
          <header className="mb-2">
            <div className="text-[11px] font-bold uppercase tracking-wider text-info">Reserve match</div>
            <div className="text-[11px] text-text-dim">names the transaction</div>
          </header>
          <div className="mb-3 flex flex-wrap items-end gap-3">
            <label className={FIELD_LABEL} title="Slots before his buy.">
              Slots before
              <Input
                type="number"
                min={0}
                max={50}
                value={p.slotsBefore}
                onChange={(e) => p.onSlotsBefore(Number(e.target.value))}
                className={NUM}
              />
            </label>
            <label
              className={FIELD_LABEL}
              title="A candidate quote may differ from the ceiling's curve SOL by this many lamports and still match."
            >
              Slack, lamports
              <Input
                type="number"
                min={0}
                value={p.slackLamports}
                onChange={(e) => p.onSlack(Number(e.target.value))}
                className="w-[88px] font-normal normal-case tracking-normal"
              />
            </label>
            <div>
              <div className="flex items-center gap-2">
                <span className={FIELD_LABEL}>Slippage</span>
                <Switch checked={p.slippageOn} onChange={p.onSlippageOn} label="Edit slippage" />
              </div>
              {p.slippageOn ? (
                <Input
                  value={p.slippage}
                  placeholder="10, 20, 30"
                  title="Percents, comma-separated. These are the options the match tries."
                  onChange={(e) => p.onSlippage(e.target.value)}
                  className="mt-1 w-[140px] font-normal normal-case tracking-normal"
                />
              ) : (
                <div className="mt-1 text-sm font-normal normal-case tracking-normal text-text">
                  {slippageText(p.readings) || (p.ready ? 'no definite entry' : '—')}
                </div>
              )}
            </div>
          </div>
          <div className="flex flex-wrap gap-2">
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
      </div>
      {p.focus !== 'all' && (
        <p className="mt-1 text-[11px] text-text-dim">Showing these buys. Click the count again for every buy.</p>
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
      className={cn('min-w-[108px] rounded-md border px-3 py-2 text-left', face)}
    >
      <div className="text-lg font-bold leading-none">{tone === 'gold' ? `★ ${value}` : value}</div>
      <div className="mt-1 text-[10px] font-bold uppercase tracking-wider">{label}</div>
      <div className="text-[10px] opacity-80">{detail}</div>
    </button>
  );
}

/** One buy's reserve outcome. Definite is the only filled gold. */
export function ReserveCell({ entry }: { entry: EntryRow }) {
  const call = entry.reserve_call;
  if (!call) {
    return <span className="text-text-dim">{entry.reserve ? 'reserve' : '—'}</span>;
  }
  if (call.kind === 'definite') {
    return (
      <div className="leading-tight">
        <div className="font-bold text-warning">★ DEFINITE · {formatSlip(call.slippage_pct)}%</div>
        <div className="text-[10px] text-text-dim">
          {call.quiet_secs != null ? `quiet ${call.quiet_secs}s` : 'quiet'} · one print before him
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
        <div className="text-[10px]">nothing within the slack</div>
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
      label: 'Reserve match',
      group: 'buy',
      tooltip:
        'Reserve match names the transaction he priced.\n' +
        'DEFINITE = one print before him, quiet for 5 seconds, slippage read from his ceiling.\n' +
        'CROWDED = several prints, and one quote matches his ceiling, or one floor matches his min_tokens_out.\n' +
        'A floor buy is BuyExactSolIn or an exact-quote buy. Its slippage is not a ceiling slippage.\n' +
        'Blank = crowded with no stored bound, nothing within the slack, or several prints matched.',
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
      label: 'Reserve match',
      group: 'reserve',
      tooltip: 'This token\'s buys, by reserve match. ★ is a definite entry.',
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
