//! Markdown → HTML rendering for display.
//!
//! Rendering lives in this crate so the pure core owns the one function the
//! whole product depends on for XSS safety. The rule is absolute: raw HTML
//! in the source is escaped, never passed through, because issue bodies and
//! comments arrive from other people via `git pull`. There is no option to
//! change that.

/// Render markdown to HTML. Raw HTML and active constructs in the source
/// come back inert — comrak's safe mode drops them and leaves a visible
/// marker — so the output is safe to hand to a browser under a strict CSP.
pub fn render_html(text: &str) -> String {
    let mut options = crate::fmt::dit_options();
    // The fmt profile turns on CommonMark-minimizing output, which is wrong
    // for HTML rendering — it would re-escape content that already rendered.
    options.render.experimental_minimize_commonmark = false;
    // Display only: a single newline inside a paragraph shows as a line
    // break, as people expect when they press Enter. The bytes on disk are
    // untouched — `dit fmt` uses its own options.
    options.render.hardbreaks = true;
    // Lets the stylesheet drop the bullet in front of a checkbox.
    options.render.tasklist_classes = true;
    // The single most important line in this file: with `unsafe_` left off,
    // comrak escapes raw HTML instead of passing it through. It must never
    // be enabled anywhere in this workspace.
    comrak::markdown_to_html(text, &options)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn plain_markdown_becomes_html() {
        let html = render_html("hello **world**\n");
        assert!(html.contains("<strong>world</strong>"), "{html}");
    }

    #[test]
    fn raw_html_is_dropped_not_passed_through() {
        // comrak's safe mode replaces raw HTML with a visible marker comment
        // rather than escaping it into text — same guarantee (nothing active
        // survives), and the pinned expectation keeps any future options
        // change from quietly letting HTML through.
        let html = render_html("hi <script>alert(1)</script>\n");
        assert!(
            !html.contains("<script>"),
            "script tags must not survive: {html}"
        );
        assert!(html.contains("raw HTML omitted"), "{html}");
    }

    #[test]
    fn raw_html_in_the_middle_of_a_line_is_dropped_too() {
        let html = render_html("click <img src=x onerror=alert(1)> now\n");
        assert!(!html.contains("<img"), "{html}");
    }

    #[test]
    fn tables_and_task_lists_still_render() {
        let html = render_html("| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert!(html.contains("<table"), "{html}");
        let html = render_html("- [ ] todo\n");
        assert!(html.contains("checkbox"), "{html}");
    }

    #[test]
    fn a_single_newline_inside_a_paragraph_shows_as_a_line_break() {
        // People type Enter and expect the next line to start below — the
        // way GitHub shows issues and comments. Read as CommonMark it would
        // be a space, and the two lines would run together.
        let html = render_html("first line\nsecond line\n");
        assert!(html.contains("first line<br />"), "{html}");
        assert!(html.contains("second line"), "{html}");
    }

    #[test]
    fn task_items_carry_classes_the_stylesheet_can_target() {
        // Without classes a task item is an ordinary bulleted `li`, so the
        // bullet and the checkbox both show.
        let html = render_html("- [ ] open\n- [x] done\n");
        assert!(html.contains("contains-task-list"), "{html}");
        assert!(html.contains("task-list-item"), "{html}");
        assert!(html.contains("checked"), "{html}");
    }

    #[test]
    fn dangerous_autolinks_are_not_emitted_as_clickable_html() {
        let html = render_html("<javascript:alert(1)>\n");
        assert!(!html.contains("<a href=\"javascript"), "{html}");
    }
}
