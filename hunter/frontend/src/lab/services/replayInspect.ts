// Wire types for the event-log replay inspector (redesign FE6 / backend Phase 6):
// `POST /api/replay/inspect` re-runs the pure engine `reduce` over a recorded live
// event log and dumps every `event → effects` decision. Lab-only (needs the log).
// Shapes mirror `hunter/lab/src/strategies/replay_inspect.rs`.

/** `POST /api/replay/inspect` request. All fields optional. */
export interface InspectRequest {
  /** Log dir (default: server `EVENT_LOG_DIR`, else `event_log`). */
  dir?: string;
  /** A single `YYYY-MM-DD` day-file; omitted ⇒ every day-file in `dir`. */
  date?: string;
  /** Only dump steps touching this mint (whole log is still folded). */
  mint?: string;
  /** Only dump steps at/after this RFC3339 instant. */
  since?: string;
  /** Only dump steps at/before this RFC3339 instant. */
  until?: string;
  /** Interleave synthetic 500 ms ticks (default true). */
  synthetic_ticks?: boolean;
  /** Replay against only active rules (default false ⇒ all rules). */
  active_only?: boolean;
  /** Restrict the loaded rule set to these ids (default: all). */
  rule_ids?: string[];
  /** Cap on dumped steps (default 10 000). */
  max_steps?: number;
}

/** A derived intent id `(rule, mint, seq)`. */
export interface InspectIntent {
  rule: string;
  mint: string;
  seq: number;
}

/** A fill (entry or exit). */
export interface InspectFill {
  price: number;
  sol: number;
  token_amount: number;
  at: string;
}

/** One effect in a step — `effect`-tagged (a projection of the engine `Effect`). */
export interface InspectEffect {
  effect: 'SubmitBuy' | 'SubmitSell' | 'PositionUpdate' | 'ArmedChanged';
  intent?: InspectIntent;
  rule?: string;
  mint?: string;
  lamports?: number;
  position?: string;
  /** Exit reason on a SubmitSell / close PositionUpdate. */
  reason?: string | null;
  /** `strategy_positions` status on a PositionUpdate. */
  status?: string;
  fill?: InspectFill | null;
  /** Armed-state tag on an ArmedChanged. */
  state?: string;
}

/** One `event → effects` decision. `event` is the externally-tagged logged event
 *  (`{"Trade": {…}}`) or a synthetic `{"Tick": {"now": …}}`. */
export interface InspectStep {
  seq: number;
  at?: string | null;
  event: Record<string, unknown>;
  effects: InspectEffect[];
}

/** `POST /api/replay/inspect` response. */
export interface InspectRun {
  rules_loaded: number;
  fingerprints_loaded: number;
  logged_events: number;
  synthetic_ticks: number;
  events_replayed: number;
  steps_returned: number;
  /** Recording stopped at `max_steps` before the log was exhausted. */
  truncated: boolean;
  steps: InspectStep[];
  dir: string;
  files: string[];
}

/** The single top-level tag of an externally-tagged logged event
 *  (`TokenCreated` | `FirstSlotSettled` | `Trade` | `Tick` | `FillConfirmed` |
 *  `FillFailed` | `Migrated` | `ManualClose`). */
export function eventKind(event: Record<string, unknown>): string {
  return Object.keys(event)[0] ?? 'Unknown';
}

/** The body of the tagged event (the value under its single key). */
export function eventBody(event: Record<string, unknown>): Record<string, unknown> {
  const k = eventKind(event);
  const v = event[k];
  return v && typeof v === 'object' ? (v as Record<string, unknown>) : {};
}

/** The mint an event references, if any (for the chart-focus + timeline label). */
export function eventMint(event: Record<string, unknown>): string | null {
  const body = eventBody(event);
  const m = body.mint;
  return typeof m === 'string' ? m : null;
}
