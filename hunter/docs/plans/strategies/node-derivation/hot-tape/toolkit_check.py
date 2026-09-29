"""The toolkit against the hot-tape record: each method step re-run through toolkit/, next to the
number the step-numbered script recorded (evidence 1.11-1.17).

  seat       the members that pay book +2.2..+2.3 %/trade at RACE cap15 (1.11)
  contrast   the sells 8fStGV buys vs those it ignores, same coin: 15 recipes in 5 s against 7,
             3.94 SOL bought in 2 s against 0.40, a new high 5.4 s ago against 80.3, a seller
             who bought 20.5 s ago against 50.8 (medians, case step 24)
  hazard     753 closes of rule 1's kind on the permission's pool; sells hard at +15..+20 % (1.17)

  python toolkit_check.py [seat] [contrast] [hazard]
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import NODE_NAME

import sys
import time

import numpy as np
import pandas as pd

from toolkit import candidates, contrast, hazard, seat, tapes
from toolkit.book import ledger

pd.set_option("display.width", 400)
pd.set_option("display.max_columns", 30)


def main() -> None:
    parts = sys.argv[1:] or ["seat", "contrast", "hazard"]
    t0 = time.time()
    S = tapes.load("study", tapes.roster(NODE_NAME))
    w = {p: S.wallet(p) for p in ("8fStGV", "AbQcLH", "49uohd", "sssssw")}
    print("study tape, instruments %s  %ds" % (sorted(S.ids.values()), time.time() - t0), flush=True)

    if "seat" in parts:
        for p in ("8fStGV", "AbQcLH", "49uohd", "sssssw"):
            E = seat.seat_book(S, seat.episodes(S, w[p]), caps=(15.0,))
            for col in ("race15", "follow15"):
                L = ledger(E.assign(y=E[col], why="-"), S.days)
                print("%s %-9s episodes %5d  %+.2f %%/trade  days %s" % (p, col, L["n"], L["pct"],
                                                                      L["pos"]))
        print("  %ds" % (time.time() - t0), flush=True)

    if "contrast" in parts:
        runs8 = set(np.unique(S.T.run_of[S.T.wallet == w["8fStGV"]]).tolist())
        trig = lambda R: (R.pub & (R.side == -1) & (R.sol >= 1.0) & (R.age >= 60.0)
                          if R.r in runs8 else np.zeros(R.n, dtype=bool))
        C = candidates.build(S, trig)
        C = contrast.label_acted(C, S, w["8fStGV"], window=0.3)
        A = C[C.act == 1]; Kc = C[(C.act == 0) & C.run.isin(set(A.run))]
        cols = ["nb5", "buys2", "stall", "shold", "spnl", "ssize", "hold_n", "age", "vres"]
        print("\nsells >= 1 on its coins %s, it buys %s" % (f"{len(C):,}", f"{len(A):,}"))
        print(contrast.strat_rank(A, Kc, cols).to_string(index=False))
        print(pd.DataFrame({"acted": A[cols].median(), "ignored": Kc[cols].median()}).round(2)
              .to_string())
        print("  %ds" % (time.time() - t0), flush=True)

    if "hazard" in parts:
        bigsell = {}

        def pool(R, e0):
            if R.r not in bigsell:
                bigsell[R.r] = R.tm[R.pub & (R.side == -1) & (R.sol >= 1.0)]
            bs = bigsell[R.r]
            q = int(np.searchsorted(bs, R.tm[e0], side="right"))
            ekind = q > 0 and R.tm[e0] - bs[q - 1] <= 0.300
            p = R.holders()[e0] >= 368 and R.age[e0] >= 158.0
            return ("P" if p else "notP") + (" E-kind" if ekind else " other")

        Ecl, hz, cnt = hazard.closing_hazard(S, w["8fStGV"], pool)
        print("\ncloses by pool:", Ecl.pool.value_counts().to_dict())
        print("hazard, P E-kind (% of in-hold public prints at which it closes next):")
        print(hz["P E-kind"].to_string())
        print("  %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
