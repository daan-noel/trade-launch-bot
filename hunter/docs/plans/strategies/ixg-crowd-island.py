"""Same harvest island on every crowd shape.

Gap-then-crowd (same-template or mixed, hole or tight pack), not solos.
Fire = completing print. Fill = last print with ts <= fire + 95 ms.
Land after a tight pack is the fill, not a middle-of-bundle scalp.

Island cut, live at the completing print:
  trail >= 15  (already off peak)
  age  >= 20 s (not brand-new)
  vsol_pre < 46 and not-all-repeat are already in cm_cand.

Exits: arm_death 8 (tape), clock 20 (probe), first-gap (scalp contrast).
Cost = 125 bps/leg + own B/vsol at B = 0.10 SOL.
One episode per mint except the re-entry row.

Usage: DATABASE_URL in hunter/.env (never printed). Needs ixg.cm_cand.
  python ixg-crowd-island.py
"""
from __future__ import annotations

import sys
from pathlib import Path

import pandas as pd
import psycopg2

HUNTER_ENV = Path(__file__).resolve().parents[3] / ".env"
LAG_MS = 95
CROWD = ("separated", "bundle", "mixed_gap", "mixed_tight")


def load_url() -> str:
    for line in HUNTER_ENV.read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s.startswith("DATABASE_URL=") and not s.startswith("#"):
            return s.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit("DATABASE_URL missing in hunter/.env")


def load_mod(name, file_name):
    import importlib.util

    path = str(Path(__file__).with_name(file_name))
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    cell = load_mod("ixg_cell_path", "ixg-cell-path.py")
    walk = load_mod("ixg_honest_exit", "ixg-honest-exit.py")
    hpath = load_mod("ixg_harvest_path", "ixg-harvest-path.py")
    path_one = hpath._bind_path_one(walk.fill_idx)
    hpath.selftest(walk.fill_idx, path_one)
    cell.selftest_exits(walk)

    url = load_url()
    conn = psycopg2.connect(url)
    conn.autocommit = True
    cur = conn.cursor()
    cur.execute("SET statement_timeout = 0")
    cur.execute("SET work_mem = '64MB'")
    cur.execute("SET max_parallel_workers_per_gather = 0")
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
    print("ixg.cm_cand present", flush=True)

    print("loading crowd events ...", flush=True)
    ev = walk.q(
        conn,
        """
        SELECT mint, slot, tx_index, ts, shape, trail, created_at, tight,
               EXTRACT(EPOCH FROM (ts - created_at)) AS age_s
        FROM ixg.cm_cand
        WHERE shape IN ('separated', 'bundle', 'mixed_gap', 'mixed_tight')
        ORDER BY mint, ts
        """,
    )
    ev["trail"] = pd.to_numeric(ev["trail"], errors="coerce")
    ev["age_s"] = pd.to_numeric(ev["age_s"], errors="coerce")
    ev["ts"] = pd.to_datetime(ev["ts"], utc=True).dt.tz_localize(None)
    print(f"crowd ev={len(ev)} mints={ev['mint'].nunique()}", flush=True)
    print("\n== n by shape (all crowd / trail>=15 / island)", flush=True)
    print(f"{'shape':<14} {'all':>8} {'trail>=15':>10} {'island':>8} {'1/mint':>8}")
    island = ev[(ev["trail"] >= 15) & (ev["age_s"] >= 20)]
    for sh in CROWD:
        a = ev[ev["shape"] == sh]
        t = a[a["trail"] >= 15]
        i = island[island["shape"] == sh]
        print(
            f"{sh:<14} {len(a):8d} {len(t):10d} {len(i):8d} "
            f"{len(cell.first_per_mint(i)) if len(i) else 0:8d}"
        )
    print(
        f"{'ALL CROWD':<14} {len(ev):8d} {len(ev[ev['trail']>=15]):10d} "
        f"{len(island):8d} {len(cell.first_per_mint(island)):8d}"
    )
    if island.empty:
        print("empty island")
        conn.close()
        return

    print("loading tape ...", flush=True)
    mints = tuple(island["mint"].unique())
    tape = walk.q(
        conn,
        """
        SELECT mint, slot, tx_index, ts, trade_type, px, vsol_lp, sol_lp
        FROM ixg.fall
        WHERE mint IN (
            SELECT DISTINCT mint FROM ixg.cm_cand
            WHERE shape IN ('separated', 'bundle', 'mixed_gap', 'mixed_tight')
              AND trail >= 15
              AND EXTRACT(EPOCH FROM (ts - created_at)) >= 20
        )
          AND px IS NOT NULL AND px > 0 AND vsol_lp IS NOT NULL AND vsol_lp > 0
        ORDER BY mint, slot, tx_index
        """,
    )
    conn.close()
    tape["ts"] = pd.to_datetime(tape["ts"], utc=True).dt.tz_localize(None)
    print(f"tape={len(tape)} island_mints={len(mints)}", flush=True)
    tape_g = {m: g.reset_index(drop=True) for m, g in tape.groupby("mint", sort=False)}

    books = {"ALL CROWD": cell.first_per_mint(island)}
    for sh in CROWD:
        sub = island[island["shape"] == sh]
        if len(sub):
            books[sh] = cell.first_per_mint(sub)

    for name, df in books.items():
        print(f"\n--- path {name} n={len(df)} ---", flush=True)
        path = hpath.map_book(
            tape_g, df, LAG_MS, path_one, walk.index_events, policy="first"
        )
        cell.show_path(path, f"{name} first 95ms")
        if not path.empty:
            cell.show_split(path, "after")
            cell.show_split(path, "peak_where")
            path["armed"] = path["t_arm"].notna()
            cell.show_split(path, "armed")

        jobs = [
            ("arm_death 8", "arm_death", None, 8),
            ("clock 20", "clock", None, None),
        ]
        if name == "ALL CROWD":
            jobs.append(("gap", "gap", None, None))
        print(f"\n== money {name} 95ms first-per-mint", flush=True)
        for label, mode, clock_s, death_s in jobs:
            print(f"running {name} {label} n={len(df)}...", flush=True)
            out = walk.run_book(
                tape_g, df, LAG_MS, mode, "first",
                clock_s=clock_s, death_s=death_s,
            )
            cell.fmt_book(out, f"{name} {label}")

    print("\n== money ALL CROWD re-entry 95ms", flush=True)
    print(f"running reentry arm_death 8 n={len(island)}...", flush=True)
    out = walk.run_book(
        tape_g, island, LAG_MS, "arm_death", "reentry", death_s=8,
    )
    cell.fmt_book(out, "ALL CROWD reentry arm_death 8")


if __name__ == "__main__":
    main()
