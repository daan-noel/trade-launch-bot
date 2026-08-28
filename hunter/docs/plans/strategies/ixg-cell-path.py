"""Path + harvest-exit walk on the concentrated cell only.

Cell = fillable combined-machine, shape=separated, trail>=15, age>=20.
Fill = last print with ts <= fire + 95 ms. One episode per mint except
the re-entry rows. Cost = 125 bps/leg + own B/vsol at B = 0.10 SOL.

Path is gross (no fees). Money walks charge the cost model.
Exits are pre-committed from mechanism, not mined:
  clock 20 / 60  — 8dtx median hold, then the 60 s mark
  trail          — arm 10 / trail 18, unarmed first-gap (existing)
  trail_hold     — same trail, unarmed holds to cap
  death 8 / 20   — leave on that many seconds of buy silence
  arm_death 8    — trail once armed; unarmed dies on 8 s silence
  harvest_clock  — leave on dump/death after the 0.8 s gap (refuted here)
  gap            — first 2-slot buy silence (refuted here)

Usage: DATABASE_URL in hunter/.env (never printed). Needs ixg.cm_fact.
  python ixg-cell-path.py
"""
from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
import pandas as pd
import psycopg2

HUNTER_ENV = Path(__file__).resolve().parents[3] / ".env"
LAG_MS = 95
DAYS = 12


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


def first_per_mint(df):
    return df.sort_values(["mint", "ts"]).groupby("mint", sort=False, as_index=False).head(1)


def selftest_exits(walk):
    t0 = np.datetime64("2026-08-11T00:00:00", "ns")

    def mk(offsets_ms, prices, buys, vsols=None):
        times = np.array(
            [t0 + np.timedelta64(int(m), "ms") for m in offsets_ms],
            dtype="datetime64[ns]",
        )
        n = len(times)
        slots = np.array([int(m // 400) for m in offsets_ms], dtype=np.int64)
        px = np.array(prices, dtype=np.float64)
        is_buy = np.array(buys, dtype=bool)
        vsol = np.array(vsols if vsols is not None else [40.0] * n, dtype=np.float64)
        return times, slots, px, is_buy, vsol

    # never arms, trail_hold holds to cap (~300 s)
    times, slots, px, is_buy, vsol = mk(
        [0, 200, 2000, 8000, 20000, 310000],
        [1.0, 1.02, 1.01, 0.95, 0.90, 0.85],
        [True, True, True, False, False, False],
    )
    g = walk.walk_one(
        times, slots, px, vsol, is_buy, (0, times[0], int(slots[0])), 95, "trail_hold"
    )
    assert g is not None and g[2] == "cap" and g[1] > 250

    # arms then retraces 18% off peak -> trail
    times, slots, px, is_buy, vsol = mk(
        [0, 200, 2000, 5000, 8000],
        [1.0, 1.02, 1.20, 1.25, 1.00],
        [True, True, True, True, False],
    )
    g = walk.walk_one(
        times, slots, px, vsol, is_buy, (0, times[0], int(slots[0])), 95, "trail_hold"
    )
    assert g is not None and g[2] == "trail"

    # death 8 s: last buy at 1 s, then silence. Fire at 9 s; fill is last
    # known print (the 1 s buy) because the next print is at 12 s.
    times, slots, px, is_buy, vsol = mk(
        [0, 200, 1000, 12000, 20000],
        [1.0, 1.01, 1.02, 0.90, 0.80],
        [True, True, True, False, False],
    )
    g = walk.walk_one(
        times, slots, px, vsol, is_buy,
        (0, times[0], int(slots[0])), 95, "death", death_s=8,
    )
    assert g is not None and g[2] == "death"
    assert g[1] < 2.0

    # death 8 s with a sell inside the wait: fill that sell, not the last buy
    times, slots, px, is_buy, vsol = mk(
        [0, 200, 1000, 6000, 20000],
        [1.0, 1.01, 1.02, 0.80, 0.70],
        [True, True, True, False, False],
    )
    g = walk.walk_one(
        times, slots, px, vsol, is_buy,
        (0, times[0], int(slots[0])), 95, "death", death_s=8,
    )
    assert g is not None and g[2] == "death"
    assert 5.0 < g[1] < 8.0

    # continuous buys: death 8 s never fires, hits cap
    times, slots, px, is_buy, vsol = mk(
        [0] + [int(i * 2000) for i in range(1, 160)],
        [1.0] * 160,
        [True] * 160,
    )
    g = walk.walk_one(
        times, slots, px, vsol, is_buy,
        (0, times[0], int(slots[0])), 95, "death", death_s=8,
    )
    assert g is not None and g[2] == "cap"

    # arm_death: never arms, 8 s silence -> death
    times, slots, px, is_buy, vsol = mk(
        [0, 200, 1000, 12000],
        [1.0, 1.01, 1.02, 0.90],
        [True, True, True, False],
    )
    g = walk.walk_one(
        times, slots, px, vsol, is_buy,
        (0, times[0], int(slots[0])), 95, "arm_death", death_s=8,
    )
    assert g is not None and g[2] == "death"

    print("exit selftest ok", flush=True)


def show_path(df, label):
    print(f"\n== path {label}  n={len(df)} mints={df['mint'].nunique() if len(df) else 0}", flush=True)
    if df.empty:
        print("  empty")
        return
    marks = [c for c in ("r1", "r4", "r8", "r20", "r60", "r300", "mfe", "mae") if c in df.columns]
    hdr = f"{'mark':<8} {'mean':>8} {'med':>8} {'p>0':>6} {'p>+10':>7} {'p>+50':>7} {'p>+100':>7} {'p<-10':>7}"
    print(hdr)
    print("-" * len(hdr))
    for c in marks:
        s = df[c].astype(float)
        print(
            f"  {c:<6} {100*s.mean():8.2f} {100*s.median():8.2f} "
            f"{100*(s>0).mean():6.1f} {100*(s>=0.10).mean():7.1f} "
            f"{100*(s>=0.50).mean():7.1f} {100*(s>=1.00).mean():7.1f} "
            f"{100*(s<=-0.10).mean():7.1f}"
        )
    print("  peak_where:", df["peak_where"].value_counts().to_dict())
    print("  after:", df["after"].value_counts().to_dict())
    print(
        f"  fill_same_slot={100*df['fill_same_slot'].mean():.1f}%  "
        f"peak_same_slot={100*df['peak_same_slot'].mean():.1f}%  "
        f"n_lag p50={df['n_lag'].median():.0f}"
    )
    armed = df[df["t_arm"].notna()]
    run50 = df[df["t_run50"].notna()]
    run100 = df[df["t_run100"].notna()]
    print(
        f"  arm+10={100*len(armed)/len(df):.1f}% t_arm_p50="
        f"{armed['t_arm'].median() if len(armed) else float('nan'):.2f}s  "
        f"run+50={100*len(run50)/len(df):.1f}% t_p50="
        f"{run50['t_run50'].median() if len(run50) else float('nan'):.2f}s  "
        f"run+100={100*len(run100)/len(df):.1f}% t_p50="
        f"{run100['t_run100'].median() if len(run100) else float('nan'):.2f}s"
    )
    if "first_gap_s" in df.columns:
        g = df["first_gap_s"].dropna()
        print(
            f"  first_gap_s p50={g.median() if len(g) else float('nan'):.2f}s  "
            f"p>2s={100*(g>2).mean() if len(g) else float('nan'):.1f}%  "
            f"p>8s={100*(g>8).mean() if len(g) else float('nan'):.1f}%"
        )
    if "t_first_sell" in df.columns:
        s = df["t_first_sell"].dropna()
        print(
            f"  first_sell p50={s.median() if len(s) else float('nan'):.2f}s  "
            f"none={100*(df['t_first_sell'].isna().mean()):.1f}%"
        )
    ts = pd.to_datetime(df["ts"])
    oos = df[ts >= "2026-08-18"]
    if len(oos):
        s = oos["r20"].astype(float)
        print(
            f"  OOS r20 mean={100*s.mean():.2f}% med={100*s.median():.2f}% "
            f"p>0={100*(s>0).mean():.1f}%"
        )


def show_split(df, key, valcol="r20"):
    print(f"\n== {valcol} by {key}", flush=True)
    if df.empty or key not in df.columns:
        print("  empty")
        return
    g = df.groupby(key)[valcol].agg(
        n="size",
        mean="mean",
        med="median",
        ppos=lambda s: (s > 0).mean(),
        p10=lambda s: (s >= 0.10).mean(),
        p_m10=lambda s: (s <= -0.10).mean(),
    )
    print(g.to_string())


def fmt_book(df, label):
    if df is None or df.empty:
        print(f"{label}: empty")
        return
    n = len(df)
    mean = float(df["net"].mean())
    med = float(df["net"].median())
    win = float((df["net"] > 0).mean())
    sol = float(0.10 * df["net"].sum())
    hold = float(df["hold"].median())
    days = pd.to_datetime(df["ts"]).dt.date
    by = pd.DataFrame({"d": days, "net": df["net"]}).groupby("d")["net"].mean()
    npos = int((by > 0).sum())
    nd = int(len(by))
    ts = pd.to_datetime(df["ts"])
    is_ = df[ts < "2026-08-17"]
    oos = df[ts >= "2026-08-18"]
    reasons = df["reason"].value_counts().to_dict()
    print(
        f"{label}: n={n} ({n/DAYS:.0f}/d) mean={100*mean:.2f}% med={100*med:.2f}% "
        f"win={100*win:.1f}% sol={sol:.2f} hold_p50={hold:.1f}s days+={npos}/{nd} "
        f"reasons={reasons}"
    )
    if len(is_):
        print(
            f"  IS  n={len(is_)} mean={100*is_['net'].mean():.2f}% "
            f"sol={0.10*is_['net'].sum():.2f} days+="
            f"{int((is_.assign(d=pd.to_datetime(is_['ts']).dt.date).groupby('d')['net'].mean()>0).sum())}"
        )
    if len(oos):
        print(
            f"  OOS n={len(oos)} mean={100*oos['net'].mean():.2f}% "
            f"sol={0.10*oos['net'].sum():.2f} days+="
            f"{int((oos.assign(d=pd.to_datetime(oos['ts']).dt.date).groupby('d')['net'].mean()>0).sum())}"
        )
    print(
        "  by day:",
        " ".join(f"{i} {100*r:.1f}%" for i, r in by.items()),
    )


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    walk = load_mod("ixg_honest_exit", "ixg-honest-exit.py")
    hpath = load_mod("ixg_harvest_path", "ixg-harvest-path.py")
    path_one = hpath._bind_path_one(walk.fill_idx)
    hpath.selftest(walk.fill_idx, path_one)
    selftest_exits(walk)

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
        WHERE n.nspname = 'ixg' AND c.relkind = 'r' AND c.relname = 'cm_fact'
        """
    )
    if cur.fetchone() is None:
        print("missing ixg.cm_fact — run ixg-concentrate.py first")
        conn.close()
        return
    print("ixg.cm_fact present", flush=True)

    print("loading tape + cell ...", flush=True)
    tape = walk.q(
        conn,
        """
        SELECT mint, slot, tx_index, ts, trade_type, px, vsol_lp, sol_lp
        FROM ixg.fall
        WHERE mint IN (
            SELECT DISTINCT mint FROM ixg.cm_fact
            WHERE shape = 'separated' AND trail >= 15 AND age_s >= 20
        )
          AND px IS NOT NULL AND px > 0 AND vsol_lp IS NOT NULL AND vsol_lp > 0
        ORDER BY mint, slot, tx_index
        """,
    )
    ev = walk.q(
        conn,
        """
        SELECT mint, slot, tx_index, ts, shape, trail, age_s, he1
        FROM ixg.cm_fact
        WHERE shape = 'separated' AND trail >= 15 AND age_s >= 20
        ORDER BY mint, ts
        """,
    )
    conn.close()
    ev["trail"] = pd.to_numeric(ev["trail"], errors="coerce")
    ev["age_s"] = pd.to_numeric(ev["age_s"], errors="coerce")
    ev["he1"] = ev["he1"].fillna(False).astype(bool)
    ev["ts"] = pd.to_datetime(ev["ts"], utc=True).dt.tz_localize(None)
    tape["ts"] = pd.to_datetime(tape["ts"], utc=True).dt.tz_localize(None)
    print(f"tape={len(tape)} ev={len(ev)} mints={ev['mint'].nunique()}", flush=True)
    tape_g = {m: g.reset_index(drop=True) for m, g in tape.groupby("mint", sort=False)}

    first = first_per_mint(ev)
    print(f"first-per-mint={len(first)}", flush=True)

    print("mapping path ...", flush=True)
    path = hpath.map_book(tape_g, first, LAG_MS, path_one, walk.index_events, policy="first")
    show_path(path, "cell first 95ms")
    show_split(path, "after")
    show_split(path, "peak_where")
    if "t_arm" in path.columns:
        path["armed"] = path["t_arm"].notna()
        show_split(path, "armed")

    jobs = [
        ("clock 20", "clock", None, None),
        ("clock 60", "clock", 60, None),
        ("gap", "gap", None, None),
        ("trail (unarmed=gap)", "trail", None, None),
        ("trail_hold", "trail_hold", None, None),
        ("harvest_clock", "harvest_clock", None, None),
        ("death 8", "death", None, 8),
        ("death 20", "death", None, 20),
        ("arm_death 8", "arm_death", None, 8),
    ]

    print("\n== money first-per-mint 95ms", flush=True)
    priced = {}
    for label, mode, clock_s, death_s in jobs:
        print(f"\nrunning {label} n={len(first)}...", flush=True)
        out = walk.run_book(
            tape_g, first, LAG_MS, mode, "first",
            clock_s=clock_s, death_s=death_s,
        )
        priced[label] = out
        fmt_book(out, label)

    print("\n== money re-entry 95ms (non-overlap)", flush=True)
    re_jobs = [
        ("clock 20", "clock", None, None),
        ("trail_hold", "trail_hold", None, None),
        ("death 8", "death", None, 8),
        ("arm_death 8", "arm_death", None, 8),
    ]
    for label, mode, clock_s, death_s in re_jobs:
        print(f"\nrunning reentry {label} n={len(ev)}...", flush=True)
        out = walk.run_book(
            tape_g, ev, LAG_MS, mode, "reentry",
            clock_s=clock_s, death_s=death_s,
        )
        fmt_book(out, f"reentry {label}")


if __name__ == "__main__":
    main()
