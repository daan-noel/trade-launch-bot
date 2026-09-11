"""Hot-tape node, step 39b: rule 1's exit, re-derived on its own pool and booked on both tapes.

Step 39a reads 8fStGV's closing hazard on the holds that pass rule 1's permission (753 closes of
the E kind): on established coins it holds through +8..+12 % (0.5-1.8 % a print), starts selling
at +12..+15 % (2.6-6.3 %) and sells hard at +15..+20 % (12-30 %); the stop spikes at -25..-30 %
(3-11 %, under 1 % between -20 and -25 %); the -20..+5 % band waits for the clock at 60 s.
Derived: take profit +15 %, stop -25 %, 60 s - the +10 % of step 30 was read on the wrong pool.

Booked on rule 1's fires (E x established coin), each exit with its own occupancy, on the study
tape (where any choice is made) and on the holdout (where it is only confirmed):

  the step-30 bracket, the re-derived bracket and its neighbours, a take profit scaled by the
  headroom to the wall, and a ride that holds past the arm while public buying continues
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
import pandas as pd

from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import LOCAL, NODE_NAME, node_ids
from cvx_hot_exit7 import X
import cvx_hot_perm2 as P

B = 0.2
T_MIN = datetime(2026, 9, 6, 12, 0, tzinfo=timezone.utc).timestamp()
CELL = [("rule 1", 0, P.HOLD_MIN, P.AGE_MIN)]
EXITS = [
    X("tp10 sl25 t60 (step 30)", arm=0.10),
    X("tp12 sl25 t60", arm=0.12),
    X("tp15 sl25 t60 (re-derived)", arm=0.15),
    X("tp20 sl25 t60", arm=0.20),
    X("tp15 sl30 t60", arm=0.15, sl=0.30),
    X("tp15 sl20 t60", arm=0.15, sl=0.20),
    X("tp15 sl25 t45", arm=0.15, cap=45.0),
    X("tp15 sl25 t90", arm=0.15, cap=90.0),
    X("headroom x0.10 sl25 t60", hf=0.10),
    X("headroom x0.15 sl25 t60", hf=0.15),
    X("headroom x0.20 sl25 t60", hf=0.20),
    X("ride from +10 while buying sl25 t90", kind="ride", arm=0.10, cap=90.0),
    X("ride from +15 while buying sl25 t90", kind="ride", arm=0.15, cap=90.0),
    X("half +10, half tp20 sl25 t120", kind="scale", arm=0.10, tp2=0.20, cap2=120.0),
]

pd.set_option("display.width", 490)
pd.set_option("display.max_rows", 200)


def study():
    lab = node_ids()
    inv = {v: k for k, v in lab.items()}
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    days = (T.t.max() - T.t.min()) / 86400.0
    return P.run(T, c_s, np.isin(T.wallet, list(lab)), inv["8fStGV"], CELL, exits=EXITS), days


def holdout():
    T = Tape(str(data_file("cvx_holdout_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    W = pd.read_parquet(data_file("cvx_holdout_wallets.parquet"))
    R = pd.read_csv(LOCAL / "solo-traders.csv", usecols=["wallet_address", "node", "use_it"])
    R = R[(R.node == NODE_NAME) & (R.use_it == "yes")]
    node = W[W.address.isin(R.wallet_address)]
    w8 = int(node[node.address.str.startswith("8fStGV")].wallet_id.iloc[0])
    tok = pd.read_parquet(data_file("cvx_holdout_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    days = (T.t.max() - T_MIN) / 86400.0
    return P.run(T, c_s, np.isin(T.wallet, node.wallet_id.to_numpy()), w8, CELL, t_min=T_MIN,
                 exits=EXITS), days


def book(d, days):
    s = d.y.sum()
    top = d.y.nlargest(max(1, int(round(0.01 * len(d))))).sum()
    c = cell(d, days=days)
    c["body"] = round(s - top, 2)
    c["top1"] = round(100 * top / s, 1) if s > 0 else np.nan
    c["maxcoin"] = round(100 * d.groupby("run").y.sum().max() / s, 1) if s > 0 else np.nan
    dl = np.sort(d.day.unique()); h = (len(dl) + 1) // 2
    c["half1"] = round(float(d[d.day.isin(dl[:h])].y.mean()) / B * 100, 2)
    c["half2"] = round(float(d[d.day.isin(dl[h:])].y.mean()) / B * 100, 2)
    c["hold"] = round(float(d.hold.median()), 1)
    mix = d.why.value_counts(normalize=True)
    c["mix"] = " ".join("%s%.0f" % (w, 100 * mix[w]) for w in mix.index)
    return c


def main() -> None:
    t0 = time.time()
    out = []
    for nm, fn in (("study", study), ("holdout", holdout)):
        F, days = fn()
        F.to_parquet(data_file("cvx_hot_exit8_%s.parquet" % nm), index=False)
        for xi_, sp in enumerate(EXITS):
            out.append(dict(tape=nm, exit=sp["name"], **book(F[F.exit == xi_], days)))
        print("%s done  %ds" % (nm, time.time() - t0), flush=True)
    show(out, "rule 1 under each exit, own occupancy")


if __name__ == "__main__":
    main()
