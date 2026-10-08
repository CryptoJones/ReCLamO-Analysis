"""REPL worker for ReCLamO-Analysis.

Phase-2 worker. Read once by `SubprocessRepl::spawn` from a tempfile that the
orchestrator wrote via `include_str!`. The orchestrator is in Rust.

Protocol (JSON Lines on stdin/stdout):

- First line the orchestrator sends is always an `init` message with the
  context.
- Worker replies with `{"type":"ready"}`.
- Then either party can send:
  - Orchestrator → worker:
      `{"type":"exec","id":"...","code":"..."}`
      `{"type":"lookup","id":"...","name":"..."}`
      `{"type":"snapshot","id":"..."}`
      `{"type":"shutdown"}`
- Worker → orchestrator:
      `{"type":"subcall_request","id":"...","prompt":"..."}` (during exec)
      `{"type":"result","id":"...","ok":true,"stdout":"...","stderr":"..."}`
      `{"type":"error","id":"...","message":"..."}` (only for protocol violations;
       unhandled Python exceptions come back inside the exec result with
       `ok=false`, `error="<ClassName>"`)

Helper bootstrap (matches the prompt):

- ``commit(text)`` writes to ``answer['content']``.
- ``llm_query(prompt, *, subcall_chars=20_000)`` is **wired** — it round-
  trips a sub-call through the orchestrator's provider and returns the
  string. **Never** raises ``NotImplementedError``; if the orchestrator
  cannot dispatch (e.g. test-only stub), it raises ``HarnessError``.
- ``llm_query_batched(prompts)`` is serialized — one round-trip per
  prompt — so order is preserved and the model sees them in sequence.
- ``extract_event_table(text, regex, key_groups=None)`` is implemented.
- ``SHOW_VARS()`` prints all names in the namespace.

Runtime: stdlib only. Run via ``python3 -I <path>``.
"""

from __future__ import annotations

import base64
import contextlib
import io
import json
import re
import sys
import traceback
import uuid
from typing import Any, Callable, Iterable

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


def _write_message(msg: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(msg, ensure_ascii=False))
    sys.stdout.write("\n")
    sys.stdout.flush()


def _read_message() -> dict[str, Any] | None:
    line = sys.stdin.readline()
    if not line:
        return None
    line = line.strip()
    if not line:
        return None
    return json.loads(line)


def _send_subcall(
    send_fn: Callable[[dict[str, Any]], None],
    read_fn: Callable[[], dict[str, Any] | None],
    prompt: str,
) -> tuple[str, int]:
    """Round-trip one sub-call to the orchestrator's provider.

    Returns ``(text, tokens)``. Blocks on stdin until a matching
    ``subcall_response`` arrives.
    """
    sid = str(uuid.uuid4())
    send_fn({
        "type": "subcall_request",
        "id": sid,
        "prompt": prompt,
    })
    while True:
        msg = read_fn()
        if msg is None:
            raise HarnessError(
                "WorkerClosed",
                "stdin closed during sub-call",
            )
        if msg.get("type") != "subcall_response":
            # Other kinds shouldn't reach the worker here, but stay defensive.
            continue
        if msg.get("id") != sid:
            continue
        if not msg.get("ok", True):
            raise HarnessError(
                "SubcallError",
                str(msg.get("error") or "sub-call failed"),
            )
        text = msg.get("result", "") or ""
        try:
            tokens = int(msg.get("tokens", 0))
        except (TypeError, ValueError):
            tokens = 0
        return text, tokens


def _run_exec(
    namespace: dict[str, Any],
    code: str,
    send_fn: Callable[[dict[str, Any]], None],
) -> dict[str, Any]:
    """Run ``code`` in ``namespace``. Return a result dict.

    The result dict carries a ``subcalls`` list — one entry per
    ``llm_query`` / ``llm_query_batched`` invocation that round-tripped
    through the orchestrator. Each entry has ``prompt``, ``response``,
    ``tokens``. ``duration_secs`` is filled by the orchestrator's clock,
    not here.
    """
    sink = io.StringIO()
    err = io.StringIO()
    ok = True
    err_class = None
    err_message = ""
    subcalls: list[dict[str, Any]] = []

    def llm_query(prompt: str, *, subcall_chars: int = 20_000) -> str:
        if not isinstance(prompt, str):
            prompt = str(prompt)
        if len(prompt) > subcall_chars:
            prompt = prompt[:subcall_chars] + f"\n[truncated to {subcall_chars} chars]"
        text, tokens = _send_subcall(send_fn, _read_message, prompt)
        subcalls.append({
            "prompt": prompt,
            "response": text,
            "duration_secs": 0.0,
            "tokens": tokens,
        })
        return text

    def llm_query_batched(prompts: Iterable[str]) -> list[str]:
        return [llm_query(p) for p in prompts]

    # IMPORTANT: the model's code may print freely — `contextlib.redirect_stdout`
    # captures stdout so model prints land in `sink` instead of going to the
    # orchestrator's wire. But our own `_write_message` writes through
    # `sys.stdout`, which would ALSO be captured. We sidestep that by passing
    # in a `send_fn` bound to the **real** stdout at bootstrap time (not the
    # redirected `sys.stdout`). Stdin is unaffected by redirect_stdout so
    # `_read_message` is fine.

    # Snapshot the namespace so the helper definitions don't pollute it.
    inner_ns: dict[str, Any] = dict(namespace)
    inner_ns["llm_query"] = llm_query
    inner_ns["llm_query_batched"] = llm_query_batched

    try:
        with contextlib.redirect_stdout(sink), contextlib.redirect_stderr(err):
            compiled = compile(code, "<repl>", "exec")
            exec(compiled, inner_ns)
    except SystemExit as e:
        err_class = "SystemExit"
        err_message = str(e)
        ok = False
    except BaseException as e:  # noqa: BLE001 (we want every error class)
        err_class = _safe_klass(e)
        err_message = str(e)[:500]
        ok = False
    # Mirror `answer` back to the outer namespace so the orchestrator's
    # `lookup_var("answer")` sees whatever the model committed. This is
    # the commit-early wire (NEXT-STEPS fix #2).
    if "answer" in inner_ns:
        namespace["answer"] = inner_ns["answer"]
    out = _truncate(sink.getvalue())
    # Don't echo err output to the model if the run succeeded and stderr is empty.
    err_out = _truncate(err.getvalue()) if not ok else ""
    return {
        "type": "result",
        "ok": ok,
        "stdout": out,
        "stderr": err_out,
        "error": None if ok else err_class,
        "subcalls": subcalls,
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
    text: str,
    regex: str,
    key_groups: Iterable[str] | None = None,
) -> list[dict[str, Any]]:
    """Pull matches out of prose into a Python list of dicts.

    ``key_groups`` maps the regex group names to row keys. If omitted,
    returns a list of ``{"match": <string>}``. Missing groups become
    ``None`` rather than raising.
    """
    try:
        compiled = re.compile(regex)
    except re.error as e:
        raise HarnessError("RegexError", f"bad regex {regex!r}: {e}")
    keys = list(key_groups) if key_groups is not None else None
    rows: list[dict[str, Any]] = []
    for m in compiled.finditer(text):
        if keys is None:
            rows.append({"match": m.group(0)})
            continue
        row: dict[str, Any] = {}
        for k in keys:
            try:
                row[k] = m.group(k)
            except (IndexError, re.error):
                row[k] = None
        rows.append(row)
    return rows


def bootstrap_namespace(context: str) -> dict[str, Any]:
    """Initial namespace exposed to the REPL. ``llm_query`` is NOT bound
    here — ``_run_exec`` injects one tied to the live sub-call channel so
    the bound handler matches the current ``send_fn``."""
    ns: dict[str, Any] = {
        "__name__": "__main__",
        "context": context,
    }

    def commit(text: str) -> None:
        return _commit(text, ns)

    def SHOW_VARS() -> None:
        return _show_vars(ns)

    def extract_event_table(
        text: str,
        regex: str,
        key_groups: Iterable[str] | None = None,
    ) -> list[dict[str, Any]]:
        return _extract_event_table(text, regex, key_groups)

    ns["commit"] = commit
    ns["SHOW_VARS"] = SHOW_VARS
    ns["extract_event_table"] = extract_event_table
    ns["answer"] = {"content": ""}
    return ns


def _safe_repr(v: Any) -> Any:
    """Best-effort JSON-safe repr of an arbitrary Python object."""
    if v is None or isinstance(v, (bool, int, float, str)):
        return v
    if isinstance(v, dict):
        return {str(k): _safe_repr(vv) for k, vv in v.items()}
    if isinstance(v, (list, tuple)):
        return [_safe_repr(x) for x in v]
    return f"<{type(v).__name__}: {str(v)[:200]}>"


def _serve(namespace: dict[str, Any]) -> None:
    # Bind `send_fn` to the **original** stdout, not `sys.stdout`. Inside
    # `_run_exec` we wrap the user's code with `contextlib.redirect_stdout`
    # so the model cannot accidentally print JSON-on-stdout that the
    # orchestrator would mistake for a control message. By binding to the
    # original fd once, sub-call messages bypass the redirect.
    real_stdout = sys.stdout

    def send_fn(msg: dict[str, Any]) -> None:
        real_stdout.write(json.dumps(msg, ensure_ascii=False))
        real_stdout.write("\n")
        real_stdout.flush()

    while True:
        msg = _read_message()
        if msg is None:
            return
        kind = msg.get("type")
        if kind == "exec":
            code = msg.get("code", "")
            result = _run_exec(namespace, code, send_fn)
            result["id"] = msg.get("id")
            _write_message(result)
        elif kind == "lookup":
            name = msg.get("name", "")
            v = namespace.get(name, None)
            _write_message({
                "type": "result",
                "id": msg.get("id"),
                "ok": True,
                "value": v,
            })
        elif kind == "snapshot":
            visible = {
                k: _safe_repr(v) for k, v in namespace.items() if not k.startswith("_")
            }
            _write_message({
                "type": "result",
                "id": msg.get("id"),
                "ok": True,
                "value": visible,
            })
        elif kind == "shutdown":
            return
        else:
            _write_message({
                "type": "error",
                "id": msg.get("id"),
                "message": f"unknown message kind: {kind!r}",
            })


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
