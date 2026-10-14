import { useCallback, useMemo } from 'react';

import type { RuleWalletCell } from 'components/strategy/RulesView';
import type { StrategyRule } from 'lib/strategy/types';
import {
  useAssignRuleWalletMutation,
  useGetTraderWalletsQuery,
  useGetWalletAssignmentsQuery,
  walletCanSign,
} from '@live/store/liveEndpoints';

/**
 * Wallets a rule may be assigned and the rule assignment writer. Lab never
 * mounts this. A wallet that cannot sign is offered only to the rule already on
 * it, labelled, so the editor shows the truth instead of a blank select.
 */
export function useLiveRuleWallets() {
  const { data: book } = useGetTraderWalletsQuery();
  const { data: assignments = [] } = useGetWalletAssignmentsQuery();
  const [assign] = useAssignRuleWalletMutation();
  const byRule = useMemo(() => {
    const map = new Map<string, string>();
    for (const row of assignments) map.set(row.rule_id, row.wallet);
    return map;
  }, [assignments]);
  const wallets = useMemo(() => {
    const assigned = new Set(byRule.values());
    return (book?.wallets ?? [])
      .filter((w) => w.status === 'active')
      .filter((w) => walletCanSign(w.readiness) || assigned.has(w.pubkey))
      .map((w) => ({
        pubkey: w.pubkey,
        name: walletCanSign(w.readiness) ? w.name : `${w.name} (cannot sign)`,
      }));
  }, [book, byRule]);
  const primary = useMemo(() => (book?.wallets ?? []).find((w) => w.is_primary) ?? null, [book]);
  const byPubkey = useMemo(
    () => new Map((book?.wallets ?? []).map((w) => [w.pubkey, w])),
    [book],
  );
  /** The wallet an entry on a rule signs with now: its own, else the primary. */
  const walletOf = useCallback(
    (rule: StrategyRule): RuleWalletCell | null => {
      const own = byRule.get(rule.id);
      const w = own ? byPubkey.get(own) : primary;
      if (!w) return null;
      return {
        name: w.name,
        viaPrimary: !own,
        canSign: walletCanSign(w.readiness),
        reason: w.readiness_reason,
      };
    },
    [byRule, byPubkey, primary],
  );
  return {
    wallets,
    walletFor: (ruleId: string) => byRule.get(ruleId) ?? null,
    walletOf,
    assignWallet: async (ruleId: string, wallet: string | null) => {
      await assign({ ruleId, wallet }).unwrap();
    },
  };
}
