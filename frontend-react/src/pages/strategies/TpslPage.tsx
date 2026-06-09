import { useCallback, useEffect, useMemo, useState } from 'react';
import { DataTable } from 'components/table/DataTable';
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
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import {
  createTpslRule,
  deleteTpslRule,
  fetchMatchedTokens,
  fetchRulePositions,
  fetchTpslRules,
  simulateTpslRule,
  updateTpslRule,
} from 'services/api';
import { POLL_INTERVAL_MS } from 'services/config';
import type { RulePositionRecord, RuleRecord, SimulatedTokenResult } from 'types';
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
    <div className="mt-4 rounded-lg border border-white/6 bg-white/2 p-4">
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
      </div>
    );
  };

  const matchedRuleName =
    matchedResult &&
    rules.find((r) => r.id === matchedResult.ruleId)?.rule_name;

  return (
    <div>
      {simResult && (
        <SimSummaryCard
          ruleName={simResult.ruleName}
          tokens={simResult.tokens}
          price={price}
          onClose={() => {
            setSimResult(null);
            setSimError(null);
          }}
        />
      )}

      <div className="mb-3.5 flex items-center justify-between">
        <h2 className="text-base font-bold text-primary">TPSL Strategies</h2>
        <Button variant="primary" onClick={openAdd}>
          + Add Rule
        </Button>
      </div>

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
        <div className="mt-4">
          {positionsLoading && (
            <p className="text-text-dim">Loading positions…</p>
          )}
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
        </div>
      )}

      {matchedLoading && (
        <p className="mt-4 text-text-dim">Loading matched tokens…</p>
      )}
      {matchedError && <InlineAlert variant="error">{matchedError}</InlineAlert>}
      {matchedResult && !matchedLoading && (
        <div className="mt-4 overflow-hidden rounded-[10px] border border-primary/20 bg-white/2">
          <div className="flex items-center justify-between border-b border-[#9370db]/12 bg-[#9370db]/6 px-4 py-2.5">
            <span className="text-[13px] font-bold text-[#9370db]">
              Matched Tokens — {matchedRuleName}{' '}
              <span className="ml-2 rounded-full bg-[#9370db]/15 px-2 py-0.5 text-[10px]">
                {matchedResult.tokens.length}
              </span>
            </span>
            <button type="button" onClick={() => setMatchedResult(null)} className="text-text-dim">
              ✕
            </button>
          </div>
          {matchedResult.tokens.length === 0 ? (
            <p className="p-6 text-center text-text-dim">
              No tokens in the database match this rule&apos;s entry criteria.
            </p>
          ) : (
            <div className="p-2">
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
            </div>
          )}
        </div>
      )}

      {(simLoading || simError || simResult) && <SectionDivider />}
      {simLoading && <SimProgressBar />}
      {simError && <InlineAlert variant="error">{simError}</InlineAlert>}
      {simResult && !simLoading && (
        <div className="mt-4 overflow-hidden rounded-[10px] border border-primary/20 bg-white/2">
          <div className="flex items-center justify-between border-b border-primary/15 bg-primary/7 px-4 py-2.5">
            <span className="text-[13px] font-bold text-primary">
              Simulated Tokens — {simResult.ruleName}{' '}
              <span className="ml-2 rounded-full bg-primary/15 px-2 py-0.5 text-[10px]">
                {simResult.tokens.length}
              </span>
            </span>
          </div>
          {simResult.tokens.length === 0 ? (
            <p className="p-6 text-center text-text-dim">
              No tokens matched this rule&apos;s entry criteria.
            </p>
          ) : (
            <div className="p-2">
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
            </div>
          )}
        </div>
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
