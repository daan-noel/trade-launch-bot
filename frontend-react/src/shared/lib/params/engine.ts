// Generic, spec-driven engine for rule params — one implementation of
// serialize / apply / build-payload / empty-form / form-from-rule, shared by all
// 3 strategies. Every function is a mechanical pass over a `StrategySpec`; there
// is no per-strategy branching. See `types.ts` for the model and
// `copy-params-unify-plan.md` for the design.
//
// Pure module: NO React / DOM imports, so it's unit-testable and safe in either
// build tree. `RuleRecord` is imported type-only.

import type { RuleRecord } from 'types';
import { parseIxLabels } from '@shared/components/tpsl2/utils';
import type {
  ApplyResult,
  FormState,
  LockState,
  ParamField,
  ParamGroup,
  PasteMode,
  RuleParamsBlob,
  StrategySpec,
} from './types';

/** The two administrative form fields, always present + always sent, never in
 *  `spec.fields`. */
export const RULE_NAME_KEY = 'rule_name';
export const TRADE_MODE_KEY = 'trade_mode';

// ── form construction ───────────────────────────────────────────────────────

/** Blank form: every field '' plus the admin fields. `trade_mode` defaults to the
 *  first mode option; `tolerance_pct` seeds '0' to mirror the old `emptyForm`
 *  (tolerance is a required match-band with a 0 default, shown not blank). */
export function emptyForm(spec: StrategySpec): FormState {
  const out: FormState = { [RULE_NAME_KEY]: '', [TRADE_MODE_KEY]: spec.modeOptions[0] };
  for (const f of spec.fields) out[f.column] = '';
  if ('tolerance_pct' in out) out.tolerance_pct = '0';
  return out;
}

/** Read a rule record into form state — each field's column off the record,
 *  `.toString()` or '' for null. `p_token_ix_labels` (array) stringifies to JSON. */
export function formFromRule(spec: StrategySpec, rule: RuleRecord): FormState {
  const r = rule as unknown as Record<string, unknown>;
  const out: FormState = {
    [RULE_NAME_KEY]: rule.rule_name,
    [TRADE_MODE_KEY]: rule.trade_mode,
  };
  for (const f of spec.fields) {
    const v = r[f.column];
    if (f.kind === 'array') {
      out[f.column] = Array.isArray(v) ? JSON.stringify(v) : '';
    } else {
      out[f.column] = v == null ? '' : String(v);
    }
  }
  return out;
}

// ── serialize (→ clipboard blob) ────────────────────────────────────────────

/** Serialize a whole rule to a blob — every spec column, null when absent. */
export function serializeRule(spec: StrategySpec, rule: RuleRecord): RuleParamsBlob {
  const r = rule as unknown as Record<string, unknown>;
  const params: Record<string, unknown> = {};
  for (const f of spec.fields) params[f.column] = r[f.column] ?? null;
  return { strategy: spec.strategy, version: 1, params };
}

/** JSON text of {@link serializeRule} (2-space indented), for the ⎘ copy button. */
export function serializeRuleJson(spec: StrategySpec, rule: RuleRecord): string {
  return JSON.stringify(serializeRule(spec, rule), null, 2);
}

/** Map from `comboKey` → column, built once per spec. */
function comboKeyIndex(spec: StrategySpec): Map<string, string> {
  const m = new Map<string, string>();
  for (const f of spec.fields) if (f.comboKey) m.set(f.comboKey, f.column);
  return m;
}

/** Serialize a sweep combo's swept params to a blob. Each `combo.params[comboKey]`
 *  maps to its field's canonical column via the spec — no prefix guessing. Keys
 *  the spec doesn't know are passed through under a `p_`-prefixed column as a
 *  last resort (keeps a novel swept knob visible rather than silently dropped). */
export function serializeCombo(
  spec: StrategySpec,
  combo: { params: Record<string, number | null> },
): RuleParamsBlob {
  const idx = comboKeyIndex(spec);
  const params: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(combo.params)) {
    const col = idx.get(k) ?? (k.startsWith('p_') ? k : `p_${k}`);
    params[col] = v;
  }
  return { strategy: spec.strategy, version: 1, params };
}

/** JSON text of {@link serializeCombo}, for the combo ⎘ copy button. */
export function serializeComboJson(
  spec: StrategySpec,
  combo: { params: Record<string, number | null> },
): string {
  return JSON.stringify(serializeCombo(spec, combo), null, 2);
}

// ── parse (clipboard text → blob) ───────────────────────────────────────────

/** Parse raw text as a blob. Returns null on invalid JSON / missing fields. */
export function parseBlob(raw: string): RuleParamsBlob | null {
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (typeof parsed !== 'object' || parsed === null) return null;
    const b = parsed as Record<string, unknown>;
    if (b.strategy !== 'tpsl1' && b.strategy !== 'tpsl2' && b.strategy !== 'swing_1') return null;
    if (b.version !== 1) return null;
    if (typeof b.params !== 'object' || b.params === null || Array.isArray(b.params)) return null;
    return b as unknown as RuleParamsBlob;
  } catch {
    return null;
  }
}

// ── apply (blob → form) ─────────────────────────────────────────────────────

/** Apply a parsed blob onto form state.
 *
 *  - `merge`   — overwrite only the fields present in the blob.
 *  - `replace` — reset to `emptyForm` first, then apply. `rule_name` is always kept.
 *  - `live`    — only `sizing`-group fields are applied; all else is skipped (mirrors
 *                the modal lock-group guards while a rule is running).
 *
 *  Blob keys that don't match a spec column are dropped (counted). Cross-strategy
 *  guard is the caller's job (PasteParamsSection checks `blob.strategy`). */
export function applyBlob(
  spec: StrategySpec,
  current: FormState,
  blob: RuleParamsBlob,
  opts: { live: boolean; mode: PasteMode },
): { next: FormState; result: ApplyResult } {
  const byColumn = new Map(spec.fields.map((f) => [f.column, f]));
  const next: FormState = opts.mode === 'replace' ? emptyForm(spec) : { ...current };
  // Rule name is administrative — always preserved across a paste.
  next[RULE_NAME_KEY] = current[RULE_NAME_KEY] ?? '';

  let applied = 0;
  let skipped = 0;
  let dropped = 0;
  const coveredGroups = new Set<ParamGroup>();

  for (const [col, value] of Object.entries(blob.params)) {
    const f = byColumn.get(col);
    if (!f) { dropped++; continue; }
    coveredGroups.add(f.group);
    if (opts.live && f.group !== 'sizing') { skipped++; continue; }
    if (f.kind === 'array') {
      next[col] = Array.isArray(value) ? JSON.stringify(value) : '';
    } else if (value === null || value === undefined) {
      next[col] = '';
    } else {
      next[col] = String(value);
    }
    applied++;
  }

  // Groups the spec defines that this blob never touched at all (as opposed to
  // touched-but-blank) — e.g. a swing1 detect-page copy has no `sizing`/
  // `fingerprint` keys, so those groups silently stay whatever the form already
  // had. Surfacing this tells the user those params weren't part of the paste.
  const emptyGroups = [...new Set(spec.fields.map((f) => f.group))].filter(
    (g) => !coveredGroups.has(g),
  );

  return { next, result: { applied, skipped, dropped, emptyGroups } };
}

// ── build payloads (form → wire) ────────────────────────────────────────────

const parseField = (f: ParamField, raw: string): number => (
  f.kind === 'int' ? parseInt(raw, 10) : parseFloat(raw)
);

/** POST body for a new rule. Required → parse always; optional blank → null (or 0
 *  when `createBlankZero`); arrays → parseIxLabels. `rule_name`/`trade_mode` sent. */
export function buildCreatePayload(spec: StrategySpec, form: FormState): Record<string, unknown> {
  const payload: Record<string, unknown> = {
    rule_name: form[RULE_NAME_KEY] ?? '',
    trade_mode: form[TRADE_MODE_KEY] ?? spec.modeOptions[0],
  };
  for (const f of spec.fields) {
    const raw = (form[f.column] ?? '').trim();
    if (f.kind === 'array') {
      payload[f.column] = parseIxLabels(form[f.column] ?? '');
    } else if (f.required) {
      payload[f.column] = parseField(f, raw);
    } else if (raw) {
      payload[f.column] = parseField(f, raw);
    } else {
      payload[f.column] = f.createBlankZero ? 0 : null;
    }
  }
  return payload;
}

/** PUT body from only the UNLOCKED sections. A locked section contributes no keys,
 *  so the backend Option-merge leaves those fields untouched. Within an unlocked
 *  section: required → parse; blank optional → 0 (ignore_zero); arrays →
 *  parseIxLabels. `rule_name`/`trade_mode` always sent. */
export function buildUpdatePayload(
  spec: StrategySpec,
  form: FormState,
  unlocked: LockState,
): Record<string, unknown> {
  const payload: Record<string, unknown> = {
    rule_name: form[RULE_NAME_KEY] ?? '',
    trade_mode: form[TRADE_MODE_KEY] ?? spec.modeOptions[0],
  };
  for (const f of spec.fields) {
    if (!unlocked[f.section]) continue;
    const raw = (form[f.column] ?? '').trim();
    if (f.kind === 'array') {
      payload[f.column] = parseIxLabels(form[f.column] ?? '');
    } else if (f.required) {
      payload[f.column] = parseField(f, raw);
    } else {
      payload[f.column] = raw ? parseField(f, raw) : 0;
    }
  }
  return payload;
}

// ── lock helpers ────────────────────────────────────────────────────────────

/** Fresh all-locked state: every section key `false`. */
export function freshLocks(spec: StrategySpec): LockState {
  const out: LockState = {};
  for (const s of spec.sections) out[s.key] = false;
  return out;
}
