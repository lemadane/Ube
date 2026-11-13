//! USSR's deliberately bounded HTML grammar. Literal markup is trusted source;
//! interpolation positions are classified before expressions are lowered.
use super::{Expr, ExprKind};

pub(super) fn validate(parts: Vec<Expr>, literals: Vec<bool>) -> Result<Vec<(Expr, u8)>, String> {
    let mut source = String::new();
    for (part, literal) in parts.iter().zip(&literals) {
        if *literal {
            let ExprKind::TextBlock(text) = &part.kind else {
                unreachable!()
            };
            if text.contains('\0') {
                return Err("NUL is not allowed in HTML templates".into());
            }
            source.push_str(text);
        } else {
            source.push('\0');
        }
    }
    let contexts = classify(&source)?;
    let mut contexts = contexts.into_iter();
    Ok(parts
        .into_iter()
        .zip(literals)
        .map(|(part, literal)| (part, if literal { 0 } else { contexts.next().unwrap() }))
        .collect())
}

fn classify(source: &str) -> Result<Vec<u8>, String> {
    let bytes = source.as_bytes();
    let lower = source.to_ascii_lowercase();
    let mut i = 0;
    let mut stack: Vec<String> = Vec::new();
    let mut contexts = Vec::new();
    while i < bytes.len() {
        if stack
            .last()
            .is_some_and(|tag| matches!(tag.as_str(), "script" | "style" | "textarea" | "title"))
        {
            let close = format!("</{}", stack.last().unwrap());
            let end = lower[i..]
                .find(&close)
                .map(|n| n + i)
                .ok_or("Unclosed HTML raw-text element")?;
            if source[i..end].contains('\0') {
                return Err(
                    "Interpolation in script, style, textarea, or title is not supported".into(),
                );
            }
            i = end;
        }
        if bytes[i] == 0 {
            contexts.push(1);
            i += 1;
            continue;
        }
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        if source[i..].starts_with("<!--") {
            let end = source[i + 4..]
                .find("-->")
                .map(|n| i + 4 + n)
                .ok_or("Unclosed HTML comment")?;
            if source[i..end].contains('\0')
                || source[i + 4..end].contains("--")
                || source[i + 4..end].starts_with('>')
            {
                return Err("HTML comments must be static and well formed".into());
            }
            i = end + 3;
            continue;
        }
        if lower[i..].starts_with("<!doctype html>") {
            i += 15;
            continue;
        }
        i += 1;
        let closing = bytes.get(i) == Some(&b'/');
        if closing {
            i += 1;
        }
        let start = i;
        while bytes.get(i).is_some_and(|c| c.is_ascii_alphanumeric()) {
            i += 1;
        }
        let tag = lower[start..i].to_owned();
        if !TAGS.contains(&tag.as_str()) {
            return Err(format!(
                "Unsupported HTML tag '{tag}'; tag names must be static"
            ));
        }
        let mut attributes = std::collections::HashSet::new();
        loop {
            let before_space = i;
            while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
            }
            if bytes.get(i) == Some(&b'>') {
                i += 1;
                break;
            }
            if bytes.get(i..i + 2) == Some(b"/>") {
                if !VOID.contains(&tag.as_str()) {
                    return Err("Only void HTML elements may use />".into());
                }
                i += 2;
                break;
            }
            if closing || i == before_space {
                return Err("Expected a static HTML attribute or >".into());
            }
            let start = i;
            while bytes
                .get(i)
                .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b':'))
            {
                i += 1;
            }
            if start == i {
                return Err("HTML attribute names must be static".into());
            }
            let attr = &lower[start..i];
            if !attributes.insert(attr) {
                return Err(format!("Duplicate HTML attribute '{attr}'"));
            }
            let attr_end = i;
            while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
            }
            if bytes.get(i) != Some(&b'=') {
                i = attr_end;
                continue;
            }
            i += 1;
            while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
            }
            let quote = *bytes.get(i).ok_or("Unclosed HTML tag")?;
            if quote != b'\'' && quote != b'"' {
                return Err("HTML attribute values must be quoted".into());
            }
            i += 1;
            let start = i;
            while bytes.get(i).is_some_and(|c| *c != quote) {
                i += 1;
            }
            if i == bytes.len() {
                return Err("Unclosed HTML attribute".into());
            }
            let value = &source[start..i];
            if value.contains('\0') {
                // HTMX interprets data-hx-* exactly like hx-*. Do not let
                // executable attributes or URLs bypass their context checks
                // through the general data-* allowance.
                let checked_attr = if attr.starts_with("data-hx-") {
                    &attr[5..]
                } else {
                    attr
                };
                let url = URLS.contains(&checked_attr);
                if url && value != "\0" {
                    return Err("A dynamic URL must occupy the entire quoted attribute; build the URL inside ${...}".into());
                }
                if !url
                    && !(ATTRS.contains(&checked_attr)
                        || checked_attr.starts_with("aria-")
                        || checked_attr.starts_with("data-"))
                {
                    return Err(format!(
                        "Interpolation in HTML attribute '{attr}' is not supported"
                    ));
                }
                contexts.extend(
                    value
                        .bytes()
                        .filter(|c| *c == 0)
                        .map(|_| if url { 3 } else { 2 }),
                );
            }
            i += 1;
        }
        if closing {
            if stack.pop().as_deref() != Some(&tag) {
                return Err(format!("Mismatched HTML closing tag '{tag}'"));
            }
        } else if !VOID.contains(&tag.as_str()) {
            stack.push(tag);
        }
    }
    if !stack.is_empty() {
        return Err("HTML templates must contain balanced elements".into());
    }
    Ok(contexts)
}

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];
const TAGS: &[&str] = &[
    "html",
    "head",
    "body",
    "title",
    "meta",
    "link",
    "script",
    "style",
    "main",
    "header",
    "footer",
    "nav",
    "section",
    "article",
    "aside",
    "div",
    "span",
    "p",
    "a",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "dl",
    "dt",
    "dd",
    "form",
    "label",
    "input",
    "button",
    "select",
    "option",
    "optgroup",
    "textarea",
    "fieldset",
    "legend",
    "table",
    "caption",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "th",
    "td",
    "col",
    "colgroup",
    "img",
    "picture",
    "source",
    "video",
    "audio",
    "track",
    "figure",
    "figcaption",
    "br",
    "hr",
    "wbr",
    "strong",
    "em",
    "b",
    "i",
    "u",
    "s",
    "small",
    "code",
    "pre",
    "blockquote",
    "q",
    "time",
    "details",
    "summary",
    "dialog",
    "progress",
    "meter",
    "output",
    "address",
    "abbr",
    "mark",
    "sub",
    "sup",
    "del",
    "ins",
];
const URLS: &[&str] = &[
    "href",
    "src",
    "action",
    "formaction",
    "poster",
    "cite",
    "hx-get",
    "hx-post",
    "hx-put",
    "hx-patch",
    "hx-delete",
];
const ATTRS: &[&str] = &[
    "id",
    "class",
    "title",
    "name",
    "value",
    "alt",
    "placeholder",
    "role",
    "for",
    "type",
    "width",
    "height",
    "min",
    "max",
    "step",
    "rows",
    "cols",
    "colspan",
    "rowspan",
    "maxlength",
    "minlength",
    "pattern",
    "autocomplete",
    "lang",
    "dir",
    "datetime",
    "tabindex",
    "hx-target",
    "hx-swap",
    "hx-select",
];
