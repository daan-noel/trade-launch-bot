// Unified rule-params model — one canonical vocabulary for copy/paste + the rule
// form, shared by all 3 strategies (tpsl1, tpsl2, swing_1).
//
// The single source of truth is the backend **column** name (`p_exit_take_profit`,
// `buy_amount_sol`, `tolerance_pct`, `trade_mode`, …). Form state, the clipboard blob,
// and the create/update payloads are ALL keyed by these columns, so the engine is
// a set of mechanical passes over a spec — no camelCase maps, no nested `axes`, no
// per-representation key translators. See `copy-params-unify-plan.md`.

/** How a field's string form-value parses to a wire value.
 *  - `float`  → parseFloat
 *  - `int`    → parseInt(…, 10)
 *  - `array`  → JSON array (only `p_token_ix_labels`), parsed via `parseIxLabels`. */
export type ParamKind = 'float' | 'int' | 'array';

/** The role bucket a field belongs to. Drives lock-groups (a locked group sends no
 *  keys on update) and the coarse tpsl section layout. swing1 refines `entry`/`exit`
 *  into finer render sections via {@link ParamField.section} but keeps these groups
 *  for the lock semantics (only `sizing` is unlockable while a rule is live). */
export type ParamGroup = 'fingerprint' | 'sizing' | 'entry' | 'exit' | 'mode';

/** A finer, presentation-only section within a `group`. swing1 splits its
 *  entry/exit knobs across these; tpsl leaves it undefined and renders one section
 *  per `group`. A section is also the lock-group unit for swing1 (each subgroup
 *  locks independently), so it doubles as swing1's `LockGroup`. */
export type ParamSection = 'swing' | 'kill' | 'volume' | 'confirm' | 'ladder' | 'next-kill';

/** One editable rule parameter, fully described so the generic engine + form can
 *  serialize / apply / build-payload / render it without any per-strategy code. */
export interface ParamField {
  /** Canonical backend column — the key everywhere (form state, blob, payload).
   *  e.g. `p_exit_take_profit`, `buy_amount_sol`, `tolerance_pct`, `trade_mode`. */
  column: string;
  /** Role bucket (lock-group + coarse layout). */
  group: ParamGroup;
  /** Which render section this field appears under. For tpsl this equals `group`;
   *  for swing1's entry/exit knobs it's the finer bucket (swing/kill/…). The
   *  universal `sizing`/`fingerprint`/`mode` fields use their group name. */
  section: ParamGroup | ParamSection;
  kind: ParamKind;
  /** Required (non-nullable) number: parsed with parseFloat even when the group is
   *  otherwise blank→null/0. Only TP/SL and `buy_amount_sol`. */
  required: boolean;
  /** On CREATE, a blank optional field normally serializes to `null`. Set this to
   *  emit `0` instead (swing1's `tolerance_pct` is `0`-defaulted, not null). Ignored
   *  for `required` fields. */
  createBlankZero?: boolean;
  /** The sweep combo's bare key for this field, IF it's a swept axis
   *  (e.g. `exit_take_profit`, `dust_frac`). Absent ⇒ never appears in a combo. */
  comboKey?: string;
  /** The swing1 **detect page's** bare key for this field, IF it appears there.
   *  Absent ⇒ not a detect-page param. Lets the detect adapter be spec-derived. */
  detectKey?: string;

  // ── presentation (drives the generic field renderer) ──
  label: string;
  /** Tooltip help key into `TPSL_PARAM_HELP` (tpsl fields) — omitted ⇒ plain label
   *  with an optional `help` string tooltip instead. */
  helpKey?: string;
  /** Plain-text tooltip when there's no `helpKey` (swing1 axis descriptions). */
  help?: string;
  unit?: string; // '◎' | '%' | 's' | 'ms'
  step?: string;
  min?: number;
  max?: number;
  /** Show the "0 = off" blank-zero behavior + placeholder (optional gates). */
  nullable?: boolean;
  /** Placeholder override (e.g. '0 = until dead'); defaults to '0 = off' when
   *  `nullable`. */
  placeholder?: string;
  /** Optional extra class on the input (focus accent), copied from the old modals. */
  inputClass?: string;
}

/** One accordion section, in render order. `group` is the lock-group the section's
 *  fields belong to; `liveEditable` marks the one section (sizing) that stays
 *  unlockable while the rule is running. */
export interface SpecSection {
  /** The lock-group / section key. For swing1 subgroups this is a `ParamSection`;
   *  for the universal sections it's a `ParamGroup`. */
  key: ParamGroup | ParamSection;
  label: string;
  hint: string;
  accent: string;
  /** Editable while the rule is live (only `sizing`). */
  liveEditable: boolean;
  /** Grid columns for this section's field grid. */
  cols: number;
}

export type Strategy = 'tpsl1' | 'tpsl2' | 'swing_1';

/** A complete, standalone description of one strategy's editable params. */
export interface StrategySpec {
  strategy: Strategy;
  /** Human title stem for the form header ("TPSL1", "Swing 1"). */
  title: string;
  /** Full ordered field list. */
  fields: ParamField[];
  /** Selectable trade modes: `['paper','real']` for tpsl, `['paper']` for swing1. */
  modeOptions: string[];
  /** Accordion sections in render order (excludes the always-visible mode+name row). */
  sections: SpecSection[];
}

// ── clipboard blob (unchanged wire format: p_*-keyed, version 1) ──

export interface RuleParamsBlob {
  strategy: Strategy;
  version: 1;
  params: Record<string, unknown>;
}

export type PasteMode = 'merge' | 'replace';

export interface ApplyResult {
  applied: number;
  skipped: number;
  dropped: number;
  /** Groups the target spec defines that got ZERO fields from this blob — e.g.
   *  pasting a swing1 detect-page copy (swing/kill/volume/exit only) into a rule
   *  form never carries `sizing`/`fingerprint`, since the detect page has no
   *  concept of position size or token matching. Empty when the blob's source
   *  covers every group (e.g. a rule→rule or combo→rule copy). */
  emptyGroups: ParamGroup[];
}

/** Form state: every field's raw string value, keyed by its canonical column. The
 *  two administrative fields `rule_name` and `trade_mode` live here too. */
export type FormState = Record<string, string>;

/** Lock state: one boolean per section key ("is this section unlocked for edit?"). */
export type LockState = Record<string, boolean>;
