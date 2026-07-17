import { TokensPage } from 'pages/tokens/TokensPage';
import { LabTokenInspect } from '@lab/components/strategy/LabTokenInspect';

/**
 * Lab Tokens page — same table as live, but the detail panel mounts chart +
 * metric panes (shared crosshair / rule overlays) instead of the bare chart.
 */
export function LabTokensPage() {
  return (
    <TokensPage
      renderDetailChart={({ detail }) => (
        <LabTokenInspect
          detail={detail}
          showDetailPanel={false}
          tableId="token_detail_trades"
        />
      )}
    />
  );
}
