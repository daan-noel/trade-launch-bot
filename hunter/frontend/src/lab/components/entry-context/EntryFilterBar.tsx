import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { AXIS_BY_KEY, ENTRY_AXES } from '@lab/lib/entryContext/axes';
import type { AxisCondition } from '@lab/lib/entryContext/analysis';

/**
 * The Entry Context filter: one line per axis condition, ANDed; OR lives inside a
 * line (`<5 | >90`). Every axis comes from `ENTRY_AXES`, so a new axis is offered
 * here the moment it is defined, with its definition on hover.
 */
export function EntryFilterBar({
  lines,
  onChange,
  errors,
  windowSecs,
}: {
  lines: AxisCondition[];
  onChange: (next: AxisCondition[]) => void;
  errors: ReadonlyMap<number, string>;
  windowSecs: number;
}) {
  const set = (i: number, patch: Partial<AxisCondition>) =>
    onChange(lines.map((l, j) => (j === i ? { ...l, ...patch } : l)));

  return (
    <div className="mb-3 flex flex-wrap items-center gap-2 text-[11px]">
      <span className="text-[10px] font-bold uppercase tracking-widest text-text-dim">Filter</span>
      {lines.map((line, i) => {
        const axis = AXIS_BY_KEY.get(line.axis);
        const err = errors.get(i);
        return (
          <span
            key={i}
            className={`inline-flex items-center gap-1 rounded border px-1.5 py-0.5 ${
              err ? 'border-red/50 bg-red/5' : 'border-white/10 bg-white/5'
            }`}
            title={err ?? axis?.definition(windowSecs)}
          >
            <Select
              value={line.axis}
              onChange={(e) => set(i, { axis: e.target.value })}
              className="h-6 py-0 text-[11px]"
              aria-label="Axis"
            >
              {ENTRY_AXES.map((a) => (
                <option key={a.key} value={a.key}>
                  {a.label}
                </option>
              ))}
            </Select>
            <Input
              value={line.cond}
              onChange={(e) => set(i, { cond: e.target.value })}
              placeholder=">50"
              className="h-6 w-[110px] py-0 font-mono text-[11px]"
              aria-label={`Condition on ${axis?.label ?? line.axis}`}
            />
            <button
              type="button"
              className="text-text-dim hover:text-red"
              onClick={() => onChange(lines.filter((_, j) => j !== i))}
              aria-label="Remove this filter"
            >
              ×
            </button>
          </span>
        );
      })}
      <Button
        size="xs"
        variant="ghost"
        onClick={() => onChange([...lines, { axis: ENTRY_AXES[0].key, cond: '' }])}
        title="Add a condition on one axis. Lines are ANDed; write OR inside a line as <5 | >90."
      >
        + Add filter
      </Button>
      {lines.length > 0 && (
        <Button size="xs" variant="link" onClick={() => onChange([])}>
          clear
        </Button>
      )}
    </div>
  );
}
