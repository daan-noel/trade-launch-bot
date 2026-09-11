"""Hot-tape re-entry, loop [2]: fire the state on the WHOLE tape and book it at lag_115.

The event comes from cvx_hottape.py, where two hypotheses died and one survived a control:

  REFUTED  absorption (SOL bought per percent of price given up). Lift runs the WRONG way -
           1.39 in the lowest bin, 0.66 in the highest. On a constant-product curve there is
           no resting size to absorb into: windowed flow IS the price path, so a buy-per-fall
           ratio divides the signal out. The order-book intuition does not port to a curve.
  REFUTED  deceleration (the last 2 s of the fall against the 5 s pace). Lift 0.57 where the
           fall has stopped, 1.35-1.48 where it is still accelerating. They buy INTO the
           flush, not after it turns.
  SURVIVES buy-flow ACCELERATION - SOL bought in the last 5 s against the 5 s before it.
           Lift 2.15 in the top bin, and 2.16 with the node's own SOL removed from both
           windows. It is public tape, not the operator's own print, so it is spellable
           without naming a wallet.

E   on the state STRICTLY BEFORE the print (prints 0..i-1, reserve v[i-1]):
      dd    <= -20 %   the coin is well below its own peak so far        lift 1.14 .. 2.04
      fall5 <= -5 %    price has just flushed, fast                      lift 1.40 .. 2.02
      acc   >= 2       buy SOL has at least doubled against the last 5 s lift 1.18 .. 2.15
      n20   >= 20      the tape is busy                                  lift 1.24
      age   >= 120 s   not a launch                                      lift 1.24 .. 1.83
      v in [42.43, 85] the -50 % outcome is REACHABLE and the wall is not close
D   none. The ticket supply is thousands of unrelated coins by construction, which is the
    whole reason for coming here (the-client-is-the-out-of-sample-unit).
P   none yet.
X   the settled exit and the incumbent trail, side by side.
R   one position per token at a time.   S 0.2 SOL.   seat lag_115 on both legs.

THE ANCHOR. The state at index k is built from prints 0..k-1, so the decision is taken when
print k-1 lands and the fill is the last print by t[k-1] + 115 ms. Anchoring at k instead
would fill LATER, and on a falling tape a later fill is a CHEAPER buy - an optimism worth
about half a print, in exactly the direction this cell is looking.

Nothing here is scored on W/L. The gates are money, the PER-DAY ticket floor (never a mean),
day split, and concentration.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd

from cvx import B, DAY0, cell, show
from kernel import book
from tape import Tape


SHIPPED = dict(arm=21.0, trail=36.0, stop=43.75, cap=1200.0)   # reserve 10 / 20 / -25, squared
INCUMBENT = dict(trail=40.0, cap=600.0)

pd.set_option("display.width", 400)
pd.set_option("display.max_rows", 300)
pd.set_option("display.max_columns", 60)


def pct_from_reserve(v_now, v_then):
    with np.errstate(divide="ignore", invalid="ignore"):
        r = np.where(v_then > 0, (v_now / v_then) ** 2 - 1.0, np.nan)
    return r * 100.0


def run_state(t, v, sol, side, age):
    """State strictly before each print. Identical arithmetic to cvx_hottape.py."""
    n = len(t)
    idx = np.arange(n)
    buy = side == 1
    cb = np.concatenate(([0.0], np.cumsum(np.where(buy, sol, 0.0))))
    cs = np.concatenate(([0.0], np.cumsum(np.where(~buy, sol, 0.0))))
    j5 = np.searchsorted(t, t - 5.0, side="right")
    j10 = np.searchsorted(t, t - 10.0, side="right")
    j20 = np.searchsorted(t, t - 20.0, side="right")
    buy5 = cb[idx] - cb[j5]
    sell5 = cs[idx] - cs[j5]
    buy5p = cb[j5] - cb[j10]
    n20 = idx - j20
    vprev = np.concatenate(([np.nan], v[:-1]))
    p5 = np.maximum(j5 - 1, 0)
    fall5 = pct_from_reserve(vprev, v[p5])
    peak = np.concatenate(([np.nan], np.maximum.accumulate(v)[:-1]))
    dd = pct_from_reserve(vprev, peak)
    with np.errstate(divide="ignore", invalid="ignore"):
        acc = np.where(buy5p > 1e-9, buy5 / buy5p, np.nan)
        bshare5 = np.where(buy5 + sell5 > 0, buy5 / (buy5 + sell5), np.nan)
    return dict(vprev=vprev, dd=dd, fall5=fall5, acc=acc, n20=n20, bshare5=bshare5,
                buy5=buy5, sell5=sell5, age=age)


VARIANTS = {
    #  name          dd     fall5   acc   n20  age_lo  v_lo    v_hi
    "E1":         (-20.0,  -5.0,   2.0,  20,  120.0,  42.43,  85.0),
    "deeper":     (-50.0,  -5.0,   2.0,  20,  120.0,  42.43,  85.0),
    "harder_dip": (-20.0, -10.0,   2.0,  20,  120.0,  42.43,  85.0),
    "acc5":       (-20.0,  -5.0,   5.0,  20,  120.0,  42.43,  85.0),
    "no_acc":     (-20.0,  -5.0,   0.0,  20,  120.0,  42.43,  85.0),
    "no_dip":     (-20.0,  1e9,    2.0,  20,  120.0,  42.43,  85.0),
    "no_dd":      (  0.0,  -5.0,   2.0,  20,  120.0,  42.43,  85.0),
    "old":        (-20.0,  -5.0,   2.0,  20,  600.0,  42.43,  85.0),
}


def fires(f, spec):
    dd, fall5, acc, n20, age_lo, v_lo, v_hi = spec
    m = (
        (f["dd"] <= dd)
        & (f["fall5"] <= fall5)
        & (f["n20"] >= n20)
        & (f["age"] >= age_lo)
        & (f["vprev"] >= v_lo)
        & (f["vprev"] <= v_hi)
    )
    if acc > 0:
        m = m & (f["acc"] >= acc)
    return np.nan_to_num(m, nan=False).astype(bool)


def gates(d, days, name, exit_name):
    """Money, the PER-DAY ticket floor, the day split and concentration. Never a mean."""
    if len(d) == 0:
        return dict(cell=name, exit=exit_name, n=0)
    c = cell(d, days=days)
    per_day = d.groupby("day").run.nunique().sort_index()
    tick = list(per_day.astype(int).values)
    ok_days = int((per_day >= 50).sum())
    top1 = d.y.nlargest(max(1, int(round(0.01 * len(d))))).sum()
    by_mint = d.groupby("run").y.sum().sort_values(ascending=False)
    by_build = d.groupby("cbuild").y.sum().sort_values(ascending=False)
    tot = float(d.y.sum())
    return dict(
        cell=name, exit=exit_name, n=c["n"], mints=c["mints"], sol=c["sol"], pct=c["pct"],
        pos=c["pos"], worst=c["worst"],
        tickets=str(tick), floor="%d/%d" % (ok_days, len(per_day)),
        top1_pct=round(100 * float(top1) / tot, 1) if tot > 0 else None,
        top_mint_pct=round(100 * float(by_mint.iloc[0]) / tot, 1) if tot > 0 else None,
        clients=int(d.cbuild.nunique()),
        top_client_pct=round(100 * float(by_build.iloc[0]) / tot, 1) if tot > 0 else None,
        loo=round(tot - float(by_build.iloc[0]), 2),
        l50_pct=round(100 * float((d.y <= -0.5 * B).mean()), 1),
    )


def main() -> None:
    t0 = time.time()
    T = Tape(
        str(data_file("cvx_prints.parquet")),
        cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
              "wallet_id", "build"],
    )
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0

    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (
        pd.to_datetime(tok.created_at, utc=True, format="ISO8601") - pd.Timestamp(0, tz="UTC")
    ).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()

    sw = pd.read_parquet(data_file("cvx_swdoor.parquet")).drop_duplicates("mint").set_index("mint")
    cbuild = sw.reindex(T.mints).c_build.fillna("?").to_numpy()

    print("tape prints %s  tokens %s  days %.2f  %ds"
          % (f"{T.n:,}", f"{len(T.mints):,}", days, time.time() - t0), flush=True)

    books = {k: {"shipped": [], "incumbent": []} for k in VARIANTS}
    e1_fires = {}

    for r in range(len(T.start)):
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 10 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]
        v = T.v[a:b]
        age = t - c_s[r]
        if age[-1] < 120.0:
            continue
        sol = T.sol[a:b]
        side = T.side[a:b]
        sl = T.slot[a:b]
        f = run_state(t, v, sol, side, age)
        day = T.day[a:b]

        for name, spec in VARIANTS.items():
            m = fires(f, spec)
            if not m.any():
                continue
            ks = np.nonzero(m)[0]
            if name == "E1":
                e1_fires[r] = [int(x) for x in ks]
            for xname, xspec in (("shipped", SHIPPED), ("incumbent", INCUMBENT)):
                last = -1
                acc_rows = books[name][xname]
                for k in ks:
                    k = int(k) - 1          # decide when print k-1 lands (see THE ANCHOR)
                    if k <= last or k < 1 or k >= n - 1:
                        continue
                    y, ei, xi, reason = book(t, v, sl, k, xspec, lag=0.115, B=B)
                    acc_rows.append((y, r, int(day[k]), reason, str(cbuild[r]),
                                     float(f["vprev"][k + 1]), float(f["dd"][k + 1]),
                                     float(f["fall5"][k + 1]), float(f["acc"][k + 1]),
                                     float(age[k]), t[xi] - t[ei],
                                     (v[ei] / v[k]) ** 2 - 1.0))
                    last = xi

        if r % 20000 == 0:
            print("  run %d  E1 fires %s  %ds"
                  % (r, f"{len(books['E1']['shipped']):,}", time.time() - t0), flush=True)

    cols = ["y", "run", "day", "reason", "cbuild", "v", "dd", "fall5", "acc", "age", "hold",
            "fill_dir"]
    rows = []
    frames = {}
    for name in VARIANTS:
        for xname in ("shipped", "incumbent"):
            d = pd.DataFrame(books[name][xname], columns=cols)
            frames[(name, xname)] = d
            rows.append(gates(d, days, name, xname))
    show(rows, "the state fired on the whole tape, both exit families, lag_115")

    e1 = frames[("E1", "shipped")]
    e1.to_parquet(data_file("cvx_hottape_fires.parquet"), index=False)

    if len(e1):
        print("\nE1 exit reasons:", e1.reason.value_counts().to_dict(), flush=True)
        print("E1 hold seconds p10/p50/p90: %.1f / %.1f / %.1f"
              % tuple(e1.hold.quantile([0.1, 0.5, 0.9])), flush=True)
        for col, edges in (("dd", [-101, -60, -50, -40, -30, -20]),
                           ("fall5", [-101, -30, -20, -15, -10, -5]),
                           ("acc", [2, 3, 5, 10, 30, 1e9]),
                           ("v", [42.43, 50, 55, 60, 70, 85]),
                           ("age", [120, 300, 600, 1800, 3600, 1e9])):
            g = e1.groupby(pd.cut(e1[col], edges), observed=True).agg(
                n=("y", "size"), sol=("y", "sum"), pct=("y", "mean"))
            g["pct"] = (g["pct"] / B * 100).round(2)
            g["sol"] = g["sol"].round(2)
            print("\n=== E1 by %s" % col, flush=True)
            print(g.to_string(), flush=True)

    # ---- the exit family the STORY asks for ---------------------------------------------
    # The node holds 15-25 s. The exits above hold 165 s at the median. A bounce off a flush
    # is a seconds-scale move, so the sentence is not read until its own exit family is read.
    GRID = {
        "cap10": dict(cap=10.0), "cap20": dict(cap=20.0), "cap30": dict(cap=30.0),
        "cap60": dict(cap=60.0), "cap120": dict(cap=120.0),
        "tp3_c30": dict(tp=3.0, cap=30.0), "tp5_c30": dict(tp=5.0, cap=30.0),
        "tp5_c60": dict(tp=5.0, cap=60.0), "tp10_c60": dict(tp=10.0, cap=60.0),
        "tp10_c120": dict(tp=10.0, cap=120.0), "tp20_c300": dict(tp=20.0, cap=300.0),
        "tp5_s10_c60": dict(tp=5.0, stop=10.0, cap=60.0),
        "tp10_s20_c120": dict(tp=10.0, stop=20.0, cap=120.0),
        "tr15_c60": dict(trail=15.0, cap=60.0), "tr20_c120": dict(trail=20.0, cap=120.0),
        "tr25_c300": dict(trail=25.0, cap=300.0),
        "shipped": SHIPPED, "incumbent": INCUMBENT,
    }
    grid_rows = {k: [] for k in GRID}
    for r, ks in e1_fires.items():
        a, b = T.start[r], T.end[r]
        n = b - a
        t = T.t[a:b]; v = T.v[a:b]; sl = T.slot[a:b]; day = T.day[a:b]
        for gname, gspec in GRID.items():
            last = -1
            for k in ks:
                k = int(k) - 1
                if k <= last or k < 1 or k >= n - 1:
                    continue
                y, ei, xi, reason = book(t, v, sl, k, gspec, lag=0.115, B=B)
                grid_rows[gname].append((y, r, int(day[k]), reason, str(cbuild[r]),
                                         t[xi] - t[ei], (v[ei] / v[k]) ** 2 - 1.0))
                last = xi
    gcols = ["y", "run", "day", "reason", "cbuild", "hold", "fill_dir"]
    rows = []
    for gname in GRID:
        d = pd.DataFrame(grid_rows[gname], columns=gcols)
        if not len(d):
            continue
        c = cell(d, days=days)
        rows.append(dict(exit=gname, n=c["n"], mints=c["mints"], sol=c["sol"], pct=c["pct"],
                         pos=c["pos"], worst=c["worst"], win=c["win"],
                         hold_p50=round(float(d.hold.median()), 1),
                         reasons=str(d.reason.value_counts().to_dict())))
    show(rows, "E1 under every exit family, including the node's own 15-25 s shape")

    # ---- is the seat actually a tailwind here? ------------------------------------------
    d = frames[("E1", "shipped")]
    if len(d):
        fd = d.fill_dir * 100.0
        print("\n=== THE SEAT ON THE BUY LEG (price at the 115 ms fill vs the decision print)")
        print("  p10 %.2f %%  p25 %.2f %%  p50 %.2f %%  p75 %.2f %%  p90 %.2f %%"
              % tuple(fd.quantile([0.1, 0.25, 0.5, 0.75, 0.9])), flush=True)
        print("  mean %.2f %%   share filled CHEAPER than the decision print %.1f %%"
              % (float(fd.mean()), 100 * float((fd < 0).mean())), flush=True)

    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
