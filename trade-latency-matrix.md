# Trade latency matrix — Curve/AMM × Buy/Sell × Manual/Bot (2026-06-22)

Detailed per-phase latency for every existing trade path. Each phase is attributed
to the actual operation the code performs (every RPC round-trip, every blocking
`.await`, every cache hit), so the totals are traceable, not hand-waved.

> **Measured vs modeled.** Only **one** path was directly stopwatch-measured —
> *AMM sell, manual, cold* = **5,005 ms** (`📤 sent 2758ms` / `✅ confirmed 3675ms`),
> see [latency-analysis.md](latency-analysis.md). Every other number here is
> **modeled from the code's exact operation sequence** using the per-operation
> cost units below, which are themselves calibrated from that one measurement.
> Treat modeled totals as ±30%; the *shape* (which phases dominate) is exact.

---

## Per-operation cost units (the building blocks)

| Operation | Cost | On hot path? | Why / source |
| --- | --- | --- | --- |
| PDA derivation, ix build, math | ~0 (<1 ms) | — | pure in-process |
| Recent-blockhash (cache) | ~0 | cache hit | bg refresh 2 s, max age 10 s (`blockhash.rs`) |
| Jito tip (cache) | ~0 | cache hit | bg refresh 3 s, max age 30 s (`jito_tip.rs`) |
| Priority-fee / CU ixs | ~0 | static | built once at init (`init.rs`) |
| Nonce acquire (cache hit) | ~0 | yes | hashes pre-fetched at init; refresh is background (`nonce.rs`) |
| Nonce acquire (all slots busy) | 20 ms–4 s | rare tail | spin-wait, ≤200 iters × 20 ms |
| **One Helius RPC round-trip** | **~400 ms** | varies | modeled unit (range 300–600) |
| `get_account` / `get_token_balance` | ~400 ms | varies | 1 RPC |
| `resolve_buy_routing` | ~500 ms | manual only | migration + token-program read |
| Curve `curve_reserves` (cold) | ~400 ms | if slippage & cache miss | 1 RPC `get_account(bonding_curve)` |
| AMM `amm_pool_info` (cold) | ~1,500 ms | first AMM trade | pool acct + fee-share-marker (`getSignatures`+`getTx`) |
| AMM `amm_config` (cold) | ~400 ms | first AMM trade | 1 RPC, runs **concurrent** with pool_info |
| AMM reserves (cold) | ~400 ms | first AMM trade | vault balances |
| `send_transaction` (sender fan-out) | ~150 ms | yes | returns on first endpoint success (range 50–300) |
| **RPC confirm poll** (`confirm=true`) | **~900 ms** | manual only | polls `[250,400,700,1000,1000]`; landed-fast ≈900, worst ≈3,350 |
| **Feed confirm** (`confirm=false`) | off-path | bot only | LaserStream `trades` loop, ≤5 s deadline, typ. 0.5–2 s |

**Warm vs cold:** *cold* = first trade of a mint in a fresh process (all caches
empty). *warm* = `token_pdas`, `user_token_accounts`, reserves (WS snapshot),
AMM `pool_info`/`config` already populated (from a prior trade or the ingest feed).
The **bot path is effectively always warm** — reserves/routing are threaded in from
the ingest cache.

---

## The 8 core cases

Legend: 🟢 in-process/cached (~0) · 🟡 RPC round-trip · 🔴 confirm wait · ⚪ off critical path

### A. CURVE · BUY · MANUAL  (`manual_buy` → `buy_token`)

| # | Phase | Op | Cost | |
| --- | --- | --- | --- | --- |
| 1 | Route resolve | `resolve_buy_routing` (RPC) | ~500 ms | 🟡 |
| 2 | ATA existence | `get_account(ata)` (RPC) | ~400 ms | 🟡 |
| 3 | Slippage reserves | WS cache hit 0 / cold `curve_reserves` | 0–400 ms | 🟡 |
| 4 | Build + nonce + bh/tip/CU | all cached | ~0 | 🟢 |
| 5 | Submit | `send_transaction` | ~150 ms | 🟡 |
| 6 | Confirm | RPC poll | ~900 ms | 🔴 |
| | **Total (cold)** | | **~2.0–2.4 s** | |
| | **Total (warm)** | reserves from WS, rest same | **~1.6–2.0 s** | |

### B. CURVE · BUY · BOT/SNIPE  (`buy_token_snipe`, `skip_ata_check`+`skip_confirm`)

| # | Phase | Op | Cost | |
| --- | --- | --- | --- | --- |
| 1 | Reserves | threaded from ingest cache | ~0 | 🟢 |
| 2 | ATA check | **skipped** | ~0 | 🟢 |
| 3 | Build + nonce + bh/tip/CU | all cached | ~0 | 🟢 |
| 4 | Submit | `send_transaction` | ~150 ms | 🟡 |
| 5 | Confirm | **feed (off-path)** | — | ⚪ |
| | **Submit latency** | signal → tx sent | **~150 ms** | |
| | Confirm (off engine) | feed balance loop | 0.5–2 s ⚪ | |

### C. CURVE · SELL · MANUAL  (`manual_sell` clear-loop → `sell_token`)

| # | Phase | Op | Cost | |
| --- | --- | --- | --- | --- |
| 1 | Route resolve | `resolve_buy_routing` (RPC) | ~500 ms | 🟡 |
| 2 | Pre balance read | `get_token_balance` (RPC) | ~400 ms | 🟡 |
| 3 | Resolve PDAs/acct | cache miss → 1 RPC | 0–400 ms | 🟡 |
| 4 | Slippage reserves | cold `curve_reserves` | 0–400 ms | 🟡 |
| 5 | Build + nonce + bh/tip | cached | ~0 | 🟢 |
| 6 | Submit | `send_transaction` | ~150 ms | 🟡 |
| 7 | Confirm | RPC poll | ~900 ms | 🔴 |
| 8 | Post balance read (=0) | clear-loop verify (RPC) | ~400 ms | 🟡 |
| | **Total (cold, 1 pass)** | | **~2.6–3.1 s** | |
| | + per extra retry | fresh nonce, +confirm +backoff | +~1.3 s each | up to `MAX_SELL_ATTEMPTS=5` |

### D. CURVE · SELL · BOT  (`sell_token_once`, `confirm=false`)

| # | Phase | Op | Cost | |
| --- | --- | --- | --- | --- |
| 1 | Routing | from token cache (WS) | ~0 | 🟢 |
| 2 | Token account | resolved once, then cached | 0 (first: ~400 ms) | 🟢 |
| 3 | Slippage reserves | WS cache hit / fallback min_out=1 | 0–400 ms | 🟡 |
| 4 | Build + nonce + send | cached + submit | ~150 ms | 🟡 |
| 5 | Confirm | **feed (off-path)** | — | ⚪ |
| | **Submit latency** | | **~150–550 ms** | |
| | Confirm (off engine) | sell-confirm feed loop | 0.5–2 s ⚪ | |

### E. AMM · BUY · MANUAL  (`manual_buy` → `amm_buy`, recent-blockhash, no nonce)

| # | Phase | Op | Cost | |
| --- | --- | --- | --- | --- |
| 1 | Route resolve | `resolve_buy_routing` (RPC) | ~500 ms | 🟡 |
| 2 | Pool + config | `amm_pool_info` ∥ `amm_config` (cold) | ~1,500 ms | 🟡 |
| 3 | Reserves | cold vault read | ~400 ms | 🟡 |
| 4 | Build + bh/tip/CU | cached (no nonce) | ~0 | 🟢 |
| 5 | Submit | `send_transaction` | ~150 ms | 🟡 |
| 6 | Confirm | RPC poll | ~900 ms | 🔴 |
| | **Total (cold)** | | **~3.0–3.5 s** | |
| | **Total (warm)** | pool/config/reserves cached | **~1.5–1.9 s** | |

### F. AMM · BUY · BOT  (`amm_buy`, `confirm=false`)

| # | Phase | Op | Cost | |
| --- | --- | --- | --- | --- |
| 1 | Routing | from cache | ~0 | 🟢 |
| 2 | Pool/config/reserves | warm (or +~2 s one-time cold) | ~0 | 🟢 |
| 3 | Build + bh + send | cached + submit | ~150 ms | 🟡 |
| 4 | Confirm | **feed (off-path)** | — | ⚪ |
| | **Submit latency (warm)** | | **~150 ms** | |
| | first-ever AMM trade (cold) | +pool/config/reserves | +~2.0 s one-time | |

### G. AMM · SELL · MANUAL  ✅ **MEASURED** (`manual_sell` → `amm_sell`, durable nonce)

| # | Phase | Op | Cost | |
| --- | --- | --- | --- | --- |
| 1 | Route resolve + pre balance | wrapper RPCs | ~900 ms | 🟡 |
| 2 | Pool + config | `amm_pool_info` ∥ `amm_config` (cold) | ~1,500 ms | 🟡 |
| 3 | Reserves + nonce + build | cold reserves + acquire | ~700 ms | 🟡 |
| 4 | Submit | `send_transaction` → **`📤 sent @2758ms`** | ~150 ms | 🟡 |
| 5 | Confirm | RPC poll → **`✅ confirmed @3675ms`** | **+917 ms** | 🔴 |
| 6 | Post balance read (=0) | clear-loop verify | ~430 ms | 🟡 |
| | **TOTAL (measured)** | | **5,005 ms** | |

### H. AMM · SELL · BOT  (`amm_sell`, `confirm=false`)

| # | Phase | Op | Cost | |
| --- | --- | --- | --- | --- |
| 1 | Routing | from cache | ~0 | 🟢 |
| 2 | Token account | cached | ~0 | 🟢 |
| 3 | Pool/config/reserves | warm | ~0 | 🟢 |
| 4 | Build + nonce + send | cached + submit | ~150 ms | 🟡 |
| 5 | Confirm | **feed (off-path)** | — | ⚪ |
| | **Submit latency (warm)** | | **~150 ms** | |
| | Confirm (off engine) | sell-confirm feed loop | 0.5–2 s ⚪ | |

---

## Summary matrix — end-to-end latency

"Submit" = signal/request → tx accepted by sender. "Total" = until the caller's
response (manual: incl. RPC confirm + clear-loop; bot: submit only, confirm is
off-engine via the feed).

| Venue | Dir | Caller | Cold total | Warm total | Submit-only | Confirm style |
| --- | --- | --- | --- | --- | --- | --- |
| Curve | Buy | Manual | ~2.0–2.4 s | ~1.6–2.0 s | ~1.0 s | RPC poll ~900 ms |
| Curve | Buy | **Bot** | — | — | **~150 ms** | feed ⚪ |
| Curve | Sell | Manual | ~2.6–3.1 s | ~2.0–2.5 s | ~1.4 s | RPC poll ~900 ms |
| Curve | Sell | **Bot** | — | — | **~150–550 ms** | feed ⚪ |
| AMM | Buy | Manual | ~3.0–3.5 s | ~1.5–1.9 s | ~2.1 s / ~0.6 s | RPC poll ~900 ms |
| AMM | Buy | **Bot** | (+~2 s 1st) | — | **~150 ms** | feed ⚪ |
| AMM | Sell | Manual | **5.0 s ✅** | ~2.0–2.5 s | ~2.8 s / ~0.6 s | RPC poll ~917 ms |
| AMM | Sell | **Bot** | — | — | **~150 ms** | feed ⚪ |

✅ = directly measured. Cold AMM totals are inflated by the one-time pool/config
cache fill (~1.5–2 s); warm columns are the steady-state.

---

## Where the time goes (manual paths)

Three costs dominate every manual trade, **all absent from the bot path**:

1. **RPC confirm poll — ~900 ms** (`confirm=true`). The bot uses `confirm=false`
   and confirms off the LaserStream feed; the submit returns immediately.
2. **`resolve_buy_routing` — ~500 ms.** The manual API re-resolves migration +
   token program live every call; the bot reads routing from the warm token cache.
3. **Cold AMM cache fill — ~1.5–2 s** (first AMM trade only) + **clear-loop balance
   reads — ~0.8 s** on sells (manual "Sell All" reads balance before *and* after).

Everything else (blockhash, Jito tip, CU/priority fee, nonce hash, PDA derivation)
is **pre-warmed and effectively free** on the hot path — confirmed in `init.rs`
(prime + background refresh loops) and the cache-first getters.

## Why the bot is fast and the manual API is slow

| Cost | Manual | Bot strategy |
| --- | --- | --- |
| Confirm | RPC poll ~900 ms (blocks response) | feed, off-engine |
| Routing | RPC ~500 ms every call | warm cache ~0 |
| Reserves | on-chain / cold | threaded from ingest ~0 |
| AMM pool/config | cold ~1.5–2 s (1st) | warm ~0 |
| Balance reads | pre+post clear-loop ~0.8 s | feed-driven, none inline |
| **Net submit** | **~1–2.8 s** | **~0.15 s** |

**Takeaway:** the 3–5 s you felt is the **manual-API** figure (cold caches +
RPC-confirm + clear-loop). The strategy engine's actual hot-path submit is
**~150 ms**, with confirmation handled asynchronously off the LaserStream feed
(typically 0.5–2 s, never blocking the next action). The durable-nonce slots,
blockhash, tip, and fee ixs are all pre-warmed, so the only unavoidable hot-path
costs on the bot path are the network submit (~150 ms) and, when slippage
protection needs a fresh quote, one reserve read.

## Tail risks (rare, worth knowing)

- **Nonce spin-wait** (curve + AMM sell): if all durable-nonce slots are in-flight,
  `acquire_nonce` spin-waits up to ~4 s. Mitigated by slot count + background refresh.
- **Confirm worst case** (manual): if the tx lands slowly, the RPC poll runs the full
  `[250,400,700,1000,1000]` ≈ 3.35 s before returning.
- **Manual sell retries**: each `OnChainRevert`+slippage retry re-runs submit+confirm
  (+~1.3 s, ×`MAX_SELL_ATTEMPTS=5`).
- **Feed index lag** (bot confirm): the sell-confirm loop polls the full window before
  retry to absorb gRPC index lag — a deliberate buffer against duplicate sells.

---

## Methodology notes

- Operation sequences extracted directly from `buy.rs`, `sell.rs`, `amm.rs`,
  `tx.rs`, `nonce.rs`, `jito_tip.rs`, `init.rs`, the manual handlers
  (`api/handlers/trading/solana.rs`), and the strategy exec
  (`strategies/tpsl_sniper_{1,2}/execution/real.rs`). tpsl1 and tpsl2 are identical
  on the buy/sell execution path.
- Per-op cost units calibrated from the single measured run (AMM sell manual cold,
  5,005 ms with the 2,758/917 internal split). RPC unit (~400 ms) is back-solved
  from the manual wrapper overhead and cross-checked against the cold AMM submit.
- To convert any modeled row to measured, time the HTTP request (manual) or read
  the trader's `📤 sent`/`✅ confirmed` log markers (both), as done for case G.
