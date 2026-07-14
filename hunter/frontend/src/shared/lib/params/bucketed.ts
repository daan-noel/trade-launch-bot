// SSOT: which fingerprint columns the backend matches by **bucket membership**
// (`grouping::same_bucket(value, rule_value, bucket_width_sol)`), not by exact
// value — see `tpsl_sniper_1/entry/mod.rs`. These are continuous SOL amounts, so
// the rule value matches any token whose value lands in the same `[lo, hi)` bucket
// of width `bucket_width_sol` (the "Bucket Size (SOL)" field).
//
// This is the rule-form (column-keyed) twin of the grouped sweep's
// `BUCKETED_GROUP_FIELDS` (GroupField-keyed) in
// `@lab/components/sweep/groupedTypes.ts`. The two are the SAME fact in two
// vocabularies; `bucketedFingerprint.parity.test.ts` asserts they stay equal.

/** Fingerprint columns matched by `[lo, hi)` bucket, not exact value. */
export const BUCKETED_FINGERPRINT_COLUMNS: ReadonlySet<string> = new Set<string>([
  'p_token_initial_buy_sol',
  'p_token_first_slot_buy_sol',
  'p_token_first_slot_sell_sol',
  'p_token_max_sol_cost',
  'p_token_spendable_sol_in',
]);

/** True when a rule column is matched by bucket membership rather than exactly. */
export function isBucketedColumn(column: string): boolean {
  return BUCKETED_FINGERPRINT_COLUMNS.has(column);
}

/** Shared tooltip copy for the "bucketed input" marker on those fields. */
export const BUCKET_MATCH_HINT =
  'Matched by bucket, not exactly: a token matches when its value lands in the ' +
  'same [lo, hi) range as this one. Range width = Bucket Size (SOL).';
