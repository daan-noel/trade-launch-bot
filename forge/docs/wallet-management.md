# Wallet Management

How the managed wallet pool works: what wallets are for, the funding flow, and
the lifecycle each wallet moves through. Written to be read top-to-bottom by
anyone (no Rust needed), with pointers to the code for detail.

## Why a wallet pool exists

The platform launches tokens using **many wallets, each used once**. If the same
wallet showed up in launch after launch, all your launches would be visibly
linked on-chain. So the pool treats every wallet like a **prepaid burner card**:

> born empty → loaded with SOL → does **one** launch → emptied and discarded.

Wallets have a **role** (what job they do) and a **status** (where they are in
their life).

### Roles

| Role | Job | Fundable? |
| --- | --- | --- |
| `treasury` | Holds the master SOL. Funds all the others. | source, not funded |
| `dev` | The launch's creator/dev wallet. | yes |
| `bundler` | Buys in the launch bundle to seed volume. | yes |
| `trading` | Post-launch trading. | not by the pool funder |

The role vocabulary lives in
[`WalletRole`](../crates/platform-core/src/models/status.rs) (migration `0002`).

## The lifecycle (status)

Every wallet moves through these six stages in order:

```
generated → funding → funded → reserved → used → retired
 (empty)   (loading)  (ready)  (assigned) (spent) (cleaned up)
```

Defined in [`WalletStatus`](../crates/platform-core/src/models/status.rs)
(migration `0008`). Each stage explained with a concrete example:

### 1. `generated` — empty card printed
A keypair was created. It has an address but **0 SOL**. Not usable yet.
> *You click "Generate 10 bundlers" → 10 empty wallets appear, all `generated`.*

### 2. `funding` — loading money onto the card
Treasury sent SOL to it, but the transfer is **not confirmed on-chain yet**.
> *You hit "Fund Pool" → wallet is claimed and a treasury transfer is submitted →
> status flips to `funding` while the network confirms.*

### 3. `funded` — loaded and ready
The SOL arrived (balance ≥ minimum). The wallet is now **warm in the pool**,
waiting to be picked for a launch. This is your "ready pool."
> *A few seconds later the balance poller sees the SOL landed → status becomes
> `funded`.*

### 4. `reserved` — assigned to one launch
A launch starts, grabs a `funded` wallet, and **stamps it to that specific
launch** (`reserved_by_launch_id`) so no other launch can touch it. It stays
`reserved` even while the bundle is being submitted — **submitting is not
completion**.
> *Launch "DOGE2" needs 5 bundlers → 5 funded wallets become `reserved`, tagged
> to DOGE2. A concurrent launch cannot steal them.*

Guarded by `check_wallet_reserved_to_bundle`
([bundle_execute.rs](../crates/launcher/src/bundle_execute.rs)): every bundle
step asserts the wallet is `reserved` **and** tagged to the right launch.

### 5. `used` — swiped, job done
The launch confirmed on-chain (`launcher::confirm` moves `reserved → used`).
This wallet did its work and **will never be reused** for another launch. It may
still hold a little leftover SOL/dust.
> *DOGE2 lands successfully → its 5 bundler wallets become `used`.*

### 6. `retired` — emptied and shredded
The **dust sweep** ([dust_sweep.rs](../crates/launcher/src/dust_sweep.rs)) scans
`used` wallets, sends any leftover SOL back to treasury, then marks them
`retired`. Retired wallets are dropped from backups and never used again. (If the
balance is below the dust/fee floor, it's retired without a sweep.)
> *Sweep finds 0.002 SOL left → sends it home → marks the wallet `retired`.*

## Funding: the "Fund Pool" button

### What it does
On the wallet pool page, **Fund pool** / **Fund** sends
`POST /api/wallet_pool/fund` with `{ role?, count? }`. The button always sends
`{ role: undefined }` → top up **both** dev and bundler roles.

Call chain:
`WalletPoolPage.onFund`
→ `POST /api/wallet_pool/fund` ([http.rs](../crates/live/src/http.rs))
→ `fund_once(scope, FundMode::Manual)`
([wallet_funding.rs](../crates/launcher/src/wallet_funding.rs))
→ claims `generated` wallets to `funding`, submits treasury transfers
→ balance poller later promotes them to `funded`.

### It funds to a target, NOT "all wallets"
Per role, the number funded per press is:

```
count = explicit count you passed
      else  target_funded(role) − currently_funded(role)
```

Defaults: **dev target = 2**, **bundler target = 5**
(`FUND_TARGET_FUNDED_DEV` / `FUND_TARGET_FUNDED_BUNDLER`).

> *You have 10 dev + 10 bundler `generated` wallets. One Fund Pool press funds
> 2 dev + 5 bundler = 7. The other 13 stay `generated`. Pressing again only
> refills the shortfall.* To fund all 20, raise the targets or send an explicit
> `count`.

### Safety rails (a funding pass stops itself)
- **Spend cap** — once cumulative sends exceed
  `FUND_MAX_SPEND_PER_INTERVAL_LAMPORTS`, remaining transfers are marked
  `skipped_cap`.
- **Treasury reserve** — a transfer is only sent from a treasury that can cover
  it while staying above `FUND_TREASURY_RESERVE_LAMPORTS` (default 0.05 SOL). A
  breach **reverts all unsent claims (`funding` → `generated`) and stops the
  whole pass** — a partial launch must never proceed.
- **Dry run** — `FUND_DRY_RUN` plans and logs without sending.
- **Master switch** — if `FUND_ENABLED` is off, the endpoint returns `503`.

### How completion is detected
Manual funding is **fire-and-forget** (submit, don't wait) so the HTTP call
returns fast. A background **balance poller** (`spawn_balance_poller`) watches
wallets in `generated`/`funding`, and promotes `funding → funded` the moment the
observed on-chain balance reaches `MIN_FUNDED_LAMPORTS`. There is no manual "mark
funded". (A rare accepted-but-dropped tx can leave a wallet stuck in `funding`;
acceptable on this supervised path since the SOL never left treasury.)

There is also a separate just-in-time path,
`POST /api/wallet_pool/fund_for_launch` (`fund_for_launch`, `FundMode::Background`
with exact template-derived amounts) — distinct from the pool "Fund" button.

### Funding config (env vars)
[`FundingConfig`](../crates/launcher/src/config.rs):

| Env var | Default | Meaning |
| --- | --- | --- |
| `FUND_ENABLED` | off | Master switch for the whole subsystem |
| `FUND_TREASURY_RESERVE_LAMPORTS` | 50_000_000 | Min SOL to leave in treasury |
| `FUND_MAX_SPEND_PER_INTERVAL_LAMPORTS` | — | Hard spend cap per pass |
| `FUND_AMOUNT_DEV_LAMPORTS` | 50_000_000 | SOL per dev wallet |
| `FUND_AMOUNT_BUNDLER_LAMPORTS` | 30_000_000 | SOL per bundler wallet |
| `FUND_AMOUNT_JITTER_PCT` | 0.15 | ±jitter on each amount (obfuscation) |
| `FUND_TARGET_FUNDED_DEV` | 2 | Warm dev wallets to keep |
| `FUND_TARGET_FUNDED_BUNDLER` | 5 | Warm bundler wallets to keep |
| `FUND_MAX_DELAY_MS` | 8_000 | Timing jitter (background funder only) |
| `FUND_DRY_RUN` | off | Plan/log without sending |

## Quick reference

```
Roles:    treasury (source) · dev · bundler · trading
Statuses: generated → funding → funded → reserved → used → retired

reserved = picked from the ready pool and locked to ONE launch (in-service)
used     = that launch finished; wallet is spent, awaiting dust sweep (done)
```

Each wallet lives a single life — funded once, used for one launch, then emptied
and discarded. That single-use design is what keeps launches from being traceable
to each other.

## Source map

| Concern | File |
| --- | --- |
| Role & status enums (SSOT) | [platform-core/src/models/status.rs](../crates/platform-core/src/models/status.rs) |
| Funding pass, strategy, safety rails | [launcher/src/wallet_funding.rs](../crates/launcher/src/wallet_funding.rs) |
| Funding config / env vars | [launcher/src/config.rs](../crates/launcher/src/config.rs) |
| `reserved` guard during bundle | [launcher/src/bundle_execute.rs](../crates/launcher/src/bundle_execute.rs) |
| Dust sweep (`used` → `retired`) | [launcher/src/dust_sweep.rs](../crates/launcher/src/dust_sweep.rs) |
| Balance poller (`funding` → `funded`) | [live/src/main.rs](../crates/live/src/main.rs) |
| HTTP endpoint | [live/src/http.rs](../crates/live/src/http.rs) |
| Fund button (frontend) | [frontend-launch/src/features/wallets/WalletPoolPage.tsx](../frontend-launch/src/features/wallets/WalletPoolPage.tsx) |
| Design plan | retired — `wallet-funding-plan.md` + `jit-funding-plan.md` (both fully implemented; full detail in git history) |
