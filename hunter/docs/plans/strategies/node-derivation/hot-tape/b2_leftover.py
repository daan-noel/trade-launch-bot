"""Hot-tape node, step B2c: calibrate derive 5.2 (leftover existence) on triggers whose fate is known.

Question: which reading of "leftover exists at our 115 ms fill on the prints it acts on" passes the
trigger that became rule 1 and kills the triggers that are known dead, and what are its cut lines?

Anchors, fixed before this run (study tape, members out of every public print):

  8fStGV  public SELL >= 1 SOL    MUST PASS   rule 1's trigger (evidence 1.12, 1.22)
  AbQcLH  burst start             MUST KILL   a race: lag p50 47 ms, ahead 5 %, -1.36 % (1.12)
  sssssw  burst start             MUST KILL   the member that loses at every seat (1.11)
  8fStGV  burst start             MUST KILL   the class it avoids (lift 0.01 at 0-25 ms, 1.12)
  49uohd  public SELL >= 1 SOL    no anchor   reachable, unspelled (1.16); read only

Each reading is `toolkit.seat.leftover` on its acted tickets against the same coins' ignored
prints of the class (up to 4 per acted ticket per coin), at its own hold p10 / p50 / p90:

  absolute   on the acted tickets where our fill lands BEHIND its buy (its fill already in the
             price; a ticket ahead of it counts its own buy as our leftover, a copied fill):
             median cost < 2 %, median peak leftover (hold p50) > 0, missed < 50 %
  control    the acted peak leftover's within-coin excess over ignored, coin bootstrap p5 > 0
  path       the share that reaches break-even before the mirror loss, acted above 1 / (r + 1)
             (about 49 %) and above ignored within the coin, bootstrap p5 > 0

A reading that passes a MUST KILL anchor passes noise and is not the veto.

  python b2_leftover.py
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import DATA, NODE_NAME

import time

import numpy as np
import pandas as pd

from toolkit import seat, tapes

MAX_TRIG = 0.3
BURST_GAP = 0.4

pd.set_option("display.width", 400)
pd.set_option("display.max_columns", 40)


def sell_ge1(R):
    return R.pub & (R.side == -1) & (R.sol >= 1.0)


def burst_start(R):
    gap = np.concatenate(([1e9], np.diff(R.tm)))
    return R.pub & (R.side == 1) & (gap >= BURST_GAP)


CASES = [("8fStGV", "sell>=1", sell_ge1, "PASS"),
         ("AbQcLH", "burst start", burst_start, "KILL"),
         ("sssssw", "burst start", burst_start, "KILL"),
         ("8fStGV", "burst start", burst_start, "KILL"),
         ("49uohd", "sell>=1", sell_ge1, "-")]


def main() -> None:
    t0 = time.time()
    S = tapes.load("study", tapes.roster(NODE_NAME))
    print("study tape, instruments %s  %ds" % (sorted(S.ids.values()), time.time() - t0), flush=True)
    rows, keep = [], []
    for p, cls, fn, want in CASES:
        w = S.wallet(p)
        E = seat.episodes(S, w)
        L = seat.leftover(S, w, E, fn, max_trig=MAX_TRIG)
        hz = L.attrs["horizons"]
        Tb, X = seat.leftover_summary(L)
        A = L[L.acted == 1]
        cov = 100.0 * len(A) / max(len(E), 1)
        print("\n=== %s x %s  (anchor: %s)  episodes %d, acted tickets %d (%.1f %% of its decisions)"
              "  hold p10/p50/p90 %s s  %ds"
              % (p, cls, want, len(E), len(A), cov, "/".join("%.1f" % h for h in hz),
                 time.time() - t0), flush=True)
        print(Tb.to_string())
        print("within-coin excess, acted minus ignored:")
        print(X.to_string())
        a, x = Tb.loc["behind"], X
        mid = "peak%d" % (len(hz) // 2)
        absolute = bool(a.cost_p50 < 2.0 and a["peak_p50_h%d" % (len(hz) // 2)] > 0
                        and a.missed < 50.0)
        control = bool(x.loc[mid, "p5"] > 0)
        path = bool(a.up > a.null and x.loc["up", "p5"] > 0)
        rows.append(dict(member=p, trigger=cls, anchor=want, decisions=len(E), acted=len(A),
                         coverage=round(cov, 1), dt_p50=a.dt_p50_ms, ahead=a.ahead,
                         missed=a.missed, cost=a.cost_p50, peak_mid=a["peak_p50_h%d" % (len(hz) // 2)],
                         ign_peak_mid=Tb.loc["ignored", "peak_p50_h%d" % (len(hz) // 2)],
                         peak_excess=x.loc[mid, "excess"], peak_p5=x.loc[mid, "p5"],
                         up=a.up, null=a.null, ign_up=Tb.loc["ignored", "up"], up_excess=x.loc["up", "excess"],
                         up_p5=x.loc["up", "p5"], absolute=absolute, control=control, path=path))
        keep.append(L.assign(member=p, trigger=cls))
    R = pd.DataFrame(rows)
    print("\n=== the three readings against the anchors")
    print(R.to_string(index=False))
    pd.concat(keep).to_parquet(DATA / "b2_leftover.parquet", index=False)
    R.to_csv(DATA / "b2_leftover.csv", index=False)
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
