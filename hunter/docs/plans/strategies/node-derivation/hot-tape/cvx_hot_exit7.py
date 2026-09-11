"""Hot-tape node, step 31: a TAPE-DRIVEN exit for the frenzy-absorbed sell, against the static bracket.

Step 30 books the member's hazard as a static bracket (take profit +10 %, stop -25 %, 60 s):
+1.22 %/trade 5/7 behind the door. Two facts say a static bracket is only the shadow of the exit:

  the member's closing sell is triggered by a PRINT - 25-50 ms after a public BUY >= 1 SOL or a
  +3 % print, at a median +14.4 % - so a fixed +10 % caps every winner at about +10 %
  the bracket's losers are costly - with no stop, the trades that reach the 60 s clock average
  -24 %; a frenzy that dies is left until the stop or the clock

So the exit families here react to the tape, each spelled from a member's own exit or from the
exit inventory, never from a sweep:

  sellbuy   once up >= arm, close on the first public buy >= 1 SOL or +3 % print (the member)
  trail     once up >= arm, ride; close on a give-back from the in-hold peak
  fade      the buying stopped: public buy SOL in the last 3 s < 0.3 - fired only UNDER WATER
            (the loss-only form; strategy law: a state cut must only fire under water)
  dump      a public sell >= 1 SOL while under water -5 % or worse (49uohd / sssssw)

Every branch resolves to a print index and the smallest wins (7.4 law 24); the fill is 115 ms
after the print that trips it; each exit has its OWN occupancy. First a diagnostic of where the
bracket leaves money: how far a take-profit's move runs on, and whether a loser was ever up.

  E  evidence 1.12      D  abs_rate <= 0.333 (1.13), and none as the control
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import FEE, FIX, K, fill_idx, net
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import node_ids
from toolkit.exits import WALL, X, first_trip, net_split, run_exit  # noqa: F401  - the exit engine

B = 0.2
LAG = 0.115
ABS_W = 15.0
ABS_MAX = 0.333

EXITS = [
    X("bracket tp10 sl25 t60 (step 30)"),
    X("sellbuy arm10 sl25 t60", kind="sellbuy"),
    X("trail arm10 give5 sl25 t60", kind="trail", give=0.05),
    X("bracket + fade-under-water", fade=True),
    X("bracket + dump-under-water", dump=True),
    X("bracket + breakeven after +3", be=0.03),
    X("bracket + breakeven after +5", be=0.05),
    X("bracket + breakeven after +7", be=0.07),
    X("half at +10, half tp20 / breakeven / t120", kind="scale", tp2=0.20, cap2=120.0, be2=True),
    X("half at +10, half tp30 / breakeven / t120", kind="scale", tp2=0.30, cap2=120.0, be2=True),
    X("half at +10, half tp20 / sl25 / t120", kind="scale", tp2=0.20, cap2=120.0, be2=False),
    X("half at +10, half tp20 / breakeven / t60", kind="scale", tp2=0.20, cap2=60.0, be2=True),
    X("BE after +5, half at +10, half tp20 / BE / t120", kind="scale", be=0.05, tp2=0.20,
      cap2=120.0, be2=True),
]

pd.set_option("display.width", 490)
pd.set_option("display.max_rows", 400)


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    inv = {v: k for k, v in lab.items()}
    w8 = inv["8fStGV"]
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    is_node = np.isin(T.wallet, NODE)
    coins8 = set(np.unique(T.run_of[np.nonzero(T.wallet == w8)[0]]).tolist())
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    print("tape %s prints  %ds" % (f"{T.n:,}", time.time() - t0), flush=True)

    rows = []
    diag = []
    for r in range(len(T.start)):
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 60 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        wal = T.wallet[a:b]; bu = T.build[a:b]
        tm = np.maximum.accumulate(t)
        age = tm - c_s[r]
        if age[-1] < 60.0:
            continue
        pub = ~is_node[a:b]
        big = pub & (side == -1) & (sol >= 1.0)
        if not (big & (age >= 60.0)).any():
            continue
        j2 = np.searchsorted(tm, tm - 2.0, side="left")
        j3 = np.searchsorted(tm, tm - 3.0, side="left")
        j5 = np.searchsorted(tm, tm - 5.0, side="left")
        cb = np.concatenate(([0.0], np.cumsum(np.where(side == 1, sol, 0.0))))
        cbp = np.concatenate(([0.0], np.cumsum(np.where(pub & (side == 1), sol, 0.0))))
        buys2 = cb[np.arange(n)] - cb[j2]
        cb3 = cbp[np.arange(n) + 1] - cbp[j3]
        mv = np.concatenate(([0.0], ((v[1:] / v[:-1]) ** 2 - 1.0) * 100.0))
        rmax = np.maximum.accumulate(v)
        newhigh = np.concatenate(([True], v[1:] > rmax[:-1]))
        t_nh = pd.Series(np.where(newhigh, t, np.nan)).ffill().to_numpy()
        stall = np.concatenate(([0.0], t[1:] - t_nh[:-1]))
        lastb = {}
        shold = np.full(n, np.nan)
        for i in range(n):
            if side[i] == -1:
                lb = lastb.get(wal[i])
                if lb is not None:
                    shold[i] = tm[i] - lb
            else:
                lastb[wal[i]] = tm[i]
        fz = []
        nb5a = {}
        for k in np.nonzero(big)[0]:
            k = int(k)
            nb5a[k] = len(np.unique(bu[j5[k]:k]))
            if nb5a[k] >= 12:
                fz.append(k)
        if not fz:
            continue
        ab_p = np.array(fz)
        ab_tc = np.empty(len(fz)); ab_ok = np.zeros(len(fz), dtype=bool)
        for q, p in enumerate(fz):
            e = int(np.searchsorted(t, t[p] + ABS_W, side="right"))
            ref = rmax[p - 1] if p > 0 else v[p]
            ab_ok[q] = (e > p + 1) and (float(v[p + 1:e].max()) > ref)
            ab_tc[q] = tm[p] + ABS_W
        ev = [k for k in fz if age[k] >= 60.0 and nb5a[k] >= 15 and buys2[k] >= 2.0
              and stall[k] <= 20.0 and shold[k] <= 30.0]
        if not ev:
            continue
        c8 = int(r in coins8)
        for door in (0, 1):
            for xi_, spec in enumerate(EXITS):
                last = -1
                for k in ev:
                    if k <= last:
                        continue
                    if door:
                        m = (ab_p < k) & (ab_tc <= tm[k])
                        if not m.any() or ab_ok[m].mean() > ABS_MAX:
                            continue
                    ei = fill_idx(t, k, LAG)
                    y, x, why, hold = run_exit(spec, t, v, side, sol, mv, cb3, ei)
                    rows.append((r, int(T.day[a + k]), c8, door, xi_, y, why, hold))
                    last = x
                    if door and xi_ == 0:
                        # where the bracket leaves money: the path to 120 s after our fill
                        v0 = float(v[ei])
                        e120 = int(np.searchsorted(t, t[ei] + 120.0, side="right"))
                        seg = (v[ei + 1:e120] / v0) ** 2 - 1.0 if e120 > ei + 1 else np.zeros(1)
                        x60 = int(np.searchsorted(t, t[ei] + 60.0, side="right"))
                        seg60 = seg[:max(x60 - ei - 1, 1)]
                        after = (v[x + 1:e120] / v0) ** 2 - 1.0 if e120 > x + 1 else np.zeros(1)
                        diag.append((why, y, float(seg60.max()), float(seg.max()),
                                     float(after.max()) if len(after) else np.nan,
                                     float(((v[x] / v0) ** 2 - 1.0))))
    F = pd.DataFrame(rows, columns=["run", "day", "c8", "door", "exit", "y", "why", "hold"])
    F.to_parquet(data_file("cvx_hot_exit7.parquet"), index=False)
    G = pd.DataFrame(diag, columns=["why", "y", "mfe60", "mfe120", "after_max", "exit_px"])
    print("fires %s  %ds" % (f"{len(F):,}", time.time() - t0), flush=True)

    print("\n=== 0. where the bracket leaves money, behind the door  (%% of our fill price)")
    for why, g in G.groupby("why"):
        print("  %-5s n %4d  book %+.2f %%/trade   peak in 60 s p25/50/75 %s   peak in 120 s p50 %.1f"
              "   after our exit, the best price to 120 s p25/50/75 %s"
              % (why, len(g), g.y.mean() / B * 100,
                 np.round(100 * g.mfe60.quantile([.25, .5, .75]).values, 1),
                 100 * g.mfe120.median(),
                 np.round(100 * g.after_max.quantile([.25, .5, .75]).values, 1)))
    lo = G[G.why != "tp"]
    print("  losers (sl + time): ever up +3 %% before the exit: %.1f %%   ever up +5 %%: %.1f %%"
          % (100 * (lo.mfe60 >= 0.03).mean(), 100 * (lo.mfe60 >= 0.05).mean()))
    print("  winners (tp): went on to +20 %% within 120 s: %.1f %%   to +40 %%: %.1f %%"
          % (100 * (G[G.why == "tp"].after_max >= 0.20).mean(),
             100 * (G[G.why == "tp"].after_max >= 0.40).mean()))

    out = []
    for door in (1, 0):
        for xi_, spec in enumerate(EXITS):
            d = F[(F.door == door) & (F.exit == xi_)]
            if not len(d):
                continue
            s = d.y.sum()
            top = d.y.nlargest(max(1, int(round(0.01 * len(d))))).sum()
            c = cell(d, days=days)
            c["body"] = round(s - top, 2)
            c["top1"] = round(100 * top / s, 1) if s > 0 else np.nan
            c["maxcoin"] = round(100 * d.groupby("run").y.sum().max() / s, 1) if s > 0 else np.nan
            c["d0_3"] = round(float(d[d.day <= 3].y.mean()) / B * 100, 2)
            c["d4_6"] = round(float(d[d.day > 3].y.mean()) / B * 100, 2)
            c["hold"] = round(float(d.hold.median()), 1)
            mix = d.why.value_counts(normalize=True)
            c["mix"] = " ".join("%s%.0f" % (w, 100 * mix[w]) for w in mix.index)
            out.append(dict(door="abs<=.33" if door else "none", exit=spec["name"], **c))
    show(out, "the frozen E under each exit, own occupancy")
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
