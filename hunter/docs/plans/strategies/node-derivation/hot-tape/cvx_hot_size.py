"""Hot-tape node, step 40: rule 1's clip (the S slot).

Rule 1 is booked at 0.2 SOL. A trade's path does not depend on the clip - the take profit, stop and
clock trip on the tape's own prints - so each ticket is re-priced at every clip from its entry and
exit reserves through the kernel's exact curve arithmetic (`kernel.net`: 125 bps and 0.000225 SOL a
leg, and our own impact on the virtual reserve both ways). What this does not price: our buy
moving the price the NEXT prints see, and a larger clip changing whether we are filled first. So
the larger clips are an upper bound, and the bound is looser the larger the clip.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time

import numpy as np
import pandas as pd

from kernel import net
from cvx import cell, show
import cvx_hot_exit8 as X8

CLIPS = (0.1, 0.2, 0.35, 0.5, 0.75, 1.0, 1.5)


def main() -> None:
    t0 = time.time()
    X8.EXITS = [X8.X("tp15 sl25 t90", arm=0.15, cap=90.0)]
    out = []
    for nm, fn in (("study", X8.study), ("holdout", X8.holdout)):
        F, days = fn()
        F.to_parquet(X8.P("cvx_hot_size_%s.parquet" % nm), index=False)
        for clip in CLIPS:
            d = F.assign(y=[net(a, b, clip) for a, b in zip(F.v0, F.v1)])
            s = d.y.sum()
            top = d.y.nlargest(max(1, int(round(0.01 * len(d))))).sum()
            c = cell(d, b=clip, days=days)
            c["sol_day"] = round(s / days, 3)
            c["body"] = round(s - top, 2)
            c["top1"] = round(100 * top / s, 1) if s > 0 else np.nan
            out.append(dict(tape=nm, clip=clip, **c))
        print("%s done  %ds" % (nm, time.time() - t0), flush=True)
    show(out, "rule 1 at each clip (tp15 sl25 t90), re-priced from the same tickets")
    F = pd.read_parquet(X8.P("cvx_hot_size_study.parquet"))
    print("\n entry reserve of the tickets, p10/50/90:", np.round(F.v0.quantile([.1, .5, .9]).values, 1))


if __name__ == "__main__":
    main()
