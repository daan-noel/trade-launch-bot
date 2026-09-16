"""Bootstrap for the instant-launch scripts: the toolkit and the shared study kernel on sys.path.

Every script here imports this first. data_file(name) resolves a data file: node-derivation/data/
first, the shared study-kernel/ second; a new output goes to data/.
"""
from __future__ import annotations

import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
MID = ROOT / "mid-tape"
if str(MID) not in sys.path:
    sys.path.append(str(MID))

import toolkit  # noqa: E402,F401  - puts study-kernel on sys.path
from toolkit.paths import DATA, HUNTER, LAKE, LOCAL, SHARED  # noqa: E402,F401
from toolkit.paths import data as data_file  # noqa: E402,F401

NODE_NAME = "instant launch"
# study fires stop where the holdout's fires start (derive 2.1)
STUDY_END = datetime(2026, 9, 6, 12, 0, tzinfo=timezone.utc).timestamp()
# the solo 26 were selected on 08-27..09-03: a pick read before this day is circular
ROSTER_OUT = datetime(2026, 9, 4, 0, 0, tzinfo=timezone.utc).timestamp()
