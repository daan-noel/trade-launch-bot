import type { ReactNode } from 'react';
import { Badge } from 'components/ui/Badge';
import { HoverPopover } from 'components/ui/HoverPopover';
import { ModeBadge } from './ModeBadge';
import { ruleParamsCell } from './RuleParamsSummary';
import { fingerprintParamsCell } from './FingerprintParamsSummary';
import { lamportsToSol, type Fingerprint, type StrategyRule } from 'lib/strategy/types';

function sectionLabel(text: string): ReactNode {
  return (
    <div className="text-[10px] font-semibold uppercase tracking-wider text-text-dim">{text}</div>
  );
}

/** Full rule snapshot for hover — reuses the table chip SSOTs (no second format). */
export function RuleDetailCard({
  rule,
  fingerprint,
}: {
  rule: StrategyRule;
  fingerprint?: Fingerprint | null;
}) {
  return (
    <div className="flex flex-col gap-2.5 normal-case tracking-normal">
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <span className="min-w-0 flex-1 truncate text-[13px] font-semibold text-text">
          {rule.rule_name}
        </span>
        <ModeBadge mode={rule.trade_mode} />
        <Badge variant={rule.is_active ? 'success' : 'neutral'} size="sm">
          {rule.is_active ? 'Active' : 'Idle'}
        </Badge>
      </div>
      <div className="flex flex-wrap gap-x-3 gap-y-0.5 font-mono text-[11px] tabular-nums text-text-dim">
        <span>buy {lamportsToSol(rule.buy_amount_lamports)}◎</span>
        <span>
          caps {rule.max_concurrent_tokens}/{rule.max_total_tokens || '∞'}
        </span>
      </div>
      {fingerprint ? (
        <div className="flex flex-col gap-1">
          {sectionLabel('Fingerprint')}
          <span className="font-mono text-[11px] text-text-dim">
            {fingerprint.name || fingerprint.id.slice(0, 8)}
          </span>
          {fingerprintParamsCell(fingerprint)}
        </div>
      ) : null}
      <div className="flex flex-col gap-1">
        {sectionLabel('Params')}
        {ruleParamsCell(rule.params)}
      </div>
    </div>
  );
}

/**
 * Wrap a rule-name (or chip) so hover shows {@link RuleDetailCard}. Portal +
 * open-only mount keeps dense tables cheap.
 */
export function RuleHoverTip({
  rule,
  fingerprint,
  children,
  side = 'bottom',
}: {
  rule: StrategyRule;
  fingerprint?: Fingerprint | null;
  children: ReactNode;
  side?: 'top' | 'bottom';
}) {
  return (
    <HoverPopover
      side={side}
      content={<RuleDetailCard rule={rule} fingerprint={fingerprint} />}
    >
      {children}
    </HoverPopover>
  );
}
