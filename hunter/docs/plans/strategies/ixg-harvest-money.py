"""Price the harvest exit derived from the path map.

Fillable combined-machine events, 95 ms fill. Exits:
  clock     — 20 s from trigger (baseline harvest clock)
  trail     — arm 10 / trail 18, unarmed first-gap
  harvest_clock / harvest_trail — leave on dump/death after the first
    buy-gap; stay on two post-gap buys.

Cost: 125 bps/leg + own B/vsol. B = 0.10 SOL. One episode per mint except
the re-entry row.

Usage: DATABASE_URL in hunter/.env (never printed). Needs ixg.cm_cand.
"""
from __future__ import annotations

import sys
from pathlib import Path

import psycopg2

HUNTER_ENV = Path(__file__).resolve().parents[3] / ".env"


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


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    url = load_url()
    conn = psycopg2.connect(url)
    conn.autocommit = True
    cur = conn.cursor()
    cur.execute("SET statement_timeout = 0")
    cur.execute(
        """
        SELECT 1 FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'ixg' AND c.relkind = 'r' AND c.relname = 'cm_cand'
        """
    )
    if cur.fetchone() is None:
        print("missing ixg.cm_cand — run ixg-combined-money.py first")
        conn.close()
        return

    mod = load_walk()
    print("loading tape + events ...", flush=True)
    tape = mod.q(
        conn,
        """
        SELECT mint, slot, tx_index, ts, trade_type, px, vsol_lp
        FROM ixg.fall
        WHERE mint IN (SELECT DISTINCT mint FROM ixg.cm_cand WHERE fillable)
          AND px IS NOT NULL AND px > 0 AND vsol_lp IS NOT NULL AND vsol_lp > 0
        ORDER BY mint, slot, tx_index
        """,
    )
    ev = mod.q(
        conn,
        """
        SELECT e.mint, e.slot, e.tx_index, e.ts, e.fam, e.shape, e.fillable,
               e.this_tmpl, e.vsol_pre, e.created_at, e.trail,
               EXTRACT(EPOCH FROM (e.ts - e.created_at)) AS age_s,
               (h.mint IS NOT NULL) AS his
        FROM ixg.cm_cand e
        LEFT JOIN (SELECT DISTINCT mint FROM w8.buys) h ON h.mint = e.mint
        WHERE e.fillable
        ORDER BY e.mint, e.ts
        """,
    )
    conn.close()
    print(f"tape={len(tape)} ev={len(ev)} mints={ev['mint'].nunique()}", flush=True)
    tape_g = {m: g.reset_index(drop=True) for m, g in tape.groupby("mint", sort=False)}

    his_flag = ev["his"].fillna(False).astype(bool)
    books = {
        "fillable": ev,
        "his": ev[his_flag],
        "other": ev[~his_flag],
        "one": ev[ev["shape"] == "one"],
        "separated": ev[ev["shape"] == "separated"],
    }

    jobs = [
        ("fillable clock 95ms first", books["fillable"], 95, "clock", "first"),
        ("fillable trail 95ms first", books["fillable"], 95, "trail", "first"),
        ("fillable harvest_clock 95ms first", books["fillable"], 95, "harvest_clock", "first"),
        ("fillable harvest_trail 95ms first", books["fillable"], 95, "harvest_trail", "first"),
        ("fillable harvest_clock 95ms reentry", books["fillable"], 95, "harvest_clock", "reentry"),
        ("HIS clock 95ms first", books["his"], 95, "clock", "first"),
        ("HIS harvest_clock 95ms first", books["his"], 95, "harvest_clock", "first"),
        ("HIS harvest_trail 95ms first", books["his"], 95, "harvest_trail", "first"),
        ("OTHER harvest_clock 95ms first", books["other"], 95, "harvest_clock", "first"),
        ("one harvest_clock 95ms first", books["one"], 95, "harvest_clock", "first"),
        ("separated harvest_clock 95ms first", books["separated"], 95, "harvest_clock", "first"),
    ]

    for label, df, lag, mode, pol in jobs:
        print(f"\nrunning {label} n={len(df)}...", flush=True)
        if len(df) == 0:
            print(f"{label}: empty")
            continue
        out = mod.run_book(tape_g, df, lag, mode, pol)
        mod.summarize(out, label)


if __name__ == "__main__":
    main()
