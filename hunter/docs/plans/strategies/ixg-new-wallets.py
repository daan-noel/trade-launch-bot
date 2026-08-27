"""Thermometer: burst wallets first-on-this-mint vs repeat.

Reads DATABASE_URL from hunter/.env (never prints it).
Requires ixg.wal / ixg.bmem / ixg.burst / ixg.his_slot from ixg-burst-kinds.sql.
"""
from __future__ import annotations

import sys
from pathlib import Path

import psycopg2


HUNTER_ENV = Path(__file__).resolve().parents[3] / ".env"  # hunter/.env
SQL_FILE = Path(__file__).with_name("ixg-new-wallets.sql")


def load_url() -> str:
    for line in HUNTER_ENV.read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s.startswith("DATABASE_URL=") and not s.startswith("#"):
            return s.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit("DATABASE_URL missing in hunter/.env")


def q(cur, sql: str):
    cur.execute(sql)
    cols = [d[0] for d in cur.description]
    rows = cur.fetchall()
    return cols, rows


def show(title: str, cols, rows, pct_cols=()):
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
            elif cols[i] in pct_cols and isinstance(v, (int, float)):
                s = f"{100.0 * float(v):.2f}"
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
        SELECT n.nspname, c.relname
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'ixg'
          AND c.relname IN ('wal', 'bmem', 'burst', 'his_slot', 'dict')
          AND c.relkind = 'r'
        ORDER BY 2
        """
    )
    have = {r[1] for r in cur.fetchall()}
    need = {"wal", "bmem", "burst", "his_slot"}
    missing = need - have
    if missing:
        print("missing ixg tables:", sorted(missing))
        print("rebuild ixg-burst-kinds.sql first")
        conn.close()
        return
    print("ixg tables ok:", sorted(have))

    cur.execute("SELECT count(*) FROM ixg.bmem")
    print("bmem", cur.fetchone()[0], flush=True)
    cur.execute("SELECT count(*) FROM ixg.burst")
    print("burst", cur.fetchone()[0], flush=True)
    cur.execute("SELECT count(*) FROM ixg.wal")
    print("wal", cur.fetchone()[0], flush=True)

    print("building wal0 / bmem_new / bnew ...", flush=True)
    sql = SQL_FILE.read_text(encoding="utf-8")
    cur.execute(sql)
    print("built", flush=True)
    cur.execute("SELECT count(*) FROM ixg.bnew")
    print("bnew", cur.fetchone()[0], flush=True)

    show(
        "member: ATA vs first-on-mint (print grain = wallet new this slot)",
        *q(
            cur,
            """
            SELECT
              ata,
              wal_new,
              count(*)::bigint AS n,
              round(100.0 * count(*) / sum(count(*)) OVER (), 2) AS pct
            FROM ixg.bmem_new
            WHERE wallet_id IS NOT NULL
            GROUP BY 1, 2
            ORDER BY 1, 2 DESC
            """,
        ),
    )

    show(
        "member: working tmpl vs first-on-mint",
        *q(
            cur,
            """
            SELECT
              working,
              wal_new,
              count(*)::bigint AS n,
              round(100.0 * count(*) / sum(count(*)) OVER (PARTITION BY working), 2) AS pct_in_row
            FROM ixg.bmem_new
            WHERE wallet_id IS NOT NULL
            GROUP BY 1, 2
            ORDER BY 1 DESC, 2 DESC
            """,
        ),
    )

    show(
        "kind x new_kind (all wallets)",
        *q(
            cur,
            """
            SELECT
              b.kind,
              n.new_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            GROUP BY 1, 2
            ORDER BY 1, 2
            """,
        ),
    )

    show(
        "kind x new_kind, tot in [0.9, 4)",
        *q(
            cur,
            """
            SELECT
              b.kind,
              n.new_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            WHERE b.tot >= 0.9 AND b.tot < 4
            GROUP BY 1, 2
            ORDER BY 1, 2
            """,
        ),
    )

    show(
        "same_tmpl_nwal working-heavy: new_kind_w (working wallets only)",
        *q(
            cur,
            """
            SELECT
              n.new_kind_w,
              n.nwal_new_w,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            WHERE b.kind = 'same_tmpl_nwal'
              AND b.tot >= 0.9 AND b.tot < 4
              AND n.nwal_new_w + n.nwal_rep_w >= 2
            GROUP BY 1, 2
            ORDER BY 1, 2
            """,
        ),
    )

    show(
        "same_tmpl_nwal tot[0.9,4): new_kind collapsed (ignore nwal_new_w grain)",
        *q(
            cur,
            """
            SELECT
              n.new_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            WHERE b.kind = 'same_tmpl_nwal'
              AND b.tot >= 0.9 AND b.tot < 4
            GROUP BY 1
            ORDER BY 1
            """,
        ),
    )

    show(
        "multi_tmpl_nwal tot[0.9,4): new_kind",
        *q(
            cur,
            """
            SELECT
              n.new_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            WHERE b.kind = 'multi_tmpl_nwal'
              AND b.tot >= 0.9 AND b.tot < 4
            GROUP BY 1
            ORDER BY 1
            """,
        ),
    )

    show(
        "solo tot[0.9,4): new vs repeat",
        *q(
            cur,
            """
            SELECT
              n.new_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            WHERE b.kind = 'solo'
              AND b.tot >= 0.9 AND b.tot < 4
            GROUP BY 1
            ORDER BY 1
            """,
        ),
    )

    show(
        "solo any size: new vs repeat",
        *q(
            cur,
            """
            SELECT
              n.new_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            WHERE b.kind = 'solo'
            GROUP BY 1
            ORDER BY 1
            """,
        ),
    )

    show(
        "solo + working + tot[0.3,4): new vs repeat",
        *q(
            cur,
            """
            SELECT
              n.new_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            JOIN ixg.bmem_new m ON m.mint = b.mint AND m.slot = b.slot
            WHERE b.kind = 'solo'
              AND m.working
              AND b.tot >= 0.3 AND b.tot < 4
            GROUP BY 1
            ORDER BY 1
            """,
        ),
    )

    show(
        "coverage of his quiet-resume fires (he1) by kind x new_kind",
        *q(
            cur,
            """
            SELECT
              b.kind,
              n.new_kind,
              count(*)::bigint AS hits,
              round(100.0 * count(*) / sum(count(*)) OVER (), 2) AS pct_of_hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            WHERE b.he1
            GROUP BY 1, 2
            ORDER BY count(*) DESC
            """,
        ),
    )

    show(
        "nwal_new inside same_tmpl_nwal tot[0.9,4)",
        *q(
            cur,
            """
            SELECT
              n.nwal_new,
              n.nwal_rep,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            WHERE b.kind = 'same_tmpl_nwal'
              AND b.tot >= 0.9 AND b.tot < 4
            GROUP BY 1, 2
            HAVING count(*) >= 50
            ORDER BY 1, 2
            """,
        ),
    )

    show(
        "same_tmpl_nwal tot[0.9,4) x has_ata x new_kind",
        *q(
            cur,
            """
            SELECT
              n.has_ata,
              n.new_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            WHERE b.kind = 'same_tmpl_nwal'
              AND b.tot >= 0.9 AND b.tot < 4
            GROUP BY 1, 2
            ORDER BY 1 DESC, 2
            """,
        ),
    )

    show(
        "age band x new_kind, tot[0.9,4), multi+same nwal",
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
              n.new_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            JOIN tokens t ON t.mint_address = b.mint
            CROSS JOIN LATERAL (
              SELECT EXTRACT(EPOCH FROM (
                (SELECT min(m.ts) FROM ixg.bmem m
                 WHERE m.mint = b.mint AND m.slot = b.slot)
                - t.created_at
              )) AS age_s
            ) a
            WHERE b.kind IN ('same_tmpl_nwal', 'multi_tmpl_nwal')
              AND b.tot >= 0.9 AND b.tot < 4
            GROUP BY 1, 2
            ORDER BY 1, 2
            """,
        ),
    )

    cur.execute(
        """
        SELECT 1 FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'ixg' AND c.relname = 'perm' AND c.relkind = 'r'
        """
    )
    if cur.fetchone() is None:
        print()
        print("ixg.perm missing; skip door/vsol cuts")
        conn.close()
        print()
        print("done")
        return

    show(
        "perm (door + named + tot band) x vsol x new_kind",
        *q(
            cur,
            """
            SELECT
              CASE
                WHEN p.vsol < 33 THEN 'lt33'
                WHEN p.vsol < 46 THEN '33-46'
                ELSE 'ge46'
              END AS vsol,
              n.new_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(p.he1::int), 2) AS resp,
              round(100.0 * avg(p.he_causal::int), 2) AS causal,
              sum(p.he1::int)::bigint AS hits
            FROM ixg.perm p
            JOIN ixg.bnew n ON n.mint = p.mint AND n.slot = p.slot
            GROUP BY 1, 2
            ORDER BY 1, 2
            """,
        ),
    )

    show(
        "perm vsol<46: kind x new_kind",
        *q(
            cur,
            """
            SELECT
              p.kind,
              n.new_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(p.he1::int), 2) AS resp,
              round(100.0 * avg(p.he_causal::int), 2) AS causal,
              sum(p.he1::int)::bigint AS hits
            FROM ixg.perm p
            JOIN ixg.bnew n ON n.mint = p.mint AND n.slot = p.slot
            WHERE p.vsol < 46
            GROUP BY 1, 2
            ORDER BY 1, 2
            """,
        ),
    )

    conn.close()
    print()
    print("done")


if __name__ == "__main__":
    main()
