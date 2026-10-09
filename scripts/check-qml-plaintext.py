#!/usr/bin/env python3
"""Fails when a QML Text, Label or TextEdit does not say how its text is read.

Names, paths, URLs, file content, server answers and other programs' text are
untrusted; a Text with the default format (AutoText) shows `<b>x</b>` or
`<img src="https://...">` as markup. Every item of these types in Files' QML
sets `textFormat` (`Text.PlainText`; `TextEdit.PlainText` for an editor).

    python3 -I scripts/check-qml-plaintext.py apps/telamon-explorer/qml

Types checked: Text, Label, QQC2.Label, Controls.Label, TextEdit, TextArea.
Telamon.Ui's own components (TelamonLabel, TelamonDialog...) are plain text
unless told otherwise; the framework checks that itself.
"""
import re
import sys
from pathlib import Path

TYPES = r"(?:Text|Label|QQC2\.Label|Controls\.Label|TextEdit|TextArea|T\.Label)"
START = re.compile(r"^(\s*)(?:component\s+\w+\s*:\s*|[\w.]+\s*:\s*)?" + TYPES + r"\s*\{", re.M)


def block_end(src: str, open_at: int) -> int:
    """Index of the brace that closes the one at `open_at`, skipping strings,
    comments and character literals."""
    depth = 0
    i = open_at
    n = len(src)
    while i < n:
        c = src[i]
        if c == "/" and src[i + 1 : i + 2] == "/":
            i = src.find("\n", i)
            if i < 0:
                return n
            continue
        if c == "/" and src[i + 1 : i + 2] == "*":
            i = src.find("*/", i) + 2
            continue
        if c in "\"'`":
            q = c
            i += 1
            while i < n and src[i] != q:
                i += 2 if src[i] == "\\" else 1
            i += 1
            continue
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return n


def main(dirs: list[str]) -> int:
    bad = 0
    checked = 0
    for d in dirs:
        for path in sorted(Path(d).rglob("*.qml")):
            src = path.read_text(encoding="utf-8")
            for m in START.finditer(src):
                open_at = src.index("{", m.start())
                body = src[open_at : block_end(src, open_at) + 1]
                # (a Text holds no other Text; any inside is checked on its own)
                own = body
                checked += 1
                if "textFormat" not in own:
                    line = src.count("\n", 0, m.start()) + 1
                    print(f"{path}:{line}: text item without textFormat")
                    bad += 1
    print(f"{checked} text items checked, {bad} without textFormat")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:] or ["apps"]))
