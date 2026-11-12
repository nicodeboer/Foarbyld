//! Markdown -> sanitized HTML.
//!
//! The webview shows the result as-is, so everything here is about making that
//! safe: raw HTML in the Markdown is sanitized (no scripts, no event handlers),
//! local images are inlined as `data:` URIs (the webview CSP only allows
//! `'self'` and `data:`), and remote images are turned into plain links so a
//! document cannot make the viewer phone home.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};

/// Largest local image that is inlined. Bigger files are left as a link.
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

/// Renders `markdown` to sanitized HTML. Relative image paths are resolved
/// against `base_dir` (normally the directory of the Markdown file).
pub fn render(markdown: &str, base_dir: Option<&Path>) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION;

    // An image we could not (or must not) inline is emitted as a link, so its
    // alt text stays readable. The matching end tag has to be rewritten too.
    let mut image_as_link = false;
    let events = Parser::new_ext(markdown, options).map(|event| match event {
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => match inline_image(&dest_url, base_dir) {
            Some(data_uri) => Event::Start(Tag::Image {
                link_type,
                dest_url: CowStr::from(data_uri),
                title,
                id,
            }),
            None => {
                image_as_link = true;
                Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                })
            }
        },
        Event::End(TagEnd::Image) if image_as_link => {
            image_as_link = false;
            Event::End(TagEnd::Link)
        }
        other => other,
    });

    let mut out = String::new();
    html::push_html(&mut out, events);
    sanitizer().clean(&out).to_string()
}

fn sanitizer() -> ammonia::Builder<'static> {
    let mut b = ammonia::Builder::default();
    // Task list checkboxes.
    b.add_tags(["input"]);
    b.add_tag_attributes("input", ["type", "checked", "disabled"]);
    // `data:` is needed for inlined images. Links are only ever opened by the
    // frontend for http(s)/mailto, so a `data:` link is inert.
    b.url_schemes(HashSet::from(["http", "https", "mailto", "data"]));
    b.link_rel(Some("noopener noreferrer"));
    b
}

/// Returns a `data:` URI for a local image, or `None` if `dest` is remote,
/// not an image, unreadable or too large.
fn inline_image(dest: &str, base_dir: Option<&Path>) -> Option<String> {
    if dest.starts_with("data:image/") {
        return Some(dest.to_string());
    }
    if has_scheme(dest) || dest.starts_with("//") {
        return None;
    }
    let path = resolve(dest, base_dir?)?;
    let mime = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml", // safe: scripts never run inside an <img>
        _ => return None,
    };
    if std::fs::metadata(&path).ok()?.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{b64}"))
}

fn has_scheme(s: &str) -> bool {
    match s.find(':') {
        Some(i) if i > 1 => s[..i]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')),
        _ => false, // "C:\..." (i == 1) is a Windows path, not a scheme
    }
}

fn resolve(dest: &str, base_dir: &Path) -> Option<PathBuf> {
    // Drop "?query" and "#fragment", then undo %-escapes (CommonMark keeps them).
    let end = dest.find(['?', '#']).unwrap_or(dest.len());
    let decoded = percent_decode(&dest[..end])?;
    let p = Path::new(&decoded);
    Some(if p.is_absolute() { p.to_path_buf() } else { base_dir.join(p) })
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("md-core-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn renders_basic_markdown() {
        let h = render("# Title\n\nsome **bold** text\n\n- a\n- b\n", None);
        assert!(h.contains("<h1>Title</h1>"));
        assert!(h.contains("<strong>bold</strong>"));
        assert!(h.contains("<li>a</li>"));
    }

    #[test]
    fn renders_gfm_extensions() {
        let h = render("| a | b |\n|---|---|\n| 1 | 2 |\n\n~~gone~~\n\n- [x] done\n- [ ] todo\n", None);
        assert!(h.contains("<table>"));
        assert!(h.contains("<del>gone</del>"));
        assert!(h.contains("type=\"checkbox\""));
        assert!(h.contains("checked"));
    }

    #[test]
    fn strips_scripts_and_handlers() {
        let h = render(
            "<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>\n\n[x](javascript:alert(1))\n",
            None,
        );
        assert!(!h.contains("<script"));
        assert!(!h.contains("onerror"));
        assert!(!h.contains("javascript:"));
    }

    #[test]
    fn remote_image_becomes_link() {
        let h = render("![logo](https://example.com/a.png)", None);
        assert!(!h.contains("<img"));
        assert!(h.contains("<a href=\"https://example.com/a.png\""));
        assert!(h.contains("logo"));
    }

    #[test]
    fn local_image_is_inlined() {
        let d = tmp_dir("img");
        std::fs::write(d.join("my pic.png"), [0x89, b'P', b'N', b'G']).unwrap();
        let h = render("![pic](my%20pic.png)", Some(&d));
        assert!(h.contains("<img"));
        assert!(h.contains("src=\"data:image/png;base64,"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn non_image_and_missing_files_are_not_inlined() {
        let d = tmp_dir("secret");
        std::fs::write(d.join("secret.txt"), "hunter2").unwrap();
        let h = render("![a](secret.txt) ![b](missing.png)", Some(&d));
        assert!(!h.contains("data:"));
        assert!(!h.contains("<img"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn scheme_detection() {
        assert!(has_scheme("https://x"));
        assert!(has_scheme("file:///x"));
        assert!(!has_scheme("img/a.png"));
        assert!(!has_scheme("C:\\img\\a.png"));
    }
}
