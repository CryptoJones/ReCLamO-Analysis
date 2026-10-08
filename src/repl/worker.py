"""REPL worker for ReCLamO-Analysis.

Phase-2 worker. Read once by `SubprocessRepl::spawn` from a tempfile that the
orchestrator wrote via `include_str!`. The orchestrator is in Rust.

Protocol (JSON Lines on stdin/stdout):

- First line the orchestrator sends is always an `init` message with the
  context.
- Worker replies with `{"type":"ready"}`.
- Then either party can send:
  - Orchestrator → worker: `{"type":"exec","id":"...","code":"..."}`
  - Orchestrator → worker: `{"type":"lookup","id":"...","name":"..."}`
  - Orchestrator → worker: `{"type":"snapshot","id":"..."}`
  - Orchestrator → worker: `{"type":"shutdown"}`
- Worker → orchestrator (in response): `{"type":"result"|"error","id":"...",
  "ok":true,"stdout":"...","stderr":"..."}` (fields vary by request).

Helper bootstrap (matches the prompt):

- ``commit(text)`` writes to ``answer['content']``.
- ``llm_query(prompt)`` is a stub for phase 2 — it raises.
- ``llm_query_batched(prompts)`` is a stub for phase 2.
- ``extract_event_table(text, regex, key_groups=None)`` is implemented enough
  for tests.
- ``SHOW_VARS()`` prints all names in the namespace.
"""

from __future__ import annotations

import base64
import contextlib
import io
import json
import re
import sys
import traceback
from typing import Any, Iterable

# Output lines are truncated at this many characters to keep the orchestrator's
# history bounded. Matches the v0.1 default.
OUTPUT_LIMIT = 2000


def _truncate(s: str, n: int = OUTPUT_LIMIT) -> str:
    if len(s) <= n:
        return s
    return s[:n] + f"\n[truncated: {len(s)} chars; store it in a variable]"


class HarnessError(Exception):
    """An exception that's safe to surface to the model as a one-liner."""

    def __init__(self, klass: str, message: str) -> None:
        super().__init__(message)
        self.klass = klass
        self.message = message


def _safe_klass(exc: BaseException) -> str:
    return type(exc).__name__


def _run_exec(namespace: dict[str, Any], code: str) -> dict[str, Any]:
    """Run ``code`` in ``namespace``. Return a result dict."""
    sink = io.StringIO()
    err = io.StringIO()
    ok = True
    err_class = None
    err_message = ""
    try:
        with contextlib.redirect_stdout(sink), contextlib.redirect_stderr(err):
            compiled = compile(code, "<repl>", "exec")
            exec(compiled, namespace)
    except SystemExit as e:
        err_class = "SystemExit"
        err_message = str(e)
        ok = False
    except BaseException as e:  # noqa: BLE001 (we want every error class)
        err_class = _safe_klass(e)
        err_message = str(e)[:500]
        ok = False
    out = _truncate(sink.getvalue())
    # Don't echo err output to the model if the run succeeded and stderr is empty.
    err_out = _truncate(err.getvalue()) if not ok else ""
    return {
        "type": "result",
        "ok": ok,
        "stdout": out,
        "stderr": err_out,
        "error": None if ok else err_class,
    }


def _commit(text: str, namespace: dict[str, Any]) -> None:
    """Set ``answer['content']`` = ``text`` and echo a one-line confirmation."""
    ans = namespace.get("answer")
    if not isinstance(ans, dict):
        ans = {}
        namespace["answer"] = ans
    ans["content"] = text
    print(f"[commit] answer['content'] = {text!r}"[:OUTPUT_LIMIT])


def _show_vars(namespace: dict[str, Any]) -> None:
    """Print every (non-underscored) name in the namespace."""
    names = sorted(k for k in namespace if not k.startswith("_"))
    for n in names:
        print(f"{n}: {type(namespace[n]).__name__}")


def _extract_event_table(
    text: str, regex: str, key_groups: Iterable[str] | None = None
) -> list[dict[str, Any]]:
    """Pull matches out of prose into a Python list of dicts.

    ``key_groups`` maps the regex group names to row keys. If omitted,
    returns a list of ``{"match": <string>}``.
    """
    compiled = re.compile(regex)
    keys = list(key_groups) if key_groups is not None else None
    rows = []
    for m in compiled.finditer(text):
        if keys is None:
            rows.append({"match": m.group(0)})
        else:
            row = {}
            for k in keys:
                try:
                    row[k] = m.group(k)
                except (IndexError, error := Exception()) if False else Exception:  # noqa: E501
                    row[k] = None
            rows.append(row)
    return rows


def _llm_query_stub(prompt: str, subcall_chars: int = 12_000) -> str:
    raise HarnessError(
        "NotImplemented",
        "llm_query is a phase-2 stub; wire it to the orchestrator's sub-call channel",
    )


def _llm_query_batched_stub(prompts: list[str]) -> list[str]:
    raise HarnessError(
        "NotImplemented",
        "llm_query_batched is a phase-2 stub; wire it to the orchestrator's sub-call channel",
    )


def bootstrap_namespace(context: str) -> dict[str, Any]:
    """Initial namespace exposed to the REPL."""
    ns: dict[str, Any] = {"__name__": "__main__", "context": context}

    def commit(text: str) -> None:
        return _commit(text, ns)

    def llm_query(prompt: str, subcall_chars: int = 12_000) -> str:
        return _llm_query_stub(prompt, subcall_chars)

    def llm_query_batched(prompts: list[str]) -> list[str]:
        return _llm_query_batched_stub(prompts)

    def SHOW_VARS() -> None:
        return _show_vars(ns)

    def extract_event_table(
        text: str, regex: str, key_groups: Iterable[str] | None = None
    ) -> list[dict[str, Any]]:
        return _extract_event_table(text, regex, key_groups)

    ns["commit"] = commit
    ns["llm_query"] = llm_query
    ns["llm_query_batched"] = llm_query_batched
    ns["SHOW_VARS"] = SHOW_VARS
    ns["extract_event_table"] = extract_event_table
    ns["answer"] = {"content": ""}
    return ns


def _read_message() -> dict[str, Any] | None:
    line = sys.stdin.readline()
    if not line:
        return None
    line = line.strip()
    if not line:
        return None
    return json.loads(line)


def _write_message(msg: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(msg, ensure_ascii=False))
    sys.stdout.write("\n")
    sys.stdout.flush()


def _serve(namespace: dict[str, Any]) -> None:
    while True:
        msg = _read_message()
        if msg is None:
            return
        kind = msg.get("type")
        if kind == "exec":
            code = msg.get("code", "")
            result = _run_exec(namespace, code)
            result["id"] = msg.get("id")
            _write_message(result)
        elif kind == "lookup":
            name = msg.get("name", "")
            v = namespace.get(name, None)
            _write_message(
                {
                    "type": "result",
                    "id": msg.get("id"),
                    "ok": True,
                    "value": v,
                }
            )
        elif kind == "snapshot":
            visible = {
                k: _safe_repr(v) for k, v in namespace.items() if not k.startswith("_")
            }
            _write_message(
                {
                    "type": "result",
                    "id": msg.get("id"),
                    "ok": True,
                    "value": visible,
                }
            )
        elif kind == "shutdown":
            return
        else:
            _write_message(
                {
                    "type": "error",
                    "id": msg.get("id"),
                    "message": f"unknown message kind: {kind!r}",
                }
            )


def _safe_repr(v: Any) -> Any:
    """Best-effort JSON-safe repr of an arbitrary Python object."""
    if v is None or isinstance(v, (bool, int, float, str)):
        return v
    if isinstance(v, dict):
        return {str(k): _safe_repr(vv) for k, vv in v.items()}
    if isinstance(v, (list, tuple)):
        return [_safe_repr(x) for x in v]
    return f"<{type(v).__name__}: {str(v)[:200]}>"


def main() -> int:
    init = _read_message()
    if init is None or init.get("type") != "init":
        sys.stderr.write("worker: expected init message\n")
        return 2

    context_raw = init.get("context", "")
    fmt = init.get("context_format", "raw")
    if fmt == "base64":
        try:
            context_raw = base64.b64decode(context_raw).decode("utf-8", "replace")
        except Exception as e:  # noqa: BLE001
            sys.stderr.write(f"worker: decode context: {e}\n")
            return 3
    namespace = bootstrap_namespace(context_raw)
    _write_message({"type": "ready"})
    try:
        _serve(namespace)
    except Exception as e:  # noqa: BLE001
        sys.stderr.write(f"worker: serve loop died: {traceback.format_exc()}\n")
        return 4
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
