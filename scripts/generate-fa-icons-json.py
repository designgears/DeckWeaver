#!/usr/bin/env python3
"""Generate Font Awesome icon manifest for the OpenDeck property inspector."""

from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = (
    ROOT
    / "opendeck"
    / "com.designgears.deckweaver.sdPlugin"
    / "propertyInspector"
    / "fontawesome-icons.json"
)

# The crate emits raw strings whose hash count has changed between releases
# (r#"..."# in 7.2, r##"..."## in 7.3), so match the opening hashes and
# backreference them for the terminator.
ICON_BLOCK = re.compile(
    r'svg: r(?P<hashes>#+)"(?P<svg>.*?)"(?P=hashes),\s*'
    r'slug: "(?P<slug>[^"]+)",.*?'
    r'family: "(?P<family>[^"]+)",.*?'
    r'label: "(?P<label>[^"]+)"',
    re.DOTALL,
)


def fa_crate_root() -> Path:
    lock = (ROOT / "Cargo.lock").read_text(encoding="utf-8")
    match = re.search(r'name = "fontawesome-free-pack"\nversion = "([^"]+)"', lock)
    version = match.group(1) if match else "7.2.0"
    registry = Path.home() / ".cargo/registry/src"
    matches = sorted(registry.glob(f"*/fontawesome-free-pack-{version}"))
    if not matches:
        raise SystemExit(
            f"fontawesome-free-pack {version} not found; run cargo build -p deckweaver-opendeck first"
        )
    return matches[-1]


def collect_icon_data(crate_root: Path) -> dict[str, dict[str, str]]:
    icons: dict[str, dict[str, str]] = {}
    icons_dir = crate_root / "src/icons"
    # regular.rs sits at the top level while brands/ and solid/ are split into
    # part_*.rs subdirectories, so recurse rather than globbing a fixed depth.
    for part in sorted(icons_dir.rglob("*.rs")):
        text = part.read_text(encoding="utf-8")
        for block in ICON_BLOCK.finditer(text):
            family = block.group("family")
            slug_suffix = block.group("slug")
            full_slug = f"{family}/{slug_suffix}"
            icons[full_slug] = {
                "slug": full_slug,
                "label": block.group("label"),
                "svg": block.group("svg"),
            }
    return icons


def collect_slugs(crate_root: Path) -> set[str]:
    slugs: set[str] = set()
    finder_dir = crate_root / "src/finder"
    slug_pattern = re.compile(r'"(solid|regular|brands)/([^"]+)"')
    for part in sorted(finder_dir.glob("part_*.rs")):
        text = part.read_text(encoding="utf-8")
        for match in slug_pattern.finditer(text):
            slugs.add(f"{match.group(1)}/{match.group(2)}")
    return slugs


def collect_icons(crate_root: Path) -> list[dict[str, str]]:
    icon_data = collect_icon_data(crate_root)
    icons: list[dict[str, str]] = []
    for slug in sorted(collect_slugs(crate_root)):
        if slug in icon_data:
            icons.append(icon_data[slug])
            continue
        family, name = slug.split("/", 1)
        icons.append(
            {
                "slug": slug,
                "label": name.replace("-", " ").title(),
                "svg": "",
            }
        )
    icons.sort(key=lambda item: item["label"].lower())
    return icons


def main() -> int:
    crate_root = fa_crate_root()
    icons = collect_icons(crate_root)
    if not icons:
        raise SystemExit("No Font Awesome icons found")

    # An SVG-less manifest still renders a dropdown, just one with no icons in
    # it, so fail loudly instead of shipping a silently broken picker.
    missing = [icon["slug"] for icon in icons if not icon["svg"]]
    if len(missing) > len(icons) // 10:
        raise SystemExit(
            f"{len(missing)}/{len(icons)} icons have no SVG "
            f"(e.g. {', '.join(missing[:5])}); the crate layout or the "
            f"Icon literal format in {crate_root} likely changed"
        )

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(icons, separators=(",", ":")), encoding="utf-8")
    print(f"Wrote {len(icons)} icons to {OUT} ({len(missing)} without SVG)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
