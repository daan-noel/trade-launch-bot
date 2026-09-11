"""Mid-tape node, derive phases 1-5: who pays, shape, trigger, DELAY.

Question: among the mid-tape members, who pays at which seat, what is each member's episode
shape, which public print does 9Uq8GV react to, and is that leftover at lag_115?

[_!___derive.md] phases 1-5. Instrument: 9Uq8GV. Other members are split (law 27), not pooled.
Study tape only. Nothing is chosen on the holdout.
"""
from __future__ import annotations

import _paths  # noqa: F401
from _paths import DATA

import time

import numpy as np
import pandas as pd

from toolkit import book, seat, tapes, trigger
from toolkit.trigger import BURST_GAP, CLASSES

NODE = "mid-tape one-shot"
FOCUS = "9Uq8GV"
SKIP_CLASSES = {"any", "NODE(diag)"}
PEAK_ROWS = ["buy>=1", "buy>=0.5", "burst_start", "sell>=1", "pro", "new_build",
             "up>=3%", "down>=2%"]

pd.set_option("display.width", 500)
pd.set_option("display.max_rows", 400)
pd.set_option("display.max_columns", 40)


def _clip_p50(S, E):
    if len(E) == 0:
        return np.nan
    T = S.T
    return float(np.median([T.sol[int(T.start[int(r)] + int(k))]
                            for r, k in zip(E.run.to_numpy(), E.k.to_numpy())]))


def _max_open(S, E):
    if len(E) == 0:
        return 0
    T = S.T
    ev = []
    for r, k, ks in zip(E.run.to_numpy(), E.k.to_numpy(), E.ks.to_numpy()):
        a, b = int(T.start[int(r)]), int(T.end[int(r)])
        t0 = float(T.t[a + int(k)])
        t1 = float(T.t[a + int(ks)]) if int(ks) >= 0 else float(T.t[b - 1])
        ev.append((t0, 1))
        ev.append((t1, -1))
    ev.sort()
    cur = mx = 0
    for _, d in ev:
        cur += d
        mx = max(mx, cur)
    return mx


def _shape(S, E):
    n = len(E)
    if n == 0:
        return dict(n=0)
    per = E.groupby("run").size()
    first = E.sort_values("k").groupby("run").cumcount() == 0
    held = E.held.dropna()
    closed = E.pnl.dropna()
    return dict(
        n=n, closed=int(len(closed)), mints=int(E.run.nunique()),
        share_2plus=round(100 * float((per >= 2).mean()), 1),
        reentry=round(100 * float((~first).mean()), 1),
        age_p50=round(float(E.age.median()), 1),
        v_p50=round(float(E.v.median()), 2),
        clip_p50=round(_clip_p50(S, E), 3),
        hold_p10=round(float(held.quantile(0.10)), 1) if len(held) else np.nan,
        hold_p50=round(float(held.quantile(0.50)), 1) if len(held) else np.nan,
        hold_p90=round(float(held.quantile(0.90)), 1) if len(held) else np.nan,
        max_open=_max_open(S, E),
        own_med=round(float(closed.median()), 2) if len(closed) else np.nan,
        own_win=round(100 * float((closed > 0).mean()), 1) if len(closed) else np.nan,
    )


def _trig_fn(cls):
    def fn(R):
        pub, side, sol = R.pub, R.side, R.sol
        if cls == "buy>=1":
            return pub & (side == 1) & (sol >= 1.0)
        if cls == "buy>=0.5":
            return pub & (side == 1) & (sol >= 0.5)
        if cls == "burst_start":
            gap = np.concatenate(([1e9], np.diff(R.tm)))
            return pub & (side == 1) & (gap >= BURST_GAP)
        if cls == "sell>=1":
            return pub & (side == -1) & (sol >= 1.0)
        if cls == "sell>=0.5":
            return pub & (side == -1) & (sol >= 0.5)
        raise ValueError(cls)
    fn.__name__ = cls
    return fn


def _top_cells(lift, k=8):
    rows = []
    for cls in lift.index:
        if cls in SKIP_CLASSES:
            continue
        row = lift.loc[cls]
        if not np.isfinite(row.to_numpy(dtype=float)).any():
            continue
        bin_, v = trigger.peak(lift, cls)
        if not np.isfinite(v):
            continue
        rows.append((cls, bin_, v))
    rows.sort(key=lambda x: -x[2] if np.isfinite(x[2]) else -1e9)
    return rows[:k]


def main() -> None:
    t0 = time.time()
    S = tapes.load("study", tapes.roster(NODE))
    print("tape %s  days %.2f  instruments %s  %ds" % (
        S.name, S.days, sorted(S.ids.values()), time.time() - t0), flush=True)

    members = sorted(S.ids.values())
    w = {}
    for p in members:
        try:
            w[p] = S.wallet(p)
        except KeyError:
            print("no wallet id for %s" % p, flush=True)

    print("\n=== phase 3-4  shape and seat (clock 15) ===", flush=True)
    shape_rows = []
    for p, wid in w.items():
        E0 = seat.episodes(S, wid)
        sh = _shape(S, E0)
        sh["member"] = p
        if len(E0):
            E = seat.seat_book(S, E0, caps=(15.0,))
            E.to_parquet(DATA / ("mt_ep_%s.parquet" % p), index=False)
            for col in ("race15", "follow15"):
                L = book.ledger(E.assign(y=E[col], why="-"), S.days)
                sh[col + "_n"] = L["n"]
                sh[col + "_pct"] = L["pct"]
                sh[col + "_sol"] = L["sol"]
                sh[col + "_days"] = L["pos"]
                sh[col + "_body"] = L["body"]
                sh[col + "_top1"] = L["top1"]
                sh[col + "_med"] = round(100 * float(E[col].median()) / 0.2, 2)
            print("%s  eps %d  mints %d  2+ %s%%  re %s%%  age p50 %s  hold %s/%s/%s  "
                  "clip %s  maxopen %d  own med %+s win %s%%" % (
                      p, sh["n"], sh["mints"], sh["share_2plus"], sh["reentry"],
                      sh["age_p50"], sh["hold_p10"], sh["hold_p50"], sh["hold_p90"],
                      sh["clip_p50"], sh["max_open"], sh["own_med"], sh["own_win"]),
                  flush=True)
            print("     RACE   %+6.2f %%/trade  %s  body %s  top1 %s  med %+s" % (
                sh["race15_pct"], sh["race15_days"], sh["race15_body"],
                sh["race15_top1"], sh["race15_med"]), flush=True)
            print("     FOLLOW %+6.2f %%/trade  %s  body %s  top1 %s  med %+s" % (
                sh["follow15_pct"], sh["follow15_days"], sh["follow15_body"],
                sh["follow15_top1"], sh["follow15_med"]), flush=True)
        else:
            print("%s  no episodes on this tape" % p, flush=True)
        shape_rows.append(sh)
    pd.DataFrame(shape_rows).to_csv(DATA / "mt_p15_shape.csv", index=False)
    print("  %ds" % (time.time() - t0), flush=True)

    print("\n=== phase 5.1  excess intensity ===", flush=True)
    groups = {p: [wid] for p, wid in w.items()}
    out = trigger.excess_intensity(S, groups)
    peak_rows = []
    for p in members:
        if p not in out:
            continue
        lift, _ex, nk, nc = out[p]
        lift.to_csv(DATA / ("mt_lift_%s.csv" % p))
        tops = _top_cells(lift)
        print("\n%s  cases %d  controls %d" % (p, nk, nc), flush=True)
        print(lift.loc[[c for c in PEAK_ROWS if c in lift.index]].to_string())
        print("  top:", ", ".join("%s @ %s lift %.2f" % t for t in tops[:5]), flush=True)
        for cls, bin_, v in tops:
            peak_rows.append(dict(member=p, cls=cls, bin=bin_, lift=round(v, 2), n=nk, nc=nc))
    pd.DataFrame(peak_rows).to_csv(DATA / "mt_p15_peaks.csv", index=False)
    print("  %ds" % (time.time() - t0), flush=True)

    print("\n=== phase 5.2  seat against the trigger (focus %s) ===" % FOCUS, flush=True)
    if FOCUS not in w:
        print("focus %s not on tape" % FOCUS, flush=True)
        return
    E0 = seat.episodes(S, w[FOCUS])
    focus_lift = out[FOCUS][0]
    classes = []
    for c in [t[0] for t in _top_cells(focus_lift, k=6)] + ["buy>=1", "burst_start"]:
        if c in ("buy>=1", "buy>=0.5", "burst_start", "sell>=1") and c not in classes:
            classes.append(c)
    react_rows = []
    for cls in classes:
        R = seat.reaction(S, E0, _trig_fn(cls), max_trig=1.0)
        hit = R[R.dt.notna()]
        n = len(R)
        nh = len(hit)
        ahead = 100 * float(hit.ahead.mean()) if nh else np.nan
        dt_p50 = float(hit.dt.median()) if nh else np.nan
        y = hit.y15.dropna()
        pct = round(100 * float(y.mean()) / 0.2, 2) if len(y) else np.nan
        behind = hit[hit.ahead < 0.5]
        yb = behind.y15.dropna()
        pct_b = round(100 * float(yb.mean()) / 0.2, 2) if len(yb) else np.nan
        print("%s  hits %d/%d (%.1f%%)  dt p50 %.0f ms  ahead %.1f%%  "
              "FOLLOW-on-trig %+s %%/trade  when behind %+s" % (
                  cls, nh, n, 100 * nh / max(n, 1),
                  1000 * dt_p50 if np.isfinite(dt_p50) else np.nan,
                  ahead if np.isfinite(ahead) else np.nan,
                  pct, pct_b), flush=True)
        react_rows.append(dict(
            cls=cls, n=n, hits=nh, hit_pct=round(100 * nh / max(n, 1), 1),
            dt_p50_ms=round(1000 * dt_p50, 0) if np.isfinite(dt_p50) else np.nan,
            ahead_pct=round(ahead, 1) if np.isfinite(ahead) else np.nan,
            y15_pct=pct, y15_behind_pct=pct_b,
        ))
        hit.to_parquet(DATA / ("mt_react_%s_%s.parquet" % (FOCUS, cls.replace(">=", "ge"))),
                       index=False)
    pd.DataFrame(react_rows).to_csv(DATA / "mt_p15_react.csv", index=False)
    print("done %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
