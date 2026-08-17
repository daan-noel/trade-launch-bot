import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useSearchParams } from 'react-router-dom';

import { Accordion } from 'components/ui/Accordion';
import { Checkbox } from 'components/ui/Checkbox';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import { IconButton } from 'components/ui/IconButton';
import { PlayIcon, SpinnerIcon } from 'components/ui/icons';
import { Input } from 'components/ui/Input';
import { InlineAlert } from 'components/ui/Modal';
import { PageHeader } from 'components/ui/PageHeader';
import { Select } from 'components/ui/Select';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { SearchableSelect, type SearchableSelectOption } from 'components/ui/SearchableSelect';
import { LabelTip } from 'components/strategy/LabelTip';
import { FingerprintScopeControl } from 'components/strategy/FingerprintScopeControl';
import { useFingerprintMatches } from '@lab/components/strategy/useFingerprintMatches';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { ACCORDION_IDS, STORAGE_KEYS } from 'lib/storage';
import { apiErrorMessage } from 'store/baseApi';
import { connectFamilySearchFinished, connectSimulationFinished, onSseReopen } from 'services/sse';
import { useGetFingerprintsQuery, useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import {
  COST_MODELS,
  FILL_MODELS,
  solToLamports,
  type CostModelId,
  type EngineRuleDraft,
  type FillModelId,
  type Fingerprint,
  type PromotedRuleDraft,
} from 'lib/strategy/types';
import type { RuleEditorDraft } from 'components/strategy/RuleEditor';
import { EXECUTION_MODEL_HELP, type HelpTip } from 'lib/strategy/strategyHelp';
import { STRATEGY_PARAMS } from 'lib/strategy/nav';
import { PromoteRuleModal } from '@lab/components/sweep/PromoteRuleModal';
import { DryRunDetail } from '@lab/components/strategy/DryRunDetail';
import { FamilySearchBoard } from '@lab/components/family/FamilySearchBoard';
import {
  useBackgroundJobActions,
  useBackgroundJobsState,
} from '@lab/context/BackgroundJobsContext';
import {
  useGetLastFamilySearchQuery,
  useLazyGetFamilySearchQuery,
  useStartEngineSimulationMutation,
  useStartFamilySearchMutation,
} from '@lab/store/labEndpoints';
import type {
  FamilyAxisName,
  FamilyCandidateRow,
  FamilySearchReport,
} from '@lab/lib/familySearchTypes';

/** The fingerprint columns a family may vary. `''` ⇒ let the backend resolve it. */
const AXES: { id: FamilyAxisName | ''; label: string }[] = [
  { id: '', label: 'Auto — the axis with the most siblings' },
  { id: 'spendable_in', label: 'spendable_lamports_in' },
  { id: 'init_buy', label: 'init_buy_lamports' },
  { id: 'max_cost', label: 'max_cost_lamports' },
  { id: 'cu_price', label: 'cu_price' },
  { id: 'cu_limit', label: 'cu_limit' },
  { id: 'first_slot_buy', label: 'first_slot_buy_count' },
  { id: 'first_slot_sell', label: 'first_slot_sell_count' },
];

interface Config {
  fingerprintId: string | null;
  createdAfter: string;
  createdBefore: string;
  buyAmountSol: number;
  fillModel: FillModelId;
  costModel: CostModelId;
  skipDuplicateIdentity: boolean;
  tokenCap: number;
  slots: number;
  variedAxis: FamilyAxisName | '';
  freshnessSlackHours: number;
  costClearanceMargin: number;
  minWinRatePct: number;
  minClosed: number;
  standingExit: string;
  maxConcurrentTokens: number;
  maxTotalTokens: number;
  incumbentRuleId: string | null;
}

const DEFAULTS: Config = {
  fingerprintId: null,
  createdAfter: '',
  createdBefore: '',
  buyAmountSol: 0.01,
  fillModel: 'worst_case',
  costModel: 'pumpfun_impact',
  skipDuplicateIdentity: true,
  tokenCap: 10000,
  slots: 40,
  variedAxis: '',
  freshnessSlackHours: 1,
  costClearanceMargin: 0,
  minWinRatePct: 0,
  minClosed: 8,
  standingExit: '',
  maxConcurrentTokens: 0,
  maxTotalTokens: 0,
  incumbentRuleId: null,
};

/**
 * Standing exit terms, one per line — mechanics that ride into every candidate and
 * the ungated control but are never searched, ablated or credited. Written exactly as
 * the attribution table prints them, so a term can be copied straight back out of a
 * result: `liquidity >= 85`, `nonvol_buy(2s) >= 0.9`.
 */
function standingTerms(raw: string): string[] {
  return raw
    .split(/\r?\n/)
    .map((t) => t.trim())
    .filter(Boolean);
}

function toUtc(local: string): string | undefined {
  if (!local) return undefined;
  const d = new Date(local.endsWith('Z') ? local : `${local}Z`);
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString();
}

const TIPS = {
  fingerprint: {
    title: 'Fingerprint (required)',
    body: 'The TARGET: the cohort the reported number comes from. Its siblings — fingerprints identical on every axis but one — are resolved mechanically off the fingerprints table, so you pick one launch shape and the run finds its family.',
  },
  range: {
    title: 'Created range · UTC',
    body: 'Tokens created in this UTC window, for every cohort in the family. A run whose upper bound outruns the lake is REFUSED rather than silently shortened — sync the lake first if that happens.',
  },
  buySol: {
    title: 'Buy SOL',
    body: 'Size of each simulated buy, for every cohort. It is physics, not a rule setting: cost is U-shaped under pumpfun_impact, so this moves the economics of every candidate. Nothing reads it off a saved rule.',
  },
  slots: {
    title: 'Candidate slots',
    body: 'How many candidates the generator earns from the target cohort\'s own signatures. No end-event family may take more than 40% of them, so one kind of alarm cannot fill the search.',
  },
  tokenCap: {
    title: 'Token cap',
    body: 'Hard cap on tokens loaded per cohort. A cohort that hits it is scored on its newest N only, and the run says so.',
  },
  axis: {
    title: 'Varied axis',
    body: 'Which fingerprint column the family is allowed to differ on. Auto picks the axis with the most siblings. Pin it when a fingerprint sits in more than one family and you want a specific one.',
  },
  slack: {
    title: 'Freshness slack',
    body: 'How far the requested upper bound may outrun the lake\'s newest print before the run refuses. A sealed-day export is routinely under an hour behind; two days behind is a silently shorter range answering a different question.',
  },
  clearance: {
    title: 'Cost bar (x round trip)',
    body: 'How much headroom over one round trip the cohort\'s typical best available exit must leave before a search runs at all. 0 refuses only the unarguable case — the best exit does not clear its own costs, so no exit rule can beat it. 1 is the stricter bar the dump-scalp result sets: a rule only ever takes a fraction of the best exit, so a cohort clearing by less than its own cost looks tradeable offline and is not.',
  },
  caps: {
    title: 'Concurrency caps',
    body: 'Max concurrent / total tokens the simulated engine may hold, 0 = unlimited. They change which tokens are entered at all, so they come from this form and never from a saved rule.',
  },
  minWin: {
    title: 'Min win rate %',
    body: 'An absolute floor on top of the bar the cohort sets for itself. The draft must clear BOTH this and the ungated control’s own win rate — entry decides safety, so a gate that does not enter more safely than buying everything is not filtering anything. 0 leaves the control as the only bar.',
  },
  minClosed: {
    title: 'Min closes',
    body: 'Closed positions a candidate needs before its win rate is believed. Three wins in four trades is not a 75% win rate, and without this floor the search picks whichever rule traded least.',
  },
  standing: {
    title: 'Standing exit terms',
    body: 'Mechanical alarms you always want, one per line, written exactly as the attribution table prints them: `liquidity >= 85` (sell at migration), `nonvol_buy(2s) >= 0.9`. Each rides into every candidate AND the ungated control, so the numbers describe a rule you would really run — and none of them is searched, ablated or credited with the edge. A term that does not parse fails the run rather than being silently dropped.',
  },
  incumbent: {
    title: 'Incumbent (display only)',
    body: 'A saved rule scored on the target cohort as one extra column. It seeds nothing and supplies no buy size, cap, threshold or structure — a search anchored to a promoted rule can only rediscover it.',
  },
} satisfies Record<string, HelpTip>;

/**
 * Lab page: run family search for one fingerprint and board the result.
 *
 * The job grades a fingerprint's sibling family — fingerprints identical on every
 * axis but one — fitting the candidate ordering across the siblings and taking the
 * reported level from the held-out target cohort. The page ends where the operator
 * needs it to: Promote the draft to an inactive paper rule, or Simulate it unsaved.
 *
 * Charter: `hunter/docs/roadmap/family-search.md`. Job: `hunter/docs/arch/sweep.md`.
 */
export function FamilySearchPage() {
  const [stored, setConfig] = useLocalStorage<Config>(STORAGE_KEYS.familySearchConfig, DEFAULTS);
  const config: Config = { ...DEFAULTS, ...stored };
  const set = <K extends keyof Config>(key: K, value: Config[K]) =>
    setConfig((prev) => ({ ...DEFAULTS, ...prev, [key]: value }));

  const { data: fingerprints = [] } = useGetFingerprintsQuery();
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const selectedFp = config.fingerprintId
    ? fingerprints.find((f) => f.id === config.fingerprintId)
    : undefined;
  const fpMatches = useFingerprintMatches(config.fingerprintId, selectedFp?.name);

  const [start] = useStartFamilySearchMutation();
  const [fetchResult] = useLazyGetFamilySearchQuery();
  const { data: lastResult } = useGetLastFamilySearchQuery();
  const [startSim] = useStartEngineSimulationMutation();
  const { markStarting, markFinished } = useBackgroundJobActions();
  const { isRunning } = useBackgroundJobsState();
  const running = isRunning('family_search', 'family_search');

  const [result, setResult] = useState<FamilySearchReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [promoteDraft, setPromoteDraft] = useState<PromotedRuleDraft | null>(null);
  const [simRunId, setSimRunId] = useState<string | null>(null);
  const [simDraft, setSimDraft] = useState<RuleEditorDraft | null>(null);
  const [simBusy, setSimBusy] = useState(false);
  const simHandle = useRef<{ close: () => void } | null>(null);
  const searchRunId = useRef<string | null>(null);

  useEffect(() => () => simHandle.current?.close(), []);

  // The board lands on `family_search_finished` (then GET) — no poll deadline, same
  // as sweep and rule search. A reopen retries the GET in case the terminal frame
  // was missed during the gap.
  useEffect(() => {
    const apply = async (runId: string, missingOk: boolean) => {
      try {
        const res = await fetchResult(runId).unwrap();
        setResult(res.result);
        setError(null);
        searchRunId.current = null;
        markFinished('family_search', 'family_search');
      } catch (e) {
        if (missingOk) return;
        setError(apiErrorMessage(e as never, 'Failed to load family-search result'));
        markFinished('family_search', 'family_search');
      }
    };

    const handle = connectFamilySearchFinished((ev) => {
      if (searchRunId.current && ev.run_id !== searchRunId.current) return;
      searchRunId.current = null;
      if (ev.error) {
        setError(ev.error);
        markFinished('family_search', 'family_search');
        return;
      }
      if (ev.cancelled) {
        markFinished('family_search', 'family_search');
        return;
      }
      void apply(ev.run_id, false);
    });
    const offReopen = onSseReopen(() => {
      const id = searchRunId.current;
      if (id) void apply(id, true);
    });
    return () => {
      handle.close();
      offReopen();
    };
  }, [fetchResult, markFinished]);

  useEffect(() => {
    if (lastResult && !result) setResult(lastResult.result);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once, when the cache arrives
  }, [lastResult]);

  const [searchParams] = useSearchParams();
  const appliedFpParam = useRef<string | null>(null);
  useEffect(() => {
    const fp = searchParams.get(STRATEGY_PARAMS.fingerprint);
    if (!fp || appliedFpParam.current === fp) return;
    if (!fingerprints.some((f) => f.id === fp)) return;
    appliedFpParam.current = fp;
    if (fp !== config.fingerprintId) set('fingerprintId', fp);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- param + loaded list are the SSOTs
  }, [searchParams, fingerprints]);

  const incumbentOptions: SearchableSelectOption<(typeof rules)[number]>[] = useMemo(() => {
    const fpId = config.fingerprintId;
    return rules
      .filter((r) => !fpId || r.fingerprint_id === fpId)
      .map((r) => ({
        value: r.id,
        label: r.rule_name,
        searchText: `${r.rule_name} ${r.id}`,
        data: r,
      }));
  }, [rules, config.fingerprintId]);

  async function handleRun() {
    if (running) return;
    if (!config.fingerprintId) {
      setError('Pick a fingerprint — the family is resolved from it.');
      return;
    }
    setError(null);
    setSimRunId(null);
    markStarting('family_search', 'family_search', 'Family search');
    try {
      const { run_id } = await start({
        fingerprint_id: config.fingerprintId,
        created_after: toUtc(config.createdAfter),
        created_before: toUtc(config.createdBefore),
        buy_amount_sol: config.buyAmountSol,
        fill_model: config.fillModel,
        cost_model: config.costModel,
        skip_duplicate_identity: config.skipDuplicateIdentity,
        max_concurrent_tokens: config.maxConcurrentTokens,
        max_total_tokens: config.maxTotalTokens,
        token_cap: config.tokenCap,
        varied_axis: config.variedAxis || undefined,
        slots: config.slots,
        freshness_slack_secs: Math.round(config.freshnessSlackHours * 3600),
        cost_clearance_margin: config.costClearanceMargin,
        min_win_rate_pct: config.minWinRatePct,
        min_closed: config.minClosed,
        standing_exit: standingTerms(config.standingExit),
        incumbent_rule_id: config.incumbentRuleId ?? undefined,
      }).unwrap();
      searchRunId.current = run_id;
      try {
        const res = await fetchResult(run_id).unwrap();
        setResult(res.result);
        searchRunId.current = null;
        markFinished('family_search', 'family_search');
      } catch {
        /* still running — the board lands on family_search_finished */
      }
    } catch (e) {
      searchRunId.current = null;
      markFinished('family_search', 'family_search');
      setError(apiErrorMessage(e as never, 'Failed to start family search'));
    }
  }

  /** The fingerprint a promoted rule binds to is the run's TARGET, never the form's
   *  current pick — the two diverge as soon as the form is edited after a run. */
  function targetFingerprint(): Fingerprint | undefined {
    return result ? fingerprints.find((f) => f.id === result.fingerprint_id) : undefined;
  }

  function promote(row: FamilyCandidateRow, label: string) {
    const fp = targetFingerprint();
    if (!fp) {
      setError('Promote needs the run\'s target fingerprint, which is no longer in the list.');
      return;
    }
    setPromoteDraft({
      rule_name: `family-search · ${label}`,
      fingerprint_id: fp.id,
      trade_mode: 'paper',
      buy_amount_lamports: solToLamports(config.buyAmountSol) ?? 10_000_000,
      max_concurrent_tokens: config.maxConcurrentTokens,
      max_total_tokens: config.maxTotalTokens,
      params: row.params,
      fingerprint: fp,
    });
  }

  async function simulateDraft() {
    const draft = result?.draft;
    const fp = targetFingerprint();
    if (!draft || !fp) return;
    setError(null);
    setSimRunId(null);
    setSimBusy(true);
    const engineDraft: EngineRuleDraft = {
      fingerprint_id: fp.id,
      params: draft.params,
      buy_amount_sol: config.buyAmountSol,
      max_concurrent_tokens: config.maxConcurrentTokens,
      max_total_tokens: config.maxTotalTokens,
      trade_mode: 'paper',
    };
    const editorDraft: RuleEditorDraft = {
      rule_name: 'family-search draft',
      fingerprint_id: fp.id,
      trade_mode: 'paper',
      buy_amount_lamports: solToLamports(config.buyAmountSol) ?? 10_000_000,
      max_concurrent_tokens: config.maxConcurrentTokens,
      max_total_tokens: config.maxTotalTokens,
      params: draft.params,
      tags: ['src:family-search'],
    };
    try {
      const res = await startSim({
        draft: engineDraft,
        since: toUtc(config.createdAfter),
        until: toUtc(config.createdBefore),
        fill_model: config.fillModel,
        cost_model: config.costModel,
        skip_duplicate_identity: config.skipDuplicateIdentity,
      }).unwrap();
      simHandle.current?.close();
      simHandle.current = connectSimulationFinished((ev) => {
        if (ev.rule_id !== res.run_id) return;
        simHandle.current?.close();
        simHandle.current = null;
        setSimBusy(false);
        if (!ev.cancelled) {
          setSimDraft(editorDraft);
          setSimRunId(res.run_id);
        }
      });
    } catch (e) {
      setSimBusy(false);
      setError(apiErrorMessage(e as never, 'Simulate failed'));
    }
  }

  // The most likely first-run failure, and it is fixable in one command — so it
  // gets the command rather than a bare refusal string.
  const staleRefusal = error?.includes('refused on freshness') ?? false;

  return (
    <div className="pt-2">
      <PageHeader
        title="Family search"
        description="One launch shape and the sibling fingerprints that differ from it on a single axis. The candidate ordering is fitted across the siblings; the number you read comes from the one cohort held out of that fit. Nothing in a saved rule enters the search."
      />

      {/* ── Required: what to search ──────────────────────────────────────── */}
      <div className="mb-3">
        <FingerprintScopeControl
          fingerprints={fingerprints}
          value={config.fingerprintId}
          onChange={(id) => {
            set('fingerprintId', id || null);
            set('incumbentRuleId', null);
          }}
          tip={TIPS.fingerprint}
          label="Fingerprint"
          emptyOptionLabel="Pick a fingerprint"
          scopedDescription="This cohort is held out: the family is fitted without it, and its replay is the number the board reports."
          manualHint="A fingerprint is required — the family is resolved from it, not from a corpus-wide grid."
          matchedCount={fpMatches.count}
          matchedCountLoading={fpMatches.countLoading}
          onViewMatches={fpMatches.openMatches}
          onRequestMatchCount={fpMatches.ensureCount}
        />
        {fpMatches.matchesModal}
      </div>

      <div className="mb-3 flex flex-wrap items-end gap-3">
        <Field label="Created range · UTC" tip={TIPS.range}>
          <DateTimeRangePicker
            aria-label="Created range"
            zoneLabel="UTC"
            emptyLabel="All history"
            customPreset="custom"
            value={{ preset: 'custom', from: config.createdAfter, to: config.createdBefore }}
            onChange={({ from, to }) => {
              set('createdAfter', from);
              set('createdBefore', to);
            }}
          />
        </Field>
        <NumField
          label="Buy SOL"
          tip={TIPS.buySol}
          value={config.buyAmountSol}
          min={0.001}
          step={0.01}
          onChange={(v) => set('buyAmountSol', v)}
          width="w-[92px]"
        />
        <IconButton
          variant="primary"
          size="lg"
          onClick={() => void handleRun()}
          disabled={running || !config.fingerprintId}
          label={running ? 'Running…' : 'Search family'}
          title={
            !config.fingerprintId
              ? 'Pick a fingerprint first'
              : running
                ? 'Running… (cancel from the jobs indicator)'
                : 'Search family'
          }
        >
          {running ? <SpinnerIcon /> : <PlayIcon />}
        </IconButton>
        {running && (
          <span className="pb-2 text-[11px] text-text-dim">
            Loading one cohort at a time — progress and cancel are in the jobs indicator.
          </span>
        )}
      </div>

      {/* ── Everything else, out of the way but never hidden ──────────────── */}
      <div className="mb-4">
        <Accordion
          title={<span className="text-xs text-text-mid">Execution, scope and comparison</span>}
          badge={
            <span className="text-[11px] text-text-dim">
              {FILL_MODELS.find((m) => m.id === config.fillModel)?.label} ·{' '}
              {COST_MODELS.find((m) => m.id === config.costModel)?.label} ·{' '}
              {config.skipDuplicateIdentity ? 'copycat on' : 'copycat off'} · {config.slots} slots
              {config.incumbentRuleId ? ' · incumbent set' : ''}
            </span>
          }
          padding="sm"
          storageKey={ACCORDION_IDS.familySearchAdvanced}
          defaultOpen={false}
        >
          <div className="flex flex-wrap items-end gap-3 px-1 pb-1">
            <label className="flex flex-col gap-1 text-xs text-text-dim">
              <span className="flex items-center gap-1 text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
                Fill
                <InfoTooltip {...EXECUTION_MODEL_HELP.fillModel} />
              </span>
              <Select
                fieldSize="sm"
                value={config.fillModel}
                onChange={(e) => set('fillModel', e.target.value as FillModelId)}
                className="w-36"
              >
                {FILL_MODELS.map((m) => (
                  <option key={m.id} value={m.id} title={m.hint}>
                    {m.label}
                  </option>
                ))}
              </Select>
            </label>
            <label className="flex flex-col gap-1 text-xs text-text-dim">
              <span className="flex items-center gap-1 text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
                Cost
                <InfoTooltip {...EXECUTION_MODEL_HELP.costModel} />
              </span>
              <Select
                fieldSize="sm"
                value={config.costModel}
                onChange={(e) => set('costModel', e.target.value as CostModelId)}
                className="w-36"
              >
                {COST_MODELS.map((m) => (
                  <option key={m.id} value={m.id} title={m.hint}>
                    {m.label}
                  </option>
                ))}
              </Select>
            </label>
            <Field label="Varied axis" tip={TIPS.axis}>
              <Select
                fieldSize="sm"
                value={config.variedAxis}
                onChange={(e) => set('variedAxis', e.target.value as FamilyAxisName | '')}
                className="w-56"
              >
                {AXES.map((a) => (
                  <option key={a.id || 'auto'} value={a.id}>
                    {a.label}
                  </option>
                ))}
              </Select>
            </Field>
            <NumField
              label="Slots"
              tip={TIPS.slots}
              value={config.slots}
              min={1}
              max={400}
              onChange={(v) => set('slots', v)}
              width="w-[80px]"
            />
            <NumField
              label="Token cap"
              tip={TIPS.tokenCap}
              value={config.tokenCap}
              min={1}
              max={100000}
              onChange={(v) => set('tokenCap', v)}
              width="w-[110px]"
            />
            <NumField
              label="Slack (h)"
              tip={TIPS.slack}
              value={config.freshnessSlackHours}
              min={0}
              step={0.5}
              onChange={(v) => set('freshnessSlackHours', v)}
              width="w-[90px]"
            />
            <NumField
              label="Cost bar (x)"
              tip={TIPS.clearance}
              value={config.costClearanceMargin}
              min={0}
              step={0.5}
              onChange={(v) => set('costClearanceMargin', v)}
              width="w-[100px]"
            />
            <NumField
              label="Min win %"
              tip={TIPS.minWin}
              value={config.minWinRatePct}
              min={0}
              max={100}
              step={1}
              onChange={(v) => set('minWinRatePct', v)}
              width="w-[95px]"
            />
            <NumField
              label="Min closes"
              tip={TIPS.minClosed}
              value={config.minClosed}
              min={1}
              onChange={(v) => set('minClosed', v)}
              width="w-[95px]"
            />
            <NumField
              label="Max concurrent"
              tip={TIPS.caps}
              value={config.maxConcurrentTokens}
              min={0}
              onChange={(v) => set('maxConcurrentTokens', v)}
              width="w-[110px]"
            />
            <NumField
              label="Max total"
              tip={TIPS.caps}
              value={config.maxTotalTokens}
              min={0}
              onChange={(v) => set('maxTotalTokens', v)}
              width="w-[100px]"
            />
            <label className="flex h-[34px] items-center gap-1.5 text-sm text-text-mid">
              <Checkbox
                checked={config.skipDuplicateIdentity}
                onChange={(e) => set('skipDuplicateIdentity', e.target.checked)}
              />
              <LabelTip
                tip={{
                  title: 'Copycat guard',
                  body: 'Skip a second token sharing a creator identity inside the window. ON by default: without it the ungated control counts the same launch several times, and the draft-vs-ungated comparison inverts.',
                }}
              >
                copycat
              </LabelTip>
            </label>
            <div className="w-full max-w-md">
              <Field label="Standing exit (one per line)" tip={TIPS.standing}>
                <textarea
                  rows={2}
                  spellCheck={false}
                  value={config.standingExit}
                  onChange={(e) => set('standingExit', e.target.value)}
                  placeholder={'liquidity >= 85'}
                  className="w-full rounded border border-white/15 bg-black/20 px-2 py-1 font-mono text-xs text-text-mid placeholder:text-text-dim/50 focus:border-primary/50 focus:outline-none"
                />
              </Field>
            </div>
            <div className="w-full max-w-md">
              <Field label="Incumbent (display only)" tip={TIPS.incumbent}>
                <SearchableSelect
                  options={incumbentOptions}
                  value={config.incumbentRuleId}
                  onChange={(id) => set('incumbentRuleId', id || null)}
                  placeholder="None"
                  emptyOptionLabel="None — no incumbent"
                  disabled={!config.fingerprintId}
                  noResultsLabel="No rules for this fingerprint"
                />
              </Field>
              <p className="mt-1 text-[11px] leading-snug text-text-dim">
                Scored as one extra column and nothing else. Buy size and caps come from this form
                either way — an incumbent that supplied them would move the economics of every
                candidate it is being compared against.
              </p>
            </div>
          </div>
        </Accordion>
      </div>

      {error && (
        <div className="mb-4">
          <InlineAlert variant="error">
            <span className="block">{error}</span>
            {staleRefusal && (
              <span className="mt-1 block text-[11px]">
                Sync the lake, then re-run:{' '}
                <code className="rounded bg-black/30 px-1 py-0.5">
                  scripts/db-incremental-sync.ps1 -IncludeToday -ExportLake
                </code>{' '}
                — or lower the range&apos;s upper bound to what the lake already covers.
              </span>
            )}
          </InlineAlert>
        </div>
      )}

      {result && (
        <FamilySearchBoard
          report={result}
          onPromote={promote}
          onSimulate={() => void simulateDraft()}
          simBusy={simBusy}
        />
      )}

      {simRunId && simDraft && (
        <div className="mt-8">
          <DryRunDetail runId={simRunId} draft={simDraft} />
        </div>
      )}

      <PromoteRuleModal
        draft={promoteDraft}
        onClose={() => setPromoteDraft(null)}
        sourceTag="src:family-search"
      />
    </div>
  );
}

function Field({ label, tip, children }: { label: string; tip?: HelpTip; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
        {tip ? <LabelTip tip={tip}>{label}</LabelTip> : label}
      </span>
      {children}
    </div>
  );
}

function NumField({
  label,
  tip,
  value,
  min,
  max,
  step,
  onChange,
  width,
}: {
  label: string;
  tip?: HelpTip;
  value: number;
  min?: number;
  max?: number;
  step?: number;
  onChange: (v: number) => void;
  width: string;
}) {
  return (
    <div className={`flex flex-col gap-1 ${width}`}>
      <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
        {tip ? <LabelTip tip={tip}>{label}</LabelTip> : label}
      </span>
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
