import { useMemo, useState, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { Checkbox } from 'components/ui/Checkbox';
import { Badge } from 'components/ui/Badge';
import {
  GROUP_FIELDS,
  GROUP_FIELD_LABELS,
  TPSL2_AXES,
  type AxisDef,
  type GroupField,
  type GroupedSweepStartArgs,
  type Tpsl2AxesSpec,
} from './groupedTypes';

/** Mirror of the backend `MAX_COMBOS` cap — the form blocks Run above it. */
const MAX_COMBOS = 5000;

interface SweepConfigFormProps {
  strategyId: string;
  running: boolean;
  onRun: (args: GroupedSweepStartArgs) => void;
}

function Field({
  label,
  hint,
  className,
  children,
}: {
  label: string;
  hint?: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={cn('flex flex-col gap-1', className)}>
      <span className="flex items-center gap-1 text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
        {label}
        {hint && <span className="font-normal normal-case tracking-normal text-text-dim/45">{hint}</span>}
      </span>
      {children}
    </div>
  );
}

/** Render an axis default to its editable comma string (`null` → "off"). */
function axisToText(values: (number | null)[]): string {
  return values.map((v) => (v == null ? 'off' : String(v))).join(', ');
}

/** Parse an axis text box → candidate values. For nullable axes, `off/null/none/-`
 *  (or an empty token) becomes `null`; non-nullable axes drop those. NaN dropped. */
function parseAxis(text: string, nullable: boolean): (number | null)[] {
  const out: (number | null)[] = [];
  for (const raw of text.split(',')) {
    const t = raw.trim();
    if (t === '') continue;
    if (/^(off|null|none|-)$/i.test(t)) {
      if (nullable && !out.some((v) => v === null)) out.push(null);
      continue;
    }
    const n = Number(t);
    if (!Number.isNaN(n) && !out.some((v) => v === n)) out.push(n);
  }
  return out;
}

/** Effective length of an axis for combo-count projection: what the user typed,
 *  else the backend default (an empty box falls back to the default server-side). */
function axisLen(text: string, def: AxisDef): number {
  const n = parseAxis(text, def.nullable).length;
  return n > 0 ? n : def.default.length;
}

export function SweepConfigForm({ strategyId, running, onRun }: SweepConfigFormProps) {
  const [createdAfter, setCreatedAfter] = useState('');
  const [createdBefore, setCreatedBefore] = useState('');
  const [groupBy, setGroupBy] = useState<GroupField[]>(['creator_wallet']);
  const [axesText, setAxesText] = useState<Record<string, string>>(() =>
    Object.fromEntries(TPSL2_AXES.map((a) => [a.key, axisToText(a.default)])),
  );
  const [methodKind, setMethodKind] = useState<'grid' | 'random'>('grid');
  const [randomN, setRandomN] = useState(500);
  const [minTokens, setMinTokens] = useState(10);
  const [tokenCap, setTokenCap] = useState(10000);
  const [curveOnly, setCurveOnly] = useState(false);

  // Projected combos: grid = product of axis lengths; random = N (both capped).
  const projected = useMemo(() => {
    if (methodKind === 'random') return Math.max(1, randomN);
    return TPSL2_AXES.reduce((acc, a) => acc * axisLen(axesText[a.key] ?? '', a), 1);
  }, [methodKind, randomN, axesText]);

  const overCap = projected > MAX_COMBOS;

  function toggleGroupField(f: GroupField) {
    setGroupBy((prev) => (prev.includes(f) ? prev.filter((x) => x !== f) : [...prev, f]));
  }

  function buildAxes(): Tpsl2AxesSpec {
    const spec: Tpsl2AxesSpec = {};
    for (const a of TPSL2_AXES) {
      // Backend types: take_profit/stop_loss are number[]; the rest (number|null)[].
      const vals = parseAxis(axesText[a.key] ?? '', a.nullable);
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (spec as any)[a.key] = a.nullable ? vals : vals.filter((v): v is number => v !== null);
    }
    return spec;
  }

  function handleRun() {
    if (overCap || running) return;
    onRun({
      strategy_id: strategyId,
      created_after: toUtc(createdAfter),
      created_before: toUtc(createdBefore),
      curve_only: curveOnly,
      group_by: groupBy,
      min_tokens: minTokens,
      method: methodKind === 'random' ? `random:${Math.max(1, randomN)}` : 'grid',
      axes: buildAxes(),
      token_cap: tokenCap,
    });
  }

  return (
    <div className="mb-4 rounded-md border border-white/10 bg-surface p-3">
      {/* Selection + grouping + run controls */}
      <div className="flex flex-wrap items-end gap-3">
        <Field label="Created range" hint="UTC" className="w-fit">
          <div className="flex items-center gap-1">
            <Input
              type="datetime-local"
              value={createdAfter}
              onChange={(e) => setCreatedAfter(e.target.value)}
            />
            <span className="text-[10px] text-text-dim/50">–</span>
            <Input
              type="datetime-local"
              value={createdBefore}
              onChange={(e) => setCreatedBefore(e.target.value)}
            />
          </div>
        </Field>

        <Field label="Method" className="w-[140px]">
          <Select value={methodKind} onChange={(e) => setMethodKind(e.target.value as 'grid' | 'random')}>
            <option value="grid">Full grid</option>
            <option value="random">Random N</option>
          </Select>
        </Field>

        {methodKind === 'random' && (
          <Field label="Samples (N)" className="w-[110px]">
            <Input
              type="number"
              min={1}
              value={randomN}
              onChange={(e) => setRandomN(Math.max(1, Number(e.target.value) || 1))}
            />
          </Field>
        )}

        <Field label="Min tokens / group" className="w-[140px]">
          <Input
            type="number"
            min={1}
            value={minTokens}
            onChange={(e) => setMinTokens(Math.max(1, Number(e.target.value) || 1))}
          />
        </Field>

        <Field label="Token cap" className="w-[120px]">
          <Input
            type="number"
            min={1}
            value={tokenCap}
            onChange={(e) => setTokenCap(Math.max(1, Number(e.target.value) || 1))}
          />
        </Field>

        <Field label="Curve only" className="w-fit">
          <label className="flex h-[34px] items-center gap-1.5 text-sm text-text-mid">
            <Checkbox checked={curveOnly} onChange={(e) => setCurveOnly(e.target.checked)} />
            <span>bonding curve trades</span>
          </label>
        </Field>

        <div className="ml-auto flex items-center gap-2.5">
          <Badge variant={overCap ? 'danger' : 'primary'} className="font-mono">
            ~{projected.toLocaleString()} combos/group
          </Badge>
          <Button
            variant="primary"
            onClick={handleRun}
            disabled={running || overCap}
            title={overCap ? `Over the ${MAX_COMBOS} combo cap — narrow the grid or use Random N` : 'Run the grouped sweep'}
          >
            {running ? 'Sweeping…' : 'Run grouped sweep'}
          </Button>
        </div>
      </div>

      {/* Group-by field picker */}
      <div className="mt-3 border-t border-white/10 pt-3">
        <span className="mb-1.5 block text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
          Group by (fingerprint) · selection order = compound key
        </span>
        <div className="flex flex-wrap gap-x-4 gap-y-1.5">
          {GROUP_FIELDS.map((f) => (
            <label key={f} className="flex items-center gap-1.5 text-sm text-text-mid">
              <Checkbox checked={groupBy.includes(f)} onChange={() => toggleGroupField(f)} />
              <span>{GROUP_FIELD_LABELS[f]}</span>
            </label>
          ))}
        </div>
        {groupBy.length === 0 && (
          <p className="mt-1 text-xs text-text-dim/70">
            No fields selected → one “ALL” group (a single global sweep).
          </p>
        )}
      </div>

      {/* Editable param grid */}
      <div className="mt-3 border-t border-white/10 pt-3">
        <span className="mb-1.5 block text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
          Param grid · comma-separated · “off” disables a nullable knob
        </span>
        <div className="grid grid-cols-2 gap-2 md:grid-cols-4">
          {TPSL2_AXES.map((a) => (
            <Field key={a.key} label={a.label}>
              <Input
                value={axesText[a.key] ?? ''}
                onChange={(e) => setAxesText((prev) => ({ ...prev, [a.key]: e.target.value }))}
                placeholder={a.nullable ? 'off, 20, 35' : '50, 100, 200'}
              />
            </Field>
          ))}
        </div>
      </div>
    </div>
  );
}

/** datetime-local ("YYYY-MM-DDTHH:MM", read as UTC) → RFC3339, or undefined. */
function toUtc(local: string): string | undefined {
  if (!local) return undefined;
  const d = new Date(local.endsWith('Z') ? local : `${local}Z`);
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString();
}
