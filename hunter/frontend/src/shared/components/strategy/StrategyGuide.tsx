// The Guide: how a rule runs and what every word means. The workflow and the terms
// are the only text written here; every rule part, span, tag matcher, family and metric
// is rendered from the registry's one definition with its example.

import { useMemo, useState, type ReactNode } from 'react';

import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { Modal } from 'components/ui/Modal';
import { cn } from 'lib/cn';
import { useStrategyRegistry, type MetricSpec, type StrategyRegistry } from 'lib/strategy/registry';

const TERMS: [string, string, string][] = [
  ['fingerprint', 'Which coins a rule watches (the creation shape), plus its tags.', 'cu_price = 1000 and 3 ix'],
  ['tag', 'A named trade list on a fingerprint. Every trade of a coin carries it or not.', 'volume = program 9ddjzq or the creator'],
  ['@tag / @!tag', 'The trades with the tag / the trades without it.', 'm_flow.buy_sol @!volume = what outsiders bought'],
  ['family', 'One subject: the pool, the chart, money moving, our position, ...', 'm_flow'],
  ['metric', 'One measured number in a family; its last word is its unit.', 'm_flow.buy_sol (SOL)'],
  ['span', 'The stretch a metric counts over. None = the whole life.', '[10s], [20sl], [5p], [age60s]'],
  ['condition', 'A metric read judged against numbers.', 'm_flow.buy_sol @!volume [10s] >= 2'],
  ['signal', 'A named condition, written once and used by name in any line.', 'cashout'],
  ['line', 'If every condition holds: sell and/or go to a stage.', 'cashout -> sell "spike"'],
  ['stage', 'A step after the buy with its own lines and an optional deadline.', 'early (until age 20 s) -> late'],
  ['ix shape', "A transaction's exact ordered instruction list.", '["Compute Budget: SetComputeUnitLimit", "Pump.Fun: Buy"]'],
  ['ix template', 'The coarse shape program|CU|ATA|N|S|F.', 'Axiom Trade|CU|ATA|1|0|0'],
];

function H({ children }: { children: ReactNode }) {
  return <h3 className="mt-3 text-[13px] font-semibold text-text">{children}</h3>;
}

function Row({ name, summary, example, note, mono = true }: { name: ReactNode; summary: string; example?: string; note?: string; mono?: boolean }) {
  return (
    <div className="grid grid-cols-[minmax(10rem,16rem)_1fr] gap-x-3 border-b border-white/5 py-1 text-[12px]">
      <span className={cn('text-text', mono && 'font-mono text-[11px]')}>{name}</span>
      <span className="text-text-mid">
        {summary}
        {note && <span className="block text-text-dim">{note}</span>}
        {example && <span className="block font-mono text-[11px] text-text-dim">e.g. {example}</span>}
      </span>
    </div>
  );
}

function accepts(m: MetricSpec): string {
  const tag = m.tags === 'none' ? 'no tag' : m.tags === 'required' ? `needs a ${m.tag_level === 'wallet_class' ? 'wallet class' : 'tag'}` : 'tag optional';
  const spans = [
    m.spans.life && 'life',
    m.spans.window && (m.spans.slice ? 'window + slice' : 'window'),
    m.spans.since_age && 'since age',
  ].filter(Boolean);
  return `${m.unit} · ${tag} · ${spans.join(', ')}${m.position ? ' · sell lines only' : ''}`;
}

export function StrategyGuideBody({ reg }: { reg: StrategyRegistry }) {
  const [q, setQ] = useState('');
  const needle = q.trim().toLowerCase();
  const families = useMemo(() => {
    const hit = (...xs: string[]) => !needle || xs.some((x) => x.toLowerCase().includes(needle));
    return reg.families
      .map((f) => ({ ...f, metrics: f.metrics.filter((m) => hit(m.path, m.phrase, m.summary, m.example, f.title)) }))
      .filter((f) => f.metrics.length > 0);
  }, [reg, needle]);
  return (
    <div className="flex flex-col gap-1 pb-4">
      <H>How a rule runs</H>
      <ol className="list-decimal pl-5 text-[12px] leading-relaxed text-text-mid">
        <li>
          A new coin matches the rule's <b>fingerprint</b> (its creation shape). The rule starts watching it.
        </li>
        <li>
          <b>Buy</b>: on every print (and every 200 ms tick), the rule checks <i>Buy on</i> and <i>Only if</i>. When all hold, it buys.
          It never buys while one of its own sell lines already holds.
        </li>
        <li>
          <b>Sell</b>: after the buy, on every print and tick, one step: stop loss, take profit and the <i>Always</i> lines first, then
          the current stage's lines. The first line that holds acts: it sells (all, or a percent of the first bag) and/or moves to another stage.
        </li>
        <li>
          At a stage's <b>deadline</b> its at-deadline lines run once; if none acts, the rule moves on to the next stage.
        </li>
        <li>
          The exit is booked with the line's label (or, with no label, its first condition), so every result says which line sold.
        </li>
      </ol>

      <H>Terms</H>
      {TERMS.map(([n, s, e]) => (
        <Row key={n} name={n} summary={s} example={e} mono={false} />
      ))}

      <H>Rule parts</H>
      {reg.rule_parts.map((p) => (
        <Row key={p.key} name={`${p.title} (${p.key})`} summary={p.summary} example={p.example} mono={false} />
      ))}

      <H>Spans</H>
      {reg.spans.map((s) => (
        <Row key={s.key} name={`${s.title}${s.text ? ` · ${s.text}` : ''}`} summary={s.summary} example={s.example} mono={false} />
      ))}

      <H>Tags</H>
      <p className="text-[12px] text-text-mid">
        {reg.tags.summary} <span className="font-mono text-[11px] text-text-dim">e.g. {reg.tags.example}</span>
      </p>
      {reg.tags.fields.map((f) => (
        <Row key={f.key} name={`${f.title} (${f.kind === 'match' ? 'matcher' : 'option'})`} summary={f.summary} example={f.example} mono={false} />
      ))}
      {reg.tags.builtin.map((b) => (
        <Row key={b.name} name={`@${b.name} (built in)`} summary={b.summary} />
      ))}

      <H>Metrics</H>
      <Input fieldSize="sm" className="w-72" placeholder="search metrics: buy_sol, wallet, slot ..." value={q} onChange={(e) => setQ(e.target.value)} />
      {families.map((f) => (
        <div key={f.name} className="mt-2 flex flex-col">
          <span className="text-[12px] font-semibold text-text">
            {f.title} <span className="font-mono text-[11px] text-text-dim">{f.name}</span>
          </span>
          <span className="text-[11px] text-text-dim">
            {f.summary} <span className="font-mono">e.g. {f.example}</span>
          </span>
          {f.metrics.map((m) => (
            <Row
              key={m.path}
              name={
                <span className="flex flex-col">
                  <span style={{ color: `hsl(${m.hue}, 70%, 72%)` }}>{m.path}</span>
                  <span className="font-sans text-[10px] text-text-dim">{accepts(m)}</span>
                </span>
              }
              summary={m.summary}
              note={m.note || undefined}
              example={m.example}
            />
          ))}
        </div>
      ))}
    </div>
  );
}

/** The Guide as a page. */
export function StrategyGuide() {
  const { data: reg } = useStrategyRegistry();
  if (!reg) return <p className="p-3 text-[12px] text-text-dim">loading registry…</p>;
  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-2 p-4">
      <h2 className="text-[16px] font-semibold text-text">Guide: rules, fingerprints and metrics</h2>
      <StrategyGuideBody reg={reg} />
    </div>
  );
}

/** A button that opens the Guide in a modal, for the editors. */
export function GuideButton({ className }: { className?: string }) {
  const [open, setOpen] = useState(false);
  const { data: reg } = useStrategyRegistry();
  return (
    <>
      <Button variant="subtle" size="xs" className={className} onClick={() => setOpen(true)} title="How rules run, and every metric, span and tag explained">
        Guide
      </Button>
      <Modal title="Guide: rules, fingerprints and metrics" open={open} onClose={() => setOpen(false)} size="xl">
        {reg ? <StrategyGuideBody reg={reg} /> : <p className="text-[12px] text-text-dim">loading registry…</p>}
      </Modal>
    </>
  );
}
