# -*- coding: utf-8 -*-
"""状态锁原语（Action Locks）：统一管理单位的行动约束锁（禁移动/禁开火/禁牵引/禁发报/禁休整/禁架设）。"""

from __future__ import annotations

ACTION_LOCKS = (
    "hold_still",
    "forbid_move",
    "forbid_fire",
    "forbid_tow",
    "forbid_radio",
    "forbid_rest",
    "forbid_deploy",
)


def empty_locks():
    return {k: False for k in ACTION_LOCKS}


def extract_locks(spec):
    out = empty_locks()
    if not spec:
        return out
    for k in ACTION_LOCKS:
        if spec.get(k):
            out[k] = True
    if out["hold_still"]:
        out["forbid_move"] = True
    return out


def merge_locks(*sources):
    out = empty_locks()
    for src in sources:
        if not src:
            continue
        for k in ACTION_LOCKS:
            if src.get(k):
                out[k] = True
    if out["hold_still"]:
        out["forbid_move"] = True
    return out


def is_locked(locks, key):
    return bool((locks or {}).get(key, False))
