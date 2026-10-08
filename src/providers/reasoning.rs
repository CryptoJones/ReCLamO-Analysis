//! Reasoning-format utilities.
//!
//! Different upstreams represent a model's reasoning differently. The loop
//! only ever sees clean `(visible, reasoning)` tuples, so all normalization
//! happens here.
//!
//! - **ThinkTags** (`<think>…</think>`): Qwen-style; either the upstream
//!   strips it or we have to. We always strip defensively.
//! - **ReasoningField**: a separate `reasoning_content` field with empty
//!   `content` (Strata, OpenRouter).
//! - **NativeBlocks**: Anthropic `thinking` blocks in `content[]` (already
//!   handled by the Anthropic provider, not by string handling).
//! - **None**: nothing to do.

/// Strip `<think>…</think>` blocks from `text` and return `(visible, reasoning)`.
///
/// Greedy, single-pass. Handles nested or repeated blocks. If there are no
/// `<think>` tags at all, returns `(text, None)`.
pub fn extract_think_tags(text: &str) -> (String, Option<String>) {
    let mut visible = String::with_capacity(text.len());
    let mut reasoning = String::new();
    let mut any = false;
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        visible.push_str(&rest[..start]);
        let after_open = &rest[start + "<think>".len()..];
        match after_open.find("</think>") {
            Some(end) => {
                let think = after_open[..end].trim().to_string();
                if !think.is_empty() {
                    if any {
                        reasoning.push('\n');
                    }
                    reasoning.push_str(&think);
                    any = true;
                }
                rest = &after_open[end + "</think>".len()..];
            }
            None => {
                // Unterminated `<think>` — treat as plain text, don't crash.
                visible.push_str(&rest[start..]);
                rest = "";
                break;
            }
        }
    }
    visible.push_str(rest);
    let visible = visible.trim().to_string();
    if any {
        (visible, Some(reasoning))
    } else {
        // No real `<think>` blocks (or all were empty/whitespace); still
        // return the stripped visible text, not the original input —
        // a stray empty `<think></think>` should not leak into the
        // conversation.
        (visible, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_tags_passes_through() {
        let (v, r) = extract_think_tags("hello world");
        assert_eq!(v, "hello world");
        assert_eq!(r, None);
    }

    #[test]
    fn single_block_stripped() {
        let (v, r) = extract_think_tags("<think>reasoning here</think>answer");
        assert_eq!(v, "answer");
        assert_eq!(r.as_deref(), Some("reasoning here"));
    }

    #[test]
    fn multiple_blocks_concatenated() {
        let (v, r) = extract_think_tags("<think>a</think>X<think>b</think>Y");
        assert_eq!(v, "XY");
        assert_eq!(r.as_deref(), Some("a\nb"));
    }

    #[test]
    fn empty_think_is_ignored() {
        let (v, r) = extract_think_tags("<think></think>just answer");
        assert_eq!(v, "just answer");
        assert_eq!(r, None);
    }

    #[test]
    fn whitespace_only_think_is_ignored() {
        let (v, r) = extract_think_tags("<think>   \n </think>visible");
        assert_eq!(v, "visible");
        assert_eq!(r, None);
    }

    #[test]
    fn unterminated_block_passes_through() {
        let (v, r) = extract_think_tags("<think>open but never closed");
        assert_eq!(v, "<think>open but never closed");
        assert_eq!(r, None);
    }

    #[test]
    fn leading_whitespace_is_trimmed() {
        let (v, r) = extract_think_tags("  \n<think>r</think>\n visible \n");
        assert_eq!(v, "visible");
        assert_eq!(r.as_deref(), Some("r"));
    }
}
