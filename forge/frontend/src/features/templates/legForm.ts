import type { BuyVariant, DecoStep, LegStructureRecipe } from '@shared/types';

// The dev-buy is NOT a separate variant — it's driven purely by the template's
// `dev_buy_quote` (> 0 ⇒ create+dev-buy, else plain create), so `create_v2` and
// the old `create_v2_devbuy` behaved identically. One variant per builder.
export const VARIANTS = [
  'pumpfun.create_v2',
  'pumpfun.create_v1',
] as const;

// All four pump.fun buy encodings are selectable for a bundler leg. Each consumes
// the leg's per-leg SOL budget: the SOL-in encodings (`buy_exact_sol_in`,
// `buy_exact_quote_in_v2`) spend it directly; the tokens-out encodings (`buy`,
// `buy_v2`) take it as `max_sol_cost` and derive the token amount from the curve
// reserves at build time. Mixing encodings across legs diversifies the on-chain
// instruction discriminators (anti-fingerprint). `buy_exact_quote_in_v2` is the real
// v2 SOL-in ix (the old non-v2 `buy_exact_quote_in` was not a valid instruction).
export const BUY_VARIANTS: BuyVariant[] = [
  'buy',
  'buy_exact_sol_in',
  'buy_v2',
  'buy_exact_quote_in_v2',
];

// The form works in human quote units; `params` stores quote base units. These
// convert at the decimals boundary so operators never hand-compute lamports.
export interface LegRow {
  variant: BuyVariant;
  slippage_bps_min: string;
  slippage_bps_max: string;
  cu_limit_min: string;
  cu_limit_max: string;
  cu_price_min: string;
  cu_price_max: string;
  tip_quote_min: string;
  tip_quote_max: string;
  // Authored ix layout (decoration step order); undefined ⇒ canonical buy shape.
  layout?: DecoStep[];
}

export function emptyLegRow(): LegRow {
  return {
    variant: 'buy_exact_sol_in',
    slippage_bps_min: '',
    slippage_bps_max: '',
    cu_limit_min: '',
    cu_limit_max: '',
    cu_price_min: '',
    cu_price_max: '',
    tip_quote_min: '',
    tip_quote_max: '',
    layout: undefined,
  };
}

export function toBaseUnits(human: string, decimals: number): number | undefined {
  if (!human.trim()) return undefined;
  const n = Number(human);
  if (!Number.isFinite(n)) return undefined;
  return Math.round(n * 10 ** decimals);
}

export function toHumanUnits(base: number | null | undefined, decimals: number): string {
  if (base == null) return '';
  return String(base / 10 ** decimals);
}

export function toInt(value: string): number | undefined {
  if (!value.trim()) return undefined;
  const n = Number(value);
  return Number.isFinite(n) ? Math.round(n) : undefined;
}

export function legRowToRecipe(row: LegRow, decimals: number): LegStructureRecipe {
  return {
    variant: row.variant,
    slippage_bps_min: toInt(row.slippage_bps_min),
    slippage_bps_max: toInt(row.slippage_bps_max),
    cu_limit_min: toInt(row.cu_limit_min),
    cu_limit_max: toInt(row.cu_limit_max),
    cu_price_min: toInt(row.cu_price_min),
    cu_price_max: toInt(row.cu_price_max),
    tip_quote_min: toBaseUnits(row.tip_quote_min, decimals),
    tip_quote_max: toBaseUnits(row.tip_quote_max, decimals),
    layout: row.layout,
  };
}

export function recipeToLegRow(recipe: LegStructureRecipe, decimals: number): LegRow {
  return {
    variant: recipe.variant,
    slippage_bps_min: recipe.slippage_bps_min?.toString() ?? '',
    slippage_bps_max: recipe.slippage_bps_max?.toString() ?? '',
    cu_limit_min: recipe.cu_limit_min?.toString() ?? '',
    cu_limit_max: recipe.cu_limit_max?.toString() ?? '',
    cu_price_min: recipe.cu_price_min?.toString() ?? '',
    cu_price_max: recipe.cu_price_max?.toString() ?? '',
    tip_quote_min: toHumanUnits(recipe.tip_quote_min, decimals),
    tip_quote_max: toHumanUnits(recipe.tip_quote_max, decimals),
    layout: recipe.layout,
  };
}
