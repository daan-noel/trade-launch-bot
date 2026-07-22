import { useEffect, useRef, useState } from 'react';

import { IconButton } from 'components/ui/IconButton';
import { SimulateIcon, SpinnerIcon } from 'components/ui/icons';
import { Select } from 'components/ui/Select';
import { apiErrorMessage } from 'store/baseApi';
import { connectSimulationFinished } from 'services/sse';
import type { RuleEditorDraft } from 'components/strategy/RuleEditor';
import {
  lamportsToSol,
  FILL_MODELS,
  type EngineRuleDraft,
  type FillModelId,
} from 'lib/strategy/types';
import type { SimulatedSummary } from 'types';
import {
  useStartEngineSimulationMutation,
  useGetEngineSimSummaryMutation,
} from '@lab/store/labEndpoints';
import { SimSummary } from './SimSummary';

const WINDOWS = [
  { label: 'last 24h', hours: 24 },
  { label: 'last 7d', hours: 168 },
  { label: 'all history', hours: 0 },
];

/**
 * The in-editor dry-run panel (FE3, lab-only): simulate the *unsaved* draft
 * against a recent creation window via the generic simulate endpoint (inline
 * draft), then show the full shared run-summary panel (same tiles as Simulate).
 * Injected into the shared `RuleEditor` via its `renderDryRun` render-prop, so
 * the shared editor never imports the lab-only simulate endpoints (boundary-clean).
 */
export function DryRunPanel({ draft, canRun }: { draft: RuleEditorDraft | null; canRun: boolean }) {
  const [start] = useStartEngineSimulationMutation();
  const [fetchSummary] = useGetEngineSimSummaryMutation();
  const [running, setRunning] = useState(false);
  const [summary, setSummary] = useState<SimulatedSummary | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [windowHours, setWindowHours] = useState(24);
  const [fillModel, setFillModel] = useState<FillModelId>('worst_case');
  const handleRef = useRef<{ close: () => void } | null>(null);
  const runIdRef = useRef<string | null>(null);

  // Close the SSE subscription on unmount.
  useEffect(() => () => handleRef.current?.close(), []);

  const run = async () => {
    if (!draft) return;
    setErr(null);
    setSummary(null);
    setRunning(true);
    const engineDraft: EngineRuleDraft = {
      fingerprint_id: draft.fingerprint_id,
      params: draft.params,
      buy_amount_sol: lamportsToSol(draft.buy_amount_lamports) ?? 0,
      max_concurrent_tokens: draft.max_concurrent_tokens,
      max_total_tokens: draft.max_total_tokens,
      trade_mode: draft.trade_mode,
    };
    const since =
      windowHours > 0
        ? new Date(Date.now() - windowHours * 3600 * 1000).toISOString()
        : undefined;
    try {
      const res = await start({ draft: engineDraft, since, fill_model: fillModel }).unwrap();
      runIdRef.current = res.run_id;
      handleRef.current?.close();
      handleRef.current = connectSimulationFinished(async (ev) => {
        if (ev.rule_id !== runIdRef.current) return;
        handleRef.current?.close();
        handleRef.current = null;
        if (ev.cancelled) {
          setRunning(false);
          return;
        }
        try {
          setSummary(await fetchSummary(res.run_id).unwrap());
        } catch (e) {
          setErr(apiErrorMessage(e as never) ?? 'summary failed');
        } finally {
          setRunning(false);
        }
      });
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'simulate failed');
      setRunning(false);
    }
  };

  return (
    <div className="mt-2 rounded-md border border-white/10 bg-white/2 p-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[12px] font-semibold text-text">Dry run</span>
        <span className="text-[11px] text-text-dim">replay</span>
        <Select
          fieldSize="sm"
          value={String(windowHours)}
          onChange={(e) => setWindowHours(Number(e.target.value))}
          className="w-28"
        >
          {WINDOWS.map((w) => (
            <option key={w.hours} value={w.hours}>
              {w.label}
            </option>
          ))}
        </Select>
        <Select
          fieldSize="sm"
          value={fillModel}
          onChange={(e) => setFillModel(e.target.value as FillModelId)}
          className="w-36"
          title={FILL_MODELS.find((m) => m.id === fillModel)?.hint}
        >
          {FILL_MODELS.map((m) => (
            <option key={m.id} value={m.id} title={m.hint}>
              {m.label}
            </option>
          ))}
        </Select>
        <IconButton
          variant="primary"
          size="lg"
          disabled={!canRun || running}
          onClick={run}
          label={running ? 'Running…' : 'Run'}
          title={running ? 'Running…' : 'Run'}
        >
          {running ? <SpinnerIcon /> : <SimulateIcon />}
        </IconButton>
        {!canRun && <span className="text-[11px] text-text-dim/70">fix the draft to enable</span>}
      </div>
      {err && <p className="mt-2 text-[11px] text-red">{err}</p>}
      {summary && <SimSummary summary={summary} />}
    </div>
  );
}
