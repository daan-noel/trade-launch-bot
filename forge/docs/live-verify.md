# Live verification checklist (Phase 2b)

Manual mainnet checks before Phase 3 trading work. Automated schema proof lives
in `ingest-host/tests/roundtrip.rs` (set `PLATFORM_TEST_DATABASE_URL`).

## 1. Ingest round-trip (live feed)

```powershell
# .env: DATABASE_URL, HELIUS_LASERSTREAM_URL, HELIUS_API_KEY
cargo run -p live
```

Spot-check after a few minutes:

```sql
-- trades carry generalized dimensions + reserves
SELECT mint_address, launchpad_id, quote_asset_id, reserve_quote, reserve_base, trade_type
FROM trades ORDER BY slot DESC LIMIT 20;

-- USD derived in view (needs SOL usd_rate from poller)
SELECT mint_address, amount_quote, amount_usd, exec_price_quote, quote_usd_rate
FROM trades_priced ORDER BY slot DESC LIMIT 20;
```

Expect `launchpad_id = 1` (pump_fun), `quote_asset_id = 1` (SOL), non-null reserves
on curve trades.

## 2. Launch + bundle E2E

Prereqs: funded dev + bundler wallets in keystore, launch template with
`bundle_leg_count` + `leg_structures`, Jito URL in `.env`.

```powershell
# POST /api/launches/execute — auto-plans + auto-submits bundle
curl -X POST http://127.0.0.1:8230/api/launches/execute `
  -H "Content-Type: application/json" `
  -d '{"template_id":"<uuid>","dev_wallet_id":"<uuid>"}'
```

Response includes `bundle.jito_bundle_id` when sniper legs submitted.

Confirm landing (feed-based, no RPC poll):

```powershell
curl http://127.0.0.1:8230/api/bundles/<bundle_id>
# status → landed | dropped | partial
```

Verify sniper buy legs appear in ingest for the launch mint:

```sql
SELECT tx_signature, wallet_id, trade_type, amount_quote, slot
FROM trades t
LEFT JOIN wallet_dict w ON w.id = t.wallet_id
WHERE mint_address = '<mint>' AND trade_type = 'buy'
ORDER BY slot;
```

## 3. Dep partition

```powershell
.\scripts\dep-partition-check.ps1
```

CI runs the same guard on every push (`.github/workflows/ci.yml`).

## 4. Launch console UI

```powershell
# Terminal 1 — backend
cargo run -p live

# Terminal 2 — seed once (edit pubkeys in script first)
# psql $env:DATABASE_URL -f scripts/seed-dev-launch.sql

# Terminal 3 — UI (proxies /api → :8230)
cd frontend-launch
npm install
npm run dev
```

Open http://127.0.0.1:5175 — pick template + dev wallet, click **Launch**, watch bundle
status + ingested trades poll every 3s.
