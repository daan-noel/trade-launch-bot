import type { ReactNode } from 'react';
import {
  activeFilterCount,
  defaultFilters,
  type TokenFilters,
} from './filters';

interface FilterPanelProps {
  filters: TokenFilters;
  onChange: (field: keyof TokenFilters, value: string) => void;
  onClear: () => void;
}

function RangeInput({
  label,
  minKey,
  maxKey,
  filters,
  onChange,
  step = '0.1',
}: {
  label: string;
  minKey: keyof TokenFilters;
  maxKey: keyof TokenFilters;
  filters: TokenFilters;
  onChange: FilterPanelProps['onChange'];
  step?: string;
}) {
  return (
    <div className="flex items-center gap-1.5">
      <span className="whitespace-nowrap text-[10px] font-semibold text-text-dim">{label}</span>
      <div className="flex items-center gap-0.5">
        <input
          type="number"
          min={0}
          step={step}
          placeholder="min"
          value={filters[minKey]}
          onChange={(e) => onChange(minKey, e.target.value)}
          className="w-[70px] rounded border border-white/8 bg-white/4 px-1 py-0.5 font-mono text-[11px] text-text outline-none focus:border-primary/40"
        />
        <span className="text-[10px] text-text-dim">–</span>
        <input
          type="number"
          min={0}
          step={step}
          placeholder="max"
          value={filters[maxKey]}
          onChange={(e) => onChange(maxKey, e.target.value)}
          className="w-[70px] rounded border border-white/8 bg-white/4 px-1 py-0.5 font-mono text-[11px] text-text outline-none focus:border-primary/40"
        />
      </div>
    </div>
  );
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="mb-2">
      <div className="mb-1 flex items-center gap-2 text-[8px] font-bold uppercase tracking-widest text-text-dim/70">
        {title}
        <span className="h-px flex-1 bg-white/5" />
      </div>
      <div className="flex flex-wrap items-center gap-x-5 gap-y-1.5">{children}</div>
    </div>
  );
}

export function FilterPanel({ filters, onChange, onClear }: FilterPanelProps) {
  const active = activeFilterCount(filters);

  return (
    <div className="mb-2 rounded-lg border border-white/7 bg-white/2 px-3.5 py-2.5">
      <div className="mb-2.5 flex items-center justify-between">
        <span className="text-[9px] font-bold uppercase tracking-widest text-text-dim">Filters</span>
        {active > 0 && (
          <button
            type="button"
            onClick={onClear}
            className="text-[10px] font-semibold text-text-dim hover:text-red"
          >
            Clear all ({active})
          </button>
        )}
      </div>

      <Group title="Time">
        <RangeInput label="Age (h)" minKey="age_min" maxKey="age_max" filters={filters} onChange={onChange} />
        <RangeInput label="Last Trade (h)" minKey="last_trade_min" maxKey="last_trade_max" filters={filters} onChange={onChange} />
        <RangeInput label="ATH Age (h)" minKey="ath_age_min" maxKey="ath_age_max" filters={filters} onChange={onChange} />
      </Group>

      <Group title="Performance">
        <RangeInput label="ATH/FEP (×)" minKey="ath_fep_min" maxKey="ath_fep_max" filters={filters} onChange={onChange} />
        <RangeInput label="Cur/FEP (×)" minKey="cur_fep_min" maxKey="cur_fep_max" filters={filters} onChange={onChange} />
        <RangeInput label="ATH Price" minKey="ath_price_min" maxKey="ath_price_max" filters={filters} onChange={onChange} step="any" />
        <RangeInput label="Price" minKey="price_min" maxKey="price_max" filters={filters} onChange={onChange} step="any" />
      </Group>

      <Group title="Market">
        <RangeInput label="Volume (SOL)" minKey="volume_min" maxKey="volume_max" filters={filters} onChange={onChange} step="0.01" />
        <RangeInput label="MCap (SOL)" minKey="mcap_min" maxKey="mcap_max" filters={filters} onChange={onChange} step="0.01" />
        <RangeInput label="Init Buy (SOL)" minKey="init_buy_min" maxKey="init_buy_max" filters={filters} onChange={onChange} step="0.001" />
        <RangeInput label="Init Supply" minKey="init_supply_min" maxKey="init_supply_max" filters={filters} onChange={onChange} step="1" />
        <RangeInput label="Token Amount" minKey="token_amount_min" maxKey="token_amount_max" filters={filters} onChange={onChange} step="1" />
        <RangeInput label="Max SOL Cost" minKey="max_sol_cost_min" maxKey="max_sol_cost_max" filters={filters} onChange={onChange} step="1" />
        <RangeInput label="Spendable SOL In" minKey="spendable_sol_in_min" maxKey="spendable_sol_in_max" filters={filters} onChange={onChange} step="1" />
        <RangeInput label="Min Tokens Out" minKey="min_tokens_out_min" maxKey="min_tokens_out_max" filters={filters} onChange={onChange} step="1" />
        <RangeInput label="Trades" minKey="trades_min" maxKey="trades_max" filters={filters} onChange={onChange} step="1" />
      </Group>

      <Group title="Technical">
        <RangeInput label="CU Limit" minKey="cu_limit_min" maxKey="cu_limit_max" filters={filters} onChange={onChange} step="1" />
        <RangeInput label="CU Price" minKey="cu_price_min" maxKey="cu_price_max" filters={filters} onChange={onChange} step="1" />
        <RangeInput label="IX Count" minKey="ix_count_min" maxKey="ix_count_max" filters={filters} onChange={onChange} step="1" />
        <div className="flex items-center gap-1.5">
          <span className="text-[10px] font-semibold text-text-dim">IX Label</span>
          <input
            type="text"
            placeholder="Jito, BuyExact…"
            value={filters.ix_label}
            onChange={(e) => onChange('ix_label', e.target.value)}
            className="w-[140px] rounded border border-white/8 bg-white/4 px-1 py-0.5 text-[11px] text-text outline-none focus:border-primary/40"
          />
        </div>
      </Group>

      <Group title="Other">
        <div className="flex items-center gap-1.5">
          <span className="text-[10px] font-semibold text-text-dim">Migrated</span>
          <select
            value={filters.migrated}
            onChange={(e) => onChange('migrated', e.target.value)}
            className="w-20 rounded border border-white/8 bg-white/4 px-1 py-0.5 text-[11px] text-text outline-none"
          >
            <option value="">All</option>
            <option value="yes">Yes</option>
            <option value="no">No</option>
          </select>
        </div>
        <div className="flex items-center gap-1.5">
          <span className="text-[10px] font-semibold text-text-dim">Creator</span>
          <input
            type="text"
            placeholder="address substring…"
            value={filters.creator}
            onChange={(e) => onChange('creator', e.target.value)}
            className="w-[140px] rounded border border-white/8 bg-white/4 px-1 py-0.5 text-[11px] text-text outline-none focus:border-primary/40"
          />
        </div>
      </Group>
    </div>
  );
}

export { defaultFilters };
