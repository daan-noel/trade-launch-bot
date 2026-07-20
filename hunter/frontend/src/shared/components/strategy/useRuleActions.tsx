import { useState, type ReactNode } from 'react';

import { Modal } from 'components/ui/Modal';
import { RuleEditor, type RuleEditorDraft } from './RuleEditor';
import { apiErrorMessage } from 'store/baseApi';
import {
  useCreateStrategyRuleMutation,
  useUpdateStrategyRuleMutation,
  useDeleteStrategyRuleMutation,
  useGetStrategyRulesQuery,
} from 'store/sharedEndpoints';
import {
  duplicateRuleMessage,
  findIdenticalRule,
  ruleIdentityOf,
} from 'lib/strategy/matchRuleIdentity';
import type { StrategyRule } from 'lib/strategy/types';

export interface UseRuleActionsOptions {
  /** Lab-only dry-run render-prop forwarded to the editor (see RulesView FE3). */
  renderDryRun?: (draft: RuleEditorDraft | null, canRun: boolean) => ReactNode;
}

export interface RuleActions {
  /** The edit/new/duplicate editor modal — render once per host page. */
  editorNode: ReactNode;
  /** Last create/update/delete error, or null. */
  err: string | null;
  /** Open the editor on a blank rule. */
  openNew: () => void;
  /** Open the editor to edit `r`. */
  edit: (r: StrategyRule) => void;
  /** Open the editor pre-filled from `r` as a new (id-less, idle) draft. */
  duplicate: (r: StrategyRule) => void;
  /** Confirm + delete `r`. */
  remove: (r: StrategyRule) => Promise<void>;
  /** A create/update is in flight (drives the editor submit spinner). */
  saving: boolean;
}

/**
 * Shared rule create/edit/duplicate/delete behaviour + the editor modal, so both
 * the Rules page (RulesView) and the Simulate page render the same Actions column
 * (Edit / Duplicate / Delete) over the one generic `strategy_rules` CRUD without
 * duplicating the mutation wiring. Execute-column actions (activate/pause/stop,
 * simulate) stay on each host page — they differ by app.
 */
export function useRuleActions({ renderDryRun }: UseRuleActionsOptions = {}): RuleActions {
  const [createRule, { isLoading: creating }] = useCreateStrategyRuleMutation();
  const [updateRule, { isLoading: updating }] = useUpdateStrategyRuleMutation();
  const [deleteRule] = useDeleteStrategyRuleMutation();
  // Cache for early duplicate detect — backend Duplicate gate remains authoritative.
  const { data: existingRules = [] } = useGetStrategyRulesQuery();

  const [editing, setEditing] = useState<StrategyRule | 'new' | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const submit = async (draft: RuleEditorDraft) => {
    setErr(null);
    const editingId =
      editing && editing !== 'new' && editing.id ? editing.id : undefined;
    const dup = findIdenticalRule(existingRules, ruleIdentityOf(draft), editingId);
    if (dup) {
      setErr(duplicateRuleMessage(dup));
      return;
    }
    try {
      if (editingId) {
        await updateRule({
          id: editingId,
          body: {
            rule_name: draft.rule_name,
            trade_mode: draft.trade_mode,
            buy_amount_lamports: draft.buy_amount_lamports,
            max_concurrent_tokens: draft.max_concurrent_tokens,
            max_total_tokens: draft.max_total_tokens,
            params: draft.params,
          },
        }).unwrap();
      } else {
        await createRule(draft).unwrap();
      }
      setEditing(null);
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'Save failed');
    }
  };

  const remove = async (r: StrategyRule) => {
    if (!window.confirm(`Delete rule "${r.rule_name}"?`)) return;
    setErr(null);
    try {
      await deleteRule(r.id).unwrap();
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'Delete failed');
    }
  };

  const editorNode = (
    <Modal
      title={editing && editing !== 'new' && editing.id ? 'Edit rule' : 'New rule'}
      open={editing !== null}
      onClose={() => setEditing(null)}
      size="xl"
    >
      {editing !== null && (
        <RuleEditor
          initial={editing === 'new' ? undefined : editing.id ? editing : { ...editing }}
          onSubmit={submit}
          onCancel={() => setEditing(null)}
          submitting={creating || updating}
          error={err}
          renderDryRun={renderDryRun}
        />
      )}
    </Modal>
  );

  return {
    editorNode,
    err,
    saving: creating || updating,
    openNew: () => setEditing('new'),
    edit: (r) => setEditing(r),
    duplicate: (r) =>
      setEditing({
        ...r,
        id: '',
        rule_name: `${r.rule_name} copy`,
        is_active: false,
        is_enabled: true,
      }),
    remove,
  };
}
