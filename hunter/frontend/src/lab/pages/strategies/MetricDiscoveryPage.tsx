import { useEffect, useState } from 'react';

import { Badge } from 'components/ui/Badge';
import { Checkbox } from 'components/ui/Checkbox';
import { IconButton } from 'components/ui/IconButton';
import { PlayIcon, SpinnerIcon } from 'components/ui/icons';
import { Input } from 'components/ui/Input';
import { InlineAlert } from 'components/ui/Modal';
import { LabelTip } from 'components/strategy/LabelTip';
import { FingerprintScopeControl } from 'components/strategy/FingerprintScopeControl';
import { useFingerprintMatches } from '@lab/components/strategy/useFingerprintMatches';
import { PageHeader } from 'components/ui/PageHeader';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { apiErrorMessage } from 'store/baseApi';
import { sseSubscribe } from 'services/sse';
import { useGetFingerprintsQuery } from 'store/sharedEndpoints';
import { solToLamports, type Fingerprint } from 'lib/strategy/types';
import type { PromotedRuleDraft } from 'lib/strategy/types';
import { PromoteRuleModal } from '@lab/components/sweep/PromoteRuleModal';
import {
  useGetLastMetricDiscoveryQuery,
  useLazyGetMetricDiscoveryQuery,
  useStartMetricDiscoveryMutation,
} from '@lab/store/labEndpoints';
import type {
  CandidateValidation,
  FamilyResult,
  MetricResponse,
  PipelineDto,
} from '@lab/lib/metricDiscoveryTypes';

interface Config {
  createdAfter: string;
  createdBefore: string;
  tokenCap: number;
  curveOnly: boolean;
  fingerprintId: string | null;
  takeProfitPct: number;
  stopLossPct: number;
  minClosed: number;
  splitFraction: number;
}

const DEFAULTS: Config = {
  createdAfter: '',
  createdBefore: '',
  tokenCap: 5000,
  curveOnly: false,
  fingerprintId: null,
  takeProfitPct: 30,
  stopLossPct: 15,
  minClosed: 20,
  splitFraction: 0.7,
};

function toUtc(local: string): string | undefined {
  if (!local) return undefined;
  const d = new Date(local.endsWith('Z') ? local : `${local}Z`);
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString();
}

const fmt = (n: number | null | undefined, digits = 2): string =>
  n == null || !Number.isFinite(n) ? '—' : n.toFixed(digits);

type Variant = 'success' | 'warning' | 'danger' | 'neutral' | 'info';

const verdictBadge: Record<string, Variant> = {
  keep: 'success',
  holds: 'success',
  independent: 'info',
  interacting: 'warning',
  degraded: 'warning',
  inconclusive: 'neutral',
  drop_no_edge: 'neutral',
  drop_spike: 'danger',
  drop_thin: 'neutral',
  drop_no_baseline: 'neutral',
  failed: 'danger',
  thin_validate: 'neutral',
  no_fire_validate: 'neutral',
  unrankable_train: 'neutral',
};

function badgeVariant(tag: string): Variant {
  return verdictBadge[tag] ?? 'neutral';
}

/**
 * Lab page: run the metric-combo discovery pipeline (screen → family-grid →
 * validate) over a token cohort and show the ranked shortlist, family winners +
 * interaction map, and out-of-sample verdicts. A validated winner promotes into
 * the shared rule editor (reuses the sweep's Promote modal) against a fingerprint.
 *
 * Plan: `hunter/docs/roadmap/metric-combo-discovery.md`.
 */
export function MetricDiscoveryPage() {
  const [stored, setConfig] = useLocalStorage<Config>('hunter.lab.metricDiscovery.config', DEFAULTS);
  const config: Config = { ...DEFAULTS, ...stored };
  const set = <K extends keyof Config>(key: K, value: Config[K]) =>
    setConfig((prev) => ({ ...DEFAULTS, ...prev, [key]: value }));

  const { data: fingerprints = [] } = useGetFingerprintsQuery();
  const selectedFp = config.fingerprintId
    ? fingerprints.find((f) => f.id === config.fingerprintId)
    : undefined;
  const fpMatches = useFingerprintMatches(config.fingerprintId, selectedFp?.name);
  const [start, startState] = useStartMetricDiscoveryMutation();
  const [fetchResult] = useLazyGetMetricDiscoveryQuery();
  const { data: lastResult } = useGetLastMetricDiscoveryQuery();

  const [result, setResult] = useState<PipelineDto | null>(null);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<{ processed: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [promoteDraft, setPromoteDraft] = useState<PromotedRuleDraft | null>(null);

  // Rehydrate the last cached result on mount, unless this session already has one.
  useEffect(() => {
    if (lastResult && !result) setResult(lastResult.result);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once, when the cache arrives
  }, [lastResult]);

  // Live progress from the pipeline's own SSE channel (page-local — the run is
  // single-flight so no id filter is needed).
  useEffect(() => {
    const off = sseSubscribe('metric_discovery_progress', (e) => {
      if (typeof e.data !== 'string') return;
      try {
        const p = JSON.parse(e.data) as { processed: number; total: number };
        setProgress(p.total > 0 ? { processed: p.processed, total: p.total } : null);
      } catch {
        /* ignore malformed frames */
      }
    });
    return off;
  }, []);

  async function handleRun() {
    if (running) return;
    setError(null);
    setRunning(true);
    setProgress(null);
    try {
      const { run_id } = await start({
        created_after: toUtc(config.createdAfter),
        created_before: toUtc(config.createdBefore),
        curve_only: config.curveOnly,
        token_cap: config.tokenCap,
        fingerprint_id: config.fingerprintId ?? undefined,
        take_profit_pct: config.takeProfitPct,
        stop_loss_pct: config.stopLossPct,
        min_closed: config.minClosed,
        split_fraction: config.splitFraction,
      }).unwrap();

      // Poll until the finished SSE lands the result server-side (or timeout).
      for (let i = 0; i < 600; i++) {
        await new Promise((r) => setTimeout(r, 500));
        try {
          const res = await fetchResult(run_id).unwrap();
          setResult(res.result);
          setRunning(false);
          return;
        } catch {
          /* still running */
        }
      }
      setError('Timed out waiting for the pipeline result.');
      setRunning(false);
    } catch (e) {
      setError(apiErrorMessage(e as never, 'Failed to start the pipeline'));
      setRunning(false);
    }
  }

  /** Build a promote draft client-side from a winner's params + a fingerprint — the
   *  same shape the sweep's `/promote` endpoint returns, so `PromoteRuleModal` opens
   *  the shared editor with the combo pre-filled. Requires a fingerprint: a rule can
   *  only arm on one, so an unscoped run must pick which. */
  function promote(params: Record<string, unknown>, label: string) {
    const fp: Fingerprint | undefined = config.fingerprintId
      ? fingerprints.find((f) => f.id === config.fingerprintId)
      : fingerprints[0];
    if (!fp) {
      setError('Promote needs a fingerprint — scope the run to one, or create a fingerprint first.');
      return;
    }
    setPromoteDraft({
      rule_name: `discovery · ${label}`,
      fingerprint_id: fp.id,
      trade_mode: 'paper',
      buy_amount_lamports: solToLamports(1) ?? 1_000_000_000,
      max_concurrent_tokens: 1,
      max_total_tokens: 0,
      params,
      fingerprint: fp,
    });
  }

  const pct = progress && progress.total > 0 ? (progress.processed / progress.total) * 100 : null;

  return (
    <div className="pt-2">
      <PageHeader
        title="Metric-combo discovery"
        description="Screen every metric → grid the survivors by family → validate out-of-sample"
      />

      <div className="mb-4 flex flex-wrap items-end gap-3">
        <Field label="Created range · UTC">
          <div className="flex items-center gap-1">
            <Input
              type="datetime-local"
              value={config.createdAfter}
              onChange={(e) => set('createdAfter', e.target.value)}
            />
            <span className="text-[10px] text-text-dim/50">–</span>
            <Input
              type="datetime-local"
              value={config.createdBefore}
              onChange={(e) => set('createdBefore', e.target.value)}
            />
          </div>
        </Field>
        <NumField label="Token cap" value={config.tokenCap} min={1} max={100000}
          onChange={(v) => set('tokenCap', v)} width="w-[110px]" />
        <NumField label="Take-profit %" value={config.takeProfitPct} min={0}
          onChange={(v) => set('takeProfitPct', v)} width="w-[110px]" />
        <NumField label="Stop-loss %" value={config.stopLossPct} min={0}
          onChange={(v) => set('stopLossPct', v)} width="w-[110px]" />
        <NumField label="Min closed" value={config.minClosed} min={1}
          onChange={(v) => set('minClosed', v)} width="w-[110px]" />
        <Field label="Train split">
          <Input
            type="number"
            step={0.05}
            min={0.1}
            max={0.95}
            value={config.splitFraction}
            onChange={(e) => set('splitFraction', Math.min(0.95, Math.max(0.1, Number(e.target.value) || 0.7)))}
            className="w-[90px]"
          />
        </Field>
        <label className="flex h-[34px] items-center gap-1.5 text-sm text-text-mid">
          <Checkbox checked={config.curveOnly} onChange={(e) => set('curveOnly', e.target.checked)} />
          <LabelTip tip={{ title: 'Curve only', body: 'Drop migrated-AMM legs — bonding-curve trades only.' }}>
            curve only
          </LabelTip>
        </label>
        <IconButton
          variant="primary"
          size="lg"
          onClick={handleRun}
          disabled={running || startState.isLoading}
          label={running ? 'Running…' : 'Run pipeline'}
          title={running ? 'Running…' : 'Run pipeline'}
        >
          {running ? <SpinnerIcon /> : <PlayIcon />}
        </IconButton>
      </div>

      <div className="mb-4 border-t border-white/10 pt-3">
        <FingerprintScopeControl
          fingerprints={fingerprints}
          value={config.fingerprintId}
          onChange={(id) => set('fingerprintId', id || null)}
          tip={{
            title: 'Scope by fingerprint',
            body: 'Score only tokens the engine matches against this fingerprint — keep the cohort single-regime (a combo tuned across archetypes averages to mush).',
          }}
          scopedDescription="The pipeline fits + validates on this fingerprint's token group, and a promoted winner arms on it."
          manualHint="Unscoped runs the whole cohort (noisier); a promoted winner then needs a fingerprint chosen at promote time."
          matchedCount={fpMatches.count}
          matchedCountLoading={fpMatches.countLoading}
          onViewMatches={fpMatches.openMatches}
        />
        {fpMatches.matchesModal}
      </div>

      {running && pct != null && (
        <div className="mb-4">
          <div className="mb-1 flex justify-between text-[11px] text-text-dim">
            <span>Running pipeline…</span>
            <span>{progress!.processed.toLocaleString()} / {progress!.total.toLocaleString()}</span>
          </div>
          <div className="h-1.5 w-full overflow-hidden rounded-full bg-white/6">
            <div className="h-full rounded-full bg-accent" style={{ width: `${pct}%` }} />
          </div>
        </div>
      )}

      {(error || startState.error) && (
        <InlineAlert variant="error">{error || apiErrorMessage(startState.error, 'Error')}</InlineAlert>
      )}

      {result && <Results result={result} onPromote={promote} />}

      <PromoteRuleModal draft={promoteDraft} onClose={() => setPromoteDraft(null)} />
    </div>
  );
}

// ── Results ──────────────────────────────────────────────────────────────────

function Results({
  result,
  onPromote,
}: {
  result: PipelineDto;
  onPromote: (params: Record<string, unknown>, label: string) => void;
}) {
  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-text-dim">
        <span><b className="text-text">{result.cohort_tokens.toLocaleString()}</b> cohort tokens</span>
        <span><b className="text-text">{result.fit_tokens.toLocaleString()}</b> fit (train)</span>
        <span>{result.screen.combos_scanned + result.family.combos_scanned} combos scanned</span>
        {result.screen.n_gated > 0 && (
          <Badge variant="neutral" size="sm">{result.screen.n_gated} min-N gated (Layer 1)</Badge>
        )}
      </div>

      {/* Layer 1 — shortlist */}
      <Section title="Layer 1 · metric shortlist" count={result.screen.shortlist.length}>
        {result.screen.shortlist.length === 0 ? (
          <Empty>No metric beat its own baseline on this cohort.</Empty>
        ) : (
          <table className="w-full text-xs">
            <thead>
              <tr>
                <Th>metric</Th><Th>op</Th><Th right>lift</Th><Th right>plateau</Th>
                <Th right>best</Th><Th>narrowed → Layer 2</Th>
              </tr>
            </thead>
            <tbody>
              {result.screen.shortlist.map((m, i) => (
                <ShortlistRow key={i} m={m} />
              ))}
            </tbody>
          </table>
        )}
      </Section>

      {/* Layer 2 — families + interactions */}
      <Section title="Layer 2 · family winners" count={result.family.families.length}>
        {result.family.families.length === 0 ? (
          <Empty>No family had a survivor to grid.</Empty>
        ) : (
          <div className="flex flex-col gap-3">
            {result.family.families.map((f, i) => (
              <FamilyCard key={i} f={f} onPromote={onPromote} />
            ))}
            {result.family.interactions.length > 0 && (
              <div className="rounded border border-white/8 p-2">
                <div className="mb-1 text-[10px] font-bold uppercase tracking-wider text-text-dim/80">
                  Interaction map (A pinned → B swept)
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {result.family.interactions.map((it, i) => (
                    <Badge key={i} variant={badgeVariant(it.verdict)} size="sm">
                      {it.pinned} → {it.swept}: {it.verdict}
                    </Badge>
                  ))}
                </div>
              </div>
            )}
          </div>
        )}
      </Section>

      {/* Layer 3 — validation */}
      <Section title="Layer 3 · out-of-sample validation">
        {result.validation ? (
          <>
            <div className="mb-2 text-[11px] text-text-dim">
              train {result.validation.train_tokens} · validate {result.validation.validate_tokens}
              {result.validation.boundary && ` · split @ ${new Date(result.validation.boundary).toISOString().slice(0, 16).replace('T', ' ')}`}
            </div>
            {result.validation.candidates.length === 0 ? (
              <Empty>No candidates to validate.</Empty>
            ) : (
              <table className="w-full text-xs">
                <thead>
                  <tr>
                    <Th>candidate</Th><Th>verdict</Th><Th right>train</Th><Th right>validate</Th>
                    <Th right>retention</Th><Th right>val n</Th><Th />
                  </tr>
                </thead>
                <tbody>
                  {result.validation.candidates.map((c, i) => (
                    <ValidationRow key={i} c={c} onPromote={onPromote} />
                  ))}
                </tbody>
              </table>
            )}
          </>
        ) : (
          <Empty>
            {result.no_validation === 'degenerate_split'
              ? 'Cohort too small to hold out a validation slice — Layers 1–2 ran on the whole cohort. Widen the range or token cap.'
              : 'No family winner to validate.'}
          </Empty>
        )}
      </Section>
    </div>
  );
}

function ShortlistRow({ m }: { m: MetricResponse }) {
  return (
    <tr className="border-t border-white/6">
      <Td>
        <span className="font-mono">{m.side}·{m.group}.{m.metric}</span>
        {m.window_sec != null && <span className="text-text-dim"> @{m.window_sec}s</span>}
      </Td>
      <Td>{m.operator}</Td>
      <Td right>{fmt(m.lift)}</Td>
      <Td right>{fmt(m.plateau)}</Td>
      <Td right>{fmt(m.best_value)}</Td>
      <Td><span className="font-mono text-text-dim">{m.narrowed.map((v) => fmt(v, 1)).join(', ')}</span></Td>
    </tr>
  );
}

function FamilyCard({
  f,
  onPromote,
}: {
  f: FamilyResult;
  onPromote: (params: Record<string, unknown>, label: string) => void;
}) {
  return (
    <div className="rounded border border-white/8 p-3">
      <div className="mb-1.5 flex flex-wrap items-center gap-2">
        <Badge variant="accent" size="sm">{f.family}</Badge>
        <span className="text-[11px] text-text-dim">{f.combos} combos · {f.members.length} members</span>
        {f.n_gated > 0 && <span className="text-[10px] text-text-dim">{f.n_gated} gated</span>}
        {f.dropped.map((d, i) => (
          <span key={i} className="text-[10px] text-warning" title={d.reason}>
            dropped {d.metric} ({d.reason})
          </span>
        ))}
      </div>
      <div className="mb-2 flex flex-wrap gap-1.5">
        {f.members.map((m, i) => (
          <span key={i} className="rounded bg-white/5 px-1.5 py-0.5 font-mono text-[10px] text-text-mid">
            {m.metric} {m.operator} {m.values.map((v) => fmt(v, 1)).join('/')}
          </span>
        ))}
      </div>
      {f.best ? (
        <div className="flex flex-wrap items-center gap-3 text-[11px] text-text-dim">
          <span>score <b className="text-text">{fmt(f.best.score, 3)}</b></span>
          <span>fired {f.best.n_fired} · closed {f.best.n_closed}</span>
          <button
            type="button"
            className="rounded border border-accent/40 px-2 py-0.5 text-[11px] font-semibold text-accent transition hover:bg-accent/10"
            onClick={() => onPromote(f.best!.params, f.family)}
          >
            Promote…
          </button>
        </div>
      ) : (
        <span className="text-[11px] text-text-dim">No rankable combo (every one min-N gated).</span>
      )}
    </div>
  );
}

function ValidationRow({
  c,
  onPromote,
}: {
  c: CandidateValidation;
  onPromote: (params: Record<string, unknown>, label: string) => void;
}) {
  return (
    <tr className="border-t border-white/6">
      <Td><span className="font-mono">{c.label}</span></Td>
      <Td><Badge variant={badgeVariant(c.verdict)} size="sm">{c.verdict}</Badge></Td>
      <Td right>{fmt(c.train.score, 3)}</Td>
      <Td right>{fmt(c.validate.score, 3)}</Td>
      <Td right>{c.retention == null ? '—' : `${(c.retention * 100).toFixed(0)}%`}</Td>
      <Td right>{c.validate.n_closed}</Td>
      <Td right>
        <button
          type="button"
          className="rounded border border-accent/40 px-2 py-0.5 text-[11px] font-semibold text-accent transition hover:bg-accent/10"
          onClick={() => onPromote(c.params, c.label)}
        >
          Promote…
        </button>
      </Td>
    </tr>
  );
}

// ── small presentational helpers ─────────────────────────────────────────────

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80">{label}</span>
      {children}
    </div>
  );
}

function NumField({
  label, value, min, max, onChange, width,
}: {
  label: string; value: number; min?: number; max?: number;
  onChange: (v: number) => void; width: string;
}) {
  return (
    <div className={`flex flex-col gap-1 ${width}`}>
      <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80">{label}</span>
      <Input
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(e) => {
          let n = Number(e.target.value);
          if (!Number.isFinite(n)) n = min ?? 0;
          if (min != null) n = Math.max(min, n);
          if (max != null) n = Math.min(max, n);
          onChange(n);
        }}
      />
    </div>
  );
}

function Section({ title, count, children }: { title: string; count?: number; children: React.ReactNode }) {
  return (
    <div>
      <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-text-mid">
        {title}
        {count != null && <span className="ml-1.5 text-text-dim">({count})</span>}
      </h2>
      {children}
    </div>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return <p className="text-xs text-text-dim">{children}</p>;
}

function Th({ children, right }: { children?: React.ReactNode; right?: boolean }) {
  return (
    <th className={`pb-1 text-[10px] font-bold uppercase tracking-wider text-text-dim/70 ${right ? 'text-right' : 'text-left'}`}>
      {children}
    </th>
  );
}

function Td({ children, right }: { children?: React.ReactNode; right?: boolean }) {
  return <td className={`py-1 ${right ? 'text-right' : 'text-left'}`}>{children}</td>;
}
