import { useEffect, useMemo, type ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { cn } from 'lib/cn';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { IconButton } from 'components/ui/IconButton';
import { LinkIcon, PlayIcon, SpinnerIcon } from 'components/ui/icons';
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
  SOL_BUCKET_WIDTH,
} from './groupedTypes';
import { parseNumbers, parseIxLabelsFilter } from './fingerprintFilters';
import { formatIxLabelsText } from 'lib/ixLabels';
import { FingerprintGroupPicker } from './FingerprintGroupPicker';
import { GenericAxisBuilder } from './GenericAxisBuilder';
import { VolumeIxPatternsEditor } from 'components/strategy/VolumeIxPatternsEditor';
import { fingerprintParamsCell } from 'components/strategy/FingerprintParamsSummary';
import { FINGERPRINT_FIELD_HELP } from 'lib/strategy/strategyHelp';
import { LabelTip } from 'components/strategy/LabelTip';
import { fingerprintsHref } from 'lib/strategy/nav';
import {
  COST_MODELS,
  FILL_MODELS,
  type CostModelId,
  type Fingerprint,
  type FillModelId,
} from 'lib/strategy/types';
import { useGetFingerprintsQuery } from 'store/sharedEndpoints';
import {
  axisRowError,
  comboCount,
  newAxisRow,
  serializeAxisRows,
  pnlAxisSugarDuplicateError,
  sharedWindowError,
  type AxisKind,
  type AxisSpecWire,
  type GenericAxisRow,
  type MetricAxisSide,
} from './genericAxes';
import { SWEEP_FIELD_HELP } from 'lib/strategy/strategyHelp';
import { tidySolDecimal } from 'utils/format';

/** Backend `MAX_COMBOS` default + `HARD_MAX_COMBOS` backstop (mirror). */
const DEFAULT_MAX_COMBOS = 100000;
const HARD_MAX_COMBOS = 1000000;

/** Mirror `registry::{DEFAULT,MAX}_TOKEN_CAP` — corpus newest-N load ceiling. */
const DEFAULT_TOKEN_CAP = 10000;
const MAX_TOKEN_CAP = 100000;

/**
 * Desktop RAM reserve choices (MB) — how much host RAM the run leaves free for
 * OS + desktop. Every admission ceiling the backend computes is
 * `host free − reserve`, so a smaller reserve admits bigger sweeps on a box
 * you're not using; a bigger one keeps the machine responsive while it runs.
 * Mirrors `registry::{DEFAULT,MIN,MAX}_SWEEP_RAM_RESERVE_MB` (values are clamped
 * server-side regardless).
 */
const RAM_RESERVE_CHOICES = [
  { mb: 4096, label: '4G' },
  { mb: 2048, label: '2G' },
  { mb: 1024, label: '1G' },
  { mb: 512, label: '512M' },
  { mb: 256, label: '256M' },
] as const;
/** Must equal `registry::DEFAULT_SWEEP_RAM_RESERVE_MB` (hunter/lab/src/sweep/registry.rs)
 *  — the form omits `ram_reserve_mb` from the request when it still matches, so a drift
 *  here silently pins every run to a stale reserve instead of the backend default. */
const DEFAULT_RAM_RESERVE_MB = 1024;

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
  /** Host RAM (MB) left free for OS + desktop while the run sizes its peaks. */
  ramReserveMb: number;
  /** Opt into the AVX-512 vectorized exit scan (lab-only; host-gated server-side). */
  useAvx512: boolean;
  /** Corpus-wide volume-ix patterns when axes reference m_flow_split /
   *  m_flow_split_window (not m_flow_lifetime / m_flow_window). */
  volumeIxPatterns: string[][];
  /** Which trade in the fill window prices each leg. Unlike the RAM/AVX knobs this
   *  changes the RESULT, so it is persisted on the run and shown on its header. */
  fillModel: FillModelId;
  /** Which execution-cost model prices the round-trips. */
  costModel: CostModelId;
  /** Saved fingerprint the corpus is scoped to (engine match SSOT). When set the
   *  manual value filters are not sent — the backend matches instead. */
  seedFingerprintId: string | null;
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
    tokenCap: DEFAULT_TOKEN_CAP,
    maxCombos: DEFAULT_MAX_COMBOS,
    curveOnly: false,
    buyAmountSol: 1.0,
    bucketWidthSol: SOL_BUCKET_WIDTH,
    ramReserveMb: DEFAULT_RAM_RESERVE_MB,
    // Default OFF: the scalar scan is the SSOT. Flip to `true` once the workstation
    // A/B (plan §P5) confirms the speedup on your corpus — the result is identical.
    useAvx512: false,
    volumeIxPatterns: [],
    // New runs default to the pair the fill-sensitivity analysis was measured under:
    // the next print after the signal, with slippage charged ONCE (in the fill price,
    // not again in the cost model). The old worst-case + fee+slippage pair is still
    // selectable, and stays the wire default so stored runs keep their meaning — but
    // it is the regime this strategy loses money in, so it is not what a fresh grid
    // should rank inside.
    fillModel: 'first_in_window',
    costModel: 'pumpfun_fee_only',
    seedFingerprintId: null,
  };
}

function axesReferenceFlow(rows: GenericAxisRow[]): boolean {
  return rows.some((r) => r.kind === 'metric' && (r.group === 'm_flow_split' || r.group === 'm_flow_split_window'));
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
    valuesText: (a.values ?? []).map((v) => (v == null ? 'off' : v)).join(', '),
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
      run.ix_labels_filter && run.ix_labels_filter.length > 0
        ? formatIxLabelsText(run.ix_labels_filter)
        : '',
    cashbackFilter,
    fieldFiltersText,
    axisRows: rows.length ? rows : defaults.axisRows,
    methodKind,
    randomN,
    refineTopK,
    minTokens: run.min_tokens,
    // Clamp legacy runs that stored a pre-clamp fat-finger (e.g. 1e6) so re-run
    // shows / sends the ceiling the backend actually honors.
    tokenCap: Math.min(MAX_TOKEN_CAP, Math.max(1, run.token_cap ?? defaults.tokenCap)),
    maxCombos: run.max_combos ?? defaults.maxCombos,
    curveOnly: run.curve_only,
    buyAmountSol: tidySolDecimal(run.buy_amount_sol ?? defaults.buyAmountSol),
    bucketWidthSol: tidySolDecimal(run.bucket_width_sol ?? defaults.bucketWidthSol),
    volumeIxPatterns: run.volume_ix_patterns ?? defaults.volumeIxPatterns,
    // Legacy rows (null) were computed under what the sweep hardcoded then — restore
    // THAT, not today's default, or a "re-run" would quietly reprice the comparison.
    fillModel: run.fill_model ?? 'worst_case',
    costModel: run.cost_model ?? 'pumpfun_default',
    // Restore the scope so a re-run sweeps the SAME matched slice — the manual
    // filters are NULL on a scoped run, so without this the re-run would silently
    // widen to the whole selection window.
    seedFingerprintId: run.fingerprint_id ?? null,
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
    // Sanitize stale localStorage / pre-clamp values (backend max is 100k).
    tokenCap: Math.min(MAX_TOKEN_CAP, Math.max(1, stored.tokenCap ?? DEFAULTS.tokenCap)),
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
    ramReserveMb,
    useAvx512,
    volumeIxPatterns,
    fillModel,
    costModel,
    seedFingerprintId,
  } = config;

  const { data: fingerprints = [] } = useGetFingerprintsQuery();
  const seedFp: Fingerprint | null =
    (seedFingerprintId && fingerprints.find((f) => f.id === seedFingerprintId)) || null;

  /** Scope the corpus to a saved fingerprint (engine match, server-side) — or clear
   *  back to manual group-by / filters. Selecting one drops the group-by selection
   *  (one "ALL" group over the matched tokens is the point of scoping) and the
   *  min-tokens floor, which is sized for wide multi-group runs and would silently
   *  drop a fingerprint that matches only a handful of tokens. Bucket width follows
   *  the fingerprint's own so a promoted rule keeps "swept = run". The value filters
   *  are left untouched but not sent — the note under the select says so. */
  function selectSeedFingerprint(id: string) {
    if (!id) {
      setField('seedFingerprintId', null);
      return;
    }
    const fp = fingerprints.find((f) => f.id === id);
    if (!fp) return;
    setConfig((prev) => ({
      ...DEFAULTS,
      ...prev,
      seedFingerprintId: fp.id,
      groupBy: [],
      minTokens: 1,
      bucketWidthSol: tidySolDecimal(
        fp.bucket_size_amount > 0 ? fp.bucket_size_amount : SOL_BUCKET_WIDTH,
      ),
    }));
  }

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
  // A scoped run never sends the label filter (the engine match replaces it), so a
  // stale parse error in that box must not block the run.
  const ixFilterError = !ixLabelsGrouped && !seedFingerprintId ? ixFilter.error : null;

  // Axis validity + projected combos.
  const rowErrors = useMemo(
    () => axisRows.map((r) => axisRowError(r, registry)),
    [axisRows, registry],
  );
  const windowErr = useMemo(() => sharedWindowError(axisRows, registry), [axisRows, registry]);
  const pnlSugarErr = useMemo(() => pnlAxisSugarDuplicateError(axisRows), [axisRows]);
  const axesValid =
    axisRows.length > 0 && rowErrors.every((e) => e == null) && !windowErr && !pnlSugarErr;
  const wireAxes: AxisSpecWire[] = useMemo(
    () => serializeAxisRows(axisRows, registry),
    [axisRows, registry],
  );
  const needsFlowPatterns = axesReferenceFlow(axisRows);
  const flowPatternsOk =
    !needsFlowPatterns ||
    volumeIxPatterns.some((p) => p.some((s) => s.trim().length > 0));

  const projected = useMemo(() => {
    if (methodKind !== 'grid') return Math.max(1, randomN);
    return comboCount(axisRows, registry);
  }, [methodKind, randomN, axisRows, registry]);

  const effectiveCap = Math.min(Math.max(1, maxCombos || DEFAULT_MAX_COMBOS), HARD_MAX_COMBOS);
  const overCap = projected > effectiveCap;
  const canRun = axesValid && !overCap && !running && !ixFilterError && flowPatternsOk;

  function toggleGroupField(f: GroupField) {
    setField('groupBy', groupBy.includes(f) ? groupBy.filter((x) => x !== f) : [...groupBy, f]);
  }

  function handleRun() {
    if (!canRun) return;
    // Scoped run: the backend matches on the fingerprint's own axes, so the manual
    // filters are not sent at all (sending them would only look like they applied —
    // the server ignores them, and the run row stores the fingerprint instead).
    const scoped = !!seedFingerprintId;
    const fieldFilters: Record<string, (number | boolean)[]> = {};
    if (!scoped) {
      for (const f of GROUP_FIELDS) {
        if (f === 'ix_labels' || f === 'is_cashback_enabled') continue;
        const nums = parseNumbers(fieldFiltersText[f] ?? '');
        if (nums.length > 0) fieldFilters[f] = nums;
      }
      if (cashbackFilter !== 'all') {
        fieldFilters['is_cashback_enabled'] = [cashbackFilter === 'true'];
      }
    }

    onRun({
      strategy_id: GENERIC_STRATEGY_ID,
      created_after: toUtc(createdAfter),
      created_before: toUtc(createdBefore),
      curve_only: curveOnly,
      group_by: groupBy,
      bucket_width_sol: bucketWidthSol,
      fingerprint_id: seedFingerprintId ?? undefined,
      ix_labels_filter:
        !scoped && !ixLabelsGrouped && ixFilter.labels ? ixFilter.labels : undefined,
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
      token_cap: Math.min(MAX_TOKEN_CAP, Math.max(1, tokenCap)),
      max_combos: effectiveCap !== DEFAULT_MAX_COMBOS ? effectiveCap : undefined,
      buy_amount_sol: buyAmountSol,
      // Always sent: these decide what the numbers MEAN, so leaving them to the
      // wire default (the legacy pair) would silently contradict the form.
      fill_model: fillModel,
      cost_model: costModel,
      ram_reserve_mb: ramReserveMb !== DEFAULT_RAM_RESERVE_MB ? ramReserveMb : undefined,
      // Omit-when-default (same shape as ram_reserve_mb): only send when opted in.
      use_avx512: useAvx512 ? true : undefined,
      volume_ix_patterns: needsFlowPatterns
        ? volumeIxPatterns
            .map((p) => p.map((s) => s.trim()).filter(Boolean))
            .filter((p) => p.length > 0)
        : undefined,
    });
  }

  const runTitle = overCap
    ? `Over the ${effectiveCap.toLocaleString()} combo cap — narrow the grid, raise Max combos, or use Random N`
    : !axesValid
      ? 'Fix the axes: every row needs a valid metric/operator and at least one value'
      : ixFilterError
        ? `Fix the instruction-label filter: ${ixFilterError}`
        : !flowPatternsOk
          ? 'Flow axes require at least one volume_ix_patterns row'
          : 'Run the grouped sweep';

  return (
    <div className="mb-4 bg-surface">
      <div className="flex flex-wrap items-end gap-3">
        <Field
          label="Created range"
          hint="UTC"
          desc="Only tokens created in this UTC window are swept. Leave either end empty for open-ended."
          className="w-fit"
        >
          <div className="flex items-center gap-1">
            <Input type="datetime-local" value={createdAfter} onChange={(e) => setField('createdAfter', e.target.value)} />
            <span className="text-[10px] text-text-dim/50">–</span>
            <Input type="datetime-local" value={createdBefore} onChange={(e) => setField('createdBefore', e.target.value)} />
          </div>
        </Field>

        <Field label="Method" desc={SWEEP_FIELD_HELP.method.body} className="w-[140px]">
          <Select value={methodKind} onChange={(e) => setField('methodKind', e.target.value as GenericSweepConfig['methodKind'])}>
            <option value="grid">Full grid</option>
            <option value="random">Random N</option>
            <option value="refine">Coarse → refine</option>
          </Select>
        </Field>

        {methodKind !== 'grid' && (
          <Field
            label={methodKind === 'refine' ? 'Coarse N' : 'Samples (N)'}
            desc={SWEEP_FIELD_HELP.samples.body}
            className="w-[110px]"
          >
            <Input type="number" min={1} value={randomN} onChange={(e) => setField('randomN', Math.max(1, Number(e.target.value) || 1))} />
          </Field>
        )}
        {methodKind === 'refine' && (
          <Field
            label="Top-K / group"
            hint="survivors refined"
            desc={SWEEP_FIELD_HELP.topK.body}
            className="w-[120px]"
          >
            <Input type="number" min={1} value={refineTopK} onChange={(e) => setField('refineTopK', Math.max(1, Number(e.target.value) || 1))} />
          </Field>
        )}

        <Field label="Min tokens / group" desc={SWEEP_FIELD_HELP.minTokens.body} className="w-[140px]">
          <Input type="number" min={1} value={minTokens} onChange={(e) => setField('minTokens', Math.max(1, Number(e.target.value) || 1))} />
        </Field>
        <Field
          label="Token cap"
          hint={`≤ ${MAX_TOKEN_CAP.toLocaleString()}`}
          desc={SWEEP_FIELD_HELP.tokenCap.body}
          className="w-[140px]"
        >
          <Input
            type="number"
            min={1}
            max={MAX_TOKEN_CAP}
            value={tokenCap}
            onChange={(e) =>
              setField(
                'tokenCap',
                Math.min(MAX_TOKEN_CAP, Math.max(1, Number(e.target.value) || 1)),
              )
            }
          />
        </Field>
        <Field
          label="Max combos / group"
          hint={`≤ ${HARD_MAX_COMBOS.toLocaleString()}`}
          desc={SWEEP_FIELD_HELP.maxCombos.body}
          className="w-[140px]"
        >
          <Input
            type="number"
            min={1}
            max={HARD_MAX_COMBOS}
            value={maxCombos}
            onChange={(e) => setField('maxCombos', Math.min(HARD_MAX_COMBOS, Math.max(1, Number(e.target.value) || 1)))}
          />
        </Field>
        <Field label="Buy amount (SOL)" hint="per trade" desc={SWEEP_FIELD_HELP.buyAmount.body} className="w-[140px]">
          <Input
            type="number"
            min={0.001}
            step={0.01}
            numeric
            numericValue={buyAmountSol}
            onNumericChange={(n) => setField('buyAmountSol', n == null ? 0.001 : Math.max(0.001, n))}
          />
        </Field>
        <Field
          label="Fill model"
          hint="per leg"
          desc={SWEEP_FIELD_HELP.fillModel.body}
          className="w-[150px]"
        >
          <Select
            value={fillModel}
            onChange={(e) => setField('fillModel', e.target.value as FillModelId)}
            title={FILL_MODELS.find((m) => m.id === fillModel)?.hint}
          >
            {FILL_MODELS.map((m) => (
              <option key={m.id} value={m.id} title={m.hint}>
                {m.label}
              </option>
            ))}
          </Select>
        </Field>
        <Field
          label="Cost model"
          hint="per round-trip"
          desc={SWEEP_FIELD_HELP.costModel.body}
          className="w-[150px]"
        >
          <Select
            value={costModel}
            onChange={(e) => setField('costModel', e.target.value as CostModelId)}
            title={COST_MODELS.find((m) => m.id === costModel)?.hint}
          >
            {COST_MODELS.map((m) => (
              <option key={m.id} value={m.id} title={m.hint}>
                {m.label}
              </option>
            ))}
          </Select>
        </Field>
        <Field
          label="RAM reserve"
          hint="kept free"
          desc={SWEEP_FIELD_HELP.ramReserve.body}
          className="w-fit"
        >
          <div role="radiogroup" aria-label="RAM reserve" className="flex h-[34px] items-center gap-1">
            {RAM_RESERVE_CHOICES.map((c) => (
              <button
                key={c.mb}
                type="button"
                role="radio"
                aria-checked={ramReserveMb === c.mb}
                onClick={() => setField('ramReserveMb', c.mb)}
                className={cn(
                  'rounded border px-2 py-1 font-mono text-[11px] transition-colors',
                  ramReserveMb === c.mb
                    ? 'border-accent/60 bg-accent/15 text-accent'
                    : 'border-white/10 text-text-dim hover:border-white/25 hover:text-text-mid',
                )}
              >
                {c.label}
              </button>
            ))}
          </div>
        </Field>
        <Field label="AVX-512" hint="exit scan" desc={SWEEP_FIELD_HELP.avx512.body} className="w-fit">
          <div role="radiogroup" aria-label="AVX-512 exit scan" className="flex h-[34px] items-center gap-1">
            {([['On', true], ['Off', false]] as const).map(([label, on]) => (
              <button
                key={label}
                type="button"
                role="radio"
                aria-checked={useAvx512 === on}
                onClick={() => setField('useAvx512', on)}
                className={cn(
                  'rounded border px-2 py-1 font-mono text-[11px] transition-colors',
                  useAvx512 === on
                    ? 'border-accent/60 bg-accent/15 text-accent'
                    : 'border-white/10 text-text-dim hover:border-white/25 hover:text-text-mid',
                )}
              >
                {label}
              </button>
            ))}
          </div>
        </Field>
        <Field label="Curve only" desc={SWEEP_FIELD_HELP.curveOnly.body} className="w-fit">
          <label className="flex h-[34px] items-center gap-1.5 text-sm text-text-mid">
            <Checkbox checked={curveOnly} onChange={(e) => setField('curveOnly', e.target.checked)} />
            <span>bonding curve trades</span>
          </label>
        </Field>

        <div className="ml-auto flex items-center gap-2.5">
          <Badge variant={overCap ? 'danger' : 'primary'} className="font-mono">
            ~{projected.toLocaleString()} combos/group
          </Badge>
          <IconButton
            variant="primary"
            size="lg"
            onClick={handleRun}
            disabled={!canRun}
            label={running ? 'Sweeping…' : 'Run sweep'}
            title={runTitle}
          >
            {running ? <SpinnerIcon /> : <PlayIcon />}
          </IconButton>
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
          {/* Saved-fingerprint scope — the engine-match path (same control the Flow
              discovery page uses). Set → the corpus is the fingerprint's matched
              token set and the value filters below are ignored; empty → manual
              group-by / filters select the corpus. */}
          <div className="mb-3 flex flex-col gap-2 rounded border border-white/8 p-3">
            <label className="flex min-w-[16rem] flex-col gap-1 text-[11px] text-text-dim">
              <LabelTip
                tip={SWEEP_FIELD_HELP.seedFingerprint}
                className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80"
              >
                Scope by saved fingerprint
              </LabelTip>
              <Select
                fieldSize="sm"
                value={seedFingerprintId ?? ''}
                onChange={(e) => selectSeedFingerprint(e.target.value)}
              >
                <option value="">Manual group-by / filters below</option>
                {fingerprints.map((f) => (
                  <option key={f.id} value={f.id}>
                    {f.name || f.id.slice(0, 8)}
                    {f.used_by != null ? ` · used by ${f.used_by}` : ''}
                  </option>
                ))}
              </Select>
            </label>
            {seedFp ? (
              <div className="flex flex-col gap-1.5">
                <div className="flex flex-wrap items-center gap-2">
                  <Link
                    to={fingerprintsHref(seedFp.id)}
                    className="inline-flex items-center gap-1 rounded-md hover:opacity-90"
                    title={`Open fingerprint “${seedFp.name}”`}
                  >
                    <Badge variant="info">engine match · {seedFp.name}</Badge>
                    <LinkIcon className="h-3.5 w-3.5 text-accent" />
                  </Link>
                  <span className="text-[10px] text-text-dim">
                    Only tokens this fingerprint matches are swept (exact axes exact,
                    SOL axes by bucket) — the value filters below are <b>not sent</b>.
                    Group-by still splits inside that slice.
                  </span>
                </div>
                {fingerprintParamsCell(seedFp)}
              </div>
            ) : (
              <p className="text-[10px] text-text-dim">
                Pick a fingerprint to sweep exactly the tokens it matches — or leave
                empty and select the corpus with the group-by / filters below.
              </p>
            )}
          </div>
          <FingerprintGroupPicker
            groupBy={groupBy}
            onToggleField={toggleGroupField}
            fieldFiltersText={fieldFiltersText}
            onSetFieldFilter={setFieldFilterText}
            cashbackFilter={cashbackFilter}
            onSetCashback={(v) => setField('cashbackFilter', v)}
            bucketWidthSol={bucketWidthSol}
            onSetBucketWidth={(n) => setField('bucketWidthSol', n <= 0 ? SOL_BUCKET_WIDTH : n)}
            ixLabelsText={ixLabelsFilter}
            onSetIxLabels={(v) => setField('ixLabelsFilter', v)}
            ixFilter={ixFilter}
            emptyHint={
              seedFingerprintId
                ? 'No fields selected → one "ALL" group over the fingerprint\'s matched tokens.'
                : 'No fields selected → one "ALL" group (a single global sweep).'
            }
          />
        </Accordion>
      </div>

      {/* Registry-driven axis builder (replaces the static per-strategy grid). */}
      <div className="mt-3 border-t border-white/10 pt-3">
        <Accordion title="Sweep axes · metric conditions + TP / SL" defaultOpen>
          <GenericAxisBuilder rows={axisRows} onChange={(rows) => setField('axisRows', rows)} projected={projected} />
        </Accordion>
      </div>

      {needsFlowPatterns && (
        <div className="mt-3 border-t border-white/10 pt-3">
          <Accordion title="Volume-ix patterns (flow axes)" defaultOpen>
            <div className="flex flex-col gap-2">
              <span className="text-[11px] text-text-dim">
                <LabelTip tip={FINGERPRINT_FIELD_HELP.volume_ix_patterns}>
                  Corpus-wide patterns for this run
                </LabelTip>
                {' — '}required when axes use m_flow_split / m_flow_split_window. Promote copies
                them into the fingerprint.
              </span>
              <VolumeIxPatternsEditor
                patterns={volumeIxPatterns}
                onChange={(p) => setField('volumeIxPatterns', p)}
                disabled={running}
              />
              {!flowPatternsOk && (
                <InlineAlert variant="error">
                  Add at least one non-empty volume-ix pattern before running.
                </InlineAlert>
              )}
            </div>
          </Accordion>
        </div>
      )}

      {ixFilterError && (
        <div className="mt-2">
          <InlineAlert variant="error">Fix the instruction-label filter: {ixFilterError}</InlineAlert>
        </div>
      )}
    </div>
  );
}
