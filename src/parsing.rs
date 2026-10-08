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

/// Parse one model response.
///
/// `text` should be the **visible** content (no `<think>…</think>` blocks);
/// the provider normalizes that for us.
pub fn parse_response(text: &str) -> ParsedResponse {
    let blocks = extract_code_blocks(text);
    let final_intent = extract_final(text);
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

/// Extract `FINAL(value)` or `FINAL_VAR(name)` from prose, outside code
/// blocks. Greedy, balanced-paren.
pub fn extract_final(text: &str) -> Option<FinalIntent> {
    // Walk the text looking for the tokens outside any code block.
    let blocks = extract_code_blocks(text);
    let mut spans: Vec<(usize, usize)> = blocks.iter().map(|b| (b.start, b.end)).collect();
    spans.sort_by_key(|s| s.0);

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
}
