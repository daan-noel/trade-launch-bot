"""Map the path after a 95 ms fill. No exit, no money.

Same combined-machine events (door + 5-slot gap + vsol<46 + not-all-repeat
+ working completing print, crowd or turn). Fillable shapes only. Fill =
last print with ts <= fire + 95 ms. Marks are last-print gross return from
that fill. Peak location is vs the first 2-slot buy gap after the fill.

Usage: DATABASE_URL in hunter/.env (never printed).
  python ixg-harvest-path.py
  python ixg-harvest-path.py --skip-build
"""
from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
import pandas as pd
import psycopg2

HUNTER_ENV = Path(__file__).resolve().parents[3] / ".env"
LAG_MS = 95
SLOT_MS = 400
GAP_SLOTS = 2
HOLD_CAP_S = 300
MARKS_S = (1, 4, 8, 20, 60, 300)


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


def _bind_path_one(fill_idx):
    def path_one(times, slots, px, is_buy, sol, trig, lag_ms):
        n = len(times)
        ti, tts, tslot = trig
        if ti < 0 or ti >= n or px[ti] <= 0:
            return None
        eidx = fill_idx(times, tts, lag_ms, ti, n)
        epx = float(px[eidx])
        if epx <= 0:
            return None
        ets = times[eidx]
        eslot = int(slots[eidx])
        cap_at = ets + np.timedelta64(HOLD_CAP_S, "s")
        gap_delay = np.timedelta64(GAP_SLOTS * SLOT_MS, "ms")

        last_buy_ts = tts
        last_buy_i = ti
        if is_buy[eidx]:
            last_buy_ts = times[eidx]
            last_buy_i = eidx

        peak = epx
        peak_i = eidx
        trough = epx
        t_arm = None
        t_run50 = None
        t_run100 = None
        t_first_sell = None
        arm_lvl = epx * 1.10
        run50_lvl = epx * 1.50
        run100_lvl = epx * 2.00

        gap_fired = False
        first_gap_ts = None
        buy_sol_pre = 0.0
        sell_sol_pre = 0.0
        buy_sol_post = 0.0
        sell_sol_post = 0.0
        n_buy_post = 0
        n_sell_post = 0
        n_lag = int(eidx - ti)

        mark_i = {s: eidx for s in MARKS_S}

        for k in range(eidx, n):
            t = times[k]
            if t > cap_at:
                break
            dt_s = float((t - ets) / np.timedelta64(1, "s"))
            for s in MARKS_S:
                if dt_s <= s:
                    mark_i[s] = k
            p = float(px[k])
            if p > 0:
                if p > peak:
                    peak = p
                    peak_i = k
                if p < trough:
                    trough = p
                if t_arm is None and p >= arm_lvl:
                    t_arm = dt_s
                if t_run50 is None and p >= run50_lvl:
                    t_run50 = dt_s
                if t_run100 is None and p >= run100_lvl:
                    t_run100 = dt_s
            if k == eidx:
                continue
            if not gap_fired:
                gap_at = last_buy_ts + gap_delay
                if t > gap_at:
                    gap_fired = True
                    first_gap_ts = gap_at
            s_sol = float(sol[k]) if sol is not None else 0.0
            if is_buy[k]:
                last_buy_ts = t
                last_buy_i = k
                if gap_fired:
                    n_buy_post += 1
                    buy_sol_post += s_sol
                else:
                    buy_sol_pre += s_sol
            else:
                if t_first_sell is None:
                    t_first_sell = dt_s
                if gap_fired:
                    n_sell_post += 1
                    sell_sol_post += s_sol
                else:
                    sell_sol_pre += s_sol

        if not gap_fired:
            # continuous printing through the cap, or no later print
            if last_buy_i > eidx and times[n - 1] > last_buy_ts + gap_delay:
                first_gap_ts = last_buy_ts + gap_delay
                gap_fired = True
            elif last_buy_i == eidx and (times[min(eidx + 1, n - 1)] if n > eidx + 1 else cap_at) > last_buy_ts + gap_delay:
                first_gap_ts = last_buy_ts + gap_delay
                gap_fired = True

        peak_ts = times[peak_i]
        if peak_i == eidx:
            peak_where = "at_fill"
        elif first_gap_ts is not None and peak_ts <= first_gap_ts:
            peak_where = "pre_gap"
        elif first_gap_ts is not None and peak_ts > first_gap_ts:
            peak_where = "post_gap"
        else:
            peak_where = "run"

        if not gap_fired:
            after = "run"
        elif n_buy_post == 0 and n_sell_post == 0:
            after = "death"
        elif buy_sol_post > sell_sol_post and n_buy_post >= 2:
            after = "wave"
        elif sell_sol_post >= buy_sol_post and n_sell_post >= 1:
            after = "dump"
        else:
            after = "thin"

        def ret_at(i):
            p = float(px[i])
            return (p / epx - 1.0) if p > 0 else None

        out = {
            "fill_same_slot": eslot == int(tslot),
            "n_lag": n_lag,
            "mfe": peak / epx - 1.0,
            "mae": trough / epx - 1.0,
            "peak_s": float((peak_ts - ets) / np.timedelta64(1, "s")),
            "peak_where": peak_where,
            "peak_same_slot": int(slots[peak_i]) == eslot,
            "after": after,
            "t_arm": t_arm,
            "t_run50": t_run50,
            "t_run100": t_run100,
            "t_first_sell": t_first_sell,
            "first_gap_s": (
                float((first_gap_ts - ets) / np.timedelta64(1, "s"))
                if first_gap_ts is not None
                else None
            ),
            "n_buy_post": n_buy_post,
            "n_sell_post": n_sell_post,
            "buy_sol_post": buy_sol_post,
            "sell_sol_post": sell_sol_post,
        }
        for s in MARKS_S:
            out[f"r{s}"] = ret_at(mark_i[s])
        return out

    return path_one


def map_book(tape_g, ev, lag_ms, path_one, index_events, policy="first"):
    rows = []
    ev = ev.copy()
    ev["_i"] = index_events(tape_g, ev)
    for mint, grp in ev.groupby("mint", sort=False):
        t = tape_g.get(mint)
        if t is None:
            continue
        times = t["ts"].to_numpy(dtype="datetime64[ns]")
        slots = t["slot"].to_numpy()
        px = t["px"].to_numpy(dtype=np.float64)
        is_buy = t["trade_type"].to_numpy() == "buy"
        sol = (t["sol_lp"].to_numpy(dtype=np.float64) / 1e9) if "sol_lp" in t.columns else None
        last_exit = None
        for rec, idx in zip(grp.itertuples(index=False), grp["_i"].to_numpy()):
            if idx < 0:
                continue
            ts = np.datetime64(rec.ts, "ns")
            if policy == "first" and last_exit is not None:
                break
            if policy == "reentry" and last_exit is not None and ts < last_exit:
                continue
            got = path_one(
                times, slots, px, is_buy, sol,
                (int(idx), ts, int(rec.slot)), lag_ms,
            )
            if got is None:
                continue
            row = {
                "mint": mint,
                "ts": rec.ts,
                "shape": getattr(rec, "shape", None),
                "his": bool(getattr(rec, "his", False)),
            }
            row.update(got)
            rows.append(row)
            # path map has no exit; reentry skips only overlapping by cap
            last_exit = ts + np.timedelta64(HOLD_CAP_S, "s") if policy == "reentry" else ts
            if policy == "first":
                break
    if not rows:
        return pd.DataFrame()
    return pd.DataFrame(rows)


def pct(s, pred):
    s = s.dropna()
    if len(s) == 0:
        return float("nan")
    return float(pred(s).mean())


def show_marks(df, label):
    print(f"\n== {label}  n={len(df)} mints={df['mint'].nunique() if len(df) else 0}", flush=True)
    if df.empty:
        print("  empty")
        return
    cols = [f"r{s}" for s in MARKS_S] + ["mfe", "mae"]
    hdr = f"{'mark':<8} {'mean':>8} {'med':>8} {'p>0':>6} {'p>+10':>7} {'p>+50':>7} {'p>+100':>7} {'p<-10':>7}"
    print(hdr)
    print("-" * len(hdr))
    for c in cols:
        s = df[c].astype(float)
        print(
            f"  {c:<6} {100*s.mean():8.2f} {100*s.median():8.2f} "
            f"{100*(s>0).mean():6.1f} {100*(s>=0.10).mean():7.1f} "
            f"{100*(s>=0.50).mean():7.1f} {100*(s>=1.00).mean():7.1f} "
            f"{100*(s<=-0.10).mean():7.1f}"
        )
    print("  peak_where:", df["peak_where"].value_counts().to_dict())
    print("  after:", df["after"].value_counts().to_dict())
    print(f"  fill_same_slot={100*df['fill_same_slot'].mean():.1f}%  "
          f"peak_same_slot={100*df['peak_same_slot'].mean():.1f}%  "
          f"n_lag p50={df['n_lag'].median():.0f}")
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
    # among armed: was the +10% before or after the first gap?
    if "peak_where" in df.columns and len(armed):
        print("  armed peak_where:", armed["peak_where"].value_counts().to_dict())
    if len(run50):
        print("  runner50 peak_where:", run50["peak_where"].value_counts().to_dict())


def selftest(fill_idx, path_one):
    """Handmade paths. Fail loud."""
    t0 = np.datetime64("2026-08-11T00:00:00", "ns")

    def mk(offsets_ms, prices, buys, sols=None):
        times = np.array([t0 + np.timedelta64(int(m), "ms") for m in offsets_ms], dtype="datetime64[ns]")
        n = len(times)
        slots = np.array([int(m // SLOT_MS) for m in offsets_ms], dtype=np.int64)
        px = np.array(prices, dtype=np.float64)
        is_buy = np.array(buys, dtype=bool)
        sol = np.array(sols if sols is not None else [0.5] * n, dtype=np.float64)
        return times, slots, px, is_buy, sol

    # flat: every mark 0, peak at fill
    times, slots, px, is_buy, sol = mk(
        [0, 200, 1000, 4000, 20000],
        [1.0, 1.0, 1.0, 1.0, 1.0],
        [True, True, True, True, True],
    )
    g = path_one(times, slots, px, is_buy, sol, (0, times[0], int(slots[0])), 95)
    assert abs(g["r1"]) < 1e-12 and abs(g["r20"]) < 1e-12
    assert g["peak_where"] == "at_fill"
    assert g["t_arm"] is None

    # remainder of slot pops after the 95 ms fill, then dies
    times, slots, px, is_buy, sol = mk(
        [0, 90, 200, 5000, 20000],
        [1.0, 1.00, 1.25, 0.95, 0.90],
        [True, True, True, False, False],
    )
    g = path_one(times, slots, px, is_buy, sol, (0, times[0], int(slots[0])), 95)
    assert g["fill_same_slot"]
    assert abs(g["mfe"] - 0.25) < 1e-9
    assert g["r20"] < 0
    assert g["peak_where"] == "pre_gap"

    # quiet after fill, then a 10s second wave: peak post_gap, r20 high
    times, slots, px, is_buy, sol = mk(
        [0, 80, 2000, 10000, 12000, 20000],
        [1.0, 1.01, 0.98, 1.40, 1.55, 1.50],
        [True, True, False, True, True, True],
        [0.5, 0.5, 0.8, 1.0, 1.0, 0.5],
    )
    g = path_one(times, slots, px, is_buy, sol, (0, times[0], int(slots[0])), 95)
    assert g["r20"] > 0.4
    assert g["peak_where"] == "post_gap"
    assert g["t_arm"] is not None and g["t_arm"] > 1.0
    assert g["after"] == "wave"

    # dump after pause
    times, slots, px, is_buy, sol = mk(
        [0, 80, 2000, 4000, 8000],
        [1.0, 1.00, 0.85, 0.70, 0.60],
        [True, True, False, False, False],
        [0.5, 0.5, 1.5, 1.5, 1.5],
    )
    g = path_one(times, slots, px, is_buy, sol, (0, times[0], int(slots[0])), 95)
    assert g["r8"] < -0.2
    assert g["after"] == "dump"
    print("selftest ok", flush=True)


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    walk = load_walk()
    path_one = _bind_path_one(walk.fill_idx)
    selftest(walk.fill_idx, path_one)

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

    print("loading tape + events ...", flush=True)
    tape = walk.q(
        conn,
        """
        SELECT mint, slot, tx_index, ts, trade_type, px, vsol_lp, sol_lp
        FROM ixg.fall
        WHERE mint IN (SELECT DISTINCT mint FROM ixg.cm_cand WHERE fillable)
          AND px IS NOT NULL AND px > 0 AND vsol_lp IS NOT NULL AND vsol_lp > 0
        ORDER BY mint, slot, tx_index
        """,
    )
    ev = walk.q(
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

    print("mapping first-per-mint ...", flush=True)
    first = map_book(tape_g, ev, LAG_MS, path_one, walk.index_events, policy="first")
    show_marks(first, "fillable first 95ms")

    his = first[first["his"].fillna(False).astype(bool)]
    other = first[~first["his"].fillna(False).astype(bool)]
    show_marks(his, "fillable HIS first 95ms")
    show_marks(other, "fillable OTHER first 95ms")

    for shape in ("one", "separated", "mixed_gap"):
        show_marks(first[first["shape"] == shape], f"shape={shape} first 95ms")

    print("mapping reentry (non-overlap 300s) ...", flush=True)
    re = map_book(tape_g, ev, LAG_MS, path_one, walk.index_events, policy="reentry")
    show_marks(re, "fillable reentry 95ms")
    show_marks(
        re[re["his"].fillna(False).astype(bool)],
        "fillable HIS reentry 95ms",
    )

    # after-kind x r20 body
    print("\n== r20 by after-kind (first)", flush=True)
    if not first.empty:
        g = first.groupby("after")["r20"].agg(
            n="size", mean="mean", med="median",
            ppos=lambda s: (s > 0).mean(),
            p10=lambda s: (s >= 0.10).mean(),
        )
        print(g.to_string())

    print("\n== r20 by peak_where (first)", flush=True)
    if not first.empty:
        g = first.groupby("peak_where")["r20"].agg(
            n="size", mean="mean", med="median",
            ppos=lambda s: (s > 0).mean(),
            p10=lambda s: (s >= 0.10).mean(),
        )
        print(g.to_string())

    print("\n== HIS vs OTHER r20 (first)", flush=True)
    for name, d in (("HIS", his), ("OTHER", other)):
        if d.empty:
            continue
        s = d["r20"].astype(float)
        print(
            f"  {name}: n={len(d)} mean={100*s.mean():.2f}% med={100*s.median():.2f}% "
            f"p>0={100*(s>0).mean():.1f}% p>+10={100*(s>=0.10).mean():.1f}% "
            f"after={d['after'].value_counts().to_dict()}"
        )


if __name__ == "__main__":
    main()
