"""Derive step 2: the trader read as a person, before any scan.

A scan finds what is frequent in his prints; a portrait finds what he means. This module produces
the two halves of derive 3 and nothing else: it describes, it never scores. Its numbers illustrate
a hypothesis, and a verdict comes only from the tests steps 3-6 run on the clauses it names.

  shape(S, w, E)           derive 3.1, his shape in numbers: positions, episodes per coin,
                           re-entry rate, hold p10 / p50 / p90, clip, entry reserve, entry age,
                           how many he holds at once, the tail share of his own net
  trades(S, w, E, n)       derive 3.2, the 2n trades to read print by print: the n best by SOL
                           and the n losers nearest his median loss, picked by that rule and never
                           by eye. One row a trade, with the coin, the minute before, the print
                           just before his buy, what happened inside his hold, and the print just
                           before his sell
  markdown(S, w, E, ...)   both, as the case file's section 1 tables, ready to paste

`w` is the member's wallet id (`S.wallet(prefix)`), `E` its episodes (`seat.episodes`).
`is_pro_b` / `who` are `trigger.pro_builds(S.T)` / `trigger.who_builds(S.T)`; pass them to fill the
"who printed" columns, leave them out and those columns read `-`.

Every column is built from prints before the one it describes, and his own prints are excluded
from every public count (law 3): the portrait reads the tape he read, not the tape he made.
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from kernel import B_DEFAULT as B, net

from .facts import Run

MINUTE = 60.0


def _pct(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return ((a / b) ** 2 - 1.0) * 100.0


def _open_at_once(E):
    """The most positions he holds at the same moment, over the whole tape."""
    d = E[(E.ks >= 0) & np.isfinite(E.t_open) & np.isfinite(E.t_close)]
    if d.empty:
        return 0
    ev = np.concatenate([d.t_open.to_numpy(), d.t_close.to_numpy()])
    mv = np.concatenate([np.ones(len(d)), -np.ones(len(d))])
    o = np.argsort(ev, kind="stable")
    return int(np.max(np.cumsum(mv[o]))) if len(ev) else 0


def _times(S, E):
    """Wall-clock open and close of each episode, needed for `how many at once`."""
    T = S.T
    t0 = np.full(len(E), np.nan); t1 = np.full(len(E), np.nan)
    for r, g in E.groupby("run"):
        a = T.start[int(r)]
        t = T.t[a:T.end[int(r)]]
        i = E.index.get_indexer(g.index)
        t0[i] = t[g.k.to_numpy().astype(int)]
        ks = g.ks.to_numpy().astype(int)
        t1[i] = np.where(ks >= 0, t[np.maximum(ks, 0)], np.nan)
    return E.assign(t_open=t0, t_close=t1)


def shape(S, w, E, clip=B):
    """Derive 3.1. One row: what kind of trader he is, before any claim about why."""
    E = _times(S, E)
    C = E[E.ks >= 0]
    sol = np.full(len(C), np.nan); y = np.full(len(C), np.nan)
    for r, g in C.groupby("run"):
        R = Run(S, int(r))
        i = C.index.get_indexer(g.index)
        sol[i] = [float(R.sol[int(k)]) for k in g.k]
        # his own decisions through OUR kernel at OUR clip: we buy the reserve his print met
        # (vbef), so we pay our impact and not his. The basis own_exit.book uses, so the two agree
        y[i] = [net(float(R.vbef[int(k)]), float(R.v[int(ks) - 1]), clip)
                for k, ks in zip(g.k, g.ks)]
    per_coin = C.groupby("run").size()
    first = ~C.duplicated("run")
    rank = pd.Series(-y).rank(method="first").to_numpy() if len(y) else np.array([])
    return pd.Series(dict(
        positions=len(C), coins=int(C.run.nunique()), still_open=int((E.ks < 0).sum()),
        per_day=round(len(C) / max(S.days, 1e-9), 1),
        episodes_per_coin=round(float(per_coin.mean()), 2),
        re_entry_pct=round(100.0 * float((~first).mean()), 1) if len(C) else np.nan,
        hold_p10=round(float(C.held.quantile(0.10)), 1),
        hold_p50=round(float(C.held.quantile(0.50)), 1),
        hold_p90=round(float(C.held.quantile(0.90)), 1),
        hold_p90_over_p50=round(float(C.held.quantile(0.90) / max(C.held.quantile(0.50), 1e-9)), 1),
        clip_p50=round(float(np.nanmedian(sol)), 3),
        entry_vsol_p50=round(float(C.v.median()), 1),
        entry_age_p50=round(float(C.age.median()), 0),
        open_at_once=_open_at_once(E),
        median_trade_pct=round(float(C.pnl.median()), 2),
        win_pct=round(100.0 * float((C.pnl > 0).mean()), 1),
        lose_20_pct=round(100.0 * float((C.pnl <= -20.0).mean()), 1),
        his_sol_at_our_clip=round(float(y.sum()), 2) if len(y) else np.nan,
        top1_pct_of_his_net=round(100.0 * float(y[rank <= max(round(0.01 * len(y)), 1)].sum()
                                                / y.sum()), 1) if len(y) and y.sum() > 0 else np.nan,
    ))


def _who(R, k, is_pro_b, who):
    if who is None or is_pro_b is None:
        return "-"
    b = int(R.bu[k])
    tags = [nm for nm, key in (("tool", "tool"), ("nonce", "nonce"), ("direct", "direct"),
                               ("seed racer", "seed")) if who[key][b]]
    if is_pro_b[b]:
        tags.append("operator")
    return "+".join(tags) if tags else "-"


def trades(S, w, E, n=10, is_pro_b=None, who=None, clip=B):
    """Derive 3.2: the 2n trades to read print by print, and the facts to read on each.

    The pick is a rule, not a judgement: the n largest by SOL at our clip, and the n losers whose
    percent sits nearest his median loss. Reading only the disasters teaches the tail; reading a
    typical loser teaches the decision."""
    C = E[E.ks >= 0].copy()
    y = np.full(len(C), np.nan)
    for r, g in C.groupby("run"):
        R = Run(S, int(r))
        y[C.index.get_indexer(g.index)] = [net(float(R.vbef[int(k)]), float(R.v[int(ks) - 1]), clip)
                                           for k, ks in zip(g.k, g.ks)]
    C["sol"] = y
    losers = C[C.pnl <= 0]
    med = float(losers.pnl.median()) if len(losers) else np.nan
    best = C.nlargest(min(n, len(C)), "sol").assign(pick="best")
    typ = (losers.assign(d=(losers.pnl - med).abs()).nsmallest(min(n, len(losers)), "d")
           .drop(columns="d").assign(pick="typical loss")) if len(losers) else C.iloc[:0]
    P = pd.concat([best, typ]).sort_values(["pick", "sol"], ascending=[True, False])
    rows = []
    for r, g in P.groupby("run"):
        R = Run(S, int(r))
        hc = R.holders()
        for i, k, ks in zip(g.index, g.k.to_numpy(), g.ks.to_numpy()):
            k, ks = int(k), int(ks)
            lo = int(R.j(MINUTE)[k])
            pub = R.pub[lo:k]
            side = R.side[lo:k]; sl = R.sol[lo:k]
            j = k - 1
            while j >= 0 and R.mine[j]:            # the last print he could have answered
                j -= 1
            hold = np.arange(k + 1, max(ks, k + 1))
            hold = hold[R.pub[hold]] if len(hold) else hold
            v0 = float(R.v[k])
            up = _pct(R.v[k:ks], v0) if ks > k else np.array([0.0])
            js = ks - 1
            while js > k and R.mine[js]:
                js -= 1
            rows.append(dict(
                pick=P.pick[i], coin=int(r), day=int(R.day[k]),
                age_s=round(float(R.age[k]), 0), vsol=round(v0, 1),
                best_vsol_before=round(float(R.rmax[max(k - 1, 0)]), 1),
                holders=int(hc[k]),
                min_prints=int(pub.sum()),
                min_buy_sol=round(float(sl[pub & (side == 1)].sum()), 2),
                min_sell_sol=round(float(sl[pub & (side == -1)].sum()), 2),
                pre_who=_who(R, j, is_pro_b, who) if j >= 0 else "-",
                pre_side="buy" if j >= 0 and R.side[j] == 1 else ("sell" if j >= 0 else "-"),
                pre_sol=round(float(R.sol[j]), 3) if j >= 0 else np.nan,
                pre_move_pct=round(float(R.mv[j]), 2) if j >= 0 else np.nan,
                pre_quiet_s=round(float(R.tm[j] - R.tm[j - 1]), 3) if j >= 1 else np.nan,
                his_lag_s=round(float(R.tm[k] - R.tm[j]), 3) if j >= 0 else np.nan,
                hold_pub_buys=int((R.side[hold] == 1).sum()) if len(hold) else 0,
                hold_best_pct=round(float(up.max()), 1),
                hold_worst_pct=round(float(up.min()), 1),
                exit_who=_who(R, js, is_pro_b, who) if js > k else "-",
                exit_side="buy" if js > k and R.side[js] == 1 else ("sell" if js > k else "-"),
                exit_sol=round(float(R.sol[js]), 3) if js > k else np.nan,
                from_best_pct=round(float(_pct(R.v[ks - 1], R.v[k:ks].max())), 1) if ks > k else np.nan,
                held_s=round(float(P.held[i]), 1), pnl_pct=round(float(P.pnl[i]), 1),
                sol=round(float(P.sol[i]), 4)))
    return pd.DataFrame(rows).sort_values(["pick", "sol"], ascending=[True, False])


def markdown(S, w, E, n=10, is_pro_b=None, who=None, clip=B, name="<wallet>"):
    """Derive 3.1 and 3.2 as the case file's section 1, ready to paste. The reading itself - what is
    the same in the best entries, where the losers differ, what he never buys - is written by hand
    under it: the table is what a person reads, not the answer."""
    sh = shape(S, w, E, clip=clip)
    P = trades(S, w, E, n=n, is_pro_b=is_pro_b, who=who, clip=clip)
    out = ["## 1. His logic (the portrait, derive 3)", "",
           "> He buys **<what, when>** because he expects **<who spends next, and why>**; he skips",
           "> **<what>** because **<reason>**; he leaves when **<what>** because then",
           "> **<the reason is gone>**.", "",
           "**Who he is (%s, tape `%s`, %.2f days).**" % (name, S.name, S.days), "",
           "| fact | value |", "| --- | ---: |"]
    out += ["| %s | %s |" % (k.replace("_", " "), v) for k, v in sh.items()]
    out += ["", "| clause | slot | his likely reason | the yes / no test | step | answer |",
            "| --- | --- | --- | --- | --- | --- |",
            "| | E | | | 5.1 | |", "| | D / P | | | 6.1, 7, 9 | |", "| | X | | | 8.0 | |", "",
            "### The %d trades" % len(P), "",
            "The %d best by SOL and the %d losers nearest his median loss, picked by that rule."
            % (n, n), ""]
    out += [P.to_markdown(index=False)] if hasattr(P, "to_markdown") else [P.to_string(index=False)]
    out += ["", "What is the same in the best entries: <...>.",
            "Where the losers differ: <at the entry / only after>.",
            "What he never buys: <...>."]
    return "\n".join(out)
