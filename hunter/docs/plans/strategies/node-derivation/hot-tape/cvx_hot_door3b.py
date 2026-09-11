"""Hot-tape node, step 28b: the two door facts that survive the day split, checked and stacked.

Step 28 walks 30 unpriced coin facts on the frozen E (2,219 fires, -0.68 %/trade, 1/7 days). Two
cuts pass both halves of the walk-forward, each fold picking nearly the same threshold alone:

  abs_rate <= 0.33   few of this coin's earlier frenzy-sells made a new high within 15 s
                     test +0.45 % 3/4 and +2.40 % 3/3
  s_pnl >= 30        the seller takes a profit of 30 % or more on the sale
                     test +0.08 % 3/4 and +1.73 % 2/3

Thirty facts and two folds make a chance pass likely, so before either is a filling:

  1. is it HEADROOM in disguise?  winners and losers inside the coin separate only by reserve and
     age (1.11); read each fact inside reserve and age bands
  2. does it hold on the broad event (every frenzy-sell), where n is seven times larger?
  3. the two stacked, with the per-day list, body, biggest coin, tail, and both exits
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

from pathlib import Path

import numpy as np
import pandas as pd

from cvx import show
from cvx_hot_door3 import book

DAYS = 6.76
ABS = 0.333
PNL = 30.0

pd.set_option("display.width", 490)
pd.set_option("display.max_rows", 400)


def full(d, nm, ycol="y15"):
    c = book(d, DAYS, ycol)
    s = d[ycol].sum()
    top = d[ycol].nlargest(max(1, int(round(0.01 * len(d))))).sum()
    c["top1"] = round(100 * top / s, 1) if s > 0 else np.nan
    return dict(slice=nm, **c)


def main() -> None:
    D = pd.read_parquet(data_file("cvx_hot_door3.parquet"))
    E = D[D.ev == 1].copy()
    F = D[D.ev == 0].copy()
    for d in (E, F):
        d["a_lo"] = d.abs_rate <= ABS
        d["p_hi"] = d.s_pnl >= PNL
        d["vband"] = pd.cut(d.v, [0, 42.43, 50, 60, 70, 85, 1e9],
                            labels=["<42", "42-50", "50-60", "60-70", "70-85", ">85"])
        d["aband"] = pd.cut(d.age, [0, 120, 300, 900, 3600, 1e12],
                            labels=["1-2m", "2-5m", "5-15m", "15-60m", ">1h"])

    print("=== 0. correlation of the two facts with headroom, frozen E")
    print(E[["abs_rate", "s_pnl", "v", "age", "abs_n", "prev_n"]].corr(method="spearman")
          .round(3).to_string())

    rows = []
    for band in ("vband", "aband"):
        for bv, g in E.groupby(band, observed=True):
            for nm, m in (("abs_rate<=.33", g.a_lo), ("abs_rate>.33", ~g.a_lo & g.abs_rate.notna()),
                          ("s_pnl>=30", g.p_hi), ("s_pnl<30", ~g.p_hi & g.s_pnl.notna())):
                if m.sum() >= 60:
                    rows.append(dict(band=band, value=str(bv), cut=nm,
                                     **{k: v for k, v in full(g[m], "")
                                        .items() if k != "slice"}))
    show(rows, "1. each fact inside a reserve band and an age band, frozen E, cap15")

    rows = []
    for nm, d in (("frozen E", E), ("every frenzy-sell", F)):
        rows.append(full(d, nm + ", no door"))
        rows.append(full(d[d.a_lo], nm + ", abs_rate<=.33"))
        rows.append(full(d[d.p_hi], nm + ", s_pnl>=30"))
        rows.append(full(d[d.a_lo & d.p_hi], nm + ", both"))
        rows.append(full(d[d.a_lo | d.p_hi], nm + ", either"))
        rows.append(full(d[~d.a_lo & ~d.p_hi], nm + ", neither"))
    show(rows, "2. the two facts alone and stacked, cap15")

    rows = []
    for nm, m in (("no door", E.index == E.index), ("abs_rate<=.33", E.a_lo),
                  ("s_pnl>=30", E.p_hi), ("both", E.a_lo & E.p_hi)):
        rows.append(full(E[m], nm + ", cap60", "y60"))
    show(rows, "3. the same on the frozen E, cap60")

    print("\n=== 4. per-day, frozen E")
    for nm, m in (("no door", E.index == E.index), ("abs_rate<=.33", E.a_lo),
                  ("s_pnl>=30", E.p_hi), ("both", E.a_lo & E.p_hi)):
        g = E[m].groupby("day")
        print("  %-15s n %s   sol %s" % (nm, g.size().to_dict(),
                                         g.y15.sum().round(2).to_dict()))

    print("\n=== 5. coin list and arrival inside each door (diagnostic only)")
    rows = []
    for nm, m in (("abs_rate<=.33", E.a_lo), ("both", E.a_lo & E.p_hi)):
        for c8 in (1, 0):
            rows.append(full(E[m & (E.c8 == c8)], "%s, %s" % (nm, "its coins" if c8 else "others")))
        for a8 in (1, 0):
            rows.append(full(E[m & (E.arr8 == a8)], "%s, 8fStGV %s" % (nm, "arrives" if a8
                                                                       else "does not")))
    show(rows, "5.")


if __name__ == "__main__":
    main()
