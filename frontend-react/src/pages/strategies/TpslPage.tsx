import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { DataTable } from 'components/table/DataTable';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { InlineAlert } from 'components/ui/Modal';
import {
  buildCreatePayload,
  buildUpdatePayload,
  emptyForm,
  formFromRule,
  RuleFormModal,
  type RuleFormData,
} from 'components/tpsl/RuleFormModal';
import { ruleColumns } from 'components/tpsl/ruleColumns';
import { SimSummaryCard } from 'components/tpsl/SimSummaryCard';
import {
  matchedColumns,
  positionColumns,
  simColumns,
} from 'components/tpsl/tableColumns';
import { fmtTime } from 'components/tpsl/utils';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import {
  createTpslRule,
  deleteTpslRule,
  fetchMatchedTokens,
  fetchPaperResult,
  fetchRulePositions,
  fetchTpslRules,
  simulateTpslRule,
  updateTpslRule,
} from 'services/api';
import { connectPaperTestStream } from 'services/sse';
import { POLL_INTERVAL_MS } from 'services/config';
import type {
  PaperResultResponse,
  PaperTestFinishedEvent,
  RulePositionRecord,
  RuleRecord,
  SimulatedTokenResult,
} from 'types';
import { cn } from 'lib/cn';

function SectionDivider() {
  return <div role="separator" className="my-6 border-t border-white/6" />;
}

/** Indeterminate "trickle" progress bar for a simulation run. The simulate
 *  endpoint returns its result in one shot with no streaming progress, so we
 *  ease toward ~90% while the request is in flight rather than reporting real
 *  percentages; the bar unmounts when the result lands. */
function SimProgressBar() {
  const [percent, setPercent] = useState(8);
  useEffect(() => {
    const id = setInterval(() => {
      setPercent((p) => (p >= 90 ? p : p + (90 - p) * 0.12));
    }, 200);
    return () => clearInterval(id);
  }, []);
  return (
    <div className="mt-4">
      <div className="mb-2 flex items-center justify-between gap-2">
        <span className="text-[11px] font-bold uppercase tracking-widest text-primary">
          Running Simulation
        </span>
        <span className="font-mono text-[11px] text-text-dim">{Math.round(percent)}%</span>
      </div>
      <div className="h-2 overflow-hidden rounded-full bg-white/6">
        <div
          className="h-full animate-pulse rounded-full bg-primary transition-[width] duration-300"
          style={{ width: `${percent}%` }}
        />
      </div>
    </div>
  );
}

/** Heading for a section: a colored marker bar + title + optional count badge,
 *  subtitle, and right-aligned actions. Reused across the page so every section
 *  reads at a glance — content sits directly below it, with no surrounding card
 *  chrome, so only the real tables look like tables. */
function SectionHeading({
  title,
  count,
  marker = 'bg-primary',
  badge = 'primary',
  badgeClass,
  size = 'h3',
  subtitle,
  action,
}: {
  title: string;
  count?: number;
  marker?: string;
  badge?: BadgeVariant;
  badgeClass?: string;
  size?: 'h2' | 'h3';
  subtitle?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="mb-3.5 flex items-center gap-2.5">
      <span className={cn('w-1 rounded-full', size === 'h2' ? 'h-5' : 'h-4', marker)} />
      {size === 'h2' ? (
        <h2 className="text-base font-bold text-primary">{title}</h2>
      ) : (
        <h3 className="text-sm font-bold text-text">{title}</h3>
      )}
      {count != null && (
        <Badge variant={badge} size="sm" className={cn('font-mono font-normal', badgeClass)}>
          {count}
        </Badge>
      )}
      {subtitle && <span className="truncate font-mono text-[11px] text-text-dim">{subtitle}</span>}
      {action && (
        <>
          <span className="flex-1" />
          {action}
        </>
      )}
    </div>
  );
}

/** Renders the latest paper-test run for a rule: run-status header, the shared
 *  summary card, and the per-token table (reusing the simulation column set). */
function PaperResultSection({
  data,
  price,
  simCols,
  onClose,
}: {
  data: PaperResultResponse;
  price: ReturnType<typeof usePriceDisplay>;
  simCols: ReturnType<typeof simColumns>;
  onClose: () => void;
}) {
  const { run } = data;

  if (!run) {
    return (
      <section>
        <SectionHeading
          title="Paper Test"
          marker="bg-info"
          badge="info"
          subtitle={data.rule_name}
          action={
            <button
              type="button"
              onClick={onClose}
              className="text-text-dim transition hover:text-text"
            >
              ✕
            </button>
          }
        />
        <p className="text-text-dim">
          This rule hasn&apos;t been run in paper mode yet. Activate it to start a paper test.
        </p>
      </section>
    );
  }

  const statusVariant: BadgeVariant =
    run.status === 'Finished' ? 'primary' : run.status === 'Stopped' ? 'neutral' : 'info';

  return (
    <>
      <div className="mb-4 flex flex-wrap items-center gap-x-4 gap-y-2 text-[11px] text-text-dim">
        <Badge variant={statusVariant} size="sm" pill className="uppercase">
          {run.status === 'Running' ? '● Running' : run.status}
        </Badge>
        <span className="font-mono">Run #{run.run_seq}</span>
        <span>
          Cap:{' '}
          <span className="font-mono text-text">
            {run.max_total_tokens != null ? run.max_total_tokens : '∞'}
          </span>
        </span>
        <span>
          Started <span className="font-mono text-text">{fmtTime(run.started_at)}</span>
        </span>
        {run.finished_at && (
          <span>
            Ended <span className="font-mono text-text">{fmtTime(run.finished_at)}</span>
          </span>
        )}
      </div>

      <SimSummaryCard
        title="Paper Test Results"
        ruleName={data.rule_name}
        tokens={data.tokens}
        price={price}
        onClose={onClose}
      />

      <section>
        <SectionHeading title="Paper Positions" count={data.tokens.length} subtitle={data.rule_name} />
        {data.tokens.length === 0 ? (
          <p className="text-text-dim">No positions recorded for this run yet.</p>
        ) : (
          <DataTable
            columns={simCols}
            rows={data.tokens}
            rowKey={(r) => r.mint}
            defaultPageSize={20}
            pageSizeOptions={[20, 50, 100]}
            searchable
            colFilters
            selectable={false}
          />
        )}
      </section>
    </>
  );
}

export function TpslPage() {
  const price = usePriceDisplay();

  const [rules, setRules] = useState<RuleRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [selectedRuleId, setSelectedRuleId] = useState<string | null>(null);
  const [positions, setPositions] = useState<RulePositionRecord[]>([]);
  const [positionsLoading, setPositionsLoading] = useState(false);
  const [positionsError, setPositionsError] = useState<string | null>(null);

  const [modalOpen, setModalOpen] = useState(false);
  const [editRule, setEditRule] = useState<RuleRecord | null>(null);
  const [form, setForm] = useState<RuleFormData>(emptyForm());
  const [formError, setFormError] = useState<string | null>(null);
  const [formLoading, setFormLoading] = useState(false);

  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [deleteLoading, setDeleteLoading] = useState(false);

  const [simResult, setSimResult] = useState<{
    ruleName: string;
    tokens: SimulatedTokenResult[];
  } | null>(null);
  const [simError, setSimError] = useState<string | null>(null);
  const [simLoading, setSimLoading] = useState(false);

  const [matchedResult, setMatchedResult] = useState<{
    ruleId: string;
    tokens: import('types').MatchedTokenRecord[];
  } | null>(null);
  const [matchedError, setMatchedError] = useState<string | null>(null);
  const [matchedLoading, setMatchedLoading] = useState(false);

  const [paperResult, setPaperResult] = useState<{
    ruleId: string;
    data: PaperResultResponse;
  } | null>(null);
  const [paperError, setPaperError] = useState<string | null>(null);
  const [paperLoading, setPaperLoading] = useState(false);
  // Transient banner shown when a paper test finishes (cap reached + all exited).
  const [paperNotice, setPaperNotice] = useState<PaperTestFinishedEvent | null>(null);
  // Mirror of the rule whose paper result is open, read by the SSE handler so it
  // can refresh that view (status → Finished) without re-subscribing the stream.
  const openPaperRuleId = useRef<string | null>(null);
  useEffect(() => {
    openPaperRuleId.current = paperResult?.ruleId ?? null;
  }, [paperResult]);

  const loadRules = useCallback(async (silent = false) => {
    if (!silent) setLoading(true);
    try {
      const data = await fetchTpslRules();
      setRules(data);
      setError(null);
    } catch (e) {
      if (!silent) setError(e instanceof Error ? e.message : 'Failed to load rules');
    } finally {
      if (!silent) setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadRules();
    const id = setInterval(() => loadRules(true), POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [loadRules]);

  useEffect(() => {
    if (!selectedRuleId) {
      setPositions([]);
      setPositionsError(null);
      return;
    }
    setPositionsLoading(true);
    fetchRulePositions(selectedRuleId)
      .then(setPositions)
      .catch((e) =>
        setPositionsError(e instanceof Error ? e.message : 'Failed to load positions'),
      )
      .finally(() => setPositionsLoading(false));

    const id = setInterval(() => {
      fetchRulePositions(selectedRuleId)
        .then(setPositions)
        .catch(() => {});
    }, POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [selectedRuleId]);

  // Live paper-test completion: when a run finishes (cap reached + all exited)
  // the backend auto-deactivates the rule and broadcasts `paper_test_finished`.
  // Show a banner, refresh the rule list (so it flips to Inactive), and refresh
  // the open paper-result view if it's the finished rule.
  useEffect(() => {
    const es = connectPaperTestStream((ev) => {
      setPaperNotice(ev);
      loadRules(true);
      if (openPaperRuleId.current === ev.rule_id) {
        fetchPaperResult(ev.rule_id)
          .then((data) => setPaperResult({ ruleId: ev.rule_id, data }))
          .catch(() => {});
      }
    });
    return () => es.close();
  }, [loadRules]);

  useEffect(() => {
    if (!paperNotice) return;
    const id = setTimeout(() => setPaperNotice(null), 9000);
    return () => clearTimeout(id);
  }, [paperNotice]);

  const handleToggleActive = useCallback(async (rule: RuleRecord) => {
    try {
      const updated = await updateTpslRule(rule.id, { is_active: !rule.is_active });
      setRules((prev) => prev.map((r) => (r.id === updated.id ? updated : r)));
    } catch {
      /* ignore */
    }
  }, []);

  const columns = useMemo(() => ruleColumns(handleToggleActive), [handleToggleActive]);
  const posCols = useMemo(() => positionColumns(price), [price]);
  const simCols = useMemo(() => simColumns(price), [price]);

  const openAdd = () => {
    setEditRule(null);
    setForm(emptyForm());
    setFormError(null);
    setModalOpen(true);
  };

  const openEdit = (rule: RuleRecord) => {
    setEditRule(rule);
    setForm(formFromRule(rule));
    setFormError(null);
    setModalOpen(true);
  };

  const handleSave = async (allowParams: boolean) => {
    setFormError(null);
    if (!form.ruleName.trim()) {
      setFormError('Rule name is required');
      return;
    }
    for (const [label, val] of [
      ['buy amount', form.buyAmount],
      ['take profit', form.takeProfit],
      ['stop loss', form.stopLoss],
    ] as const) {
      if (!val.trim() || Number.isNaN(parseFloat(val))) {
        setFormError(`Invalid ${label}`);
        return;
      }
    }

    setFormLoading(true);
    try {
      if (editRule) {
        const updated = await updateTpslRule(
          editRule.id,
          buildUpdatePayload(form, allowParams),
        );
        setRules((prev) => prev.map((r) => (r.id === updated.id ? updated : r)));
      } else {
        const created = await createTpslRule(buildCreatePayload(form));
        setRules((prev) => [...prev, created]);
      }
      setModalOpen(false);
    } catch (e) {
      setFormError(e instanceof Error ? e.message : 'Save failed');
    } finally {
      setFormLoading(false);
    }
  };

  const handleDelete = async (ruleId: string) => {
    setDeleteLoading(true);
    try {
      await deleteTpslRule(ruleId);
      setRules((prev) => prev.filter((r) => r.id !== ruleId));
      if (selectedRuleId === ruleId) setSelectedRuleId(null);
    } catch {
      /* ignore */
    } finally {
      setConfirmDeleteId(null);
      setDeleteLoading(false);
    }
  };

  const handleSimulate = async (rule: RuleRecord) => {
    setSimResult(null);
    setSimError(null);
    setSimLoading(true);
    try {
      const tokens = await simulateTpslRule(rule.id);
      setSimResult({ ruleName: rule.rule_name, tokens });
    } catch (e) {
      setSimError(e instanceof Error ? e.message : 'Simulation failed');
    } finally {
      setSimLoading(false);
    }
  };

  const handleMatched = async (rule: RuleRecord) => {
    if (matchedResult?.ruleId === rule.id) {
      setMatchedResult(null);
      return;
    }
    setMatchedResult(null);
    setMatchedError(null);
    setMatchedLoading(true);
    try {
      const tokens = await fetchMatchedTokens(rule.id);
      setMatchedResult({ ruleId: rule.id, tokens });
    } catch (e) {
      setMatchedError(e instanceof Error ? e.message : 'Failed to load matched tokens');
    } finally {
      setMatchedLoading(false);
    }
  };

  const handlePaperResult = async (rule: RuleRecord) => {
    // Toggle: a second click on the open rule closes the result.
    if (paperResult?.ruleId === rule.id) {
      setPaperResult(null);
      setPaperError(null);
      return;
    }
    setPaperResult(null);
    setPaperError(null);
    setPaperLoading(true);
    try {
      const data = await fetchPaperResult(rule.id);
      setPaperResult({ ruleId: rule.id, data });
    } catch (e) {
      setPaperError(e instanceof Error ? e.message : 'Failed to load paper result');
    } finally {
      setPaperLoading(false);
    }
  };

  const ruleActions = (rule: RuleRecord) => {
    if (confirmDeleteId === rule.id) {
      return (
        <div className="flex items-center justify-center gap-1">
          <span className="text-[11px] font-semibold text-red">Delete?</span>
          <Button variant="danger" size="xs" disabled={deleteLoading} onClick={() => handleDelete(rule.id)}>
            Yes
          </Button>
          <Button variant="ghost" size="xs" onClick={() => setConfirmDeleteId(null)}>
            No
          </Button>
        </div>
      );
    }
    const matchedActive = matchedResult?.ruleId === rule.id;
    return (
      <div className="flex items-center justify-center gap-1">
        <Button
          variant="ghost"
          size="xs"
          disabled={rule.is_active}
          onClick={() => openEdit(rule)}
          title={rule.is_active ? 'Cannot edit active rules' : 'Edit'}
        >
          Edit
        </Button>
        <Button
          variant="ghost"
          size="xs"
          disabled={rule.is_active}
          onClick={() => setConfirmDeleteId(rule.id)}
          title={rule.is_active ? 'Cannot delete active rules' : 'Delete'}
          className="text-red"
        >
          Del
        </Button>
        <Button
          variant="ghost"
          size="xs"
          disabled={simLoading}
          onClick={() => handleSimulate(rule)}
          className="text-primary"
          title="Simulate"
        >
          ▶
        </Button>
        <Button
          variant="ghost"
          size="xs"
          disabled={matchedLoading}
          onClick={() => handleMatched(rule)}
          className={cn(
            matchedActive && 'border-[#9370db]/45 bg-[#9370db]/8 text-[#9370db]',
          )}
          title="Matched tokens"
        >
          ⊞
        </Button>
        {rule.trade_mode === 'paper' && (
          <Button
            variant="ghost"
            size="xs"
            disabled={paperLoading}
            onClick={() => handlePaperResult(rule)}
            className={cn(
              'text-info',
              paperResult?.ruleId === rule.id && 'border-info/45 bg-info/8',
            )}
            title="Paper test result"
          >
            ▦
          </Button>
        )}
      </div>
    );
  };

  const matchedRuleName =
    matchedResult &&
    rules.find((r) => r.id === matchedResult.ruleId)?.rule_name;

  const selectedRuleName = selectedRuleId
    ? rules.find((r) => r.id === selectedRuleId)?.rule_name ?? null
    : null;

  return (
    <div>
      {paperNotice && (
        <div className="mb-4 flex items-center gap-3 rounded-lg border border-primary/40 bg-primary/10 px-4 py-3">
          <span className="text-base text-primary">✓</span>
          <div className="flex-1 text-sm text-text">
            <span className="font-bold text-primary">Paper test finished</span>
            <span className="text-text-dim"> — </span>
            <span className="font-mono">{paperNotice.rule_name}</span>
            <span className="text-text-dim">
              {' '}(run #{paperNotice.run_seq}, {paperNotice.tokens_traded} tokens). Rule
              deactivated.
            </span>
          </div>
          <button
            type="button"
            onClick={() => setPaperNotice(null)}
            className="text-text-dim transition hover:text-text"
          >
            ✕
          </button>
        </div>
      )}

      <SectionHeading
        size="h2"
        title="TPSL Strategies"
        count={!loading && !error ? rules.length : undefined}
        action={
          <Button variant="primary" onClick={openAdd}>
            + Add Rule
          </Button>
        }
      />

      {loading && <p className="text-text-dim">Loading rules…</p>}
      {error && <InlineAlert variant="error">{error}</InlineAlert>}

      {!loading && !error && (
        <DataTable
          columns={columns}
          rows={rules}
          rowKey={(r) => r.id}
          rowActions={ruleActions}
          selectedKey={selectedRuleId}
          onSelect={setSelectedRuleId}
          defaultPageSize={10}
          pageSizeOptions={[10, 25, 50]}
          searchable
          colFilters
          colToggle
          storageKey="tpsl_rules_cols"
          emptyMessage="No rules found"
        />
      )}

      {selectedRuleId && (
        <>
          <SectionDivider />
          <section>
            <SectionHeading
              title="Positions"
              marker="bg-info"
              badge="info"
              count={positionsLoading || positionsError ? undefined : positions.length}
              subtitle={selectedRuleName ?? undefined}
            />
            {positionsLoading && <p className="text-text-dim">Loading positions…</p>}
            {positionsError && <InlineAlert variant="error">{positionsError}</InlineAlert>}
            {!positionsLoading && !positionsError && (
              <DataTable
                columns={posCols}
                rows={positions}
                rowKey={(r) => r.id}
                defaultPageSize={20}
                pageSizeOptions={[20, 50, 100]}
                colFilters
                colToggle
                selectable={false}
                emptyMessage="No positions for this rule."
              />
            )}
          </section>
        </>
      )}

      {(matchedLoading || matchedError || matchedResult) && <SectionDivider />}
      {matchedLoading && <p className="text-text-dim">Loading matched tokens…</p>}
      {matchedError && <InlineAlert variant="error">{matchedError}</InlineAlert>}
      {matchedResult && !matchedLoading && (
        <section>
          <SectionHeading
            title="Matched Tokens"
            marker="bg-[#9370db]"
            badge="neutral"
            badgeClass="border-[#9370db]/40 bg-[#9370db]/12 text-[#9370db]"
            count={matchedResult.tokens.length}
            subtitle={matchedRuleName ?? undefined}
            action={
              <button
                type="button"
                onClick={() => setMatchedResult(null)}
                className="text-text-dim transition hover:text-text"
              >
                ✕
              </button>
            }
          />
          {matchedResult.tokens.length === 0 ? (
            <p className="text-text-dim">
              No tokens in the database match this rule&apos;s entry criteria.
            </p>
          ) : (
            <DataTable
              columns={matchedColumns}
              rows={matchedResult.tokens}
              rowKey={(r) => r.mint}
              defaultPageSize={20}
              pageSizeOptions={[20, 50, 100]}
              searchable
              colFilters
              selectable={false}
            />
          )}
        </section>
      )}

      {(simLoading || simError || simResult) && <SectionDivider />}
      {simLoading && <SimProgressBar />}
      {simError && <InlineAlert variant="error">{simError}</InlineAlert>}
      {simResult && !simLoading && (
        <>
          <SimSummaryCard
            ruleName={simResult.ruleName}
            tokens={simResult.tokens}
            price={price}
            onClose={() => {
              setSimResult(null);
              setSimError(null);
            }}
          />
          <section>
            <SectionHeading
              title="Simulated Tokens"
              count={simResult.tokens.length}
              subtitle={simResult.ruleName}
            />
            {simResult.tokens.length === 0 ? (
              <p className="text-text-dim">No tokens matched this rule&apos;s entry criteria.</p>
            ) : (
              <DataTable
                columns={simCols}
                rows={simResult.tokens}
                rowKey={(r) => r.mint}
                defaultPageSize={20}
                pageSizeOptions={[20, 50, 100]}
                searchable
                colFilters
                selectable={false}
              />
            )}
          </section>
        </>
      )}

      {(paperLoading || paperError || paperResult) && <SectionDivider />}
      {paperLoading && <p className="text-text-dim">Loading paper-test result…</p>}
      {paperError && <InlineAlert variant="error">{paperError}</InlineAlert>}
      {paperResult && !paperLoading && (
        <PaperResultSection
          data={paperResult.data}
          price={price}
          simCols={simCols}
          onClose={() => {
            setPaperResult(null);
            setPaperError(null);
          }}
        />
      )}

      <RuleFormModal
        open={modalOpen}
        editRule={editRule}
        loading={formLoading}
        error={formError}
        form={form}
        onChange={setForm}
        onClose={() => setModalOpen(false)}
        onSave={handleSave}
      />
    </div>
  );
}
