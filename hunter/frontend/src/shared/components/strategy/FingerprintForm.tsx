import { useMemo, useState } from 'react';

import { Input } from 'components/ui/Input';
import { Button } from 'components/ui/Button';
import { IxLabelsInput } from 'components/ui/IxLabelsInput';
import { formatIxLabelsText, parseIxLabelsText } from 'lib/ixLabels';
import {
  lamportsToSol,
  solToLamports,
  type Fingerprint,
  type FingerprintDraft,
} from 'lib/strategy/types';

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
  /** Textarea text — pretty JSON string array (see `parseIxLabelsText`). */
  ix_labels: string;
}

function fromFingerprint(fp?: Fingerprint): FormState {
  return {
    name: fp?.name ?? '',
    cu_limit: fp?.cu_limit ?? null,
    cu_price: fp?.cu_price ?? null,
    init_buy_sol: lamportsToSol(fp?.init_buy_lamports),
    max_cost_sol: lamportsToSol(fp?.max_cost_lamports),
    spendable_sol: lamportsToSol(fp?.spendable_lamports_in),
    first_slot_buy_sol: lamportsToSol(fp?.first_slot_buy_lamports),
    first_slot_sell_sol: lamportsToSol(fp?.first_slot_sell_lamports),
    bucket_size_amount: fp?.bucket_size_amount ?? 0.1,
    ix_labels: formatIxLabelsText(fp?.ix_labels),
  };
}

function toDraft(s: FormState): FingerprintDraft {
  const { labels } = parseIxLabelsText(s.ix_labels);
  return {
    name: s.name.trim(),
    cu_limit: s.cu_limit,
    cu_price: s.cu_price,
    init_buy_lamports: solToLamports(s.init_buy_sol),
    max_cost_lamports: solToLamports(s.max_cost_sol),
    spendable_lamports_in: solToLamports(s.spendable_sol),
    first_slot_buy_lamports: solToLamports(s.first_slot_buy_sol),
    first_slot_sell_lamports: solToLamports(s.first_slot_sell_sol),
    bucket_size_amount: s.bucket_size_amount ?? 0.1,
    ix_labels: labels,
  };
}

/** How many match criteria the current form configures (mirrors the backend
 *  `has_any_criterion` — the create endpoint rejects a criterion-less draft). */
function criterionCount(s: FormState): number {
  const axes = [
    s.cu_limit,
    s.cu_price,
    s.init_buy_sol,
    s.max_cost_sol,
    s.spendable_sol,
    s.first_slot_buy_sol,
    s.first_slot_sell_sol,
  ];
  const labels = s.ix_labels.trim() ? 1 : 0;
  return axes.filter((v) => v != null).length + labels;
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

  const ixParsed = useMemo(() => parseIxLabelsText(s.ix_labels), [s.ix_labels]);
  const criteria = criterionCount(s);
  const nameOk = s.name.trim().length > 0;
  const canSubmit = criteria > 0 && nameOk && !submitting && !ixParsed.error;

  const solField = (label: string, key: keyof FormState) => (
    <label className="flex flex-col gap-1 text-[11px] text-text-dim">
      {label}
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
        Name
        <Input
          fieldSize="sm"
          value={s.name}
          onChange={(e) => set('name', e.target.value)}
          placeholder="e.g. 3.5◎ create_v2 6-ix"
        />
      </label>

      <div className="grid grid-cols-2 gap-2">
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          cu_limit (exact)
          <Input
            fieldSize="sm"
            numeric
            integer
            numericValue={s.cu_limit}
            onNumericChange={(n) => set('cu_limit', n)}
          />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          cu_price (exact)
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
        {solField('init_buy', 'init_buy_sol')}
        {solField('max_cost', 'max_cost_sol')}
        {solField('spendable_in', 'spendable_sol')}
        {solField('first_slot_buy', 'first_slot_buy_sol')}
        {solField('first_slot_sell', 'first_slot_sell_sol')}
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          bucket width (◎)
          <Input
            fieldSize="sm"
            numeric
            unit="◎"
            numericValue={s.bucket_size_amount}
            onNumericChange={(n) => set('bucket_size_amount', n)}
          />
        </label>
      </div>

      <label className="flex flex-col gap-1 text-[11px] text-text-dim">
        ix_labels (exact ordered sequence, JSON array)
        <IxLabelsInput
          value={s.ix_labels}
          onValueChange={(v) => set('ix_labels', v)}
          error={ixParsed.error}
        />
      </label>

      <div className="flex items-center justify-between">
        <span className="text-[11px] text-text-dim/80">
          {criteria === 0 ? (
            <span className="text-red">needs ≥1 match criterion</span>
          ) : (
            `${criteria} criterion${criteria === 1 ? '' : 'a'} · matched by ${s.bucket_size_amount ?? 0.1}◎ bucket`
          )}
        </span>
        <div className="flex gap-2">
          {onCancel && (
            <Button variant="ghost" size="sm" onClick={onCancel} disabled={submitting}>
              Cancel
            </Button>
          )}
          <Button variant="primary" size="sm" disabled={!canSubmit} onClick={() => onSubmit(toDraft(s))}>
            {initial ? 'Save' : 'Create'}
          </Button>
        </div>
      </div>
      {error && <p className="text-[11px] text-red">{error}</p>}
    </div>
  );
}
