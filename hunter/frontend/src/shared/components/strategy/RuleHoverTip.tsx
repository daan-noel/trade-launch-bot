import type { ReactNode } from 'react';
import { Badge } from 'components/ui/Badge';
import { HoverPopover } from 'components/ui/HoverPopover';
import { ModeBadge } from './ModeBadge';
import { ruleChainCell } from './RuleParamsSummary';
import { fingerprintParamsCell } from './FingerprintParamsSummary';
import { capsDisplayText } from './capsRuleColumns';
import { lamportsToSol, type Fingerprint, type StrategyRule } from 'lib/strategy/types';
import { useStrategyRegistry } from 'lib/strategy/registry';
import { copyViewParams } from 'lib/strategy/copyRule';
import { ruleDocFromJson } from 'lib/strategy/ruleDoc';
import { RuleSentences } from './rule/RuleSentences';

/** The rule in words, when its params are a format-2 document. */
function RuleWords({ params, watch }: { params: unknown; watch?: string }) {
  const { data: reg } = useStrategyRegistry();
  if (!reg) return null;
  try {
    return <RuleSentences doc={ruleDocFromJson(copyViewParams(params))} reg={reg} watch={watch} />;
  } catch {
    return null;
  }
}

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
        <span>caps {capsDisplayText(rule)}</span>
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
        {ruleChainCell(rule.params)}
      </div>
      <div className="flex max-w-xl flex-col gap-1">
        {sectionLabel('In words')}
        <RuleWords params={rule.params} watch={fingerprint?.name} />
      </div>
    </div>
  );
}

/** How long the pointer rests before the In words reading opens. */
export const RULE_WORDS_TIP_DELAY_MS = 1_000;

/**
 * Wrap a cell so hover shows the rule. Portal + open-only mount keeps dense
 * tables cheap. `openDelayMs` holds the panel until the pointer has rested
 * that long. `wordsOnly` is the In words reading and nothing else.
 */
export function RuleHoverTip({
  rule,
  fingerprint,
  children,
  side = 'bottom',
  openDelayMs = 0,
  fillCell = false,
  wordsOnly = false,
  className,
}: {
  rule: StrategyRule;
  fingerprint?: Fingerprint | null;
  children: ReactNode;
  side?: 'top' | 'bottom';
  openDelayMs?: number;
  /** Cover the whole cell. The column's `td` must be `relative`. */
  fillCell?: boolean;
  /** In words only: the sentence reading, with no name, caps, or chain. */
  wordsOnly?: boolean;
  className?: string;
}) {
  return (
    <HoverPopover
      side={side}
      openDelayMs={openDelayMs}
      fillCell={fillCell}
      className={className}
      width={wordsOnly ? 880 : undefined}
      content={
        wordsOnly ? (
          <div className="normal-case tracking-normal">
            <RuleWords params={rule.params} watch={fingerprint?.name} />
          </div>
        ) : (
          <RuleDetailCard rule={rule} fingerprint={fingerprint} />
        )
      }
    >
      {children}
    </HoverPopover>
  );
}
