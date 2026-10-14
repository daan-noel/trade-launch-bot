// The Guide: how a rule runs, drawn as a picture, then every word from the registry.
// The picture is the only workflow text written here. Every rule part, span, tag matcher,
// family and metric is rendered from the registry's one definition with its example.

import { useMemo, useState, type ReactNode } from 'react';

import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { Modal } from 'components/ui/Modal';
import { cn } from 'lib/cn';
import { metricRowStyle, metricTone } from 'lib/strategy/metricColors';
import { useStrategyRegistry, type MetricSpec, type StrategyRegistry } from 'lib/strategy/registry';
import { Card, Chip, DownJoin, Fall, Lane, Pass } from './GuidePicture';

const TERMS: [string, string, string][] = [
  ['fingerprint', 'Which coins a rule watches (the creation shape), plus its tags.', 'cu_price = 1000 and 3 ix'],
  ['tag', 'A named trade list on a fingerprint. Every trade of a coin carries it or not.', 'volume = program 9ddjzq or the creator'],
  ['@tag / @!tag', 'The trades with the tag / the trades without it.', 'm_flow.buy_sol @!volume = what outsiders bought'],
  ['family', 'One subject: the pool, the chart, money moving, our position, ...', 'm_flow'],
  ['metric', 'One measured number in a family; its last word is its unit.', 'm_flow.buy_sol (SOL)'],
  ['span', 'The stretch a metric counts over. None = the whole life.', '[10s], [20sl], [5p], [age60s]'],
  ['condition', 'A metric read judged against numbers.', 'm_flow.buy_sol @!volume [10s] >= 2'],
  ['signal', 'A named condition, written once and used by name in any condition.', 'cashout'],
  ['line', 'If every condition holds: sell and/or go to a stage. A signal plus that line\'s own metrics.', 'cashout and age_sec < 20 -> sell "spike"'],
  ['stage', 'A step after the buy with its own lines and an optional deadline.', 'open -> ride'],
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

function RuleWorkflow() {
  return (
    <div className="flex flex-col gap-4 pb-2">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <span className="text-[13px] font-semibold text-text">One rule on one coin</span>
        <span className="text-[11px] text-text-dim">Watch, then buy, then sell</span>
        <span className="ml-auto flex items-center gap-3 text-[11px] text-text-mid">
          <span className="flex items-center gap-1.5">
            <span className="size-2 rounded-sm bg-green" />
            Buys
          </span>
          <span className="flex items-center gap-1.5">
            <span className="size-2 rounded-sm border border-white/40" />
            Tries again
          </span>
          <span className="flex items-center gap-1.5">
            <span className="size-2 rounded-sm bg-red" />
            Stops the coin
          </span>
        </span>
      </div>

      <div className="flex flex-wrap items-start gap-4">
        <div className="flex min-w-0 flex-1 flex-col pl-10">
          <Pass n={1} title="Watch">
            <div className="grid gap-2 sm:grid-cols-2">
              <Card>
                <strong className="text-[12px] text-text">Fingerprint matches</strong>
                <Chip>Rule sees the coin</Chip>
              </Card>
              <Card>
                <strong className="text-[12px] text-text">No match</strong>
                <Chip tone="drop">Never seen</Chip>
              </Card>
            </div>
          </Pass>
          <DownJoin />
          <Pass n={2} title="Buy When" hint={<Chip>On starts a try</Chip>}>
            <Lane
              ask={
                <Card>
                  <strong className="text-[12px] text-text">Only If</strong>
                  <span className="text-[11px] text-text-dim">Must hold on that try</span>
                </Card>
              }
              via="Holds"
            >
              <Chip>This try can buy</Chip>
            </Lane>
            <Fall label="Miss" />
            <Lane ask={<Card><strong className="text-[12px] text-text">Only If misses</strong></Card>} via="Miss">
              <Chip>This try fails</Chip>
              <Chip tone="drop">Once Per Coin stops</Chip>
              <Chip>Else tries again</Chip>
            </Lane>
          </Pass>
          <DownJoin />
          <Pass n={3} title="Keep Looking" hint={<span className="text-[11px] text-text-dim">When On may start the next try</span>}>
            <div className="grid gap-2 sm:grid-cols-3">
              <Card>
                <strong className="text-[12px] text-text">Any Print</strong>
                <span className="text-[11px] text-text-dim">Every trade and clock tick where On is true</span>
              </Card>
              <Card>
                <strong className="text-[12px] text-text">Once Per Coin</strong>
                <span className="text-[11px] text-text-dim">The first trade where On is true. A tick never tries</span>
                <Chip tone="drop">A failed Only If stops</Chip>
              </Card>
              <Card>
                <strong className="text-[12px] text-text">Once Per Slot</strong>
                <span className="text-[11px] text-text-dim">Once per block, the first moment On is true</span>
              </Card>
            </div>
            <Lane
              ask={
                <Card>
                  <strong className="text-[12px] text-text">Give Up</strong>
                  <span className="text-[11px] text-text-dim">A try that would otherwise buy. No Give Up line: the try buys</span>
                </Card>
              }
              via="Holds"
            >
              <Chip tone="keep">Buys</Chip>
            </Lane>
            <Fall label="Miss" />
            <Lane ask={<Card><strong className="text-[12px] text-text">Give Up misses</strong></Card>} via="Miss">
              <Chip tone="drop">Stops the coin</Chip>
            </Lane>
          </Pass>
          <DownJoin />
          <Pass n={4} title="Sell" hint={<span className="text-[11px] text-text-dim">Every print. The first line that holds acts</span>}>
            <div className="flex flex-wrap items-center gap-1.5">
              <Chip>TP / SL, when set</Chip>
              <span className="text-[12px] text-text-dim">→</span>
              <Chip>Always</Chip>
              <span className="text-[12px] text-text-dim">→</span>
              <Chip>Open stage</Chip>
            </div>
            <div className="grid gap-2 sm:grid-cols-2">
              <Card>
                <strong className="text-[12px] text-text">A line holds</strong>
                <Chip>Sells</Chip>
                <Chip>Or moves stage</Chip>
              </Card>
              <Card>
                <strong className="text-[12px] text-text">Deadline</strong>
                <Chip>At-deadline lines once</Chip>
                <Chip>Then the next stage</Chip>
              </Card>
            </div>
          </Pass>
          <DownJoin />
          <Pass n={5} title="Again">
            <div className="grid gap-2 sm:grid-cols-3">
              <Card>
                <strong className="text-[12px] text-text">Buy again on</strong>
                <Chip>Wait after the sell</Chip>
                <Chip>Watch the coin again</Chip>
                <span className="text-[11px] text-text-dim">Up to the buy limit</span>
              </Card>
              <Card>
                <strong className="text-[12px] text-text">Buy again off</strong>
                <Chip>One buy</Chip>
              </Card>
              <Card>
                <strong className="text-[12px] text-text">Booked reason</strong>
                <Chip>The line's label</Chip>
                <Chip>First condition, if the label is blank</Chip>
              </Card>
            </div>
          </Pass>
        </div>
        <Card className="w-full shrink-0 sm:w-52">
          <strong className="text-[12px] text-text">Limits</strong>
          <Chip>Fixed buy</Chip>
          <Chip>Or % of pool</Chip>
          <Chip>At once</Chip>
          <Chip>Total</Chip>
          <Chip>Exclusive</Chip>
          <span className="text-[11px] text-text-dim">Two exclusive rules: the higher priority buys</span>
        </Card>
      </div>
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
      <div className="border-b border-white/10 pb-4">
        <RuleWorkflow />
      </div>

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
      <p className="text-[12px] text-text-mid">
        One color per metric, from the registry hue. A family is one band. Metrics in that band differ by a few degrees, so a row here matches the same row in the editor.
        Buy SOL and sell SOL keep the chart's green and red.
      </p>
      <Input fieldSize="sm" className="w-72" placeholder="search metrics: buy_sol, wallet, slot ..." value={q} onChange={(e) => setQ(e.target.value)} />
      {families.map((f) => (
        <div key={f.name} className="mt-2 flex flex-col">
          <span className="text-[12px] font-semibold" style={{ color: metricTone(f.metrics[0].hue).color }}>
            {f.title} <span className="font-mono text-[11px]">{f.name}</span>
          </span>
          <span className="text-[11px] text-text-dim">
            {f.summary} <span className="font-mono">e.g. {f.example}</span>
          </span>
          {f.metrics.map((m) => (
            <div key={m.path} className="rounded-sm" style={metricRowStyle(m.hue)}>
              <Row
                name={
                  <span className="flex flex-col">
                    <span style={{ color: metricTone(m.hue).color }}>{m.path}</span>
                    <span className="font-sans text-[10px] text-text-dim">{accepts(m)}</span>
                  </span>
                }
                summary={m.summary}
                note={m.note || undefined}
                example={m.example}
              />
            </div>
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
