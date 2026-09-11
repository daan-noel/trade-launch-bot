"""Rule 1 update, step G1: one candidate table per tape (toolkit/candidates.py); its actor columns are G0.

The trigger is rule 1's event class: every PUBLIC sell >= 0.5 SOL on a coin aged >= 60 s. The
loose floors contain rule 1 and every loosening tried: >= 8 recipes in 5 s, a new high <= 90 s
ago, the seller bought <= 300 s ago. Each row carries its outcome under rule 1's exit before the
update (take profit +15 %, stop -25 %, 90 s) from a fill 115 ms later. 8fStGV is the actor: its
columns are diagnostics only, renamed to this study's names (pk8 it bought the sell <= 0.5 s
after, lag8, pk8_pre its buy landed by our fill, in8 it bought inside our hold).

  python cvx_r1u_cand.py [study|holdout ...]
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path

import sys
import time

from toolkit import book, candidates, tapes
from toolkit.exits import X
from cvx_hot_event import NODE_NAME
from cvx_r1u import PREFIX

EXIT = X("tp15 sl25 t90", arm=0.15, cap=90.0)
ACTOR = "8fStGV"


def trigger(R):
    return R.pub & (R.side == -1) & (R.sol >= 0.5) & (R.age >= 60.0)


def floor(f):
    return f["nb5"] >= 8 and f["stall"] <= 90.0 and f["shold"] <= 300.0


def build(name):
    S = tapes.load(name, tapes.roster(NODE_NAME))
    C = candidates.build(S, trigger, floor, EXIT, actor=S.wallet(ACTOR))
    C = C.rename(columns={"act": "pk8", "act_lag": "lag8", "act_pre": "pk8_pre", "act_in": "in8"})
    return C, S.days


if __name__ == "__main__":
    for name in sys.argv[1:] or ["study", "holdout"]:
        t0 = time.time()
        C, days = build(name)
        book.save(C, days, PREFIX, name)
        print("%s: %s candidates on %s coins, %.2f days  %ds"
              % (name, f"{len(C):,}", f"{C.run.nunique():,}", days, time.time() - t0), flush=True)
