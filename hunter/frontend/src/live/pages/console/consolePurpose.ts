/**
 * The Console's General / Copy split.
 *
 * One query key (`purpose`) scopes the live lanes and the History and Arms
 * sections together. Absent means General: metric rules, manual positions, and
 * any rule id that is not a copy rule. `copy` means copy rules only.
 */

import { useCallback, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import type { FilterSpec } from 'components/table/numericFilter';
import { isCopyRule } from 'lib/strategy/copyRule';
import { OPS_PARAMS } from 'lib/strategy/nav';
import type { StrategyRule } from 'lib/strategy/types';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';

export type ConsolePurpose = 'general' | 'copy';

/** A rule id that matches no row. An empty Copy set uses it so the server
 *  predicate is a real constraint: an empty `in` list is dropped and would
 *  return every row. */
export const EMPTY_RULE_SCOPE_ID = '00000000-0000-0000-0000-000000000000';

/** `include` is the Copy tab. `exclude` is General (copy-rule ids leave;
 *  a null or one-off rule id stays). */
export type RulePurposeScope =
  | { kind: 'include'; ids: readonly string[] }
  | { kind: 'exclude'; ids: readonly string[] };

export function consolePurposeFromParam(raw: string | null): ConsolePurpose {
  return raw === 'copy' ? 'copy' : 'general';
}

export function copyRuleIdSet(rules: Pick<StrategyRule, 'id' | 'params'>[]): Set<string> {
  const ids = new Set<string>();
  for (const rule of rules) {
    if (isCopyRule(rule)) ids.add(rule.id);
  }
  return ids;
}

/** Whether a live row belongs on the tab. A manual row is General. */
export function ruleMatchesPurpose(
  purpose: ConsolePurpose,
  copyIds: ReadonlySet<string>,
  ruleId: string | null | undefined,
  origin?: string | null,
): boolean {
  if (origin === 'manual' || !ruleId) return purpose === 'general';
  const copy = copyIds.has(ruleId);
  return purpose === 'copy' ? copy : !copy;
}

export function rulePurposeScope(
  purpose: ConsolePurpose,
  copyIds: ReadonlySet<string>,
): RulePurposeScope {
  const ids = [...copyIds];
  return purpose === 'copy' ? { kind: 'include', ids } : { kind: 'exclude', ids };
}

function idAllowed(scope: RulePurposeScope, id: string): boolean {
  return scope.kind === 'include' ? scope.ids.includes(id) : !scope.ids.includes(id);
}

/**
 * Fold the tab into a request's `rule_id` filter.
 *
 * A specific rule that belongs on the tab stays. One that does not becomes the
 * empty-scope id, so the page shows no rows instead of the other tab's book.
 * With no specific rule, Copy is `in` and General is `nin` (null rule ids stay).
 */
export function withPurposeRuleScope(
  filters: Record<string, FilterSpec>,
  scope: RulePurposeScope | null | undefined,
): Record<string, FilterSpec> {
  if (!scope) return filters;
  const current = filters.rule_id;
  if (current?.op === 'eq' && typeof current.val === 'string') {
    if (idAllowed(scope, current.val)) return filters;
    return { ...filters, rule_id: { op: 'eq', val: EMPTY_RULE_SCOPE_ID } };
  }
  if (scope.kind === 'include') {
    if (scope.ids.length === 0) {
      return { ...filters, rule_id: { op: 'eq', val: EMPTY_RULE_SCOPE_ID } };
    }
    return { ...filters, rule_id: { op: 'in', val: [...scope.ids] } };
  }
  if (scope.ids.length === 0) return filters;
  return { ...filters, rule_id: { op: 'nin', val: [...scope.ids] } };
}

const RULE_PARAM_KEYS = [OPS_PARAMS.rule, OPS_PARAMS.hRule, OPS_PARAMS.aRule] as const;

function clearForeignRules(
  next: URLSearchParams,
  purpose: ConsolePurpose,
  copyIds: ReadonlySet<string>,
): void {
  const ok = (id: string) => ruleMatchesPurpose(purpose, copyIds, id);
  for (const key of RULE_PARAM_KEYS) {
    const value = next.get(key);
    if (value && !ok(value)) next.delete(key);
  }
  const focus = next.get(OPS_PARAMS.hFocus);
  if (focus?.startsWith('rule:')) {
    const id = focus.slice('rule:'.length);
    if (id && !ok(id)) next.delete(OPS_PARAMS.hFocus);
  }
}

export function useConsoleRuleScope(): {
  purpose: ConsolePurpose;
  setPurpose: (id: string, opts?: { keepRules?: boolean }) => void;
  copyIds: ReadonlySet<string>;
  scope: RulePurposeScope;
  rulesReady: boolean;
  matches: (ruleId: string | null | undefined, origin?: string | null) => boolean;
} {
  const [params, setParams] = useSearchParams();
  const { data: rules = [], isSuccess, isError } = useGetStrategyRulesQuery();
  const purpose = consolePurposeFromParam(params.get(OPS_PARAMS.purpose));
  const copyIds = useMemo(() => copyRuleIdSet(rules), [rules]);
  const scope = useMemo(() => rulePurposeScope(purpose, copyIds), [purpose, copyIds]);
  const matches = useCallback(
    (ruleId: string | null | undefined, origin?: string | null) =>
      ruleMatchesPurpose(purpose, copyIds, ruleId, origin),
    [purpose, copyIds],
  );

  const setPurpose = useCallback(
    (id: string, opts?: { keepRules?: boolean }) => {
      if (id !== 'general' && id !== 'copy') return;
      setParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          if (id === 'general') next.delete(OPS_PARAMS.purpose);
          else next.set(OPS_PARAMS.purpose, 'copy');
          if (!opts?.keepRules) clearForeignRules(next, id, copyIds);
          return next;
        },
        { replace: true },
      );
    },
    [copyIds, setParams],
  );

  return { purpose, setPurpose, copyIds, scope, rulesReady: isSuccess || isError, matches };
}
