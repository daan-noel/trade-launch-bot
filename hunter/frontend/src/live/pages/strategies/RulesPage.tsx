import { useSelector } from 'react-redux';
import { RulesView } from 'components/strategy/RulesView';
import { selectRuleOpenCounts } from '@live/slices/liveStatusSlice';

/** Live Rules — activate/pause/stop + live open counts + Analyze drill-in. */
export function RulesPage() {
  const ruleLiveCounts = useSelector(selectRuleOpenCounts);
  return <RulesView linkToAnalyze ruleLiveCounts={ruleLiveCounts} />;
}
