"""Price the solo-turn cell on the full tape.

Turn = solo_new_work (already door + gap + tot[0.9,4) + vsol<46) with
trail >= 15 behind the completing print. Control = same book with trail < 15.

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
SQL_FILE = Path(__file__).with_name("ixg-solo-turn-money.sql")


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
    cur.execute("SET work_mem = '2GB'")
    cur.execute("SET synchronous_commit = off")

    cur.execute(
        """
        SELECT c.relname FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'ixg' AND c.relkind = 'r'
          AND c.relname IN ('fall', 'ncand', 'tcand')
        """
    )
    have = {r[0] for r in cur.fetchall()}
    missing = {"fall", "ncand"} - have
    if missing:
        print("missing ixg tables:", sorted(missing))
        print("rebuild with ixg-new-money.py first")
        conn.close()
        return
    print("ixg tables ok:", sorted(have), flush=True)

    skip = "--skip-build" in sys.argv
    if skip and "tcand" in have:
        print("skip-build: ixg.tcand present", flush=True)
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
        SELECT
          count(*)::bigint,
          count(*) FILTER (WHERE trail IS NULL)::bigint,
          count(*) FILTER (WHERE COALESCE(trail, 0) >= 15)::bigint,
          count(*) FILTER (WHERE COALESCE(trail, 0) < 15)::bigint,
          count(*) FILTER (
            WHERE trail >= 30 AND trail < 60 AND n_sell_gap > 0
          )::bigint,
          count(*) FILTER (
            WHERE trail >= 15 AND trail < 30 AND n_sell_gap = 0
          )::bigint
        FROM ixg.tcand
        """
    )
    n, n_unk, n_turn, n_nodip, n_deep, n_mod = cur.fetchone()
    print(
        f"tcand n={n} trail_unk={n_unk} turn={n_turn} no_dip={n_nodip} "
        f"deep_gap={n_deep} mod_quiet={n_mod}",
        flush=True,
    )

    mod = load_walk()
    print("loading tape + events ...", flush=True)
    tape = mod.q(
        conn,
        """
        SELECT mint, slot, tx_index, ts, trade_type, px, vsol_lp
        FROM ixg.fall
        WHERE mint IN (SELECT DISTINCT mint FROM ixg.tcand)
          AND px IS NOT NULL AND vsol_lp IS NOT NULL AND vsol_lp > 0
        ORDER BY mint, slot, tx_index
        """,
    )
    ev = mod.q(
        conn,
        """
        SELECT e.mint, e.slot, e.tx_index, e.ts, e.fam, e.working,
               e.this_tmpl, e.vsol_pre, e.created_at, e.trail,
               e.n_sell_gap, e.last_side,
               EXTRACT(EPOCH FROM (e.ts - e.created_at)) AS age_s,
               (h.mint IS NOT NULL) AS his
        FROM ixg.tcand e
        LEFT JOIN (SELECT DISTINCT mint FROM w8.buys) h ON h.mint = e.mint
        ORDER BY e.mint, e.ts
        """,
    )
    conn.close()
    print(f"tape={len(tape)} ev={len(ev)} mints={ev['mint'].nunique()}", flush=True)
    tape_g = {m: g.reset_index(drop=True) for m, g in tape.groupby("mint", sort=False)}

    trail = ev["trail"]
    gap = ev["n_sell_gap"].fillna(0).astype(int)
    books = {
        "turn": ev[trail.fillna(0) >= 15],
        "no_dip": ev[trail.fillna(0) < 15],
        "turn_deep": ev[(trail >= 30) & (trail < 60) & (gap > 0)],
        "turn_mod": ev[(trail >= 15) & (trail < 30) & (gap == 0)],
    }
    turn_his_flag = books["turn"]["his"].fillna(False).astype(bool)

    jobs = []
    for name in ("turn", "no_dip", "turn_deep", "turn_mod"):
        df = books[name]
        jobs.append((f"{name} gap 0ms first", df, 0, "gap", "first"))
        jobs.append((f"{name} gap 95ms first", df, 95, "gap", "first"))
    jobs.append(("turn clock 0ms first", books["turn"], 0, "clock", "first"))
    jobs.append(("turn HIS gap 95ms", books["turn"][turn_his_flag], 95, "gap", "first"))
    jobs.append(("turn OTHER gap 95ms", books["turn"][~turn_his_flag], 95, "gap", "first"))

    for label, df, lag, mode, pol in jobs:
        print(f"running {label} n={len(df)}...", flush=True)
        if len(df) == 0:
            print(f"{label}: empty")
            continue
        out = mod.run_book(tape_g, df, lag, mode, pol)
        mod.summarize(out, label)


if __name__ == "__main__":
    main()
