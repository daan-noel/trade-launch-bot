import type { ReactNode } from 'react';
import { ruleParamsHeaderStrip } from 'components/strategy/RuleParamsSummary';
import type { MetricPanesRuleOverride } from '@lab/components/strategy/MetricPanes';

/** Modal title row: token heading + optional pinned rule params (always visible above chart). */
export function labTokenInspectModalTitle({
  heading,
  titleSuffix,
  ruleOverride = null,
}: {
  heading: string;
  titleSuffix: string;
  ruleOverride?: MetricPanesRuleOverride | null;
}): ReactNode {
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <h2 className="truncate text-[15px] font-bold text-text">
        {heading} — {titleSuffix}
      </h2>
      {ruleOverride && (
        <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
          <span
            className="shrink-0 rounded border border-white/10 bg-white/5 px-1.5 py-0.5 text-[10px] font-medium text-text-mid"
            title="Inspected rule"
          >
            {ruleOverride.label}
          </span>
          <div className="min-w-0 flex-1">{ruleParamsHeaderStrip(ruleOverride.paramsJson)}</div>
        </div>
      )}
    </div>
  );
}
