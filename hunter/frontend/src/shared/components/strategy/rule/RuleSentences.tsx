// Signal is its own panel: its own outline and fill, above the panel that holds
// watch, buy, sell, and again. Watch and buy are two rows in a column, split
// by a divider. Buy gates are rows inside a green outline. Sell is the red
// outline beside them. Inside sell, Always is its own frame. A double rule
// separates it from the stages. Stages have a full-width title and no frame.
// Again is
// its own section after sell. A signal holds when
// any group holds. Reads inside one group share the sell if/then bracket.
// The word or sits between groups.

import { cn } from 'lib/cn';
import { metricTone } from 'lib/strategy/metricColors';
import { ROLE, wash } from 'lib/strategy/roleColors';
import { familyName, findFamily, findMetric, type StrategyRegistry } from 'lib/strategy/registry';
import { flowSplit, type FlowBlock, type FlowRow } from 'lib/strategy/ruleFlow';
import type { RuleDoc } from 'lib/strategy/ruleDoc';
import { CaseMark, FamilyMark, StageMark, StepMark } from './MetricMark';

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** Stage names that follow `go`, `go to`, or `→` become outline badges. */
function StageText({ text, stages }: { text: string; stages: readonly string[] }) {
  const names = stages.filter((n) => n.length > 0);
  if (!text || names.length === 0) return <>{text}</>;
  const alt = [...names].sort((a, b) => b.length - a.length).map(escapeRegExp).join('|');
  const re = new RegExp(`(?<=go to |go |→ )(${alt})\\b`, 'g');
  const out: React.ReactNode[] = [];
  let last = 0;
  let n = 0;
  for (const m of text.matchAll(re)) {
    const at = m.index ?? 0;
    if (at > last) out.push(text.slice(last, at));
    if (m[1]) out.push(<StageMark key={n++} name={m[1]} />);
    last = at + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return <>{out}</>;
}

function treeMarks(depths: number[]): { bars: boolean[]; last: boolean; hasChild: boolean }[] {
  return depths.map((depth, i) => {
    const bars: boolean[] = [];
    for (let c = 0; c < depth; c++) {
      let cont = false;
      for (let j = i + 1; j < depths.length; j++) {
        if (depths[j] <= c) break;
        if (depths[j] === c + 1) {
          cont = true;
          break;
        }
      }
      bars.push(cont);
    }
    return {
      bars,
      last: depth === 0 || !bars[depth - 1],
      hasChild: i + 1 < depths.length && depths[i + 1] > depth,
    };
  });
}

/** One tree level. Wide enough that a child is obvious beside its parent. */
const TREE_STEP = 40;

/** v moves one step right of if. The tree line stays on if's column. */
const TUCK = TREE_STEP;

function PairStem({ kind }: { kind: 'open' | 'mid' | 'close' }) {
  return (
    <span className="relative shrink-0" style={{ width: TREE_STEP }}>
      <span
        className={cn(
          'absolute left-1/2 w-px bg-white/25',
          kind === 'open' && 'top-1/2 bottom-0',
          kind === 'mid' && 'inset-y-0',
          kind === 'close' && 'top-0 h-1/2',
        )}
      />
      {kind !== 'mid' && <span className="absolute top-1/2 left-1/2 h-px w-1/2 bg-white/25" />}
    </span>
  );
}

function TreeGutter({ depth, bars, last, plain }: { depth: number; bars: boolean[]; last: boolean; plain?: boolean }) {
  return (
    <span className="flex shrink-0">
      {Array.from({ length: depth }, (_, c) => {
        const elbow = c === depth - 1;
        return (
          <span key={c} className="relative" style={{ width: TREE_STEP }}>
            {elbow ? (
              <>
                <span className={cn('absolute left-1/2 w-px bg-white/25', last ? 'top-0 h-1/2' : 'inset-y-0')} />
                {!plain && <span className="absolute top-1/2 left-1/2 h-px w-1/2 bg-white/25" />}
              </>
            ) : (
              bars[c] && <span className="absolute inset-y-0 left-1/2 w-px bg-white/25" />
            )}
          </span>
        );
      })}
    </span>
  );
}

function Acts({ acts, stages }: { acts: FlowRow['acts']; stages: string[] }) {
  if (!acts?.length) return null;
  return (
    <>
      {acts.map((a, i) => (
        <span key={i} className="inline-flex items-center gap-1.5">
          {a.gate ? <CaseMark name={a.gate} /> : null}
          {a.gate === 'go' && <span className="text-accent">→</span>}
          {a.gate === 'go' && stages.includes(a.text) ? (
            <StageMark name={a.text} />
          ) : a.text ? (
            <span className={a.gate ? undefined : 'text-text-dim'}>{a.text}</span>
          ) : null}
        </span>
      ))}
    </>
  );
}

function FlowRows({ rows, reg, stages, signals }: { rows: FlowRow[]; reg: StrategyRegistry; stages: string[]; signals: Set<string> }) {
  const marks = treeMarks(rows.map((r) => r.depth ?? 0));
  return (
    <div className="flex flex-col font-mono text-[13px] leading-6">
      {rows.map((r, i) => {
        const depth = r.depth ?? 0;
        const mark = marks[i];
        return (
          <div key={i} className={cn('relative flex min-h-7 items-stretch', r.dim && 'text-text-dim')}>
            {mark?.hasChild && (
              <span className="absolute bottom-0 w-px bg-white/25" style={{ left: depth * TREE_STEP + TREE_STEP / 2, top: '50%' }} />
            )}
            <TreeGutter depth={depth} bars={mark?.bars ?? []} last={mark?.last ?? true} plain={r.tuck || r.along} />
            {r.brace ? <PairStem kind={r.brace} /> : null}
            {r.tuck ? <span className="shrink-0" style={{ width: TUCK }} /> : null}
            <span className="flex items-center gap-2 py-0.5 whitespace-nowrap">
              {r.head ? <StepMark name={r.head} /> : null}
              {r.padIf ? (
                <span className="invisible" aria-hidden>
                  <CaseMark name="if" />
                </span>
              ) : null}
              {r.gate ? (
                stages.includes(r.gate) ? <StageMark name={r.gate} /> : <CaseMark name={r.gate} signal={signals.has(r.gate)} />
              ) : null}
              {r.gate === 'go' && <span className="text-accent">→</span>}
              {r.signal ? (
                <span className="inline-flex items-center gap-1">
                  {r.text.startsWith('not ') && <span className="text-text-mid">not</span>}
                  <CaseMark name={r.signal} signal />
                </span>
              ) : r.gate === 'go' && stages.includes(r.text) ? (
                <StageMark name={r.text} />
              ) : (
                (r.text || r.metric) && (
                  <span className="inline-flex items-baseline gap-1 whitespace-nowrap">
                    {r.metric && (
                      <FamilyMark
                        family={familyName(r.metric)}
                        subject={findFamily(reg, familyName(r.metric))?.title}
                        hue={findMetric(reg, r.metric)?.hue}
                      />
                    )}
                    <span
                      className={cn(
                        'whitespace-nowrap',
                        r.dim && 'text-text-dim',
                        !r.dim && !r.metric && r.gate !== 'TP' && r.gate !== 'SL' && 'text-text',
                        r.gate === 'TP' && 'font-semibold text-buy',
                        r.gate === 'SL' && 'font-semibold text-sell',
                      )}
                      style={r.metric && !r.dim ? { color: metricTone(findMetric(reg, r.metric)?.hue ?? 0).color } : undefined}
                    >
                      <StageText text={r.text} stages={stages} />
                    </span>
                  </span>
                )
              )}
              <Acts acts={r.acts} stages={stages} />
            </span>
          </div>
        );
      })}
    </div>
  );
}

function isOrPair(rows: FlowRow[]): boolean {
  return rows.length === 1 && rows[0].gate === 'v';
}

/** One AND group shares the sell if/then bracket. A lone read stays a plain row. */
function braceAnd(rows: FlowRow[]): FlowRow[] {
  if (rows.length < 2) return rows;
  return rows.map((r, i) => ({
    ...r,
    along: i > 0,
    brace: i === 0 ? 'open' : i === rows.length - 1 ? 'close' : 'mid',
  }));
}

/** Groups of one signal. A `v` row from the flow is only the break between groups. */
function signalGroups(pairs: FlowRow[][]): FlowRow[][] {
  const groups: FlowRow[][] = [];
  let buf: FlowRow[] = [];
  const flush = () => {
    if (buf.length) groups.push(braceAnd(buf));
    buf = [];
  };
  for (const pair of pairs) {
    if (isOrPair(pair)) flush();
    else buf.push(...pair);
  }
  flush();
  return groups;
}

/** `or` is the parent. Each group is a child, so a lone read takes the same step as a bracket. */
function alignSignal(groups: FlowRow[][]): FlowRow[][] {
  const child = groups.length > 1;
  if (!child) return groups;
  return groups.map((rows) => rows.map((r) => (r.brace ? r : { ...r, tuck: true })));
}

function SignalBody({
  block,
  reg,
  stages,
  signals,
}: {
  block: FlowBlock;
  reg: StrategyRegistry;
  stages: string[];
  signals: Set<string>;
}) {
  const groups = alignSignal(signalGroups(block.pairs));
  return (
    <div className="flex w-max shrink-0 flex-col gap-1">
      <BlockHead block={block} signal />
      <div className="flex flex-col gap-1">
        {groups.map((rows, i) => (
          <div key={i} className="flex flex-col">
            {i > 0 && (
              <div className="flex items-center gap-2 py-0.5">
                <span className="text-[11px] font-semibold tracking-wide uppercase" style={{ color: ROLE.signal }}>
                  or
                </span>
                <span className="h-px w-10 bg-white/25" />
              </div>
            )}
            <FlowRows rows={rows} reg={reg} stages={stages} signals={signals} />
          </div>
        ))}
      </div>
    </div>
  );
}

function BlockHead({ block, signal = false }: { block: FlowBlock; signal?: boolean }) {
  if (!block.head) return null;
  if (block.kind === 'stage') return <StageMark name={block.head} />;
  if (block.kind === 'again') return <StepMark name={block.head} />;
  return <CaseMark name={block.head} signal={signal} />;
}

function SellBlock({ block, reg, stages, signals }: { block: FlowBlock; reg: StrategyRegistry; stages: string[]; signals: Set<string> }) {
  return (
    <div className="flex w-max shrink-0 flex-col gap-1">
      <BlockHead block={block} signal={signals.has(block.head)} />
      {block.pairs.map((rows, i) => (
        <div key={i} className={cn(i > 0 && 'mt-1 border-t border-white/10 pt-1')}>
          <FlowRows rows={rows} reg={reg} stages={stages} signals={signals} />
        </div>
      ))}
    </div>
  );
}

function BlockStrip({
  blocks,
  reg,
  stages,
  signals,
  bookends = false,
}: {
  blocks: FlowBlock[];
  reg: StrategyRegistry;
  stages: string[];
  signals: Set<string>;
  /** A divider before the first block and after the last, same as the ones between. */
  bookends?: boolean;
}) {
  return (
    <div className="flex items-stretch">
      {blocks.map((block, i) => (
        <div
          key={block.key}
          className={cn(
            'flex',
            (bookends || i > 0) && 'border-l border-white/10 pl-3',
            i > 0 && 'ml-3',
            bookends && i === blocks.length - 1 && 'border-r pr-3',
          )}
        >
          <SellBlock block={block} reg={reg} stages={stages} signals={signals} />
        </div>
      ))}
    </div>
  );
}

export function RuleSentences({ doc, reg, watch }: { doc: RuleDoc; reg: StrategyRegistry; /** Fingerprint name on the WATCH row. */ watch?: string }) {
  const split = flowSplit(doc, watch);
  const stages = doc.stages.map((s) => s.name);
  const signals = new Set(doc.signals.map((s) => s.name));
  const watchRows = split.entry.filter((r) => r.head === 'WATCH');
  const buyRows = split.entry.filter((r) => r.head !== 'WATCH');
  const sellAlways = split.sell.filter((b) => b.kind === 'always');
  const sellStages = split.sell.filter((b) => b.kind === 'stage');
  const sellOther = split.sell.filter((b) => b.kind !== 'always' && b.kind !== 'stage');
  const flow = { reg, stages, signals };
  return (
    <div className="flex flex-col gap-3">
      {split.signals.length > 0 && (
        <div
          className="flex w-max max-w-full shrink-0 flex-col gap-2 self-start rounded-lg border px-3 py-2"
          style={{ borderColor: ROLE.signal, backgroundColor: wash(ROLE.signal, 4) }}
        >
          <StepMark name="SIGNAL" />
          <div className="flex items-stretch overflow-x-auto">
            {split.signals.map((block, i) => (
              <div key={block.key} className={cn('flex', i > 0 && 'ml-3 border-l border-white/15 pl-3')}>
                <SignalBody block={block} {...flow} />
              </div>
            ))}
          </div>
        </div>
      )}
      <div className="flex items-start gap-6 overflow-x-auto rounded-lg border border-white/15 bg-bg-card px-3 py-2">
        <div className="flex w-max shrink-0 flex-col">
          <div className="px-3 pt-2 pb-2">
            <FlowRows rows={watchRows} {...flow} />
          </div>
          <div className="border-t border-white/30" />
          <div className="mt-2 rounded-lg border border-buy/40 px-3 py-2">
            {buyRows.length > 0 && <FlowRows rows={buyRows} {...flow} />}
            {split.buy.map((block) => (
              <div key={block.key} className="mt-1 border-t border-white/10 pt-1">
                <SellBlock block={block} {...flow} />
              </div>
            ))}
          </div>
        </div>
        <div className="flex w-max shrink-0 flex-col gap-2 rounded-lg border border-sell/40 px-3 py-2">
          <StepMark name="SELL" />
          <div className="flex items-stretch overflow-x-auto">
            <div className="flex items-stretch gap-3">
              {sellOther.length > 0 && <BlockStrip blocks={sellOther} {...flow} />}
              {sellAlways.length > 0 && (
                <div
                  className="rounded-md border border-dashed px-2 py-1.5"
                  style={{ borderColor: ROLE.info, backgroundColor: wash(ROLE.info, 5) }}
                >
                  <BlockStrip blocks={sellAlways} {...flow} />
                </div>
              )}
            </div>
            {sellStages.length > 0 && (sellAlways.length > 0 || sellOther.length > 0) && (
              <div className="mx-10 flex shrink-0 gap-1 self-stretch py-0.5" aria-hidden>
                <span className="w-0.5 rounded-full bg-white/55" />
                <span className="w-0.5 rounded-full bg-white/55" />
              </div>
            )}
            {sellStages.length > 0 && (
              <div className="flex flex-col gap-1.5">
                <div
                  className="w-full rounded px-2 py-0.5 text-[11px] font-semibold tracking-wide"
                  style={{ color: ROLE.stage, backgroundColor: wash(ROLE.stage, 16) }}
                >
                  STAGES
                </div>
                <BlockStrip blocks={sellStages} bookends {...flow} />
              </div>
            )}
          </div>
        </div>
        {split.again && (
          <div className="w-max shrink-0 self-start rounded-lg border border-white/15 px-3 py-2">
            <SellBlock block={split.again} {...flow} />
          </div>
        )}
      </div>
    </div>
  );
}
