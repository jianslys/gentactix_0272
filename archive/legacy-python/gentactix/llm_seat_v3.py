# -*- coding: utf-8 -*-
"""可选外挂 LLM 盲座席位接口（llm_seat_v3）：严禁嵌入引擎数学/物理层。

只向外部 LLM 回调喂入 `baguwen_v3.collect_commander_uplink_baguwen` 汇总后的上行战报（断连黑盒严格隔离），
支持返回「保持静默」或一条结构化下行八股文命令（当 `llm_callback` 未传入时确定性返回保持静默）。
"""

from __future__ import annotations

from . import baguwen_v3


def evaluate_llm_commander_seat(world, commander_id, llm_callback=None, auto_issue=False):
    """外挂盲座席位评估：只喂上行战报；若保持静默则零状态改变。"""
    uplink = baguwen_v3.collect_commander_uplink_baguwen(world, commander_id)
    if llm_callback is None:
        return {
            "commander_id": commander_id,
            "decision": "保持静默",
            "issued": None,
            "uplink_text": uplink["text"],
            "omitted_disconnected_ids": uplink["omitted_disconnected_ids"],
        }

    resp = llm_callback({
        "commander_id": commander_id,
        "echelon": uplink.get("echelon"),
        "uplink_text": uplink["text"],
        "reports": uplink["reports"],
    })
    if not resp or (isinstance(resp, str) and resp.strip() in ("", "保持静默", "SILENCE")):
        return {
            "commander_id": commander_id,
            "decision": "保持静默",
            "issued": None,
            "uplink_text": uplink["text"],
            "omitted_disconnected_ids": uplink["omitted_disconnected_ids"],
        }
    if isinstance(resp, dict) and resp.get("silence"):
        return {
            "commander_id": commander_id,
            "decision": "保持静默",
            "issued": None,
            "uplink_text": uplink["text"],
            "omitted_disconnected_ids": uplink["omitted_disconnected_ids"],
        }

    issued = None
    if auto_issue and isinstance(resp, dict):
        issued = baguwen_v3.issue_downlink_baguwen_order(world, commander_id, resp, author_commander_id=commander_id)
    return {
        "commander_id": commander_id,
        "decision": "order",
        "order": resp if isinstance(resp, dict) else {"raw_text": str(resp)},
        "issued": issued,
        "uplink_text": uplink["text"],
        "omitted_disconnected_ids": uplink["omitted_disconnected_ids"],
    }
