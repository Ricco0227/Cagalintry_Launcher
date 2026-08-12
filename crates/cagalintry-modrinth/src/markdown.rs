//! Rendering project descriptions.
//!
//! Modrinth bodies are Markdown that may also contain raw HTML, all of it
//! written by whoever uploaded the mod. It is rendered here, on the Rust side
//! of the IPC boundary, and sanitised before it is ever handed to the webview —
//! so nothing untrusted reaches the DOM in the first place, rather than relying
//! on the frontend to be careful with it.

use std::collections::{HashMap, HashSet};

use ammonia::Builder;
use pulldown_cmark::{Options, Parser, html};

/// Convert a Markdown description into HTML that is safe to insert.
///
/// Scripts, event handlers, styles, iframes and anything with a scheme other
/// than http(s) are removed. What survives is the formatting a description
/// actually needs.
pub fn to_safe_html(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);

    let mut unsafe_html = String::new();
    html::push_html(&mut unsafe_html, Parser::new_ext(markdown, options));

    sanitiser().clean(&unsafe_html).to_string()
}

fn sanitiser() -> Builder<'static> {
    let mut builder = Builder::default();

    builder
        .tags(HashSet::from([
            "p", "br", "hr", "h1", "h2", "h3", "h4", "h5", "h6", "strong", "b", "em", "i", "del",
            "s", "code", "pre", "blockquote", "ul", "ol", "li", "a", "img", "table", "thead",
            "tbody", "tr", "th", "td", "details", "summary", "span", "div",
        ]))
        .tag_attributes(HashMap::from([
            ("a", HashSet::from(["href", "title"])),
            ("img", HashSet::from(["src", "alt", "title"])),
        ]))
        // No javascript:, no data: — the two ways a link or image smuggles in
        // something executable.
        .url_schemes(HashSet::from(["http", "https"]))
        .link_rel(Some("noopener noreferrer"))
        // Descriptions must not restyle the launcher around them.
        .generic_attributes(HashSet::new());

    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_ordinary_markdown() {
        let html = to_safe_html("# Sodium\n\nA **fast** renderer with `chunks`.\n\n- one\n- two");
        assert!(html.contains("<h1>Sodium</h1>"));
        assert!(html.contains("<strong>fast</strong>"));
        assert!(html.contains("<code>chunks</code>"));
        assert!(html.contains("<li>one</li>"));
    }

    #[test]
    fn renders_tables_and_strikethrough() {
        // Both appear constantly in mod compatibility tables.
        let html = to_safe_html("| a | b |\n|---|---|\n| 1 | 2 |\n\n~~gone~~");
        assert!(html.contains("<table>"));
        assert!(html.contains("<td>1</td>"));
        assert!(html.contains("<del>gone</del>"));
    }

    #[test]
    fn strips_scripts() {
        let html = to_safe_html("Hello <script>alert('xss')</script> world");
        assert!(!html.contains("<script"));
        assert!(!html.contains("alert"));
        assert!(html.contains("Hello"));
    }

    #[test]
    fn strips_event_handlers() {
        let html = to_safe_html(r#"<img src="https://example.test/a.png" onerror="alert(1)">"#);
        assert!(!html.contains("onerror"));
        assert!(!html.contains("alert"));
        // The image itself is legitimate and survives.
        assert!(html.contains("https://example.test/a.png"));
    }

    #[test]
    fn rejects_dangerous_url_schemes() {
        let html = to_safe_html("[click](javascript:alert(1))");
        assert!(!html.contains("javascript:"));

        let html = to_safe_html(r#"<img src="data:text/html;base64,PHNjcmlwdD4=">"#);
        assert!(!html.contains("data:"));
    }

    #[test]
    fn strips_iframes_and_inline_styles() {
        let html = to_safe_html(r#"<iframe src="https://evil.test"></iframe>"#);
        assert!(!html.contains("<iframe"));

        // A description must not be able to restyle the launcher around it.
        let html = to_safe_html(r#"<p style="position:fixed;inset:0">covering</p>"#);
        assert!(!html.contains("style="));
        assert!(html.contains("covering"));
    }

    #[test]
    fn external_links_are_marked_noopener() {
        let html = to_safe_html("[Modrinth](https://modrinth.com)");
        assert!(html.contains(r#"href="https://modrinth.com""#));
        assert!(html.contains("noopener"));
    }

    #[test]
    fn images_and_links_from_arbitrary_hosts_survive() {
        // Descriptions routinely embed screenshots from GitHub or Imgur.
        let html = to_safe_html("![shot](https://i.imgur.com/x.png)\n\n[repo](https://github.com/a/b)");
        assert!(html.contains("https://i.imgur.com/x.png"));
        assert!(html.contains("https://github.com/a/b"));
    }

    #[test]
    fn an_empty_description_renders_to_nothing() {
        assert!(to_safe_html("").trim().is_empty());
    }
}
