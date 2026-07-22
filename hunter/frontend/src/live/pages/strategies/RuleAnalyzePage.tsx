import { Link, useParams } from 'react-router-dom';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import { rulesHref } from 'lib/strategy/nav';
import { InlineAlert } from 'components/ui/Modal';
import { RuleAnalyzePanel } from 'components/strategy/RuleAnalyzePanel';

/**
 * Standalone Evidence route — same panel as the Rules Control embed.
 * Prefer selecting a rule on `/strategies/rules?rule=`; this route remains for
 * deep links and bookmarks.
 */
export function RuleAnalyzePage() {
  const { ruleId = '' } = useParams<{ ruleId: string }>();
  const { data: rules = [] } = useGetStrategyRulesQuery('current');
  const rule = rules.find((r) => r.id === ruleId);

  if (!ruleId) {
    return <InlineAlert variant="error">Missing rule id.</InlineAlert>;
  }

  return (
    <div className="flex flex-col gap-4">
      <Link
        to={rulesHref(ruleId)}
        className="text-sm text-accent hover:text-primary hover:underline"
      >
        ← Rules Control
      </Link>
      <RuleAnalyzePanel ruleId={ruleId} rule={rule ?? null} liveUpdates />
    </div>
  );
}
