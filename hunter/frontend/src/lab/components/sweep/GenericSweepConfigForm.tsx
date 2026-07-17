import { useEffect, useMemo, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { Checkbox } from 'components/ui/Checkbox';
import { Badge } from 'components/ui/Badge';
import { Accordion } from 'components/ui/Accordion';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { InlineAlert } from 'components/ui/Modal';
import { useStrategyRegistry } from 'lib/strategy/registry';
import {
  GROUP_FIELDS,
  type GroupField,
  type GroupedSweepRunRecord,
  type GroupedSweepStartArgs,
} from './groupedTypes';
import { parseNumbers, parseIxLabelsFilter } from './fingerprintFilters';
import { FingerprintGroupPicker } from './FingerprintGroupPicker';
import { GenericAxisBuilder } from './GenericAxisBuilder';
import {
  axisRowError,
  comboCount,
  newAxisRow,
  serializeAxisRows,
  sharedWindowError,
  type AxisKind,
  type AxisSpecWire,
  type GenericAxisRow,
  type MetricAxisSide,
} from './genericAxes';

/** Backend `MAX_COMBOS` default + `HARD_MAX_COMBOS` backstop (mirror). */
const DEFAULT_MAX_COMBOS = 100000;
const HARD_MAX_COMBOS = 1000000;

/** The one strategy id the generic engine's sweep tables use. */
export const GENERIC_STRATEGY_ID = 'generic';

interface GenericSweepConfigFormProps {
  storageKey: string;
  running: boolean;
  onRun: (args: GroupedSweepStartArgs) => void;
  reuseNonce?: number;
  reuseRun?: GroupedSweepRunRecord | null;
}

function Field({
  label,
  hint,
  desc,
  className,
  children,
}: {
  label: string;
  hint?: string;
  desc?: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={cn('flex flex-col gap-1', className)}>
      <span className="flex items-center gap-1 text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
        {label}
        {desc && <InfoTooltip title={label} body={desc} />}
        {hint && <span className="font-normal normal-case tracking-normal text-text-dim/45">{hint}</span>}
      </span>
      {children}
    </div>
  );
}

/** Persisted form state (per storageKey). Axis rows carry their own client ids. */
interface GenericSweepConfig {
  createdAfter: string;
  createdBefore: string;
  groupBy: GroupField[];
  ixLabelsFilter: string;
  cashbackFilter: 'all' | 'true' | 'false';
  fieldFiltersText: Record<string, string>;
  axisRows: GenericAxisRow[];
  methodKind: 'grid' | 'random' | 'refine';
  randomN: number;
  refineTopK: number;
  minTokens: number;
  tokenCap: number;
  maxCombos: number;
  curveOnly: boolean;
  buyAmountSol: number;
  bucketWidthSol: number;
}

function defaultConfig(): GenericSweepConfig {
  return {
    createdAfter: '',
    createdBefore: '',
    groupBy: ['cu_price'],
    ixLabelsFilter: '',
    cashbackFilter: 'all',
    fieldFiltersText: {},
    // A sensible starter grid: TP × SL, plus a time-since-creation entry gate.
    axisRows: [
      { ...newAxisRow('metric'), side: 'entry', group: 'm_snapshot', metric: 'time', operator: '>', valuesText: '5, 10' },
      newAxisRow('take_profit'),
      newAxisRow('stop_loss'),
    ],
    methodKind: 'grid',
    randomN: 500,
    refineTopK: 3,
    minTokens: 10,
    tokenCap: 10000,
    maxCombos: DEFAULT_MAX_COMBOS,
    curveOnly: false,
    buyAmountSol: 1.0,
    bucketWidthSol: 0.1,
  };
}

function isoToLocalInput(iso: string | null): string {
  if (!iso) return '';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())}T${p(d.getUTCHours())}:${p(d.getUTCMinutes())}`;
}

function toUtc(local: string): string | undefined {
  if (!local) return undefined;
  const d = new Date(local.endsWith('Z') ? local : `${local}Z`);
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString();
}

function parseMethodTag(method: string): Pick<GenericSweepConfig, 'methodKind' | 'randomN' | 'refineTopK'> {
  const m = method.trim();
  if (m.startsWith('refine:')) {
    const [, n, k] = m.split(':');
    return { methodKind: 'refine', randomN: Math.max(1, Number(n) || 500), refineTopK: Math.max(1, Number(k) || 3) };
  }
  if (m.startsWith('random:') || m.startsWith('lhs:')) {
    const [, n] = m.split(':');
    return { methodKind: 'random', randomN: Math.max(1, Number(n) || 500), refineTopK: 3 };
  }
  return { methodKind: 'grid', randomN: 500, refineTopK: 3 };
}

/** Rebuild editor rows from a stored run's `axes_spec` (`{ axes: AxisSpec[] }`). */
function axesSpecToRows(spec: unknown): GenericAxisRow[] {
  const axes = (spec as { axes?: AxisSpecWire[] } | null | undefined)?.axes;
  if (!Array.isArray(axes)) return [];
  return axes.map((a) => ({
    ...newAxisRow((a.kind as AxisKind) ?? 'metric'),
    side: (a.side as MetricAxisSide) ?? 'entry',
    group: a.group ?? '',
    metric: a.metric ?? '',
    operator: a.operator ?? '>',
    window: a.window != null ? String(a.window) : '',
    valuesText: (a.values ?? []).join(', '),
  }));
}

function runToConfig(run: GroupedSweepRunRecord, defaults: GenericSweepConfig): GenericSweepConfig {
  const { methodKind, randomN, refineTopK } = parseMethodTag(run.method);
  const fieldFiltersText: Record<string, string> = {};
  let cashbackFilter: GenericSweepConfig['cashbackFilter'] = 'all';
  for (const [field, vals] of Object.entries(run.field_filters ?? {})) {
    if (field === 'is_cashback_enabled') {
      const v = vals[0];
      cashbackFilter = v === true ? 'true' : v === false ? 'false' : 'all';
    } else if (field !== 'ix_labels') {
      fieldFiltersText[field] = vals.join(', ');
    }
  }
  const rows = axesSpecToRows(run.axes_spec);
  return {
    ...defaults,
    createdAfter: isoToLocalInput(run.created_after),
    createdBefore: isoToLocalInput(run.created_before),
    groupBy: run.grouping_spec,
    ixLabelsFilter:
      run.ix_labels_filter && run.ix_labels_filter.length > 0 ? JSON.stringify(run.ix_labels_filter) : '',
    cashbackFilter,
    fieldFiltersText,
    axisRows: rows.length ? rows : defaults.axisRows,
    methodKind,
    randomN,
    refineTopK,
    minTokens: run.min_tokens,
    tokenCap: run.token_cap ?? defaults.tokenCap,
    maxCombos: run.max_combos ?? defaults.maxCombos,
    curveOnly: run.curve_only,
    buyAmountSol: run.buy_amount_sol ?? defaults.buyAmountSol,
    bucketWidthSol: run.bucket_width_sol ?? defaults.bucketWidthSol,
  };
}

/**
 * Config form for the generic-engine grouped sweep (redesign FE5.2). Reuses the
 * corpus/method/caps controls + `FingerprintGroupPicker`; the strategy-specific
 * param grid is replaced by the registry-driven `GenericAxisBuilder`, which emits
 * `AxisSpec[]` (`{side, group, metric, operator, values[, window]}`) instead of a
 * flat knob grid.
 */
export function GenericSweepConfigForm({
  storageKey,
  running,
  onRun,
  reuseNonce,
  reuseRun,
}: GenericSweepConfigFormProps) {
  const { data: registry } = useStrategyRegistry();
  const DEFAULTS = useMemo(() => defaultConfig(), []);
  const [stored, setConfig] = useLocalStorage<GenericSweepConfig>(storageKey, DEFAULTS);

  useEffect(() => {
    if (!reuseNonce || !reuseRun) return;
    setConfig(() => runToConfig(reuseRun, DEFAULTS));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reuseNonce]);

  const config: GenericSweepConfig = {
    ...DEFAULTS,
    ...stored,
    groupBy: (stored.groupBy ?? DEFAULTS.groupBy).filter((f): f is GroupField =>
      (GROUP_FIELDS as readonly string[]).includes(f),
    ),
    axisRows: stored.axisRows ?? DEFAULTS.axisRows,
  };
  const {
    createdAfter,
    createdBefore,
    groupBy,
    ixLabelsFilter,
    cashbackFilter,
    fieldFiltersText,
    axisRows,
    methodKind,
    randomN,
    refineTopK,
    minTokens,
    tokenCap,
    maxCombos,
    curveOnly,
    buyAmountSol,
    bucketWidthSol,
  } = config;

  function setField<K extends keyof GenericSweepConfig>(key: K, value: GenericSweepConfig[K]) {
    setConfig((prev) => ({ ...DEFAULTS, ...prev, [key]: value }));
  }
  const setFieldFilterText = (field: string, value: string) =>
    setConfig((prev) => {
      const base = { ...DEFAULTS, ...prev };
      return { ...base, fieldFiltersText: { ...base.fieldFiltersText, [field]: value } };
    });

  const ixLabelsGrouped = groupBy.includes('ix_labels');
  const ixFilter = useMemo(() => parseIxLabelsFilter(ixLabelsFilter), [ixLabelsFilter]);
  const ixFilterError = !ixLabelsGrouped ? ixFilter.error : null;

  // Axis validity + projected combos.
  const rowErrors = useMemo(
    () => axisRows.map((r) => axisRowError(r, registry)),
    [axisRows, registry],
  );
  const windowErr = useMemo(() => sharedWindowError(axisRows, registry), [axisRows, registry]);
  const axesValid = axisRows.length > 0 && rowErrors.every((e) => e == null) && !windowErr;
  const wireAxes: AxisSpecWire[] = useMemo(
    () => serializeAxisRows(axisRows, registry),
    [axisRows, registry],
  );

  const projected = useMemo(() => {
    if (methodKind !== 'grid') return Math.max(1, randomN);
    return comboCount(axisRows, registry);
  }, [methodKind, randomN, axisRows, registry]);

  const effectiveCap = Math.min(Math.max(1, maxCombos || DEFAULT_MAX_COMBOS), HARD_MAX_COMBOS);
  const overCap = projected > effectiveCap;
  const canRun = axesValid && !overCap && !running && !ixFilterError;

  function toggleGroupField(f: GroupField) {
    setField('groupBy', groupBy.includes(f) ? groupBy.filter((x) => x !== f) : [...groupBy, f]);
  }

  function handleRun() {
    if (!canRun) return;
    const fieldFilters: Record<string, (number | boolean)[]> = {};
    for (const f of GROUP_FIELDS) {
      if (f === 'ix_labels' || f === 'is_cashback_enabled') continue;
      const nums = parseNumbers(fieldFiltersText[f] ?? '');
      if (nums.length > 0) fieldFilters[f] = nums;
    }
    if (cashbackFilter !== 'all') fieldFilters['is_cashback_enabled'] = [cashbackFilter === 'true'];

    onRun({
      strategy_id: GENERIC_STRATEGY_ID,
      created_after: toUtc(createdAfter),
      created_before: toUtc(createdBefore),
      curve_only: curveOnly,
      group_by: groupBy,
      bucket_width_sol: bucketWidthSol,
      ix_labels_filter: !ixLabelsGrouped && ixFilter.labels ? ixFilter.labels : undefined,
      field_filters: Object.keys(fieldFilters).length > 0 ? fieldFilters : undefined,
      min_tokens: minTokens,
      method:
        methodKind === 'refine'
          ? `refine:${Math.max(1, randomN)}:${Math.max(1, refineTopK)}`
          : methodKind === 'random'
            ? `random:${Math.max(1, randomN)}`
            : 'grid',
      // Generic wire axes: `AxesRequest { axes: [...] }` (cast — same slot the
      // legacy per-strategy `AxesSpec` used; resolved by `strategy_id`).
      axes: { axes: wireAxes } as unknown as GroupedSweepStartArgs['axes'],
      token_cap: tokenCap,
      max_combos: effectiveCap !== DEFAULT_MAX_COMBOS ? effectiveCap : undefined,
      buy_amount_sol: buyAmountSol,
    });
  }

  const runTitle = overCap
    ? `Over the ${effectiveCap.toLocaleString()} combo cap — narrow the grid, raise Max combos, or use Random N`
    : !axesValid
      ? 'Fix the axes: every row needs a valid metric/operator and at least one value'
      : ixFilterError
        ? `Fix the instruction-label filter: ${ixFilterError}`
        : 'Run the grouped sweep';

  return (
    <div className="mb-4 bg-surface">
      <div className="flex flex-wrap items-end gap-3">
        <Field label="Created range" hint="UTC" className="w-fit">
          <div className="flex items-center gap-1">
            <Input type="datetime-local" value={createdAfter} onChange={(e) => setField('createdAfter', e.target.value)} />
            <span className="text-[10px] text-text-dim/50">–</span>
            <Input type="datetime-local" value={createdBefore} onChange={(e) => setField('createdBefore', e.target.value)} />
          </div>
        </Field>

        <Field label="Method" className="w-[140px]">
          <Select value={methodKind} onChange={(e) => setField('methodKind', e.target.value as GenericSweepConfig['methodKind'])}>
            <option value="grid">Full grid</option>
            <option value="random">Random N</option>
            <option value="refine">Coarse → refine</option>
          </Select>
        </Field>

        {methodKind !== 'grid' && (
          <Field label={methodKind === 'refine' ? 'Coarse N' : 'Samples (N)'} className="w-[110px]">
            <Input type="number" min={1} value={randomN} onChange={(e) => setField('randomN', Math.max(1, Number(e.target.value) || 1))} />
          </Field>
        )}
        {methodKind === 'refine' && (
          <Field label="Top-K / group" hint="survivors refined" className="w-[120px]">
            <Input type="number" min={1} value={refineTopK} onChange={(e) => setField('refineTopK', Math.max(1, Number(e.target.value) || 1))} />
          </Field>
        )}

        <Field label="Min tokens / group" className="w-[140px]">
          <Input type="number" min={1} value={minTokens} onChange={(e) => setField('minTokens', Math.max(1, Number(e.target.value) || 1))} />
        </Field>
        <Field label="Token cap" className="w-[120px]">
          <Input type="number" min={1} value={tokenCap} onChange={(e) => setField('tokenCap', Math.max(1, Number(e.target.value) || 1))} />
        </Field>
        <Field label="Max combos / group" hint={`≤ ${HARD_MAX_COMBOS.toLocaleString()}`} className="w-[140px]">
          <Input
            type="number"
            min={1}
            max={HARD_MAX_COMBOS}
            value={maxCombos}
            onChange={(e) => setField('maxCombos', Math.min(HARD_MAX_COMBOS, Math.max(1, Number(e.target.value) || 1)))}
          />
        </Field>
        <Field label="Buy amount (SOL)" hint="per trade" className="w-[140px]">
          <Input
            type="number"
            min={0.001}
            step={0.01}
            numeric
            numericValue={buyAmountSol}
            onNumericChange={(n) => setField('buyAmountSol', n == null ? 0.001 : Math.max(0.001, n))}
          />
        </Field>
        <Field label="Curve only" className="w-fit">
          <label className="flex h-[34px] items-center gap-1.5 text-sm text-text-mid">
            <Checkbox checked={curveOnly} onChange={(e) => setField('curveOnly', e.target.checked)} />
            <span>bonding curve trades</span>
          </label>
        </Field>

        <div className="ml-auto flex items-center gap-2.5">
          <Badge variant={overCap ? 'danger' : 'primary'} className="font-mono">
            ~{projected.toLocaleString()} combos/group
          </Badge>
          <Button variant="primary" onClick={handleRun} disabled={!canRun} title={runTitle}>
            {running ? 'Sweeping…' : 'Run grouped sweep'}
          </Button>
        </div>
      </div>

      {/* Group-by field picker + per-field value filters. */}
      <div className="mt-3 border-t border-white/10 pt-3">
        <Accordion
          title="Group by fingerprint"
          badge={
            <span
              className="cursor-default select-none text-[10px] text-text-dim/40 hover:text-text-dim/70"
              title="Selection order = compound key. Filter inputs restrict which tokens enter the sweep; they don't require the field to be checked."
            >
              ⓘ
            </span>
          }
        >
          <FingerprintGroupPicker
            groupBy={groupBy}
            onToggleField={toggleGroupField}
            fieldFiltersText={fieldFiltersText}
            onSetFieldFilter={setFieldFilterText}
            cashbackFilter={cashbackFilter}
            onSetCashback={(v) => setField('cashbackFilter', v)}
            bucketWidthSol={bucketWidthSol}
            onSetBucketWidth={(n) => setField('bucketWidthSol', n <= 0 ? 0.1 : n)}
            ixLabelsText={ixLabelsFilter}
            onSetIxLabels={(v) => setField('ixLabelsFilter', v)}
            ixFilter={ixFilter}
            emptyHint='No fields selected → one "ALL" group (a single global sweep).'
          />
        </Accordion>
      </div>

      {/* Registry-driven axis builder (replaces the static per-strategy grid). */}
      <div className="mt-3 border-t border-white/10 pt-3">
        <Accordion title="Sweep axes · metric conditions + TP / SL" defaultOpen>
          <GenericAxisBuilder rows={axisRows} onChange={(rows) => setField('axisRows', rows)} projected={projected} />
        </Accordion>
      </div>

      {ixFilterError && (
        <div className="mt-2">
          <InlineAlert variant="error">Fix the instruction-label filter: {ixFilterError}</InlineAlert>
        </div>
      )}
    </div>
  );
}
