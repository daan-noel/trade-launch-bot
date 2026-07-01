// swing1 axis metadata — the page-editable param grid + subgroup layout for the
// kill→volume swing-phase strategy. Lives in `@shared` (not `@lab`) so BOTH the
// lab sweep/detect screens AND the live swing1 rule page can import it (a `@live`
// page cannot import `@lab`). `@lab/components/sweep/groupedTypes.ts` re-exports
// every symbol here, so existing lab imports keep working unchanged.
//
// The generic `AxisDef` / `AxisSubgroup` shapes live here too (they're used by the
// TPSL axes as well as swing1); `groupedTypes` re-exports them so its own
// TPSL1_AXES/TPSL2_AXES definitions and all `@lab` consumers resolve them.

// --- axis shape -------------------------------------------------------------

/** A finer, presentation-only bucket within a `group`. Only swing1 sets it (its
 *  25 knobs split into 6 pipeline-order buckets); TPSL1/TPSL2 leave it undefined
 *  and render as a single grid per group. Wire payloads are unaffected. */
export type AxisSubgroup = 'swing' | 'kill' | 'volume' | 'confirm' | 'ladder' | 'next-kill';

/** One editable axis: its key, label, the param role it belongs to (drives the
 *  entry/exit grouping in the sweep param grid), whether `null` (disabled) is a
 *  valid option, and the default candidate list (mirrors the strategy's
 *  `Axes::default` on the backend, so the projected combo count is accurate and
 *  the grid is prefilled). `key` is a plain string so one `AxisDef[]` shape
 *  serves any strategy's axes. */
export interface AxisDef {
  key: string;
  label: string;
  group: 'entry' | 'exit';
  /** Optional finer bucket within `group` (swing1 only) for the labelled-row
   *  param layout. Undefined ⇒ the axis renders in its group's single grid. */
  subgroup?: AxisSubgroup;
  nullable: boolean;
  default: (number | null)[];
  /** Optional plain-language explanation of what the knob does, rendered as a
   *  dimmed hint next to the field label in the sweep config form. */
  desc?: string;
}

// --- swing1 editable axes ---------------------------------------------------

/** The page-editable param grid for swing1 — the kill→volume swing-phase param
 *  space (25 knobs). Mirrors the backend `swing1::AxesSpec` field-for-field;
 *  `null` inside a nullable axis is that knob's "disabled / inert" option.
 *  NOTE: the `*_depth_*_pct` knobs are FRACTIONAL (0–1); the `swing_*_pct` /
 *  `entry_pullback_pct` knobs are 0–100. */
export interface Swing1AxesSpec {
  take_profit?: number[];
  stop_loss?: number[];
  trailing_stop_pct?: (number | null)[];
  time_stop_secs?: (number | null)[];
  stall_secs?: (number | null)[];
  liquidity_drop_pct?: (number | null)[];
  swing_high_to_low_sol?: (number | null)[];
  swing_high_to_low_pct?: (number | null)[];
  swing_low_to_high_sol?: (number | null)[];
  swing_low_to_high_pct?: (number | null)[];
  swing_min_leg_trades?: (number | null)[];
  dust_frac?: (number | null)[];
  kill_depth_min_pct?: (number | null)[];
  kill_max_duration_ms?: (number | null)[];
  kill_min_net_flow_per_sec?: (number | null)[];
  vol_depth_max_pct?: (number | null)[];
  vol_min_duration_ms?: (number | null)[];
  vol_min_up_duration_ms?: (number | null)[];
  min_kills_before_volume?: (number | null)[];
  entry_pullback_pct?: (number | null)[];
  entry_higher_low_secs?: (number | null)[];
  entry_max_age_secs?: (number | null)[];
  entry_min_liquidity_sol?: (number | null)[];
  entry_max_cohort_held?: (number | null)[];
  exit_next_kill_depth_min_pct?: (number | null)[];
  exit_next_kill_max_duration_ms?: (number | null)[];
}

// Order + grouping: the swing/kill/volume detection + entry-confirmation knobs
// gate the BUY, so they're `'entry'`; the exit ladder + symmetric next-kill flee
// are `'exit'`. Defaults mirror the backend `Swing1Axes::default` so the
// projected combo count is honest. Depth knobs are 0–1 (labelled so), the
// swing/pullback knobs are 0–100.
export const SWING1_AXES: AxisDef[] = [
  // Entry — swing detection (reversal thresholds + leg quality floor).
  // Defaults are a real SWEEP spread centered on the probe-validated firing
  // values (swing % ≈ 15, SOL floor off, kill depth ≈ 0.4, vol depth ≈ 0.6,
  // min_kills 0..1, pullback ≈ 10). Tighter thesis-pure values are included as
  // extra candidates so one run compares loose-vs-thesis. See the swing1 memory's
  // `swing-probe` / `swing-census` findings before narrowing these.
  { key: 'swing_high_to_low_pct', label: 'Swing high→low % (0–100)', group: 'entry', subgroup: 'swing', nullable: true, default: [15, 20, 25],
    desc: 'how far price must drop from a local high to count that down-leg as a swing' },
  { key: 'swing_low_to_high_pct', label: 'Swing low→high % (0–100)', group: 'entry', subgroup: 'swing', nullable: true, default: [15, 20, 25],
    desc: 'how far price must rise from a local low to count that up-leg as a swing' },
  { key: 'swing_high_to_low_sol', label: 'Swing high→low (SOL)', group: 'entry', subgroup: 'swing', nullable: true, default: [null],
    desc: 'same down-leg gate, but in absolute SOL move instead of % (off = use % only)' },
  { key: 'swing_low_to_high_sol', label: 'Swing low→high (SOL)', group: 'entry', subgroup: 'swing', nullable: true, default: [null],
    desc: 'same up-leg gate, but in absolute SOL move instead of % (off = use % only)' },
  { key: 'swing_min_leg_trades', label: 'Swing min leg trades', group: 'entry', subgroup: 'swing', nullable: true, default: [null, 2],
    desc: 'min trades a leg needs to count — filters out noise legs of 1–2 trades' },
  { key: 'dust_frac', label: 'Dust frac (0–1)', group: 'entry', subgroup: 'swing', nullable: true, default: [null],
    desc: 'drop a trade smaller than this fraction of the active leg’s biggest trade (e.g. 0.05 = ignore trades under 5% of the leg’s real size). scale-free — no SOL amount to guess. off = keep all non-zero trades' },
  // Entry — kill-low profile (deep + short).
  { key: 'kill_depth_min_pct', label: 'Kill depth min (0–1)', group: 'entry', subgroup: 'kill', nullable: true, default: [0.4, 0.5, 0.6],
    desc: 'a "kill" low must drop at least this fraction of the prior high (0.6 = −60%)' },
  { key: 'kill_max_duration_ms', label: 'Kill max duration (ms)', group: 'entry', subgroup: 'kill', nullable: true, default: [8000, 10000],
    desc: 'a kill must happen fast — within this many ms (devs dump quickly to eat snipers)' },
  { key: 'kill_min_net_flow_per_sec', label: 'Kill min net flow (SOL/s)', group: 'entry', subgroup: 'kill', nullable: true, default: [null],
    desc: 'min SOL/s of selling during the kill — confirms it was a real flush (off = ignore)' },
  // Entry — volume-low profile + count-free transition floor.
  { key: 'vol_depth_max_pct', label: 'Vol depth max (0–1)', group: 'entry', subgroup: 'volume', nullable: true, default: [0.4, 0.6],
    desc: 'a "volume-phase" low must be shallower than this — the dev is now drawing traders, not flushing' },
  { key: 'vol_min_duration_ms', label: 'Vol min duration (ms)', group: 'entry', subgroup: 'volume', nullable: true, default: [null, 10000],
    desc: 'a volume-phase low must last at least this long — slower, organic accumulation' },
  { key: 'vol_min_up_duration_ms', label: 'Vol min up-leg (ms)', group: 'entry', subgroup: 'volume', nullable: true, default: [null],
    desc: 'min duration of the recovery up-leg after a volume low (off = no floor)' },
  { key: 'min_kills_before_volume', label: 'Min kills before volume', group: 'entry', subgroup: 'volume', nullable: true, default: [0, 1],
    desc: 'how many kill lows must occur before we accept the volume phase (0 = "buy first dip", ≥1 = require the thesis)' },
  // Entry — confirmation (pullback / higher-low / age + guards).
  { key: 'entry_pullback_pct', label: 'Entry pullback % (0–100)', group: 'entry', subgroup: 'confirm', nullable: true, default: [null, 10, 20],
    desc: 'wait for this % pullback off the confirming high before buying — avoids buying the spike top' },
  { key: 'entry_higher_low_secs', label: 'Entry higher-low (s)', group: 'entry', subgroup: 'confirm', nullable: true, default: [null],
    desc: 'require a higher-low to hold for this many seconds before entering (off = enter on first confirm)' },
  { key: 'entry_max_age_secs', label: 'Entry max age (s)', group: 'entry', subgroup: 'confirm', nullable: true, default: [null],
    desc: 'skip the token if it is already older than this at entry (off = no age cap)' },
  { key: 'entry_min_liquidity_sol', label: 'Entry min liq (SOL)', group: 'entry', subgroup: 'confirm', nullable: true, default: [null],
    desc: 'only enter if pool liquidity is at least this many SOL (off = no minimum)' },
  { key: 'entry_max_cohort_held', label: 'Entry max cohort held %', group: 'entry', subgroup: 'confirm', nullable: true, default: [null],
    desc: 'skip if the launch-cohort still holds more than this % of supply — dump risk (off = ignore)' },
  // Exit — the reused ladder (TP/SL lead, then trailing/time/stall/liq).
  { key: 'take_profit', label: 'Take profit %', group: 'exit', subgroup: 'ladder', nullable: false, default: [50, 100, 200],
    desc: 'sell once unrealized gain reaches this %' },
  { key: 'stop_loss', label: 'Stop loss %', group: 'exit', subgroup: 'ladder', nullable: false, default: [30, 50],
    desc: 'sell once unrealized loss reaches this %' },
  { key: 'trailing_stop_pct', label: 'Trailing stop %', group: 'exit', subgroup: 'ladder', nullable: true, default: [null, 25],
    desc: 'sell if price falls this % from its peak since entry (locks in run-ups; off = no trail)' },
  { key: 'time_stop_secs', label: 'Time stop (s)', group: 'exit', subgroup: 'ladder', nullable: true, default: [null],
    desc: 'hard exit after holding this many seconds regardless of PnL (off = no time cap)' },
  { key: 'stall_secs', label: 'Stall (s)', group: 'exit', subgroup: 'ladder', nullable: true, default: [null, 60],
    desc: 'exit if no meaningful price move for this many seconds — the move is dead (off = ignore)' },
  { key: 'liquidity_drop_pct', label: 'Liq-drop exit %', group: 'exit', subgroup: 'ladder', nullable: true, default: [null],
    desc: 'bail if pool liquidity drops this % from entry — rug/drain signal (off = ignore)' },
  // Exit — symmetric next-kill flee (separate thresholds from the entry kill_*).
  { key: 'exit_next_kill_depth_min_pct', label: 'Next-kill depth min (0–1)', group: 'exit', subgroup: 'next-kill', nullable: true, default: [null, 0.6],
    desc: 'flee if a fresh kill-style drop of at least this depth starts after entry — the dev is rugging (off = ignore)' },
  { key: 'exit_next_kill_max_duration_ms', label: 'Next-kill max duration (ms)', group: 'exit', subgroup: 'next-kill', nullable: true, default: [8000],
    desc: 'how fast that fresh drop must be to count as a next-kill flee trigger' },
];

// --- subgroup metadata + bucketing helper -----------------------------------

/** Ordered, presentation-only metadata for the swing1 sub-buckets: the labelled
 *  row layout renders these in this pipeline order (swing → kill → volume →
 *  confirm, then ladder → next-kill). Shared by the sweep config form, the swing1
 *  detect page, and the live swing1 rule accordion so the screens read identically. */
export const SWING1_SUBGROUPS: { key: AxisSubgroup; label: string; hint: string; accent: string }[] = [
  { key: 'swing', label: 'Swing', hint: 'reversal legs', accent: 'text-sky-300' },
  { key: 'kill', label: 'Kill', hint: 'deep + fast flush', accent: 'text-red' },
  { key: 'volume', label: 'Volume', hint: 'shallow accumulation', accent: 'text-green' },
  { key: 'confirm', label: 'Confirm', hint: 'pullback + guards', accent: 'text-accent' },
  { key: 'ladder', label: 'Exit ladder', hint: 'TP/SL + trails', accent: 'text-warning' },
  { key: 'next-kill', label: 'Next-kill flee', hint: 'symmetric exit', accent: 'text-orange-300' },
];

/** Bucket axes into the ordered [`SWING1_SUBGROUPS`], dropping empty buckets. Any
 *  axes without a `subgroup` (TPSL1/TPSL2) collapse into a single trailing bucket
 *  with `meta = null`, so callers can fall back to one flat grid for those. Pure
 *  — safe to call inside a `useMemo`. */
export function groupAxesBySubgroup(
  axes: AxisDef[],
): { meta: (typeof SWING1_SUBGROUPS)[number] | null; axes: AxisDef[] }[] {
  const out: { meta: (typeof SWING1_SUBGROUPS)[number] | null; axes: AxisDef[] }[] = [];
  for (const meta of SWING1_SUBGROUPS) {
    const inBucket = axes.filter((a) => a.subgroup === meta.key);
    if (inBucket.length) out.push({ meta, axes: inBucket });
  }
  const untagged = axes.filter((a) => a.subgroup == null);
  if (untagged.length) out.push({ meta: null, axes: untagged });
  return out;
}
