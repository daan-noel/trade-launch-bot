import { Input } from 'components/ui/Input';
import { cn } from 'lib/cn';
import { ConditionInput } from './ConditionInput';
import type { Condition } from 'lib/strategy/grammar';
import type { SideConditions, GroupConditions } from 'lib/strategy/ruleParams';
import type { StrategyRegistry, MetricUnit } from 'lib/strategy/registry';

export interface ConditionSideEditorProps {
  /** ENTRY or EXIT — used only for labels/ids. */
  side: 'entry' | 'exit';
  registry: StrategyRegistry;
  value: SideConditions;
  onChange: (next: SideConditions) => void;
  disabled?: boolean;
}

/**
 * One side (ENTRY or EXIT) of the rule editor: every metric group from the
 * registry, each with its strict params (e.g. `window_size_sec`) and a
 * {@link ConditionInput} per metric. Fully registry-driven — a metric added in
 * Rust appears here on the next load with no code change (plan §8). Empty metric
 * inputs mean "unconstrained"; a group with no conditions is dropped at save.
 */
export function ConditionSideEditor({
  side,
  registry,
  value,
  onChange,
  disabled,
}: ConditionSideEditorProps) {
  const group = (name: string): GroupConditions => value[name] ?? { strict: {}, metrics: {} };

  const setMetric = (groupName: string, metric: string, conds: Condition[]) => {
    const g = group(groupName);
    onChange({ ...value, [groupName]: { ...g, metrics: { ...g.metrics, [metric]: conds } } });
  };
  const setStrict = (groupName: string, param: string, n: number | null) => {
    const g = group(groupName);
    const strict = { ...g.strict };
    if (n == null) delete strict[param];
    else strict[param] = n;
    onChange({ ...value, [groupName]: { ...g, strict } });
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="text-[11px] font-semibold uppercase tracking-wide text-text-dim">
        {side}
      </div>
      {registry.groups.map((gspec) => {
        const g = group(gspec.name);
        return (
          <div key={gspec.name} className="rounded-md border border-white/8 bg-white/2 p-2">
            <div className="mb-2 flex items-center gap-2">
              <span className="font-mono text-[12px] text-text">{gspec.name}</span>
              <span className="text-[10px] uppercase text-text-dim/70">{gspec.kind}</span>
            </div>
            {gspec.strict_params.length > 0 && (
              <div className="mb-2 flex flex-wrap gap-2">
                {gspec.strict_params.map((p) => (
                  <label key={p.name} className="flex items-center gap-1.5 text-[11px] text-text-dim">
                    {p.name}
                    {p.required && <span className="text-red">*</span>}
                    <Input
                      fieldSize="sm"
                      numeric
                      numericValue={g.strict[p.name] ?? null}
                      onNumericChange={(n) => setStrict(gspec.name, p.name, n)}
                      disabled={disabled}
                      className="w-20"
                    />
                  </label>
                ))}
              </div>
            )}
            <div className="grid grid-cols-[minmax(6rem,auto)_1fr] items-start gap-x-3 gap-y-2">
              {gspec.metrics.map((m) => (
                <div key={m.name} className="contents">
                  <label
                    className="pt-1.5 font-mono text-[12px] text-text-dim"
                    title={`unit: ${m.unit} · =tol ${m.eq_tolerance}${m.monotonic ? ' · monotonic' : ''}`}
                  >
                    {m.name}
                  </label>
                  <ConditionInput
                    value={g.metrics[m.name] ?? []}
                    onChange={(conds) => setMetric(gspec.name, m.name, conds)}
                    unit={m.unit as MetricUnit}
                    disabled={disabled}
                  />
                </div>
              ))}
            </div>
          </div>
        );
      })}
      <p className={cn('text-[11px] text-text-dim/70')}>
        Empty = unconstrained. A group with no conditions is ignored.
      </p>
    </div>
  );
}
