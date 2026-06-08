import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { Button } from 'components/ui/Button';
import { Input, Textarea } from 'components/ui/Input';
import {
  activeFilterCount,
  defaultFilters,
  type TokenFilters,
  type TriState,
} from './filters';

interface FilterPanelProps {
  /** Currently applied filters (source of truth). */
  filters: TokenFilters;
  /** Commit the draft as the active filters. */
  onApply: (next: TokenFilters) => void;
  /** Reset to empty filters. */
  onClear: () => void;
}

type SetField = <K extends keyof TokenFilters>(key: K, value: TokenFilters[K]) => void;

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

function TextField({
  label,
  field,
  draft,
  set,
  placeholder,
  className,
}: {
  label: string;
  field: keyof TokenFilters;
  draft: TokenFilters;
  set: SetField;
  placeholder?: string;
  className?: string;
}) {
  return (
    <Field label={label} className={cn('w-[170px]', className)}>
      <Input
        value={draft[field] as string}
        placeholder={placeholder}
        onChange={(e) => set(field, e.target.value as TokenFilters[typeof field])}
      />
    </Field>
  );
}

function RangeField({
  label,
  hint,
  minKey,
  maxKey,
  draft,
  set,
  step = 'any',
}: {
  label: string;
  hint?: string;
  minKey: keyof TokenFilters;
  maxKey: keyof TokenFilters;
  draft: TokenFilters;
  set: SetField;
  step?: string;
}) {
  return (
    <Field label={label} hint={hint} className="w-[150px]">
      <div className="flex items-center gap-1">
        <Input
          type="number"
          step={step}
          placeholder="min"
          value={draft[minKey] as string}
          onChange={(e) => set(minKey, e.target.value as TokenFilters[typeof minKey])}
        />
        <span className="text-[10px] text-text-dim/50">–</span>
        <Input
          type="number"
          step={step}
          placeholder="max"
          value={draft[maxKey] as string}
          onChange={(e) => set(maxKey, e.target.value as TokenFilters[typeof maxKey])}
        />
      </div>
    </Field>
  );
}

function DateRangeField({
  label,
  fromKey,
  toKey,
  draft,
  set,
}: {
  label: string;
  fromKey: keyof TokenFilters;
  toKey: keyof TokenFilters;
  draft: TokenFilters;
  set: SetField;
}) {
  return (
    <Field label={label} hint="UTC" className="w-fit">
      <div className="flex items-center gap-1">
        <Input
          type="datetime-local"
          value={draft[fromKey] as string}
          onChange={(e) => set(fromKey, e.target.value as TokenFilters[typeof fromKey])}
        />
        <span className="text-[10px] text-text-dim/50">–</span>
        <Input
          type="datetime-local"
          value={draft[toKey] as string}
          onChange={(e) => set(toKey, e.target.value as TokenFilters[typeof toKey])}
        />
      </div>
    </Field>
  );
}

const TRI_OPTIONS: { value: TriState; label: string }[] = [
  { value: '', label: 'All' },
  { value: 'yes', label: 'Yes' },
  { value: 'no', label: 'No' },
];

function TriToggle({
  label,
  field,
  draft,
  set,
}: {
  label: string;
  field: keyof TokenFilters;
  draft: TokenFilters;
  set: SetField;
}) {
  const value = draft[field] as TriState;
  return (
    <Field label={label} className="w-auto">
      <div className="inline-flex overflow-hidden rounded-md border border-white/10">
        {TRI_OPTIONS.map((opt, i) => {
          const selected = value === opt.value;
          return (
            <button
              key={opt.value || 'all'}
              type="button"
              onClick={() => set(field, opt.value as TokenFilters[typeof field])}
              className={cn(
                'px-3 py-1 text-[11px] font-semibold transition',
                i > 0 && 'border-l border-white/10',
                !selected && 'bg-white/2 text-text-dim hover:bg-white/4 hover:text-text',
                selected && opt.value === '' && 'bg-white/10 text-text',
                selected && opt.value === 'yes' && 'bg-primary/15 text-primary',
                selected && opt.value === 'no' && 'bg-red/15 text-red',
              )}
            >
              {opt.label}
            </button>
          );
        })}
      </div>
    </Field>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div>
      <div className="mb-2 flex items-center gap-2 text-[8px] font-bold uppercase tracking-widest text-text-dim/60">
        {title}
        <span className="h-px flex-1 bg-white/6" />
      </div>
      <div className="flex flex-wrap items-end gap-x-4 gap-y-3">{children}</div>
    </div>
  );
}

export function FilterPanel({ filters, onApply, onClear }: FilterPanelProps) {
  const [draft, setDraft] = useState<TokenFilters>(filters);

  // Re-sync the draft whenever the applied filters change externally (apply / clear).
  useEffect(() => {
    setDraft(filters);
  }, [filters]);

  const set: SetField = (key, value) => setDraft((d) => ({ ...d, [key]: value }));

  const draftCount = activeFilterCount(draft);
  const dirty = useMemo(
    () => JSON.stringify(draft) !== JSON.stringify(filters),
    [draft, filters],
  );

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        onApply(draft);
      }}
      className="w-full mb-2 rounded-lg border border-white/8 bg-white/2 px-4 py-3"
    >
      <div className="mb-3 flex items-center justify-between gap-3 border-b border-white/6 pb-2.5">
        <div className="flex items-center gap-2">
          <span className="text-[10px] font-bold uppercase tracking-widest text-text-dim">
            Global Filters
          </span>
          {draftCount > 0 && (
            <span className="rounded border border-primary/30 bg-primary/12 px-1.5 py-0.5 font-mono text-[10px] font-bold text-primary">
              {draftCount}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="link"
            size="sm"
            onClick={() => {
              setDraft(defaultFilters());
              onClear();
            }}
            disabled={draftCount === 0}
            className="hover:text-red disabled:hover:text-text-dim"
          >
            Clear all
          </Button>
          <Button
            variant="primary"
            size="md"
            type="submit"
            disabled={!dirty}
            className="min-h-9 px-6 font-bold uppercase tracking-wider"
          >
            {dirty ? 'Apply' : 'Applied'}
          </Button>
        </div>
      </div>

      <div className="flex flex-col gap-4">
        <Section title="Identity">
          <TextField label="Symbol" field="symbol" draft={draft} set={set} placeholder="e.g. PEPE" className="w-[130px]" />
          {/* <TextField label="Name" field="name" draft={draft} set={set} placeholder="name contains…" /> */}
          <TextField label="Mint" field="mint" draft={draft} set={set} placeholder="address substring…" />
          <TextField label="Creator" field="creator" draft={draft} set={set} placeholder="address substring…" />
          <TextField label="Create TX" field="create_tx" draft={draft} set={set} placeholder="signature substring…" />
        </Section>

        <Section title="Time (UTC)">
          <DateRangeField label="Created" fromKey="created_from" toKey="created_to" draft={draft} set={set} />
          <DateRangeField label="Last Trade" fromKey="last_trade_from" toKey="last_trade_to" draft={draft} set={set} />
          <DateRangeField label="ATH At" fromKey="ath_from" toKey="ath_to" draft={draft} set={set} />
          <RangeField label="Life (min)" hint="dead only" minKey="life_min" maxKey="life_max" draft={draft} set={set} step="0.5" />
        </Section>

        <Section title="Performance">
          <RangeField label="ATH / FEP (×)" minKey="ath_fep_min" maxKey="ath_fep_max" draft={draft} set={set} step="0.1" />
          <RangeField label="Cur / FEP (×)" minKey="cur_fep_min" maxKey="cur_fep_max" draft={draft} set={set} step="0.1" />
          <RangeField label="ATH Price" minKey="ath_price_min" maxKey="ath_price_max" draft={draft} set={set} />
          <RangeField label="Price" minKey="price_min" maxKey="price_max" draft={draft} set={set} />
        </Section>

        <Section title="Market">
          <RangeField label="Volume (SOL)" minKey="volume_min" maxKey="volume_max" draft={draft} set={set} step="0.01" />
          <RangeField label="MCap (SOL)" minKey="mcap_min" maxKey="mcap_max" draft={draft} set={set} step="0.01" />
          <RangeField label="Trades" minKey="trades_min" maxKey="trades_max" draft={draft} set={set} step="1" />
          <RangeField label="Init Buy (SOL)" minKey="init_buy_min" maxKey="init_buy_max" draft={draft} set={set} step="0.001" />
          <RangeField label="Init Supply" minKey="init_supply_min" maxKey="init_supply_max" draft={draft} set={set} step="1" />
          <RangeField label="Token Amount" minKey="token_amount_min" maxKey="token_amount_max" draft={draft} set={set} step="1" />
          <RangeField label="Max SOL Cost" minKey="max_sol_cost_min" maxKey="max_sol_cost_max" draft={draft} set={set} step="0.001" />
          <RangeField label="Spendable SOL In" minKey="spendable_sol_in_min" maxKey="spendable_sol_in_max" draft={draft} set={set} step="0.001" />
          <RangeField label="Min Tokens Out" minKey="min_tokens_out_min" maxKey="min_tokens_out_max" draft={draft} set={set} step="1" />
        </Section>

        <Section title="Technical">
          <RangeField label="CU Limit" minKey="cu_limit_min" maxKey="cu_limit_max" draft={draft} set={set} step="1" />
          <RangeField label="CU Price" minKey="cu_price_min" maxKey="cu_price_max" draft={draft} set={set} step="1" />
          <RangeField label="IX Count" minKey="ix_count_min" maxKey="ix_count_max" draft={draft} set={set} step="1" />
          <Field label="IX Labels" hint="one per line — matches any" className="w-[280px]">
            <Textarea
              value={draft.ix_label}
              placeholder={'Jito\nBuyExact'}
              onChange={(e) => set('ix_label', e.target.value)}
            />
          </Field>
        </Section>

        <Section title="Flags">
          <TriToggle label="Migrated" field="migrated" draft={draft} set={set} />
          <TriToggle label="Mayhem Mode" field="mayhem" draft={draft} set={set} />
          <TriToggle label="Cashback" field="cashback" draft={draft} set={set} />
        </Section>
      </div>
    </form>
  );
}

export { defaultFilters };
