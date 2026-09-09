#!/usr/bin/env python3
"""Deterministic app-server fixture exercising real i0i MCP and finalization.

Only the model/runtime is replaced. A scenario beside this script names the
saved HTML paper and whether the correction should succeed, fail, or wait.
"""

import json
import sys
import urllib.request
from pathlib import Path
from typing import Any


def emit(message: dict[str, Any]) -> None:
    """Write one flushed app-server protocol message."""
    print(json.dumps(message), flush=True)


def rpc(endpoint: str, headers: dict[str, str], method: str, params: dict[str, Any]) -> dict[str, Any]:
    """Call the real streamable HTTP MCP endpoint, accepting JSON or SSE."""
    request = urllib.request.Request(endpoint, data=json.dumps({
        "jsonrpc": "2.0", "id": 1, "method": method, "params": params,
    }).encode(), headers=headers)
    with urllib.request.urlopen(request, timeout=5) as response:
        if session := response.headers.get("mcp-session-id"):
            headers["mcp-session-id"] = session
        body = response.read().decode()
    payload = next((line[5:].strip() for line in body.splitlines() if line.startswith("data:") and line[5:].strip()), body)
    message = json.loads(payload)
    if "error" in message:
        raise RuntimeError(message["error"])
    return message["result"]


def outcome(reference: str, valid: bool) -> dict[str, Any]:
    """Return the synthetic regression payload, preserving the Finding/Gap distinction."""
    evidence = [{"passageRef": reference, "relationship": "supports", "explanation": "The original HTML states the tested scope."}]
    return {
        "summary": "Evidence establishes a bounded finding and motivates a gap.",
        "displayItems": [], "paperDispositions": [], "taskOutcomes": [],
        "unansweredQuestions": [], "nextDirection": "Test the unexamined conditions.",
        "stateSynthesis": {"changes": [
            {"operation": "create", "handle": "f", "kind": "finding", "epistemicStatus": "source_supported",
             "statement": "The study tested two benchmarks.", "evidence": evidence, "relations": [], "reason": "New evidence"},
            {"operation": "create", "handle": "g", "kind": "gap", "epistemicStatus": "agent_synthesis",
             "statement": "Generalization beyond these benchmarks remains untested in this corpus.",
             "evidence": [] if valid else evidence, "relations": [{"target": "f", "kind": "derived_from"}], "reason": "Bounded inference"},
        ], "unresolvedEntryIds": ["g"], "nextDirectionEntryIds": ["g"], "noChangeReason": None},
    }


def main() -> None:
    """Serve the minimum supported app-server methods used by the controller."""
    if sys.argv[1:2] == ["mcp"]:
        print("[]")
        return
    scenario = json.loads(Path(__file__).with_name("scenario.json").read_text())
    threads: dict[str, dict[str, Any]] = {}
    reference = ""
    for line in sys.stdin:
        request = json.loads(line)
        method = request.get("method")
        params = request.get("params", {})
        if "id" not in request:
            continue
        result: dict[str, Any] = {}
        if method == "thread/start":
            assert "url" in params["config"]["mcp_servers"]["ioi"], "Disabled HTTP servers still require a transport"
            thread = f"thread-{len(threads)}"
            threads[thread] = params["config"]
            result = {"thread": {"id": thread}}
        elif method == "mcpServerStatus/list":
            enabled = threads[params["threadId"]]["mcp_servers"]["ioi"]["enabled"]
            result = {"data": [{"name": "ioi", "tools": {"reader_read": {}, "search_start": {}} if enabled else {}}], "nextCursor": None}
        elif method == "turn/start":
            thread = params["threadId"]
            turn = f"turn-{thread}"
            config = threads[thread]["mcp_servers"]["ioi"]
            emit({"id": request["id"], "result": {"turn": {"id": turn}}})
            if config["enabled"]:
                headers = {**config["http_headers"], "Content-Type": "application/json", "Accept": "application/json, text/event-stream"}
                rpc(config["url"], headers, "initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "fixture", "version": "1"}})
                initialized = urllib.request.Request(config["url"], data=json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}).encode(), headers=headers)
                with urllib.request.urlopen(initialized, timeout=5) as response:
                    response.read()
                response = rpc(config["url"], headers, "tools/call", {"name": "reader_read", "arguments": {"paper_id": scenario["paperId"]}})
                passage = response["structuredContent"]["passages"][0]
                assert passage["stateCitable"] and passage.get("pageStart") is None
                reference = passage["passageRef"]
                proposal = outcome(reference, False)
            else:
                context = json.loads(params["input"][0]["text"])
                assert reference in context["evidence"] and context["issues"]
                if scenario["correction"] in ("wait", "timeout"):
                    continue
                proposal = outcome(reference, scenario["correction"] == "valid")
            emit({"method": "item/completed", "params": {"threadId": thread, "turnId": turn, "item": {"type": "agentMessage", "text": json.dumps(proposal)}}})
            emit({"method": "turn/completed", "params": {"threadId": thread, "turn": {"id": turn, "status": "completed"}}})
            continue
        elif method == "turn/interrupt":
            emit({"method": "turn/completed", "params": {"threadId": params["threadId"], "turn": {"id": params["turnId"], "status": "interrupted"}}})
        emit({"id": request["id"], "result": result})


if __name__ == "__main__":
    main()
