"""Hot-tape node, step 14: does the exit work anywhere, on any slice of their entries?

Steps 12-13 searched the exit shape and found it empty: 480 causal shapes, all negative behind
their print, and at the RACE seat - the unreachable one, sequenced ahead of their own print - the
best shape reads +0.21 %/trade on 5/8 days and goes NEGATIVE with its top 1 % of tickets removed.
Against a perfect-exit ceiling of +53 to +58 %.

That is a verdict on the exit as a STANDALONE slot, and it is exactly the kind of verdict this
program has learned not to trust on its own, because no slot closes alone (7.4 law 17). The
remaining question is whether the exit fails everywhere or only on average: a payoff this convex
can hide a slice where an exit does clear the toll, and a slice IS the P slot.

So the same two shapes are split by facts knowable AT THE DECISION - never by the outcome:

  reserve at entry     where on the curve the entry sits. A pure state variable.
  token age at entry   seconds since creation, from the token table.
  hour of day          the crudest possible regime control, and a known trap: an hour that works
                       on eight days is eight observations, not eight thousand.
  wallet               which of the six. Not shippable (7.4 law 20 forbids a wallet term) and
                       reported only to see whether the failure is one program or all six.
  concurrent agreement how many DISTINCT hot-tape wallets bought this coin in the 60 s before the
                       entry, counted strictly before it. This one IS spellable without naming a
                       wallet - it is "several independent programs are acting at once" - and it
                       is the closest thing in reach to the P slot the node still lacks.

Both seats are reported so the slice question and the seat question do not contaminate each other.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd

from cvx import DAY0, show
from tape import Tape
from cvx_hot_event import node_ids
from cvx_hot_exit2 import B, ladder, robust
from cvx_hot_exit3 import SHAPES, prep_seat

PICK = ("cap15 flat", "tp30 f.5 tr30 cap15")

pd.set_option("display.width", 470)
pd.set_option("display.max_rows", 700)
pd.set_option("display.max_columns", 44)


def main() -> None:
    t0 = time.time()
    E = pd.read_parquet(data_file("cvx_hot_exit_eps.parquet"))
    lab = node_ids()
    NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    is_node = np.isin(T.wallet, NODE)

    byrun = {}
    for i, r in enumerate(E.run.values):
        byrun.setdefault(int(r), []).append(i)
    print("episodes %s  %ds" % (f"{len(E):,}", time.time() - t0), flush=True)

    day = E.day.to_numpy()
    rows = []
    for r, idxs in byrun.items():
        a, b = T.start[r], T.end[r]
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        wal = T.wallet[a:b]
        nb = np.nonzero(is_node[a:b] & (side == 1))[0]
        nbt = t[nb]
        for i in idxs:
            k = int(E.ki.values[i])
            if k >= len(t) - 1:
                continue
            # how many DISTINCT hot-tape wallets bought this coin in the 60 s strictly before k
            lo = np.searchsorted(nbt, t[k] - 60.0, side="left")
            hi = np.searchsorted(nbt, t[k], side="left")
            agree = int(len(np.unique(wal[nb[lo:hi]]))) if hi > lo else 0
            age = float(t[k] - c_s[r]) if np.isfinite(c_s[r]) else np.nan
            rec = dict(i=i, day=int(day[i]), run=r, w=E.w.values[i], agree=agree, age=age,
                       v0=float(v[k] - sol[k] * (1.0 if side[k] == 1 else -1.0)),
                       hour=int((t[k] % 86400) // 3600))
            for seat in ("follow", "race"):
                W = prep_seat(t, v, sol, side, k, seat)
                if W is None:
                    for nm in PICK:
                        rec["%s|%s" % (seat, nm)] = np.nan
                    continue
                for nm in PICK:
                    legs, tr, dd, md = SHAPES[nm]
                    rec["%s|%s" % (seat, nm)], _ = ladder(t, v, W, legs, tr, dd, md)
            rows.append(rec)
    D = pd.DataFrame(rows)
    D["v0_b"] = pd.cut(D.v0, [0, 32, 35, 40, 50, 70, 120, 1e9])
    D["age_b"] = pd.cut(D.age, [-1, 30, 120, 600, 3600, 1e9])
    D["agree_b"] = pd.cut(D.agree, [-1, 0, 1, 2, 6])
    OUT = D.copy()
    for c in OUT.columns:                     # parquet cannot hold Categorical/Interval
        if str(OUT[c].dtype) == "category" or OUT[c].dtype == object:
            OUT[c] = OUT[c].astype(str)
    OUT.to_parquet(data_file("cvx_hot_exit4.parquet"), index=False)
    print("booked %s  %ds" % (f"{len(D):,}", time.time() - t0), flush=True)

    for seat in ("follow", "race"):
        for nm in PICK:
            col = "%s|%s" % (seat, nm)
            base = D[np.isfinite(D[col])].rename(columns={col: "y"})
            out = [robust(base, days, "ALL")]
            for axis in ("v0_b", "age_b", "agree_b", "hour", "w"):
                for lv, g in base.groupby(axis, observed=True):
                    if len(g) < 300:
                        continue
                    out.append(robust(g, days, "%s = %s" % (axis, lv)))
            show(out, "SEAT %s   %s   sliced by decision-time facts" % (seat.upper(), nm))
            print("  %ds" % (time.time() - t0), flush=True)

    print("\n=== the convexity itself, by the same slices (MFE within 600 s of our fill)")
    M = D.merge(E[["mfe600", "arrive"]].reset_index().rename(columns={"index": "i"}), on="i")
    for axis in ("v0_b", "age_b", "agree_b"):
        o = []
        for lv, g in M.groupby(axis, observed=True):
            if len(g) < 300:
                continue
            o.append(dict(slice="%s = %s" % (axis, lv), n=len(g),
                          mfe_p50=round(100 * float(g.mfe600.median()), 1),
                          mfe_p90=round(100 * float(g.mfe600.quantile(0.9)), 1),
                          share_ge25=round(100 * float((g.mfe600 >= 0.25).mean()), 1),
                          share_ge100=round(100 * float((g.mfe600 >= 1.00).mean()), 1)))
        show(o, "available convexity by %s" % axis)

    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
