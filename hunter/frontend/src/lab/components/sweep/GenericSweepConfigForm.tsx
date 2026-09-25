import { useEffect, useMemo, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { IconButton } from 'components/ui/IconButton';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import { PlayIcon, SpinnerIcon } from 'components/ui/icons';
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
import { buildFieldFilters, parseIxLabelsFilter } from './fingerprintFilters';
import {
  LAMPORTS_GROUP_FIELDS,
  GROUP_FIELD_LABELS,
  type PartitionSpec,
} from './groupedTypes';
import { formatIxLabelsText } from 'lib/ixLabels';
import { FingerprintGroupPicker } from './FingerprintGroupPicker';
import { GenericAxisBuilder } from './GenericAxisBuilder';
import { FingerprintScopeControl } from 'components/strategy/FingerprintScopeControl';
import { useFingerprintMatches } from '@lab/components/strategy/useFingerprintMatches';
import { tagNames, tagsFromJson, validateTags } from 'lib/strategy/tagsDoc';
import { RunTagsEditor, StagePlanEditor } from './SweepPlanEditors';
import { stagePlanErrors, stagesFromWire } from './stagePlan';
import {
  COST_MODELS,
  storedCostModel,
  FILL_MODELS,
  type CostModelId,
  type FillModelId,
} from 'lib/strategy/types';
import { useGetFingerprintsQuery } from 'store/sharedEndpoints';
import {
  axisRowError,
  axesSpecToRows,
  axisTagNames,
  comboCount,
  newAxisRow,
  serializeAxisRows,
  storedAxisRows,
  type AxisSpecWire,
  type GenericAxisRow,
} from './genericAxes';
import { SWEEP_FIELD_HELP } from 'lib/strategy/strategyHelp';
import { tidySolDecimal } from 'utils/format';
import { STORAGE_KEYS } from 'lib/storage';
import type { DiscoverySweepHandoff } from '@lab/lib/metricDiscoveryTypes';

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
  /** How each grouped field is partitioned, keyed by field tag. A field not named
   *  here is `{kind:'distinct'}` — one group per value.
   *
   *  Explicit edges, never a width: the windows a run scored over travel WITH the
   *  run, so the promoted rule and the dashboard read the same ones instead of
   *  three surfaces re-deriving them from one number. */
  partition: Record<string, PartitionSpec>;
  /** Host RAM (MB) left free for OS + desktop while the run sizes its peaks. */
  ramReserveMb: number;
  /** Opt into the AVX-512 vectorized exit scan (lab-only; host-gated server-side). */
  useAvx512: boolean;
  /** The run's tags document (wire `tags` JSON): what an axis `@tag` reads. Sent only
   *  when an axis reads a tag; Promote writes it onto the promoted fingerprint. */
  tags: Record<string, unknown>;
  /** The scope fingerprint whose tags were last loaded into `tags`, so picking one
   *  loads its tags once and a later edit is not overwritten. */
  tagsFromFingerprintId: string | null;
  /** Which trade in the fill window prices each leg. Unlike the RAM/AVX knobs this
   *  changes the RESULT, so it is persisted on the run and shown on its header. */
  fillModel: FillModelId;
  /** Which execution-cost model prices the round-trips. */
  costModel: CostModelId;
  /** Saved fingerprint the corpus is scoped to (engine match SSOT). When set the
   *  manual value filters are not sent — the backend matches instead. */
  seedFingerprintId: string | null;
  /** The Pass-2 stage plan: one wire `stages` array. Empty = Pass 2 off. */
  stagePlan: unknown[];
  /** Top-K combos per group Pass 2 re-scores. */
  stagePlansTopK: number;
}

function defaultConfig(): GenericSweepConfig {
  return {
    createdAfter: '',
    createdBefore: '',
    groupBy: ['cu_price'],
    ixLabelsFilter: '',
    cashbackFilter: 'all',
    fieldFiltersText: {},
    // A starter grid: TP x SL, plus a coin-age entry filter.
    axisRows: [
      { ...newAxisRow('metric', undefined, 'entry', { metric: 'm_state.age_sec' }), operator: '>', valuesText: '5, 10' },
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
    partition: {},
    ramReserveMb: DEFAULT_RAM_RESERVE_MB,
    // Default OFF: the scalar scan is the SSOT. Flip to `true` once the workstation
    // A/B (plan §P5) confirms the speedup on your corpus — the result is identical.
    useAvx512: false,
    tags: {},
    tagsFromFingerprintId: null,
    // New runs default to the pair the fill-sensitivity analysis was measured under:
    // the next print after the signal, with slippage charged ONCE (in the fill price,
    // not again in the cost model — the model that did that is retired).
    //
    // Cost is `pumpfun_impact` because a GRID is exactly where size-blindness hurts:
    // the fixed per-leg tip and our own footprint pull in opposite directions with
    // buy size, so a size-blind model does not merely shift every combo's PnL down,
    // it re-orders combos that fire at different rates.
    fillModel: 'first_in_window',
    costModel: 'pumpfun_impact',
    seedFingerprintId: null,
    stagePlan: [],
    stagePlansTopK: 3,
  };
}

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);

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
function runAxesSpecToRows(spec: unknown): GenericAxisRow[] {
  return axesSpecToRows(spec as { axes?: AxisSpecWire[] } | null | undefined);
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
  const rows = runAxesSpecToRows(run.axes_spec);
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
    // A NULL stored width means the run grouped on exact amounts.
    // The run's own partition, never the form's: re-running at a different one
    // would score windows the stored groups never showed.
    partition: Object.fromEntries(run.partition ?? []),
    // The run's own tags, marked as loaded for its scope so they are not replaced.
    tags: isObj(run.tags) ? run.tags : {},
    tagsFromFingerprintId: run.fingerprint_id ?? null,
    // Legacy rows (null fill) were computed under what the sweep hardcoded then —
    // restore THAT, not today's default, or a "re-run" would quietly reprice the
    // comparison.
    fillModel: run.fill_model ?? 'worst_case',
    // The cost half cannot be restored the same way: a run written under the deleted
    // flat-slippage model has no model to restore, so it comes back under
    // `pumpfun_impact`. That is also what you want — double-counted execution cost is
    // the reason to re-run it, not something to reproduce.
    costModel: storedCostModel(run.cost_model),
    // Restore the scope so a re-run sweeps the SAME matched slice — the manual
    // filters are NULL on a scoped run, so without this the re-run would silently
    // widen to the whole selection window.
    seedFingerprintId: run.fingerprint_id ?? null,
    // `stage_plans` is `Stage[][]` on the wire; the form authors one plan, so it
    // restores the first.
    stagePlan: Array.isArray(run.stage_plans?.[0]) ? run.stage_plans[0] : [],
    stagePlansTopK: run.stage_plans_top_k ?? defaults.stagePlansTopK,
  };
}

/**
 * Config form for the grouped sweep: the corpus / method / caps controls, the
 * `FingerprintGroupPicker`, the axes (`GenericAxisBuilder`, wire `AxisSpec[]`), the
 * run's tags and the Pass-2 stage plan.
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
  const [stored, setConfig] = useLocalStorage<GenericSweepConfig>(storageKey, DEFAULTS, {
    debounceMs: 400,
  });

  useEffect(() => {
    if (!reuseNonce || !reuseRun) return;
    setConfig(() => runToConfig(reuseRun, DEFAULTS));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reuseNonce]);

  // Apply a one-shot discovery → sweep seed written by MetricDiscoveryPage.
  useEffect(() => {
    try {
      const raw = sessionStorage.getItem(STORAGE_KEYS.sweepDiscoverySeed);
      if (!raw) return;
      sessionStorage.removeItem(STORAGE_KEYS.sweepDiscoverySeed);
      const handoff = JSON.parse(raw) as DiscoverySweepHandoff;
      if (!handoff?.seed?.axes || !Array.isArray(handoff.seed.axes)) return;
      const axes = handoff.includeOptional
        ? [...handoff.seed.axes, ...(handoff.seed.optional_axes ?? [])]
        : handoff.seed.axes;
      const rows = axesSpecToRows(axes);
      if (rows.length === 0) return;
      setConfig((prev) => ({
        ...DEFAULTS,
        ...prev,
        axisRows: rows,
        createdAfter: handoff.createdAfter || prev.createdAfter || DEFAULTS.createdAfter,
        createdBefore: handoff.createdBefore || prev.createdBefore || DEFAULTS.createdBefore,
        curveOnly: handoff.curveOnly ?? prev.curveOnly,
        tokenCap: Math.min(
          MAX_TOKEN_CAP,
          Math.max(1, handoff.tokenCap || prev.tokenCap || DEFAULTS.tokenCap),
        ),
        buyAmountSol: tidySolDecimal(handoff.buyAmountSol || prev.buyAmountSol || DEFAULTS.buyAmountSol),
        ixLabelsFilter: handoff.ixLabelsFilter || prev.ixLabelsFilter || '',
        // The tags the discovery run screened with, so a seeded `@tag` axis reads the
        // same trades; marked as loaded for the scope so its own tags do not replace
        // them. Without them, the scope fingerprint's tags load as usual.
        ...(isObj(handoff.tags)
          ? { tags: handoff.tags, tagsFromFingerprintId: handoff.fingerprintId ?? null }
          : {}),
        seedFingerprintId: handoff.fingerprintId ?? prev.seedFingerprintId,
        groupBy: handoff.fingerprintId ? [] : prev.groupBy ?? DEFAULTS.groupBy,
        minTokens: handoff.fingerprintId ? 1 : prev.minTokens ?? DEFAULTS.minTokens,
      }));
    } catch {
      /* ignore malformed handoff */
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const config: GenericSweepConfig = {
    ...DEFAULTS,
    ...stored,
    groupBy: (stored.groupBy ?? DEFAULTS.groupBy).filter((f): f is GroupField =>
      (GROUP_FIELDS as readonly string[]).includes(f),
    ),
    // Rows saved before the read replaced group / metric / window restore as the defaults.
    axisRows: storedAxisRows(stored.axisRows) ?? DEFAULTS.axisRows,
    tags: isObj(stored.tags) ? stored.tags : DEFAULTS.tags,
    stagePlan: Array.isArray(stored.stagePlan) ? stored.stagePlan : DEFAULTS.stagePlan,
    // Sanitize stale localStorage / pre-clamp values (backend max is 100k).
    tokenCap: Math.min(MAX_TOKEN_CAP, Math.max(1, stored.tokenCap ?? DEFAULTS.tokenCap)),
    // Same reason, different failure: `localStorage` outlives a deploy, so a saved grid
    // can still name a cost model this build deleted. The backend rejects an unknown one
    // outright, so an unsanitized value is a 400 the user cannot see or clear.
    costModel: storedCostModel(stored.costModel),
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
    partition,
    ramReserveMb,
    useAvx512,
    tags,
    tagsFromFingerprintId,
    fillModel,
    costModel,
    seedFingerprintId,
    stagePlan,
    stagePlansTopK,
  } = config;

  const { data: fingerprints = [] } = useGetFingerprintsQuery();
  const fingerprintsById = useMemo(() => {
    const map = new Map(fingerprints.map((f) => [f.id, f]));
    return map;
  }, [fingerprints]);
  const seedFp = seedFingerprintId
    ? fingerprintsById.get(seedFingerprintId)
    : undefined;
  const fpMatches = useFingerprintMatches(seedFingerprintId, seedFp?.name);

  // Picking a scope fingerprint (here, by a re-run or by a discovery handoff) loads its
  // tags into the run once, so an axis can read them.
  useEffect(() => {
    if (!seedFp || tagsFromFingerprintId === seedFp.id) return;
    setConfig((prev) => ({
      ...DEFAULTS,
      ...prev,
      tags: isObj(seedFp.tags) ? seedFp.tags : {},
      tagsFromFingerprintId: seedFp.id,
    }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seedFp, tagsFromFingerprintId]);

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
    const fp = fingerprintsById.get(id);
    if (!fp) return;
    setConfig((prev) => ({
      ...DEFAULTS,
      ...prev,
      seedFingerprintId: fp.id,
      groupBy: [],
      minTokens: 1,
      // One ALL group over the tokens this fingerprint matches — there is nothing
      // to partition, and the run is scoped by `seedFingerprintId` anyway.
      partition: {},
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
  const clearFieldFilters = () =>
    setConfig((prev) => ({
      ...DEFAULTS,
      ...prev,
      fieldFiltersText: {},
      cashbackFilter: 'all',
      ixLabelsFilter: '',
    }));

  const ixLabelsGrouped = groupBy.includes('ix_labels');
  const ixFilter = useMemo(() => parseIxLabelsFilter(ixLabelsFilter), [ixLabelsFilter]);
  // A scoped run never sends the label filter (the engine match replaces it), so a
  // stale parse error in that box must not block the run.
  const ixFilterError = !ixLabelsGrouped && !seedFingerprintId ? ixFilter.error : null;

  // Axis validity + projected combos.
  const definedTags = useMemo(() => tagNames(tags), [tags]);
  const axesValid = useMemo(
    () => axisRows.length > 0 && axisRows.every((r) => axisRowError(r, registry, definedTags) == null),
    [axisRows, registry, definedTags],
  );
  const wireAxes: AxisSpecWire[] = useMemo(() => serializeAxisRows(axisRows), [axisRows]);
  // The tags document is sent (and has to be valid) only when an axis reads a tag.
  const readTags = useMemo(() => axisTagNames(axisRows, registry), [axisRows, registry]);
  const tagsOk = readTags.length === 0 || validateTags(tagsFromJson(tags), registry).length === 0;
  const planOk = useMemo(
    () => stagePlanErrors(stagesFromWire(stagePlan), registry).length === 0,
    [stagePlan, registry],
  );

  const projected = useMemo(() => {
    if (methodKind !== 'grid') return Math.max(1, randomN);
    return comboCount(axisRows);
  }, [methodKind, randomN, axisRows]);

  // Per-field value filters, parsed once. Blocks Run on a bad entry exactly as an
  // ix-labels parse error does — a silently dropped SOL filter would run the whole
  // sweep over a corpus the operator thinks was narrowed.
  const fieldFilterParse = useMemo(
    () =>
      buildFieldFilters(fieldFiltersText, {
        fields: GROUP_FIELDS,
        bucketed: LAMPORTS_GROUP_FIELDS,
        cashback: cashbackFilter,
        labels: GROUP_FIELD_LABELS,
      }),
    [fieldFiltersText, cashbackFilter],
  );
  // Scoped runs ignore the manual filters entirely, so a stale parse error there
  // must not block the run.
  const fieldFilterError = seedFingerprintId ? null : fieldFilterParse.error;

  const effectiveCap = Math.min(Math.max(1, maxCombos || DEFAULT_MAX_COMBOS), HARD_MAX_COMBOS);
  const overCap = projected > effectiveCap;
  const canRun =
    axesValid && !overCap && !running && !ixFilterError && !fieldFilterError && tagsOk && planOk;

  function toggleGroupField(f: GroupField) {
    setField('groupBy', groupBy.includes(f) ? groupBy.filter((x) => x !== f) : [...groupBy, f]);
  }

  function handleRun() {
    if (!canRun) return;
    // Scoped run: the backend matches on the fingerprint's own axes, so the manual
    // filters are not sent at all (sending them would only look like they applied —
    // the server ignores them, and the run row stores the fingerprint instead).
    const scoped = !!seedFingerprintId;
    const fieldFilters = scoped ? {} : fieldFilterParse.filters;

    onRun({
      strategy_id: GENERIC_STRATEGY_ID,
      created_after: toUtc(createdAfter),
      created_before: toUtc(createdBefore),
      curve_only: curveOnly,
      group_by: groupBy,
      // Exact mode replaces the width outright (backend ignores it there).
      ...(Object.keys(partition).length > 0 ? { partition } : {}),
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
      // The backend `AxesRequest { axes: [...] }`.
      axes: { axes: wireAxes },
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
      tags: readTags.length > 0 ? tags : undefined,
      // `Stage[][]` on the wire; the form authors one plan, sent as its sole entry.
      stage_plans: stagePlan.length > 0 ? [stagePlan] : undefined,
      stage_plans_top_k: stagePlan.length > 0 ? Math.max(1, stagePlansTopK) : undefined,
    });
  }

  const runTitle = overCap
    ? `Over the ${effectiveCap.toLocaleString()} combo cap — narrow the grid, raise Max combos, or use Random N`
    : !axesValid
      ? 'Fix the axes: every row needs a valid read and at least one value'
      : ixFilterError
        ? `Fix the instruction-label filter: ${ixFilterError}`
        : fieldFilterError
          ? `Fix the value filter — ${fieldFilterError}`
          : !tagsOk
            ? 'Fix the run tags'
            : !planOk
              ? 'Fix the stage plan'
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
          <DateTimeRangePicker
            aria-label="Created range"
            zoneLabel="UTC"
            emptyLabel="All history"
            customPreset="custom"
            value={{ preset: 'custom', from: createdAfter, to: createdBefore }}
            onChange={({ from, to }) => {
              setField('createdAfter', from);
              setField('createdBefore', to);
            }}
          />
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
          <FingerprintScopeControl
            fingerprints={fingerprints}
            value={seedFingerprintId}
            onChange={selectSeedFingerprint}
            tip={SWEEP_FIELD_HELP.seedFingerprint}
            scopedDescription={
              <>
                Only tokens this fingerprint matches are swept (exact axes exact, SOL
                axes by bucket) — the value filters below are <b>not sent</b>. Group-by
                still splits inside that slice.
              </>
            }
            manualHint="Pick a fingerprint to sweep exactly the tokens it matches — or leave empty and select the corpus with the group-by / filters below."
            matchedCount={fpMatches.count}
            matchedCountLoading={fpMatches.countLoading}
            onViewMatches={fpMatches.openMatches}
            onRequestMatchCount={fpMatches.ensureCount}
          />
          {fpMatches.matchesModal}
          <FingerprintGroupPicker
            groupBy={groupBy}
            onToggleField={toggleGroupField}
            fieldFiltersText={fieldFiltersText}
            onSetFieldFilter={setFieldFilterText}
            onClearFilters={clearFieldFilters}
            cashbackFilter={cashbackFilter}
            onSetCashback={(v) => setField('cashbackFilter', v)}
            partition={partition}
            onSetPartition={(f, spec) =>
              setField('partition', { ...partition, [f]: spec })
            }
            ixLabelsText={ixLabelsFilter}
            onSetIxLabels={(v) => setField('ixLabelsFilter', v)}
            ixFilter={ixFilter}
            filtersDisabled={!!seedFingerprintId}
            emptyHint={
              seedFingerprintId
                ? 'No fields selected → one "ALL" group over the fingerprint\'s matched tokens.'
                : 'No fields selected → one "ALL" group (a single global sweep).'
            }
          />
        </Accordion>
      </div>

      <div className="mt-3 border-t border-white/10 pt-3">
        <Accordion title="Sweep axes · metric conditions + TP / SL" defaultOpen>
          <GenericAxisBuilder
            rows={axisRows}
            onChange={(rows) => setField('axisRows', rows)}
            tags={definedTags}
            projected={projected}
          />
        </Accordion>
      </div>

      <div className="mt-3 border-t border-white/10 pt-3">
        <Accordion title="Tags (what @tag reads)" defaultOpen={readTags.length > 0}>
          {registry ? (
            <RunTagsEditor
              tags={tags}
              onChange={(t) => setField('tags', t)}
              registry={registry}
              readTags={readTags}
              disabled={running}
            />
          ) : (
            <span className="text-[11px] text-text-dim/60">Loading strategy registry…</span>
          )}
        </Accordion>
      </div>

      <div className="mt-3 border-t border-white/10 pt-3">
        <Accordion title="Stage plan (Pass 2)" defaultOpen={stagePlan.length > 0}>
          {registry ? (
            <StagePlanEditor
              plan={stagePlan}
              onChange={(p) => setField('stagePlan', p)}
              topK={stagePlansTopK}
              onTopK={(k) => setField('stagePlansTopK', k)}
              registry={registry}
              disabled={running}
            />
          ) : (
            <span className="text-[11px] text-text-dim/60">Loading strategy registry…</span>
          )}
        </Accordion>
      </div>

      {ixFilterError && (
        <div className="mt-2">
          <InlineAlert variant="error">Fix the instruction-label filter: {ixFilterError}</InlineAlert>
        </div>
      )}
      {fieldFilterError && (
        <div className="mt-2">
          <InlineAlert variant="error">Fix the value filter — {fieldFilterError}</InlineAlert>
        </div>
      )}
    </div>
  );
}
