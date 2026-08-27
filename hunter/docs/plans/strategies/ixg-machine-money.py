"""Price the door + crossing-burst + vsol<46 machine on the full tape.

Fill: last print with ts <= fire + lag_ms (fallback: the crossing print).
Exit: first-gap (2 slots) or clock 20s. Cost: 125 bps/leg + own B/vsol impact.
B = 0.10 SOL. Policy: first event per mint.

Usage: DATABASE_URL in env.
"""
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
    url = os.environ["DATABASE_URL"]
    conn = psycopg2.connect(url)
    print("loading...", flush=True)
    tape = mod.q(
        conn,
        """
        SELECT mint, slot, tx_index, ts, trade_type, px, vsol_lp
        FROM ixg.fall
        WHERE mint IN (SELECT DISTINCT mint FROM ixg.fcand)
          AND px IS NOT NULL AND vsol_lp IS NOT NULL AND vsol_lp > 0
        ORDER BY mint, slot, tx_index
        """,
    )
    ev = mod.q(
        conn,
        """
        SELECT e.mint, e.slot, e.tx_index, e.ts, e.book, e.fam,
               e.this_tmpl, e.vsol_pre, e.created_at,
               EXTRACT(EPOCH FROM (e.ts - e.created_at)) AS age_s,
               (h.mint IS NOT NULL) AS his
        FROM ixg.fcand e
        LEFT JOIN (SELECT DISTINCT mint FROM w8.buys) h ON h.mint = e.mint
        ORDER BY e.mint, e.ts
        """,
    )
    conn.close()
    print(f"tape={len(tape)} ev={len(ev)} mints={ev['mint'].nunique()}", flush=True)
    tape_g = {m: g.reset_index(drop=True) for m, g in tape.groupby("mint", sort=False)}

    books = {
        "same_work": ev[ev["book"] == "same_work"],
        "mixed": ev[ev["book"] == "mixed"],
        "same_dead": ev[ev["book"] == "same_dead"],
        "onewal": ev[ev["book"] == "onewal"],
    }
    young = ev[(ev["book"] == "same_work") & (ev["age_s"] < 180)]
    books["same_work_age180"] = young

    jobs = []
    for name, df in books.items():
        if name in ("same_dead", "onewal"):
            jobs.append((f"{name} gap 95ms first", df, 95, "gap", "first"))
            continue
        jobs.append((f"{name} gap 0ms first", df, 0, "gap", "first"))
        jobs.append((f"{name} gap 95ms first", df, 95, "gap", "first"))
        jobs.append((f"{name} clock 0ms first", df, 0, "clock", "first"))
        jobs.append((f"{name} clock 95ms first", df, 95, "clock", "first"))

    his_sw = ev[(ev["book"] == "same_work") & (ev["his"].fillna(False).astype(bool))]
    oth_sw = ev[(ev["book"] == "same_work") & (~ev["his"].fillna(False).astype(bool))]
    jobs.append(("same_work HIS gap 95ms", his_sw, 95, "gap", "first"))
    jobs.append(("same_work OTHER gap 95ms", oth_sw, 95, "gap", "first"))

    for label, df, lag, mode, pol in jobs:
        print(f"running {label} n={len(df)}...", flush=True)
        out = mod.run_book(tape_g, df, lag, mode, pol)
        mod.summarize(out, label)


if __name__ == "__main__":
    main()
