//! Readmes shown in the app (docs/PLAN.md, "Phase 3 design"): Markdown as
//! formatted text, anything else as plain text. Raw HTML in a readme is shown as
//! text, never passed through, so a downloaded readme can't run anything.

use pulldown_cmark::{html, CowStr, Event, Options, Parser};

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The file as HTML for the page.
pub fn to_html(name: &str, bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lower = name.to_lowercase();
    if lower.ends_with(".md") || lower.ends_with(".markdown") {
        let parser = Parser::new_ext(
            &text,
            Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
        )
        .map(|e| match e {
            Event::Html(s) | Event::InlineHtml(s) => Event::Text(s),
            // links and pictures only to the web (or nowhere): no javascript: and the like
            Event::Start(pulldown_cmark::Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => Event::Start(pulldown_cmark::Tag::Link {
                link_type,
                dest_url: safe(dest_url),
                title,
                id,
            }),
            Event::Start(pulldown_cmark::Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => Event::Start(pulldown_cmark::Tag::Image {
                link_type,
                dest_url: safe(dest_url),
                title,
                id,
            }),
            e => e,
        });
        let mut out = String::new();
        html::push_html(&mut out, parser);
        out
    } else {
        format!("<pre class=\"doc-text\">{}</pre>", escape(&text))
    }
}

fn safe(url: CowStr) -> CowStr {
    if url.starts_with("http://") || url.starts_with("https://") {
        url
    } else {
        CowStr::Borrowed("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_without_raw_html() {
        let h = to_html("README.md", b"# Tyrant\n\nPrint at **0.05 mm**. <script>alert(1)</script> [site](https://x.com) [bad](javascript:alert(1))");
        assert!(h.contains("<h1>Tyrant</h1>") && h.contains("<strong>0.05 mm</strong>"));
        assert!(!h.contains("<script>") && h.contains("&lt;script&gt;"));
        assert!(h.contains("href=\"https://x.com\"") && !h.contains("javascript:"));
        assert_eq!(
            to_html("a.txt", b"<b>"),
            "<pre class=\"doc-text\">&lt;b&gt;</pre>"
        );
    }
}
