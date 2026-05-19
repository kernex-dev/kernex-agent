//! Adapter-shared helpers.
//!
//! Cross-adapter utilities used by more than one shell-CLI adapter
//! (Codex, OpenCode, Cursor, Cline). Keeping them here rather than in
//! `claude.rs` or `codex.rs` keeps the per-adapter modules focused on
//! the per-tool contract and avoids the "first adapter wins" ownership
//! pattern.

/// Idempotently merge `kernex_block` into `existing` between
/// `begin_marker` and `end_marker`.
///
/// Used for marker-based files where kernex owns a single, delimited
/// region and the rest of the file is user-owned content (e.g. Codex
/// `<cwd>/AGENTS.md`, OpenCode `AGENTS.md`, Cursor `.cursorrules`,
/// Cline `.clinerules`). The markers are caller-supplied because each
/// file format prefers a different comment syntax (HTML comments for
/// markdown, `#` for plain text).
///
/// Behaviour:
/// - Both markers present and ordered (`begin` before `end`): replace
///   the marker block (inclusive of markers) in place. Content outside
///   the block is byte-preserved.
/// - Either marker absent, or `end` before `begin`: append a fresh
///   marker block at the end of `existing`, separated by a blank line
///   if needed.
/// - `existing` is empty: returns just the marker block.
///
/// The marker block always renders as `{begin}\n{kernex_block}\n{end}\n`.
/// A trailing newline on `kernex_block` is normalised so successive
/// merges are stable (idempotent).
pub fn merge_marker_block(
    existing: &str,
    kernex_block: &str,
    begin_marker: &str,
    end_marker: &str,
) -> String {
    let body = kernex_block.trim_end_matches('\n');
    let block = format!("{begin_marker}\n{body}\n{end_marker}\n");

    if let (Some(begin_idx), Some(end_idx)) =
        (existing.find(begin_marker), existing.find(end_marker))
    {
        if begin_idx < end_idx {
            let end_close = end_idx + end_marker.len();
            // Consume one trailing newline after the end marker so
            // successive merges do not accumulate blank lines.
            let end_close = if existing[end_close..].starts_with('\n') {
                end_close + 1
            } else {
                end_close
            };
            let mut out = String::with_capacity(existing.len() + block.len());
            out.push_str(&existing[..begin_idx]);
            out.push_str(&block);
            out.push_str(&existing[end_close..]);
            return out;
        }
    }

    if existing.is_empty() {
        return block;
    }

    let mut out = String::with_capacity(existing.len() + block.len() + 2);
    out.push_str(existing);
    if !existing.ends_with('\n') {
        out.push('\n');
    }
    if !existing.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(&block);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEGIN: &str = "<!-- kernex:begin -->";
    const END: &str = "<!-- kernex:end -->";
    const BLOCK: &str = "Kernex says hi.\nLine 2.";

    #[test]
    fn appends_when_markers_absent() {
        let existing = "# User docs\n\nFirst paragraph.\n";
        let merged = merge_marker_block(existing, BLOCK, BEGIN, END);
        assert!(
            merged.starts_with(existing),
            "user content preserved verbatim"
        );
        assert!(merged.contains(BEGIN));
        assert!(merged.contains(END));
        assert!(merged.contains("Kernex says hi."));
    }

    #[test]
    fn appends_when_existing_empty() {
        let merged = merge_marker_block("", BLOCK, BEGIN, END);
        assert_eq!(
            merged,
            format!("{BEGIN}\n{BLOCK}\n{END}\n"),
            "empty input yields just the block"
        );
    }

    #[test]
    fn replaces_existing_marker_block() {
        let existing = format!(
            "# User docs\n\n{BEGIN}\nOLD kernex content.\n{END}\n\nTrailing user paragraph.\n"
        );
        let merged = merge_marker_block(&existing, BLOCK, BEGIN, END);
        assert!(
            !merged.contains("OLD kernex content."),
            "stale block replaced"
        );
        assert!(merged.contains("Kernex says hi."), "new block present");
        assert!(
            merged.contains("Trailing user paragraph."),
            "post-block user content preserved"
        );
        assert!(
            merged.starts_with("# User docs\n"),
            "pre-block user content preserved"
        );
    }

    #[test]
    fn idempotent_on_repeated_merge() {
        let existing = "# User docs\n";
        let once = merge_marker_block(existing, BLOCK, BEGIN, END);
        let twice = merge_marker_block(&once, BLOCK, BEGIN, END);
        assert_eq!(once, twice, "second merge does not drift");
    }

    #[test]
    fn ignores_out_of_order_markers() {
        // end appears before begin: treat as no usable existing block
        // and append a fresh one rather than mangling user text.
        let existing = format!("{END}\nstuff\n{BEGIN}\n");
        let merged = merge_marker_block(&existing, BLOCK, BEGIN, END);
        assert!(
            merged.contains("Kernex says hi."),
            "new block appended: {merged}"
        );
        assert!(
            merged.starts_with(&existing),
            "original out-of-order text preserved before the new block"
        );
    }

    #[test]
    fn normalises_trailing_newline_on_block() {
        let with_nl = "Body with newline.\n";
        let without_nl = "Body with newline.";
        let merged_a = merge_marker_block("", with_nl, BEGIN, END);
        let merged_b = merge_marker_block("", without_nl, BEGIN, END);
        assert_eq!(
            merged_a, merged_b,
            "trailing newline on input does not drift"
        );
    }
}
