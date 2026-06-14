#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
INSTALLER_TARGETS = {
    "x64": "x86_64-pc-windows-msvc",
    "arm64": "aarch64-pc-windows-msvc",
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest().upper()


def split_line_ending(line: str) -> tuple[str, str]:
    if line.endswith("\r\n"):
        return line[:-2], "\r\n"
    if line.endswith("\n"):
        return line[:-1], "\n"
    return line, ""


def read_lines(path: Path) -> list[str]:
    return path.read_text(encoding="utf-8").splitlines(keepends=True)


def write_lines(path: Path, lines: list[str]) -> None:
    path.write_text("".join(lines), encoding="utf-8", newline="")


def rewrite_scalar(path: Path, key: str, value: str) -> None:
    lines = read_lines(path)
    pattern = re.compile(rf"^(\s*{re.escape(key)}:\s*).*$")
    updated = False

    for index, line in enumerate(lines):
        body, ending = split_line_ending(line)
        match = pattern.match(body)
        if match:
            lines[index] = f"{match.group(1)}{value}{ending}"
            updated = True

    if not updated:
        raise SystemExit(f"{path.relative_to(ROOT)} is missing {key}")

    write_lines(path, lines)


def read_scalar(path: Path, key: str) -> str:
    pattern = re.compile(rf"^\s*{re.escape(key)}:\s*(.+?)\s*$")
    for line in read_lines(path):
        body, _ = split_line_ending(line)
        match = pattern.match(body)
        if match:
            return match.group(1)

    raise SystemExit(f"{path.relative_to(ROOT)} is missing {key}")


def infer_release_repo(locale_manifest: Path, installer_manifest: Path) -> str:
    candidates = [
        read_scalar(locale_manifest, "ReleaseNotesUrl"),
        installer_manifest.read_text(encoding="utf-8"),
    ]

    for candidate in candidates:
        match = re.search(r"https://github\.com/([^/\s]+/[^/\s]+)/releases", candidate)
        if match:
            return match.group(1)

    raise SystemExit("Could not infer release repository from existing WinGet manifests")


def asset_name(asset_prefix: str, version: str, target: str) -> str:
    return f"{asset_prefix}-{version}-{target}.exe"


def installer_metadata(
    assets_dir: Path,
    release_repo: str,
    tag: str,
    version: str,
    asset_prefix: str,
) -> dict[str, dict[str, str]]:
    metadata = {}
    for architecture, target in INSTALLER_TARGETS.items():
        name = asset_name(asset_prefix, version, target)
        asset = assets_dir / name
        if not asset.is_file():
            raise SystemExit(f"Missing WinGet release asset: {asset}")

        metadata[architecture] = {
            "url": f"https://github.com/{release_repo}/releases/download/{tag}/{name}",
            "sha256": sha256(asset),
        }
    return metadata


def parse_architecture(line: str) -> str | None:
    body, _ = split_line_ending(line)
    match = re.match(r"^\s*(?:-\s*)?Architecture:\s*(\S+)\s*$", body)
    if match:
        return match.group(1)
    return None


def rewrite_installer_manifest(path: Path, version: str, metadata: dict[str, dict[str, str]]) -> None:
    rewrite_scalar(path, "PackageVersion", version)
    lines = read_lines(path)
    current_architecture = None
    seen = {(architecture, field): False for architecture in metadata for field in ("url", "sha256")}

    for index, line in enumerate(lines):
        architecture = parse_architecture(line)
        if architecture is not None:
            current_architecture = architecture
            continue

        if current_architecture not in metadata:
            continue

        body, ending = split_line_ending(line)
        url_match = re.match(r"^(\s*InstallerUrl:\s*).*$", body)
        if url_match:
            lines[index] = f"{url_match.group(1)}{metadata[current_architecture]['url']}{ending}"
            seen[(current_architecture, "url")] = True
            continue

        sha_match = re.match(r"^(\s*InstallerSha256:\s*).*$", body)
        if sha_match:
            lines[index] = f"{sha_match.group(1)}{metadata[current_architecture]['sha256']}{ending}"
            seen[(current_architecture, "sha256")] = True

    missing = [f"{architecture}:{field}" for (architecture, field), found in seen.items() if not found]
    if missing:
        raise SystemExit(f"{path.relative_to(ROOT)} is missing installer fields: {', '.join(missing)}")

    write_lines(path, lines)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Update repository WinGet manifests for a release.")
    parser.add_argument("--assets-dir", required=True, type=Path, help="Directory containing release assets")
    parser.add_argument("--repo", help="GitHub release repository, e.g. owner/repo")
    parser.add_argument("--tag", required=True, help="Release tag, e.g. v3.2.4")
    parser.add_argument("--version", required=True, help="Release version, e.g. 3.2.4")
    parser.add_argument("--asset-prefix", default="skillmux", help="Release asset filename prefix")
    parser.add_argument("--manifest-dir", default=ROOT / "packaging" / "winget", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    assets_dir = args.assets_dir if args.assets_dir.is_absolute() else ROOT / args.assets_dir
    manifest_dir = args.manifest_dir if args.manifest_dir.is_absolute() else ROOT / args.manifest_dir

    version_manifest = manifest_dir / "ZackZhu.SkillMux.yaml"
    locale_manifest = manifest_dir / "ZackZhu.SkillMux.locale.en-US.yaml"
    installer_manifest = manifest_dir / "ZackZhu.SkillMux.installer.yaml"
    release_repo = args.repo or infer_release_repo(locale_manifest, installer_manifest)

    metadata = installer_metadata(
        assets_dir=assets_dir,
        release_repo=release_repo,
        tag=args.tag,
        version=args.version,
        asset_prefix=args.asset_prefix,
    )

    rewrite_scalar(version_manifest, "PackageVersion", args.version)
    rewrite_scalar(locale_manifest, "PackageVersion", args.version)
    rewrite_scalar(locale_manifest, "ReleaseNotesUrl", f"https://github.com/{release_repo}/releases/tag/{args.tag}")
    rewrite_installer_manifest(installer_manifest, args.version, metadata)

    for path in (version_manifest, locale_manifest, installer_manifest):
        print(path.relative_to(ROOT))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
