// The building blocks of the rule editor: a part heading explained from the registry,
// a list of conditions, and a clause rail. Each condition is its own plate.
// A line is if / and / then on one bar. else sits between lines, on a rule,
// because the first line that holds is the one that acts.

import { Fragment, type ReactNode } from 'react';

import { Button } from 'components/ui/Button';
import { IconButton } from 'components/ui/IconButton';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { CloseIcon, PlusIcon } from 'components/ui/icons';
import { cn } from 'lib/cn';
import { defaultRef } from 'lib/strategy/metricRef';
import { leadColor, ROLE } from 'lib/strategy/roleColors';
import { rulePart } from 'lib/strategy/registry';
import { metricCond, newLine, signalCond, MAX_SELL_PCT, type Cond, type Line } from 'lib/strategy/ruleDoc';
import { actionSentence, autoLineLabel } from 'lib/strategy/sentences';
import { CondRow, type CondContext } from './CondRow';

/** A section heading: the rule part's title. The summary and example are the info tip. */
export function PartHeader({
  ctx,
  part,
  title,
  children,
}: {
  ctx: Pick<CondContext, 'reg'>;
  part: string;
  /** Overrides the registry title. */
  title?: string;
  /** Controls on the right. */
  children?: ReactNode;
}) {
  const p = rulePart(ctx.reg, part);
  return (
    <div className="flex flex-wrap items-start gap-2">
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="inline-flex items-center gap-1 text-[12px] font-semibold text-text">
          {title ?? p?.title ?? part}
          {p && <InfoTooltip title={p.title} body={`${p.summary}\n\nExample: ${p.example}`} />}
        </span>
      </div>
      {children}
    </div>
  );
}

/** One logic word. if / then / any / or take their role color. and / all / else stay dim. */
export function Lead({ word }: { word: string }) {
  return (
    <span
      className="flex h-7 w-12 shrink-0 items-center text-[11px] font-semibold tracking-wide uppercase"
      style={{ color: leadColor(word) }}
    >
      {word}
    </span>
  );
}

/** A new condition: the first metric the context allows, no value yet. */
function newMetricCond(ctx: CondContext): Cond {
  const first = ctx.reg.families.flatMap((f) => f.metrics).find((m) => !(ctx.beforeBuy && m.position) && m.tags !== 'required');
  return metricCond(first ? defaultRef(first) : { metric: '' });
}

/** Conditions joined in one column. The first row's word names the join (`if`, `all`); the rest say `and`. */
export function CondList({
  conds,
  onChange,
  ctx,
  empty,
  metricOnly,
  word,
}: {
  conds: Cond[];
  onChange: (cs: Cond[]) => void;
  ctx: CondContext;
  /** Shown when the list is empty. */
  empty?: string;
  /** No signal conditions (inside a signal). */
  metricOnly?: boolean;
  /** The logic word for row `i`. Default: blank, then `and`. */
  word?: (index: number) => string;
}) {
  const set = (i: number, c: Cond) => onChange(conds.map((x, j) => (j === i ? c : x)));
  const remove = (i: number) => onChange(conds.filter((_, j) => j !== i));
  const leadOf = (i: number) => (word ? word(i) : i === 0 ? '' : 'and');
  const showLead = conds.some((_, i) => leadOf(i) !== '') || (conds.length === 0 && word != null);
  return (
    <div className="flex flex-col gap-2">
      {conds.length === 0 && empty && (
        <div className="flex items-center gap-2">
          {showLead && <Lead word={leadOf(0)} />}
          <p className="text-[11px] italic text-text-dim/70">{empty}</p>
        </div>
      )}
      {conds.map((c, i) => (
        <div key={c.id} className="flex items-start gap-2">
          {showLead && (
            <div className="pt-1">
              <Lead word={leadOf(i)} />
            </div>
          )}
          <div className="min-w-0 flex-1 rounded-md border border-white/12 bg-white/4 px-2 py-1">
            <CondRow cond={c} ctx={ctx} onChange={(n) => set(i, n)} onRemove={() => remove(i)} />
          </div>
        </div>
      ))}
      <div className={cn('flex gap-1', showLead && 'pl-14')}>
        <Button variant="subtle" size="xs" disabled={ctx.disabled} onClick={() => onChange([...conds, newMetricCond(ctx)])}>
          <PlusIcon className="size-3" /> condition
        </Button>
        {!metricOnly && ctx.signals.length > 0 && (
          <Button variant="subtle" size="xs" disabled={ctx.disabled} onClick={() => onChange([...conds, signalCond(ctx.signals[0])])}>
            <PlusIcon className="size-3" /> signal
          </Button>
        )}
      </div>
    </div>
  );
}

/** One line: its conditions, then what it does. */
export function LineEditor({
  line,
  onChange,
  onRemove,
  onMove,
  ctx,
  stages,
  checkpoint,
  index,
}: {
  line: Line;
  onChange: (l: Line) => void;
  onRemove: () => void;
  onMove?: (delta: -1 | 1) => void;
  ctx: CondContext;
  /** Stage names a line may go to. */
  stages: readonly string[];
  /** An at-deadline line: no condition means "always, at the deadline". */
  checkpoint?: boolean;
  index: number;
}) {
  const sellOn = line.sell != null;
  return (
    <div className={cn('flex gap-2', line.off && 'opacity-50')}>
      <span className="mt-1 flex h-7 w-5 shrink-0 items-center justify-end text-[12px] font-semibold tabular-nums text-text">{index + 1}</span>
      <div className="flex min-w-0 flex-1 gap-2">
        <div className="w-0.5 shrink-0 rounded-full" style={{ backgroundColor: ROLE.info }} />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <CondList
            conds={line.if}
            onChange={(cs) => onChange({ ...line, if: cs })}
            ctx={ctx}
            word={(i) => (i === 0 ? 'if' : 'and')}
            empty={checkpoint ? 'No condition: acts whenever the deadline is reached.' : 'Add a condition.'}
          />
          <div className="mt-1 flex items-start gap-2 border-t border-white/12 pt-2" title={actionSentence(line)}>
            <Lead word="then" />
            <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-text-dim">
              <label className="flex items-center gap-1">
                <input
                  type="checkbox"
                  className="accent-accent"
                  checked={sellOn}
                  disabled={ctx.disabled}
                  onChange={(e) => onChange({ ...line, sell: e.target.checked ? { label: '', pct: null } : null })}
                />
                sell
              </label>
              {sellOn && (
                <>
                  <Input
                    fieldSize="sm"
                    className="w-16"
                    numeric
                    unit="%"
                    placeholder="all"
                    title={`Blank sells everything left. A percent sells that share of the first buy's bag (at most ${MAX_SELL_PCT}) and the line must also go to another stage.`}
                    numericValue={line.sell?.pct ?? null}
                    disabled={ctx.disabled}
                    onNumericChange={(n) => onChange({ ...line, sell: { label: line.sell?.label ?? '', pct: n } })}
                  />
                  <span className="whitespace-nowrap">{line.sell?.pct != null ? 'of the first bag, as' : 'everything, as'}</span>
                  <Input
                    fieldSize="sm"
                    className="w-40"
                    value={line.sell?.label ?? ''}
                    placeholder={autoLineLabel(line)}
                    title="The exit reason this sell books. Blank uses the first condition."
                    disabled={ctx.disabled}
                    onChange={(e) => onChange({ ...line, sell: { label: e.target.value, pct: line.sell?.pct ?? null } })}
                  />
                </>
              )}
              <span className="whitespace-nowrap">{sellOn ? 'and go to' : 'go to'}</span>
              <Select
                className="w-32"
                value={line.go ?? ''}
                disabled={ctx.disabled}
                onChange={(e) => onChange({ ...line, go: e.target.value || null })}
              >
                <option value="">{sellOn ? '(stay)' : 'pick a stage...'}</option>
                {line.go && !stages.includes(line.go) && <option value={line.go}>{line.go} (missing)</option>}
                {stages.map((s) => (
                  <option key={s} value={s}>
                    {s}
                  </option>
                ))}
              </Select>
              <div className="ml-auto flex items-center gap-1">
                {onMove && (
                  <>
                    <IconButton variant="ghost" size="sm" disabled={ctx.disabled} onClick={() => onMove(-1)} title="Move up: lines are checked top to bottom">
                      ↑
                    </IconButton>
                    <IconButton variant="ghost" size="sm" disabled={ctx.disabled} onClick={() => onMove(1)} title="Move down">
                      ↓
                    </IconButton>
                  </>
                )}
                <label className="flex items-center gap-1 text-[11px] text-text-dim" title="Off: kept in the rule and checked for mistakes, but never used.">
                  <input type="checkbox" className="accent-accent" checked={!line.off} disabled={ctx.disabled} onChange={(e) => onChange({ ...line, off: !e.target.checked })} />
                  on
                </label>
                <IconButton variant="ghost" size="sm" onClick={onRemove} disabled={ctx.disabled} aria-label="Remove line" title="Remove line">
                  <CloseIcon />
                </IconButton>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

/** An ordered list of lines. The engine reads them top to bottom and the first that
 *  holds acts, so order matters. */
export function LineList({
  lines,
  onChange,
  ctx,
  stages,
  checkpoint,
  addLabel = 'line',
  defaultGo,
}: {
  lines: Line[];
  onChange: (ls: Line[]) => void;
  ctx: CondContext;
  stages: readonly string[];
  checkpoint?: boolean;
  addLabel?: string;
  /** A new line goes here instead of selling (e.g. the next stage). */
  defaultGo?: string;
}) {
  const set = (i: number, l: Line) => onChange(lines.map((x, j) => (j === i ? l : x)));
  const move = (i: number, d: -1 | 1) => {
    const j = i + d;
    if (j < 0 || j >= lines.length) return;
    const next = [...lines];
    [next[i], next[j]] = [next[j], next[i]];
    onChange(next);
  };
  return (
    <div className="flex flex-col gap-3">
      {lines.map((l, i) => (
        <Fragment key={l.id}>
          {i > 0 && (
            <div className="flex items-center gap-2">
              <span className="w-5 shrink-0" />
              <div className="flex min-w-0 flex-1 items-center gap-2">
                <span className="w-0.5 shrink-0" />
                <Lead word="else" />
                <span className="shrink-0 text-[11px] text-text-dim">only if {i} did not fire</span>
                <span className="h-px min-w-8 flex-1 bg-white/12" />
              </div>
            </div>
          )}
          <LineEditor
            index={i}
            line={l}
            ctx={ctx}
            stages={stages}
            checkpoint={checkpoint}
            onChange={(n) => set(i, n)}
            onRemove={() => onChange(lines.filter((_, j) => j !== i))}
            onMove={lines.length > 1 ? (d) => move(i, d) : undefined}
          />
        </Fragment>
      ))}
      <div>
        <Button
          variant="subtle"
          size="xs"
          disabled={ctx.disabled}
          onClick={() => onChange([...lines, defaultGo ? newLine({ sell: null, go: defaultGo }) : newLine()])}
        >
          <PlusIcon className="size-3" /> {addLabel}
        </Button>
      </div>
    </div>
  );
}
