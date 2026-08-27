"""Gap-length gate + early fire (first working new print after quiet).

Live fire = print 1 of the resume slot, working + first-on-mint, vsol<46.
Gap duration is a permission band. Later-slot shape is diagnostic only.

Fill: last print with ts <= fire + lag_ms. Exits: first-gap, armed trail,
clock 4s. Cost: 125 bps/leg + own B/vsol. B = 0.10 SOL. One episode per mint.

Usage: DATABASE_URL in hunter/.env (never printed).
"""
from __future__ import annotations

import sys
from pathlib import Path

import psycopg2

HUNTER_ENV = Path(__file__).resolve().parents[3] / ".env"
SQL_FILE = Path(__file__).with_name("ixg-early-gap.sql")
NEED = ("dmint", "fall", "fbuy", "nwal0", "nprev")


def load_url() -> str:
    for line in HUNTER_ENV.read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s.startswith("DATABASE_URL=") and not s.startswith("#"):
            return s.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit("DATABASE_URL missing in hunter/.env")


def load_walk():
    import importlib.util

    path = str(Path(__file__).with_name("ixg-honest-exit.py"))
    spec = importlib.util.spec_from_file_location("ixg_honest_exit", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


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


def show(cur, title: str, sql: str):
    print(f"\n== {title}", flush=True)
    cur.execute(sql)
    cols = [d[0] for d in cur.description]
    rows = cur.fetchall()
    if not rows:
        print("(empty)")
        return
    widths = [len(c) for c in cols]
    str_rows = []
    for r in rows:
        cells = []
        for v in r:
            if v is None:
                s = ""
            elif isinstance(v, float):
                s = f"{100.0 * v:.2f}" if 0 <= v <= 1 else f"{v:.3f}"
            else:
                s = str(v)
            cells.append(s)
        str_rows.append(cells)
        for i, s in enumerate(cells):
            widths[i] = max(widths[i], len(s))
    fmt = "  ".join(f"{{:{w}}}" for w in widths)
    print(fmt.format(*cols))
    print(fmt.format(*["-" * w for w in widths]))
    for cells in str_rows:
        print(fmt.format(*cells))


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    url = load_url()
    conn = psycopg2.connect(url)
    conn.autocommit = True
    cur = conn.cursor()
    cur.execute("SET statement_timeout = 0")
    cur.execute("SET work_mem = '64MB'")
    cur.execute("SET max_parallel_workers_per_gather = 0")
    cur.execute("SET synchronous_commit = off")

    cur.execute(
        """
        SELECT c.relname FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'ixg' AND c.relkind = 'r'
          AND c.relname = ANY(%s)
        """,
        (list(NEED),),
    )
    have = {r[0] for r in cur.fetchall()}
    missing = set(NEED) - have
    if missing:
        print("missing ixg tables:", sorted(missing))
        conn.close()
        return
    print("ixg tables ok:", sorted(have), flush=True)

    skip = "--skip-build" in sys.argv
    cur.execute(
        """
        SELECT 1 FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'ixg' AND c.relkind = 'r' AND c.relname = 'eg_cand'
        """
    )
    have_cand = cur.fetchone() is not None
    if skip and have_cand:
        print("skip-build: ixg.eg_cand present", flush=True)
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

    show(
        cur,
        "counts by gap_band x later (vsol<46, working new print 1)",
        """
        SELECT gap_band, later, count(*)::bigint AS n,
               avg(he1::int) AS resp, avg(he_causal::int) AS causal
        FROM ixg.eg_cand
        GROUP BY 1, 2
        ORDER BY 1, 2
        """,
    )
    show(
        cur,
        "his-mint thermometer: gap_band (dslot>=5, sol [0.3,0.9))",
        """
        SELECT gap_band, count(*)::bigint AS n,
               avg(he1::int) AS resp, avg(he_causal::int) AS causal
        FROM ixg.eg_cand
        WHERE dslot >= 5 AND this_sol >= 0.3 AND this_sol < 0.9
          AND mint IN (SELECT DISTINCT mint FROM w8.buys)
        GROUP BY 1
        ORDER BY 1
        """,
    )
    show(
        cur,
        "his-mint thermometer: later shape (dslot>=5, sol [0.3,0.9))",
        """
        SELECT later, count(*)::bigint AS n,
               avg(he1::int) AS resp, avg(he_causal::int) AS causal
        FROM ixg.eg_cand
        WHERE dslot >= 5 AND this_sol >= 0.3 AND this_sol < 0.9
          AND mint IN (SELECT DISTINCT mint FROM w8.buys)
        GROUP BY 1
        ORDER BY resp DESC
        """,
    )
    show(
        cur,
        "his-mint thermometer: this_sol (dslot>=5)",
        """
        SELECT
          CASE
            WHEN this_sol < 0.3 THEN 'lt0.3'
            WHEN this_sol < 0.9 THEN '0.3-0.9'
            WHEN this_sol < 4 THEN '0.9-4'
            ELSE 'ge4'
          END AS sol_band,
          count(*)::bigint AS n,
          avg(he1::int) AS resp, avg(he_causal::int) AS causal
        FROM ixg.eg_cand
        WHERE dslot >= 5
          AND mint IN (SELECT DISTINCT mint FROM w8.buys)
        GROUP BY 1
        ORDER BY 1
        """,
    )
    show(
        cur,
        "his-mint thermometer: gap_band all sizes dslot>=2",
        """
        SELECT gap_band, count(*)::bigint AS n,
               avg(he1::int) AS resp, avg(he_causal::int) AS causal
        FROM ixg.eg_cand
        WHERE mint IN (SELECT DISTINCT mint FROM w8.buys)
        GROUP BY 1
        ORDER BY 1
        """,
    )

    mod = load_walk()
    print("\nloading tape + events ...", flush=True)
    tape = mod.q(
        conn,
        """
        SELECT mint, slot, tx_index, ts, trade_type, px, vsol_lp
        FROM ixg.fall
        WHERE mint IN (SELECT DISTINCT mint FROM ixg.eg_cand WHERE dslot >= 5)
          AND px IS NOT NULL AND px > 0 AND vsol_lp IS NOT NULL AND vsol_lp > 0
        ORDER BY mint, slot, tx_index
        """,
    )
    ev = mod.q(
        conn,
        """
        SELECT e.mint, e.slot, e.tx_index, e.ts, e.this_sol, e.this_tmpl,
               e.dslot, e.gap_band, e.later, e.vsol_pre, e.created_at,
               EXTRACT(EPOCH FROM (e.ts - e.created_at)) AS age_s,
               (h.mint IS NOT NULL) AS his
        FROM ixg.eg_cand e
        LEFT JOIN (SELECT DISTINCT mint FROM w8.buys) h ON h.mint = e.mint
        WHERE e.dslot >= 5
        ORDER BY e.mint, e.ts
        """,
    )
    conn.close()
    print(f"tape={len(tape)} ev={len(ev)} mints={ev['mint'].nunique()}", flush=True)
    tape_g = {m: g.reset_index(drop=True) for m, g in tape.groupby("mint", sort=False)}

    sol = ev["this_sol"]
    starter = ev[(sol >= 0.3) & (sol < 0.9)]
    big = ev[(sol >= 0.9) & (sol < 4)]
    oracle = starter[starter["later"] == "separated"]
    books = {
        "starter": starter,
        "oracle_sep": oracle,
        "big": big,
        "g5-9": starter[starter["gap_band"] == "5-9"],
        "g10-19": starter[starter["gap_band"] == "10-19"],
        "g20-39": starter[starter["gap_band"] == "20-39"],
        "g40+": starter[starter["gap_band"] == "40+"],
    }
    his_flag = books["starter"]["his"].fillna(False).astype(bool)

    jobs = [
        ("starter gap 0ms first", books["starter"], 0, "gap", "first", None),
        ("starter gap 95ms first", books["starter"], 95, "gap", "first", None),
        ("starter trail 0ms first", books["starter"], 0, "trail", "first", None),
        ("starter trail 95ms first", books["starter"], 95, "trail", "first", None),
        ("starter clock4 0ms first", books["starter"], 0, "clock", "first", 4),
        ("starter clock4 95ms first", books["starter"], 95, "clock", "first", 4),
        ("starter clock20 0ms first", books["starter"], 0, "clock", "first", 20),
        ("oracle_sep gap 0ms first", books["oracle_sep"], 0, "gap", "first", None),
        ("oracle_sep gap 95ms first", books["oracle_sep"], 95, "gap", "first", None),
        ("oracle_sep trail 95ms first", books["oracle_sep"], 95, "trail", "first", None),
        ("big gap 0ms first", books["big"], 0, "gap", "first", None),
        ("big gap 95ms first", books["big"], 95, "gap", "first", None),
        ("g5-9 gap 95ms first", books["g5-9"], 95, "gap", "first", None),
        ("g10-19 gap 95ms first", books["g10-19"], 95, "gap", "first", None),
        ("g20-39 gap 95ms first", books["g20-39"], 95, "gap", "first", None),
        ("g40+ gap 95ms first", books["g40+"], 95, "gap", "first", None),
        ("starter HIS gap 95ms", books["starter"][his_flag], 95, "gap", "first", None),
    ]

    for label, df, lag, mode, pol, clock_s in jobs:
        print(f"\nrunning {label} n={len(df)}...", flush=True)
        if len(df) == 0:
            print(f"{label}: empty")
            continue
        out = mod.run_book(tape_g, df, lag, mode, pol, clock_s=clock_s)
        mod.summarize(out, label)


if __name__ == "__main__":
    main()
