import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useSearchParams } from 'react-router-dom';

import { Badge } from 'components/ui/Badge';
import { Checkbox } from 'components/ui/Checkbox';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import { IconButton } from 'components/ui/IconButton';
import { PlayIcon, SimulateIcon, SpinnerIcon } from 'components/ui/icons';
import { Input } from 'components/ui/Input';
import { InlineAlert } from 'components/ui/Modal';
import { Select } from 'components/ui/Select';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { LabelTip } from 'components/strategy/LabelTip';
import { FingerprintScopeControl } from 'components/strategy/FingerprintScopeControl';
import { SearchableSelect, type SearchableSelectOption } from 'components/ui/SearchableSelect';
import { ruleParamsCell } from 'components/strategy/RuleParamsSummary';
import { useFingerprintMatches } from '@lab/components/strategy/useFingerprintMatches';
import { PageHeader } from 'components/ui/PageHeader';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';
import { apiErrorMessage } from 'store/baseApi';
import { connectSimulationFinished } from 'services/sse';
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
import {
  useBackgroundJobActions,
  useBackgroundJobsState,
} from '@lab/context/BackgroundJobsContext';
import {
  useGetLastRuleSearchQuery,
  useLazyGetRuleSearchQuery,
  useStartEngineSimulationMutation,
  useStartRuleSearchMutation,
} from '@lab/store/labEndpoints';
import type {
  RuleSearchReport,
  RuleSearchScored,
  RuleSearchVerdict,
} from '@lab/lib/ruleSearchTypes';

interface Config {
  createdAfter: string;
  createdBefore: string;
  tokenCap: number;
  fingerprintId: string | null;
  buyAmountSol: number;
  fillModel: FillModelId;
  costModel: CostModelId;
  skipDuplicateIdentity: boolean;
  incumbentRuleId: string | null;
}

const DEFAULTS: Config = {
  createdAfter: '',
  createdBefore: '',
  tokenCap: 10000,
  fingerprintId: null,
  buyAmountSol: 1.0,
  fillModel: 'worst_case',
  costModel: 'pumpfun_impact',
  skipDuplicateIdentity: true,
  incumbentRuleId: null,
};

function toUtc(local: string): string | undefined {
  if (!local) return undefined;
  const d = new Date(local.endsWith('Z') ? local : `${local}Z`);
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString();
}

const fmt = (n: number | null | undefined, digits = 2): string =>
  n == null || !Number.isFinite(n) ? '—' : n.toFixed(digits);

const pct = (n: number | null | undefined): string =>
  n == null || !Number.isFinite(n) ? '—' : `${(n * 100).toFixed(0)}%`;

const VERDICT: Record<RuleSearchVerdict, { variant: 'danger' | 'warning' | 'success'; label: string; help: string }> = {
  refuse: {
    variant: 'danger',
    label: 'Refuse',
    help: 'Nothing paid on this range — not the champion, not empty-entry. Paper the next launch burst, or pick a new range.',
  },
  ungated: {
    variant: 'warning',
    label: 'Ungated',
    help: 'Empty-entry (buy everything that matches) pays as well as or better than any selector. The juice is ungated — a filter does not add SOL here.',
  },
  candidate: {
    variant: 'success',
    label: 'Candidate',
    help: 'A selector beats empty-entry on authority SOL. Promote to inactive paper, then Simulate before going live.',
  },
};

const TIPS = {
  tokenCap: {
    title: 'Token cap',
    body: 'Hard cap on tokens loaded for this fingerprint + range. Raise it only if the matched count is larger and you want the full cohort.',
  },
  buySol: {
    title: 'Buy SOL',
    body: 'Size of each simulated buy. When an incumbent is set, that rule\'s buy size and concurrency caps replace this field.',
  },
  incumbent: {
    title: 'Incumbent',
    body: 'A saved rule scored on this same range for comparison. It never seeds the search — the champion is built from this cohort\'s cuts, not from the incumbent\'s clauses.',
  },
  champion: {
    title: 'Champion',
    body: 'Best selector on this range after replay. Ranking, verdict, and archive order use authority SOL (this run\'s Fill), not the first-in-window number.',
  },
  emptyEntry: {
    title: 'Empty-entry',
    body: 'Buy every matched token; same exit bag as the champion. If this column wins, a selector is not adding SOL — verdict Ungated.',
  },
  incumbentCol: {
    title: 'Incumbent',
    body: 'Your saved rule, replayed on this range with the same Fill / cost / copycat. Compare only.',
  },
  solPair: {
    title: 'Authority vs first-in-window',
    body: 'Left (authority) uses the Fill you ran with — default Worst-case — and is the only number that ranks champion, verdict, and archive. Right re-prices the same closed trades at the next print after the signal (first-in-window). A gap is fill sensitivity, not a second ranking.',
  },
  nClosed: {
    title: 'Closed (n)',
    body: 'Round-trips that entered and exited. Too few closed trades → no rule, even if SOL looks large.',
  },
  pf: {
    title: 'Profit factor',
    body: 'Gross wins ÷ gross losses under authority Fill. Above 1 means winners cover losers. Ranking still uses total SOL, not PF.',
  },
  enterPct: {
    title: 'Enter %',
    body: 'Share of matched tokens that entered, with copycat ON (this job\'s default). A selective claim wants this well below 100% — necessary, not sufficient.',
  },
  unguarded: {
    title: 'Unguarded enter %',
    body: 'Same enter rate with copycat OFF. If this is much higher than enter %, the guard is doing a lot of the selecting.',
  },
  archive: {
    title: 'Archive',
    body: 'Top combos by authority (replay) SOL. Row 1 is the champion. Promote any row; Simulate is champion-only.',
  },
  nMatched: {
    title: 'Matched / combos',
    body: 'Matched = tokens this fingerprint hit in the range (after the cap). Combos = entry × exit fillings scored. The board is the replay of the top archive, not the fast walk.',
  },
} satisfies Record<string, HelpTip>;

/**
 * Lab page: run rule search for one fingerprint + datetime range and board the
 * champion vs empty-entry vs incumbent. Promote uses the sweep modal.
 *
 * Architecture: `hunter/docs/arch/sweep.md` "Rule search".
 */
export function RuleSearchPage() {
  const [stored, setConfig] = useLocalStorage<Config>(STORAGE_KEYS.ruleSearchConfig, DEFAULTS);
  const config: Config = { ...DEFAULTS, ...stored };
  const set = <K extends keyof Config>(key: K, value: Config[K]) =>
    setConfig((prev) => ({ ...DEFAULTS, ...prev, [key]: value }));

  const { data: fingerprints = [] } = useGetFingerprintsQuery();
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const selectedFp = config.fingerprintId
    ? fingerprints.find((f) => f.id === config.fingerprintId)
    : undefined;
  const fpMatches = useFingerprintMatches(config.fingerprintId, selectedFp?.name);

  const [start] = useStartRuleSearchMutation();
  const [fetchResult] = useLazyGetRuleSearchQuery();
  const { data: lastResult } = useGetLastRuleSearchQuery();
  const [startSim] = useStartEngineSimulationMutation();
  const { markStarting, markFinished } = useBackgroundJobActions();
  const { isRunning } = useBackgroundJobsState();
  const running = isRunning('rule_search', 'rule_search');

  const [result, setResult] = useState<RuleSearchReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [promoteDraft, setPromoteDraft] = useState<PromotedRuleDraft | null>(null);
  const [simRunId, setSimRunId] = useState<string | null>(null);
  const [simDraft, setSimDraft] = useState<RuleEditorDraft | null>(null);
  const [simBusy, setSimBusy] = useState(false);
  const simHandle = useRef<{ close: () => void } | null>(null);

  useEffect(() => () => simHandle.current?.close(), []);

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

  const incumbent = config.incumbentRuleId
    ? rules.find((r) => r.id === config.incumbentRuleId)
    : undefined;

  async function handleRun() {
    if (running) return;
    if (!config.fingerprintId) {
      setError('Pick a fingerprint — this job searches one habit window.');
      return;
    }
    setError(null);
    markStarting('rule_search', 'rule_search', 'Rule search');
    try {
      const { run_id } = await start({
        fingerprint_id: config.fingerprintId,
        created_after: toUtc(config.createdAfter),
        created_before: toUtc(config.createdBefore),
        buy_amount_sol: config.buyAmountSol,
        fill_model: config.fillModel,
        cost_model: config.costModel,
        skip_duplicate_identity: config.skipDuplicateIdentity,
        incumbent_rule_id: config.incumbentRuleId ?? undefined,
        token_cap: config.tokenCap,
      }).unwrap();

      for (let i = 0; i < 600; i++) {
        await new Promise((r) => setTimeout(r, 500));
        try {
          const res = await fetchResult(run_id).unwrap();
          setResult(res.result);
          markFinished('rule_search', 'rule_search');
          return;
        } catch {
          /* still running */
        }
      }
      markFinished('rule_search', 'rule_search');
      setError('Timed out waiting for the rule-search result.');
    } catch (e) {
      markFinished('rule_search', 'rule_search');
      setError(apiErrorMessage(e as never, 'Failed to start rule search'));
    }
  }

  function promote(row: RuleSearchScored, label: string) {
    const fp: Fingerprint | undefined = config.fingerprintId
      ? fingerprints.find((f) => f.id === config.fingerprintId)
      : undefined;
    if (!fp) {
      setError('Promote needs a fingerprint.');
      return;
    }
    const buySol = incumbent ? (incumbent.buy_amount_lamports / 1e9) : config.buyAmountSol;
    setPromoteDraft({
      rule_name: `rule-search · ${label}`,
      fingerprint_id: fp.id,
      trade_mode: 'paper',
      buy_amount_lamports: solToLamports(buySol) ?? 1_000_000_000,
      max_concurrent_tokens: incumbent?.max_concurrent_tokens ?? 0,
      max_total_tokens: incumbent?.max_total_tokens ?? 0,
      params: row.params,
      fingerprint: fp,
    });
  }

  async function simulateChampion() {
    const champ = result?.champion;
    if (!champ || !config.fingerprintId) return;
    setError(null);
    setSimRunId(null);
    setSimBusy(true);
    const buySol = incumbent ? (incumbent.buy_amount_lamports / 1e9) : config.buyAmountSol;
    const engineDraft: EngineRuleDraft = {
      fingerprint_id: config.fingerprintId,
      params: champ.params,
      buy_amount_sol: buySol,
      max_concurrent_tokens: incumbent?.max_concurrent_tokens ?? 0,
      max_total_tokens: incumbent?.max_total_tokens ?? 0,
      trade_mode: 'paper',
    };
    const editorDraft: RuleEditorDraft = {
      rule_name: 'rule-search champion',
      fingerprint_id: config.fingerprintId,
      trade_mode: 'paper',
      buy_amount_lamports: solToLamports(buySol) ?? 1_000_000_000,
      max_concurrent_tokens: incumbent?.max_concurrent_tokens ?? 0,
      max_total_tokens: incumbent?.max_total_tokens ?? 0,
      params: champ.params,
      tags: ['src:rule-search'],
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

  const verdict = result ? VERDICT[result.verdict] : null;

  return (
    <div className="pt-2">
      <PageHeader
        title="Rule search"
        description="One fingerprint + one range. The registry fills roles from this cohort's cuts — you do not pick metrics. Board: champion vs buy-everything vs your saved rule."
      />

      <div className="mb-4 flex flex-wrap items-end gap-3">
        <Field
          label="Created range · UTC"
          tip={{
            title: 'Created range',
            body: 'Tokens created in this UTC window. Empty = all history this fingerprint has matched. The search scores that cohort only.',
          }}
        >
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
          label="Buy SOL"
          tip={TIPS.buySol}
          value={config.buyAmountSol}
          min={0.01}
          onChange={(v) => set('buyAmountSol', v)}
          width="w-[90px]"
          step={0.1}
        />
        <label className="flex items-center gap-1.5 text-xs text-text-dim">
          <span className="flex items-center gap-1">
            Fill
            <InfoTooltip {...EXECUTION_MODEL_HELP.fillModel} />
          </span>
          <Select
            fieldSize="sm"
            value={config.fillModel}
            onChange={(e) => set('fillModel', e.target.value as FillModelId)}
            className="w-36"
            title={FILL_MODELS.find((m) => m.id === config.fillModel)?.hint}
          >
            {FILL_MODELS.map((m) => (
              <option key={m.id} value={m.id} title={m.hint}>
                {m.label}
              </option>
            ))}
          </Select>
        </label>
        <label className="flex items-center gap-1.5 text-xs text-text-dim">
          <span className="flex items-center gap-1">
            Cost
            <InfoTooltip {...EXECUTION_MODEL_HELP.costModel} />
          </span>
          <Select
            fieldSize="sm"
            value={config.costModel}
            onChange={(e) => set('costModel', e.target.value as CostModelId)}
            className="w-36"
            title={COST_MODELS.find((m) => m.id === config.costModel)?.hint}
          >
            {COST_MODELS.map((m) => (
              <option key={m.id} value={m.id} title={m.hint}>
                {m.label}
              </option>
            ))}
          </Select>
        </label>
        <label className="flex h-[34px] items-center gap-1.5 text-sm text-text-mid">
          <Checkbox
            checked={config.skipDuplicateIdentity}
            onChange={(e) => set('skipDuplicateIdentity', e.target.checked)}
          />
          <LabelTip
            tip={{
              title: 'Copycat guard',
              body: 'Skip a second token with the same creator identity inside the window. ON for the empty-entry vs champion verdict (this job\'s default).',
            }}
          >
            copycat
          </LabelTip>
        </label>
        <IconButton
          variant="primary"
          size="lg"
          onClick={() => void handleRun()}
          disabled={running}
          label={running ? 'Running…' : 'Search'}
          title={running ? 'Running…' : 'Search'}
        >
          {running ? <SpinnerIcon /> : <PlayIcon />}
        </IconButton>
      </div>

      <div className="mb-4 border-t border-white/10 pt-3">
        <FingerprintScopeControl
          fingerprints={fingerprints}
          value={config.fingerprintId}
          onChange={(id) => {
            set('fingerprintId', id || null);
            set('incumbentRuleId', null);
          }}
          tip={{
            title: 'Fingerprint (required)',
            body: 'Search only tokens the engine matches against this fingerprint. Metrics, windows, and thresholds come from this range\'s cuts — not from another rule.',
          }}
          label="Fingerprint"
          emptyOptionLabel="Pick a fingerprint"
          scopedDescription="The search loads this fingerprint's matched tokens and fills roles from the registry."
          manualHint="A fingerprint is required — this job is one habit window, not a corpus-wide grid."
          matchedCount={fpMatches.count}
          matchedCountLoading={fpMatches.countLoading}
          onViewMatches={fpMatches.openMatches}
          onRequestMatchCount={fpMatches.ensureCount}
        />
        {fpMatches.matchesModal}
      </div>

      <div className="mb-4 max-w-md">
        <Field label="Incumbent (compare only)" tip={TIPS.incumbent}>
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
        {incumbent && (
          <p className="mt-1 text-[11px] text-text-dim">
            Buy size and caps come from <span className="text-text-mid">{incumbent.rule_name}</span>
            {' '}({(incumbent.buy_amount_lamports / 1e9).toFixed(2)} SOL). The incumbent is never a seed.
          </p>
        )}
      </div>

      {error && (
        <div className="mb-4">
          <InlineAlert variant="error">{error}</InlineAlert>
        </div>
      )}

      {result && verdict && (
        <div className="space-y-4">
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant={verdict.variant} size="md" title={verdict.help}>
              {verdict.label}
            </Badge>
            <LabelTip tip={TIPS.nMatched}>
              <span className="text-xs text-text-dim">
                {result.n_matched} matched tokens · {result.n_combos} combos scored
              </span>
            </LabelTip>
            {result.archive_replay_disagree && (
              <Badge
                variant="warning"
                size="sm"
                title="Fast walk and replay disagreed. The board uses replay — champion is the replay winner, not a shorter prefix of the fast ranking."
              >
                archive ≠ replay — board uses replay
              </Badge>
            )}
          </div>
          <p className="text-xs text-text-mid">{verdict.help}</p>
          {result.diagnostics.map((d) => (
            <p key={d} className="text-xs text-text-dim">
              {d}
            </p>
          ))}

          <p className="rounded-md border border-white/8 bg-white/2 px-3 py-2 text-[11px] leading-snug text-text-mid">
            <LabelTip tip={TIPS.solPair}>
              <span className="font-medium text-text">SOL pair</span>
            </LabelTip>
            {' — '}
            <span className="text-text">left = authority</span>
            {' (this run\'s Fill; ranks champion, verdict, archive). '}
            <span className="text-text">right = first-in-window</span>
            {' (same trades, next print after the signal). A gap is fill sensitivity, not a second ranking.'}
          </p>

          <div className="grid gap-3 sm:grid-cols-3">
            <ScoreCard
              title="Champion"
              blurb="Best selector. Ranking uses authority SOL."
              tip={TIPS.champion}
              emptyHint="No paying selector on this range."
              row={result.champion}
              actions={
                result.champion ? (
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    <button
                      type="button"
                      className="rounded border border-white/20 px-2 py-0.5 text-[11px] text-text-mid transition hover:bg-white/5"
                      onClick={() => result.champion && promote(result.champion, 'champion')}
                      title="Save as an inactive paper rule"
                    >
                      Promote…
                    </button>
                    <IconButton
                      variant="ghost"
                      size="sm"
                      onClick={() => void simulateChampion()}
                      disabled={simBusy}
                      label={simBusy ? 'Simulating…' : 'Simulate'}
                      title="Full Simulate of the unsaved champion — same range, Fill, cost, copycat as this form"
                    >
                      {simBusy ? <SpinnerIcon /> : <SimulateIcon />}
                    </IconButton>
                  </div>
                ) : null
              }
            />
            <ScoreCard
              title="Empty-entry"
              blurb="Buy everything that matched. Same exit bag as the champion."
              tip={TIPS.emptyEntry}
              emptyHint="No empty-entry score on this run."
              row={result.empty_entry}
            />
            <ScoreCard
              title="Incumbent"
              blurb="Your saved rule on this range. Compare only — never a seed."
              tip={TIPS.incumbentCol}
              emptyHint="No incumbent on this run. Pick a saved rule above to compare."
              row={result.incumbent}
            />
          </div>

          {result.champion && (
            <div>
              <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-text-mid">
                Champion params
              </h2>
              <p className="mb-2 text-[11px] text-text-dim">
                Clauses the search landed on. Promote writes these; Simulate runs them unsaved.
              </p>
              {ruleParamsCell(result.champion.params)}
            </div>
          )}

          {result.archive.length > 0 && (
            <div>
              <h2 className="mb-1 text-xs font-semibold uppercase tracking-wide text-text-mid">
                <LabelTip tip={TIPS.archive}>
                  Archive
                  <span className="ml-1.5 font-normal normal-case tracking-normal text-text-dim">
                    ({result.archive.length} · ranked by authority SOL)
                  </span>
                </LabelTip>
              </h2>
              <div className="overflow-x-auto rounded-md border border-white/8">
                <table className="w-full text-left text-xs">
                  <thead className="text-[10px] uppercase tracking-wide text-text-dim">
                    <tr>
                      <th className="px-2 py-1.5">Params</th>
                      <th className="px-2 py-1.5 text-right">
                        <LabelTip tip={TIPS.nClosed} className="justify-end">Closed</LabelTip>
                      </th>
                      <th className="px-2 py-1.5 text-right">
                        <LabelTip tip={TIPS.solPair} className="justify-end">Auth / first ◎</LabelTip>
                      </th>
                      <th className="px-2 py-1.5 text-right">
                        <LabelTip tip={TIPS.pf} className="justify-end">PF</LabelTip>
                      </th>
                      <th className="px-2 py-1.5 text-right">
                        <LabelTip tip={TIPS.enterPct} className="justify-end">Enter%</LabelTip>
                      </th>
                      <th className="px-2 py-1.5" />
                    </tr>
                  </thead>
                  <tbody>
                    {result.archive.map((row, i) => (
                      <tr key={i} className="border-t border-white/6">
                        <td className="px-2 py-1.5">{ruleParamsCell(row.params)}</td>
                        <td className="px-2 py-1.5 text-right font-mono">{row.n_closed}</td>
                        <td className="px-2 py-1.5 text-right font-mono">
                          <SolCell row={row} />
                        </td>
                        <td className="px-2 py-1.5 text-right font-mono">{fmt(row.profit_factor)}</td>
                        <td className="px-2 py-1.5 text-right font-mono">{pct(row.enter_pct)}</td>
                        <td className="px-2 py-1.5">
                          <button
                            type="button"
                            className="rounded border border-white/20 px-2 py-0.5 text-[11px] text-text-mid transition hover:bg-white/5"
                            onClick={() => promote(row, `archive ${i + 1}`)}
                          >
                            Promote…
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {simRunId && simDraft && <DryRunDetail runId={simRunId} draft={simDraft} />}
        </div>
      )}

      <PromoteRuleModal
        draft={promoteDraft}
        onClose={() => setPromoteDraft(null)}
        sourceTag="src:rule-search"
      />
    </div>
  );
}

function ScoreCard({
  title,
  blurb,
  tip,
  emptyHint,
  row,
  actions,
}: {
  title: string;
  blurb: string;
  tip: HelpTip;
  emptyHint: string;
  row: RuleSearchScored | null;
  actions?: ReactNode;
}) {
  return (
    <div className="rounded-lg border border-white/8 bg-white/2 p-3">
      <h3 className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
        <LabelTip tip={tip}>{title}</LabelTip>
      </h3>
      <p className="mb-2 text-[11px] leading-snug text-text-dim">{blurb}</p>
      {!row ? (
        <p className="text-xs text-text-dim">{emptyHint}</p>
      ) : (
        <>
          <SolCell row={row} variant="card" />
          <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-text-dim">
            <Stat tip={TIPS.nClosed} label="closed" value={String(row.n_closed)} />
            <Stat tip={TIPS.pf} label="PF" value={fmt(row.profit_factor)} />
            <Stat tip={TIPS.enterPct} label="enter" value={pct(row.enter_pct)} />
            {row.enter_pct_unguarded != null && row.enter_pct_unguarded !== row.enter_pct && (
              <Stat tip={TIPS.unguarded} label="unguarded" value={pct(row.enter_pct_unguarded)} />
            )}
          </div>
          {actions}
        </>
      )}
    </div>
  );
}

function Stat({ tip, label, value }: { tip: HelpTip; label: string; value: string }) {
  return (
    <span className="inline-flex items-baseline gap-1" title={`${tip.title}: ${tip.body}`}>
      <span className="text-text-dim/80">{label}</span>
      <span className="font-mono text-text-mid">{value}</span>
    </span>
  );
}

function SolCell({
  row,
  variant = 'inline',
}: {
  row: RuleSearchScored;
  variant?: 'inline' | 'card';
}) {
  const auth = fmt(row.total_pnl_sol, 3);
  const opti =
    row.total_pnl_sol_optimistic != null ? fmt(row.total_pnl_sol_optimistic, 3) : null;
  const spread = opti != null && opti !== auth;

  if (variant === 'card') {
    return (
      <div className="grid grid-cols-2 gap-2">
        <div>
          <p className="font-mono text-lg leading-tight text-text">{auth} ◎</p>
          <p className="text-[10px] leading-tight text-text-dim">authority · ranks</p>
        </div>
        <div>
          {spread ? (
            <>
              <p className="font-mono text-lg leading-tight text-text-mid">{opti} ◎</p>
              <p className="text-[10px] leading-tight text-text-dim">first-in-window</p>
            </>
          ) : (
            <p className="pt-1 text-[10px] leading-tight text-text-dim">
              first-in-window matches
            </p>
          )}
        </div>
      </div>
    );
  }

  return (
    <span className="inline-flex flex-col items-end leading-tight">
      <span>
        {auth}
        {spread && <span className="text-text-dim"> / {opti}</span>}
      </span>
      {spread && (
        <span className="text-[9px] font-sans normal-case tracking-normal text-text-dim">
          auth / first
        </span>
      )}
    </span>
  );
}

function Field({
  label,
  tip,
  children,
}: {
  label: string;
  tip?: HelpTip;
  children: ReactNode;
}) {
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
  label, tip, value, min, max, onChange, width, step,
}: {
  label: string; tip?: HelpTip; value: number; min?: number; max?: number; step?: number;
  onChange: (v: number) => void; width: string;
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
