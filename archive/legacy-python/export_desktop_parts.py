#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Export full Gentactix source code into 3 cleanly partitioned text files on Desktop without truncation.
Part 1: Rust Core & Server crates + Cargo configurations.
Part 2: Web frontend (index.html, JS, CSS) + Docs & Specifications.
Part 3: Python modules (gentactix/ package + root runners & test tools).
"""

import os
from pathlib import Path

ROOT = Path(r"D:\tools\gentactix")
DESKTOP = Path(r"C:\Users\admin\Desktop")

PART1_OUT = DESKTOP / "gentactix_source_part1_rust.txt"
PART2_OUT = DESKTOP / "gentactix_source_part2_web_data_docs.txt"
PART3_OUT = DESKTOP / "gentactix_source_part3_python.txt"

EXCLUDE_DIRS = {
    ".git", ".venv", "venv", "__pycache__", "target", "node_modules",
    ".idea", ".vscode", "logs", "dist", "build", ".pytest_cache"
}

EXCLUDE_EXTS = {
    ".pyc", ".pyd", ".exe", ".dll", ".so", ".dylib", ".r16", ".pdb", ".bin"
}

def is_excluded(path: Path) -> bool:
    for part in path.parts:
        if part in EXCLUDE_DIRS:
            return True
    if path.suffix.lower() in EXCLUDE_EXTS:
        return True
    return False

def format_file_dump(path: Path, rel_path: str) -> str:
    try:
        content = path.read_text(encoding="utf-8", errors="replace")
    except Exception as e:
        content = f"<ERROR READING FILE: {e}>\n"
    
    header = f"\n{'='*80}\nFILE: {rel_path} ({len(content.splitlines())} lines, {len(content)} chars)\n{'='*80}\n"
    return header + content + "\n"

def main():
    print("Collecting Gentactix source files...")
    all_files = []
    for p in sorted(ROOT.rglob("*")):
        if p.is_file() and not is_excluded(p):
            # Skip massive data files like items_catalog.json (23k lines, 1.2MB) from simple txt dumps if needed, or include schema
            rel = p.relative_to(ROOT).as_posix()
            all_files.append((p, rel))

    part1_files = []
    part2_files = []
    part3_files = []

    for p, rel in all_files:
        low = rel.lower()
        if low.endswith(".rs") or low.endswith("cargo.toml") or low.endswith("cargo.lock") or "crates/" in low:
            part1_files.append((p, rel))
        elif low.startswith("web/") or low.endswith(".html") or low.endswith(".css") or low.endswith(".js") or low.endswith(".md") or (low.startswith("docs/") or low.startswith("specs/")):
            part2_files.append((p, rel))
        elif low.endswith(".py") or low.startswith("gentactix/") or low.endswith(".sh") or low.endswith(".json") and not "items_catalog.json" in low:
            part3_files.append((p, rel))
        else:
            # Fallback
            if p.suffix in [".toml", ".yaml", ".yml", ".txt"]:
                part2_files.append((p, rel))
            else:
                part3_files.append((p, rel))

    print(f"Part 1 (Rust): {len(part1_files)} files")
    print(f"Part 2 (Web/Docs): {len(part2_files)} files")
    print(f"Part 3 (Python): {len(part3_files)} files")

    # Write Part 1
    with open(PART1_OUT, "w", encoding="utf-8") as f:
        f.write("# GENTACTIX SOURCE CODE - PART 1: RUST CORE & SERVER CRATES\n")
        f.write(f"# Total Files: {len(part1_files)}\n\n")
        for p, rel in part1_files:
            f.write(format_file_dump(p, rel))
    with open(DESKTOP / "gentactix_source_part1.txt", "w", encoding="utf-8") as f:
        f.write(PART1_OUT.read_text(encoding="utf-8"))
    print(f"Written Part 1 -> {PART1_OUT} ({PART1_OUT.stat().st_size} bytes)")

    # Write Part 2
    with open(PART2_OUT, "w", encoding="utf-8") as f:
        f.write("# GENTACTIX SOURCE CODE - PART 2: WEB FRONTEND, UI & DOCS\n")
        f.write(f"# Total Files: {len(part2_files)}\n\n")
        for p, rel in part2_files:
            f.write(format_file_dump(p, rel))
    with open(DESKTOP / "gentactix_source_part2.txt", "w", encoding="utf-8") as f:
        f.write(PART2_OUT.read_text(encoding="utf-8"))
    print(f"Written Part 2 -> {PART2_OUT} ({PART2_OUT.stat().st_size} bytes)")

    # Write Part 3
    with open(PART3_OUT, "w", encoding="utf-8") as f:
        f.write("# GENTACTIX SOURCE CODE - PART 3: PYTHON V3.2 MODULES & RUNNERS\n")
        f.write(f"# Total Files: {len(part3_files)}\n\n")
        for p, rel in part3_files:
            f.write(format_file_dump(p, rel))
    with open(DESKTOP / "gentactix_source_part3.txt", "w", encoding="utf-8") as f:
        f.write(PART3_OUT.read_text(encoding="utf-8"))
    print(f"Written Part 3 -> {PART3_OUT} ({PART3_OUT.stat().st_size} bytes)")

if __name__ == "__main__":
    main()
