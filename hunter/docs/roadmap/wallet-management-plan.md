# Wallet management: open work

The model in [wallet-management.md](../plans/wallets/wallet-management.md) holds: many wallets,
one primary, a rule names a wallet or falls back to the primary, a move carries everything.
This file is what is missing for that model to be usable day to day.

## Gaps today

| # | Gap | Effect |
| --- | --- | --- |
| G1 | The env wallet (`WALLET_PRIVATE_KEY` + `NONCE_ACCOUNTS`) is adopted as `main` but has no registry row and no key file | It cannot be moved, provisioned, or archived. Importing the same key creates a row with zero nonces, and its trader fails init with "At least one nonce account is required". |
| G2 | Import takes a key only | Nonce accounts the wallet already owns are not attached. |
| G3 | No readiness | A wallet with no SOL, no nonces, or a failed trader init shows `active`, and a rule can be assigned to it. Entries on that rule then fail closed in the log only. |
| G4 | Per-position manual actions use `DeployState.trader` (the boot primary) | `positions.rs` Re-sell / Verify read the bag under the primary's pubkey. A position bought by another wallet reads as "bag gone" there and books closed. |
| G5 | Rent reclaim, cashback claim, manual buy/sell, My Wallet, the boot reconcile read one wallet | The other wallets' rent, cashback, and stray tokens are invisible. |
| G6 | `GET /wallets` calls `get_sol_balance` per wallet on every 15 s poll | Helius spend that grows with the wallet count. The push feed already keeps a cached balance. |
| G7 | A move advances inside the HTTP request and stops at `inflight` | The operator must press Resume until it finishes. A restart mid-move needs a manual Resume too. |
| G8 | No SOL transfer between wallets, no rename, no archive of an idle empty wallet, no restore | Spreading SOL across rule wallets is done outside the bot. |
| G9 | New nonce accounts are watched on the push feed only after a restart | Until then sells on the new nonces rely on the nonce poll. |

## Phase 1: correctness (before more wallets trade)

1. **Bring the env wallet in (G1).** One action, shown as a banner while `main` has no row:
   write `main.enc` from `WALLET_PRIVATE_KEY`, insert the row as primary, insert each
   `NONCE_ACCOUNTS` pubkey into `trader_nonces`. One `getMultipleAccounts` checks every nonce's
   authority before it is attached. After this the env key is needed only by `probe`.
2. **Attach existing nonces (G2).** The import form takes an optional nonce list. A wallet row
   also gets "Attach nonces". The same single `getMultipleAccounts` check rejects a nonce whose
   authority is another key. The trader is rebuilt once at the end.
3. **Readiness (G3).** `Slot` keeps the init error. Each wallet reports one state:
   `ready`, `needs SOL`, `needs nonces`, `init failed: <reason>`, `key not on this box`.
   Assigning a rule to a wallet that is not `ready` is refused. Setting a non-ready wallet as
   primary is refused. Provision and attach rebuild the trader, so the state clears on its own.
4. **Every position action signs with the position's wallet (G4).** `positions.rs`, the orphan
   exit, and the manual sell look the trader up by `strategy_positions` wallet pubkey through
   `WalletRuntime::trader`, never `DeployState.trader`. A no-DB test: a position bought by wallet B
   is never read under wallet A.
5. **Cached balance on the list (G6).** The list reads `cached_balance_lamports`. One batched
   `getMultipleAccounts` fills only the wallets with no cached value.

## Phase 2: the page

Layout, top to bottom:

- **Summary strip.** Total SOL across wallets, wallets ready / total, primary name, open move.
- **Banner** when the env wallet is not in the registry (phase 1 step 1).
- **Tabs.** In use (primary, or named by at least one rule) / Idle / Archived. In use and Idle
  are derived, not stored: status stays `active | draining | archived`.
- **Wallet table.** Name with a primary star, readiness badge, SOL, nonces, rules (count, hover
  lists names), open positions, realized PnL 24 h. The row's one primary action is the fix for
  its readiness state ("Provision nonces", "Fund", "Retry init"). The rest sit in a menu.
- **Wallet drawer** (click a row):
  - Overview: pubkey with copy, balances, readiness checklist with a fix button per line.
  - Rules: every live rule with a checkbox. Checked = this wallet signs it. Unchecking returns
    the rule to the primary. Assignment can be managed from the wallet side, not only per rule.
  - Nonces: address, on-chain state, add / attach.
  - Activity: moves, transfers, provisions for this wallet.
- **Actions** (menu): Set primary, Rename, Transfer SOL to another managed wallet, Withdraw to
  an external address (type the wallet name to confirm), Archive (only when idle, no open
  positions, no tokens, no nonces), Restore an archived wallet.
- **Rules page.** The wallet column shows the name, or "primary (name)" for an unassigned rule,
  with the readiness dot, so a rule on a broken wallet is visible where rules are switched on.

## Phase 3: move everything

1. **Preview.** Before confirming, the modal lists exactly what moves: rules, the primary flag,
   token bags (mint, amount), nonce accounts, spendable SOL, and any position still in flight.
2. **Destination "new wallet".** The modal can create the destination in the same flow. It needs
   no nonce provisioning, since the source's nonces move.
3. **Server-driven.** The move runs as a background task that advances on its own, re-checks
   `inflight` on a timer, and resumes after a restart. The page shows the step timeline and polls
   faster while a move is open. Resume stays as the manual retry after an error.
4. **Watch new nonces live (G9).** Provision, attach, and a move push the new nonce pubkeys onto
   the account subscription without a restart.

## Phase 4: per-wallet surfaces (G5)

Rent reclaim, cashback claim, manual buy/sell, My Wallet, and the boot reconcile take a wallet
argument and default to the primary. The boot reconcile walks every loaded wallet, staggered so
it stays one RPC listing per wallet per boot.

## Open questions

- Withdraw to an external address: allowed, or only between managed wallets?
- Readiness "needs SOL": one global minimum, or the rule's buy size plus fees for each rule the
  wallet signs?
