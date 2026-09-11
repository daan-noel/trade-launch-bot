"""Rule 1 update, step G4: new E terms, each a one-sided cut added to the G2-G3 sentence.

Base is rule 1 with "bought >= 2 SOL in 2 s" dropped (G2) and the reserve at the sell <= 100
SOL. Each candidate fact is cut at the 5..50 % quantiles of the base fires, from above and from
below; the cut masks the candidates BEFORE occupancy. toolkit/walkforward.new_terms decides; a
term that passes with a test-half gain inside cut_noise() is chance. The holdout is never read.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path

import numpy as np
import pandas as pd

from toolkit.book import fires, ledger
from toolkit.walkforward import cut_noise, new_terms
from cvx_r1u import RULE1_UPDATED as BASE, cand

NEW = ["ssize", "nb2", "nb3", "nb10", "buys3", "buys5", "sells5", "buys10", "sells10", "np5", "nw5",
       "spnl", "sfrac", "dd", "mvk", "mv3", "mv10", "mv60", "nbig30", "rel5", "net5", "net10"]

pd.set_option("display.width", 300)
pd.set_option("display.max_rows", 300)


def derive(C):
    C = C.copy()
    C["rel5"] = C.ssize / np.maximum(C.buys5, 0.01)
    C["net5"] = C.buys5 - C.sells5
    C["net10"] = C.buys10 - C.sells10
    return C


def main() -> None:
    C, days = cand("study")
    C = derive(C)
    Fb = fires(C, BASE)
    print("base study", ledger(Fb, days))
    print("chance: SD of the SOL a random 5 %% / 10 %% of the tickets carry, per test half: %s / %s"
          % (cut_noise(Fb, 0.05), cut_noise(Fb, 0.10)))
    W, curves = new_terms(C, days, BASE, NEW)
    print("\n=== study curves")
    print(curves.to_string(index=False))
    print("\n=== walk-forward, new terms")
    print(W.to_string(index=False))


if __name__ == "__main__":
    main()
