"""Thermometer: solo quiet-resume as a turn into selling / a dip.

Reads DATABASE_URL from hunter/.env (never printed).
Requires ixg.burst / bmem_old / mint0 / his_slot from burst + old-wallet builds.
"""
from __future__ import annotations

import sys
from pathlib import Path

import psycopg2

HUNTER_ENV = Path(__file__).resolve().parents[3] / ".env"
SQL_FILE = Path(__file__).with_name("ixg-solo-turn.sql")


def load_url() -> str:
    for line in HUNTER_ENV.read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s.startswith("DATABASE_URL=") and not s.startswith("#"):
            return s.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit("DATABASE_URL missing in hunter/.env")


def split_sql(text: str) -> list[str]:
    stmts = []
    buf: list[str] = []
    for line in text.splitlines():
        buf.append(line)
        if line.rstrip().endswith(";"):
            stmt = "\n".join(buf).strip()
            body = [
                ln
                for ln in stmt.splitlines()
                if ln.strip() and not ln.strip().startswith("--")
            ]
            if body:
                stmts.append(stmt)
            buf = []
    return stmts


def q(cur, sql: str):
    cur.execute(sql)
    cols = [d[0] for d in cur.description]
    rows = cur.fetchall()
    return cols, rows


def show(title: str, cols, rows):
    print()
    print("== " + title)
    if not rows:
        print("(empty)")
        return
    widths = [len(c) for c in cols]
    str_rows = []
    for r in rows:
        cells = []
        for i, v in enumerate(r):
            if v is None:
                s = ""
            elif isinstance(v, float):
                s = f"{v:.4f}" if abs(v) < 10 else f"{v:.2f}"
            else:
                s = str(v)
            cells.append(s)
            widths[i] = max(widths[i], len(s))
        str_rows.append(cells)
    fmt = "  ".join(f"{{:{w}}}" for w in widths)
    print(fmt.format(*cols))
    print(fmt.format(*["-" * w for w in widths]))
    for cells in str_rows:
        print(fmt.format(*cells))


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    conn = psycopg2.connect(load_url())
    conn.autocommit = True
    cur = conn.cursor()
    cur.execute("SET statement_timeout = 0")
    cur.execute("SET work_mem = '2GB'")
    cur.execute("SET synchronous_commit = off")

    cur.execute(
        """
        SELECT c.relname FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'ixg' AND c.relkind = 'r'
          AND c.relname IN (
            'burst', 'bmem_old', 'mint0', 'his_slot', 'sturn'
          )
        """
    )
    have = {r[0] for r in cur.fetchall()}
    need = {"burst", "bmem_old", "mint0", "his_slot"}
    missing = need - have
    if missing:
        print("missing ixg tables:", sorted(missing))
        conn.close()
        return
    print("ixg tables ok:", sorted(have), flush=True)

    skip = "--skip-build" in sys.argv
    if skip and "sturn" in have:
        print("skip-build: ixg.sturn present", flush=True)
    else:
        stmts = split_sql(SQL_FILE.read_text(encoding="utf-8"))
        print(f"building {len(stmts)} sql steps ...", flush=True)
        for i, stmt in enumerate(stmts, 1):
            head = next(
                (
                    ln.strip()[:80]
                    for ln in stmt.splitlines()
                    if ln.strip() and not ln.strip().startswith("--")
                ),
                stmt[:80],
            )
            print(f"  [{i}/{len(stmts)}] {head}", flush=True)
            cur.execute(stmt)
        print("built", flush=True)

    cur.execute("SELECT count(*) FROM ixg.scand")
    print("scand", cur.fetchone()[0], flush=True)
    cur.execute("SELECT count(*) FROM ixg.stape")
    print("stape", cur.fetchone()[0], flush=True)
    cur.execute("SELECT count(*) FROM ixg.sturn")
    print("sturn", cur.fetchone()[0], flush=True)

    show(
        "solo tot band x gap-sell (any sell in [S-5, S))",
        *q(
            cur,
            """
            SELECT
              CASE
                WHEN tot < 0.3 THEN 'lt0.3'
                WHEN tot < 0.9 THEN '0.3-0.9'
                WHEN tot < 2 THEN '0.9-2'
                WHEN tot < 4 THEN '2-4'
                ELSE 'ge4'
              END AS tot,
              (n_sell_gap > 0) AS gap_sell,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.sturn
            GROUP BY 1, 2
            ORDER BY 1, 2 DESC
            """,
        ),
    )

    show(
        "tot[0.9,4): wal_new x working x gap_sell",
        *q(
            cur,
            """
            SELECT
              wal_new, working, (n_sell_gap > 0) AS gap_sell,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.sturn
            WHERE tot >= 0.9 AND tot < 4
            GROUP BY 1, 2, 3
            ORDER BY 1 DESC, 2 DESC, 3 DESC
            """,
        ),
    )

    show(
        "live cell all_new working tot[0.9,4): sol_sell_gap band",
        *q(
            cur,
            """
            SELECT
              CASE
                WHEN n_sell_gap = 0 THEN 'none'
                WHEN sol_sell_gap < 0.3 THEN 'lt0.3'
                WHEN sol_sell_gap < 1 THEN '0.3-1'
                WHEN sol_sell_gap < 3 THEN '1-3'
                ELSE 'ge3'
              END AS sell_sol,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.sturn
            WHERE tot >= 0.9 AND tot < 4 AND wal_new AND working
            GROUP BY 1
            ORDER BY 1
            """,
        ),
    )

    show(
        "live cell: last_side (print immediately before the solo)",
        *q(
            cur,
            """
            SELECT
              COALESCE(last_side, 'none') AS last_side,
              (n_sell_gap > 0) AS gap_sell,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.sturn
            WHERE tot >= 0.9 AND tot < 4 AND wal_new AND working
            GROUP BY 1, 2
            ORDER BY 1, 2 DESC
            """,
        ),
    )

    show(
        "live cell: trail band x gap_sell",
        *q(
            cur,
            """
            SELECT
              CASE
                WHEN trail IS NULL THEN 'unk'
                WHEN trail < 5 THEN 'lt5'
                WHEN trail < 15 THEN '5-15'
                WHEN trail < 30 THEN '15-30'
                WHEN trail < 60 THEN '30-60'
                ELSE 'ge60'
              END AS trail,
              (n_sell_gap > 0) AS gap_sell,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.sturn
            WHERE tot >= 0.9 AND tot < 4 AND wal_new AND working
            GROUP BY 1, 2
            ORDER BY 1, 2 DESC
            """,
        ),
    )

    show(
        "live cell: vsol_pre x gap_sell",
        *q(
            cur,
            """
            SELECT
              CASE
                WHEN vsol IS NULL THEN 'unk'
                WHEN vsol < 33 THEN 'lt33'
                WHEN vsol < 46 THEN '33-46'
                ELSE 'ge46'
              END AS vsol,
              (n_sell_gap > 0) AS gap_sell,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.sturn
            WHERE tot >= 0.9 AND tot < 4 AND wal_new AND working
            GROUP BY 1, 2
            ORDER BY 1, 2 DESC
            """,
        ),
    )

    show(
        "live cell: same-slot sell before the solo",
        *q(
            cur,
            """
            SELECT
              (n_sell_before > 0) AS sell_before,
              (n_sell_gap > 0) AS gap_sell,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.sturn
            WHERE tot >= 0.9 AND tot < 4 AND wal_new AND working
            GROUP BY 1, 2
            ORDER BY 1 DESC, 2 DESC
            """,
        ),
    )

    show(
        "live cell tot[0.3,0.9): gap_sell (smaller solo into a turn?)",
        *q(
            cur,
            """
            SELECT
              (n_sell_gap > 0) AS gap_sell,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.sturn
            WHERE tot >= 0.3 AND tot < 0.9 AND wal_new AND working
            GROUP BY 1
            ORDER BY 1 DESC
            """,
        ),
    )

    show(
        "live cell vsol<46: trail x gap_sell",
        *q(
            cur,
            """
            SELECT
              CASE
                WHEN trail IS NULL THEN 'unk'
                WHEN trail < 5 THEN 'lt5'
                WHEN trail < 15 THEN '5-15'
                WHEN trail < 30 THEN '15-30'
                WHEN trail < 60 THEN '30-60'
                ELSE 'ge60'
              END AS trail,
              (n_sell_gap > 0) AS gap_sell,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.sturn
            WHERE tot >= 0.9 AND tot < 4 AND wal_new AND working
              AND vsol IS NOT NULL AND vsol < 46
            GROUP BY 1, 2
            ORDER BY 1, 2 DESC
            """,
        ),
    )

    show(
        "live cell vsol<46: last_side=sell vs trail>=15 vs gap_sell",
        *q(
            cur,
            """
            SELECT
              (last_side = 'sell') AS last_sell,
              (COALESCE(trail, 0) >= 15) AS dip15,
              (n_sell_gap > 0) AS gap_sell,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.sturn
            WHERE tot >= 0.9 AND tot < 4 AND wal_new AND working
              AND vsol IS NOT NULL AND vsol < 46
            GROUP BY 1, 2, 3
            ORDER BY 1 DESC, 2 DESC, 3 DESC
            """,
        ),
    )

    show(
        "coverage of his solo fires (he1) by gap_sell x last_side x dip15",
        *q(
            cur,
            """
            SELECT
              (n_sell_gap > 0) AS gap_sell,
              COALESCE(last_side, 'none') AS last_side,
              (COALESCE(trail, 0) >= 15) AS dip15,
              count(*)::bigint AS hits,
              round(100.0 * count(*) / sum(count(*)) OVER (), 2) AS pct_of_hits
            FROM ixg.sturn
            WHERE he1
            GROUP BY 1, 2, 3
            ORDER BY count(*) DESC
            """,
        ),
    )

    show(
        "his solo fires tot[0.9,4) all_new working: turn flags",
        *q(
            cur,
            """
            SELECT
              (n_sell_gap > 0) AS gap_sell,
              (last_side = 'sell') AS last_sell,
              (COALESCE(trail, 0) >= 15) AS dip15,
              count(*)::bigint AS hits,
              round(100.0 * count(*) / sum(count(*)) OVER (), 2) AS pct
            FROM ixg.sturn
            WHERE he1 AND tot >= 0.9 AND tot < 4 AND wal_new AND working
            GROUP BY 1, 2, 3
            ORDER BY count(*) DESC
            """,
        ),
    )

    show(
        "age band x gap_sell, live cell vsol<46",
        *q(
            cur,
            """
            SELECT
              CASE
                WHEN age_s < 20 THEN 'lt20'
                WHEN age_s < 60 THEN '20-60'
                WHEN age_s < 180 THEN '60-180'
                WHEN age_s < 600 THEN '180-600'
                ELSE 'ge600'
              END AS age,
              (n_sell_gap > 0) AS gap_sell,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.sturn
            WHERE tot >= 0.9 AND tot < 4 AND wal_new AND working
              AND vsol IS NOT NULL AND vsol < 46
            GROUP BY 1, 2
            ORDER BY 1, 2 DESC
            """,
        ),
    )

    conn.close()
    print()
    print("done")


if __name__ == "__main__":
    main()
