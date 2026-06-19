import { useEffect, useMemo, useRef, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { Checkbox } from 'components/ui/Checkbox';
import { Badge } from 'components/ui/Badge';
import { Accordion } from 'components/ui/Accordion';
import {
  GROUP_FIELDS,
  GROUP_FIELD_LABELS,
  type AxisDef,
  type GroupField,
  type GroupedSweepRunRecord,
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
  /** Re-run: when this nonce increments (and a `reuseRun` is set), the form
   *  replaces its config with that run's stored settings. Keyed on the nonce
   *  (not the run) so a background refetch of the same run doesn't clobber edits. */
  reuseNonce?: number;
  /** The run whose config to apply when `reuseNonce` bumps. */
  reuseRun?: GroupedSweepRunRecord | null;
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
  /** Raw JSON-array text for the exact-set ix_labels corpus filter. Used only
   *  when the `ix_labels` group-by is OFF (the textarea is disabled otherwise). */
  ixLabelsFilter: string;
  /** `is_cashback_enabled` corpus filter: `"all"` = no filter, `"true"` = cashback
   *  only, `"false"` = no-cashback only. Sent via `field_filters`. */
  cashbackFilter: 'all' | 'true' | 'false';
  /** Comma-separated number text per numeric GroupField (e.g. `{ cu_price: "1000, 5000" }`).
   *  Empty string = no filter for that field. Not used for ix_labels or is_cashback_enabled. */
  fieldFiltersText: Record<string, string>;
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
    ixLabelsFilter: '',
    cashbackFilter: 'all',
    fieldFiltersText: {},
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

/** datetime-local string ("YYYY-MM-DDTHH:MM") from an RFC3339 UTC instant.
 *  Mirrors `toUtc` (which reads the input as UTC), so a re-run round-trips. */
function isoToLocalInput(iso: string | null): string {
  if (!iso) return '';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())}T${p(d.getUTCHours())}:${p(d.getUTCMinutes())}`;
}

/** Parse a stored `method` tag back into the form's method controls. `lhs:N`
 *  has no form control, so it maps to `random:N` (the closest editable shape). */
function parseMethodTag(method: string): Pick<SweepConfig, 'methodKind' | 'randomN' | 'refineTopK'> {
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

/** Map a saved run record into the form's editable config (re-run). Falls back to
 *  `defaults` for anything the run didn't store (legacy rows). */
function runToConfig(run: GroupedSweepRunRecord, axes: AxisDef[], defaults: SweepConfig): SweepConfig {
  const { methodKind, randomN, refineTopK } = parseMethodTag(run.method);
  // axes_spec → editable text per axis; default text for any axis it omits.
  const axesText = { ...defaultAxesText(axes) };
  const spec = run.axes_spec as Record<string, (number | null)[]> | null | undefined;
  if (spec) {
    for (const a of axes) {
      const vals = spec[a.key];
      if (Array.isArray(vals)) axesText[a.key] = axisToText(vals);
    }
  }
  // field_filters → per-field comma text + the cashback enum.
  const fieldFiltersText: Record<string, string> = {};
  let cashbackFilter: SweepConfig['cashbackFilter'] = 'all';
  for (const [field, vals] of Object.entries(run.field_filters ?? {})) {
    if (field === 'is_cashback_enabled') {
      const v = vals[0];
      cashbackFilter = v === true ? 'true' : v === false ? 'false' : 'all';
    } else if (field !== 'ix_labels') {
      fieldFiltersText[field] = vals.join(', ');
    }
  }
  return {
    ...defaults,
    createdAfter: isoToLocalInput(run.created_after),
    createdBefore: isoToLocalInput(run.created_before),
    groupBy: run.grouping_spec,
    ixLabelsFilter:
      run.ix_labels_filter && run.ix_labels_filter.length > 0 ? JSON.stringify(run.ix_labels_filter) : '',
    cashbackFilter,
    fieldFiltersText,
    axesText,
    methodKind,
    randomN,
    refineTopK,
    minTokens: run.min_tokens,
    tokenCap: run.token_cap ?? defaults.tokenCap,
    maxCombos: run.max_combos ?? defaults.maxCombos,
    curveOnly: run.curve_only,
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

/** Parse the ix_labels filter textarea (a JSON array of strings) into a label
 *  set, or an error message. Empty text ⇒ no filter (`null` labels, no error).
 *  Whitespace-only labels are dropped; an array that parses to no usable labels
 *  is treated as no filter so an empty `[]` doesn't accidentally pin "no labels". */
function parseIxLabelsFilter(text: string): { labels: string[] | null; error: string | null } {
  const t = text.trim();
  if (t === '') return { labels: null, error: null };
  let parsed: unknown;
  try {
    parsed = JSON.parse(t);
  } catch {
    return { labels: null, error: 'Invalid JSON' };
  }
  if (!Array.isArray(parsed) || !parsed.every((x) => typeof x === 'string')) {
    return { labels: null, error: 'Expected a JSON array of strings, e.g. ["Pump.Fun: Buy"]' };
  }
  const labels = (parsed as string[]).map((s) => s.trim()).filter((s) => s !== '');
  return { labels: labels.length > 0 ? labels : null, error: null };
}

/** Effective length of an axis for combo-count projection: what the user typed,
 *  else the backend default (an empty box falls back to the default server-side). */
function axisLen(text: string, def: AxisDef): number {
  const n = parseAxis(text, def.nullable).length;
  return n > 0 ? n : def.default.length;
}

/** Parse comma-separated number text into a deduped number array. Non-numbers are dropped. */
function parseNumbers(text: string): number[] {
  const seen = new Set<number>();
  const out: number[] = [];
  for (const s of text.split(',')) {
    const n = Number(s.trim());
    if (s.trim() && !isNaN(n) && !seen.has(n)) {
      seen.add(n);
      out.push(n);
    }
  }
  return out;
}

/** Units note shown at the bottom of each numeric field's filter tooltip. */
const FIELD_UNIT_HINTS: Partial<Record<GroupField, string>> = {
  cu_limit: 'Raw integer (e.g. 200000). Match values shown in group keys.',
  cu_price: 'Raw integer (e.g. 1000). Match values shown in group keys.',
  max_sol_cost: 'In lamports — 1 SOL = 1,000,000,000. Match values shown in group keys.',
  spendable_sol_in: 'In lamports — 1 SOL = 1,000,000,000. Match values shown in group keys.',
  initial_buy_sol: 'In SOL (e.g. 0.5, 1.0). Match values shown in group keys.',
};

/** Tooltip for a numeric field's filter input — explains the 3-state interaction. */
function fieldFilterTooltip(field: GroupField, isGrouped: boolean): string {
  const label = GROUP_FIELD_LABELS[field];
  const units = FIELD_UNIT_HINTS[field] ?? 'Comma-separated numbers.';
  const whenOn = isGrouped
    ? '☑ ON  + values here → only tokens matching a value enter the sweep,\n        then split into one group PER value (narrowed grouping).\n☑ ON  + empty       → all values included, each in its own group (default).'
    : '☐ OFF + values here → only tokens matching a value enter the sweep,\n        all lumped into ONE combined group.\n☐ OFF + empty       → no filter; all values flow into the sweep (default).';
  return [
    `Filter the corpus to specific ${label} values (comma-separated numbers).`,
    `Leave empty = all values pass through (no filter on this field).`,
    ``,
    whenOn,
    ``,
    `Units: ${units}`,
  ].join('\n');
}

/** Tooltip for a group-by checkbox. */
function groupByCheckboxTooltip(field: GroupField, isGrouped: boolean): string {
  const label = GROUP_FIELD_LABELS[field];
  if (isGrouped) {
    return `Grouping by ${label}.\nTokens are split into one group per distinct value; each group sweeps params independently.\n\nUncheck to stop splitting by this field.`;
  }
  return `Click to group by ${label}.\nChecked → tokens split into separate groups (one per distinct value), swept independently.\nUnchecked → this field is ignored for grouping (all values mixed together).\n\nYou can still filter to specific values using the input on the right.`;
}

export function SweepConfigForm({
  strategyId,
  axes,
  storageKey,
  running,
  onRun,
  reuseNonce,
  reuseRun,
}: SweepConfigFormProps) {
  // Defaults depend on this strategy's axes, so memoize per axis list.
  const DEFAULT_SWEEP_CONFIG = useMemo(() => defaultSweepConfig(axes), [axes]);
  // Whole form persisted as one object; merge over defaults so a future-added
  // field is never `undefined` when reading an older stored shape.
  const [stored, setConfig] = useLocalStorage<SweepConfig>(storageKey, DEFAULT_SWEEP_CONFIG);

  // Re-run: replace the form with a saved run's stored config when the nonce
  // bumps. Keyed only on the nonce so a background refetch of `reuseRun` (same
  // run, new object identity) never silently clobbers the user's edits.
  useEffect(() => {
    if (!reuseNonce || !reuseRun) return;
    setConfig(() => runToConfig(reuseRun, axes, DEFAULT_SWEEP_CONFIG));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reuseNonce]);
  const config = { ...DEFAULT_SWEEP_CONFIG, ...stored };
  const {
    createdAfter,
    createdBefore,
    groupBy,
    ixLabelsFilter,
    cashbackFilter,
    fieldFiltersText,
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
  const setIxLabelsFilter = (v: string) => setField('ixLabelsFilter', v);
  const setCashbackFilter = (v: SweepConfig['cashbackFilter']) => setField('cashbackFilter', v);
  const setFieldFilterText = (field: string, value: string) =>
    setConfig((prev) => {
      const base = { ...DEFAULT_SWEEP_CONFIG, ...prev };
      return { ...base, fieldFiltersText: { ...base.fieldFiltersText, [field]: value } };
    });
  const setAxesText = (fn: (prev: Record<string, string>) => Record<string, string>) =>
    setConfig((prev) => {
      const base = { ...DEFAULT_SWEEP_CONFIG, ...prev };
      return { ...base, axesText: fn(base.axesText) };
    });

  const entryAxes = useMemo(() => axes.filter((a) => a.group === 'entry'), [axes]);
  const exitAxes = useMemo(() => axes.filter((a) => a.group === 'exit'), [axes]);

  // The ix_labels group-by and the ix_labels corpus filter are mutually exclusive:
  // group by the label set, OR pin one set and sweep it. When grouping is on, the
  // filter textarea is disabled and never sent.
  const ixLabelsGrouped = groupBy.includes('ix_labels');
  const ixFilter = useMemo(() => parseIxLabelsFilter(ixLabelsFilter), [ixLabelsFilter]);
  // Only an active (grouping OFF) filter with a parse error blocks the run.
  const ixFilterError = !ixLabelsGrouped ? ixFilter.error : null;

  const ixLabelsRef = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    const el = ixLabelsRef.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${el.scrollHeight}px`;
  }, [ixLabelsFilter]);

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
    if (overCap || running || ixFilterError) return;
    // Build per-field value filters from the comma-separated text inputs.
    const fieldFilters: Record<string, (number | boolean)[]> = {};
    for (const f of GROUP_FIELDS) {
      if (f === 'ix_labels' || f === 'is_cashback_enabled') continue;
      const nums = parseNumbers(fieldFiltersText[f] ?? '');
      if (nums.length > 0) fieldFilters[f] = nums;
    }
    if (cashbackFilter !== 'all') {
      fieldFilters['is_cashback_enabled'] = [cashbackFilter === 'true'];
    }
    onRun({
      strategy_id: strategyId,
      created_after: toUtc(createdAfter),
      created_before: toUtc(createdBefore),
      curve_only: curveOnly,
      group_by: groupBy,
      // Send the exact-set filter only when grouping by ix_labels is OFF and the
      // textarea parsed to a non-empty label set.
      ix_labels_filter: !ixLabelsGrouped && ixFilter.labels ? ixFilter.labels : undefined,
      field_filters: Object.keys(fieldFilters).length > 0 ? fieldFilters : undefined,
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
    <div className="mb-4 bg-surface">
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
            disabled={running || overCap || !!ixFilterError}
            title={
              overCap
                ? `Over the ${effectiveCap.toLocaleString()} combo cap — narrow the grid, raise Max combos, or use Random N`
                : ixFilterError
                  ? `Fix the instruction-label filter: ${ixFilterError}`
                  : 'Run the grouped sweep'
            }
          >
            {running ? 'Sweeping…' : 'Run grouped sweep'}
          </Button>
        </div>
      </div>

      {/* Group-by field picker + per-field value filters */}
      <div className="mt-3 border-t border-white/10 pt-3">
        <Accordion
          title="Group by fingerprint"
          badge={
            <span
              className="cursor-default select-none text-[10px] text-text-dim/40 hover:text-text-dim/70"
              title="Selection order = compound key — the first checked field is the primary group, the second is secondary, etc. Filter inputs restrict which tokens enter the sweep; they don't require the field to be checked."
            >
              ⓘ
            </span>
          }
        >
          {/* Numeric fields — 2-col grid, each row: badge · checkbox · label · text filter */}
          <div className="grid grid-cols-2 gap-x-4 gap-y-1">
            {GROUP_FIELDS.filter((f) => f !== 'ix_labels' && f !== 'is_cashback_enabled').map((f) => {
              const isGrouped = groupBy.includes(f);
              const groupIndex = groupBy.indexOf(f);
              const filterText = fieldFiltersText[f] ?? '';
              const hasFilter = filterText.trim() !== '';
              return (
                <div key={f} className="flex min-w-0 items-center gap-1.5">
                  <span className="flex h-4 w-4 shrink-0 items-center justify-center">
                    {isGrouped && (
                      <span className="flex h-4 w-4 items-center justify-center rounded-full bg-accent/20 text-[9px] font-bold leading-none text-accent">
                        {groupIndex + 1}
                      </span>
                    )}
                  </span>
                  <label
                    className={cn(
                      'flex w-36 shrink-0 cursor-pointer items-center gap-1.5 text-sm',
                      isGrouped ? 'text-text-base' : 'text-text-mid',
                    )}
                    title={groupByCheckboxTooltip(f, isGrouped)}
                  >
                    <Checkbox checked={isGrouped} onChange={() => toggleGroupField(f)} />
                    <span className="whitespace-nowrap">{GROUP_FIELD_LABELS[f]}</span>
                  </label>
                  <input
                    type="text"
                    value={filterText}
                    onChange={(e) => setFieldFilterText(f, e.target.value)}
                    placeholder="all values"
                    title={fieldFilterTooltip(f, isGrouped)}
                    className="min-w-0 flex-1 rounded border border-white/10 bg-surface px-2 py-0.5 text-xs text-text-mid placeholder:text-text-dim/30 focus:border-white/25 focus:outline-none"
                  />
                  {hasFilter && (
                    <span
                      className="shrink-0 text-[10px] text-text-dim/60"
                      title={isGrouped ? 'Groups restricted to these values' : 'Corpus pinned to these values'}
                    >
                      {isGrouped ? 'filtered' : 'pinned'}
                    </span>
                  )}
                </div>
              );
            })}
          </div>

          {/* Enum fields — each on its own full-width row, below the numeric grid */}
          <div className="mt-1.5 flex flex-col gap-1.5">
            {/* Cashback — inline checkbox + select */}
            {(() => {
              const f = 'is_cashback_enabled' as const;
              const isGrouped = groupBy.includes(f);
              const groupIndex = groupBy.indexOf(f);
              return (
                <div className="flex min-w-0 items-center gap-1.5">
                  <span className="flex h-4 w-4 shrink-0 items-center justify-center">
                    {isGrouped && (
                      <span className="flex h-4 w-4 items-center justify-center rounded-full bg-accent/20 text-[9px] font-bold leading-none text-accent">
                        {groupIndex + 1}
                      </span>
                    )}
                  </span>
                  <label
                    className={cn(
                      'flex w-36 shrink-0 cursor-pointer items-center gap-1.5 text-sm',
                      isGrouped ? 'text-text-base' : 'text-text-mid',
                    )}
                    title={groupByCheckboxTooltip(f, isGrouped)}
                  >
                    <Checkbox checked={isGrouped} onChange={() => toggleGroupField(f)} />
                    <span className="whitespace-nowrap">{GROUP_FIELD_LABELS[f]}</span>
                  </label>
                  <select
                    value={cashbackFilter}
                    onChange={(e) => setCashbackFilter(e.target.value as SweepConfig['cashbackFilter'])}
                    title={
                      isGrouped
                        ? 'Filter the corpus by Cashback on value.\n\n☑ ON  + cashback only → only cashback=true tokens enter the sweep, in their own group.\n☑ ON  + no cashback  → only cashback=false tokens, in their own group.\n☑ ON  + all          → all tokens included, split into true/false groups (default).'
                        : 'Filter the corpus by Cashback on value.\n\n☐ OFF + cashback only → only cashback=true tokens enter the sweep, all in ONE group.\n☐ OFF + no cashback  → only cashback=false tokens, all in ONE group.\n☐ OFF + all          → no filter; all tokens flow into the sweep (default).'
                    }
                    className="w-36 rounded border border-white/10 bg-surface px-2 py-0.5 text-xs text-text-mid focus:border-white/25 focus:outline-none"
                  >
                    <option value="all">all</option>
                    <option value="true">cashback only</option>
                    <option value="false">no cashback</option>
                  </select>
                </div>
              );
            })()}

            {/* Instruction labels — checkbox row, then textarea below it */}
            {(() => {
              const f = 'ix_labels' as const;
              const isGrouped = groupBy.includes(f);
              const groupIndex = groupBy.indexOf(f);
              return (
                <div className="flex flex-col gap-0.5">
                  <div className="flex min-w-0 items-center gap-1.5">
                    <span className="flex h-4 w-4 shrink-0 items-center justify-center">
                      {isGrouped && (
                        <span className="flex h-4 w-4 items-center justify-center rounded-full bg-accent/20 text-[9px] font-bold leading-none text-accent">
                          {groupIndex + 1}
                        </span>
                      )}
                    </span>
                    <label
                      className={cn(
                        'flex w-36 shrink-0 cursor-pointer items-center gap-1.5 text-sm',
                        isGrouped ? 'text-text-base' : 'text-text-mid',
                      )}
                      title={groupByCheckboxTooltip(f, isGrouped)}
                    >
                      <Checkbox checked={isGrouped} onChange={() => toggleGroupField(f)} />
                      <span className="whitespace-nowrap">{GROUP_FIELD_LABELS[f]}</span>
                    </label>
                    {!ixLabelsGrouped && ixFilter.labels && (
                      <span className="text-[10px] text-text-dim/60">
                        {ixFilter.labels.length} label{ixFilter.labels.length !== 1 ? 's' : ''} pinned
                      </span>
                    )}
                  </div>
                  <div className="ml-5 flex flex-col gap-0.5">
                    <textarea
                      ref={ixLabelsRef}
                      rows={1}
                      value={ixLabelsFilter}
                      disabled={ixLabelsGrouped}
                      onChange={(e) => setIxLabelsFilter(e.target.value)}
                      placeholder='["Pump.Fun: Create"]'
                      title={
                        ixLabelsGrouped
                          ? 'Disabled: grouping by instruction labels. Uncheck to pin a specific label set here.'
                          : 'Filter the corpus to tokens whose instruction-label set exactly matches this JSON array.\nLeave empty = all label sets included.'
                      }
                      className="w-full resize-none overflow-hidden rounded border border-white/10 bg-surface px-2 py-1 font-mono text-xs text-text-mid placeholder:text-text-dim/30 focus:border-white/25 focus:outline-none disabled:cursor-not-allowed disabled:opacity-40"
                    />
                    {ixFilterError && (
                      <span className="text-[10px] text-danger">{ixFilterError}</span>
                    )}
                  </div>
                </div>
              );
            })()}
          </div>

          {groupBy.length === 0 && (
            <p className="mt-1.5 text-xs text-text-dim/70">
              No fields selected → one "ALL" group (a single global sweep).
            </p>
          )}
        </Accordion>
      </div>

      {/* Editable param grid — split into entry/exit groups, ordered to match
          the TPSL2 rule modal so the two screens read the same. */}
      <div className="mt-3 border-t border-white/10 pt-3">
        <Accordion title='Param grid · comma-separated · "off" disables a nullable knob'>
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
        </Accordion>
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
