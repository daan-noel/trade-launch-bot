"""Price the combined live-gate machine, bundle vs fillable.

Conjunction: door + 5-slot gap + vsol<46 + not-all-repeat + working
completing print, crowd OR turn. Tight consecutive-tx_index packs are
the bundle (unfillable). Fillable = separated same-ix, mixed with a
tx_index hole, or one big print (turn).

Fill: last print with ts <= fire + lag_ms (fallback: the firing print).
Exit: first-gap (2 slots). Cost: 125 bps/leg + own B/vsol. B = 0.10 SOL.
One episode per mint per book.

Usage: DATABASE_URL in hunter/.env (never printed).
"""
from __future__ import annotations

import sys
from pathlib import Path

import psycopg2

HUNTER_ENV = Path(__file__).resolve().parents[3] / ".env"
SQL_FILE = Path(__file__).with_name("ixg-combined-money.sql")
NEED = ("dmint", "fall", "fbuy", "fquiet", "nwal0", "nprev", "tlag")


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
        print("rebuild with ixg-fulltape-money.sql + ixg-new-money.sql first")
        conn.close()
        return
    print("ixg tables ok:", sorted(have), flush=True)

    skip = "--skip-build" in sys.argv
    cur.execute(
        """
        SELECT 1 FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'ixg' AND c.relkind = 'r' AND c.relname = 'cm_cand'
        """
    )
    have_cand = cur.fetchone() is not None
    if skip and have_cand:
        print("skip-build: ixg.cm_cand present", flush=True)
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

    cur.execute(
        """
        SELECT shape, fillable, count(*)::bigint
        FROM ixg.cm_cand
        GROUP BY 1, 2
        ORDER BY 1
        """
    )
    print("cm_cand by shape:")
    for shape, fillable, n in cur.fetchall():
        print(f"  shape={shape} fillable={fillable} n={n}")

    mod = load_walk()
    print("loading tape + events ...", flush=True)
    tape = mod.q(
        conn,
        """
        SELECT mint, slot, tx_index, ts, trade_type, px, vsol_lp
        FROM ixg.fall
        WHERE mint IN (SELECT DISTINCT mint FROM ixg.cm_cand)
          AND px IS NOT NULL AND px > 0 AND vsol_lp IS NOT NULL AND vsol_lp > 0
        ORDER BY mint, slot, tx_index
        """,
    )
    ev = mod.q(
        conn,
        """
        SELECT e.mint, e.slot, e.tx_index, e.ts, e.fam, e.shape, e.fillable,
               e.this_tmpl, e.vsol_pre, e.created_at, e.trail, e.tight,
               EXTRACT(EPOCH FROM (e.ts - e.created_at)) AS age_s,
               (h.mint IS NOT NULL) AS his
        FROM ixg.cm_cand e
        LEFT JOIN (SELECT DISTINCT mint FROM w8.buys) h ON h.mint = e.mint
        ORDER BY e.mint, e.ts
        """,
    )
    conn.close()
    print(f"tape={len(tape)} ev={len(ev)} mints={ev['mint'].nunique()}", flush=True)
    tape_g = {m: g.reset_index(drop=True) for m, g in tape.groupby("mint", sort=False)}

    fillable = ev["fillable"].fillna(False).astype(bool)
    books = {
        "combined": ev,
        "fillable": ev[fillable],
        "unfillable": ev[~fillable],
        "one": ev[ev["shape"] == "one"],
        "separated": ev[ev["shape"] == "separated"],
        "mixed_gap": ev[ev["shape"] == "mixed_gap"],
        "bundle": ev[ev["shape"] == "bundle"],
        "mixed_tight": ev[ev["shape"] == "mixed_tight"],
    }
    fill_his = books["fillable"]["his"].fillna(False).astype(bool)

    jobs = [
        ("combined gap 0ms first", books["combined"], 0, "gap", "first"),
        ("combined gap 95ms first", books["combined"], 95, "gap", "first"),
        ("fillable gap 0ms first", books["fillable"], 0, "gap", "first"),
        ("fillable gap 95ms first", books["fillable"], 95, "gap", "first"),
        ("fillable gap 95ms reentry", books["fillable"], 95, "gap", "reentry"),
        ("fillable clock 0ms first", books["fillable"], 0, "clock", "first"),
        ("one gap 0ms first", books["one"], 0, "gap", "first"),
        ("one gap 95ms first", books["one"], 95, "gap", "first"),
        ("separated gap 0ms first", books["separated"], 0, "gap", "first"),
        ("separated gap 95ms first", books["separated"], 95, "gap", "first"),
        ("mixed_gap gap 0ms first", books["mixed_gap"], 0, "gap", "first"),
        ("mixed_gap gap 95ms first", books["mixed_gap"], 95, "gap", "first"),
        ("bundle gap 0ms first", books["bundle"], 0, "gap", "first"),
        ("bundle gap 95ms first", books["bundle"], 95, "gap", "first"),
        ("mixed_tight gap 0ms first", books["mixed_tight"], 0, "gap", "first"),
        ("mixed_tight gap 95ms first", books["mixed_tight"], 95, "gap", "first"),
        ("fillable HIS gap 95ms", books["fillable"][fill_his], 95, "gap", "first"),
        ("fillable OTHER gap 95ms", books["fillable"][~fill_his], 95, "gap", "first"),
    ]

    for label, df, lag, mode, pol in jobs:
        print(f"running {label} n={len(df)}...", flush=True)
        if len(df) == 0:
            print(f"{label}: empty")
            continue
        out = mod.run_book(tape_g, df, lag, mode, pol)
        mod.summarize(out, label)


if __name__ == "__main__":
    main()
