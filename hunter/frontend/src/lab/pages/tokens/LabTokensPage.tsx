import { TokensPage } from 'pages/tokens/TokensPage';
import { LazyLabTokenInspect } from '@lab/components/strategy/LazyLabTokenInspect';

/**
 * Lab Tokens page — same table as live, but the detail panel mounts chart +
 * metric panes (shared crosshair / rule overlays) instead of the bare chart.
 */
export function LabTokensPage() {
  return (
    <TokensPage
      renderDetailChart={({ detail }) => (
        <LazyLabTokenInspect
          detail={detail}
          showDetailPanel={false}
          tableId="token_detail_trades"
        />
      )}
    />
  );
}
