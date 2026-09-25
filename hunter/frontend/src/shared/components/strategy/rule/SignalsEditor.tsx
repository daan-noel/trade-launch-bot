// Signals: named conditions, written once and used by name in any line. A signal
// holds when ANY of its groups holds (every condition of that group).

import { Button } from 'components/ui/Button';
import { IconButton } from 'components/ui/IconButton';
import { Input } from 'components/ui/Input';
import { CloseIcon, PlusIcon } from 'components/ui/icons';
import { defaultRef } from 'lib/strategy/metricRef';
import { freshName, metricCond, newId, type MetricCond, type RuleDoc, type Signal } from 'lib/strategy/ruleDoc';
import { condsSentence } from 'lib/strategy/sentences';
import { nameError } from 'lib/strategy/validate';
import { CondList, PartHeader } from './parts';
import type { CondContext } from './CondRow';

function starterGroup(ctx: CondContext): MetricCond[] {
  const first = ctx.reg.families[0]?.metrics[0];
  return first ? [metricCond(defaultRef(first))] : [];
}

export function SignalsEditor({
  doc,
  onChange,
  onRename,
  ctx,
}: {
  doc: RuleDoc;
  onChange: (signals: Signal[]) => void;
  /** Rename, carrying every use of the old name. */
  onRename: (from: string, to: string) => void;
  ctx: CondContext;
}) {
  const signals = doc.signals;
  const set = (i: number, s: Signal) => onChange(signals.map((x, j) => (j === i ? s : x)));
  const add = () =>
    onChange([...signals, { id: newId(), name: freshName('signal', signals.map((s) => s.name)), groups: [starterGroup(ctx)] }]);
  const inner: CondContext = { ...ctx, beforeBuy: false };
  return (
    <div className="flex flex-col gap-2">
      <PartHeader ctx={ctx} part="signals">
        <Button variant="subtle" size="xs" disabled={ctx.disabled} onClick={add}>
          <PlusIcon className="size-3" /> signal
        </Button>
      </PartHeader>
      {signals.map((s, i) => {
        const nErr = nameError(s.name, 'Name');
        return (
          <div key={s.id} className="flex flex-col gap-1.5 rounded-md border border-white/10 bg-bg-card px-2 py-2">
            <div className="flex items-center gap-2">
              <span className="text-[11px] font-semibold text-text-dim">Signal</span>
              <Input
                fieldSize="sm"
                className="w-40 font-mono"
                value={s.name}
                disabled={ctx.disabled}
                onChange={(e) => onRename(s.name, e.target.value)}
              />
              <span className="text-[11px] text-text-dim">holds when any group holds</span>
              <IconButton
                className="ml-auto"
                variant="ghost"
                size="sm"
                disabled={ctx.disabled}
                onClick={() => onChange(signals.filter((_, j) => j !== i))}
                title="Remove signal"
                aria-label="Remove signal"
              >
                <CloseIcon />
              </IconButton>
            </div>
            {nErr && <p className="text-[11px] text-red">{nErr}</p>}
            {s.groups.map((g, gi) => (
              <div key={gi} className="flex flex-col gap-1">
                {gi > 0 && <span className="text-[10px] font-semibold uppercase tracking-wide text-accent">or</span>}
                <div className="flex items-start gap-1 rounded border border-white/5 p-1.5">
                  <div className="min-w-0 flex-1">
                    <CondList
                      conds={g}
                      metricOnly
                      ctx={inner}
                      empty="An empty group: add a condition or remove the group."
                      onChange={(cs) =>
                        set(i, { ...s, groups: s.groups.map((x, k) => (k === gi ? (cs as MetricCond[]) : x)) })
                      }
                    />
                  </div>
                  {s.groups.length > 1 && (
                    <IconButton
                      variant="ghost"
                      size="sm"
                      disabled={ctx.disabled}
                      onClick={() => set(i, { ...s, groups: s.groups.filter((_, k) => k !== gi) })}
                      title="Remove this group"
                      aria-label="Remove group"
                    >
                      <CloseIcon />
                    </IconButton>
                  )}
                </div>
              </div>
            ))}
            <div>
              <Button variant="subtle" size="xs" disabled={ctx.disabled} onClick={() => set(i, { ...s, groups: [...s.groups, starterGroup(ctx)] })}>
                <PlusIcon className="size-3" /> or group
              </Button>
            </div>
            <p className="text-[11px] text-text-dim/80">
              `{s.name}` holds when {s.groups.map((g) => `(${condsSentence(ctx.reg, g)})`).join(' or ')}.
            </p>
          </div>
        );
      })}
    </div>
  );
}
