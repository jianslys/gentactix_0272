"""GenTactix V2 headless CLI runner for conversation-driven wargaming."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Optional

from gentactix import command_v2, terrain_field as tf, toe
from gentactix.adjudicator_v2 import BASE_SPEED_KMH, World, run_turn
from gentactix.scenario_v2 import build_world, scenario_demo
from gentactix.viz_v2 import build_map_v2


def _configure_stdio() -> None:
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")


def _load_json_arg(
    file_path: Optional[str],
    inline_json: Optional[str],
    use_stdin: bool,
    default: Optional[dict[str, Any]] = None,
) -> dict[str, Any]:
    raw: Optional[str] = None
    if file_path:
        raw = Path(file_path).read_text(encoding="utf-8-sig")
    elif inline_json is not None:
        raw = inline_json.lstrip("\ufeff")
    elif use_stdin:
        raw = sys.stdin.read().lstrip("\ufeff")

    if raw is None or not raw.strip():
        if default is not None:
            return default
        return {}
    data = json.loads(raw)
    if not isinstance(data, dict):
        raise ValueError("JSON payload must be an object")
    return data


def _render_views(
    world: World,
    session_dir: Path,
    view: str = "all",
    show_slope: bool = False,
    report: Optional[dict[str, Any]] = None,
) -> dict[str, str]:
    targets = ["truth", "red", "blue"] if view == "all" else [view]
    outputs: dict[str, str] = {}
    for target in targets:
        view_team = None if target == "truth" else target
        fmap = build_map_v2(
            world,
            view_team=view_team,
            show_slope=show_slope,
            report=report,
        )
        out_path = session_dir / f"map_{target}.html"
        fmap.save(str(out_path))
        outputs[target] = str(out_path)
    return outputs


def _cmd_demo_spec(args: argparse.Namespace) -> int:
    spec = scenario_demo()
    text = json.dumps(spec, ensure_ascii=False, indent=2)
    if args.out:
        Path(args.out).write_text(text + "\n", encoding="utf-8")
        print(json.dumps({"out": args.out}, ensure_ascii=False))
    else:
        print(text)
    return 0


def _cmd_init(args: argparse.Namespace) -> int:
    session_dir = Path(args.session)
    session_dir.mkdir(parents=True, exist_ok=True)
    spec = _load_json_arg(
        args.spec_file,
        args.spec_json,
        args.stdin,
        default=scenario_demo(),
    )
    world = build_world(spec)
    world.save(str(session_dir))
    maps = {} if args.no_render else _render_views(world, session_dir, view=args.view)
    summary = {
        "session": str(session_dir),
        "turn": world.turn,
        "hours": world.hours,
        "night": world.night,
        "grid_shape": [int(world.grid.shape[0]), int(world.grid.shape[1])],
        "pieces": len(world.pieces),
        "depots": len(world.depots),
        "batteries": len(world.batteries),
        "wings": len(world.wings),
        "maps": maps,
    }
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    return 0


def _cmd_turn(args: argparse.Namespace) -> int:
    session_dir = Path(args.session)
    world = World.load(str(session_dir))
    orders = _load_json_arg(
        args.orders_file,
        args.orders_json,
        args.stdin,
        default={},
    )
    report = run_turn(world, orders)
    world.save(str(session_dir))
    report_path = session_dir / "last_report.json"
    report_path.write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    maps = (
        {}
        if args.no_render
        else _render_views(
            world,
            session_dir,
            view=args.view,
            show_slope=args.slope,
            report=report,
        )
    )
    summary = {
        "session": str(session_dir),
        "turn": report["turn"],
        "hours": report["hours"],
        "night": report["night"],
        "fingerprint": report["fingerprint"],
        "warnings": report["warnings"],
        "panel": report["panel"],
        "report_path": str(report_path),
        "maps": maps,
    }
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    return 0


def _cmd_render(args: argparse.Namespace) -> int:
    session_dir = Path(args.session)
    world = World.load(str(session_dir))
    report_path = session_dir / "last_report.json"
    report = None
    if report_path.exists():
        report = json.loads(report_path.read_text(encoding="utf-8-sig"))
    maps = _render_views(
        world,
        session_dir,
        view=args.view,
        show_slope=args.slope,
        report=report,
    )
    print(json.dumps({"session": str(session_dir), "maps": maps}, ensure_ascii=False, indent=2))
    return 0


def _cmd_preview(args: argparse.Namespace) -> int:
    """只读预览查询接口：不落盘，供前端实时校验路径耗时、AO 配额、通联状态与牵引评估。"""
    session_dir = Path(args.session)
    world = World.load(str(session_dir))
    q = _load_json_arg(args.query_file, args.query_json, args.stdin, default={})
    kind = q.get("kind", "move")
    out: dict[str, Any] = {"kind": kind}

    if kind == "move":
        p = world.pieces.get(q.get("piece"))
        if p is None:
            out.update(ok=False, reason="unknown_piece")
        else:
            tgt = (int(q["target"][0]), int(q["target"][1]))
            mob = p.mobility()
            res = tf.shortest_path(world.grid, p.px, tgt, mob)
            if res is None:
                out.update(ok=False, reachable=False)
            else:
                path, cum = res
                cost_km = float(cum[-1]) if cum else 0.0
                mods = world.event_manager.piece_modifiers(p, world=world)
                cell_m = world.grid.get_cell_modifiers(p.px, mobility=mob)["non_feature_speed_mult"]
                spd = BASE_SPEED_KMH[mob] * mods["speed_mult"] * cell_m
                out.update(ok=True, reachable=True, cost_km=round(cost_km, 3),
                           est_hours=round(cost_km / spd, 3) if spd > 1e-6 else None,
                           forbid_move=mods["forbid_move"], path=[list(rc) for rc in path])
    elif kind == "ao":
        cmd = world.commanders.get(q.get("commander"))
        if cmd is None:
            out.update(ok=False, reason="unknown_commander")
        else:
            geom = dict(q.get("geometry") or {})
            if geom.get("shape", "circle") == "circle" and "center" not in geom:
                geom["center"] = list(cmd.px)
            cells = command_v2.build_ao_cells(geom, grid=world.grid, pieces=world.pieces)
            quota = command_v2.effective_commander_max_area_px(cmd, world.pieces, world.grid)
            out.update(ok=len(cells) <= quota, area_px=len(cells), quota_px=quota,
                       cells=[list(rc) for rc in sorted(cells)])
    elif kind == "link":
        sup = world.commanders.get(q.get("superior"))
        sub = world.commanders.get(q.get("subordinate")) or world.pieces.get(q.get("subordinate"))
        if sup is None or sub is None:
            out.update(ok=False, reason="unknown_entity")
        else:
            ok, reason = command_v2.validate_command_link(sup, sub, world.grid, world.pieces)
            out.update(ok=ok, reason=reason)
    elif kind == "tow":
        carrier = world.pieces.get(q.get("carrier"))
        payload = world.pieces.get(q.get("payload"))
        if carrier is None or payload is None:
            out.update(ok=False, reason="unknown_piece")
        else:
            out.update(ok=True, **toe.tow_profile(carrier, payload))
    elif kind == "fire_arc":
        p = world.pieces.get(q.get("piece"))
        if p is None:
            out.update(ok=False, reason="unknown_piece")
        else:
            weapon = q.get("weapon")
            if not weapon:
                if p.deployed_weapon and p.deployed_weapon in toe.EQUIPMENT:
                    weapon = p.deployed_weapon
                else:
                    best_r = -1.0
                    for sq in p.squads():
                        for m, cnt in sq.equipment.items():
                            if cnt > 0 and m in toe.EQUIPMENT and toe.EQUIPMENT[m].get("fire"):
                                r = float(toe.EQUIPMENT[m].get("range_m", 0.0))
                                if r > best_r:
                                    best_r = r
                                    weapon = m
            spec = toe.EQUIPMENT.get(weapon) if weapon else None
            range_m = float(spec.get("range_m", 0.0)) if spec else 0.0
            if spec is None or range_m <= 0.0 or not spec.get("fire"):
                out.update(ok=False, reason="unknown_or_non_fire_weapon", weapon=weapon)
            else:
                min_range_m = float(toe.weapon_min_range_m(weapon))
                indirect = bool(toe.is_indirect_weapon(weapon))
                mods = world.event_manager.piece_modifiers(p, world=world)
                forbid_fire = bool(mods["forbid_fire"] or (p.deployed_weapon and p.unit_state == "carried"))
                cells = tf.compute_fire_arc(
                    world.grid,
                    p.pos,
                    range_m,
                    min_range_m=min_range_m,
                    indirect=indirect,
                    night=world.night,
                )
                out.update(
                    ok=True,
                    weapon=weapon,
                    range_m=range_m,
                    min_range_m=min_range_m,
                    indirect=indirect,
                    forbid_fire=forbid_fire,
                    reachable_cells=cells,
                )
    else:
        out.update(ok=False, reason=f"unknown_preview_kind:{kind}")

    print(json.dumps(out, ensure_ascii=False, indent=2))
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="GenTactix V2 headless wargaming CLI runner"
    )
    sub = parser.add_subparsers(dest="command", required=True)

    p_demo = sub.add_parser("demo-spec", help="Print or write the built-in V2 demo scenario JSON")
    p_demo.add_argument("--out", help="Optional output file path")
    p_demo.set_defaults(func=_cmd_demo_spec)

    p_init = sub.add_parser("init", help="Initialize a V2 world session")
    p_init.add_argument("--session", required=True, help="Session directory path")
    p_init.add_argument("--spec-file", help="Scenario JSON file path")
    p_init.add_argument("--spec-json", help="Inline scenario JSON string")
    p_init.add_argument("--stdin", action="store_true", help="Read scenario JSON from stdin")
    p_init.add_argument("--view", choices=["all", "truth", "red", "blue"], default="all")
    p_init.add_argument("--no-render", action="store_true", help="Skip HTML map generation")
    p_init.set_defaults(func=_cmd_init)

    p_turn = sub.add_parser("turn", help="Advance a V2 session by one turn")
    p_turn.add_argument("--session", required=True, help="Session directory path")
    p_turn.add_argument("--orders-file", help="Orders JSON file path")
    p_turn.add_argument("--orders-json", help="Inline orders JSON string")
    p_turn.add_argument("--stdin", action="store_true", help="Read orders JSON from stdin")
    p_turn.add_argument("--view", choices=["all", "truth", "red", "blue"], default="all")
    p_turn.add_argument("--slope", action="store_true", help="Include slope heatmap overlay")
    p_turn.add_argument("--no-render", action="store_true", help="Skip HTML map generation")
    p_turn.set_defaults(func=_cmd_turn)

    p_render = sub.add_parser("render", help="Render Folium maps for a V2 session")
    p_render.add_argument("--session", required=True, help="Session directory path")
    p_render.add_argument("--view", choices=["all", "truth", "red", "blue"], default="all")
    p_render.add_argument("--slope", action="store_true", help="Include slope heatmap overlay")
    p_render.set_defaults(func=_cmd_render)

    p_prev = sub.add_parser("preview", help="Read-only preview query (move/ao/link/tow)")
    p_prev.add_argument("--session", required=True, help="Session directory path")
    p_prev.add_argument("--query-file", help="Preview query JSON file path")
    p_prev.add_argument("--query-json", help="Inline preview query JSON string")
    p_prev.add_argument("--stdin", action="store_true", help="Read query JSON from stdin")
    p_prev.set_defaults(func=_cmd_preview)

    return parser


def main(argv: Optional[list[str]] = None) -> int:
    _configure_stdio()
    parser = build_parser()
    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
