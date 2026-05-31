#!/usr/bin/env python3
"""Fetch the fixed RFC 0025 extractor-spike corpus."""

from __future__ import annotations

import argparse
import sys
import tomllib
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parent
MANIFEST = ROOT / "corpus" / "manifest.toml"
PDF_DIR = ROOT / "corpus" / "pdfs"
MAX_BYTES = 100 * 1024 * 1024


@dataclass(frozen=True)
class Paper:
    id: str
    title: str
    url: str
    features: list[str]


def read_manifest(path: Path) -> list[Paper]:
    data = tomllib.loads(path.read_text())
    return [
        Paper(
            id=item["id"],
            title=item["title"],
            url=item["url"],
            features=list(item.get("features", [])),
        )
        for item in data.get("paper", [])
    ]


def fetch_pdf(paper: Paper, overwrite: bool) -> Path:
    PDF_DIR.mkdir(parents=True, exist_ok=True)
    target = PDF_DIR / f"{paper.id}.pdf"
    partial = target.with_suffix(".pdf.part")

    if target.exists() and not overwrite:
        print(f"skip {paper.id}: already fetched")
        return target

    print(f"fetch {paper.id}: {paper.url}")
    request = urllib.request.Request(
        paper.url,
        headers={"User-Agent": "i0i-extractor-spike/0.1"},
    )

    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            status = getattr(response, "status", 200)
            if status < 200 or status >= 300:
                raise RuntimeError(f"HTTP {status}")

            written = 0
            with partial.open("wb") as output:
                while True:
                    chunk = response.read(1024 * 128)
                    if not chunk:
                        break
                    written += len(chunk)
                    if written > MAX_BYTES:
                        raise RuntimeError(f"PDF exceeds {MAX_BYTES} bytes")
                    output.write(chunk)
    except (urllib.error.URLError, TimeoutError, RuntimeError) as exc:
        partial.unlink(missing_ok=True)
        raise RuntimeError(f"failed to fetch {paper.id}: {exc}") from exc

    if partial.read_bytes()[:5] != b"%PDF-":
        partial.unlink(missing_ok=True)
        raise RuntimeError(f"{paper.id} did not download as a PDF")

    partial.replace(target)
    return target


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--overwrite", action="store_true")
    args = parser.parse_args()

    failures = 0
    for paper in read_manifest(MANIFEST):
        try:
            fetch_pdf(paper, overwrite=args.overwrite)
        except RuntimeError as exc:
            failures += 1
            print(exc, file=sys.stderr)

    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
