# DB Snapshot Commands

Manual one-liners. Replace `<pw>`, `<ec2-ip>`, and `<dump-file>` with real values.
`POSTGRES_PASSWORD` is in `.env` on the EC2 box.

---

## EC2 — Dump (run on the box)

```bash
docker compose exec -T -e PGPASSWORD='<POSTGRES_PASSWORD>' postgres \
  pg_dump -U postgres -d meme_bot \
    --data-only --format=custom --compress=zstd:3 --load-via-partition-root \
    --exclude-table-data='raw_transactions*' \
    --exclude-table-data='*grouped_sweep*' \
    --exclude-table-data='app_settings' \
    --exclude-table-data='_sqlx_migrations' \
  > snapshots/meme_bot_data_$(date -u +%Y%m%d_%H%M%SZ).dump
```

---

## Local — Restore (run on Windows, in order)

### 1. Pull the latest dump from EC2

```powershell
$f = (ssh ubuntu@<ec2-ip> 'ls -t ~/meme-trading/snapshots/*.dump | head -1').Trim(); scp "ubuntu@<ec2-ip>:$f" snapshots/
```

### 2. Ensure local trades partitions exist (idempotent — safe to re-run)

```powershell
$env:PGPASSWORD='<local_pw>'; psql -U postgres -d meme_bot -c "SELECT ensure_trades_partition(d::date) FROM generate_series(current_date - 10, current_date + 2, interval '1 day') d"
```

### 3. Truncate the refresh set (sweep results / settings / raw_transactions are NOT touched)

```powershell
$env:PGPASSWORD='<local_pw>'; psql -U postgres -d meme_bot -c "TRUNCATE tokens, tokens_info, tokens_analysis, creator_profiles, trades, tpsl1_strategy_rules, tpsl2_strategy_rules, tpsl1_real_positions, tpsl2_real_positions, tpsl1_paper_positions, tpsl2_paper_positions, tpsl1_paper_test_run, tpsl2_paper_test_run, wallets, wallet_profiles, wallet_profile_tags CASCADE"
```

### 4. Restore

```powershell
$env:PGPASSWORD='<local_pw>'; pg_restore -U postgres -h localhost -p 5432 -d meme_bot -j 4 --data-only --disable-triggers snapshots/<dump-file>.dump
```

> `--disable-triggers` requires a superuser local role. Stop any local backend writing to `meme_bot` before running steps 3–4.
