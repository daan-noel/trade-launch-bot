import { useCallback, useEffect, useState } from 'react';
import { DataTable } from '../components/table/DataTable';
import { Pagination } from '../components/table/Pagination';
import { InlineAlert } from '../components/ui/Modal';
import { analysisColumns, creatorColumns } from '../components/analysis/analysisColumns';
import { fetchAnalysis, fetchCreators } from '../services/api';
import { POLL_INTERVAL_MS } from '../services/config';
import type { AnalysisRecord, CreatorRecord } from '../types';
import { cn } from '../lib/cn';

type Tab = 'creators' | 'results';


export function AnalysisPage() {
  const [tab, setTab] = useState<Tab>('creators');

  const [creators, setCreators] = useState<CreatorRecord[]>([]);
  const [creatorTotal, setCreatorTotal] = useState(0);
  const [creatorPage, setCreatorPage] = useState(1);
  const [creatorPs, setCreatorPs] = useState(25);
  const [creatorLoad, setCreatorLoad] = useState(true);
  const [creatorErr, setCreatorErr] = useState<string | null>(null);

  const [results, setResults] = useState<AnalysisRecord[]>([]);
  const [resultTotal, setResultTotal] = useState(0);
  const [resultPage, setResultPage] = useState(1);
  const [resultPs, setResultPs] = useState(25);
  const [resultLoad, setResultLoad] = useState(true);
  const [resultErr, setResultErr] = useState<string | null>(null);

  const loadCreators = useCallback(async () => {
    try {
      const offset = (creatorPage - 1) * creatorPs;
      const data = await fetchCreators(creatorPs, offset);
      setCreators(data.items);
      setCreatorTotal(data.total);
      setCreatorErr(null);
    } catch (e) {
      setCreatorErr(e instanceof Error ? e.message : 'Failed to load creators');
    } finally {
      setCreatorLoad(false);
    }
  }, [creatorPage, creatorPs]);

  const loadResults = useCallback(async () => {
    try {
      const offset = (resultPage - 1) * resultPs;
      const data = await fetchAnalysis(resultPs, offset);
      setResults(data.items);
      setResultTotal(data.total);
      setResultErr(null);
    } catch (e) {
      setResultErr(e instanceof Error ? e.message : 'Failed to load results');
    } finally {
      setResultLoad(false);
    }
  }, [resultPage, resultPs]);

  useEffect(() => {
    loadCreators();
    const id = setInterval(loadCreators, POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [loadCreators]);

  useEffect(() => {
    loadResults();
    const id = setInterval(loadResults, POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [loadResults]);

  const creatorTotalPages = Math.max(1, Math.ceil(creatorTotal / creatorPs));
  const resultTotalPages = Math.max(1, Math.ceil(resultTotal / resultPs));

  return (
    <div>
      <div className="mb-3.5 flex items-center gap-2.5">
        <h2 className="text-base font-bold text-primary">Analysis</h2>
        <span className="rounded-md border border-primary bg-primary/15 px-2.5 py-0.5 font-mono text-[11px] font-bold tracking-wide text-primary">
          {creatorTotal} creators · {resultTotal} results
        </span>
      </div>

      <div className="mb-4 flex gap-2">
        {(['creators', 'results'] as const).map((t) => (
          <button
            key={t}
            type="button"
            onClick={() => setTab(t)}
            className={cn(
              'rounded-md border px-4 py-1.5 text-[13px] font-medium transition',
              tab === t
                ? 'border-primary bg-primary/10 font-bold text-primary'
                : 'border-border bg-bg-card text-text-mid hover:border-primary hover:text-text',
            )}
          >
            {t === 'creators' ? 'Creator Profiles' : 'Analysis Results'}
          </button>
        ))}
      </div>

      {tab === 'creators' && (
        <>
          {creatorLoad && <p className="text-text-dim">Loading creator profiles…</p>}
          {creatorErr && <InlineAlert variant="error">{creatorErr}</InlineAlert>}
          {!creatorLoad && !creatorErr && (
            <>
              <DataTable
                columns={creatorColumns}
                rows={creators}
                rowKey={(c) => c.wallet_address}
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
                columns={analysisColumns}
                rows={results}
                rowKey={(r) => `${r.analyzer_name}-${r.computed_at}`}
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
