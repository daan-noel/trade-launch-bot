import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { Badge } from 'components/ui/Badge';
import { Checkbox } from 'components/ui/Checkbox';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import { IconButton } from 'components/ui/IconButton';
import { PlayIcon, SpinnerIcon } from 'components/ui/icons';
import { Input } from 'components/ui/Input';
import { InlineAlert } from 'components/ui/Modal';
import { LabelTip } from 'components/strategy/LabelTip';
import { FingerprintScopeControl } from 'components/strategy/FingerprintScopeControl';
import { VolumeIxPatternsEditor } from 'components/strategy/VolumeIxPatternsEditor';
import { useFingerprintMatches } from '@lab/components/strategy/useFingerprintMatches';
import { PageHeader } from 'components/ui/PageHeader';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';
import { apiErrorMessage } from 'store/baseApi';
import { sseSubscribe } from 'services/sse';
import { useGetFingerprintsQuery } from 'store/sharedEndpoints';
import { solToLamports, type Fingerprint } from 'lib/strategy/types';
import type { PromotedRuleDraft } from 'lib/strategy/types';
import { PromoteRuleModal } from '@lab/components/sweep/PromoteRuleModal';
import { parseWindowSpec } from 'lib/strategy/windowSpec';
import {
  useGetLastMetricDiscoveryQuery,
  useLazyGetMetricDiscoveryQuery,
  useStartMetricDiscoveryMutation,
} from '@lab/store/labEndpoints';
import type {
  BaselineSelection,
  Bracket,
  CandidateValidation,
  DiscoverySweepHandoff,
  FamilyResult,
  JointResult,
  MetricResponse,
  PipelineDto,
  Rescue,
  ScoredRow,
} from '@lab/lib/metricDiscoveryTypes';

interface Config {
  createdAfter: string;
  createdBefore: string;
  tokenCap: number;
  curveOnly: boolean;
  fingerprintId: string | null;
  takeProfitPct: number;
  stopLossPct: number;
  /** Comma-separated TP rungs for baseline selection; blank = screen at takeProfitPct. */
  takeProfitMenu: string;
  stopLossMenu: string;
  minClosed: number;
  splitFraction: number;
  buyAmountSol: number;
  /** Entry / exit screening spans, as text in the `formatWindowSpec` grammar:
   *  `30` (seconds), `30sl@1`, `20p`. Text rather than a number so a basis can be
   *  typed at all, and validated before the run is admitted. */
  entryWindow: string;
  exitWindow: string;
  volumeIxPatterns: string[][];
  ixLabelsFilter: string;
}

const DEFAULTS: Config = {
  createdAfter: '',
  createdBefore: '',
  tokenCap: 5000,
  curveOnly: false,
  fingerprintId: null,
  takeProfitPct: 30,
  stopLossPct: 15,
  takeProfitMenu: '20, 30, 60, 100',
  stopLossMenu: '10, 15, 25, 40',
  minClosed: 20,
  splitFraction: 0.7,
  buyAmountSol: 1.0,
  entryWindow: '30',
  exitWindow: '10',
  volumeIxPatterns: [],
  ixLabelsFilter: '',
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
  // Real signal on a bracket that doesn't fit — a lead, not a dead end.
  drop_negative: 'warning',
  drop_spike: 'danger',
  drop_thin: 'neutral',
  drop_no_baseline: 'neutral',
  failed: 'danger',
  thin_validate: 'neutral',
  no_fire_validate: 'neutral',
  unrankable_train: 'neutral',
};

/** What a verdict tag actually means, in one line. The tags are terse by design. */
const verdictHelp: Record<string, string> = {
  keep: 'Beat its own off pick, and the neighbouring value held the lift.',
  drop_no_edge: 'Flat — the best pick never meaningfully beat "off".',
  drop_negative: 'Lifted the baseline but stayed unprofitable: real signal, wrong bracket.',
  drop_spike: 'One value spiked and its neighbours collapsed — the overfit signature.',
  drop_thin: 'Too few closed trades to judge. Not evidence of no edge — widen the cohort.',
  drop_no_baseline: 'The bare bracket itself was unrankable, so there was nothing to measure against.',
  thin_validate: 'The held-out slice had too few trades to conclude — neither pass nor fail.',
  no_fire_validate: 'Never fired out-of-sample — neither pass nor fail.',
  unrankable_train: 'No positive in-sample score to hold anything against.',
};

function badgeVariant(tag: string): Variant {
  return verdictBadge[tag] ?? 'neutral';
}

/**
 * Drop verdicts most-actionable first. `drop_negative` is a lead (real signal, wrong
 * bracket); `drop_thin` and `drop_no_baseline` are statements about the cohort's size,
 * not about the metric — burying them under the leads is the point.
 */
const DROP_ORDER = [
  'drop_negative',
  'drop_spike',
  'drop_no_edge',
  'drop_thin',
  'drop_no_baseline',
] as const;

function bracketLabel(b: Bracket | null | undefined): string {
  if (!b) return '—';
  const tp = b.take_profit_pct == null ? null : `TP ${b.take_profit_pct}%`;
  const sl = b.stop_loss_pct == null ? null : `SL ${b.stop_loss_pct}%`;
  return [tp, sl].filter(Boolean).join(' / ') || 'no bracket';
}

/**
 * A comma-separated bracket menu. `off` / `none` / `-` express "omit this guard", so a
 * no-stop bracket is reachable from the form. Blank ⇒ no menu (screen at the single
 * TP/SL below).
 */
function parseBracketMenu(text: string): (number | null)[] | undefined {
  const parts = text
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);
  if (!parts.length) return undefined;
  const out: (number | null)[] = [];
  for (const p of parts) {
    if (/^(off|none|-)$/i.test(p)) {
      out.push(null);
      continue;
    }
    const n = Number(p);
    if (Number.isFinite(n) && n > 0) out.push(n);
  }
  return out.length ? out : undefined;
}

function parseIxLabels(text: string): string[] | undefined {
  const labels = text
    .split(/[,\n]/)
    .map((s) => s.trim())
    .filter(Boolean);
  return labels.length ? labels : undefined;
}

/**
 * Lab page: run the metric-combo discovery pipeline (screen → family-grid →
 * joint → validate) and seed a grouped sweep from the Keep shortlist + TP/SL
 * menus. Promote on OOS survivors remains a secondary exit.
 *
 * Architecture: `hunter/docs/arch/sweep.md` "Metric-combo discovery pipeline".
 */
export function MetricDiscoveryPage() {
  const navigate = useNavigate();
  const [stored, setConfig] = useLocalStorage<Config>(STORAGE_KEYS.metricDiscoveryConfig, DEFAULTS);
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
  const [includeOptional, setIncludeOptional] = useState(false);

  useEffect(() => {
    if (lastResult && !result) setResult(lastResult.result);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once, when the cache arrives
  }, [lastResult]);

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
        ix_labels_filter: config.fingerprintId ? undefined : parseIxLabels(config.ixLabelsFilter),
        buy_amount_sol: config.buyAmountSol,
        take_profit_pct: config.takeProfitPct,
        stop_loss_pct: config.stopLossPct,
        take_profit_menu: parseBracketMenu(config.takeProfitMenu),
        stop_loss_menu: parseBracketMenu(config.stopLossMenu),
        min_closed: config.minClosed,
        split_fraction: config.splitFraction,
        entry_window_sec: config.entryWindow,
        exit_window_sec: config.exitWindow,
        volume_ix_patterns: config.volumeIxPatterns.length ? config.volumeIxPatterns : undefined,
      }).unwrap();

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
      buy_amount_lamports: solToLamports(config.buyAmountSol) ?? 1_000_000_000,
      max_concurrent_tokens: 1,
      max_total_tokens: 0,
      params,
      fingerprint: fp,
    });
  }

  function openAsSweep() {
    if (!result?.sweep_seed) {
      setError('No sweep seed on this result — re-run the pipeline.');
      return;
    }
    const handoff: DiscoverySweepHandoff = {
      seed: result.sweep_seed,
      includeOptional,
      createdAfter: config.createdAfter,
      createdBefore: config.createdBefore,
      curveOnly: config.curveOnly,
      tokenCap: config.tokenCap,
      fingerprintId: config.fingerprintId,
      buyAmountSol: config.buyAmountSol,
      volumeIxPatterns: config.volumeIxPatterns,
      ixLabelsFilter: config.ixLabelsFilter,
    };
    try {
      sessionStorage.setItem(STORAGE_KEYS.sweepDiscoverySeed, JSON.stringify(handoff));
    } catch {
      setError('Could not write the sweep seed to sessionStorage.');
      return;
    }
    navigate('/strategies/sweep');
  }

  const pct = progress && progress.total > 0 ? (progress.processed / progress.total) * 100 : null;
  const seedReady = !!result?.sweep_seed?.axes?.length;

  return (
    <div className="pt-2">
      <PageHeader
        title="Metric-combo discovery"
        description="Screen metrics → grid by family → joint interacting clusters → seed a grouped sweep"
      />

      <div className="mb-4 flex flex-wrap items-end gap-3">
        <Field label="Created range · UTC">
          <DateTimeRangePicker
            aria-label="Created range"
            zoneLabel="UTC"
            emptyLabel="All history"
            customPreset="custom"
            value={{
              preset: 'custom',
              from: config.createdAfter,
              to: config.createdBefore,
            }}
            onChange={({ from, to }) => {
              set('createdAfter', from);
              set('createdBefore', to);
            }}
          />
        </Field>
        <NumField label="Token cap" value={config.tokenCap} min={1} max={100000}
          onChange={(v) => set('tokenCap', v)} width="w-[110px]" />
        <NumField label="Buy SOL" value={config.buyAmountSol} min={0.01}
          onChange={(v) => set('buyAmountSol', v)} width="w-[90px]" step={0.1} />
        <NumField label="Take-profit %" value={config.takeProfitPct} min={0}
          onChange={(v) => set('takeProfitPct', v)} width="w-[110px]" />
        <NumField label="Stop-loss %" value={config.stopLossPct} min={0}
          onChange={(v) => set('stopLossPct', v)} width="w-[110px]" />
        <NumField label="Min closed" value={config.minClosed} min={1}
          onChange={(v) => set('minClosed', v)} width="w-[110px]" />
        <div className="flex flex-col gap-1">
          <LabelTip
            tip={{
              title: 'Bracket menus (Layer 0)',
              body: 'Every metric is ranked by what it adds over a fixed TP/SL, so the wrong bracket makes the whole cohort read as "no edge". Listing rungs here measures each bracket bare first and screens against the one that fits. Blank = use the single TP/SL. Use "off" for no guard on that side.',
            }}
          >
            <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
              TP / SL menus (comma-separated)
            </span>
          </LabelTip>
          <div className="flex gap-1.5">
            <Input
              value={config.takeProfitMenu}
              onChange={(e) => set('takeProfitMenu', e.target.value)}
              placeholder="20, 30, 60"
              className="w-[150px]"
              aria-label="Take-profit menu"
            />
            <Input
              value={config.stopLossMenu}
              onChange={(e) => set('stopLossMenu', e.target.value)}
              placeholder="10, 15, 25"
              className="w-[150px]"
              aria-label="Stop-loss menu"
            />
          </div>
        </div>
        <TextField label="Entry win" value={config.entryWindow}
          onChange={(v) => set('entryWindow', v)} width="w-[100px]"
          invalid={parseWindowSpec(config.entryWindow) == null}
          title="Seconds, or a span: 30sl@1 (slots), 20p (prints)" />
        <TextField label="Exit win" value={config.exitWindow}
          onChange={(v) => set('exitWindow', v)} width="w-[100px]"
          invalid={parseWindowSpec(config.exitWindow) == null}
          title="Seconds, or a span: 30sl@1 (slots), 20p (prints)" />
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
          scopedDescription="The pipeline fits + validates on this fingerprint's token group, and a seeded sweep / promoted winner scopes to it."
          manualHint="Unscoped runs the whole cohort (noisier); optional ix_labels below still apply."
          matchedCount={fpMatches.count}
          matchedCountLoading={fpMatches.countLoading}
          onViewMatches={fpMatches.openMatches}
          onRequestMatchCount={fpMatches.ensureCount}
        />
        {fpMatches.matchesModal}
        {!config.fingerprintId && (
          <div className="mt-2">
            <Field label="ix_labels filter (comma-separated, unscoped only)">
              <Input
                value={config.ixLabelsFilter}
                onChange={(e) => set('ixLabelsFilter', e.target.value)}
                placeholder="e.g. buy,sell"
                className="max-w-md"
              />
            </Field>
          </div>
        )}
        <div className="mt-3">
          <LabelTip
            tip={{
              title: 'Volume ix patterns',
              body: 'Required to screen flow-split metrics. Same shape as the sweep / flow-discovery bind.',
            }}
          >
            <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
              Volume ix patterns
            </span>
          </LabelTip>
          <div className="mt-1">
            <VolumeIxPatternsEditor
              patterns={config.volumeIxPatterns}
              onChange={(v) => set('volumeIxPatterns', v)}
            />
          </div>
        </div>
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

      {result && (
        <Results
          result={result}
          onPromote={promote}
          onOpenAsSweep={openAsSweep}
          seedReady={seedReady}
          includeOptional={includeOptional}
          onIncludeOptional={setIncludeOptional}
        />
      )}

      <PromoteRuleModal draft={promoteDraft} onClose={() => setPromoteDraft(null)} />
    </div>
  );
}

// ── Results ──────────────────────────────────────────────────────────────────

function Results({
  result,
  onPromote,
  onOpenAsSweep,
  seedReady,
  includeOptional,
  onIncludeOptional,
}: {
  result: PipelineDto;
  onPromote: (params: Record<string, unknown>, label: string) => void;
  onOpenAsSweep: () => void;
  seedReady: boolean;
  includeOptional: boolean;
  onIncludeOptional: (v: boolean) => void;
}) {
  const dropped = result.screen.responses.filter((r) => r.verdict !== 'keep');
  const droppedBy = dropped.reduce<Record<string, MetricResponse[]>>((acc, m) => {
    (acc[m.verdict] ??= []).push(m);
    return acc;
  }, {});
  const rescues = result.family.rescues ?? [];
  const seed = result.sweep_seed;

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-xs text-text-dim">
        <span><b className="text-text">{result.cohort_tokens.toLocaleString()}</b> cohort tokens</span>
        <span><b className="text-text">{result.fit_tokens.toLocaleString()}</b> fit (train)</span>
        <span>
          {result.screen.combos_scanned + result.family.combos_scanned} combos scanned
        </span>
        {result.cohort_capped && (
          <Badge variant="warning" size="sm">
            truncated to the newest {result.token_cap?.toLocaleString() ?? '?'}
          </Badge>
        )}
        {result.screen.n_gated > 0 && (
          <Badge variant="neutral" size="sm">
            {result.screen.n_gated} gated at {result.screen.effective_min_closed} closed
          </Badge>
        )}
        {result.screen.effective_min_closed < result.screen.min_closed && (
          <Badge variant="warning" size="sm">
            gate relaxed {result.screen.min_closed} → {result.screen.effective_min_closed}
          </Badge>
        )}
        {seed && (
          <Badge variant="info" size="sm">
            seed ~{seed.combo_estimate.toLocaleString()} combos · {seed.axes.length} axes
          </Badge>
        )}
      </div>

      {/* How to read this run — the questions the layer tables can't answer alone. */}
      {(result.diagnostics?.length ?? 0) > 0 && (
        <div className="rounded border border-white/10 bg-white/2 p-3">
          <div className="mb-1.5 text-[10px] font-bold uppercase tracking-wider text-text-dim/80">
            How to read this run
          </div>
          <ul className="list-inside list-disc space-y-1 text-[11px] text-text-mid">
            {result.diagnostics!.map((d, i) => (
              <li key={i}>{d}</li>
            ))}
          </ul>
        </div>
      )}

      {/* The reference line — what every lift below is measured against. */}
      <ReferenceLine
        bracket={result.screen.baseline}
        stats={result.screen.baseline_stats}
        selection={result.baseline_selection}
      />

      {/* Primary CTA — seed sweep */}
      <div className="flex flex-wrap items-center gap-3 rounded border border-accent/30 bg-accent/5 px-3 py-2">
        <button
          type="button"
          disabled={!seedReady}
          className="rounded bg-accent px-3 py-1.5 text-xs font-semibold text-white transition hover:bg-accent/90 disabled:opacity-40"
          onClick={onOpenAsSweep}
        >
          Open as sweep
        </button>
        {seed && seed.optional_axes.length > 0 && (
          <label
            className="flex items-center gap-1.5 text-[11px] text-text-mid"
            title={
              'Metrics that missed the Keep bar but still scored positive somewhere: flat-but-positive, ' +
              'lifted-a-losing-baseline, and single spikes. The sweep re-prices them across the whole ' +
              'TP/SL ladder. A spike is unstable — keep it only if the sweep reproduces it. See Seed notes ' +
              'for the split.'
            }
          >
            <Checkbox
              checked={includeOptional}
              onChange={(e) => onIncludeOptional(e.target.checked)}
            />
            include {seed.optional_axes.length} near-miss axis(es)
          </label>
        )}
        <span className="text-[11px] text-text-dim">
          Loads Keep metrics + TP/SL menus into the grouped-sweep form. Promote stays secondary.
        </span>
      </div>

      {seed && seed.notes.length > 0 && (
        <div className="rounded border border-white/8 p-2 text-[11px] text-text-dim">
          <div className="mb-1 text-[10px] font-bold uppercase tracking-wider text-text-dim/80">
            Seed notes
          </div>
          <ul className="list-inside list-disc space-y-0.5">
            {seed.notes.map((n, i) => (
              <li key={i}>{n}</li>
            ))}
          </ul>
          {seed.clusters.length > 0 && (
            <div className="mt-2 flex flex-wrap gap-1.5">
              {seed.clusters.map((c, i) => (
                <Badge key={i} variant={c.interacting ? 'warning' : 'info'} size="sm">
                  {c.interacting ? 'joint' : 'indep'} [{c.families.join('+')}]
                </Badge>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Layer 1 — shortlist */}
      <Section title="Layer 1 · metric shortlist" count={result.screen.shortlist.length}>
        {result.screen.shortlist.length === 0 ? (
          <Empty>
            No metric beat the reference line above by the lift threshold. Check whether that line
            was profitable before reading this as "the cohort has no structure".
          </Empty>
        ) : (
          <table className="w-full text-xs">
            <thead>
              <tr>
                <Th>metric</Th><Th>op</Th><Th right>lift</Th><Th right>plateau</Th>
                <Th right>best</Th><Th>at best value</Th><Th>narrowed → Layer 2</Th><Th>curve</Th>
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

      {dropped.length > 0 && (
        <Section title="Layer 1 · dropped / skipped" count={dropped.length}>
          {/* Why the field died, before the field itself — the tally is the finding. */}
          <div className="mb-2 flex flex-wrap gap-1.5">
            {DROP_ORDER.filter((v) => droppedBy[v]?.length).map((v) => (
              <Badge key={v} variant={badgeVariant(v)} size="sm" title={verdictHelp[v] ?? ''}>
                {droppedBy[v]!.length} {v}
              </Badge>
            ))}
          </div>
          <table className="w-full text-xs">
            <thead>
              <tr>
                {/* `op` is not decoration: without it a curve point reads as
                    "metric at 6" with no way to tell `>= 6` from `< 6`, which
                    inverts the conclusion. */}
                <Th>metric</Th><Th>op</Th><Th>verdict</Th><Th right>lift</Th><Th right>best score</Th>
                <Th right>closed/gate</Th><Th>curve</Th>
              </tr>
            </thead>
            <tbody>
              {/* Leads first: a metric that lifted a losing bracket is actionable, a
                  metric with no data is not. */}
              {DROP_ORDER.flatMap((v) => droppedBy[v] ?? []).map((m, i) => (
                <DroppedRow key={i} m={m} />
              ))}
            </tbody>
          </table>
          {(result.screen.skipped.length > 0 || result.screen.gaps.length > 0) && (
            <div className="mt-2 flex flex-wrap gap-1.5 text-[10px] text-text-dim">
              {result.screen.skipped.map((s, i) => (
                <span key={`s${i}`} className="rounded bg-white/5 px-1.5 py-0.5">
                  skipped {s.group}.{s.metric}: {s.reason}
                </span>
              ))}
              {result.screen.gaps.map((g, i) => (
                <span key={`g${i}`} className="rounded bg-white/5 px-1.5 py-0.5">
                  gap {g.group}.{g.metric}: {g.reason}
                </span>
              ))}
            </div>
          )}
        </Section>
      )}

      {/* Layer 2 — families + interactions + joints */}
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

      {rescues.length > 0 && (
        <Section title="Layer 2 · synergy rescue" count={rescues.filter((r) => r.verdict === 'keep').length}>
          <p className="mb-2 text-[11px] text-text-dim">
            Layer 1 can only see a metric&apos;s standalone lift, so the second half of a pair that
            only works together never reaches a grid. These are the rejects re-screened with the
            strongest winner pinned — a kept one earns its axis <b>given that pin</b>, never alone.
          </p>
          <table className="w-full text-xs">
            <thead>
              <tr>
                <Th>metric</Th><Th>op</Th><Th>pinned</Th><Th right>pinned alone</Th>
                <Th right>lift given pin</Th><Th>verdict</Th>
              </tr>
            </thead>
            <tbody>
              {rescues.map((r, i) => (
                <RescueRow key={i} r={r} />
              ))}
            </tbody>
          </table>
        </Section>
      )}

      {(result.family.joints?.length ?? 0) > 0 && (
        <Section title="Layer 2b · joint grids" count={result.family.joints!.length}>
          <div className="flex flex-col gap-3">
            {result.family.joints!.map((j, i) => (
              <JointCard key={i} j={j} onPromote={onPromote} />
            ))}
          </div>
        </Section>
      )}

      {/* Layer 3 — validation */}
      <Section title="Layer 3 · out-of-sample validation">
        {result.validation ? (
          <>
            <div className="mb-2 text-[11px] text-text-dim">
              train {result.validation.train_tokens} · validate {result.validation.validate_tokens}
              {result.validation.effective_min_closed != null &&
                ` · needs ${result.validation.effective_min_closed} closed there for any verdict`}
              {result.validation.boundary && ` · split @ ${new Date(result.validation.boundary).toISOString().slice(0, 16).replace('T', ' ')}`}
            </div>
            {result.validation.candidates.length === 0 ? (
              <Empty>No candidates to validate.</Empty>
            ) : (
              <table className="w-full text-xs">
                <thead>
                  <tr>
                    <Th>candidate</Th><Th>verdict</Th><Th right>train</Th><Th right>validate</Th>
                    <Th right>retention</Th><Th right>val pnl ◎</Th><Th right>val n</Th><Th />
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

/**
 * The bare bracket's own result, and (when Layer 0 ran) the table it was chosen from.
 *
 * This is the single most load-bearing number on the page: every `lift` is a delta
 * against it, so a shortlist read without it can't distinguish "this metric makes money"
 * from "this metric loses less money than doing nothing".
 */
function ReferenceLine({
  bracket,
  stats,
  selection,
}: {
  bracket: Bracket;
  stats: ScoredRow | null;
  selection?: BaselineSelection | null;
}) {
  const [open, setOpen] = useState(false);
  const losing = stats != null && (stats.score == null || stats.score <= 0);
  return (
    <div className={`rounded border p-3 ${losing ? 'border-warning/40 bg-warning/5' : 'border-white/10'}`}>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5">
        <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim/80">
          Reference line
        </span>
        <Badge variant={losing ? 'warning' : 'info'} size="sm">{bracketLabel(bracket)}</Badge>
        <span className="text-[11px] text-text-dim">no metric gate</span>
        {stats ? (
          <>
            <Stat label="fired">{stats.n_fired.toLocaleString()}</Stat>
            <Stat label="closed">{stats.n_closed.toLocaleString()}</Stat>
            <Stat label="win">{(stats.win_rate * 100).toFixed(0)}%</Stat>
            <Stat label="median">{fmt(stats.median_pnl_pct, 1)}%</Stat>
            <Stat label="pnl">{fmt(stats.total_pnl_sol, 3)} ◎</Stat>
            <Stat label="score">{fmt(stats.score, 3)}</Stat>
          </>
        ) : (
          <span className="text-[11px] text-text-dim">nothing screened</span>
        )}
        {selection && (
          <button
            type="button"
            className="ml-auto rounded border border-white/20 px-2 py-0.5 text-[11px] text-text-mid transition hover:bg-white/5"
            onClick={() => setOpen((v) => !v)}
          >
            {open ? 'Hide' : 'Show'} {selection.candidates.length} measured bracket(s)
          </button>
        )}
      </div>
      {losing && (
        <p className="mt-2 text-[11px] text-warning">
          This bracket loses money bare, so a metric can only be ranked on how much of that loss it
          removes — every Keep below is a rescue, not an improvement.
        </p>
      )}
      {selection && open && (
        <table className="mt-3 w-full text-xs">
          <thead>
            <tr>
              <Th>bracket</Th><Th right>score</Th><Th right>fired</Th><Th right>closed</Th>
              <Th right>win</Th><Th right>median</Th><Th right>pnl ◎</Th>
            </tr>
          </thead>
          <tbody>
            {selection.candidates.map((c, i) => (
              <tr key={i} className={`border-t border-white/6 ${i === selection.chosen_index ? 'text-text' : 'text-text-dim'}`}>
                <Td>
                  {bracketLabel(c.bracket)}
                  {i === selection.chosen_index && (
                    <Badge variant="success" size="sm" className="ml-1.5">chosen</Badge>
                  )}
                </Td>
                <Td right>{fmt(c.score, 3)}</Td>
                <Td right>{c.n_fired}</Td>
                <Td right>{c.n_closed}</Td>
                <Td right>{(c.win_rate * 100).toFixed(0)}%</Td>
                <Td right>{fmt(c.median_pnl_pct, 1)}%</Td>
                <Td right>{fmt(c.total_pnl_sol, 3)}</Td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function Stat({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <span className="text-[11px] text-text-dim">
      {label} <b className="text-text">{children}</b>
    </span>
  );
}

/**
 * Layer 1's best pick in money. The score is a unitless ranking key, so this is the
 * only part of a row that can be sanity-checked against a real trade.
 */
function BestPickMoney({ m }: { m: MetricResponse }) {
  const best = m.curve.find((p) => p.value != null && p.value === m.best_value) ?? null;
  if (!best) return <span className="text-text-dim">—</span>;
  return (
    <span className="whitespace-nowrap text-[11px] text-text-dim">
      <b className="text-text">{fmt(best.total_pnl_sol, 3)} ◎</b>
      {' · '}{fmt(best.median_pnl_pct, 1)}% med
      {' · '}{(best.win_rate * 100).toFixed(0)}% win
      {' · '}{best.n_closed}/{best.n_fired}
    </span>
  );
}

function CurveSpark({ curve }: { curve: MetricResponse['curve'] }) {
  if (!curve.length) return <span className="text-text-dim">—</span>;
  // The hover carries the money and the sample; the inline form stays a shape.
  const title = curve
    .map((p) => {
      const at = p.value == null ? 'off' : fmt(p.value, 1);
      const score = p.score == null ? `${p.outcome}${p.gate != null ? ` (<${p.gate})` : ''}` : fmt(p.score, 2);
      return `${at}: ${score} · ${fmt(p.total_pnl_sol, 3)}◎ · ${fmt(p.median_pnl_pct, 1)}% med · ${p.n_closed}/${p.n_fired}`;
    })
    .join('\n');
  return (
    <span className="font-mono text-[10px] text-text-dim" title={title}>
      {curve.map((p, i) => (
        <span key={i} className={p.score != null && p.score > 0 ? 'text-text-mid' : ''}>
          {i > 0 ? ' · ' : ''}
          {p.value == null ? 'off' : fmt(p.value, 1)}
          <span className="opacity-60">={fmt(p.score, 2)}</span>
        </span>
      ))}
    </span>
  );
}

function ShortlistRow({ m }: { m: MetricResponse }) {
  return (
    <tr className="border-t border-white/6">
      <Td>
        <span className="font-mono">{m.side}·{m.group}.{m.metric}</span>
        {windowLabel(m) && <span className="text-text-dim"> @{windowLabel(m)}</span>}
      </Td>
      <Td>{m.operator}</Td>
      <Td right>{fmt(m.lift)}</Td>
      <Td right>{fmt(m.plateau)}</Td>
      <Td right>{fmt(m.best_value)}</Td>
      <Td><BestPickMoney m={m} /></Td>
      <Td><span className="font-mono text-text-dim">{m.narrowed.map((v) => fmt(v, 1)).join(', ')}</span></Td>
      <Td><CurveSpark curve={m.curve} /></Td>
    </tr>
  );
}

function DroppedRow({ m }: { m: MetricResponse }) {
  // `drop_thin` failed for want of data, so the gate it missed is the whole story.
  const gate = m.curve.find((p) => p.gate != null)?.gate ?? null;
  const bestClosed = m.curve
    .filter((p) => p.value != null)
    .reduce((n, p) => Math.max(n, p.n_closed), 0);
  return (
    <tr className="border-t border-white/6">
      <Td>
        <span className="font-mono">{m.side}·{m.group}.{m.metric}</span>
        {windowLabel(m) && <span className="text-text-dim"> @{windowLabel(m)}</span>}
      </Td>
      <Td>{m.operator}</Td>
      <Td>
        <Badge variant={badgeVariant(m.verdict)} size="sm" title={verdictHelp[m.verdict] ?? ''}>
          {m.verdict}
        </Badge>
      </Td>
      <Td right>{fmt(m.lift)}</Td>
      <Td right>{fmt(m.best_score, 3)}</Td>
      <Td right className="text-text-dim">
        {m.verdict === 'drop_thin' && gate != null ? `${bestClosed}/${gate}` : '—'}
      </Td>
      <Td><CurveSpark curve={m.curve} /></Td>
    </tr>
  );
}

function RescueRow({ r }: { r: Rescue }) {
  const kept = r.verdict === 'keep';
  return (
    <tr className={`border-t border-white/6 ${kept ? '' : 'text-text-dim'}`}>
      <Td><span className="font-mono">{r.side}·{r.group}.{r.metric}</span></Td>
      <Td>{r.operator}</Td>
      <Td><Badge variant="accent" size="sm">{r.pinned}</Badge></Td>
      <Td right>{fmt(r.pinned_score, 3)}</Td>
      <Td right>{fmt(r.lift, 3)}</Td>
      <Td>
        <Badge variant={badgeVariant(r.verdict)} size="sm" title={verdictHelp[r.verdict] ?? ''}>
          {kept ? 'rescued' : r.verdict}
        </Badge>
      </Td>
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
          <span
            key={i}
            className={`rounded px-1.5 py-0.5 font-mono text-[10px] ${
              m.rescued ? 'bg-warning/12 text-warning' : 'bg-white/5 text-text-mid'
            }`}
            title={m.rescued ? 'Rescued: lift is conditional on the pinned winner' : undefined}
          >
            {m.metric} {m.operator} {m.values.map((v) => fmt(v, 1)).join('/')}
            {m.rescued && ' ·rescued'}
          </span>
        ))}
      </div>
      {f.best ? (
        <div className="flex flex-wrap items-center gap-3 text-[11px] text-text-dim">
          <span>score <b className="text-text">{fmt(f.best.score, 3)}</b></span>
          <span>fired {f.best.n_fired} · closed {f.best.n_closed}</span>
          <button
            type="button"
            className="rounded border border-white/20 px-2 py-0.5 text-[11px] text-text-mid transition hover:bg-white/5"
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

function JointCard({
  j,
  onPromote,
}: {
  j: JointResult;
  onPromote: (params: Record<string, unknown>, label: string) => void;
}) {
  const label = j.families.join('+');
  return (
    <div className="rounded border border-warning/30 p-3">
      <div className="mb-1.5 flex flex-wrap items-center gap-2">
        <Badge variant="warning" size="sm">joint · {label}</Badge>
        <span className="text-[11px] text-text-dim">{j.combos} combos · {j.members.length} members</span>
        {j.dropped.map((d, i) => (
          <span key={i} className="text-[10px] text-warning">
            dropped {d.metric} ({d.reason})
          </span>
        ))}
      </div>
      <div className="mb-2 flex flex-wrap gap-1.5">
        {j.members.map((m, i) => (
          <span
            key={i}
            className={`rounded px-1.5 py-0.5 font-mono text-[10px] ${
              m.rescued ? 'bg-warning/12 text-warning' : 'bg-white/5 text-text-mid'
            }`}
            title={m.rescued ? 'Rescued: lift is conditional on the pinned winner' : undefined}
          >
            {m.metric} {m.operator} {m.values.map((v) => fmt(v, 1)).join('/')}
            {m.rescued && ' ·rescued'}
          </span>
        ))}
      </div>
      {j.best ? (
        <div className="flex flex-wrap items-center gap-3 text-[11px] text-text-dim">
          <span>score <b className="text-text">{fmt(j.best.score, 3)}</b></span>
          <span>fired {j.best.n_fired} · closed {j.best.n_closed}</span>
          <button
            type="button"
            className="rounded border border-white/20 px-2 py-0.5 text-[11px] text-text-mid transition hover:bg-white/5"
            onClick={() => onPromote(j.best!.params, `joint:${label}`)}
          >
            Promote…
          </button>
        </div>
      ) : (
        <span className="text-[11px] text-text-dim">No rankable joint combo.</span>
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
      <Td>
        <Badge variant={badgeVariant(c.verdict)} size="sm" title={verdictHelp[c.verdict] ?? ''}>
          {c.verdict}
        </Badge>
      </Td>
      <Td right>{fmt(c.train.score, 3)}</Td>
      <Td right>{fmt(c.validate.score, 3)}</Td>
      <Td right>{c.retention == null ? '—' : `${(c.retention * 100).toFixed(0)}%`}</Td>
      <Td right>{fmt(c.validate.total_pnl_sol, 3)}</Td>
      <Td right>{c.validate.n_closed}</Td>
      <Td right>
        <button
          type="button"
          className="rounded border border-white/20 px-2 py-0.5 text-[11px] text-text-mid transition hover:bg-white/5"
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

/** A free-text config field. Used where the value is a SPAN rather than a number:
 *  `30` is seconds, `30sl@1` is a lagged slot window, `20p` is twenty prints - none
 *  of which a number input can express. `invalid` tints the field so an unparseable
 *  span is visible before the run is admitted, not after every metric scores NaN. */
function TextField({
  label, value, onChange, width, invalid, title,
}: {
  label: string; value: string; onChange: (v: string) => void; width: string;
  invalid?: boolean; title?: string;
}) {
  return (
    <div className={`flex flex-col gap-1 ${width}`}>
      <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80">{label}</span>
      <Input
        type="text"
        value={value}
        title={title}
        aria-invalid={invalid || undefined}
        className={invalid ? 'border-red/60' : undefined}
        onChange={(e) => onChange(e.target.value)}
      />
    </div>
  );
}

/** The span a screened metric was read at, labelled. Prefers the backend's `window`
 *  string and falls back to the legacy seconds scalar, so a slot or print row names
 *  its own basis instead of dropping the qualifier. */
function windowLabel(m: { window?: string | null; window_sec: number | null }): string {
  if (m.window) return m.window;
  return m.window_sec != null ? `${m.window_sec}s` : '';
}

function NumField({
  label, value, min, max, onChange, width, step,
}: {
  label: string; value: number; min?: number; max?: number; step?: number;
  onChange: (v: number) => void; width: string;
}) {
  return (
    <div className={`flex flex-col gap-1 ${width}`}>
      <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80">{label}</span>
      <Input
        type="number"
        min={min}
        max={max}
        step={step}
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

function Td({
  children,
  right,
  className,
}: {
  children?: React.ReactNode;
  right?: boolean;
  className?: string;
}) {
  return (
    <td className={`py-1 ${right ? 'text-right' : 'text-left'} ${className ?? ''}`}>{children}</td>
  );
}
