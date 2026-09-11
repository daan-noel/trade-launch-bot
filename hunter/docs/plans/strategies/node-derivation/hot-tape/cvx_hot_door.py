"""Hot-tape node, step 10: THE DOOR. Which token groups do their trades live in?

Evidence 1.7 located the money: the within-mint model reproduces their moment out of sample
(AUC 0.72) and books -4 to -11 %/trade, and splitting its fires puts **11.6 points in which COIN**
- on a coin the node touches -2.63 %, on a coin it never touches -14.21 %. The event slot is
answered and the door slot is empty.

This is the door census, at coin level, on data the event study never saw. The user's framing:
find the token GROUPS their trades live in and fire only inside them.

TWO RULES THIS SCRIPT OBEYS, because both have burned this program before:

  * KNOWABLE AT DECISION TIME. `his`, `mfe`, `end_off`, `peak_in_first_run` in cvx_tok and
    `reply_count`, `ath_mc`, `complete`, `is_banned`, `last_trade_ts` in cvx_pump are OUTCOMES
    read after the fact. A door built on any of them is a lookahead, not a rule. They are
    excluded from every grouping here and reported nowhere.
  * A FAIR DENOMINATOR. The touch rate must be measured against coins the rule could actually
    fire on - at least 30 prints and 60 s of life - not against the whole tape, where dead coins
    inflate every concentration.

The prior on record is against this: the same census over all 26 solo traders found their 143
creation sequences cover 95.3 % of coins and 97.3 % of prints, concentration **1.02**, and -23.07
SOL as a door (6.8). It is the market, not a filter. This run asks whether the six hot-tape
wallets alone are different, and reports concentration on COINS and on TRADES separately because
those two answer different questions.

Their wallets are the instrument (7.4 law 20). A group is a public token fact; their coin list
never becomes a term.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import json
import time
from pathlib import Path
from urllib.parse import urlparse

import numpy as np
import pandas as pd

from cvx import DAY0, show
from tape import Tape
from cvx_hot_event import node_ids

MIN_PRINTS = 30
MIN_LIFE = 60.0

pd.set_option("display.width", 430)
pd.set_option("display.max_rows", 500)
pd.set_option("display.max_columns", 40)


def cgroup_of(seq, n_ix) -> str:
    if seq is None or (isinstance(seq, float) and not np.isfinite(seq)):
        return "?"
    labels = seq
    if isinstance(seq, str):
        try:
            labels = json.loads(seq)
        except json.JSONDecodeError:
            return "?"
    if not isinstance(labels, (list, tuple)) or not labels:
        return "?"
    n = int(n_ix) if pd.notna(n_ix) else len(labels)
    last = str(labels[-1])
    tail = last.split(":")[-1].strip() if ":" in last else last
    return "%dix:%s" % (n, tail)


def host_of(u):
    if not isinstance(u, str) or not u:
        return "?"
    try:
        h = urlparse(u).netloc.lower()
        return h or "?"
    except ValueError:
        return "?"


def census(df, col, title, min_coins=40, top=18):
    """Touch rate and concentration by group, on COINS and on TRADES."""
    g = df.groupby(col, dropna=False).agg(
        coins=("touched", "size"), touched=("touched", "sum"), trades=("ntrades", "sum"))
    g = g[g.coins >= min_coins]
    if g.empty:
        return None
    base_touch = float(df.touched.mean())
    tot_trades = float(df.ntrades.sum())
    tot_coins = float(len(df))
    g["touch_pct"] = (100 * g.touched / g.coins).round(2)
    g["touch_lift"] = (g.touch_pct / (100 * base_touch)).round(2)
    g["share_of_coins"] = (100 * g.coins / tot_coins).round(2)
    g["share_of_trades"] = (100 * g.trades / tot_trades).round(2)
    g["conc_trades"] = (g.share_of_trades / g.share_of_coins).round(2)
    g = g.sort_values("trades", ascending=False).head(top)
    print("\n=== %s   (base touch rate %.2f %%)" % (title, 100 * base_touch), flush=True)
    print(g[["coins", "touched", "touch_pct", "touch_lift", "share_of_coins",
             "share_of_trades", "conc_trades"]].to_string(), flush=True)
    return g


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    print("tape %s prints  %s coins  %ds"
          % (f"{T.n:,}", f"{len(T.mints):,}", time.time() - t0), flush=True)

    n_prints = T.end - T.start
    life = np.array([T.t[e - 1] - T.t[s] for s, e in zip(T.start, T.end)])
    is_node = np.isin(T.wallet, NODE)
    nb = is_node & (T.side == 1)
    ntrades = np.bincount(T.run_of[nb], minlength=len(T.mints))

    # early-life facts, knowable long before most fires
    res60, pr60 = np.zeros(len(T.mints)), np.zeros(len(T.mints))
    for r in range(len(T.start)):
        a, b = T.start[r], T.end[r]
        j = np.searchsorted(T.t[a:b], T.t[a] + 60.0, side="right")
        res60[r] = T.v[a + max(j - 1, 0)]
        pr60[r] = j

    D = pd.DataFrame(dict(mint=T.mints, prints=n_prints, life=life, ntrades=ntrades,
                          res60=res60, pr60=pr60))
    D["touched"] = D.ntrades > 0
    uni = D[(D.prints >= MIN_PRINTS) & (D.life >= MIN_LIFE)].copy()
    print("universe: coins with >= %d prints and >= %.0f s of life: %s of %s   they touch %s"
          % (MIN_PRINTS, MIN_LIFE, f"{len(uni):,}", f"{len(D):,}",
             f"{int(uni.touched.sum()):,}"), flush=True)

    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    sw = pd.read_parquet(data_file("cvx_swdoor.parquet")).drop_duplicates("mint").set_index("mint")
    meta = pd.read_parquet(data_file("cvx_meta.parquet")).drop_duplicates("mint").set_index("mint")
    uri = pd.read_parquet(data_file("cvx_uri.parquet")).drop_duplicates("mint").set_index("mint")
    pmp = pd.read_parquet(data_file("cvx_pump.parquet")).drop_duplicates("mint").set_index("mint")

    a = tok.reindex(uni.mint)
    uni["cgroup"] = [cgroup_of(s, n) for s, n in zip(a.seq.to_numpy(), a.ix_count.to_numpy())]
    uni["ixb"] = ["%dix" % int(x) if pd.notna(x) else "?" for x in a.ix_count.to_numpy()]
    uni["excl"] = a.excl.fillna("keep").to_numpy()
    uni["create_ata"] = a.create_ata.fillna(False).astype(bool).to_numpy()
    uni["init_lp"] = a.init_lp.fillna(False).astype(bool).to_numpy()

    b = sw.reindex(uni.mint)
    uni["bundler"] = b.bundler_group.fillna(False).to_numpy()
    uni["slow_wall"] = b.slow_wall.fillna(False).to_numpy()
    uni["c_build"] = b.c_build.fillna("?").to_numpy()
    uni["n_prev"] = pd.cut(b.n_prev.fillna(-1).to_numpy(), [-2, 0, 5, 20, 50, 200, 1e9])
    uni["sw_prev"] = pd.cut(b.sw_prev.fillna(-1).to_numpy(), [-2, 0, 0.02, 0.05, 0.10, 0.25, 1.1])

    m = meta.reindex(uni.mint)
    for c in ("has_twitter", "has_website", "has_telegram", "has_community", "show_name"):
        uni[c] = (m[c].fillna(False).astype(bool).to_numpy()
                  if c in m.columns else np.zeros(len(uni), bool))
    uni["desc_len"] = pd.cut(m.desc_len.fillna(-1).to_numpy(), [-2, 0, 30, 100, 300, 1e9])
    uni["n_keys"] = pd.cut(m.n_keys.fillna(-1).to_numpy(), [-2, 0, 4, 6, 8, 1e9])

    p = pmp.reindex(uni.mint)
    for c in ("nsfw", "has_username", "web_is_x"):
        uni[c] = (p[c].fillna(False).astype(bool).to_numpy()
                  if c in p.columns else np.zeros(len(uni), bool))
    uni["web_host"] = p.web_host.fillna("?").to_numpy() if "web_host" in p.columns else "?"
    uni["uri_host"] = [host_of(x) for x in uri.reindex(uni.mint).uri.to_numpy()]

    uni["res60_b"] = pd.cut(uni.res60, [0, 32, 35, 40, 50, 70, 1e9])
    uni["pr60_b"] = pd.cut(uni.pr60, [0, 10, 30, 80, 200, 1e9])
    uni["doc"] = uni.has_website & uni.has_telegram

    print("\nnode trades in the universe: %s on %s coins"
          % (f"{int(uni.ntrades.sum()):,}", f"{int(uni.touched.sum()):,}"), flush=True)

    for col, title in (("cgroup", "creation ix structure"),
                       ("ixb", "creation ix_count"),
                       ("excl", "universe class"),
                       ("bundler", "bundler creation group"),
                       ("slow_wall", "slow-wall launch door"),
                       ("n_prev", "launch build: coins launched the PREVIOUS day"),
                       ("sw_prev", "launch build: slow-wall RATE the previous day"),
                       ("res60_b", "reserve at age 60 s"),
                       ("pr60_b", "prints in the first 60 s"),
                       ("doc", "metadata document (website AND telegram)"),
                       ("has_website", "has a website"), ("has_telegram", "has a telegram"),
                       ("has_twitter", "has a twitter"), ("has_community", "has a community"),
                       ("desc_len", "description length"), ("n_keys", "metadata key count"),
                       ("nsfw", "nsfw flag"), ("has_username", "creator has a username"),
                       ("web_is_x", "website is an x.com link"),
                       ("uri_host", "metadata URI host"), ("web_host", "website host"),
                       ("create_ata", "creation creates an ATA"), ("init_lp", "init_lp flag")):
        try:
            census(uni, col, title)
        except (KeyError, ValueError, TypeError) as e:
            print("\n=== %s SKIPPED (%s)" % (title, e), flush=True)

    out = uni.copy()
    for c in out.columns:
        if str(out[c].dtype) == "category" or out[c].dtype == object:
            out[c] = out[c].astype(str)
    out.to_parquet(data_file("cvx_hot_door_coins.parquet"), index=False)
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
