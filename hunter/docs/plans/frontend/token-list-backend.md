# Token-list backend differs by bin

Deep-dive for the data-scale rule in [CLAUDE.md](../../../CLAUDE.md). Both bins serve the
same `/api/tokens` wire contract but back it with **different engines** — one SQL-paged for
the live universe, one in-RAM for analysis. Related: `@arch/frontend.md`, `@arch/database.md`.

## Same wire contract

`/api/tokens` is **`POST` `TableRequest`** — the unified strategy-table body. The global
filter panel + per-column filters fold into ONE `filters:{col→FilterSpec}` map, lowered onto
the internal engines by `TokenQuery::from_table_request`.

## `live` — straight from Postgres (no in-RAM cap)

`live` pages the list **straight from Postgres** — filter/sort/search are compiled to SQL by
`trading_core::api::handlers::tokens::sql` (`build_where_and_order` →
`TokenRepo::find_list_page` / `count_list`), so the full token universe (100K+) is pageable
with **no in-RAM cap**. The live in-RAM `token_list` snapshot holds **only tracking tokens**
(4 GB EC2 guardrail — live does NOT run `run_token_list_db_refresh`).

## `lab` — in-RAM over a full snapshot

`lab` runs the in-RAM `build_tokens_list` engine over a **full snapshot**
(`LAB_TOKEN_LIST_LIMIT` / `LAB_TOKEN_LIST_WINDOW_DAYS`, workstation RAM, analysis speed).

## Seed cap is not a list cap

`SEED_TRACKING_LIMIT` (formerly `SEED_TOKEN_LIMIT`) is the **live tracking-cache seed** cap
only — never the list cap.

## Parity guards

The two engines are held at parity by `token_repo::parity_tests` (not `--ignored`:
auto-runs when `DATABASE_URL` is set, self-skips otherwise) plus a **no-DB** column-key guard
`handlers::tokens::grammar_parity_tests` that runs on every `cargo test`.
