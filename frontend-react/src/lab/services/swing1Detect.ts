// swing1 per-token detection funnel client + types (lab-only). Mirrors the
// backend `POST /api/tokens/{mint}/swing1-detect` (lab/src/api/handlers/tokens/
// swing1_detect.rs). Surfaces the SAME pure-fn decisions the grouped sweep makes:
// per-swing-low kill/volume verdicts, the kill→volume latch, and the resolved
// entry/exit — so the page shows exactly what the sweep would decide.

import { API_BASE } from 'services/config';
import { request } from 'services/http';
import type { SwingLegRecord } from 'types';

/** The 25 swing1 knobs, all optional (`null`/omitted = inert, same as a blank
 *  sweep axis). Keys match the backend `Swing1DetectParams` field names. */
export interface Swing1DetectParams {
  take_profit?: number | null;
  stop_loss?: number | null;
  trailing_stop_pct?: number | null;
  time_stop_secs?: number | null;
  stall_secs?: number | null;
  liquidity_drop_pct?: number | null;
  swing_high_to_low_sol?: number | null;
  swing_high_to_low_pct?: number | null;
  swing_low_to_high_sol?: number | null;
  swing_low_to_high_pct?: number | null;
  swing_min_leg_trades?: number | null;
  dust_frac?: number | null;
  kill_depth_min_pct?: number | null;
  kill_max_duration_ms?: number | null;
  kill_min_net_flow_per_sec?: number | null;
  vol_depth_max_pct?: number | null;
  vol_min_duration_ms?: number | null;
  vol_min_up_duration_ms?: number | null;
  min_kills_before_volume?: number | null;
  entry_pullback_pct?: number | null;
  entry_higher_low_secs?: number | null;
  entry_max_age_secs?: number | null;
  entry_min_liquidity_sol?: number | null;
  entry_max_cohort_held?: number | null;
  exit_next_kill_depth_min_pct?: number | null;
  exit_next_kill_max_duration_ms?: number | null;
}

/** One swing-low row's classifier verdict (drives the funnel table + chart). */
export interface Swing1LowVerdict {
  leg_index: number;
  start_at_ms: number;
  end_at_ms: number;
  depth_pct: number;
  duration_ms: number;
  net_flow_per_sec: number;
  trade_count: number;
  pivot_price: number;
  is_kill: boolean;
  is_volume: boolean;
  /** Higher-low vs the running last-kill pivot — the exact gate the latch uses. */
  higher_low_ok: boolean;
}

export interface Swing1LatchInfo {
  volume_phase_latched: boolean;
  latched_leg_index: number | null;
  kills_seen: number;
}

export interface Swing1EntryInfo {
  trigger_index: number;
  price: number;
  time: string;
}

export interface Swing1ExitInfo {
  reason: string;
  price: number;
  time: string;
  holding_secs: number;
}

export interface Swing1DetectResponse {
  mint: string;
  trade_count: number;
  /** `false` ⇒ no entry gate configured; funnel still shows legs/verdicts/latch. */
  gate_configured: boolean;
  legs: SwingLegRecord[];
  lows: Swing1LowVerdict[];
  latch: Swing1LatchInfo;
  entry: Swing1EntryInfo | null;
  exit: Swing1ExitInfo | null;
}

/**
 * Run the swing1 detection funnel for one token.
 *
 * `opts.startMs`/`opts.endMs` restrict detection to a window in ms relative to the
 * token's first trade (`null` = open); `opts.curveOnly` restricts to bonding-curve
 * trades. Omit `opts` to run over full history.
 */
export async function fetchSwing1Detect(
  mint: string,
  params: Swing1DetectParams,
  opts?: { startMs: number | null; endMs: number | null; curveOnly?: boolean },
): Promise<Swing1DetectResponse> {
  return request(`${API_BASE}/api/tokens/${encodeURIComponent(mint)}/swing1-detect`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      params,
      window_start_ms: opts?.startMs ?? null,
      window_end_ms: opts?.endMs ?? null,
      curve_only: opts?.curveOnly ?? false,
    }),
  });
}
