"""Remaining first-gap books after same_work 0/95 already printed."""
from __future__ import annotations

import os
import sys

import psycopg2


def load_walk():
    import importlib.util
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "ixg-honest-exit.py")
    spec = importlib.util.spec_from_file_location("ixg_honest_exit", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    mod = load_walk()
    conn = psycopg2.connect(os.environ["DATABASE_URL"])
    print("loading...", flush=True)
    tape = mod.q(
        conn,
        """
        SELECT mint, slot, tx_index, ts, trade_type, px, vsol_lp
        FROM ixg.fall
        WHERE mint IN (SELECT DISTINCT mint FROM ixg.fcand
                       WHERE book IN ('mixed','same_dead','onewal','same_work'))
          AND px IS NOT NULL AND vsol_lp IS NOT NULL AND vsol_lp > 0
        ORDER BY mint, slot, tx_index
        """,
    )
    ev = mod.q(
        conn,
        """
        SELECT e.mint, e.slot, e.tx_index, e.ts, e.book,
               EXTRACT(EPOCH FROM (e.ts - e.created_at)) AS age_s,
               (h.mint IS NOT NULL) AS his
        FROM ixg.fcand e
        LEFT JOIN (SELECT DISTINCT mint FROM w8.buys) h ON h.mint = e.mint
        ORDER BY e.mint, e.ts
        """,
    )
    conn.close()
    print(f"tape={len(tape)} ev={len(ev)}", flush=True)
    tape_g = {m: g.reset_index(drop=True) for m, g in tape.groupby("mint", sort=False)}
    his = ev["his"].fillna(False).astype(bool)
    jobs = [
        ("mixed gap 0ms first", ev[ev.book == "mixed"], 0, "gap"),
        ("mixed gap 95ms first", ev[ev.book == "mixed"], 95, "gap"),
        ("same_dead gap 95ms first", ev[ev.book == "same_dead"], 95, "gap"),
        ("onewal gap 95ms first", ev[ev.book == "onewal"], 95, "gap"),
        ("same_work HIS gap 95ms", ev[(ev.book == "same_work") & his], 95, "gap"),
        ("same_work OTHER gap 95ms", ev[(ev.book == "same_work") & ~his], 95, "gap"),
        ("same_work age<180 gap 95ms", ev[(ev.book == "same_work") & (ev.age_s < 180)], 95, "gap"),
    ]
    for label, df, lag, mode in jobs:
        print(f"running {label} n={len(df)}...", flush=True)
        out = mod.run_book(tape_g, df, lag, mode, "first")
        mod.summarize(out, label)


if __name__ == "__main__":
    main()
