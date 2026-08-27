"""Thermometer: kind-gap (silence of real prints, not of all buys).

Reads DATABASE_URL from hunter/.env (never printed).
Requires ixg.bbuy / wal0 / mint0 / quiet / his_slot from burst + new-wallet builds.
"""
from __future__ import annotations

import sys
from pathlib import Path

import psycopg2

HUNTER_ENV = Path(__file__).resolve().parents[3] / ".env"
SQL_FILE = Path(__file__).with_name("ixg-kind-gap.sql")


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
          AND c.relname IN ('bbuy', 'wal0', 'mint0', 'quiet', 'his_slot')
        """
    )
    have = {r[0] for r in cur.fetchall()}
    need = {"bbuy", "wal0", "mint0", "quiet", "his_slot"}
    missing = need - have
    if missing:
        print("missing ixg tables:", sorted(missing))
        conn.close()
        return
    print("ixg tables ok:", sorted(have), flush=True)

    stmts = split_sql(SQL_FILE.read_text(encoding="utf-8"))
    print(f"building {len(stmts)} sql steps ...", flush=True)
    for i, stmt in enumerate(stmts, 1):
        head = next(
            (ln.strip()[:80] for ln in stmt.splitlines() if ln.strip() and not ln.strip().startswith("--")),
            stmt[:80],
        )
        print(f"  [{i}/{len(stmts)}] {head}", flush=True)
        cur.execute(stmt)
    print("built", flush=True)
    cur.execute("SELECT count(*) FROM ixg.kall")
    print("kall", cur.fetchone()[0], flush=True)
    cur.execute("SELECT count(*) FILTER (WHERE is_real), count(*) FROM ixg.kall")
    r = cur.fetchone()
    print("real", r[0], "of", r[1], flush=True)
    cur.execute("SELECT count(*) FROM ixg.kbrk")
    print("kbrk", cur.fetchone()[0], flush=True)

    show(
        "base: real prints after slot0 vs kind-gap breakers vs all-buy quiet",
        *q(
            cur,
            """
            SELECT
              'real_slot' AS set,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM (
              SELECT DISTINCT ON (r.mint, r.slot)
                EXISTS (
                  SELECT 1 FROM ixg.his_slot h
                  WHERE h.mint = r.mint AND h.slot IN (r.slot, r.slot + 1)
                ) AS he1,
                EXISTS (
                  SELECT 1 FROM ixg.his_slot h2
                  WHERE h2.mint = r.mint AND h2.slot = r.slot + 1
                ) AND NOT EXISTS (
                  SELECT 1 FROM ixg.his_slot h
                  WHERE h.mint = r.mint AND h.slot = r.slot
                ) AS he_causal
              FROM ixg.kreal r
              JOIN ixg.mint0 m ON m.mint = r.mint AND r.slot > m.slot0
              ORDER BY r.mint, r.slot, r.tx_index
            ) s
            UNION ALL
            SELECT 'kind5', count(*)::bigint,
              round(100.0 * avg(he1::int), 2),
              round(100.0 * avg(he_causal::int), 2),
              sum(he1::int)::bigint
            FROM ixg.kbrk
            UNION ALL
            SELECT 'kind5_and_buy5', count(*)::bigint,
              round(100.0 * avg(he1::int), 2),
              round(100.0 * avg(he_causal::int), 2),
              sum(he1::int)::bigint
            FROM ixg.kbrk WHERE buy5
            UNION ALL
            SELECT 'kind5_not_buy5', count(*)::bigint,
              round(100.0 * avg(he1::int), 2),
              round(100.0 * avg(he_causal::int), 2),
              sum(he1::int)::bigint
            FROM ixg.kbrk WHERE NOT buy5
            """,
        ),
    )

    show(
        "kind5 x buy5 (mid-run = kind gap, buys still printing)",
        *q(
            cur,
            """
            SELECT
              buy5,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.kbrk
            GROUP BY 1
            ORDER BY 1 DESC
            """,
        ),
    )

    show(
        "completing-print SOL band x buy5",
        *q(
            cur,
            """
            SELECT
              buy5,
              CASE
                WHEN sol < 0.3 THEN 'lt0.3'
                WHEN sol < 0.9 THEN '0.3-0.9'
                WHEN sol < 2 THEN '0.9-2'
                WHEN sol < 4 THEN '2-4'
                ELSE 'ge4'
              END AS sol,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.kbrk
            GROUP BY 1, 2
            ORDER BY 1 DESC, 2
            """,
        ),
    )

    show(
        "dslot_real band, mid-run only (NOT buy5)",
        *q(
            cur,
            """
            SELECT
              CASE
                WHEN dslot_real IS NULL THEN 'first_real'
                WHEN dslot_real < 10 THEN '5-9'
                WHEN dslot_real < 20 THEN '10-19'
                WHEN dslot_real < 40 THEN '20-39'
                ELSE 'ge40'
              END AS gap,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.kbrk
            WHERE NOT buy5
            GROUP BY 1
            ORDER BY 1
            """,
        ),
    )

    show(
        "kind-gap length including all-buy quiet",
        *q(
            cur,
            """
            SELECT
              buy5,
              CASE
                WHEN dslot_real IS NULL THEN 'first_real'
                WHEN dslot_real < 10 THEN '5-9'
                WHEN dslot_real < 20 THEN '10-19'
                WHEN dslot_real < 40 THEN '20-39'
                ELSE 'ge40'
              END AS gap,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.kbrk
            GROUP BY 1, 2
            ORDER BY 1 DESC, 2
            """,
        ),
    )

    show(
        "coverage of his fires (his_slot vs kbrk in S or S-1)",
        *q(
            cur,
            """
            SELECT
              count(*)::bigint AS his_slots,
              count(*) FILTER (
                WHERE EXISTS (
                  SELECT 1 FROM ixg.kbrk b
                  WHERE b.mint = h.mint AND b.slot IN (h.slot, h.slot - 1)
                )
              )::bigint AS on_kind5,
              count(*) FILTER (
                WHERE EXISTS (
                  SELECT 1 FROM ixg.kbrk b
                  WHERE b.mint = h.mint AND b.slot IN (h.slot, h.slot - 1) AND b.buy5
                )
              )::bigint AS on_buy5,
              count(*) FILTER (
                WHERE EXISTS (
                  SELECT 1 FROM ixg.kbrk b
                  WHERE b.mint = h.mint AND b.slot IN (h.slot, h.slot - 1) AND NOT b.buy5
                )
              )::bigint AS on_midrun
            FROM ixg.his_slot h
            """,
        ),
    )

    show(
        "template of the completing print, mid-run only",
        *q(
            cur,
            """
            SELECT
              tmpl,
              count(*)::bigint AS n,
              round(100.0 * avg(he1::int), 2) AS resp,
              round(100.0 * avg(he_causal::int), 2) AS causal,
              sum(he1::int)::bigint AS hits
            FROM ixg.kbrk
            WHERE NOT buy5
            GROUP BY 1
            HAVING count(*) >= 50
            ORDER BY avg(he1::int) DESC
            """,
        ),
    )

    conn.close()
    print()
    print("done")


if __name__ == "__main__":
    main()
