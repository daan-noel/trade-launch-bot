// Compact auto-name for a fingerprint — a picker/log handle, not identity.
// One chip per configured axis, in registry order. Example: `3ix:Buy · max=1~2`
//
// Rust SSOT: `Fingerprint::auto_name`. This file is the TS mirror; golden strings
// in the two test files stay byte-equal.
// Detail: hunter/docs/plans/strategies/fingerprint-auto-name.md

import { configuredIxLabels, ixLabelsCountTail } from 'lib/ixLabels';
import { formatCompact } from 'utils/format';
import { fingerprintIdentityFromGroupKey, type FingerprintIdentity } from './matchGroupFingerprint';
import {
  AXES,
  axisDef,
  configuredAxes,
  lamportsToSolLabel,
  type AxisId,
  type AxisPredicate,
  type AxisUnit,
} from './fingerprintAxes';
import { WILDCARD_NAME } from './types';

/** Chip separator. Mirrors Rust `AUTO_NAME_SEP`. */
export const AUTO_NAME_SEP = ' · ';

/** Separates the two bounds of a range chip. Deliberately not `-`: an amount chip
 *  is a decimal, so `1-2` reads as a subtraction to a human and is ambiguous
 *  against a negative bound to the grammar checker. Mirrors Rust `RANGE_SEP`. */
const RANGE_SEP = '~';

/** Axes the auto-name reads — identity only (`name` is the output). */
export type FingerprintAutoNameAxes = Pick<FingerprintIdentity, 'wildcard' | 'criteria'>;

/**
 * Build the auto-name from the configured axes.
 *
 * One chip per axis in registry order, so an axis added to `AXES` is named without
 * touching this function or its grammar. Unset axes are skipped; nothing to name →
 * `ALL`.
 *
 * A wildcard carries no axis, so it names the token set it matches — `ALL`.
 */
export function fingerprintAutoName(fp: FingerprintAutoNameAxes): string {
  if (fp.wildcard) return WILDCARD_NAME;
  const parts = configuredAxes(fp.criteria ?? {})
    .map(([id, pred]) => axisChip(id, pred))
    .filter((p): p is string => p != null);
  return parts.length === 0 ? WILDCARD_NAME : parts.join(AUTO_NAME_SEP);
}

/** One axis's chip, or `null` when the axis names nothing renderable. */
function axisChip(id: AxisId, pred: AxisPredicate): string | null {
  const def = axisDef(id);
  if (pred.kind === 'sequence') {
    // The label sequence keeps its own shape: the COUNT is what makes it readable
    // at chip size, with the trailing action as a hint of which tool.
    if (id !== 'ix_labels') return null;
    const ix = configuredIxLabels(pred.labels);
    return ix ? ixLabelsCountTail(ix) : null;
  }
  const n = (v: string) => renderBound(v, def.unit);
  const { min, max } = pred;
  let body: string;
  if (min != null && max != null) body = min === max ? n(min) : `${n(min)}${RANGE_SEP}${n(max)}`;
  else if (min != null) body = `${n(min)}${RANGE_SEP}`;
  else if (max != null) body = `${RANGE_SEP}${n(max)}`;
  else return null;
  return `${def.chip}=${body}`;
}

/** One bound in the axis's display unit. Lamports read as SOL (what the operator
 *  typed); everything else is the integer, compacted so a 200000 CU limit does not
 *  eat half the chip. */
function renderBound(v: string, unit: AxisUnit): string {
  if (unit === 'lamports') return lamportsToSolLabel(v);
  if (unit === 'compute_units') return formatCompact(Number(v), 1);
  return v;
}

/**
 * Auto-name from a stored group key — a copy of that key's predicates, not a
 * re-derivation, so a card's name is the name of the fingerprint it would promote.
 */
export function fingerprintNameFromGroupKey(gk: Record<string, unknown>): string {
  return fingerprintAutoName(fingerprintIdentityFromGroupKey(gk));
}

/**
 * Whether `name` is written in `fingerprintAutoName`'s own chip grammar: every
 * `AUTO_NAME_SEP`-separated part is a chip that function emits. Such a name was
 * generated, never typed, so {@link isStaleAutoName} may rewrite it once it stops
 * matching the axes. Mirrors Rust `is_generated_auto_name`.
 *
 * Deliberately strict — an unrecognised part makes the whole name a nickname. The
 * two mistakes do not cost the same: re-deriving a name it declined to touch is
 * free, while rewriting a real nickname destroys the only record of why that
 * fingerprint was created.
 */
export function isGeneratedAutoName(name: string): boolean {
  const n = name.trim();
  if (n === '') return false;
  if (n === WILDCARD_NAME) return true;
  return n.split(AUTO_NAME_SEP).every(isAutoNameChip);
}

/** One chip of the {@link isGeneratedAutoName} grammar. **Derived from the
 *  registry**, so an axis added there is recognised here without an edit — the
 *  drift this used to have (a chip emitted but not recognised, so its name never
 *  healed) is structurally impossible now. */
function isAutoNameChip(part: string): boolean {
  // `3ix` / `3ix:BuyExactSolIn` — the count is what makes it a chip and not a word,
  // so a nickname prefix like `8dtx` is not `{digits}ix`.
  if (/^\d+ix(:.+)?$/.test(part)) return true;
  const eq = part.indexOf('=');
  if (eq < 0) return false;
  const label = part.slice(0, eq);
  const value = part.slice(eq + 1);
  const def = AXES.find((a) => a.chip === label);
  if (!def) return false;
  const ok = (s: string) => isBound(s, def.unit);
  const sep = value.indexOf(RANGE_SEP);
  if (sep < 0) return ok(value);
  const lo = value.slice(0, sep);
  const hi = value.slice(sep + 1);
  if (lo === '' && hi === '') return false;
  if (lo === '') return ok(hi);
  if (hi === '') return ok(lo);
  return ok(lo) && ok(hi);
}

/** One rendered bound: digits, at most one `.`, and — for a compute-unit axis — an
 *  optional K/M/G scale suffix. Never signed: identity is a non-negative integer,
 *  so a `-` in a chip means the name was typed. */
function isBound(s: string, unit: AxisUnit): boolean {
  const body = unit === 'compute_units' ? s.replace(/[KMG]$/, '') : s;
  return /^\d+(\.\d+)?$/.test(body);
}

/**
 * Whether `name` is an auto-label that no longer says what the axes say — a
 * retired shape, or a current-grammar one that has drifted from `autoName`.
 * Mirrors Rust `Fingerprint::has_stale_auto_name`.
 */
export function isStaleAutoName(name: string, autoName: string): boolean {
  return isLegacyAutoName(name) || (isGeneratedAutoName(name) && name.trim() !== autoName);
}

/**
 * Retired generator shapes — `sweep {id} · group N`, C3 provenance prefixes, the
 * flow-discovery fallback, a blank, and any name carrying a **retired chip**.
 * Nicknames return false. Mirrors Rust `is_legacy_auto_name`.
 *
 * The last clause is what lets a chip retire. {@link isGeneratedAutoName} is
 * deliberately strict, so a name carrying a chip that no longer exists would
 * otherwise be frozen as a nickname and never heal.
 */
export function isLegacyAutoName(name: string): boolean {
  const n = name.trim();
  if (n === '') return true;
  if (n.toLowerCase() === 'flow-discovery bind') return true;
  if (n.startsWith('sweep ') && n.includes(' · group ')) return true;
  if (n.startsWith('c · ') || n.startsWith('f · ') || n.startsWith('s · ')) return true;
  const parts = n.split(AUTO_NAME_SEP);
  return parts.some(isRetiredAutoNameChip) && parts.every((p) => isRetiredAutoNameChip(p) || isAutoNameChip(p));
}

/** A chip `fingerprintAutoName` used to emit and no longer does.
 *
 *  `bkt=…` was the row-wide SOL bucket width. It has no successor — a width is not
 *  a property of a fingerprint any more, because each axis carries its own explicit
 *  range — so a name holding one is stale by construction. */
function isRetiredAutoNameChip(part: string): boolean {
  const eq = part.indexOf('=');
  if (eq < 0 || part.slice(0, eq) !== 'bkt') return false;
  const v = part.slice(eq + 1);
  return v === 'exact' || /^\d+(\.\d+)?$/.test(v);
}
