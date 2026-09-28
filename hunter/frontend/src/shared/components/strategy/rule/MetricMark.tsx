// A metric's glance: a family color and the metric name. The phrase and the
// definition are the hover, not a second line of text.

import { familyShort, hashHue, metricTone } from 'lib/strategy/metricColors';
import { buyRoleColor, leadColor, mix, ROLE, signalMark, wash } from 'lib/strategy/roleColors';
import { familyName, findFamily, findMetric, type StrategyRegistry } from 'lib/strategy/registry';
import type { Cond } from 'lib/strategy/ruleDoc';
import { condFace } from 'lib/strategy/sentences';

/** A stage name. Outline only, so it never reads as a filled metric-family mark. */
export function StageMark({ name }: { name: string }) {
  return (
    <span
      className="inline-flex shrink-0 items-center rounded border border-accent/80 bg-transparent px-1.5 py-px font-mono text-[12px] font-semibold leading-5 text-accent"
      title="stage"
    >
      {name}
    </span>
  );
}

interface MarkLook {
  title: string;
  color: string;
  fill?: string;
  strong?: boolean;
}

/**
 * One family per role, a step inside the family per job.
 * Buy gates stay green. Try locks stay gray. The if/then test stays blue.
 * Sell-side acts stay filled, each in the color of what it does.
 */
const signal = signalMark();

const WORD_KIND: Record<string, MarkLook> = {
  On: { title: 'buy', color: buyRoleColor('on', null) },
  'Only If': { title: 'buy', color: buyRoleColor('if', null) },
  'Give Up': { title: 'buy', color: buyRoleColor('giveup', null) },
  'Any Print': { title: 'how often', color: buyRoleColor('lock', null) },
  'Once Per Slot': { title: 'how often', color: buyRoleColor('lock', 'slot') },
  'Once Per Coin': { title: 'how often', color: buyRoleColor('lock', 'token') },
  Always: { title: 'where', color: ROLE.text },
  'at end': { title: 'where', color: mix(ROLE.text, 62, ROLE.warn) },
  if: { title: 'test', color: leadColor('if') },
  then: { title: 'test', color: leadColor('then') },
  v: { title: 'or', color: leadColor('v') },
  sell: { title: 'does', color: ROLE.sell, fill: wash(ROLE.sell, 20), strong: true },
  go: { title: 'does', color: ROLE.stage, fill: wash(ROLE.stage, 20), strong: true },
  stop: { title: 'stop loss', color: mix(ROLE.sell, 72, 'white'), fill: wash(ROLE.sell, 12), strong: true },
  SL: { title: 'stop loss', color: mix(ROLE.sell, 72, 'white'), fill: wash(ROLE.sell, 12), strong: true },
  take: { title: 'take profit', color: mix(ROLE.buy, 58, 'white'), fill: wash(ROLE.buy, 16), strong: true },
  TP: { title: 'take profit', color: mix(ROLE.buy, 58, 'white'), fill: wash(ROLE.buy, 16), strong: true },
  deadline: { title: 'limit', color: ROLE.warn },
};

const SIGNAL_KIND: MarkLook = { title: 'signal', color: signal.color, fill: signal.fill };
const PLAIN_KIND: MarkLook = { title: 'case', color: ROLE.mid };

/** A case word. The role sets the family, and the job sets the step inside it. */
export function CaseMark({ name, signal = false }: { name: string; signal?: boolean }) {
  const kind = WORD_KIND[name] ?? (signal ? SIGNAL_KIND : PLAIN_KIND);
  return (
    <span
      className={`inline-flex shrink-0 items-center rounded border px-1.5 py-px font-mono text-[12px] leading-5 ${kind.strong ? 'font-semibold' : ''}`}
      style={{ color: kind.color, borderColor: kind.color, backgroundColor: kind.fill ?? 'transparent' }}
      title={kind.title}
    >
      {name}
    </span>
  );
}

/** A step of the rule. Filled, in the same family as the work that step does. */
const STEP_KIND: Record<string, MarkLook> = {
  WATCH: { title: 'watch', color: ROLE.mid, fill: 'color-mix(in srgb, white 10%, transparent)' },
  SIGNAL: { title: 'named condition', color: signal.color, fill: signal.fill },
  BUY: { title: 'buy', color: ROLE.buy, fill: wash(ROLE.buy, 18) },
  SELL: { title: 'sell', color: ROLE.sell, fill: wash(ROLE.sell, 18) },
  AGAIN: { title: 'buy again', color: mix(ROLE.buy, 50, 'white'), fill: wash(ROLE.buy, 10) },
};

export function StepMark({ name }: { name: string }) {
  const kind = STEP_KIND[name] ?? { title: 'step', color: ROLE.text, fill: 'color-mix(in srgb, white 10%, transparent)' };
  return (
    <span
      className="inline-flex shrink-0 items-center rounded px-1.5 py-px text-[11px] font-semibold tracking-wide"
      style={{ color: kind.color, backgroundColor: kind.fill }}
      title={kind.title}
    >
      {name}
    </span>
  );
}

export function FamilyMark({ family, subject, hue }: { family: string; subject?: string; hue?: number }) {
  const tint = metricTone(hue ?? hashHue(family));
  return (
    <span
      className="inline-flex shrink-0 items-center rounded px-1 font-mono text-[10px] font-semibold leading-4"
      style={tint.style}
      title={subject || family}
    >
      {familyShort(family)}
    </span>
  );
}

export function CondFace({ cond, reg }: { cond: Cond; reg?: StrategyRegistry }) {
  if (cond.kind === 'signal') {
    return (
      <span className="inline-flex items-center gap-1">
        {cond.not && <span className="font-mono text-[11px] text-text-mid">not</span>}
        <CaseMark name={cond.signal} signal />
      </span>
    );
  }
  const spec = findMetric(reg, cond.ref.metric);
  const family = familyName(cond.ref.metric);
  const title = spec ? `${spec.phrase}. ${spec.summary}` : cond.ref.metric;
  const tone = spec ? metricTone(spec.hue) : undefined;
  return (
    <span className="inline-flex min-w-0 items-center gap-1" title={title}>
      <FamilyMark family={family} subject={findFamily(reg, family)?.title} hue={spec?.hue} />
      <span className="truncate font-mono text-[11px]" style={tone ? { color: tone.color } : undefined}>
        {condFace(cond)}
      </span>
    </span>
  );
}

/** The first live condition, and a count when the gate has more. */
export function GateFaces({ conds, reg }: { conds: Cond[]; reg?: StrategyRegistry }) {
  const live = conds.filter((c) => !c.off);
  const shown = live.length ? live : conds;
  if (!shown.length) return null;
  const extra = shown.length - 1;
  return (
    <span className="inline-flex min-w-0 items-center gap-1">
      <CondFace cond={shown[0]} reg={reg} />
      {extra > 0 && <span className="text-[10px] text-text-dim">+{extra}</span>}
    </span>
  );
}
