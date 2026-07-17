import { useState } from 'react';
import { useSearchParams } from 'react-router-dom';

import { Input } from 'components/ui/Input';
import { Button } from 'components/ui/Button';
import { apiErrorMessage, useGetTokenDetailQuery } from 'store/apiSlice';
import { LabTokenInspect } from '@lab/components/strategy/LabTokenInspect';

/**
 * Token metric panes (lab). Paste a mint (or `?mint=`) to load the trade-history
 * chart with registry-driven metric panes underneath — shared crosshair / visible
 * range, rule thresholds, and first metric entry/exit markers on the price chart.
 */
export function MetricPanesPage() {
  const [params, setParams] = useSearchParams();
  const mint = params.get('mint') ?? '';
  const [draft, setDraft] = useState(mint);

  const {
    data: detail,
    isFetching: detailLoading,
    error: detailErrorRaw,
  } = useGetTokenDetailQuery(mint, { skip: !mint });
  const detailError = detailErrorRaw ? apiErrorMessage(detailErrorRaw) : null;

  return (
    <div className="flex flex-col gap-3 p-4">
      <div>
        <h1 className="text-lg font-semibold text-text">Token metric panes</h1>
        <p className="mt-0.5 text-[12px] text-text-dim">
          Inspect engine metrics over a token&apos;s life against a rule&apos;s thresholds,
          aligned with the trade chart.
        </p>
      </div>
      <form
        className="flex items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          setParams(draft.trim() ? { mint: draft.trim() } : {});
        }}
      >
        <Input
          fieldSize="sm"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="token mint address"
          className="max-w-md font-mono"
        />
        <Button variant="primary" size="sm" type="submit">
          Load
        </Button>
      </form>
      {mint ? (
        <div className="rounded-lg border border-white/6 bg-bg-panel p-3">
          <LabTokenInspect
            detail={detail ?? null}
            loading={detailLoading}
            error={detailError}
            tableId="metric_panes_trades"
          />
        </div>
      ) : (
        <p className="text-[12px] text-text-dim/70">
          Enter a mint address to load its trade chart and metric panes.
        </p>
      )}
    </div>
  );
}
