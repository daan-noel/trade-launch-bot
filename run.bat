docker compose down -v

docker compose up -d --build


@REM Remove old "_sqlx_migrations" history:
docker compose exec postgres psql -U postgres -d meme_bot -c "DROP TABLE IF EXISTS _sqlx_migrations;"
docker compose up -d backend

@REM Check:
docker compose exec postgres psql -U postgres -d meme_bot -c "SELECT version, success FROM _sqlx_migrations ORDER BY version;"



@REM DB backup:
docker compose exec -T postgres pg_dump -U postgres -Fc -d meme_bot > backup.dump
@REM DB restore:
pg_restore -U postgres -d meme_bot --clean --if-exists < backup.dump
