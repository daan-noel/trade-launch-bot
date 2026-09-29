"""Print classes: the yes / no tests a study fires on, one per print (derive 5.1 scan).

  classes(R, is_pro_b, who)   a (class x print) bool matrix for one coin, in CLASSES order:
                              WHO printed, then this printer's past, then priced screens
  pro_builds(T)               per build: an operator structure (a pro)
  who_builds(T) / who_map()   per build: its WHO flags (tool, nonce, direct, seed racer), read from
                              the lake ix_labels and cached in data/build_who.parquet

Every class is defined once, in plain words, in _!___terms.md "Print classes a study fires on".
Whether a class is a trader's rule is its hit rate and cover (toolkit/hitrate.py, derive 5.0).
The wallets' own prints are excluded from every class but NODE(diag), a diagnostic that can never
be a term (law 20).
"""
from __future__ import annotations

import json

import numpy as np
import pandas as pd
import pyarrow.compute as pc
import pyarrow.parquet as pq

from .lake_export import build_core
from .paths import DATA, LAKE

BURST_GAP = 0.4
SELLER_RECENT_S = 30.0
STRUCT_SILENCE_SLOTS = 10
# Derive 5.1 scan: WHO, then this printer's past, then priced screens.
WHO = ["tool", "nonce", "direct", "pro", "seed_racer"]
HISTORY = ["structure_burst", "seller_recent", "seller_loss", "clip_step_up",
           "struct_first_here", "alone_in_slot", "two_struct_slot", "buy_after_sells"]
PRICED = ["any", "buy", "buy>=0.5", "buy>=1", "sell", "sell>=0.5", "sell>=1",
          "up>=1%", "up>=3%", "down>=1%", "down>=2%", "new_build", "burst_start",
          "NODE(diag)"]
CLASSES = WHO + HISTORY + PRICED

APP_INFRA = ("Compute Budget", "System Program", "Token Program", "Associated Token",
             "Memo Program")
# Public trading apps (inventory tool list + the routers the engine marks).
TOOLS = ("Axiom Trade", "Photon", "Bloom Router", "Trojan Trade", "Terminal", "GMGN",
         "BullX", "Nova", "Padre")
WHO_CACHE = "build_who.parquet"


def pro_builds(T):
    """One-operator recipes: >= 200 prints from <= 50 wallets (a bot's own trade-ix)."""
    nb = len(T.builds)
    cnt = np.bincount(T.build, minlength=nb)
    wpb = pd.DataFrame({"b": T.build, "w": T.wallet}).drop_duplicates()
    nwal = np.bincount(wpb.b.to_numpy(), minlength=nb)
    return (cnt >= 200) & (nwal <= 50)


def _who_flags(labels_json):
    """(tool, nonce-buyer, direct, seed) from one lake ix_labels JSON string."""
    if not labels_json:
        return False, False, False, False
    try:
        labs = json.loads(labels_json)
    except (TypeError, ValueError, json.JSONDecodeError):
        return False, False, False, False
    nonce = any("AdvanceNonceAccount" in x for x in labs)
    seed = any("CreateAccountWithSeed" in x for x in labs)
    tool = any(any(t in x for t in TOOLS) for x in labs)
    app = None
    for x in labs:
        p = x.split(": ", 1)[0]
        if not any(p.startswith(i) for i in APP_INFRA):
            app = p
            break
    direct = (app is None or app == "Pump.Fun") and not seed
    return tool, nonce and not seed, direct, seed


def who_map(rebuild=False):
    """build_core hash -> (tool, nonce, direct, seed). Cached from lake ix_labels."""
    path = DATA / WHO_CACHE
    if path.exists() and not rebuild:
        D = pd.read_parquet(path)
        return {str(h): (bool(t), bool(n), bool(d), bool(s))
                for h, t, n, d, s in zip(D.build, D.tool, D.nonce, D.direct, D.seed)}
    rows = {}
    days = sorted((LAKE / "trades").glob("dt=*"))
    for i, day in enumerate(days):
        t = pq.read_table(str(day / "data.parquet"), columns=["ix_labels"])
        uniq = pc.unique(t.column("ix_labels")).to_pylist()
        for lab in uniq:
            h = build_core(lab)
            if not h or h in rows:
                continue
            rows[h] = _who_flags(lab)
        print("  who-cache %s  unique hashes %d  (%d/%d)"
              % (day.name, len(rows), i + 1, len(days)), flush=True)
    D = pd.DataFrame([(h, *f) for h, f in rows.items()],
                     columns=["build", "tool", "nonce", "direct", "seed"])
    DATA.mkdir(exist_ok=True)
    D.to_parquet(path, index=False)
    return {str(h): (bool(t), bool(n), bool(d), bool(s))
            for h, t, n, d, s in zip(D.build, D.tool, D.nonce, D.direct, D.seed)}


def who_builds(T):
    """Per unique tape build: tool / nonce / direct / seed flags. `_miss` is unmatched hashes."""
    M = who_map()
    n = len(T.builds)
    out = {k: np.zeros(n, dtype=bool) for k in ("tool", "nonce", "direct", "seed")}
    miss = 0
    for i, h in enumerate(T.builds):
        row = M.get(str(h))
        if row is None:
            miss += 1
            continue
        out["tool"][i], out["nonce"][i], out["direct"][i], out["seed"][i] = row
    out["_miss"] = miss
    return out


def _history(R):
    """This-print history masks on one coin. Keys match HISTORY (plus new_build)."""
    n, pub, side, sol, bu, slot = R.n, R.pub, R.side, R.sol, R.bu, R.slot
    first = np.zeros(n, dtype=bool)
    burst = np.zeros(n, dtype=bool)
    step = np.zeros(n, dtype=bool)
    after = np.zeros(n, dtype=bool)
    nstruct = np.zeros(n, dtype=np.int32)
    seen = set()
    last_slot = {}
    last_buy = {}
    sell_run = 0
    i = 0
    while i < n:
        sl = int(slot[i])
        j = i + 1
        while j < n and int(slot[j]) == sl:
            j += 1
        nstruct[i:j] = len(np.unique(bu[i:j]))
        i = j
    for i in range(n):
        b = int(bu[i])
        sl = int(slot[i])
        if b not in seen:
            first[i] = True
            seen.add(b)
        prev = last_slot.get(b)
        if prev is not None and side[i] == 1 and (sl - prev) >= STRUCT_SILENCE_SLOTS:
            burst[i] = True
        lb = last_buy.get(b)
        if side[i] == 1 and lb is not None and sol[i] > lb:
            step[i] = True
        last_slot[b] = sl
        if side[i] == 1:
            last_buy[b] = float(sol[i])
        if pub[i]:
            if side[i] == 1:
                if sell_run >= 2:
                    after[i] = True
                sell_run = 0
            else:
                sell_run += 1
    _, shold, spnl, _ = R.holders_and_seller(pub & (side == -1))
    sell = pub & (side == -1)
    return dict(
        structure_burst=pub & burst,
        seller_recent=sell & np.isfinite(shold) & (shold <= SELLER_RECENT_S),
        seller_loss=sell & np.isfinite(spnl) & (spnl < 0.0),
        clip_step_up=pub & (side == 1) & step,
        struct_first_here=pub & first,
        new_build=pub & first,
        alone_in_slot=pub & (nstruct == 1),
        two_struct_slot=pub & (nstruct == 2),
        buy_after_sells=pub & after,
    )


def classes(R, is_pro_b, who=None):
    """Yes/no on this print for every CLASSES row. `who` is who_builds(T); None => WHO off."""
    pub, side, sol, mv, bu = R.pub, R.side, R.sol, R.mv, R.bu
    gap = np.concatenate(([1e9], np.diff(R.tm)))
    H = _history(R)
    z = np.zeros(R.n, dtype=bool)
    if who is None:
        who = {k: np.zeros(int(bu.max()) + 1 if len(bu) else 1, dtype=bool)
               for k in ("tool", "nonce", "direct", "seed")}
    masks = {
        "tool": pub & who["tool"][bu],
        "nonce": pub & who["nonce"][bu],
        "direct": pub & who["direct"][bu],
        "pro": pub & is_pro_b[bu],
        "seed_racer": pub & who["seed"][bu],
        "structure_burst": H["structure_burst"],
        "seller_recent": H["seller_recent"],
        "seller_loss": H["seller_loss"],
        "clip_step_up": H["clip_step_up"],
        "struct_first_here": H["struct_first_here"],
        "alone_in_slot": H["alone_in_slot"],
        "two_struct_slot": H["two_struct_slot"],
        "buy_after_sells": H["buy_after_sells"],
        "any": pub,
        "buy": pub & (side == 1),
        "buy>=0.5": pub & (side == 1) & (sol >= 0.5),
        "buy>=1": pub & (side == 1) & (sol >= 1.0),
        "sell": pub & (side == -1),
        "sell>=0.5": pub & (side == -1) & (sol >= 0.5),
        "sell>=1": pub & (side == -1) & (sol >= 1.0),
        "up>=1%": pub & (mv >= 1.0),
        "up>=3%": pub & (mv >= 3.0),
        "down>=1%": pub & (mv <= -1.0),
        "down>=2%": pub & (mv <= -2.0),
        "new_build": H["new_build"],
        "burst_start": pub & (gap >= BURST_GAP) & (side == 1),
        "NODE(diag)": R.mine,
    }
    C = np.empty((len(CLASSES), R.n), dtype=bool)
    for i, name in enumerate(CLASSES):
        C[i] = masks.get(name, z)
    return C

