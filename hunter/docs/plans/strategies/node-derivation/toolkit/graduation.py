"""The exits that land on the curve's completing print.

The tapes are curve prints only. When a coin completes its curve inside a hold, the exit's fill
index is the completing buy at vsol 115 and the book sells the bag back into a curve that no
longer exists: the pool has migrated to PumpSwap. The lake holds post-migration AMM prints for
0-1 % of graduated coins, so that path is not observed. Migration is price-continuous by
construction (the pool opens with ~85 SOL against ~206.9 M tokens, the curve's final price), so
the standing convention prices such an exit at the migration point - and a sentence should not
depend on it: count these exits on every book, and prefer a reserve cap that keeps the take
profit well under the wall (vsol <= WALL / sqrt(1 + tp)).

  grad_flag(F, T)   per ticket: its exit fill is the coin's last curve print at vsol >= 114
  amm_net(v0, d)    a curve buy at v0 sold into the migrated pool at its opening price x (1-d),
                    charging the curve's 125 bps (an upper bound on PumpSwap's)
"""
from __future__ import annotations

import numpy as np

from kernel import B_DEFAULT as B, FEE, FIX, K

Q_AMM = 85.0
V_WALL = 115.0


def grad_flag(F, T):
    last = T.end - T.start - 1
    return (F.x.to_numpy() == last[F.run.to_numpy()]) & (F.v1.to_numpy() >= 114.0)


def amm_net(v0, d, b=B):
    s = b / (1.0 + FEE)
    tok = K / v0 - K / (v0 + s)
    p = V_WALL ** 2 / K * (1.0 - d)
    base = Q_AMM / p
    out = Q_AMM * tok / (base + tok)
    return out * (1.0 - FEE) - b - 2.0 * FIX
