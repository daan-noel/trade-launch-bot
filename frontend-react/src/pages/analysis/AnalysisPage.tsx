import { useState } from 'react';
import { DataTable } from 'components/table/DataTable';
import { Pagination } from 'components/table/Pagination';
import { InlineAlert } from 'components/ui/Modal';
import { analysisColumns, creatorColumns } from 'components/analysis/analysisColumns';
import { Badge } from 'components/ui/Badge';
import {
  apiErrorMessage,
  useGetAnalysisPageQuery,
  useGetCreatorsPageQuery,
} from 'store/apiSlice';
import { POLL_INTERVAL_MS } from 'services/config';
import { Button } from 'components/ui/Button';

type Tab = 'creators' | 'results';

// Module-level, referentially-stable rowKey fns: each only reads the row, so a
// single shared identity lets DataTable's row memo skip churn that an inline
// `(r) => ...` would trigger on every poll-driven re-render.
const creatorKey = (c: { wallet_address: string }) => c.wallet_address;
const resultKey = (r: { analyzer_name: string; computed_at: string }) =>
  `${r.analyzer_name}-${r.computed_at}`;

export function AnalysisPage() {
  const [tab, setTab] = useState<Tab>('creators');

  const [creatorPage, setCreatorPage] = useState(1);
  const [creatorPs, setCreatorPs] = useState(25);
  const [resultPage, setResultPage] = useState(1);
  const [resultPs, setResultPs] = useState(25);

  // Both tabs subscribe so the header counts populate up front, but only the
  // ACTIVE tab polls (the inactive one's interval is 0). `skipPollingIfUnfocused`
  // (wired via setupListeners in store/index.ts) pauses polling while the window
  // is unfocused. RTK Query caches across navigation and its structural sharing
  // keeps unchanged rows referentially stable, so a poll that returns identical
  // data doesn't re-render the table.
  const creatorsQuery = useGetCreatorsPageQuery(
    { limit: creatorPs, offset: (creatorPage - 1) * creatorPs },
    { pollingInterval: tab === 'creators' ? POLL_INTERVAL_MS : 0, skipPollingIfUnfocused: true },
  );
  const resultsQuery = useGetAnalysisPageQuery(
    { limit: resultPs, offset: (resultPage - 1) * resultPs },
    { pollingInterval: tab === 'results' ? POLL_INTERVAL_MS : 0, skipPollingIfUnfocused: true },
  );

  const creators = creatorsQuery.data?.items ?? [];
  const creatorTotal = creatorsQuery.data?.total ?? 0;
  const creatorErr = apiErrorMessage(creatorsQuery.error, 'Failed to load creators');
  const creatorLoad = creatorsQuery.isLoading;

  const results = resultsQuery.data?.items ?? [];
  const resultTotal = resultsQuery.data?.total ?? 0;
  const resultErr = apiErrorMessage(resultsQuery.error, 'Failed to load results');
  const resultLoad = resultsQuery.isLoading;

  const creatorTotalPages = Math.max(1, Math.ceil(creatorTotal / creatorPs));
  const resultTotalPages = Math.max(1, Math.ceil(resultTotal / resultPs));

  return (
    <div>
      <div className="mb-3.5 flex items-center gap-2.5">
        <h2 className="text-base font-bold text-primary">Analysis</h2>
        <Badge variant="primary" className="font-mono">
          {creatorTotal} creators · {resultTotal} results
        </Badge>
      </div>

      <div className="mb-4 flex gap-2">
        {(['creators', 'results'] as const).map((t) => (
          <Button key={t} variant="ghost" active={tab === t} onClick={() => setTab(t)}>
            {t === 'creators' ? 'Creator Profiles' : 'Analysis Results'}
          </Button>
        ))}
      </div>

      {tab === 'creators' && (
        <>
          {creatorLoad && <p className="text-text-dim">Loading creator profiles…</p>}
          {creatorErr && <InlineAlert variant="error">{creatorErr}</InlineAlert>}
          {!creatorLoad && !creatorErr && (
            <>
              <DataTable
                tableId="analysis_creators"
                columns={creatorColumns}
                rows={creators}
                rowKey={creatorKey}
                searchable={false}
                colFilters={false}
                paginate={false}
                selectable={false}
                emptyMessage="No creator profiles yet."
              />
              <Pagination
                currentPage={creatorPage}
                totalPages={creatorTotalPages}
                totalItems={creatorTotal}
                pageSize={creatorPs}
                onPageChange={setCreatorPage}
                onPageSizeChange={(s) => {
                  setCreatorPs(s);
                  setCreatorPage(1);
                }}
              />
            </>
          )}
        </>
      )}

      {tab === 'results' && (
        <>
          {resultLoad && <p className="text-text-dim">Loading analysis results…</p>}
          {resultErr && <InlineAlert variant="error">{resultErr}</InlineAlert>}
          {!resultLoad && !resultErr && (
            <>
              <DataTable
                tableId="analysis_results"
                columns={analysisColumns}
                rows={results}
                rowKey={resultKey}
                searchable={false}
                colFilters={false}
                paginate={false}
                selectable={false}
                emptyMessage="No analysis results yet."
              />
              <Pagination
                currentPage={resultPage}
                totalPages={resultTotalPages}
                totalItems={resultTotal}
                pageSize={resultPs}
                onPageChange={setResultPage}
                onPageSizeChange={(s) => {
                  setResultPs(s);
                  setResultPage(1);
                }}
              />
            </>
          )}
        </>
      )}
    </div>
  );
}
