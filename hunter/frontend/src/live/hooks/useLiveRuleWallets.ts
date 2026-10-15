import { useCallback, useMemo } from 'react';

import type { RuleWalletOption } from 'components/strategy/RuleEditor';
import type { RuleWalletCell } from 'components/strategy/RulesView';
import type { StrategyRule } from 'lib/strategy/types';
import {
  useAssignRuleWalletMutation,
  useGetTraderWalletsQuery,
  useGetWalletAssignmentsQuery,
  walletCanSign,
  type TraderWalletRow,
} from '@live/store/liveEndpoints';
import { READINESS_LABEL, READINESS_VARIANT, solText } from '@live/pages/wallets/walletFormat';

/** Primary first, then wallets that sign, low SOL, and the ones that cannot. */
function walletRank(w: TraderWalletRow): number {
  if (w.is_primary) return 0;
  if (w.readiness === 'ready') return 1;
  return walletCanSign(w.readiness) ? 2 : 3;
}

function walletOption(w: TraderWalletRow): RuleWalletOption {
  const canSign = walletCanSign(w.readiness);
  const sol = w.sol == null ? '? SOL' : `${solText(w.sol, 3)} SOL`;
  const warn = w.readiness === 'ready' ? '' : ` · ${canSign ? 'low SOL' : 'cannot sign'}`;
  return {
    pubkey: w.pubkey,
    name: w.name,
    label: `${w.name}${w.is_primary ? ' ★ pinned' : ''} · ${sol}${warn}`,
    isPrimary: w.is_primary,
    canSign,
    status: {
      variant: READINESS_VARIANT[w.readiness],
      label: READINESS_LABEL[w.readiness],
      title: w.init_error ?? w.readiness_reason,
    },
    detail: `${sol} · ${w.nonce_count} nonce${w.nonce_count === 1 ? '' : 's'}`,
  };
}

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
      .filter((w) => w.is_primary || walletCanSign(w.readiness) || assigned.has(w.pubkey))
      .sort((a, b) => walletRank(a) - walletRank(b) || a.name.localeCompare(b.name))
      .map(walletOption);
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
