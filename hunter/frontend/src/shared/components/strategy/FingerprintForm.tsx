import { useMemo, useState } from 'react';

import { Input } from 'components/ui/Input';
import { IconButton } from 'components/ui/IconButton';
import { SaveIcon, SpinnerIcon } from 'components/ui/icons';
import { Button } from 'components/ui/Button';
import { Checkbox } from 'components/ui/Checkbox';
import { IxLabelsInput } from 'components/ui/IxLabelsInput';
import { configuredIxLabels, formatIxLabelsText, parseIxLabelsText } from 'lib/ixLabels';
import {
  lamportsToSol,
  solToLamports,
  MIN_BUCKET_WIDTH_SOL,
  type Fingerprint,
  type FingerprintDraft,
} from 'lib/strategy/types';
import {
  groupsWithFingerprintConfig,
  metricConfigWithVolumePatterns,
  useStrategyRegistry,
  volumeIxPatternsFromConfig,
} from 'lib/strategy/registry';
import { FINGERPRINT_FIELD_HELP } from 'lib/strategy/strategyHelp';
import { tidySolDecimal } from 'utils/format';
import { LabelTip } from './LabelTip';
import { VolumeIxPatternsEditor } from './VolumeIxPatternsEditor';

export interface FingerprintFormProps {
  /** Existing fingerprint to edit; omit to create. */
  initial?: Fingerprint;
  onSubmit: (draft: FingerprintDraft) => void;
  onCancel?: () => void;
  submitting?: boolean;
  error?: string | null;
}

interface FormState {
  name: string;
  cu_limit: number | null;
  cu_price: number | null;
  init_buy_sol: number | null;
  max_cost_sol: number | null;
  spendable_sol: number | null;
  first_slot_buy_sol: number | null;
  first_slot_sell_sol: number | null;
  bucket_size_amount: number | null;
  /** Exact-SOL matching: the fingerprint pins each SOL axis to its exact lamports
   *  amount instead of a bucket. Separate from the width so "exact" is a named
   *  mode and never a magic 0 -- the width input keeps its value while disabled. */
  exact_sol: boolean;
  /** Textarea text — pretty JSON string array (see `parseIxLabelsText`). */
  ix_labels: string;
  /** `m_flow_split.volume_ix_patterns` rows (other metric_config keys preserved on save). */
  volume_ix_patterns: string[][];
  /** Original metric_config minus flow key — merged back on save. */
  metric_config_rest: Record<string, unknown>;
}

function fromFingerprint(fp?: Fingerprint): FormState {
  const cfg = fp?.metric_config ?? {};
  const { m_flow_split: _flow, ...rest } = cfg;
  return {
    name: fp?.name ?? '',
    cu_limit: fp?.cu_limit ?? null,
    cu_price: fp?.cu_price ?? null,
    init_buy_sol: lamportsToSol(fp?.init_buy_lamports),
    max_cost_sol: lamportsToSol(fp?.max_cost_lamports),
    spendable_sol: lamportsToSol(fp?.spendable_lamports_in),
    first_slot_buy_sol: lamportsToSol(fp?.first_slot_buy_lamports),
    first_slot_sell_sol: lamportsToSol(fp?.first_slot_sell_lamports),
    bucket_size_amount: tidySolDecimal(fp?.bucket_size_amount ?? 0.1),
    // A stored NULL width IS the exact mode (see Rust `SolPrecision::from_width`).
    exact_sol: fp != null && fp.bucket_size_amount == null,
    ix_labels: formatIxLabelsText(fp?.ix_labels),
    volume_ix_patterns: volumeIxPatternsFromConfig(cfg),
    metric_config_rest: rest,
  };
}

function toDraft(s: FormState): FingerprintDraft {
  const { labels } = parseIxLabelsText(s.ix_labels);
  const flow = metricConfigWithVolumePatterns(s.volume_ix_patterns);
  return {
    name: s.name.trim(),
    cu_limit: s.cu_limit,
    cu_price: s.cu_price,
    init_buy_lamports: solToLamports(s.init_buy_sol),
    max_cost_lamports: solToLamports(s.max_cost_sol),
    spendable_lamports_in: solToLamports(s.spendable_sol),
    first_slot_buy_lamports: solToLamports(s.first_slot_buy_sol),
    first_slot_sell_lamports: solToLamports(s.first_slot_sell_sol),
    // Exact wins outright and sends NULL — never a 0 width.
    bucket_size_amount: s.exact_sol ? null : tidySolDecimal(s.bucket_size_amount ?? 0.1),
    ix_labels: labels,
    metric_config: { ...s.metric_config_rest, ...flow },
  };
}

/**
 * How many match criteria the current form configures — must agree with the
 * backend `Fingerprint::has_any_criterion`, which rejects a criterion-less draft.
 *
 * `parsedLabels` is the *parsed* label list, never the raw textarea text: text
 * like `[]` or `,` is non-empty but parses to no labels, and the backend folds
 * an empty list to `None`. Counting the raw text enabled Create on a draft the
 * server then rejected with a 400.
 */
function criterionCount(s: FormState, parsedLabels: string[] | null): number {
  const axes = [
    s.cu_limit,
    s.cu_price,
    s.init_buy_sol,
    s.max_cost_sol,
    s.spendable_sol,
    s.first_slot_buy_sol,
    s.first_slot_sell_sol,
  ];
  // `v != null`, NOT truthiness — a `0` axis is a real bucket (`[0, width)`), so
  // it counts. Only "not set" (`null`) drops out of the fingerprint's identity.
  return axes.filter((v) => v != null).length + (configuredIxLabels(parsedLabels) ? 1 : 0);
}

/** Bucket-width problem, or `null` when the width is usable. Mirrors the backend
 *  `Fingerprint::validate` so the form fails fast instead of on a 400. */
function bucketWidthError(width: number | null): string | null {
  if (width == null) return null; // blank ⇒ the 0.1 default is applied on save
  if (!Number.isFinite(width) || width < MIN_BUCKET_WIDTH_SOL) {
    return `bucket width must be at least ${MIN_BUCKET_WIDTH_SOL}◎ — 0 is not "no bucketing", it collapses every amount into one bucket`;
  }
  return null;
}

/**
 * Create / edit a fingerprint. SOL-denominated amount inputs convert to lamports
 * at the API boundary (the wire is lamports); `bucket_size_amount` stays SOL.
 * Blocks submit until at least one match criterion is set (backend contract).
 */
export function FingerprintForm({
  initial,
  onSubmit,
  onCancel,
  submitting,
  error,
}: FingerprintFormProps) {
  const [s, setS] = useState<FormState>(() => fromFingerprint(initial));
  const set = <K extends keyof FormState>(k: K, v: FormState[K]) => setS((p) => ({ ...p, [k]: v }));
  const { data: registry } = useStrategyRegistry();
  const fpConfigGroups = groupsWithFingerprintConfig(registry);

  const ixParsed = useMemo(() => parseIxLabelsText(s.ix_labels), [s.ix_labels]);
  const criteria = criterionCount(s, ixParsed.labels);
  const nameOk = s.name.trim().length > 0;
  // In exact mode the width is unused, so a stale value must not block submit.
  const widthError = s.exact_sol ? null : bucketWidthError(s.bucket_size_amount);
  const canSubmit = criteria > 0 && nameOk && !submitting && !ixParsed.error && !widthError;

  const solField = (label: string, key: keyof FormState, tip: (typeof FINGERPRINT_FIELD_HELP)[keyof typeof FINGERPRINT_FIELD_HELP]) => (
    <label className="flex flex-col gap-1 text-[11px] text-text-dim">
      <LabelTip tip={tip}>{label}</LabelTip>
      <Input
        fieldSize="sm"
        numeric
        unit="◎"
        numericValue={s[key] as number | null}
        onNumericChange={(n) => set(key, n as FormState[typeof key])}
      />
    </label>
  );

  return (
    <div className="flex flex-col gap-3">
      <label className="flex flex-col gap-1 text-[11px] text-text-dim">
        <LabelTip tip={FINGERPRINT_FIELD_HELP.name}>Name</LabelTip>
        <Input
          fieldSize="sm"
          value={s.name}
          onChange={(e) => set('name', e.target.value)}
          placeholder="e.g. 3.5◎ create_v2 6-ix"
        />
      </label>

      <div className="grid grid-cols-2 gap-2">
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={FINGERPRINT_FIELD_HELP.cu_limit}>cu_limit (exact)</LabelTip>
          <Input
            fieldSize="sm"
            numeric
            integer
            numericValue={s.cu_limit}
            onNumericChange={(n) => set('cu_limit', n)}
          />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={FINGERPRINT_FIELD_HELP.cu_price}>cu_price (exact)</LabelTip>
          <Input
            fieldSize="sm"
            numeric
            integer
            numericValue={s.cu_price}
            onNumericChange={(n) => set('cu_price', n)}
          />
        </label>
      </div>

      <div className="grid grid-cols-2 gap-2">
        {solField('init_buy', 'init_buy_sol', FINGERPRINT_FIELD_HELP.init_buy)}
        {solField('max_cost', 'max_cost_sol', FINGERPRINT_FIELD_HELP.max_cost)}
        {solField('spendable_in', 'spendable_sol', FINGERPRINT_FIELD_HELP.spendable_in)}
        {solField('first_slot_buy', 'first_slot_buy_sol', FINGERPRINT_FIELD_HELP.first_slot_buy)}
        {solField('first_slot_sell', 'first_slot_sell_sol', FINGERPRINT_FIELD_HELP.first_slot_sell)}
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={FINGERPRINT_FIELD_HELP.bucket}>bucket width (◎)</LabelTip>
          <Input
            fieldSize="sm"
            numeric
            unit="◎"
            disabled={s.exact_sol}
            title={s.exact_sol ? 'Ignored while matching exact amounts' : undefined}
            className={widthError ? 'border-red/60' : undefined}
            numericValue={s.bucket_size_amount}
            onNumericChange={(n) => set('bucket_size_amount', n)}
          />
          {/* A named mode, not a `0` width — `0` divides by zero in `bucket_index`
              and would collapse every amount into one bucket, arming on any value. */}
          <label
            className="flex cursor-pointer items-center gap-1.5 text-[11px] text-text-mid"
            title={
              'Match each SOL axis on its EXACT lamports amount instead of a bucket range.\n\n' +
              'Bucketed (default): max_cost 1.515◎ at 0.1 arms on anything in [1.5, 1.6).\n' +
              'Exact: arms only on exactly 1.515◎.\n\n' +
              'Stored as a NULL bucket width. Affects the LIVE entry gate.'
            }
          >
            <Checkbox
              checked={s.exact_sol}
              onChange={() => set('exact_sol', !s.exact_sol)}
            />
            <span className="whitespace-nowrap">exact amounts</span>
          </label>
        </label>
      </div>

      <label className="flex flex-col gap-1 text-[11px] text-text-dim">
        <LabelTip tip={FINGERPRINT_FIELD_HELP.ix_labels}>
          ix_labels (exact ordered sequence, JSON array)
        </LabelTip>
        <IxLabelsInput
          value={s.ix_labels}
          onValueChange={(v) => set('ix_labels', v)}
          error={ixParsed.error}
        />
      </label>

      {fpConfigGroups.some((g) =>
        (g.fingerprint_config ?? []).some((f) => f.name === 'volume_ix_patterns'),
      ) && (
        <div className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={FINGERPRINT_FIELD_HELP.volume_ix_patterns}>
            volume_ix_patterns (m_flow_split)
          </LabelTip>
          <VolumeIxPatternsEditor
            patterns={s.volume_ix_patterns}
            onChange={(p) => set('volume_ix_patterns', p)}
            disabled={submitting}
          />
        </div>
      )}

      <div className="flex items-center justify-between">
        <span className="text-[11px] text-text-dim/80">
          {widthError ? (
            <span className="text-red">{widthError}</span>
          ) : criteria === 0 ? (
            <span className="text-red">needs ≥1 match criterion</span>
          ) : (
            `${criteria} criterion${criteria === 1 ? '' : 'a'} · ${
              s.exact_sol ? 'matched on exact amounts' : `matched by ${s.bucket_size_amount ?? 0.1}◎ bucket`
            }`
          )}
        </span>
        <div className="flex gap-2">
          {onCancel && (
            <Button variant="ghost" size="sm" onClick={onCancel} disabled={submitting}>
              Cancel
            </Button>
          )}
          <IconButton
            variant="primary"
            size="lg"
            disabled={!canSubmit}
            onClick={() => onSubmit(toDraft(s))}
            label={submitting ? 'Saving…' : initial ? 'Save' : 'Create'}
            title={submitting ? 'Saving…' : initial ? 'Save' : 'Create'}
          >
            {submitting ? <SpinnerIcon /> : <SaveIcon />}
          </IconButton>
        </div>
      </div>
      {error && <p className="text-[11px] text-red">{error}</p>}
    </div>
  );
}
