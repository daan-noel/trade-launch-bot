/** Reading the raw on-chain `u64` fields the backend sends as JSON **strings**.
 *
 *  A JS `number` is an IEEE-754 double, so anything above `Number.MAX_SAFE_INTEGER`
 *  (2^53−1) is silently rounded on arrival. That is not hypothetical here: pump.fun's
 *  `buy` takes `max_sol_cost` as a slippage *ceiling*, so a dev wanting "fill at any
 *  price" passes `u64::MAX` (`18446744073709551615`) — thousands of tokens a month
 *  carry it — and as a JSON number it reaches the browser as `18446744073709552000`.
 *
 *  The backend therefore encodes these args as strings (`trading_core::serde_wire`).
 *  `number` stays in the accepted union so a cached response or an endpoint that
 *  still sends one keeps working — every helper here takes either shape.
 */

/** A `u64` field as it arrives: string (current), number (legacy), or absent. */
export type U64Wire = string | number | null | undefined;

/** Largest lamports value that fits an `i64` — the backend's
 *  `hunter_engine::grouping::MAX_BUCKETABLE_LAMPORTS`. Above this the arg is a "no
 *  limit" ceiling rather than an amount, and the backend groups it separately
 *  instead of binning it. */
export const MAX_BUCKETABLE_LAMPORTS = 9223372036854775807n;

const LAMPORTS_PER_SOL = 1_000_000_000n;

/** Exact value as a `bigint`, or `null` when absent/unparseable. */
export function u64Big(v: U64Wire): bigint | null {
  if (v == null) return null;
  if (typeof v === 'number') return Number.isFinite(v) ? BigInt(Math.trunc(v)) : null;
  const t = v.trim();
  if (!/^\d+$/.test(t)) return null;
  return BigInt(t);
}

/** As a JS number, for sorting/filtering/compact display.
 *
 *  **Approximate above 2^53** — that is inherent to `number` and fine for ordering
 *  (the rounding is monotonic), but never round-trip a value through this and write
 *  it back. Use {@link u64Big} or the raw string when the digits matter. */
export function u64Num(v: U64Wire): number | null {
  const b = u64Big(v);
  return b == null ? null : Number(b);
}

/** Lamports → human SOL as a number, for sort/filter in displayed units. Same
 *  approximation caveat as {@link u64Num}. */
export function u64Sol(v: U64Wire): number | null {
  const n = u64Num(v);
  return n == null ? null : n / 1e9;
}

/** Whether this arg is an out-of-`i64` "no limit" ceiling rather than a real
 *  amount — the same split the backend's group keys use. */
export function isNoLimitLamports(v: U64Wire): boolean {
  const b = u64Big(v);
  return b != null && b > MAX_BUCKETABLE_LAMPORTS;
}

/** Lamports → human SOL as an **exact** decimal string (trailing zeros trimmed),
 *  mirroring the backend's `grouping::exact_sol_label_u64`. Integer arithmetic, so
 *  it holds every digit of a `u64` — use it wherever the exact amount is shown or
 *  copied, e.g. the title of a truncated cell. `''` when absent. */
export function u64LamportsToSolText(v: U64Wire): string {
  const b = u64Big(v);
  if (b == null) return '';
  const whole = b / LAMPORTS_PER_SOL;
  const frac = b % LAMPORTS_PER_SOL;
  if (frac === 0n) return whole.toString();
  return `${whole}.${frac.toString().padStart(9, '0')}`.replace(/0+$/, '');
}
