"""Thermometer: first-on-mint wallets old-on-chain vs born this mint.

Reads DATABASE_URL from hunter/.env (never printed).
Requires ixg.bmem_new / burst / bnew / his_slot from new-wallet + burst builds.
"""
from __future__ import annotations

import sys
from pathlib import Path

import psycopg2

HUNTER_ENV = Path(__file__).resolve().parents[3] / ".env"
SQL_FILE = Path(__file__).with_name("ixg-old-wallets.sql")


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
          AND c.relname IN ('bmem_new', 'burst', 'bnew', 'his_slot', 'perm')
        """
    )
    have = {r[0] for r in cur.fetchall()}
    need = {"bmem_new", "burst", "bnew", "his_slot"}
    missing = need - have
    if missing:
        print("missing ixg tables:", sorted(missing))
        conn.close()
        return
    print("ixg tables ok:", sorted(have), flush=True)

    skip = "--skip-build" in sys.argv
    cur.execute("SELECT to_regclass(%s)", ("ixg.bold",))
    have_bold = cur.fetchone()[0] is not None
    if skip and have_bold:
        print("skip-build: ixg.bold present", flush=True)
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
    cur.execute("SELECT count(*) FROM ixg.ow_ids")
    print("ow_ids", cur.fetchone()[0], flush=True)
    cur.execute("SELECT count(*) FILTER (WHERE ts0 IS NOT NULL), count(*) FROM ixg.ow_fs")
    r = cur.fetchone()
    print("ow_fs with ts0", r[0], "of", r[1], flush=True)
    cur.execute("SELECT count(*) FROM ixg.bold")
    print("bold", cur.fetchone()[0], flush=True)

    show(
        "member grain: wal_new x wal_old x wal_born x wal_pre",
        *q(
            cur,
            """
            SELECT
              wal_new, wal_old, wal_born, wal_pre,
              count(*)::bigint AS n,
              count(DISTINCT wallet_id)::bigint AS nwal
            FROM ixg.bmem_old
            WHERE wallet_id IS NOT NULL
            GROUP BY 1, 2, 3, 4
            ORDER BY 1 DESC, 2 DESC, 3 DESC, 4 DESC
            """,
        ),
    )

    show(
        "wal_new members: first-buy slot vs this slot (old = earlier slot)",
        *q(
            cur,
            """
            SELECT
              CASE
                WHEN wal_slot0 IS NULL THEN 'no_fs'
                WHEN wal_slot0 < slot THEN 'earlier'
                WHEN wal_slot0 = slot THEN 'same_slot'
                ELSE 'later'
              END AS fs_vs_this,
              wal_old, wal_born,
              count(*)::bigint AS n
            FROM ixg.bmem_old
            WHERE wal_new AND wallet_id IS NOT NULL
            GROUP BY 1, 2, 3
            ORDER BY 1, 2 DESC, 3 DESC
            """,
        ),
    )

    show(
        "kind x age_kind, tot in [0.9, 4)",
        *q(
            cur,
            """
            SELECT
              b.kind,
              o.age_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bold o USING (mint, slot)
            WHERE b.tot >= 0.9 AND b.tot < 4
            GROUP BY 1, 2
            ORDER BY 1, 2
            """,
        ),
    )

    show(
        "all_new only (live cell): kind x age_kind, tot [0.9, 4)",
        *q(
            cur,
            """
            SELECT
              b.kind,
              o.age_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            JOIN ixg.bold o USING (mint, slot)
            WHERE b.tot >= 0.9 AND b.tot < 4
              AND n.new_kind = 'all_new'
            GROUP BY 1, 2
            ORDER BY 1, 2
            """,
        ),
    )

    show(
        "all_new tot[0.9,4): origin_kind (pre vs hop vs born)",
        *q(
            cur,
            """
            SELECT
              b.kind,
              o.origin_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            JOIN ixg.bold o USING (mint, slot)
            WHERE b.tot >= 0.9 AND b.tot < 4
              AND n.new_kind = 'all_new'
            GROUP BY 1, 2
            ORDER BY 1, 2
            """,
        ),
    )

    show(
        "same_tmpl_nwal tot[0.9,4) all_new: working-wallet age_kind_w",
        *q(
            cur,
            """
            SELECT
              o.age_kind_w,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            JOIN ixg.bold o USING (mint, slot)
            WHERE b.kind = 'same_tmpl_nwal'
              AND b.tot >= 0.9 AND b.tot < 4
              AND n.new_kind = 'all_new'
            GROUP BY 1
            ORDER BY 1
            """,
        ),
    )

    show(
        "solo tot[0.9,4) all_new: old vs born (and working)",
        *q(
            cur,
            """
            SELECT
              o.age_kind,
              m.working,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            JOIN ixg.bold o USING (mint, slot)
            JOIN ixg.bmem_old m ON m.mint = b.mint AND m.slot = b.slot
            WHERE b.kind = 'solo'
              AND b.tot >= 0.9 AND b.tot < 4
              AND n.new_kind = 'all_new'
            GROUP BY 1, 2
            ORDER BY 1, 2 DESC
            """,
        ),
    )

    show(
        "all_new tot[0.9,4): strict born (first-buy slot = this slot) vs old",
        *q(
            cur,
            """
            SELECT
              b.kind,
              CASE
                WHEN s.n_old > 0 AND s.n_strict_born = 0 AND s.n_unk = 0 THEN 'all_old'
                WHEN s.n_old = 0 AND s.n_strict_born > 0 AND s.n_unk = 0 THEN 'all_strict_born'
                WHEN s.n_old > 0 AND s.n_strict_born > 0 THEN 'mixed'
                WHEN s.n_unk > 0 THEN 'has_unk_fs'
                ELSE 'other'
              END AS strict_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            JOIN (
              SELECT
                mint, slot,
                count(DISTINCT wallet_id) FILTER (
                  WHERE wal_new AND wal_slot0 IS NOT NULL AND wal_slot0 < slot
                )::int AS n_old,
                count(DISTINCT wallet_id) FILTER (
                  WHERE wal_new AND wal_slot0 = slot
                )::int AS n_strict_born,
                count(DISTINCT wallet_id) FILTER (
                  WHERE wal_new AND (wal_slot0 IS NULL OR wal_slot0 > slot)
                )::int AS n_unk
              FROM ixg.bmem_old
              GROUP BY mint, slot
            ) s USING (mint, slot)
            WHERE b.tot >= 0.9 AND b.tot < 4
              AND n.new_kind = 'all_new'
              AND b.kind IN ('solo', 'same_tmpl_nwal', 'multi_tmpl_nwal')
            GROUP BY 1, 2
            ORDER BY 1, 2
            """,
        ),
    )

    show(
        "wal_new member chain-age at the print (this.ts - wal_ts0)",
        *q(
            cur,
            """
            SELECT
              CASE
                WHEN wal_born THEN 'born'
                WHEN EXTRACT(EPOCH FROM (ts - wal_ts0)) < 3600 THEN 'lt1h'
                WHEN EXTRACT(EPOCH FROM (ts - wal_ts0)) < 86400 THEN '1h-1d'
                WHEN EXTRACT(EPOCH FROM (ts - wal_ts0)) < 7 * 86400 THEN '1-7d'
                ELSE 'ge7d'
              END AS chain_age,
              count(*)::bigint AS n,
              count(*) FILTER (WHERE working)::bigint AS n_work
            FROM ixg.bmem_old
            WHERE wal_new AND wallet_id IS NOT NULL AND wal_ts0 IS NOT NULL
            GROUP BY 1
            ORDER BY 1
            """,
        ),
    )

    show(
        "all_new tot[0.9,4) multi+same: chain-age of youngest new wallet",
        *q(
            cur,
            """
            SELECT
              b.kind,
              CASE
                WHEN EXTRACT(EPOCH FROM (mn_age)) < 1 THEN 'born'
                WHEN EXTRACT(EPOCH FROM (mn_age)) < 3600 THEN 'lt1h'
                WHEN EXTRACT(EPOCH FROM (mn_age)) < 86400 THEN '1h-1d'
                WHEN EXTRACT(EPOCH FROM (mn_age)) < 7 * 86400 THEN '1-7d'
                ELSE 'ge7d'
              END AS min_chain_age,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            JOIN LATERAL (
              SELECT min(m.ts - m.wal_ts0) AS mn_age
              FROM ixg.bmem_old m
              WHERE m.mint = b.mint AND m.slot = b.slot AND m.wal_new
            ) a ON true
            WHERE b.kind IN ('same_tmpl_nwal', 'multi_tmpl_nwal')
              AND b.tot >= 0.9 AND b.tot < 4
              AND n.new_kind = 'all_new'
            GROUP BY 1, 2
            ORDER BY 1, 2
            """,
        ),
    )

    show(
        "coverage of his quiet-resume fires (he1) by kind x new_kind x age_kind",
        *q(
            cur,
            """
            SELECT
              b.kind,
              n.new_kind,
              o.age_kind,
              count(*)::bigint AS hits,
              round(100.0 * count(*) / sum(count(*)) OVER (), 2) AS pct_of_hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            JOIN ixg.bold o USING (mint, slot)
            WHERE b.he1
            GROUP BY 1, 2, 3
            ORDER BY count(*) DESC
            """,
        ),
    )

    show(
        "same_tmpl_nwal tot[0.9,4) all_new: ATA x age_kind",
        *q(
            cur,
            """
            SELECT
              n.has_ata,
              o.age_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            JOIN ixg.bold o USING (mint, slot)
            WHERE b.kind = 'same_tmpl_nwal'
              AND b.tot >= 0.9 AND b.tot < 4
              AND n.new_kind = 'all_new'
            GROUP BY 1, 2
            ORDER BY 1 DESC, 2
            """,
        ),
    )

    show(
        "token age band x age_kind, all_new tot[0.9,4) multi+same",
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
              END AS tok_age,
              o.age_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(b.he1::int), 2) AS resp,
              round(100.0 * avg(b.he_causal::int), 2) AS causal,
              sum(b.he1::int)::bigint AS hits
            FROM ixg.burst b
            JOIN ixg.bnew n USING (mint, slot)
            JOIN ixg.bold o USING (mint, slot)
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
              AND n.new_kind = 'all_new'
            GROUP BY 1, 2
            ORDER BY 1, 2
            """,
        ),
    )

    show(
        "tape-start censor: first-buy date of wal_new wallets",
        *q(
            cur,
            """
            SELECT
              (wal_ts0::date = DATE '2026-07-28') AS on_tape_day0,
              wal_old, wal_born,
              count(DISTINCT wallet_id)::bigint AS nwal
            FROM ixg.bmem_old
            WHERE wal_new AND wal_ts0 IS NOT NULL
            GROUP BY 1, 2, 3
            ORDER BY 1 DESC, 2 DESC, 3 DESC
            """,
        ),
    )

    if "perm" not in have:
        print()
        print("ixg.perm missing; skip door/vsol cuts")
        conn.close()
        print()
        print("done")
        return

    show(
        "perm vsol<46 all_new: kind x age_kind",
        *q(
            cur,
            """
            SELECT
              p.kind,
              o.age_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(p.he1::int), 2) AS resp,
              round(100.0 * avg(p.he_causal::int), 2) AS causal,
              sum(p.he1::int)::bigint AS hits
            FROM ixg.perm p
            JOIN ixg.bnew n ON n.mint = p.mint AND n.slot = p.slot
            JOIN ixg.bold o ON o.mint = p.mint AND o.slot = p.slot
            WHERE p.vsol < 46
              AND n.new_kind = 'all_new'
            GROUP BY 1, 2
            ORDER BY 1, 2
            """,
        ),
    )

    show(
        "perm vsol<46 all_new: kind x origin_kind",
        *q(
            cur,
            """
            SELECT
              p.kind,
              o.origin_kind,
              count(*)::bigint AS n,
              round(100.0 * avg(p.he1::int), 2) AS resp,
              round(100.0 * avg(p.he_causal::int), 2) AS causal,
              sum(p.he1::int)::bigint AS hits
            FROM ixg.perm p
            JOIN ixg.bnew n ON n.mint = p.mint AND n.slot = p.slot
            JOIN ixg.bold o ON o.mint = p.mint AND o.slot = p.slot
            WHERE p.vsol < 46
              AND n.new_kind = 'all_new'
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
