//! Response parsing.
//!
//! Two responsibilities:
//!
//! 1. Find the code blocks (` ```repl `, ` ```python `, bare ` ``` `).
//! 2. Find the final answer intent: `FINAL(...)`, `FINAL_VAR(name)`.
//!
//! The parser ignores anything inside `<think>…</think>` so reasoning text
//! never counts as code. The same parser handles both the paper's
//! `FINAL/FINAL_VAR` parsing and the v0.1 fence conventions.

/// One fenced code block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlock {
    /// `repl`, `python`, or empty.
    pub lang: String,
    /// Stripped code (between the fences).
    pub code: String,
    /// Byte offset of the opening fence in the source text.
    pub start: usize,
    /// Byte offset just past the closing fence.
    pub end: usize,
}

/// The model's "I'm done" signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinalIntent {
    /// `FINAL_VAR(name)` — caller should look `name` up in the REPL.
    FinalVar(String),
    /// `FINAL(value)` — caller should return `value` verbatim.
    Final(String),
}

impl FinalIntent {
    /// Byte offset of the `FINAL` token in the source text.
    pub fn position(&self) -> usize {
        match self {
            FinalIntent::FinalVar(_) | FinalIntent::Final(_) => 0,
        }
    }
}

/// What `parse_response` returns.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedResponse {
    /// All fenced code blocks (in source order).
    pub code_blocks: Vec<CodeBlock>,
    /// The detected final-answer intent, if any.
    pub final_intent: Option<FinalIntent>,
    /// If the FINAL should be rejected (e.g. appears before code that
    /// hasn't run yet, or the value looks like a plan).
    pub reject_reason: Option<String>,
}

/// Output of `extract_tool_call_blocks` — the same `CodeBlock` and
/// `FinalIntent` shapes as the fence-based extractor, plus the byte
/// ranges of every `<tool_call>` block (so `extract_final` can skip them
/// when scanning the surrounding prose for a bare `FINAL(`).
#[derive(Debug, Default, Clone)]
pub struct ToolCallExtraction {
    pub code: Vec<CodeBlock>,
    pub final_intent: Option<FinalIntent>,
    pub spans: Vec<(usize, usize)>,
}

/// Parse one model response.
///
/// `text` should be the **visible** content (no `<think>…</think>` blocks);
/// the provider normalizes that for us.
pub fn parse_response(text: &str) -> ParsedResponse {
    let mut blocks = extract_code_blocks(text);
    let tool_call = extract_tool_call_blocks(text);
    blocks.extend(tool_call.code);
    blocks.sort_by_key(|b| b.start);
    // The fence-based scan knows about ```repl / ```python / bare
    // fences; the tool-call scan knows about Poolside Laguna's
    // `<tool_call>repl|…</arg_value>` text form. Either may carry
    // the final intent; take whichever found one.
    let final_intent = extract_final(text).or(tool_call.final_intent);
    let mut reject_reason: Option<String> = None;

    if let Some(intent) = &final_intent {
        // Locate the FINAL token in the source. extract_final returns the
        // positional offset, but FinalIntent currently only carries the
        // semantic info. For the ordering check we re-scan for `FINAL(` /
        // `FINAL_VAR(`.
        let final_pos = find_final_token(text).unwrap_or(0);
        let min_code_start = blocks.iter().map(|b| b.start).min();
        if let Some(code_start) = min_code_start {
            // Reject FINAL that comes BEFORE the code block (the code
            // never had a chance to run). FINAL after code is fine.
            if final_pos < code_start {
                reject_reason = Some("FINAL appears before code that hasn't run yet".to_string());
            }
        }
        if reject_reason.is_none() {
            if let Some(value) = intent_value(intent) {
                if looks_like_plan(&value) {
                    reject_reason = Some(format!("FINAL looks like a plan: {:?}", value));
                }
            }
        }
    }

    ParsedResponse { code_blocks: blocks, final_intent, reject_reason }
}

fn intent_value(i: &FinalIntent) -> Option<String> {
    match i {
        FinalIntent::Final(s) => Some(s.clone()),
        FinalIntent::FinalVar(_) => None,
    }
}

fn looks_like_plan(s: &str) -> bool {
    let lower = s.trim_start().to_ascii_lowercase();
    let first = lower.chars().take(60).collect::<String>();
    first.starts_with("i will")
        || first.starts_with("we will")
        || first.starts_with("let's ")
        || first.starts_with("first, ")
        || first.starts_with("step 1")
        || first.starts_with("step one")
        || first.starts_with("plan:")
        || first.contains("\nstep ")
}

/// Find the first `FINAL(` or `FINAL_VAR(` token and return its byte offset.
fn find_final_token(text: &str) -> Option<usize> {
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if let Some(rel) = find_token(rest, "FINAL_VAR(") {
            return Some(i + rel);
        }
        if let Some(rel) = find_token(rest, "FINAL(") {
            // Avoid matching `FINALVAR(`. find_token is exact-prefix so this
            // is fine — `FINALVAR(` doesn't start with `FINAL(`.
            return Some(i + rel);
        }
        // Advance one char (UTF-8 safe).
        i += rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    None
}

fn find_token(hay: &str, needle: &str) -> Option<usize> {
    hay.find(needle)
}

/// Extract all fenced code blocks. Recognizes ```repl, ```python, bare ```.
pub fn extract_code_blocks(text: &str) -> Vec<CodeBlock> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        // Look for ```
        if &bytes[i..i + 3] == b"```" {
            // Find end of line (lang hint).
            let line_start = i + 3;
            let lang_end = text[line_start..]
                .find('\n')
                .map(|p| line_start + p)
                .unwrap_or(text.len());
            let lang = text[line_start..lang_end].trim().to_string();
            // Find closing ```.
            if let Some(close_rel) = text[lang_end..].find("```") {
                let code_start = lang_end + 1; // skip '\n'
                let code_end = lang_end + close_rel;
                // Trim trailing newline at end of code.
                let code = text[code_start..code_end].trim_end_matches('\n').to_string();
                let end = code_end + 3;
                let start = i;
                out.push(CodeBlock { lang, code, start, end });
                i = end;
                continue;
            } else {
                // Unterminated fence — treat the rest as code.
                let code = text[line_start.min(text.len())..].to_string();
                out.push(CodeBlock {
                    lang,
                    code,
                    start: i,
                    end: text.len(),
                });
                i = text.len();
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Extract Poolside Laguna S 2.1's text-form `<tool_call>` blocks.
///
/// Laguna writes code inside `content` rather than lifting it into
/// `message.tool_calls`. The shape:
///
/// ```text
/// <tool_call>repl|python|execute_python
/// [optional <arg_key>name</arg_key><arg_value>]
/// <code>
/// [closer: </arg_value> | </value> | </repl> | </tool_call> | </think>]
/// ```
///
/// and `<tool_call>FINAL(value)</tool_call>` /
/// `<tool_call>FINAL_VAR(name)</tool_call>` for the final answer.
/// Multiple blocks in one reply are common (Laguna was seen writing
/// ~12 in a row, with **fabricated REPL output** in between — bug #4
/// keeps only the first block; this extractor just collects them all
/// and lets the caller pick).
///
/// Reference: ReCLamO-Harness PR #52 (`c37b220`); the OMI note
/// `Poolside Laguna S 2.1 via API` documents the wire format.
pub fn extract_tool_call_blocks(text: &str) -> ToolCallExtraction {
    let mut out = ToolCallExtraction::default();
    let mut i = 0usize;
    while let Some(open_rel) = text[i..].find("<tool_call>") {
        let start = i + open_rel;
        let after_tag = start + "<tool_call>".len();
        // Skip whitespace between `<tool_call>` and the lang token.
        let ws_off = text[after_tag..]
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(0);
        let lang_start = after_tag + ws_off;
        // Find where the lang token ends. Whitespace, `<`, or `(` are
        // all valid terminators.
        let token_end_rel = text[lang_start..]
            .find(|c: char| c.is_whitespace() || c == '<' || c == '(')
            .unwrap_or(text.len() - lang_start);
        let token_end = lang_start + token_end_rel;
        let lang = text[lang_start..token_end].to_string();

        match lang.as_str() {
            "FINAL" => {
                // `<tool_call>FINAL(value)</tool_call>`
                let paren_open = text[token_end..]
                    .find('(')
                    .map(|p| token_end + p)
                    .unwrap_or(text.len());
                let after = paren_open + 1;
                if let Some((inner, _)) = read_balanced(&text[after..]) {
                    out.final_intent = Some(FinalIntent::Final(inner.trim().to_string()));
                }
                let block_end = text[after..]
                    .find("</tool_call>")
                    .map(|p| after + p + "</tool_call>".len())
                    .unwrap_or(text.len());
                out.spans.push((start, block_end));
                i = block_end;
            }
            "FINAL_VAR" => {
                let paren_open = text[token_end..]
                    .find('(')
                    .map(|p| token_end + p)
                    .unwrap_or(text.len());
                let after = paren_open + 1;
                if let Some((inner, _)) = read_balanced(&text[after..]) {
                    if let Some(name) = first_token(&inner) {
                        out.final_intent = Some(FinalIntent::FinalVar(name));
                    }
                }
                let block_end = text[after..]
                    .find("</tool_call>")
                    .map(|p| after + p + "</tool_call>".len())
                    .unwrap_or(text.len());
                out.spans.push((start, block_end));
                i = block_end;
            }
            "repl" => {
                if let Some((cb, end)) = parse_tool_call_code(text, start, token_end, "repl") {
                    out.code.push(cb);
                    out.spans.push((start, end));
                    i = end;
                } else {
                    i = after_tag;
                }
            }
            "python" | "execute_python" => {
                // Map `execute_python` to `python` so the REPL picks it
                // up uniformly.
                if let Some((cb, end)) = parse_tool_call_code(text, start, token_end, "python") {
                    out.code.push(cb);
                    out.spans.push((start, end));
                    i = end;
                } else {
                    i = after_tag;
                }
            }
            _ => {
                // Unknown `<tool_call>` — don't loop forever, just skip
                // past the opening tag and continue scanning.
                i = after_tag;
            }
        }
    }
    out
}

/// Helper: parse a `<tool_call>lang …[closer]` code block. Returns
/// `(CodeBlock, end_offset)` or `None` if the block is malformed.
fn parse_tool_call_code(
    text: &str,
    start: usize,
    token_end: usize,
    mapped_lang: &str,
) -> Option<(CodeBlock, usize)> {
    // Optional `<arg_key>name</arg_key><arg_value>` prefix is allowed
    // (Laguna sometimes adds it; we don't need the description).
    let mut content_start = token_end;
    if let Some(av_rel) = text[content_start..].find("<arg_value>") {
        content_start = content_start + av_rel + "<arg_value>".len();
    }
    // Trim leading whitespace from the code proper.
    let after = &text[content_start..];
    content_start = content_start
        + after.find(|c: char| !c.is_whitespace()).unwrap_or(after.len());
    let rest = &text[content_start..];
    let closers = [
        ("</arg_value>", "</arg_value>".len()),
        ("</value>", "</value>".len()),
        ("</repl>", "</repl>".len()),
        ("</think>", "</think>".len()),
        ("</tool_call>", "</tool_call>".len()),
    ];
    let close = closers
        .iter()
        .filter_map(|&(c, l)| rest.find(c).map(|p| (p, l)))
        .min_by_key(|&(p, _)| p);
    let (code_end_rel, closer_len) = match close {
        Some((p, l)) => (p, l),
        None => return None, // malformed: no closer found
    };
    let code = rest[..code_end_rel]
        .trim_end_matches('\n')
        .to_string();
    let end = content_start + code_end_rel + closer_len;
    Some((
        CodeBlock {
            lang: mapped_lang.to_string(),
            code,
            start,
            end,
        },
        end,
    ))
}

/// Extract `FINAL(value)` or `FINAL_VAR(name)` from prose, outside code
/// blocks. Greedy, balanced-paren.
pub fn extract_final(text: &str) -> Option<FinalIntent> {
    // Walk the text looking for the tokens outside any code block.
    // Both fence-based and `<tool_call>` blocks are skipped — the
    // tool_call FINAL is captured separately by `extract_tool_call_blocks`
    // and folded in by `parse_response`.
    let mut spans: Vec<(usize, usize)> = extract_code_blocks(text)
        .iter()
        .map(|b| (b.start, b.end))
        .collect();
    let tc = extract_tool_call_blocks(text);
    spans.extend(tc.spans);
    spans.sort_by_key(|s| s.0);
    spans.dedup();

    let mut i = 0;
    while i < text.len() {
        // Skip past any code block we're inside.
        if let Some((_, end)) = spans.iter().find(|(s, e)| *s <= i && i < *e) {
            i = *end;
            continue;
        }
        if let Some(pos) = find_final_token(&text[i..]) {
            let abs = i + pos;
            // Try FINAL_VAR first.
            let tok_var = "FINAL_VAR(";
            let tok = "FINAL(";
            if text[abs..].starts_with(tok_var) {
                if let Some((name, _next)) = read_balanced(&text[abs + tok_var.len()..]) {
                    if let Some(name) = first_token(&name) {
                        return Some(FinalIntent::FinalVar(name.to_string()));
                    }
                }
                i = abs + tok_var.len();
                continue;
            } else if text[abs..].starts_with(tok) {
                if let Some((value, _)) = read_balanced(&text[abs + tok.len()..]) {
                    return Some(FinalIntent::Final(value.trim().to_string()));
                }
                i = abs + tok.len();
                continue;
            }
        }
        i += text[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    None
}

/// Read a balanced `(...)` group starting at `start`. Returns the inner text
/// and the offset just past the closing paren.
///
/// The caller has already consumed `FINAL(` / `FINAL_VAR(`, so this function
/// starts at depth 1 (the content's open paren has been seen).
fn read_balanced(s: &str) -> Option<(String, usize)> {
    let mut depth: u32 = 1;
    let mut out = String::new();
    let mut in_str: Option<char> = None;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = s[i..].chars().next()?;
        if let Some(q) = in_str {
            if c == q {
                in_str = None;
            }
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        match c {
            '"' | '\'' => {
                in_str = Some(c);
                out.push(c);
                i += c.len_utf8();
                continue;
            }
            '(' => {
                depth += 1;
                out.push(c);
            }
            ')' => {
                if depth == 1 {
                    return Some((out, i + 1));
                }
                depth -= 1;
                out.push(c);
            }
            _ => out.push(c),
        }
        i += c.len_utf8();
    }
    None
}

/// Take the first identifier-ish token from `s` (so `answer["x"]` →
/// `answer`). Used so FINAL_VAR(`answer["x"]`) still counts as a variable.
fn first_token(s: &str) -> Option<String> {
    let s = s.trim();
    let end = s
        .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '"' && c != '\'')
        .unwrap_or(s.len());
    if end == 0 {
        None
    } else {
        Some(s[..end].trim_matches(|c: char| c == '"' || c == '\'').to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_one_repl_block() {
        let txt = "some prose\n```repl\nprint('hi')\n```\nmore prose";
        let blocks = extract_code_blocks(txt);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].lang, "repl");
        assert_eq!(blocks[0].code, "print('hi')");
    }

    #[test]
    fn extract_python_block() {
        let txt = "```python\nx = 1\n```";
        let blocks = extract_code_blocks(txt);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].lang, "python");
        assert_eq!(blocks[0].code, "x = 1");
    }

    #[test]
    fn extract_bare_block() {
        let txt = "```\ny = 2\n```";
        let blocks = extract_code_blocks(txt);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].lang, "");
        assert_eq!(blocks[0].code, "y = 2");
    }

    #[test]
    fn extract_multiple_blocks_keeps_order() {
        let txt = "```repl\na\n```\nsep\n```python\nb\n```";
        let blocks = extract_code_blocks(txt);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].code, "a");
        assert_eq!(blocks[1].code, "b");
    }

    #[test]
    fn final_var_parses() {
        let txt = "I am done.\nFINAL_VAR(answer)";
        let intent = extract_final(txt).unwrap();
        assert_eq!(intent, FinalIntent::FinalVar("answer".to_string()));
    }

    #[test]
    fn final_parses_with_spaces() {
        let txt = "FINAL(  hello world  )";
        let intent = extract_final(txt).unwrap();
        assert_eq!(intent, FinalIntent::Final("hello world".to_string()));
    }

    #[test]
    fn final_var_with_dict_access_yields_var_name() {
        let txt = "FINAL_VAR(answer[\"x\"])";
        let intent = extract_final(txt).unwrap();
        assert_eq!(intent, FinalIntent::FinalVar("answer".to_string()));
    }

    #[test]
    fn final_inside_code_block_ignored() {
        let txt = "```repl\nFINAL_VAR(x)\n```";
        assert_eq!(extract_final(txt), None);
    }

    #[test]
    fn plan_like_is_rejected() {
        // Text inside a `FINAL(...)` that looks like a plan gets rejected.
        let txt = "FINAL(I will compute the answer step by step.)";
        let p = parse_response(txt);
        assert!(p.final_intent.is_some());
        assert!(p.reject_reason.is_some(), "plan-like FINAL should be rejected: {:?}", p.reject_reason);
        // And the helper itself:
        assert!(looks_like_plan("I will do x"));
        assert!(looks_like_plan("Step 1: do foo"));
        assert!(looks_like_plan("First, let me check"));
        assert!(!looks_like_plan("The answer is 42"));
    }

    #[test]
    fn final_var_before_code_is_rejected() {
        let txt = "FINAL_VAR(x)\n```repl\nprint(1)\n```";
        let p = parse_response(txt);
        assert_eq!(p.final_intent, Some(FinalIntent::FinalVar("x".into())));
        assert!(p.reject_reason.is_some());
    }

    #[test]
    fn final_var_after_code_passes() {
        let txt = "```repl\nprint(1)\n```\nFINAL_VAR(x)";
        let p = parse_response(txt);
        assert_eq!(p.final_intent, Some(FinalIntent::FinalVar("x".into())));
        assert!(p.reject_reason.is_none());
    }

    #[test]
    fn balanced_parens_handle_nested_call() {
        let txt = "FINAL(max(1, 2, (3 + 4)))";
        let intent = extract_final(txt).unwrap();
        match intent {
            FinalIntent::Final(s) => assert_eq!(s, "max(1, 2, (3 + 4))"),
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn unterminated_final_returns_none() {
        let txt = "FINAL(answer without close";
        assert_eq!(extract_final(txt), None);
    }

    // ---------- <tool_call> text-form (Poolside Laguna) ----------

    #[test]
    fn tool_call_repl_block_with_arg_value_closer() {
        // The exact shape Laguna emits: `<tool_call>repl` then a
        // `<arg_key>description</arg_key><arg_value>` prefix, the
        // code, then `</arg_value>` as the closer.
        let txt = "\
<tool_call>repl\n<arg_key>description</arg_key><arg_value>print('hi')\n</arg_value>";
        let tc = extract_tool_call_blocks(txt);
        assert_eq!(tc.code.len(), 1, "expected one code block, got {tc:?}");
        assert_eq!(tc.code[0].lang, "repl");
        assert_eq!(tc.code[0].code, "print('hi')");
        assert_eq!(tc.spans.len(), 1);
    }

    #[test]
    fn tool_call_repl_block_with_tool_call_closer() {
        // Simpler shape: no arg_value prefix, just the closer tag.
        let txt = "<tool_call>repl\nprint(1)\n</tool_call>";
        let tc = extract_tool_call_blocks(txt);
        assert_eq!(tc.code.len(), 1);
        assert_eq!(tc.code[0].lang, "repl");
        assert_eq!(tc.code[0].code, "print(1)");
    }

    #[test]
    fn tool_call_python_block_maps_to_python_lang() {
        // `execute_python` is the OpenAI-style alias; the harness
        // should treat it as `python` so the REPL picks it up.
        let txt = "<tool_call>execute_python\nx = 1\n</tool_call>";
        let tc = extract_tool_call_blocks(txt);
        assert_eq!(tc.code.len(), 1);
        assert_eq!(tc.code[0].lang, "python");
        assert_eq!(tc.code[0].code, "x = 1");
    }

    #[test]
    fn tool_call_final_yields_final_intent() {
        let txt = "<tool_call>FINAL(Paris)</tool_call>";
        let tc = extract_tool_call_blocks(txt);
        assert_eq!(tc.final_intent, Some(FinalIntent::Final("Paris".into())));
    }

    #[test]
    fn tool_call_final_var_yields_final_var_intent() {
        let txt = "<tool_call>FINAL_VAR(answer)</tool_call>";
        let tc = extract_tool_call_blocks(txt);
        assert_eq!(tc.final_intent, Some(FinalIntent::FinalVar("answer".into())));
    }

    #[test]
    fn tool_call_block_uses_closest_closer() {
        // The extractor picks the first closer in source order. If
        // both `</arg_value>` and `</think>` are present, the earlier
        // one wins. (We don't currently do string-aware scanning, so
        // `</think>` inside a Python string literal would also be
        // picked — but Laguna's wire format doesn't put the closers
        // inside code, so this hasn't bitten us in practice.)
        let txt = "<tool_call>repl\nprint(1)\n</arg_value> </think> ignore";
        let tc = extract_tool_call_blocks(txt);
        assert_eq!(tc.code.len(), 1);
        assert_eq!(tc.code[0].code, "print(1)");
    }

    #[test]
    fn parse_response_merges_fence_and_tool_call_blocks() {
        // Mixed reply: one ```repl block, one `<tool_call>repl` block.
        let txt = "\
```repl\nprint('a')\n```
<tool_call>repl\nprint('b')\n</arg_value>";
        let p = parse_response(txt);
        assert_eq!(p.code_blocks.len(), 2);
        assert_eq!(p.code_blocks[0].code, "print('a')");
        assert_eq!(p.code_blocks[1].code, "print('b')");
        // Sorted by start position.
        assert!(p.code_blocks[0].start < p.code_blocks[1].start);
    }

    #[test]
    fn parse_response_uses_tool_call_final_when_fence_scan_finds_none() {
        // Only the tool-call form carries the final intent.
        let txt = "\
<tool_call>repl\nx = 1\n</arg_value>
The answer is 42.
<tool_call>FINAL_VAR(answer)</tool_call>";
        let p = parse_response(txt);
        assert_eq!(p.code_blocks.len(), 1);
        assert_eq!(p.code_blocks[0].code, "x = 1");
        assert_eq!(p.final_intent, Some(FinalIntent::FinalVar("answer".into())));
    }

    #[test]
    fn extract_final_skips_inside_tool_call_blocks() {
        // The bare `FINAL(...)` inside a `<tool_call>FINAL(...)` block
        // must NOT be picked up by `extract_final` — that's the
        // tool_call extractor's job. The same is true for
        // `<tool_call>repl\nFINAL(x)\n` — `FINAL` inside a code block
        // is just code, not a final-intent.
        let txt1 = "<tool_call>FINAL(skip me)</tool_call>";
        assert_eq!(extract_final(txt1), None);
        let txt2 = "<tool_call>repl\nFINAL(skip me)\n</arg_value>";
        assert_eq!(extract_final(txt2), None);
    }
}
