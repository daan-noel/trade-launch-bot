// The building blocks of the rule editor: a part heading explained from the registry,
// a list of conditions (AND), a line (`if ... -> sell / go`) and an ordered list of lines.

import type { ReactNode } from 'react';

import { Button } from 'components/ui/Button';
import { IconButton } from 'components/ui/IconButton';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { CloseIcon, PlusIcon } from 'components/ui/icons';
import { cn } from 'lib/cn';
import { defaultRef } from 'lib/strategy/metricRef';
import { rulePart } from 'lib/strategy/registry';
import { metricCond, newLine, signalCond, MAX_SELL_PCT, type Cond, type Line } from 'lib/strategy/ruleDoc';
import { actionSentence, autoLineLabel } from 'lib/strategy/sentences';
import { CondRow, type CondContext } from './CondRow';

/** A section heading: the rule part's title and one-line summary, the example on hover. */
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
        {p && <span className="text-[11px] text-text-dim">{p.summary}</span>}
      </div>
      {children}
    </div>
  );
}

/** A new condition: the first metric the context allows, no value yet. */
function newMetricCond(ctx: CondContext): Cond {
  const first = ctx.reg.families.flatMap((f) => f.metrics).find((m) => !(ctx.beforeBuy && m.position) && m.tags !== 'required');
  return metricCond(first ? defaultRef(first) : { metric: '' });
}

/** Conditions that must ALL hold. */
export function CondList({
  conds,
  onChange,
  ctx,
  empty,
  metricOnly,
}: {
  conds: Cond[];
  onChange: (cs: Cond[]) => void;
  ctx: CondContext;
  /** Shown when the list is empty. */
  empty?: string;
  /** No signal conditions (inside a signal). */
  metricOnly?: boolean;
}) {
  const set = (i: number, c: Cond) => onChange(conds.map((x, j) => (j === i ? c : x)));
  const remove = (i: number) => onChange(conds.filter((_, j) => j !== i));
  return (
    <div className="flex flex-col gap-1">
      {conds.length === 0 && empty && <p className="text-[11px] italic text-text-dim/70">{empty}</p>}
      {conds.map((c, i) => (
        <div key={c.id} className="flex flex-col gap-1">
          {i > 0 && <span className="pl-2 text-[10px] font-semibold uppercase tracking-wide text-text-dim/70">and</span>}
          <CondRow cond={c} ctx={ctx} onChange={(n) => set(i, n)} onRemove={() => remove(i)} />
        </div>
      ))}
      <div className="flex gap-1">
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
    <div className={cn('flex flex-col gap-1.5 rounded-md border border-white/10 bg-bg-card px-2 py-2', line.off && 'opacity-50')}>
      <div className="flex items-center gap-1.5">
        <span className="text-[11px] font-semibold text-text-dim">Line {index + 1}</span>
        <span className="text-[11px] text-text-dim">{checkpoint ? 'If (empty = always, at the deadline)' : 'If all of these hold'}</span>
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
      <CondList conds={line.if} onChange={(cs) => onChange({ ...line, if: cs })} ctx={ctx} empty={checkpoint ? 'No condition: acts whenever the deadline is reached.' : 'Add a condition.'} />
      <div className="flex flex-wrap items-center gap-2 border-t border-white/5 pt-1.5 text-[11px] text-text-dim">
        <span className="font-semibold">Then</span>
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
              title={`Blank = everything left. A percent sells that share of the FIRST buy's bag (at most ${MAX_SELL_PCT}) and must also go to another stage.`}
              numericValue={line.sell?.pct ?? null}
              disabled={ctx.disabled}
              onNumericChange={(n) => onChange({ ...line, sell: { label: line.sell?.label ?? '', pct: n } })}
            />
            <span>as</span>
            <Input
              fieldSize="sm"
              className="w-52"
              value={line.sell?.label ?? ''}
              placeholder={autoLineLabel(line)}
              title="The exit reason this sell books. Blank = its first condition."
              disabled={ctx.disabled}
              onChange={(e) => onChange({ ...line, sell: { label: e.target.value, pct: line.sell?.pct ?? null } })}
            />
          </>
        )}
        <span>{sellOn ? 'and go to' : 'go to'}</span>
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
      </div>
      <p className="text-[11px] text-text-dim/80">Then: {actionSentence(line)}.</p>
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
    <div className="flex flex-col gap-1.5">
      {lines.map((l, i) => (
        <LineEditor
          key={l.id}
          index={i}
          line={l}
          ctx={ctx}
          stages={stages}
          checkpoint={checkpoint}
          onChange={(n) => set(i, n)}
          onRemove={() => onChange(lines.filter((_, j) => j !== i))}
          onMove={lines.length > 1 ? (d) => move(i, d) : undefined}
        />
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
