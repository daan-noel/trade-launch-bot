"""Hot-tape node, step 17: THE LEVEL-ANCHORED ENTRY, public, on the full tape.

THE STORY. The hot-tape node re-enters: 76.5 % of its episodes are a return to a coin it has
already round-tripped, 61.4 % of those sit BELOW its own previous exit, and the August study of
one of its members found the payoff monotone in that depth. That anchor is not a print - it is a
LEVEL, set minutes earlier by a high the coin already made. Three things follow:

  WHO    whoever buys the rebound of a pullback on a coin that is still alive
  WHY    the coin is oscillating and the cohort that pushed it has not finished
  WHAT   price has given back d % of the coin's own recent swing high
  DELAY  the level was set minutes ago; the rebound SOL has not landed yet, and a rule waiting
         at a level is not racing anyone to a print (7.2: a state true for seconds is the only
         anchor that can be anything but a FOLLOW seat)

WHY THIS IS NOT S12. The mid-tape after-flush event (6.14) requires -20 % from the token's peak
so far AND stillness for 10 slots, on a mid-tape coin. This fires on the coin's own SWING high
with an explicit re-arm, at four depths, in three timings - at the crossing (the knife), at the
turn (r % off the low), and after stillness - so the DELAY question is answered by the data
rather than assumed.

THE RE-ARM IS THE WHOLE DIFFERENCE between this and every dip-buy this program has refuted. The
August TURN family fired on a SLIDING low and re-fired every 4 % wiggle down a collapse; 42 % of
its entries died inside three seconds. Here the machine must see a NEW swing high before it can
fire again, so one down-leg produces exactly one ticket.

  D  none | live60 (reserve at age 60 s in 50-70) | busy60 (>= 200 prints in the first minute)
  E  swing pullback of d %, in three timings, one fire per down-leg
  P  none | reserve band | age band
  X  cap15 . cap60 . cap300 . trail40 c600     (scalper control and harvester, side by side)
  R  one position per coin at a time, unlimited otherwise      S 0.2
  seat  fire at the crossing print, fill = last print landed by that print + 115 ms, both legs
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import os
import time
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import fill_idx, net
from cvx import DAY0, cell, show
from tape import Tape

B = 0.2
LAG = 0.115
DEPTHS = (15.0, 25.0, 40.0)
REBOUND = 4.0          # price percent off the swing low that counts as the turn
STILL_S = 4.0          # seconds with no new low that counts as stillness

pd.set_option("display.width", 470)
pd.set_option("display.max_rows", 900)
pd.set_option("display.max_columns", 60)


def legs(t, v, d, rearm):
    """One down-leg per fire. Returns (i_cross, i_turn, i_still, v_hi) per leg.

    up-phase: track the swing high. The leg opens at the first print whose price is d % under
    that high. down-phase: track the low; i_turn is the first print `REBOUND` % above it and
    i_still the first print `STILL_S` seconds after the last new low. The machine returns to the
    up-phase - and only then can it fire again - once price is `rearm` % above the leg's low.
    """
    n = len(v)
    kd = np.sqrt(1.0 - d / 100.0)
    kr = np.sqrt(1.0 + REBOUND / 100.0)
    ka = np.sqrt(1.0 + rearm / 100.0)
    out = []
    up = True
    hi = v[0]
    lo = np.inf
    lo_t = 0.0
    cross = turn = still = -1
    for i in range(1, n):
        x = v[i]
        if up:
            if x > hi:
                hi = x
            elif x <= hi * kd:
                up = False
                cross = i
                lo = x
                lo_t = t[i]
                turn = still = -1
        else:
            if x < lo:
                lo = x
                lo_t = t[i]
            else:
                if turn < 0 and x >= lo * kr:
                    turn = i
                if still < 0 and t[i] - lo_t >= STILL_S:
                    still = i
            if x >= lo * ka:
                out.append((cross, turn, still, hi))
                up = True
                hi = x
    if cross >= 0 and not up:
        out.append((cross, turn, still, hi))
    return out


def book_from(t, v, k, cap, trail=None):
    """Fire on print k, fill 115 ms later, exit on a clock or a trail. Index order throughout."""
    ei = fill_idx(t, k, LAG)
    v0 = float(v[ei])
    n = len(t)
    e = int(np.searchsorted(t, t[ei] + cap, side="right"))
    if trail is not None and e > ei + 1:
        pr = (v[ei + 1:e] / v0) ** 2
        peak = np.maximum.accumulate(np.maximum(pr, 1.0))
        hit = np.nonzero(pr <= peak * (1.0 - trail / 100.0))[0]
        if len(hit):
            x = fill_idx(t, ei + 1 + int(hit[0]), LAG)
            return net(v0, float(v[x]), B), ei, x
    j = max(min(e - 1, n - 1), ei)
    x = fill_idx(t, j, LAG)
    return net(v0, float(v[x]), B), ei, x


EXITS = (("cap15", 15.0, None), ("cap60", 60.0, None), ("cap300", 300.0, None),
         ("tr40_c600", 600.0, 40.0))


def main() -> None:
    t0 = time.time()
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    nruns = len(T.start)
    lim = int(os.environ.get("HOTLVL_LIMIT", "0"))
    rng = range(nruns if not lim else min(lim, nruns))
    print("tape %s prints  coins %s  %.2f days  %ds"
          % (f"{T.n:,}", f"{nruns:,}", days, time.time() - t0), flush=True)

    # coin-level door facts, from the first 60 s only
    live60 = np.full(nruns, np.nan)
    busy60 = np.zeros(nruns, dtype=np.int32)
    rows = []
    for r in rng:
        a, b = T.start[r], T.end[r]
        if b - a < 20 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]
        age = t - c_s[r]
        j60 = int(np.searchsorted(age, 60.0, side="right"))
        if j60 > 0:
            live60[r] = v[j60 - 1]
            busy60[r] = j60
        for d in DEPTHS:
            last_exit = -1
            for cross, turn, still, hi in legs(t, v, d, REBOUND * 3.0):
                for tag, k in (("knife", cross), ("turn", turn), ("still", still)):
                    if k < 0 or k <= last_exit:
                        continue
                    y = {}
                    xi = 0
                    for nm, cap, tr in EXITS:
                        yy, ei, xx = book_from(t, v, k, cap, tr)
                        y[nm] = yy
                        if nm == "cap60":
                            xi = xx
                    rows.append((r, int(T.day[a + k]), d, tag, float(v[k]), float(age[k]),
                                 float((v[k] / hi) ** 2 - 1.0) * 100.0,
                                 y["cap15"], y["cap60"], y["cap300"], y["tr40_c600"]))
                    if tag == "turn":
                        last_exit = xi
        if r % 20000 == 0:
            print("  run %s  fires %s  %ds" % (f"{r:,}", f"{len(rows):,}", time.time() - t0),
                  flush=True)

    F = pd.DataFrame(rows, columns=["run", "day", "d", "tag", "v", "age", "dep",
                                    "y_cap15", "y_cap60", "y_cap300", "y_tr40"])
    F["live60"] = live60[F.run.values]
    F["busy60"] = busy60[F.run.values]
    F.to_parquet(data_file("cvx_hot_lvl.parquet"), index=False)
    print("\nfires %s over %s coins  %ds"
          % (f"{len(F):,}", f"{F.run.nunique():,}", time.time() - t0), flush=True)

    def blk(d0, **kw):
        out = []
        for ycol, lbl in (("y_cap15", "cap15"), ("y_cap60", "cap60"),
                          ("y_cap300", "cap300"), ("y_tr40", "trail40")):
            dd = d0.copy()
            dd["y"] = dd[ycol]
            c = cell(dd, days=days)
            out.append(dict(exit=lbl, **kw, **c))
        return out

    for d in DEPTHS:
        for tag in ("knife", "turn", "still"):
            s = F[(F.d == d) & (F.tag == tag)]
            if len(s) < 500:
                continue
            rows2 = blk(s, depth=d, tag=tag)
            show(rows2, "E = pullback %.0f %%, timing %s   (no door, full tape)" % (d, tag))

    print("\n=== DOORS AND PERMISSIONS on the best timing, depth by depth")
    rows3 = []
    for d in DEPTHS:
        for tag in ("knife", "turn", "still"):
            s = F[(F.d == d) & (F.tag == tag)]
            if len(s) < 500:
                continue
            for nm, m in (("none", slice(None)),
                          ("live60 50-70", (s.live60 >= 50) & (s.live60 <= 70)),
                          ("busy60>=200", s.busy60 >= 200),
                          ("age 60-900", (s.age >= 60) & (s.age <= 900)),
                          ("v 42-85", (s.v >= 42.43) & (s.v <= 85)),
                          ("age60-900 & v42-85", (s.age >= 60) & (s.age <= 900)
                           & (s.v >= 42.43) & (s.v <= 85))):
                ss = s if isinstance(m, slice) else s[m]
                if len(ss) < 300:
                    continue
                for ycol, lbl in (("y_cap60", "cap60"), ("y_tr40", "trail40")):
                    dd = ss.copy()
                    dd["y"] = dd[ycol]
                    rows3.append(dict(depth=d, tag=tag, gate=nm, exit=lbl,
                                      **cell(dd, days=days)))
    show(rows3, "doors x permissions")
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
