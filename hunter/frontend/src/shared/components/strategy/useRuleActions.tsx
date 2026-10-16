import { useState, type ReactNode } from 'react';

import { Modal } from 'components/ui/Modal';
import { RuleEditor, type RuleEditorDraft, type RuleWalletOption } from './RuleEditor';
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
  /** Copy board: the editor offers the copy readings and stamps `copy: true`. */
  board?: 'rules' | 'copy';
  /** Live only. Active wallets for the editor dropdown. Lab omits this. */
  wallets?: RuleWalletOption[];
  /** Live only. The wallet a rule names, or null for the primary. */
  walletFor?: (ruleId: string) => string | null;
  /** Live only. Writes `rule_wallets` after the rule row saves. Lab never calls it. */
  assignWallet?: (ruleId: string, wallet: string | null) => Promise<void>;
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
export function useRuleActions({
  renderDryRun,
  board = 'rules',
  wallets,
  walletFor,
  assignWallet,
}: UseRuleActionsOptions = {}): RuleActions {
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
      const saved = editingId
        ? await updateRule({
            id: editingId,
            body: {
              rule_name: draft.rule_name,
              fingerprint_id: draft.fingerprint_id,
              trade_mode: draft.trade_mode,
              buy_amount_lamports: draft.buy_amount_lamports,
              max_concurrent_tokens: draft.max_concurrent_tokens,
              max_total_tokens: draft.max_total_tokens,
              params: draft.params,
              tags: draft.tags,
            },
          }).unwrap()
        : await createRule({
            rule_name: draft.rule_name,
            fingerprint_id: draft.fingerprint_id,
            trade_mode: draft.trade_mode,
            buy_amount_lamports: draft.buy_amount_lamports,
            max_concurrent_tokens: draft.max_concurrent_tokens,
            max_total_tokens: draft.max_total_tokens,
            params: draft.params,
            tags: draft.tags,
          }).unwrap();
      if (assignWallet && Object.prototype.hasOwnProperty.call(draft, 'wallet')) {
        await assignWallet(saved.id, draft.wallet ?? null);
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
      title={
        editing && editing !== 'new' && editing.id
          ? board === 'copy'
            ? 'Edit copy'
            : 'Edit rule'
          : board === 'copy'
            ? 'New copy'
            : 'New rule'
      }
      open={editing !== null}
      onClose={() => setEditing(null)}
      size="xxl"
    >
      {editing !== null && (
        <RuleEditor
          initial={editing === 'new' ? undefined : editing.id ? editing : { ...editing }}
          onSubmit={submit}
          onCancel={() => setEditing(null)}
          submitting={creating || updating}
          error={err}
          renderDryRun={renderDryRun}
          scope={board === 'copy' ? 'copy' : 'metric'}
          wallets={wallets}
          initialWallet={
            editing && editing !== 'new' && editing.id && walletFor ? walletFor(editing.id) : null
          }
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
    // Tags ride along with the spread — a copy of a rule belongs to the same
    // families as its original, and the editor can prune them before saving.
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
