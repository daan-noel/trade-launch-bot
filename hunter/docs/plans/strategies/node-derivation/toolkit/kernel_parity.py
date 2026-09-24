"""Guard: every round-trip formula in this tree equals the engine's kernel.

The lab prices a round trip in five places - `study-kernel/kernel.py::net`, `r1_exact.y_engine`,
`r1b_exit.y_legs`, `r1g_size.price` and `mid-tape/mt_d8_replay.y_engine` - because each node needs
it in its own terms (reserves or spot, one leg or several, scalar or frame). Copies drift: the
engine moved the venue fee to the top of the buy on 2026-09-14 (`buy_fill`, commit 5b43afb0) and
four of the five kept charging it at the end for eight days, booking every result about 2 % high.
`net` alone stayed right, which is why nobody noticed - the single source existed and the copies
outvoted it.

This is the no-DB guard CLAUDE.md asks for where duplication is unavoidable: it pins the shared
helper to the engine's own golden vectors, then asserts every copy agrees with it.

  python -m toolkit.kernel_parity        prints each check; exit 1 on any drift
"""
from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import numpy as np
import pandas as pd

from .paths import DATA, ROOT

FAIL: list[str] = []


def check(name: str, got: float, want: float, tol: float) -> None:
    ok = abs(got - want) <= tol
    print("  %-52s %s  (%.12g vs %.12g, tol %g)"
          % (name, "ok " if ok else "DRIFT", got, want, tol))
    if not ok:
        FAIL.append(name)


def load(path: Path):
    """Each node's scripts sit beside their own `_paths` bootstrap, so the folder goes on the
    path before the module is executed."""
    if str(path.parent) not in sys.path:
        sys.path.insert(0, str(path.parent))
    spec = importlib.util.spec_from_file_location(path.stem, path)
    m = importlib.util.module_from_spec(spec)
    sys.modules[path.stem] = m
    spec.loader.exec_module(m)
    return m


# ---------------------------------------------------------------- the engine, re-spelled
def sell_value_proceeds(value_sol, reserve_sol, fee, fixed_sell, close_fee, empties_bag=True):
    """hunter/core/src/strategies/kernel.rs `sell_value_proceeds`."""
    value = max(value_sol, 0.0) if np.isfinite(value_sol) else 0.0
    out = value / (1.0 + value / reserve_sol) if reserve_sol is not None else value
    return out * (1.0 - fee) - fixed_sell - (close_fee if empties_bag else 0.0)


# The literals `sell_value_proceeds_golden_vectors` pins, with that test's cost model.
GOLDEN = [(0.05, None, 0.049120000000000004),
          (0.1, 70.0, 0.098354129814550648),
          (0.030784, None, 0.0301442),
          (30.0, 3.0, 2.6929268181818182)]
G_FEE, G_FIXED_SELL, G_CLOSE = 0.0125, 0.00025, 0.000005


def main() -> int:
    print(__doc__.split("\n")[0])

    print("\n== the engine's own golden vectors (kernel.rs sell_value_proceeds_golden_vectors)")
    for value, reserve, want in GOLDEN:
        got = sell_value_proceeds(value, reserve, G_FEE, G_FIXED_SELL, G_CLOSE)
        check("sell_value_proceeds(%g, %s)" % (value, reserve), got, want, 1e-12)

    hot = ROOT / "hot-tape"
    rx = load(hot / "r1_exact.py")
    rb = load(hot / "r1b_exit.py")
    sz = load(hot / "r1g_size.py")
    d8 = load(ROOT / "mid-tape" / "mt_d8_replay.py")
    sk = importlib.import_module("kernel")          # study-kernel/kernel.py, on sys.path

    print("\n== r1_exact.y_engine is the engine's round trip")
    # the fee must reach the curve as notional / (1 + fee), so the bag is 1/1.0125 of the naive one
    s0, v0, s1, v1, b = 2.58e-13, 91.132881024, 2.821e-13, 95.294659076, 0.2
    tok, paid = rx.buy_fill(s0, v0, b)
    check("buy_fill tokens = (b / (1 + fee)) / (s0 (1 + curve_sol / v0))", tok,
          (b / 1.0125) / (s0 * (1.0 + (b / 1.0125) / v0)), 1e-6 * tok)
    check("buy_fill capital = b + fixed_buy", paid, b + rx.FIX_BUY, 1e-15)
    want = sell_value_proceeds(tok * s1, v1, rx.FEE, rx.FIX_SELL, rx.FIX_CLOSE) - paid
    check("y_engine = sell_proceeds(buy_fill) - capital", rx.y_engine(s0, v0, s1, v1, b),
          want, 1e-15)

    print("\n== every copy agrees with it")
    check("r1b_exit.y_legs, one full leg", rb.y_legs(s0, v0, [(1.0, s1, v1)], b),
          rx.y_engine(s0, v0, s1, v1, b), 1e-15)
    F = pd.DataFrame({"s0": [s0], "v0": [v0], "s1": [s1], "v1": [v1]})
    check("r1g_size.price, one row", float(sz.price(F, b)[0]), rx.y_engine(s0, v0, s1, v1, b),
          1e-15)
    check("mid-tape mt_d8_replay.y_engine", d8.y_engine(s0, v0, s1, v1, b),
          # that node charges 2 x FIX rather than per side plus the close, by its own convention
          rx.y_engine(s0, v0, s1, v1, b) + (rx.FIX_BUY + rx.FIX_SELL + rx.FIX_CLOSE) - 2 * rx.FIX,
          1e-15)
    # study-kernel/kernel.py::net works in reserves: spot = vsol^2 / K
    for v0_, v1_ in ((60.0, 66.0), (90.0, 85.0), (40.0, 80.0)):
        a_, b_ = v0_ ** 2 / sk.K, v1_ ** 2 / sk.K
        got = rx.y_engine(a_, v0_, b_, v1_, sk.B_DEFAULT) + (rx.FIX_BUY + rx.FIX_SELL
                                                             + rx.FIX_CLOSE) - 2 * sk.FIX
        check("study-kernel net(%g, %g)" % (v0_, v1_), got, sk.net(v0_, v1_), 2e-6)

    print("\n== against the engine's own replays, where one is on disk")
    for csv in ("r1v2_eng_study_exact.csv", "r1v2_eng_base_study_exact.csv"):
        p = DATA / csv
        if not p.exists():
            print("  %-52s skipped (not on disk)" % csv)
            continue
        E = pd.read_csv(p)
        y = np.array([rx.y_engine(a, b_, c, d) for a, b_, c, d in
                      zip(E.entry_price, E.entry_reserve, E.exit_price, E.exit_reserve)])
        err = float(np.abs(y - E.pnl_sol.to_numpy()).mean())
        check("%s mean |error| over %d positions" % (csv, len(E)), err, 0.0, 1e-4)

    print("\n%s" % ("DRIFT in %d check(s): %s" % (len(FAIL), ", ".join(FAIL)) if FAIL
                    else "every round-trip formula in this tree matches the engine"))
    return 1 if FAIL else 0


if __name__ == "__main__":
    sys.exit(main())
