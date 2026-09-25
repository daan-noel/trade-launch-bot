// The whole rule in plain words, in the order the engine reads it. Rendered under the
// editor and wherever a rule is shown whole.

import type { StrategyRegistry } from 'lib/strategy/registry';
import { entersOnArm, type Cond, type Line, type RuleDoc } from 'lib/strategy/ruleDoc';
import { condsSentence, deadlineSentence, lineSentence, stageNext } from 'lib/strategy/sentences';
import { formatMetricThreshold } from 'lib/strategy/windowSpec';

function live<T extends { off: boolean }>(xs: T[]): T[] {
  return xs.filter((x) => !x.off);
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-[10px] font-semibold uppercase tracking-wide text-text-dim">{title}</span>
      <div className="flex flex-col gap-0.5 pl-2 text-[12px] leading-snug text-text">{children}</div>
    </div>
  );
}

function Lines({ reg, lines }: { reg: StrategyRegistry; lines: Line[] }) {
  const ls = live(lines);
  if (!ls.length) return <p className="text-text-dim">(no lines)</p>;
  return (
    <ol className="list-decimal pl-4">
      {ls.map((l) => (
        <li key={l.id}>{lineSentence(reg, l)}</li>
      ))}
    </ol>
  );
}

function condsText(reg: StrategyRegistry, cs: Cond[]): string {
  return condsSentence(reg, live(cs));
}

/** Whether a sell line exists that the pre-entry veto reads (engine: an always line or
 *  a start-stage sell line that already holds blocks the buy). */
function vetoes(doc: RuleDoc): boolean {
  return live(doc.always).some((l) => l.sell) || live(doc.stages[0]?.on ?? []).some((l) => l.sell);
}

export function RuleSentences({ doc, reg }: { doc: RuleDoc; reg: StrategyRegistry }) {
  const e = doc.enter;
  return (
    <div className="flex flex-col gap-2">
      <Section title="1. Buy">
        {entersOnArm(doc) ? (
          <p>Buys as soon as a coin matches the fingerprint: no buy condition.</p>
        ) : (
          <>
            {live(e.event).length > 0 && (
              <p>
                <b>Buy on</b> the print where {condsText(reg, e.event)}
                {e.lock === 'token' && ' (only the first such print of the coin)'}
                {e.lock === 'slot' && ' (only the first such print of each slot)'}.
              </p>
            )}
            {live(e.filters).length > 0 && (
              <p>
                {live(e.event).length > 0 ? (
                  <>
                    <b>Only if</b> {condsText(reg, e.filters)}; otherwise keep watching.
                  </>
                ) : (
                  <>
                    <b>Buy</b> on the first print or 200 ms tick where {condsText(reg, e.filters)}.
                  </>
                )}
              </p>
            )}
            {live(e.final_filters).length > 0 && (
              <p>
                <b>Only if</b> {condsText(reg, e.final_filters)}; otherwise stop watching this coin.
              </p>
            )}
          </>
        )}
        {vetoes(doc) && (
          <p className="text-text-dim">It never buys while one of its own sell lines (Always, or the first stage) already holds.</p>
        )}
        {e.size_pct_of_pool != null && <p>Buy {formatMetricThreshold(e.size_pct_of_pool)} % of the pool's SOL instead of the fixed amount.</p>}
      </Section>

      {doc.signals.length > 0 && (
        <Section title="2. Signals">
          {doc.signals.map((s) => (
            <p key={s.id}>
              <code>{s.name}</code> holds when {s.groups.map((g) => `(${condsSentence(reg, g)})`).join(' or ')}.
            </p>
          ))}
        </Section>
      )}

      <Section title={doc.signals.length ? '3. Sell' : '2. Sell'}>
        <p className="text-text-dim">
          On every print and every 200 ms tick after the buy, one step: the lines below are read top to bottom and the
          first that holds acts.
        </p>
        {doc.stop_loss != null && <p>Stop loss: sell everything at -{formatMetricThreshold(doc.stop_loss)} %.</p>}
        {doc.take_profit != null && <p>Take profit: sell everything at +{formatMetricThreshold(doc.take_profit)} %.</p>}
        {live(doc.always).length > 0 && (
          <>
            <p>
              <b>Always</b> (in every stage):
            </p>
            <Lines reg={reg} lines={doc.always} />
          </>
        )}
        {doc.stages.map((s, i) => (
          <div key={s.id} className="flex flex-col gap-0.5">
            <p>
              <b>Stage {s.name}</b>
              {i === 0 && ' (the position starts here)'}
              {s.ends && `: ends ${deadlineSentence(s.ends)}`}
            </p>
            <Lines reg={reg} lines={s.on} />
            {s.ends && (
              <p className="pl-2">
                At the deadline: {live(s.at_end).length ? 'the first of these that holds acts; if none does, ' : ''}go to{' '}
                {stageNext(doc.stages, i) ?? '?'}.
              </p>
            )}
            {s.ends && live(s.at_end).length > 0 && <Lines reg={reg} lines={s.at_end} />}
          </div>
        ))}
        {doc.stop_loss == null && doc.take_profit == null && !live(doc.always).length && !doc.stages.some((s) => live(s.on).length || live(s.at_end).length) && (
          <p className="text-amber-300">Nothing sells: the position is held until the coin dies or migrates.</p>
        )}
      </Section>

      {(doc.reentry || doc.exclusive) && (
        <Section title="Settings">
          {doc.reentry && (
            <p>
              After a normal sell, wait {formatMetricThreshold(doc.reentry.cooldown_sec)} s and watch the coin again, up to{' '}
              {doc.reentry.max_per_coin} buys per coin.
            </p>
          )}
          {doc.exclusive && <p>Does not buy while another rule holds the coin (priority {doc.priority}).</p>}
        </Section>
      )}
    </div>
  );
}
