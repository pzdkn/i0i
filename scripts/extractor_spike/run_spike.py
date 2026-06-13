#!/usr/bin/env python3
"""Run the RFC 0025 extractor evaluation spike.

The runner captures raw outputs, writes an i0i-shaped normalized envelope, and
generates a preview/report worksheet. It does not implement a production
extractor adapter.
"""

from __future__ import annotations

import argparse
import html
import json
import os
import shlex
import shutil
import subprocess
import time
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent
VENVS_DIR = ROOT / ".venvs"
MANIFEST = ROOT / "corpus" / "manifest.toml"
PDF_DIR = ROOT / "corpus" / "pdfs"
OUTPUTS_DIR = ROOT / "outputs"
NORMALIZED_DIR = ROOT / "normalized"
PREVIEWS_DIR = ROOT / "previews"
REPORT = ROOT / "report.md"

SCORE_CRITERIA = [
    "reading_order",
    "layout_preservation",
    "text_spans_geometry",
    "figure_detection",
    "figure_export_or_crop",
    "caption_linkage",
    "table_quality",
    "equation_handling",
    "normalization_cost",
    "macos_packaging",
    "speed_memory",
    "license_redistribution",
]


@dataclass(frozen=True)
class Paper:
    id: str
    title: str
    url: str
    features: list[str]


@dataclass(frozen=True)
class Tool:
    id: str
    label: str
    env_var: str
    default_command: str | None


TOOLS = [
    Tool(
        "docling",
        "Docling",
        "I0I_DOCLING_CMD",
        f"{VENVS_DIR / 'docling' / 'bin' / 'docling'} {{pdf}} --to json --image-export-mode referenced --device cpu --output {{raw_dir}}",
    ),
    Tool(
        "marker",
        "Marker",
        "I0I_MARKER_CMD",
        f"{VENVS_DIR / 'marker' / 'bin' / 'marker_single'} {{pdf}} --output_dir {{raw_dir}} --output_format json",
    ),
    Tool(
        "mineru",
        "MinerU",
        "I0I_MINERU_CMD",
        f"{VENVS_DIR / 'mineru' / 'bin' / 'mineru'} -p {{pdf}} -o {{raw_dir}} -b pipeline",
    ),
    Tool("pdfium_basic", "Pdfium Basic", "I0I_PDFIUM_BASIC_CMD", None),
]


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


def command_for_tool(tool: Tool) -> str | None:
    return os.environ.get(tool.env_var) or tool.default_command


def command_is_available(command: str) -> bool:
    first = shlex.split(command)[0]
    return Path(first).exists() or shutil.which(first) is not None


def run_tool(
    tool: Tool,
    paper: Paper,
    pdf_path: Path,
    dry_run: bool,
    timeout_seconds: int,
) -> dict[str, Any]:
    raw_dir = OUTPUTS_DIR / tool.id / "raw" / paper.id
    if raw_dir.exists() and not dry_run:
        shutil.rmtree(raw_dir)
    raw_dir.mkdir(parents=True, exist_ok=True)
    command_template = command_for_tool(tool)
    started_at = time.time()

    if command_template is None:
        return result(
            tool,
            paper,
            "skipped",
            started_at,
            raw_dir,
            message=f"Set {tool.env_var} to enable this adapter.",
        )

    command = command_template.format(
        pdf=shlex.quote(str(pdf_path)),
        raw_dir=shlex.quote(str(raw_dir)),
        paper_id=shlex.quote(paper.id),
    )

    if not command_is_available(command):
        return result(
            tool,
            paper,
            "missing",
            started_at,
            raw_dir,
            command=command,
            message=f"Command not found: {shlex.split(command)[0]}",
        )

    if dry_run:
        return result(
            tool,
            paper,
            "dry_run",
            started_at,
            raw_dir,
            command=command,
            message="Command available; dry run requested.",
        )

    try:
        completed = subprocess.run(
            command,
            shell=True,
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        (raw_dir / "stdout.txt").write_text((exc.stdout or b"").decode(errors="replace"))
        (raw_dir / "stderr.txt").write_text((exc.stderr or b"").decode(errors="replace"))
        return result(
            tool,
            paper,
            "timeout",
            started_at,
            raw_dir,
            command=command,
            message=f"timed out after {timeout_seconds}s",
        )

    (raw_dir / "stdout.txt").write_text(completed.stdout)
    (raw_dir / "stderr.txt").write_text(completed.stderr)

    payload_files = [
        path
        for path in raw_dir.rglob("*")
        if path.is_file() and path.name not in {"stdout.txt", "stderr.txt"}
    ]
    status = "ok" if completed.returncode == 0 else "failed"
    if completed.returncode == 0 and not payload_files:
        status = "no_output"
    return result(
        tool,
        paper,
        status,
        started_at,
        raw_dir,
        command=command,
        message=f"exit code {completed.returncode}",
    )


def result(
    tool: Tool,
    paper: Paper,
    status: str,
    started_at: float,
    raw_dir: Path,
    command: str | None = None,
    message: str | None = None,
) -> dict[str, Any]:
    raw_files = sorted(
        str(path.relative_to(ROOT))
        for path in raw_dir.rglob("*")
        if path.is_file()
    )
    return {
        "paperId": paper.id,
        "extractor": tool.id,
        "status": status,
        "command": command,
        "message": message,
        "durationMs": round((time.time() - started_at) * 1000),
        "rawOutputDir": str(raw_dir.relative_to(ROOT)),
        "rawFiles": raw_files,
    }


def write_normalized_envelope(paper: Paper, tool: Tool, run: dict[str, Any]) -> Path:
    target_dir = NORMALIZED_DIR / tool.id
    target_dir.mkdir(parents=True, exist_ok=True)
    target = target_dir / f"{paper.id}.json"
    document = {
        "schemaVersion": "i0i.extracted_document.v0",
        "paperId": paper.id,
        "title": paper.title,
        "extractor": tool.id,
        "status": run["status"],
        "sourcePdf": str((PDF_DIR / f"{paper.id}.pdf").relative_to(ROOT)),
        "annotationSourceId": None,
        "sourceText": "",
        "pages": [],
        "blocks": [],
        "spans": [],
        "assets": [],
        "rawOutputDir": run["rawOutputDir"],
        "notes": [
            "This is a spike envelope. Fill pages/blocks/spans/assets when the tool-specific normalizer is added.",
        ],
    }
    target.write_text(json.dumps(document, indent=2) + "\n")
    return target


def write_run_summary(runs: list[dict[str, Any]]) -> None:
    target = OUTPUTS_DIR / "run-summary.json"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(runs, indent=2) + "\n")


def write_preview(papers: list[Paper], runs: list[dict[str, Any]]) -> None:
    PREVIEWS_DIR.mkdir(parents=True, exist_ok=True)
    rows = []
    by_key = {(run["paperId"], run["extractor"]): run for run in runs}
    for paper in papers:
        pdf_path = PDF_DIR / f"{paper.id}.pdf"
        pdf_link = f"../corpus/pdfs/{paper.id}.pdf" if pdf_path.exists() else ""
        status_cells = []
        for tool in TOOLS:
            run = by_key.get((paper.id, tool.id), {})
            normalized = f"../normalized/{tool.id}/{paper.id}.json"
            status_cells.append(
                "<td>"
                f"<strong>{html.escape(tool.label)}</strong><br>"
                f"{html.escape(run.get('status', 'not_run'))}<br>"
                f"<a href=\"{html.escape(normalized)}\">normalized</a>"
                "</td>"
            )
        pdf_embed = (
            f"<object data=\"{html.escape(pdf_link)}\" type=\"application/pdf\"></object>"
            if pdf_link
            else "<p class=\"missing\">PDF not fetched yet.</p>"
        )
        rows.append(
            "<section>"
            f"<h2>{html.escape(paper.title)}</h2>"
            f"<p><code>{html.escape(paper.id)}</code> · {html.escape(', '.join(paper.features))}</p>"
            f"{pdf_embed}"
            "<table><tr>"
            + "".join(status_cells)
            + "</tr></table>"
            "</section>"
        )

    html_text = """<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>i0i Extractor Spike Preview</title>
  <style>
    body { margin: 24px; font: 14px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; color: #202124; background: #f7f7f4; }
    h1, h2 { margin: 0 0 8px; }
    section { margin: 0 0 32px; padding: 18px; background: #fff; border: 1px solid #d7d7d0; border-radius: 8px; }
    object { width: 100%; height: 560px; border: 1px solid #d7d7d0; border-radius: 6px; background: #fafafa; }
    table { width: 100%; margin-top: 14px; border-collapse: collapse; table-layout: fixed; }
    td { padding: 10px; border: 1px solid #d7d7d0; vertical-align: top; background: #fbfbf8; }
    code { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
    .missing { padding: 24px; border: 1px dashed #b9b9b0; border-radius: 6px; }
  </style>
</head>
<body>
  <h1>i0i Extractor Spike Preview</h1>
  <p>Use this page to compare source PDFs with raw/normalized extractor output status.</p>
""" + "\n".join(rows) + """
</body>
</html>
"""
    (PREVIEWS_DIR / "index.html").write_text(html_text)


def write_report(papers: list[Paper], runs: list[dict[str, Any]]) -> None:
    lines = [
        "# Extractor Spike Report",
        "",
        "Status: In progress",
        "",
        "## Decision",
        "",
        "- Outcome: TBD",
        "- Recommended first production adapter: TBD",
        "- Rationale: TBD",
        "",
        "## Corpus",
        "",
    ]
    for paper in papers:
        lines.append(f"- `{paper.id}`: {paper.title} ({', '.join(paper.features)})")

    lines.extend(["", "## Run Summary", ""])
    for run in runs:
        lines.append(
            f"- `{run['paperId']}` / `{run['extractor']}`: {run['status']} ({run.get('message') or 'no message'})"
        )

    lines.extend(["", "## Scorecard", ""])
    header = "| Extractor | " + " | ".join(SCORE_CRITERIA) + " | Notes |"
    divider = "|---" * (len(SCORE_CRITERIA) + 2) + "|"
    lines.extend([header, divider])
    for tool in TOOLS:
        lines.append(f"| {tool.label} | " + " | ".join(["TBD"] * len(SCORE_CRITERIA)) + " | TBD |")

    lines.extend(
        [
            "",
            "## Minimum Gates",
            "",
            "- runs locally on macOS in development: TBD",
            "- produces page-level geometry: TBD",
            "- produces block-level reading order: TBD",
            "- produces enough spans or geometry for highlights: TBD",
            "- preserves or exports figures/page-region assets: TBD",
            "- has acceptable license/redistribution constraints: TBD",
            "",
            "## Preview",
            "",
            "Open `scripts/extractor_spike/previews/index.html` to inspect source PDFs and extraction status side by side.",
            "",
        ]
    )
    REPORT.write_text("\n".join(lines))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--timeout-seconds", type=int, default=900)
    parser.add_argument(
        "--tool",
        action="append",
        choices=[tool.id for tool in TOOLS],
        help="Run only this extractor. Can be passed multiple times.",
    )
    parser.add_argument(
        "--paper",
        action="append",
        help="Run only this paper id. Can be passed multiple times.",
    )
    args = parser.parse_args()

    selected_tools = [tool for tool in TOOLS if args.tool is None or tool.id in args.tool]
    papers = [
        paper
        for paper in read_manifest(MANIFEST)
        if args.paper is None or paper.id in args.paper
    ]
    runs: list[dict[str, Any]] = []

    for paper in papers:
        pdf_path = PDF_DIR / f"{paper.id}.pdf"
        if not pdf_path.exists():
            for tool in selected_tools:
                run = {
                    "paperId": paper.id,
                    "extractor": tool.id,
                    "status": "missing_pdf",
                    "command": None,
                    "message": "Run pnpm spike:extractors:fetch first.",
                    "durationMs": 0,
                    "rawOutputDir": str((OUTPUTS_DIR / tool.id / "raw" / paper.id).relative_to(ROOT)),
                    "rawFiles": [],
                }
                runs.append(run)
                write_normalized_envelope(paper, tool, run)
            continue

        for tool in selected_tools:
            run = run_tool(tool, paper, pdf_path, args.dry_run, args.timeout_seconds)
            runs.append(run)
            write_normalized_envelope(paper, tool, run)

    write_run_summary(runs)
    write_preview(papers, runs)
    write_report(papers, runs)
    print(f"wrote {REPORT.relative_to(Path.cwd())}")
    print(f"wrote {(PREVIEWS_DIR / 'index.html').relative_to(Path.cwd())}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
