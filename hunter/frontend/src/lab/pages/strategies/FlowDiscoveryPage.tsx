import { useMemo, useState } from 'react';
import { Accordion } from 'components/ui/Accordion';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Checkbox } from 'components/ui/Checkbox';
import { Input } from 'components/ui/Input';
import { InlineAlert } from 'components/ui/Modal';
import { VolumeIxPatternsEditor } from 'components/strategy/VolumeIxPatternsEditor';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { apiErrorMessage } from 'store/baseApi';
import { useGetFingerprintsQuery, useUpdateFingerprintMutation } from 'store/sharedEndpoints';
import {
  metricConfigWithVolumePatterns,
  volumeIxPatternsFromConfig,
} from 'lib/strategy/registry';
import { FingerprintGroupPicker } from '@lab/components/sweep/FingerprintGroupPicker';
import { parseIxLabelsFilter, parseNumbers } from '@lab/components/sweep/fingerprintFilters';
import {
  GROUP_FIELDS,
  type GroupField,
} from '@lab/components/sweep/groupedTypes';
import {
  useBackgroundJobActions,
  useBackgroundJobsState,
} from '@lab/context/BackgroundJobsContext';
import {
  useBindFlowDiscoveryMutation,
  useLazyGetFlowDiscoveryQuery,
  useStartFlowDiscoveryMutation,
} from '@lab/store/labEndpoints';
import type { FlowDiscoveryGroup, FlowDiscoveryResult } from 'types';
import type { Fingerprint } from 'lib/strategy/types';

interface DiscoveryConfig {
  createdAfter: string;
  createdBefore: string;
  groupBy: GroupField[];
  ixLabelsFilter: string;
  cashbackFilter: 'all' | 'true' | 'false';
  fieldFiltersText: Record<string, string>;
  minTokens: number;
  tokenCap: number;
  curveOnly: boolean;
  bucketWidthSol: number;
}

const DEFAULTS: DiscoveryConfig = {
  createdAfter: '',
  createdBefore: '',
  groupBy: ['cu_limit'],
  ixLabelsFilter: '',
  cashbackFilter: 'all',
  fieldFiltersText: {},
  minTokens: 3,
  tokenCap: 5000,
  curveOnly: false,
  bucketWidthSol: 0.1,
};

function toUtc(local: string): string | undefined {
  if (!local) return undefined;
  const d = new Date(local.endsWith('Z') ? local : `${local}Z`);
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString();
}

function fmt(n: number, digits = 1): string {
  if (!Number.isFinite(n)) return '—';
  return n.toFixed(digits);
}

function groupKeyLabel(gk: Record<string, string>): string {
  const parts = Object.entries(gk).map(([k, v]) => `${k}=${v}`);
  return parts.length ? parts.join(' · ') : 'ALL';
}

/** Match a discovery group_key to a saved fingerprint (best-effort on identity axes). */
function matchFingerprint(
  gk: Record<string, string>,
  fps: Fingerprint[],
  bucketWidth: number,
): Fingerprint | null {
  const cu = gk.cu_limit != null ? Number(gk.cu_limit) : null;
  const ix = gk.ix_labels?.split(' | ').filter(Boolean) ?? null;
  const hits = fps.filter((fp) => {
    if (cu != null && !Number.isNaN(cu) && fp.cu_limit !== cu) return false;
    if (ix && JSON.stringify(fp.ix_labels ?? []) !== JSON.stringify(ix)) return false;
    if (Math.abs((fp.bucket_size_amount ?? 0.1) - bucketWidth) > 1e-9) return false;
    return cu != null || (ix != null && ix.length > 0);
  });
  return hits[0] ?? null;
}

/**
 * Lab page: score trade ix-structures per fingerprint group, toggle volume
 * patterns, apply via fingerprint update or promote-style bind.
 */
export function FlowDiscoveryPage() {
  const [stored, setConfig] = useLocalStorage<DiscoveryConfig>(
    'hunter.lab.flowDiscovery.config',
    DEFAULTS,
  );
  const config: DiscoveryConfig = {
    ...DEFAULTS,
    ...stored,
    groupBy: (stored.groupBy ?? DEFAULTS.groupBy).filter((f): f is GroupField =>
      (GROUP_FIELDS as readonly string[]).includes(f),
    ),
  };
  const {
    createdAfter,
    createdBefore,
    groupBy,
    ixLabelsFilter,
    cashbackFilter,
    fieldFiltersText,
    minTokens,
    tokenCap,
    curveOnly,
    bucketWidthSol,
  } = config;

  function setField<K extends keyof DiscoveryConfig>(key: K, value: DiscoveryConfig[K]) {
    setConfig((prev) => ({ ...DEFAULTS, ...prev, [key]: value }));
  }

  const ixLabelsGrouped = groupBy.includes('ix_labels');
  const ixFilter = useMemo(() => parseIxLabelsFilter(ixLabelsFilter), [ixLabelsFilter]);
  const ixFilterError = !ixLabelsGrouped ? ixFilter.error : null;

  const { markStarting, markFinished } = useBackgroundJobActions();
  const { isRunning } = useBackgroundJobsState();
  const running = isRunning('discovery', 'discovery');

  const [startDiscovery, startState] = useStartFlowDiscoveryMutation();
  const [fetchResult] = useLazyGetFlowDiscoveryQuery();
  const [bindFp, bindState] = useBindFlowDiscoveryMutation();
  const [updateFp, updateState] = useUpdateFingerprintMutation();
  const { data: fingerprints = [] } = useGetFingerprintsQuery();

  const [result, setResult] = useState<FlowDiscoveryResult | null>(null);
  const [selectedGroupIdx, setSelectedGroupIdx] = useState(0);
  const [draftPatterns, setDraftPatterns] = useState<string[][]>([]);
  const [applyError, setApplyError] = useState<string | null>(null);
  const [applyOk, setApplyOk] = useState<string | null>(null);

  const selectedGroup: FlowDiscoveryGroup | null =
    result?.groups[selectedGroupIdx] ?? null;
  const boundFp = selectedGroup
    ? matchFingerprint(selectedGroup.group_key, fingerprints, bucketWidthSol)
    : null;
  const currentPatterns = volumeIxPatternsFromConfig(boundFp?.metric_config ?? {});

  async function handleRun() {
    if (running || ixFilterError) return;
    setApplyError(null);
    setApplyOk(null);
    const fieldFilters: Record<string, (number | boolean)[]> = {};
    for (const f of GROUP_FIELDS) {
      if (f === 'ix_labels' || f === 'is_cashback_enabled') continue;
      const nums = parseNumbers(fieldFiltersText[f] ?? '');
      if (nums.length > 0) fieldFilters[f] = nums;
    }
    if (cashbackFilter !== 'all') fieldFilters.is_cashback_enabled = [cashbackFilter === 'true'];

    markStarting('discovery', 'discovery', 'Flow discovery');
    try {
      const { run_id } = await startDiscovery({
        created_after: toUtc(createdAfter),
        created_before: toUtc(createdBefore),
        curve_only: curveOnly,
        group_by: groupBy,
        bucket_width_sol: bucketWidthSol,
        ix_labels_filter: !ixLabelsGrouped && ixFilter.labels ? ixFilter.labels : undefined,
        field_filters: Object.keys(fieldFilters).length > 0 ? fieldFilters : undefined,
        min_tokens: minTokens,
        token_cap: tokenCap,
      }).unwrap();

      // Poll briefly until the finished SSE lands the result (or timeout).
      for (let i = 0; i < 120; i++) {
        await new Promise((r) => setTimeout(r, 500));
        try {
          const res = await fetchResult(run_id).unwrap();
          setResult(res);
          setSelectedGroupIdx(0);
          setDraftPatterns([]);
          markFinished('discovery', 'discovery');
          return;
        } catch {
          /* still running */
        }
      }
      markFinished('discovery', 'discovery');
      setApplyError('Timed out waiting for discovery result — check the jobs indicator.');
    } catch (e) {
      markFinished('discovery', 'discovery');
      setApplyError(apiErrorMessage(e as never, 'Failed to start discovery'));
    }
  }

  function toggleStructure(labels: string[]) {
    const key = JSON.stringify(labels);
    setDraftPatterns((prev) => {
      const has = prev.some((p) => JSON.stringify(p) === key);
      if (has) return prev.filter((p) => JSON.stringify(p) !== key);
      return [...prev, labels];
    });
    setApplyOk(null);
  }

  async function handleApply() {
    if (!selectedGroup || draftPatterns.length === 0) return;
    setApplyError(null);
    setApplyOk(null);
    const patterns = draftPatterns.map((p) => p.map((s) => s.trim()).filter(Boolean)).filter((p) => p.length > 0);
    try {
      if (boundFp) {
        await updateFp({
          id: boundFp.id,
          body: {
            name: boundFp.name,
            cu_limit: boundFp.cu_limit,
            cu_price: boundFp.cu_price,
            init_buy_lamports: boundFp.init_buy_lamports,
            max_cost_lamports: boundFp.max_cost_lamports,
            spendable_lamports_in: boundFp.spendable_lamports_in,
            first_slot_buy_lamports: boundFp.first_slot_buy_lamports,
            first_slot_sell_lamports: boundFp.first_slot_sell_lamports,
            bucket_size_amount: boundFp.bucket_size_amount,
            ix_labels: boundFp.ix_labels,
            metric_config: metricConfigWithVolumePatterns(patterns),
          },
        }).unwrap();
        setApplyOk(`Updated fingerprint “${boundFp.name}”.`);
      } else {
        const fp = await bindFp({
          group_key: selectedGroup.group_key,
          bucket_width_sol: bucketWidthSol,
          volume_ix_patterns: patterns,
          name: `flow · ${groupKeyLabel(selectedGroup.group_key)}`,
        }).unwrap();
        setApplyOk(`Bound fingerprint “${fp.name}”.`);
      }
    } catch (e) {
      setApplyError(apiErrorMessage(e as never, 'Failed to apply patterns'));
    }
  }

  const applying = bindState.isLoading || updateState.isLoading;

  return (
    <div className="pt-2">
      <div className="mb-3 flex flex-wrap items-baseline gap-3">
        <h1 className="text-xl font-extrabold text-text">Flow discovery</h1>
        <span className="text-sm text-text-mid">
          Rank ix structures per fingerprint group → toggle volume patterns
        </span>
      </div>

      <div className="mb-4 flex flex-wrap items-end gap-3 bg-surface">
        <div className="flex flex-col gap-1">
          <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
            Created range · UTC
          </span>
          <div className="flex items-center gap-1">
            <Input
              type="datetime-local"
              value={createdAfter}
              onChange={(e) => setField('createdAfter', e.target.value)}
            />
            <span className="text-[10px] text-text-dim/50">–</span>
            <Input
              type="datetime-local"
              value={createdBefore}
              onChange={(e) => setField('createdBefore', e.target.value)}
            />
          </div>
        </div>
        <div className="flex flex-col gap-1 w-[120px]">
          <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
            Min tokens
          </span>
          <Input
            type="number"
            min={1}
            value={minTokens}
            onChange={(e) => setField('minTokens', Math.max(1, Number(e.target.value) || 1))}
          />
        </div>
        <div className="flex flex-col gap-1 w-[120px]">
          <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
            Token cap
          </span>
          <Input
            type="number"
            min={1}
            value={tokenCap}
            onChange={(e) => setField('tokenCap', Math.max(1, Number(e.target.value) || 1))}
          />
        </div>
        <label className="flex h-[34px] items-center gap-1.5 text-sm text-text-mid">
          <Checkbox
            checked={curveOnly}
            onChange={(e) => setField('curveOnly', e.target.checked)}
          />
          curve only
        </label>
        <Button
          variant="primary"
          onClick={handleRun}
          disabled={running || !!ixFilterError || startState.isLoading}
        >
          {running ? 'Discovering…' : 'Run discovery'}
        </Button>
      </div>

      <div className="mb-4 border-t border-white/10 pt-3">
        <Accordion title="Group by fingerprint" defaultOpen>
          <FingerprintGroupPicker
            groupBy={groupBy}
            onToggleField={(f) =>
              setField(
                'groupBy',
                groupBy.includes(f) ? groupBy.filter((x) => x !== f) : [...groupBy, f],
              )
            }
            fieldFiltersText={fieldFiltersText}
            onSetFieldFilter={(field, value) =>
              setConfig((prev) => {
                const base = { ...DEFAULTS, ...prev };
                return {
                  ...base,
                  fieldFiltersText: { ...base.fieldFiltersText, [field]: value },
                };
              })
            }
            cashbackFilter={cashbackFilter}
            onSetCashback={(v) => setField('cashbackFilter', v)}
            bucketWidthSol={bucketWidthSol}
            onSetBucketWidth={(n) => setField('bucketWidthSol', n <= 0 ? 0.1 : n)}
            ixLabelsText={ixLabelsFilter}
            onSetIxLabels={(v) => setField('ixLabelsFilter', v)}
            ixFilter={ixFilter}
            emptyHint='No fields selected → one "ALL" group (noisy lift).'
          />
        </Accordion>
      </div>

      {(applyError || ixFilterError || startState.error) && (
        <InlineAlert variant="error">
          {applyError || ixFilterError || apiErrorMessage(startState.error, 'Error')}
        </InlineAlert>
      )}
      {applyOk && <InlineAlert variant="success">{applyOk}</InlineAlert>}

      {result && (
        <div className="flex flex-col gap-3 lg:flex-row">
          <div className="lg:w-72 shrink-0 flex flex-col gap-1">
            <h2 className="text-xs font-semibold uppercase tracking-wide text-text-dim mb-1">
              Groups ({result.groups.length})
            </h2>
            {result.groups.length === 0 ? (
              <p className="text-xs text-text-dim">No groups survived min_tokens.</p>
            ) : (
              result.groups.map((g, i) => (
                <button
                  key={i}
                  type="button"
                  onClick={() => {
                    setSelectedGroupIdx(i);
                    setDraftPatterns([]);
                    setApplyOk(null);
                  }}
                  className={`rounded border px-2 py-1.5 text-left text-xs transition ${
                    i === selectedGroupIdx
                      ? 'border-accent/50 bg-accent/10 text-text'
                      : 'border-white/8 text-text-mid hover:border-white/20'
                  }`}
                >
                  <div className="flex items-center gap-1.5">
                    <span className="truncate font-mono">{groupKeyLabel(g.group_key)}</span>
                    {g.ambiguity && <Badge variant="warning">ambig</Badge>}
                  </div>
                  <div className="mt-0.5 text-[10px] text-text-dim">
                    {g.n_tokens} tokens · {g.n_trades_scored.toLocaleString()} trades
                  </div>
                </button>
              ))
            )}
          </div>

          {selectedGroup && (
            <div className="min-w-0 flex-1 flex flex-col gap-3">
              <div className="flex flex-wrap items-center gap-2">
                <h2 className="text-sm font-bold text-text">
                  {groupKeyLabel(selectedGroup.group_key)}
                </h2>
                {selectedGroup.ambiguity && (
                  <Badge variant="warning">top structure lift ≈ 1 — split may be noisy</Badge>
                )}
                {boundFp ? (
                  <Badge variant="info">bound · {boundFp.name}</Badge>
                ) : (
                  <Badge variant="neutral">unbound — Create / bind</Badge>
                )}
              </div>

              <div className="overflow-x-auto rounded border border-white/8">
                <table className="w-full text-left text-[11px]">
                  <thead className="bg-white/3 text-text-dim">
                    <tr>
                      <th className="px-2 py-1.5 font-semibold">Vol</th>
                      <th className="px-2 py-1.5 font-semibold">Structure</th>
                      <th className="px-2 py-1.5 font-semibold">Lift</th>
                      <th className="px-2 py-1.5 font-semibold">Share%</th>
                      <th className="px-2 py-1.5 font-semibold">Wash</th>
                      <th className="px-2 py-1.5 font-semibold">Recur%</th>
                      <th className="px-2 py-1.5 font-semibold">Burst%</th>
                      <th className="px-2 py-1.5 font-semibold">Reuse</th>
                      <th className="px-2 py-1.5 font-semibold">Gross◎</th>
                    </tr>
                  </thead>
                  <tbody>
                    {selectedGroup.structures.map((s) => {
                      const key = JSON.stringify(s.ix_labels);
                      const on = draftPatterns.some((p) => JSON.stringify(p) === key);
                      return (
                        <tr
                          key={key}
                          className={`border-t border-white/6 ${on ? 'bg-accent/8' : ''}`}
                        >
                          <td className="px-2 py-1">
                            <Checkbox checked={on} onChange={() => toggleStructure(s.ix_labels)} />
                          </td>
                          <td className="px-2 py-1">
                            <IxLabelsDisplay labels={s.ix_labels} />
                          </td>
                          <td className="px-2 py-1 font-mono tabular-nums">{fmt(s.group_lift, 2)}</td>
                          <td className="px-2 py-1 font-mono tabular-nums">{fmt(s.volume_share)}</td>
                          <td className="px-2 py-1 font-mono tabular-nums">{fmt(s.wash_symmetry, 2)}</td>
                          <td className="px-2 py-1 font-mono tabular-nums">
                            {fmt(s.cross_token_recurrence)}
                          </td>
                          <td className="px-2 py-1 font-mono tabular-nums">{fmt(s.slot_burst)}</td>
                          <td className="px-2 py-1 font-mono tabular-nums">{fmt(s.wallet_reuse, 2)}</td>
                          <td className="px-2 py-1 font-mono tabular-nums">{fmt(s.gross_sol)}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>

              <div className="rounded border border-white/8 p-3">
                <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                  <span className="text-xs font-semibold text-text-mid">
                    Draft volume_ix_patterns
                    {boundFp && currentPatterns.length > 0 && (
                      <span className="ml-2 font-normal text-text-dim">
                        (current: {currentPatterns.length} pattern
                        {currentPatterns.length === 1 ? '' : 's'})
                      </span>
                    )}
                  </span>
                  <Button
                    variant="primary"
                    size="sm"
                    disabled={draftPatterns.length === 0 || applying}
                    onClick={handleApply}
                  >
                    {boundFp ? 'Update fingerprint' : 'Create / bind fingerprint'}
                  </Button>
                </div>
                <VolumeIxPatternsEditor patterns={draftPatterns} onChange={setDraftPatterns} />
                {boundFp && currentPatterns.length > 0 && (
                  <details className="mt-2 text-[10px] text-text-dim">
                    <summary className="cursor-pointer">Current config</summary>
                    <pre className="mt-1 overflow-x-auto rounded bg-black/20 p-2 font-mono">
                      {JSON.stringify(metricConfigWithVolumePatterns(currentPatterns), null, 2)}
                    </pre>
                  </details>
                )}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
