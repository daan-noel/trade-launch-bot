docker compose down -v

docker compose up -d --build
@REM ==========================================================================================




@REM Remove old "_sqlx_migrations" history:
docker compose exec postgres psql -U postgres -d meme_bot -c "DROP TABLE IF EXISTS _sqlx_migrations;"
docker compose up -d backend

@REM Check:
docker compose exec postgres psql -U postgres -d meme_bot -c "SELECT version, success FROM _sqlx_migrations ORDER BY version;"





@REM ==========================================================================================
@REM DB backup:
docker compose exec -T postgres pg_dump -U postgres -Fc -d meme_bot > backup.dump
@REM DB restore:
pg_restore -U postgres -d meme_bot --clean --if-exists < backup.dump
@REM ==========================================================================================




@REM ==========================================================================================
docker compose exec -T postgres \
  pg_dump -U postgres -Fc -d meme_bot \
  -t 'public.tokens' \
  -t 'public.tokens_analysis' \
  -t 'public.tokens_info' \
  -t 'public.trades' \
  -t 'public.trades_*' \
  -t 'public.tpsl2_paper_positions' \
  -t 'public.tpsl2_paper_test_run' \
  -t 'public.tpsl2_real_positions' \
  -t 'public.tpsl2_strategy_rules' \
  > backup.dump
@REM ==========================================================================================
psql -U postgres -d meme_bot -c "TRUNCATE TABLE public.tokens, public.tokens_analysis, public.tokens_info, public.trades, public.tpsl2_paper_positions, public.tpsl2_paper_test_run, public.tpsl2_real_positions, public.tpsl2_strategy_rules RESTART IDENTITY CASCADE;"

pg_restore -U postgres -d meme_bot --data-only --disable-triggers < backup.dump
@REM ==========================================================================================




@REM ==========================================================================================
htop

free -h

df -h
sudo du -xh / --max-depth=1 2>/dev/null | sort -h
@REM ==========================================================================================
