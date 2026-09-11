"""Hot-tape re-entry, loop [3]: is there a SELECTION inside the flush that pays the toll?

Where loop [2] left it. E1 fires 17,204 times a week and books -2.82 % a trade under its best
exit (a 10 s clock), 0 of 8 days positive. Subtract the 3.2-4 % round-trip toll and the GROSS
edge is about +0.4 to +1.2 % a trade - which is the node's own margin, 1.10 %, reproduced.
So the moment is real and the clone is dead for the recorded reason: their margin on spend
cannot cover our toll (n4-is-a-lottery-with-bad-aborts).

The only route left is selection: a subset whose gross is four times the node's average. This
script asks that ONCE, with the selectors PRE-REGISTERED below, under the two exits that won
loop [2]. It is not a search. Every cell is reported with its per-day tickets and its client
concentration, whatever the money says.

  S1  the slow-wall launch door         - the only door that has ever booked positive
  S2  not a bundler creation cgroup     - a standing exclusion
  S3  the creator has not sold yet      - the term that carries the frozen sentence
  S4  drawdown x flow-acceleration      - the two strongest discriminators, crossed
  S5  how busy the tape is (n20)        - the node's own axis
  S6  who dominates the flush (bshare5) - is it a seller flush or a two-sided one
  S7  the size of the flush (sell5)     - is a big flush different from a small one
  S8  hour of day                       - a pure calendar control, expected to be flat

A cell counts only if it clears money AND the per-day ticket floor AND leaves the top client
under a third of the net. A mean is never a floor.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from cvx import B, DAY0, cell, show
from kernel import book
from tape import Tape


EXITS = {"cap20": dict(cap=20.0), "cap60": dict(cap=60.0)}
E1 = (-20.0, -5.0, 2.0, 20, 120.0, 42.43, 85.0)

pd.set_option("display.width", 400)
pd.set_option("display.max_rows", 400)
pd.set_option("display.max_columns", 60)


def pct_from_reserve(v_now, v_then):
    with np.errstate(divide="ignore", invalid="ignore"):
        r = np.where(v_then > 0, (v_now / v_then) ** 2 - 1.0, np.nan)
    return r * 100.0


def run_state(t, v, sol, side, age):
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


def gate_row(d, days, name):
    if len(d) == 0:
        return dict(cut=name, n=0)
    c = cell(d, days=days)
    per_day = d.groupby("day").run.nunique().sort_index()
    by_build = d.groupby("cbuild").y.sum().sort_values(ascending=False)
    tot = float(d.y.sum())
    return dict(
        cut=name, n=c["n"], mints=c["mints"], sol=c["sol"], pct=c["pct"], pos=c["pos"],
        worst=c["worst"], win=c["win"],
        floor="%d/%d" % (int((per_day >= 50).sum()), len(per_day)),
        tickets=",".join(str(int(x)) for x in per_day.values),
        clients=int(d.cbuild.nunique()),
        top_client_pct=round(100 * float(by_build.iloc[0]) / tot, 1) if tot > 0 else None,
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

    creator = pq.read_table(str(data_file("cvx_prints.parquet")),
                            columns=["creator_id"]).column("creator_id").to_pandas()
    creator = creator.fillna(-1).to_numpy().astype(np.int64)
    assert len(creator) == T.n, "creator column is not aligned with the tape"

    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (
        pd.to_datetime(tok.created_at, utc=True, format="ISO8601") - pd.Timestamp(0, tz="UTC")
    ).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()

    sw = pd.read_parquet(data_file("cvx_swdoor.parquet")).drop_duplicates("mint").set_index("mint")
    al = sw.reindex(T.mints)
    cbuild = al.c_build.fillna("?").to_numpy()
    door5 = al.slow_wall.fillna(False).to_numpy().astype(bool)
    bundler = al.bundler_group.fillna(False).to_numpy().astype(bool)

    print("tape prints %s  tokens %s  days %.2f  %ds"
          % (f"{T.n:,}", f"{len(T.mints):,}", days, time.time() - t0), flush=True)

    dd_c, fall_c, acc_c, n20_c, age_lo, v_lo, v_hi = E1
    rows = {k: [] for k in EXITS}

    for r in range(len(T.start)):
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 10 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]
        v = T.v[a:b]
        age = t - c_s[r]
        if age[-1] < age_lo:
            continue
        sol = T.sol[a:b]
        side = T.side[a:b]
        sl = T.slot[a:b]
        f = run_state(t, v, sol, side, age)
        day = T.day[a:b]

        # the creator's own sells on this coin, in order
        cid = creator[a] if creator[a] >= 0 else -1
        csell = np.nonzero((T.wallet[a:b] == cid) & (side == -1))[0] if cid >= 0 else np.array([])
        first_csell = int(csell[0]) if len(csell) else n + 1

        m = (
            (f["dd"] <= dd_c) & (f["fall5"] <= fall_c) & (f["acc"] >= acc_c)
            & (f["n20"] >= n20_c) & (f["age"] >= age_lo)
            & (f["vprev"] >= v_lo) & (f["vprev"] <= v_hi)
        )
        m = np.nan_to_num(m, nan=False).astype(bool)
        if not m.any():
            continue
        ks = np.nonzero(m)[0]
        hour = ((T.t[a:b] / 3600.0) % 24).astype(np.int16)

        for xname, xspec in EXITS.items():
            last = -1
            acc_rows = rows[xname]
            for k in ks:
                k = int(k) - 1
                if k <= last or k < 1 or k >= n - 1:
                    continue
                y, ei, xi, reason = book(t, v, sl, k, xspec, lag=0.115, B=B)
                acc_rows.append((
                    y, r, int(day[k]), str(cbuild[r]), bool(door5[r]), bool(bundler[r]),
                    bool(k < first_csell),
                    float(f["dd"][k + 1]), float(f["acc"][k + 1]), float(f["fall5"][k + 1]),
                    float(f["n20"][k + 1]), float(f["bshare5"][k + 1]),
                    float(f["sell5"][k + 1]), float(f["vprev"][k + 1]), float(age[k]),
                    int(hour[k]),
                ))
                last = xi

        if r % 30000 == 0:
            print("  run %d  fires %s  %ds"
                  % (r, f"{len(rows['cap20']):,}", time.time() - t0), flush=True)

    cols = ["y", "run", "day", "cbuild", "door5", "bundler", "creator_clean",
            "dd", "acc", "fall5", "n20", "bshare5", "sell5", "v", "age", "hour"]

    for xname in EXITS:
        d = pd.DataFrame(rows[xname], columns=cols)
        if not len(d):
            continue
        d.to_parquet(data_file("cvx_hottape_sel_%s.parquet" % xname), index=False)
        out = [gate_row(d, days, "ALL")]

        out.append(gate_row(d[d.door5], days, "S1 slow-wall door"))
        out.append(gate_row(d[~d.door5], days, "S1 control: not the door"))
        out.append(gate_row(d[~d.bundler], days, "S2 not a bundler cgroup"))
        out.append(gate_row(d[d.bundler], days, "S2 control: bundler cgroup"))
        out.append(gate_row(d[d.creator_clean], days, "S3 creator has not sold"))
        out.append(gate_row(d[~d.creator_clean], days, "S3 control: creator has sold"))
        show(out, "%s :: S1-S3, the pre-registered doors and permissions" % xname)

        # S4 the two strongest discriminators, crossed
        db = pd.cut(d.dd, [-101, -60, -50, -40, -30, -20])
        ab = pd.cut(d.acc, [2, 3, 5, 10, 1e9])
        g = d.groupby([db, ab], observed=True).agg(n=("y", "size"), sol=("y", "sum"),
                                                   pct=("y", "mean"))
        g["pct"] = (g["pct"] / B * 100).round(2)
        g["sol"] = g["sol"].round(2)
        print("\n=== %s :: S4 drawdown x flow acceleration" % xname, flush=True)
        print(g.to_string(), flush=True)

        for name, col, edges in (("S5 prints in the last 20 s", "n20", [20, 40, 80, 160, 1e6]),
                                 ("S6 buy share of the flush", "bshare5",
                                  [-0.01, 0.1, 0.2, 0.35, 0.5, 1.01]),
                                 ("S7 SOL sold in the flush", "sell5", [0, 2, 5, 15, 40, 1e9]),
                                 ("S8 hour of day (control)", "hour",
                                  [-1, 3, 7, 11, 15, 19, 23])):
            g = d.groupby(pd.cut(d[col], edges), observed=True).agg(
                n=("y", "size"), sol=("y", "sum"), pct=("y", "mean"),
                days_pos=("y", lambda s: int((d.loc[s.index].groupby("day").y.sum() > 0).sum())))
            g["pct"] = (g["pct"] / B * 100).round(2)
            g["sol"] = g["sol"].round(2)
            print("\n=== %s :: %s" % (xname, name), flush=True)
            print(g.to_string(), flush=True)

    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
