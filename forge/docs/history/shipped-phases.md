# forge - shipped-phase narrative

> **History.** What was built, phase by phase, as of the last full reconciliation
> (2026-07-14). Open work is in [../roadmap-plan.md](../roadmap-plan.md); design
> rationale is in [../decisions.md](../decisions.md).

Detailed narrative of what's shipped, kept here so `CLAUDE.md` stays a thin index.

Data-infrastructure **foundation complete** (7 phase commits): 5 crates, 2 bins,
migrations `0001` (Domains A–C) + `0002` (Domain D). §9 open decisions resolved (see the
ADR). **Phase-2 launcher:** create (`v1`/`v2` + dev-buy), keystore, `wallet-encrypt` CLI,
launch failure rollback, bundle leg composer (`planned` bundles),
`POST /api/bundles/{id}/execute` (Jito submit), **feed-based bundle-landing confirmation**
(migration `0003`; always-on watcher in `live/main.rs` checks leg signatures against
ingested `trades`, no RPC poll; `GET /api/bundles/{id}` for status). **Auto-submit** after
launch, multi-variant bundle legs, SOL/USD poller.

**Dev-buy ix variants** ([plans/launcher/dev-buy-variants.md](../plans/launcher/dev-buy-variants.md)):
the launch dev-buy selects any of the four curve-buy encodings (independent of the bundler
`buy_variant`), sharing the `build_curve_buy_core` SSOT with the co-buy legs; tokens-out
encodings (`buy`/`buy_v2`) require a slippage (validated). v2 dev-buy tx size not yet
real-SOL verified.

**Bundle tip sizing + drop re-bid** (migration `0003`): each leg's Jito tip is floored to
the live landed-tip auction (`trader.jito_tip_floor_lamports(level)`, split across legs,
only ever raising a persona draw), and the confirm watcher auto re-submits a `dropped`
bundle at an escalating tip (`bundles.submit_attempts` = the level; p95→p99→…) up to
`BUNDLE_MAX_RETRIES` (default 2). Phase-2b: ingest round-trip test + dep-partition CI +
a mainnet smoke checklist.

**Wallet pool** (plan finished & retired — `git show 7f0526f:docs/wallet-pool-plan.md`):
Phase 1 — migration `0004` replaces `managed_wallets.is_active` with a `status` lifecycle
(`generated`/`funded`/`reserved`/`used`/`retired`) + `funding_source`/`reserved_by_launch_id`/
`reserved_at`/`balance_lamports`; `launcher::wallet_pool` adds batch generation, balance
poller, reservation TTL sweep; `ManagedWalletRepo::claim_funded` (`FOR UPDATE SKIP LOCKED`)
+ `mark_used`. Phase 2 — `GET /api/wallet_pool` + `POST /api/wallet_pool/generate`; Wallet
Pool page. Phase 3 — launch flow consumes the pool (dev-wallet dropdown filters to `funded`,
bundler legs claimed via `claim_funded`, token identity is a single `metadata_template_id`;
`mark_used` moved to `launcher::confirm`'s landed/dropped/partial outcomes; bundle planning
moved outside `execute_launch`'s failure-reset scope). Phase 4 — `launcher::spawn_dust_sweep`
(hourly sweep `used` → `treasury`, then retire), `launcher::run_backup` (opt-in
`WALLET_BACKUP_DIR`), `wallet-verify` CLI.

**Frontend** rebuilt as a React-Router + RTK-Query + Tailwind operator dashboard (`src/app`
shell, `src/shared` ui-kit/store/lib, `src/features/*` pages); old single `App.tsx`/`api.ts`
gone. `GET /api/launches` (paged enriched `LaunchListRow`) backs the launched-tokens list.

**Operator wallet→wallet transfer** ([plans/wallet/wallet-management.md](../plans/wallet/wallet-management.md)):
move SOL between any two managed wallets from the dashboard (`launcher::transfer_between_wallets`,
`POST /api/wallet_pool/transfer`); routes through the `plan_exec::execute_transfer` SSOT;
source signs + pays fee; `funding`/`reserved`/`retired` source rejected.

**Next:** Phase 3 live trading. Wallet-pool Phase 5+ (automated multi-hop funding,
fingerprint picker UI, KMS KEK) deferred — see the Phase 5+ Growth section above.

---

