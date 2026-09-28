// Signals: named conditions, written once and used by name in any condition. A signal
// holds when ANY of its groups holds (every condition of that group). The name
// sits in a column on the left. The rail on the right says any, each group
// indents as all / and, and or sits between groups.

import { Fragment } from 'react';

import { Button } from 'components/ui/Button';
import { IconButton } from 'components/ui/IconButton';
import { Input } from 'components/ui/Input';
import { CloseIcon, PlusIcon } from 'components/ui/icons';
import { defaultRef } from 'lib/strategy/metricRef';
import { ROLE } from 'lib/strategy/roleColors';
import { freshName, metricCond, newId, type MetricCond, type RuleDoc, type Signal } from 'lib/strategy/ruleDoc';
import { nameError } from 'lib/strategy/validate';
import { CondList, Lead, PartHeader } from './parts';
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
    <div className="flex flex-col gap-4">
      <PartHeader ctx={ctx} part="signals">
        <Button variant="subtle" size="xs" disabled={ctx.disabled} onClick={add}>
          <PlusIcon className="size-3" /> signal
        </Button>
      </PartHeader>
      {signals.map((s, i) => {
        const nErr = nameError(s.name, 'Name');
        return (
          <div key={s.id} className="flex items-start gap-3 border-b border-white/10 pb-3 last:border-b-0 last:pb-0">
            <div className="flex w-44 shrink-0 flex-col gap-1.5 border-r border-white/10 pr-3">
              <div className="flex items-center gap-1">
                <span className="text-[11px] font-semibold uppercase tracking-wide" style={{ color: ROLE.signal }}>Signal</span>
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
              <Input
                fieldSize="sm"
                className="w-full font-mono"
                value={s.name}
                disabled={ctx.disabled}
                onChange={(e) => onRename(s.name, e.target.value)}
              />
              {nErr && <p className="text-[11px] text-red">{nErr}</p>}
            </div>
            <div className="flex min-w-0 flex-1 gap-2">
              <div className="w-0.5 shrink-0 rounded-full" style={{ backgroundColor: ROLE.signal }} />
              <div className="flex min-w-0 flex-1 flex-col gap-1">
                <Lead word="any" />
                {s.groups.map((g, gi) => (
                  <Fragment key={gi}>
                    {gi > 0 && (
                      <div className="flex items-center gap-2 py-0.5 pl-12">
                        <Lead word="or" />
                        <span className="h-px min-w-8 flex-1 bg-white/12" />
                      </div>
                    )}
                    <div className="flex items-start gap-1 pl-12">
                      <div className="min-w-0 flex-1">
                        <CondList
                          conds={g}
                          metricOnly
                          ctx={inner}
                          word={(k) => (k === 0 ? 'all' : 'and')}
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
                  </Fragment>
                ))}
                <div className="pl-12">
                  <Button variant="subtle" size="xs" disabled={ctx.disabled} onClick={() => set(i, { ...s, groups: [...s.groups, starterGroup(ctx)] })}>
                    <PlusIcon className="size-3" /> or group
                  </Button>
                </div>
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}
