import { useMemo, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { Checkbox } from 'components/ui/Checkbox';
import { Badge } from 'components/ui/Badge';
import {
  GROUP_FIELDS,
  GROUP_FIELD_LABELS,
  type AxisDef,
  type GroupField,
  type GroupedSweepStartArgs,
} from './groupedTypes';

/** Mirror of the backend `MAX_COMBOS` default — the per-group cap a run uses
 *  unless overridden in the form below. */
const DEFAULT_MAX_COMBOS = 5000;
/** Mirror of the backend `HARD_MAX_COMBOS` backstop — the form won't let the
 *  override exceed it (the backend clamps too). */
const HARD_MAX_COMBOS = 500000;

interface SweepConfigFormProps {
  strategyId: string;
  /** This strategy's editable param axes (e.g. `TPSL2_AXES` / `TPSL1_AXES`).
   *  Drives the param grid, the projected-combo math, and `buildAxes`. */
  axes: AxisDef[];
  /** localStorage key for this strategy's persisted form config, so each
   *  strategy's sweep page keeps its own grid/selection independently. */
  storageKey: string;
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

/** One labelled param-grid subsection (Entry / Exit), so the sweep grid groups
 *  the same way the TPSL2 rule modal does. */
function AxisGroup({
  title,
  hint,
  accent,
  axes,
  axesText,
  setAxesText,
  className,
}: {
  title: string;
  hint: string;
  accent: string;
  axes: AxisDef[];
  axesText: Record<string, string>;
  setAxesText: (fn: (prev: Record<string, string>) => Record<string, string>) => void;
  className?: string;
}) {
  return (
    <div className={className}>
      <span className="mb-1.5 flex items-baseline gap-1.5">
        <span className={cn('text-[10px] font-bold uppercase tracking-wider', accent)}>{title}</span>
        <span className="text-[9px] lowercase text-text-dim/50">{hint}</span>
      </span>
      <div className="grid grid-cols-2 gap-2">
        {axes.map((a) => (
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
  );
}

/** Render an axis default to its editable comma string (`null` → "off"). */
function axisToText(values: (number | null)[]): string {
  return values.map((v) => (v == null ? 'off' : String(v))).join(', ');
}

/** Default param-grid text for a strategy's axes (`null` → "off"). */
function defaultAxesText(axes: AxisDef[]): Record<string, string> {
  return Object.fromEntries(axes.map((a) => [a.key, axisToText(a.default)]));
}

/** The full sweep-form state — persisted per strategy under `mt:sweep.config[.id]`
 *  (replacing the former flat `sweep_cfg_*` keys). */
interface SweepConfig {
  createdAfter: string;
  createdBefore: string;
  groupBy: GroupField[];
  axesText: Record<string, string>;
  methodKind: 'grid' | 'random' | 'refine';
  randomN: number;
  /** Per-group survivors that seed the neighborhood in `refine` mode. */
  refineTopK: number;
  minTokens: number;
  tokenCap: number;
  maxCombos: number;
  curveOnly: boolean;
}

/** Default form state for a strategy — its `axesText` is prefilled from that
 *  strategy's axis defaults so the grid reads the same as its rule modal. */
function defaultSweepConfig(axes: AxisDef[]): SweepConfig {
  return {
    createdAfter: '',
    createdBefore: '',
    groupBy: ['cu_price'],
    axesText: defaultAxesText(axes),
    methodKind: 'grid',
    randomN: 500,
    refineTopK: 3,
    minTokens: 10,
    tokenCap: 10000,
    maxCombos: DEFAULT_MAX_COMBOS,
    curveOnly: false,
  };
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

export function SweepConfigForm({ strategyId, axes, storageKey, running, onRun }: SweepConfigFormProps) {
  // Defaults depend on this strategy's axes, so memoize per axis list.
  const DEFAULT_SWEEP_CONFIG = useMemo(() => defaultSweepConfig(axes), [axes]);
  // Whole form persisted as one object; merge over defaults so a future-added
  // field is never `undefined` when reading an older stored shape.
  const [stored, setConfig] = useLocalStorage<SweepConfig>(storageKey, DEFAULT_SWEEP_CONFIG);
  const config = { ...DEFAULT_SWEEP_CONFIG, ...stored };
  const {
    createdAfter,
    createdBefore,
    groupBy,
    axesText,
    methodKind,
    randomN,
    refineTopK,
    minTokens,
    tokenCap,
    maxCombos,
    curveOnly,
  } = config;

  /** Patch one config field (always writes back a complete object). */
  function setField<K extends keyof SweepConfig>(key: K, value: SweepConfig[K]) {
    setConfig((prev) => ({ ...DEFAULT_SWEEP_CONFIG, ...prev, [key]: value }));
  }
  const setCreatedAfter = (v: string) => setField('createdAfter', v);
  const setCreatedBefore = (v: string) => setField('createdBefore', v);
  const setMethodKind = (v: SweepConfig['methodKind']) => setField('methodKind', v);
  const setRandomN = (v: number) => setField('randomN', v);
  const setRefineTopK = (v: number) => setField('refineTopK', v);
  const setMinTokens = (v: number) => setField('minTokens', v);
  const setTokenCap = (v: number) => setField('tokenCap', v);
  const setMaxCombos = (v: number) => setField('maxCombos', v);
  const setCurveOnly = (v: boolean) => setField('curveOnly', v);
  const setAxesText = (fn: (prev: Record<string, string>) => Record<string, string>) =>
    setConfig((prev) => {
      const base = { ...DEFAULT_SWEEP_CONFIG, ...prev };
      return { ...base, axesText: fn(base.axesText) };
    });

  const entryAxes = useMemo(() => axes.filter((a) => a.group === 'entry'), [axes]);
  const exitAxes = useMemo(() => axes.filter((a) => a.group === 'exit'), [axes]);

  // Projected combos: grid = product of axis lengths; random/refine = the coarse
  // N (refine grows the union past N around survivors, but caps at the same cap).
  const projected = useMemo(() => {
    if (methodKind !== 'grid') return Math.max(1, randomN);
    return axes.reduce((acc, a) => acc * axisLen(axesText[a.key] ?? '', a), 1);
  }, [methodKind, randomN, axesText, axes]);

  // Effective per-group cap: what's typed, clamped to the backend's hard backstop.
  const effectiveCap = Math.min(Math.max(1, maxCombos || DEFAULT_MAX_COMBOS), HARD_MAX_COMBOS);
  const overCap = projected > effectiveCap;

  function toggleGroupField(f: GroupField) {
    setField('groupBy', groupBy.includes(f) ? groupBy.filter((x) => x !== f) : [...groupBy, f]);
  }

  function buildAxes(): GroupedSweepStartArgs['axes'] {
    const spec: Record<string, (number | null)[]> = {};
    for (const a of axes) {
      // Backend types: take_profit/stop_loss are number[]; the rest (number|null)[].
      const vals = parseAxis(axesText[a.key] ?? '', a.nullable);
      spec[a.key] = a.nullable ? vals : vals.filter((v): v is number => v !== null);
    }
    return spec as GroupedSweepStartArgs['axes'];
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
      method:
        methodKind === 'refine'
          ? `refine:${Math.max(1, randomN)}:${Math.max(1, refineTopK)}`
          : methodKind === 'random'
            ? `random:${Math.max(1, randomN)}`
            : 'grid',
      axes: buildAxes(),
      token_cap: tokenCap,
      // Only send an override when it differs from the default, so the backend
      // default stays authoritative otherwise.
      max_combos: effectiveCap !== DEFAULT_MAX_COMBOS ? effectiveCap : undefined,
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
          <Select
            value={methodKind}
            onChange={(e) => setMethodKind(e.target.value as SweepConfig['methodKind'])}
          >
            <option value="grid">Full grid</option>
            <option value="random">Random N</option>
            <option value="refine">Coarse → refine</option>
          </Select>
        </Field>

        {methodKind !== 'grid' && (
          <Field
            label={methodKind === 'refine' ? 'Coarse N' : 'Samples (N)'}
            className="w-[110px]"
          >
            <Input
              type="number"
              min={1}
              value={randomN}
              onChange={(e) => setRandomN(Math.max(1, Number(e.target.value) || 1))}
            />
          </Field>
        )}

        {methodKind === 'refine' && (
          <Field label="Top-K / group" hint="survivors refined" className="w-[120px]">
            <Input
              type="number"
              min={1}
              value={refineTopK}
              onChange={(e) => setRefineTopK(Math.max(1, Number(e.target.value) || 1))}
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

        <Field label="Max combos / group" hint={`≤ ${HARD_MAX_COMBOS.toLocaleString()}`} className="w-[140px]">
          <Input
            type="number"
            min={1}
            max={HARD_MAX_COMBOS}
            value={maxCombos}
            onChange={(e) =>
              setMaxCombos(Math.min(HARD_MAX_COMBOS, Math.max(1, Number(e.target.value) || 1)))
            }
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
            title={overCap ? `Over the ${effectiveCap.toLocaleString()} combo cap — narrow the grid, raise Max combos, or use Random N` : 'Run the grouped sweep'}
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

      {/* Editable param grid — split into entry/exit groups, ordered to match
          the TPSL2 rule modal so the two screens read the same. */}
      <div className="mt-3 border-t border-white/10 pt-3">
        <span className="mb-2 block text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
          Param grid · comma-separated · “off” disables a nullable knob
        </span>
        <div className="flex flex-col gap-3 md:flex-row md:gap-4">
          {/* Entry block only when this strategy has entry gates (TPSL1 has none). */}
          {entryAxes.length > 0 && (
            <AxisGroup
              title="Entry gates · scalp"
              hint="when to buy"
              accent="text-accent"
              axes={entryAxes}
              axesText={axesText}
              setAxesText={setAxesText}
              className="md:flex-1"
            />
          )}
          <AxisGroup
            title="Exit gates"
            hint="when to sell"
            accent="text-warning"
            axes={exitAxes}
            axesText={axesText}
            setAxesText={setAxesText}
            className={
              entryAxes.length > 0
                ? 'border-t border-white/10 pt-3 md:flex-1 md:border-l md:border-t-0 md:pl-4 md:pt-0'
                : 'md:flex-1'
            }
          />
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
