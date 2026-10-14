import { useMemo } from 'react';

import {
  useAssignRuleWalletMutation,
  useGetTraderWalletsQuery,
  useGetWalletAssignmentsQuery,
} from '@live/store/liveEndpoints';

/** Active wallets and the rule assignment writer. Lab never mounts this. */
export function useLiveRuleWallets() {
  const { data: book } = useGetTraderWalletsQuery();
  const { data: assignments = [] } = useGetWalletAssignmentsQuery();
  const [assign] = useAssignRuleWalletMutation();
  const wallets = useMemo(
    () =>
      (book?.wallets ?? [])
        .filter((w) => w.status === 'active')
        .map((w) => ({ pubkey: w.pubkey, name: w.name })),
    [book],
  );
  const byRule = useMemo(() => {
    const map = new Map<string, string>();
    for (const row of assignments) map.set(row.rule_id, row.wallet);
    return map;
  }, [assignments]);
  return {
    wallets,
    walletFor: (ruleId: string) => byRule.get(ruleId) ?? null,
    assignWallet: async (ruleId: string, wallet: string | null) => {
      await assign({ ruleId, wallet }).unwrap();
    },
  };
}
