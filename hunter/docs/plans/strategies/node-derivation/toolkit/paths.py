"""Where everything lives. One place, so a moved folder is one edit."""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]          # node-derivation/
STRATEGIES = ROOT.parent                             # hunter/docs/plans/strategies/
HUNTER = Path(__file__).resolve().parents[5]         # hunter/
SHARED = STRATEGIES / "study-kernel"                 # the shared kernel and the study tape
DATA = ROOT / "data"                                 # this folder's tables and outputs (not tracked)
LAKE = HUNTER / "lake-data"                          # the sealed parquet lake
LOCAL = HUNTER / "_local"                            # the roster (solo-traders.csv)


def data(name) -> Path:
    """A data file by name: this folder's data/ first, the shared study-kernel/ second.
    A name found in neither resolves to data/ - where every output is written."""
    p = DATA / name
    if p.exists():
        return p
    s = SHARED / name
    return s if s.exists() else p
