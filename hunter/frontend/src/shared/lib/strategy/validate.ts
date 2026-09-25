// Client-side mirror of the engine's rule validation (`RuleParams::parse` +
// `validate` in `hunter_engine::rule_params`), so the editor names a problem where it
// is, while typing. The backend stays the authority: a draft that passes here passes
// the save endpoint, and the save endpoint's message is shown when it does not.
//
// Every message starts with WHERE (`Buy on, condition 1`, `stage early, line 2`) and
// says what to change.

import type { Condition, ConditionExpr } from './grammar';
import { checkRef } from './metricRef';
import { findMetric, type StrategyRegistry } from './registry';
import {
  MAX_SELL_PCT,
  MAX_SIZE_PCT_OF_POOL,
  MAX_STAGES,
  type Cond,
  type Line,
  type RuleDoc,
  type Stage,
} from './ruleDoc';

/** A signal or stage name: `[a-z0-9_]`, 1 to 32 characters. */
export const NAME_RE = /^[a-z0-9_]{1,32}$/;

export function nameError(name: string, what: string): string | null {
  return NAME_RE.test(name) ? null : `${what} \`${name}\` must be 1-32 characters of a-z, 0-9 and _`;
}

/** Why one metric condition cannot be saved, or `null`. */
export function metricCondError(
  reg: StrategyRegistry | undefined,
  c: Extract<Cond, { kind: 'metric' }>,
  definedTags?: readonly string[],
): string | null {
  const refErr = checkRef(reg, c.ref, definedTags);
  if (refErr) return refErr;
  if (c.is.length === 0 || c.is.some((arm) => arm.length === 0)) return 'enter a value, e.g. >= 2';
  if (c.is.flat().some((x) => !Number.isFinite(x.value))) return 'every value must be a finite number';
  const tol = findMetric(reg, c.ref.metric)?.eq_tolerance ?? 0;
  return unsatisfiableReason(c.is, tol);
}

export interface RuleIssues {
  /** Block the save. */
  errors: string[];
  /** Save goes through, but the rule may not do what it says (the backend warns too). */
  warnings: string[];
}

/**
 * Validate a draft. `definedTags` = the tag names the rule's fingerprint defines;
 * reading any other tag is a warning (it reads nothing and never holds), as the
 * backend's `rule_tag_warning` says at save.
 */
export function validateRuleDoc(
  d: RuleDoc,
  reg: StrategyRegistry | undefined,
  definedTags?: readonly string[],
): RuleIssues {
  const errors: string[] = [];
  const warnings: string[] = [];
  const stageNames = d.stages.map((s) => s.name);

  const checkCond = (c: Cond, at: string, beforeBuy: boolean) => {
    if (c.kind === 'signal') {
      const sig = d.signals.find((s) => s.name === c.signal);
      if (!sig) {
        errors.push(`${at}: there is no signal \`${c.signal}\``);
        return;
      }
      if (beforeBuy && sig.groups.flat().some((g) => findMetric(reg, g.ref.metric)?.position)) {
        errors.push(`${at}: signal \`${c.signal}\` reads our position, which does not exist before the buy`);
      }
      return;
    }
    const tagMissing =
      definedTags && c.ref.tag && checkRef(reg, c.ref) === null && checkRef(reg, c.ref, definedTags) !== null;
    const err = metricCondError(reg, c, undefined);
    if (err) errors.push(`${at}: ${err}`);
    else if (tagMissing) warnings.push(`${at}: ${checkRef(reg, c.ref, definedTags)}`);
    if (beforeBuy && findMetric(reg, c.ref.metric)?.position) {
      errors.push(`${at}: ${c.ref.metric} reads our position, which does not exist before the buy`);
    }
  };
  const checkConds = (cs: Cond[], at: string, beforeBuy: boolean) =>
    cs.forEach((c, i) => checkCond(c, `${at}, condition ${i + 1}`, beforeBuy));

  // Numbers.
  for (const [label, v, hi] of [
    ['Take profit', d.take_profit, Infinity],
    ['Stop loss', d.stop_loss, Infinity],
    ['Size as % of pool', d.enter.size_pct_of_pool, MAX_SIZE_PCT_OF_POOL],
  ] as const) {
    if (v != null && !(Number.isFinite(v) && v > 0 && v <= hi)) {
      errors.push(`${label} must be above 0${Number.isFinite(hi) ? ` and at most ${hi}` : ''}`);
    }
  }
  if (d.reentry) {
    if (!(Number.isFinite(d.reentry.cooldown_sec) && d.reentry.cooldown_sec >= 0)) {
      errors.push('Buy again: the wait must be 0 s or more');
    }
    if (!(Number.isInteger(d.reentry.max_per_coin) && d.reentry.max_per_coin >= 1)) {
      errors.push('Buy again: the most buys per coin must be a whole number, 1 or more');
    }
  }
  if (!Number.isInteger(d.priority)) errors.push('Priority must be a whole number');

  // Enter.
  checkConds(d.enter.event, 'Buy on', true);
  checkConds(d.enter.filters, 'Only if', true);
  checkConds(d.enter.final_filters, 'Only if, else give up', true);
  if (d.enter.lock && d.enter.event.every((c) => c.off)) {
    errors.push('One chance needs a "Buy on" condition: the chance is the first print that makes it true');
  }

  // Signals.
  const seenSignals = new Set<string>();
  for (const s of d.signals) {
    const nErr = nameError(s.name, 'Signal name');
    if (nErr) errors.push(nErr);
    if (seenSignals.has(s.name)) errors.push(`Two signals are named \`${s.name}\``);
    seenSignals.add(s.name);
    if (s.groups.length === 0 || s.groups.some((g) => g.length === 0)) {
      errors.push(`Signal \`${s.name}\` has an empty group: give every group a condition or remove it`);
    }
    s.groups.forEach((g, gi) => checkConds(g, `Signal \`${s.name}\`, group ${gi + 1}`, false));
  }

  // Lines.
  const checkLine = (l: Line, at: string, conditional: boolean) => {
    if (conditional && !l.off && l.if.every((c) => c.off)) {
      errors.push(`${at} has no condition, so it would act on the first print`);
    }
    checkConds(l.if, at, false);
    if (!l.sell && !l.go) errors.push(`${at} does nothing: make it sell and/or go to a stage`);
    if (l.go && !stageNames.includes(l.go)) errors.push(`${at}: there is no stage \`${l.go}\``);
    if (l.sell?.pct != null) {
      if (!(Number.isFinite(l.sell.pct) && l.sell.pct > 0 && l.sell.pct <= MAX_SELL_PCT)) {
        errors.push(`${at}: the sell percent must be above 0 and at most ${MAX_SELL_PCT}`);
      }
      if (!l.go) {
        errors.push(`${at}: a partial sell must also go to another stage, or it would sell again on the next print`);
      }
    }
  };
  d.always.forEach((l, i) => checkLine(l, `Always, line ${i + 1}`, true));

  // Stages.
  if (d.stages.length > MAX_STAGES) errors.push(`At most ${MAX_STAGES} stages`);
  const seenStages = new Set<string>();
  d.stages.forEach((s, si) => {
    const nErr = nameError(s.name, 'Stage name');
    if (nErr) errors.push(nErr);
    if (seenStages.has(s.name)) errors.push(`Two stages are named \`${s.name}\``);
    seenStages.add(s.name);
    const at = `Stage \`${s.name}\``;
    if (s.ends && !(Number.isFinite(s.ends.secs) && s.ends.secs >= 0)) {
      errors.push(`${at}: the deadline must be 0 s or more`);
    }
    // A stage's own line that goes to that stage and keeps part of the bag would never
    // act there (the engine's `CompiledLine::idle_in`).
    const notToItself = (l: Line, lineAt: string) => {
      const sellsAll = l.sell != null && l.sell.pct == null;
      if (l.go === s.name && !sellsAll) errors.push(`${lineAt} goes to its own stage \`${s.name}\`, where it would never act`);
    };
    s.on.forEach((l, i) => {
      checkLine(l, `${at}, line ${i + 1}`, true);
      notToItself(l, `${at}, line ${i + 1}`);
    });
    s.at_end.forEach((l, i) => {
      checkLine(l, `${at}, at-deadline line ${i + 1}`, false);
      notToItself(l, `${at}, at-deadline line ${i + 1}`);
    });
    if (s.at_end.length && !s.ends) errors.push(`${at} has at-deadline lines but no deadline`);
    if (s.then) {
      if (!s.ends) errors.push(`${at}: "then" needs a deadline`);
      if (!stageNames.includes(s.then)) errors.push(`${at}: there is no stage \`${s.then}\``);
    } else if (s.ends && si === d.stages.length - 1) {
      errors.push(`${at} has a deadline but no stage follows it: pick where it goes next`);
    }
  });
  const loop = deadlineLoopWithoutWait(d.stages);
  if (loop) {
    errors.push(
      `Stages ${loop.join(' -> ')} loop on their deadlines with no wait: once the deadlines pass they would move on every print. Give one of them a "time in this stage" deadline above 0 s.`,
    );
  }
  return { errors, warnings };
}

/**
 * Port of the engine's `check_deadline_loops`: a loop of deadline moves in which no stage
 * waits a `stage_sec` above 0 (age and held deadlines stay passed, a 0 s stage deadline
 * passes on arrival). The deadline's target is the compiler's: no `then` = the next stage.
 * Returns the loop's stage names, first one repeated at the end.
 */
export function deadlineLoopWithoutWait(stages: Stage[]): string[] | null {
  const index = (name: string) => stages.findIndex((s) => s.name === name);
  const next = (i: number): number | null => {
    const s = stages[i];
    if (!s.ends) return null;
    if (s.then != null) return index(s.then) >= 0 ? index(s.then) : null;
    return i + 1 < stages.length ? i + 1 : null;
  };
  const waits = (i: number) => stages[i].ends?.basis === 'stage_sec' && stages[i].ends!.secs > 0;
  for (let start = 0; start < stages.length; start++) {
    const path = [start];
    let at = start;
    for (let n = next(at); n != null; n = next(at)) {
      if (n === start) {
        if (!path.some(waits)) return [...path, start].map((i) => stages[i].name);
        break;
      }
      if (path.includes(n)) break;
      path.push(n);
      at = n;
    }
  }
  return null;
}

/**
 * Port of the engine's `check_expr_satisfiable` over DNF: an expression can never hold
 * only when every OR arm can never hold. Within an arm, conditions AND (interval
 * intersection); `=` / `!=` contribute `±tol/2` bands. The message is the last arm's,
 * as the engine reports it.
 */
export function unsatisfiableReason(arms: ConditionExpr, tol: number): string | null {
  if (arms.length === 0) return null;
  let last: string | null = null;
  for (const arm of arms) {
    const why = armUnsatisfiableReason(arm, tol);
    if (!why) return null;
    last = why;
  }
  return last ?? 'can never hold: no OR group can hold';
}

function armUnsatisfiableReason(conds: Condition[], tol: number): string | null {
  const half = tol / 2;
  let lo = -Infinity;
  let loStrict = false;
  let hi = Infinity;
  let hiStrict = false;
  const neBands: Array<[number, number]> = [];
  const raiseLo = (v: number, strict: boolean) => {
    if (v > lo || (v === lo && strict)) {
      lo = v;
      loStrict = strict;
    }
  };
  const dropHi = (v: number, strict: boolean) => {
    if (v < hi || (v === hi && strict)) {
      hi = v;
      hiStrict = strict;
    }
  };
  for (const c of conds) {
    switch (c.operator) {
      case '>':
        raiseLo(c.value, true);
        break;
      case '>=':
        raiseLo(c.value, false);
        break;
      case '<':
        dropHi(c.value, true);
        break;
      case '<=':
        dropHi(c.value, false);
        break;
      case '=':
        raiseLo(c.value - half, false);
        dropHi(c.value + half, false);
        break;
      case '!=':
        neBands.push([c.value - half, c.value + half]);
        break;
    }
  }
  if (lo > hi || (lo === hi && (loStrict || hiStrict))) {
    return `can never hold: no value is inside every bound (they cross at ${lo})`;
  }
  for (const [bLo, bHi] of neBands) {
    if (Number.isFinite(lo) && Number.isFinite(hi) && bLo <= lo && hi <= bHi) {
      return `can never hold: the '!=' band [${bLo}, ${bHi}] rules out every value the other bounds allow`;
    }
  }
  return null;
}

export { normalizeConditionExpr } from './grammar';
