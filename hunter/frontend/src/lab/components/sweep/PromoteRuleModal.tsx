import { useMemo, useState } from 'react';
import { Modal } from 'components/ui/Modal';
import { RuleEditor, type RuleEditorDraft } from 'components/strategy/RuleEditor';
import { apiErrorMessage } from 'store/baseApi';
import {
  useCreateStrategyRuleMutation,
  useGetStrategyRulesQuery,
} from 'store/sharedEndpoints';
import {
  duplicateRuleMessage,
  findIdenticalRule,
  ruleIdentityOf,
} from 'lib/strategy/matchRuleIdentity';
import type { PromotedRuleDraft, StrategyRule } from 'lib/strategy/types';
import { DryRunPanel } from '@lab/components/strategy/DryRunPanel';

/**
 * The [Promote…] destination (sweep redesign FE5.3): opens the shared rule
 * editor pre-filled from the backend promote response (`POST …/promote` →
 * fingerprint find-or-created + a ready-to-save draft), with the lab dry-run
 * panel injected so the promoted combo can be replayed before saving. Save
 * creates a new rule (id-less draft) — refused when an identity-identical rule
 * already exists (same gate as Rules / Simulate create). Rendered on the sweep
 * page as a modal — the shared `RulesView` uses a local-state modal too, so this
 * matches the existing pattern (no route/query-string draft channel needed).
 */
export function PromoteRuleModal({
  draft,
  onClose,
  sourceTag = 'src:sweep',
}: {
  draft: PromotedRuleDraft | null;
  onClose: () => void;
  /** Provenance tag stamped on the new rule. */
  sourceTag?: string;
}) {
  const [createRule, { isLoading: creating }] = useCreateStrategyRuleMutation();
  const { data: existingRules = [] } = useGetStrategyRulesQuery();
  const [err, setErr] = useState<string | null>(null);

  // Synthesize an id-less `StrategyRule` for `RuleEditor.initial` — the same
  // shape the shared editor's "duplicate" flow feeds create mode.
  const initial: StrategyRule | undefined = useMemo(
    () =>
      draft
        ? {
            id: '',
            rule_name: draft.rule_name,
            fingerprint_id: draft.fingerprint_id,
            trade_mode: draft.trade_mode,
            is_active: false,
            is_enabled: true,
            buy_amount_lamports: draft.buy_amount_lamports,
            max_concurrent_tokens: draft.max_concurrent_tokens,
            max_total_tokens: draft.max_total_tokens,
            params: draft.params,
            // Seed provenance: a promoted combo is worth being able to find as a
            // group later. Editable before save.
            tags: [sourceTag],
            created_at: '',
            updated_at: '',
          }
        : undefined,
    [draft, sourceTag],
  );

  const submit = async (d: RuleEditorDraft) => {
    setErr(null);
    const dup = findIdenticalRule(existingRules, ruleIdentityOf(d));
    if (dup) {
      setErr(duplicateRuleMessage(dup));
      return;
    }
    try {
      await createRule(d).unwrap();
      onClose();
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'Save failed');
    }
  };

  return (
    <Modal title="Promote combo → rule" open={draft !== null} onClose={onClose} size="xl">
      {initial && (
        <RuleEditor
          initial={initial}
          onSubmit={submit}
          onCancel={onClose}
          submitting={creating}
          error={err}
          renderDryRun={(d, canRun) => <DryRunPanel draft={d} canRun={canRun} />}
        />
      )}
    </Modal>
  );
}
