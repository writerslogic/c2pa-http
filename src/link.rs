// Copyright 2026 WritersLogic. All rights reserved.
// Licensed under the Apache License, Version 2.0 or the MIT license,
// at your option.

//! The `c2pa-manifest` link relation, parsed and serialised per {RFC 8288}.
//!
//! When an asset is retrieved over HTTP, a validator should look for a `Link`
//! header carrying `rel="c2pa-manifest"`; its target is where the C2PA Manifest
//! Store can be retrieved. The target may also name a Manifest Store *already
//! embedded* in the asset, through a JUMBF URI fragment.
//!
//! This module is dependency-free and operates on `&str`, so it works under any
//! HTTP stack. The Tower integration lives in [`crate::layer`].
//!
//! # Grammar
//!
//! ```text
//! Link       = #link-value
//! link-value = "<" URI-Reference ">" *( OWS ";" OWS link-param )
//! link-param = token BWS "=" BWS ( token / quoted-string )
//! ```
//!
//! Commas separate link-values and semicolons separate parameters, but both may
//! appear inside a `<target>` or a quoted string — a query string with `?a=1,2`
//! is entirely legal. Splitting naively on those characters is the classic way
//! to mis-parse this header, so the scanner here tracks both contexts.

use crate::error::Error;

/// The IANA-registered link relation naming a C2PA Manifest Store.
pub const REL: &str = "c2pa-manifest";

/// The JUMBF URI fragment prefix that names an embedded Manifest Store.
const JUMBF_PREFIX: &str = "jumbf=";

/// A located `c2pa-manifest` link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestLink {
    /// The target URI.
    ///
    /// When the target carried a JUMBF fragment, any `childlabel` portion has
    /// been removed: the specification permits referencing only the Manifest
    /// Store superbox, and requires a validator to ignore a deeper reference.
    pub uri: String,
    /// The JUMBF superbox label when the target names an *embedded* Manifest
    /// Store rather than a separate resource — normally `c2pa`.
    pub jumbf: Option<String>,
}

impl ManifestLink {
    /// Whether the target names a Manifest Store embedded in the asset itself,
    /// as opposed to a resource to be fetched.
    pub fn is_embedded(&self) -> bool {
        self.jumbf.is_some()
    }
}

/// Every `c2pa-manifest` link across the given `Link` header values, in order.
///
/// A response may carry several `Link` header fields, and each may carry
/// several comma-separated link-values; all are searched.
pub fn locate_all<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<ManifestLink> {
    let mut out = Vec::new();
    for value in values {
        for raw in split_unquoted(value, b',') {
            let Some((target, params)) = parse_link_value(raw) else {
                continue;
            };
            // RFC 8288: occurrences of `rel` after the first MUST be ignored.
            let Some(rel) = params.iter().find(|(k, _)| k == "rel").map(|(_, v)| v) else {
                continue;
            };
            if !rel
                .split(|c: char| c.is_ascii_whitespace())
                .any(|t| t.eq_ignore_ascii_case(REL))
            {
                continue;
            }
            let (uri, jumbf) = split_jumbf(target);
            out.push(ManifestLink { uri, jumbf });
        }
    }
    out
}

/// The single `c2pa-manifest` link advertised by a response.
///
/// Duplicate links naming the same target collapse to one; genuinely competing
/// targets are [`Error::MultipleLinks`], since the specification defines no
/// precedence between them.
pub fn extract<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<ManifestLink, Error> {
    let mut found = locate_all(values);
    found.dedup_by(|a, b| a == b);
    if found.len() > 1 {
        // Only distinct targets conflict; the same target repeated does not.
        let first = &found[0];
        if found.iter().any(|l| l != first) {
            return Err(Error::MultipleLinks);
        }
        found.truncate(1);
    }
    found.pop().ok_or(Error::NotFound)
}

/// Build a `Link` header value advertising `uri` as the C2PA Manifest Store.
///
/// The target is percent-encoded by [`encode_target`], so *any* input yields a
/// well-formed, injection-free header. Only an empty target is an error: there
/// is nothing to advertise.
///
/// Because encoding is applied, [`extract`] reads back the *encoded* form —
/// `a b` goes out as `a%20b` and comes back as `a%20b`. That is the URI, and it
/// is what a validator will fetch. Use [`format_strict`] when you would rather
/// be told that your input needed repairing.
pub fn format(uri: &str) -> Result<String, Error> {
    if uri.is_empty() {
        return Err(Error::Malformed("target URI is empty"));
    }
    Ok(std::format!("<{}>; rel=\"{REL}\"", encode_target(uri)))
}

/// As [`format`], but fails rather than repairing a target that is not already
/// a valid URI reference.
///
/// Useful where the target comes from configuration and silently rewriting it
/// would hide a mistake: a stray space in a deployment variable becomes `%20`
/// and a 404 at validation time, rather than an error at startup.
pub fn format_strict(uri: &str) -> Result<String, Error> {
    if uri.is_empty() {
        return Err(Error::Malformed("target URI is empty"));
    }
    if uri.as_bytes().iter().copied().any(must_encode) {
        return Err(Error::Malformed(
            "target URI contains characters that a URI must percent-encode",
        ));
    }
    Ok(std::format!("<{uri}>; rel=\"{REL}\""))
}

/// Whether a byte cannot appear literally in a URI reference.
///
/// {RFC 3986} excludes the control characters, space, and `" < > \ ^ ` { | }`
/// from a URI. Bytes at or above `0x7F` are excluded too: a URI is ASCII, and
/// non-ASCII text travels as percent-encoded UTF-8.
///
/// `%` is deliberately *not* in this set, so a URI that is already encoded is
/// not encoded a second time.
fn must_encode(b: u8) -> bool {
    b <= 0x20
        || b >= 0x7F
        || matches!(
            b,
            b'"' | b'<' | b'>' | b'\\' | b'^' | b'`' | b'{' | b'|' | b'}'
        )
}

/// Percent-encode the bytes that cannot appear literally in a URI reference.
///
/// A string carrying a raw CR, LF, space, or angle bracket is not a URI to be
/// rejected — it is a URI that has not been encoded yet, and encoding it is
/// what {RFC 3986} requires. That this also makes response-header injection
/// impossible is a consequence rather than a separate mechanism: a CR becomes
/// `%0D` and can no longer terminate the field, and a `>` becomes `%3E` and can
/// no longer close the target early.
///
/// Already-encoded input passes through unchanged, because `%` is left alone.
/// The delimiters a URI needs — `? # / : @ & = +` and the other sub-delims —
/// are all legal and preserved, so a query string or fragment survives intact.
pub fn encode_target(uri: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(uri.len());
    for &b in uri.as_bytes() {
        if must_encode(b) {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0F) as usize] as char);
        } else {
            // Every byte reaching here is printable ASCII.
            out.push(b as char);
        }
    }
    out
}

/// Split on `sep`, ignoring separators inside `<...>` or a quoted string.
fn split_unquoted(s: &str, sep: u8) -> Vec<&str> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let (mut start, mut i) = (0usize, 0usize);
    let (mut in_angle, mut in_quote) = (false, false);
    while i < b.len() {
        match b[i] {
            b'\\' if in_quote => i += 1, // the next byte is escaped
            b'"' => in_quote = !in_quote,
            b'<' if !in_quote => in_angle = true,
            b'>' if !in_quote => in_angle = false,
            c if c == sep && !in_quote && !in_angle => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&s[start..]);
    out
}

/// Parse one link-value into its target and parameters. Parameter names are
/// ASCII-lowercased; values are unquoted.
fn parse_link_value(value: &str) -> Option<(&str, Vec<(String, String)>)> {
    let value = value.trim();
    let open = value.find('<')?;
    let close = open + 1 + value[open + 1..].find('>')?;
    let target = value[open + 1..close].trim();
    if target.is_empty() {
        return None;
    }

    let mut params = Vec::new();
    for param in split_unquoted(&value[close + 1..], b';') {
        let param = param.trim();
        if param.is_empty() {
            continue;
        }
        match param.find('=') {
            Some(eq) => params.push((
                param[..eq].trim().to_ascii_lowercase(),
                unquote(param[eq + 1..].trim()),
            )),
            None => params.push((param.to_ascii_lowercase(), String::new())),
        }
    }
    Some((target, params))
}

/// Strip surrounding quotes and resolve backslash escapes.
fn unquote(s: &str) -> String {
    let Some(inner) = s
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .filter(|_| s.len() >= 2)
    else {
        return s.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            _ => out.push(c),
        }
    }
    out
}

/// Separate a JUMBF fragment from the target, discarding any childlabel.
fn split_jumbf(target: &str) -> (String, Option<String>) {
    let Some(hash) = target.find('#') else {
        return (target.to_string(), None);
    };
    let (base, fragment) = (&target[..hash], &target[hash + 1..]);
    if fragment.len() < JUMBF_PREFIX.len()
        || !fragment[..JUMBF_PREFIX.len()].eq_ignore_ascii_case(JUMBF_PREFIX)
    {
        return (target.to_string(), None);
    }
    // The Manifest Store superbox is the whole reference; anything after the
    // first `/` addresses a manifest inside it, which is not permitted.
    let store = fragment[JUMBF_PREFIX.len()..]
        .split('/')
        .next()
        .unwrap_or_default();
    if store.is_empty() {
        return (target.to_string(), None);
    }
    (
        std::format!("{base}#{JUMBF_PREFIX}{store}"),
        Some(store.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(header: &str) -> ManifestLink {
        extract([header]).expect("expected exactly one c2pa-manifest link")
    }

    #[test]
    fn parses_a_quoted_relation() {
        let l = one(r#"<https://a.example/m.c2pa>; rel="c2pa-manifest""#);
        assert_eq!(l.uri, "https://a.example/m.c2pa");
        assert_eq!(l.jumbf, None);
        assert!(!l.is_embedded());
    }

    #[test]
    fn parses_an_unquoted_relation() {
        assert_eq!(
            one("<https://a.example/m.c2pa>; rel=c2pa-manifest").uri,
            "https://a.example/m.c2pa"
        );
    }

    #[test]
    fn relation_matching_is_case_insensitive() {
        assert_eq!(
            one(r#"<m.c2pa>; REL="C2PA-Manifest""#).uri,
            "m.c2pa",
            "rel name and value are both case-insensitive"
        );
    }

    #[test]
    fn a_relation_token_list_containing_the_relation_matches() {
        assert_eq!(
            one(r#"<m.c2pa>; rel="preload c2pa-manifest""#).uri,
            "m.c2pa"
        );
    }

    #[test]
    fn a_near_miss_relation_is_not_a_match() {
        for header in [
            r#"<m.c2pa>; rel="c2pa-manifest-x""#,
            r#"<m.c2pa>; rel="x-c2pa-manifest""#,
            r#"<m.c2pa>; rel="stylesheet""#,
            "<m.c2pa>",
        ] {
            assert_eq!(extract([header]), Err(Error::NotFound), "{header}");
        }
    }

    #[test]
    fn picks_the_c2pa_link_out_of_a_multi_value_header() {
        let h = r#"</style.css>; rel=preload, <https://a.example/m.c2pa>; rel="c2pa-manifest", </next>; rel=next"#;
        assert_eq!(one(h).uri, "https://a.example/m.c2pa");
    }

    #[test]
    fn searches_across_several_header_fields() {
        let l = extract(["</a>; rel=preload", r#"<m.c2pa>; rel="c2pa-manifest""#]).unwrap();
        assert_eq!(l.uri, "m.c2pa");
    }

    #[test]
    fn a_comma_inside_the_target_does_not_split_the_value() {
        // A query string may legally contain commas.
        let h = r#"<https://a.example/m.c2pa?ids=1,2,3>; rel="c2pa-manifest""#;
        assert_eq!(one(h).uri, "https://a.example/m.c2pa?ids=1,2,3");
    }

    #[test]
    fn a_comma_or_semicolon_inside_a_quoted_param_does_not_split() {
        let h = r#"<m.c2pa>; title="a, b; c"; rel="c2pa-manifest""#;
        assert_eq!(one(h).uri, "m.c2pa");
    }

    #[test]
    fn an_escaped_quote_inside_a_param_is_handled() {
        let h = r#"<m.c2pa>; title="say \"hi\", ok"; rel="c2pa-manifest""#;
        assert_eq!(one(h).uri, "m.c2pa");
    }

    #[test]
    fn only_the_first_rel_parameter_counts() {
        // RFC 8288: later occurrences MUST be ignored.
        assert_eq!(
            one(r#"<m.c2pa>; rel="c2pa-manifest"; rel="next""#).uri,
            "m.c2pa"
        );
        assert_eq!(
            extract([r#"<m.c2pa>; rel="next"; rel="c2pa-manifest""#]),
            Err(Error::NotFound),
            "a later rel must not rescue a non-matching first one"
        );
    }

    #[test]
    fn a_jumbf_fragment_names_an_embedded_store() {
        let l = one(r#"<https://a.example/image.jpg#jumbf=c2pa>; rel="c2pa-manifest""#);
        assert_eq!(l.uri, "https://a.example/image.jpg#jumbf=c2pa");
        assert_eq!(l.jumbf.as_deref(), Some("c2pa"));
        assert!(l.is_embedded());
    }

    #[test]
    fn a_jumbf_childlabel_is_discarded() {
        // Referencing a specific manifest inside the store is not permitted, and
        // the validator shall ignore the childlabel portion.
        let l = one(
            r#"<https://a.example/i.jpg#jumbf=c2pa/urn:uuid:1234/c2pa.assertions>; rel="c2pa-manifest""#,
        );
        assert_eq!(l.uri, "https://a.example/i.jpg#jumbf=c2pa");
        assert_eq!(l.jumbf.as_deref(), Some("c2pa"));
    }

    #[test]
    fn a_non_jumbf_fragment_is_left_alone() {
        let l = one(r#"<https://a.example/m.c2pa#section>; rel="c2pa-manifest""#);
        assert_eq!(l.uri, "https://a.example/m.c2pa#section");
        assert_eq!(l.jumbf, None);
    }

    #[test]
    fn duplicate_identical_links_are_not_a_conflict() {
        let h = r#"<m.c2pa>; rel="c2pa-manifest", <m.c2pa>; rel="c2pa-manifest""#;
        assert_eq!(one(h).uri, "m.c2pa");
    }

    #[test]
    fn competing_targets_are_rejected() {
        // No precedence is defined between them, so choosing would invent a rule.
        let h = r#"<a.c2pa>; rel="c2pa-manifest", <b.c2pa>; rel="c2pa-manifest""#;
        assert_eq!(extract([h]), Err(Error::MultipleLinks));
        assert_eq!(locate_all([h]).len(), 2);
    }

    #[test]
    fn malformed_values_are_skipped_not_fatal() {
        // A neighbouring link that does not parse must not hide a good one.
        let h = r#"no-brackets; rel=whatever, <m.c2pa>; rel="c2pa-manifest""#;
        assert_eq!(one(h).uri, "m.c2pa");
        assert_eq!(
            extract(["<unterminated; rel=c2pa-manifest"]),
            Err(Error::NotFound)
        );
        assert_eq!(extract(["<>; rel=c2pa-manifest"]), Err(Error::NotFound));
        assert_eq!(extract([""]), Err(Error::NotFound));
    }

    #[test]
    fn whitespace_around_the_delimiters_is_tolerated() {
        let h = "  <m.c2pa>  ;  rel  =  c2pa-manifest  ";
        assert_eq!(one(h).uri, "m.c2pa");
    }

    #[test]
    fn format_round_trips_through_the_parser() {
        let header = format("https://a.example/m.c2pa").unwrap();
        assert_eq!(header, r#"<https://a.example/m.c2pa>; rel="c2pa-manifest""#);
        assert_eq!(one(&header).uri, "https://a.example/m.c2pa");
    }

    #[test]
    fn format_neutralises_header_injection_rather_than_refusing() {
        // Each of these would split the header or close the target early if it
        // reached the wire raw. Encoding makes them inert while keeping them.
        for hostile in [
            "https://a.example/\r\nX-Injected: yes",
            "https://a.example/\nX-Injected: yes",
            "https://a.example/\r",
            "https://a.example/m>; rel=\"evil\", <b",
            "https://a.example/\u{7}bell",
            "https://a.example/a b",
        ] {
            let header = format(hostile).expect("encoding must never reject");
            assert!(
                !header.contains('\r') && !header.contains('\n'),
                "a line break survived: {header:?}"
            );
            // Exactly one target, so nothing closed it early and started a
            // second link-value.
            assert_eq!(header.matches('<').count(), 1, "{header:?}");
            assert_eq!(header.matches('>').count(), 1, "{header:?}");
            // And it still parses back to exactly one link.
            assert_eq!(locate_all([header.as_str()]).len(), 1, "{header:?}");
        }
        assert!(matches!(format(""), Err(Error::Malformed(_))));
    }

    #[test]
    fn an_injected_header_name_becomes_part_of_the_uri() {
        // The payload is preserved, not silently dropped — it is simply no
        // longer a header of its own.
        let header = format("https://a.example/\r\nX-Injected: yes").unwrap();
        assert!(header.contains("%0D%0A"), "{header}");
        let found = extract([header.as_str()]).unwrap();
        assert_eq!(found.uri, "https://a.example/%0D%0AX-Injected:%20yes");
    }

    #[test]
    fn encoding_covers_exactly_the_characters_a_uri_excludes() {
        assert_eq!(encode_target("a b"), "a%20b");
        assert_eq!(encode_target("a\r\nb"), "a%0D%0Ab");
        assert_eq!(encode_target("a<b>c"), "a%3Cb%3Ec");
        assert_eq!(
            encode_target("a\"b\\c^d`e{f|g}h"),
            "a%22b%5Cc%5Ed%60e%7Bf%7Cg%7Dh"
        );
        assert_eq!(encode_target("a\u{7F}b"), "a%7Fb");
        // Non-ASCII travels as percent-encoded UTF-8.
        assert_eq!(encode_target("café"), "caf%C3%A9");
    }

    #[test]
    fn encoding_preserves_a_uri_that_is_already_correct() {
        // Every delimiter a URI needs must survive untouched, or a query string
        // or fragment would be corrupted.
        for good in [
            "https://a.example/m.c2pa",
            "https://user@a.example:8443/p/q?x=1&y=2#frag",
            "https://a.example/i.jpg#jumbf=c2pa",
            "https://a.example/a~b_c-d.e!$&'()*+,;=:@/f",
        ] {
            assert_eq!(encode_target(good), good, "mangled a valid URI");
        }
    }

    #[test]
    fn encoding_is_idempotent() {
        // `%` is left alone, so an already-encoded target is not double-encoded
        // into `%2520`.
        let once = encode_target("a b");
        assert_eq!(encode_target(&once), once);
        assert_eq!(encode_target("%20"), "%20");
    }

    #[test]
    fn format_strict_reports_what_format_would_have_repaired() {
        assert!(format_strict("https://a.example/m.c2pa").is_ok());
        for needs_repair in ["https://a.example/a b", "https://a.example/\r\n", "café"] {
            assert!(
                matches!(format_strict(needs_repair), Err(Error::Malformed(_))),
                "strict mode accepted {needs_repair:?}"
            );
            // What strict rejects, lenient repairs.
            assert!(format(needs_repair).is_ok());
        }
        assert!(matches!(format_strict(""), Err(Error::Malformed(_))));
    }

    #[test]
    fn format_accepts_a_jumbf_target() {
        let header = format("https://a.example/i.jpg#jumbf=c2pa").unwrap();
        assert!(one(&header).is_embedded());
    }

    #[test]
    fn the_scanner_terminates_on_adversarial_input() {
        // Unbalanced delimiters must not loop or panic.
        for h in [
            "<<<<",
            "\"\"\"",
            "<a\"b>; rel=c2pa-manifest",
            ";;;;",
            ",,,,",
            "<a>;rel=",
            "\\",
            "<a>; rel=\"unterminated",
        ] {
            let _ = locate_all([h]);
        }
    }

    #[test]
    fn multibyte_targets_do_not_split_a_character() {
        let h = "<https://a.example/café/münchen.c2pa>; rel=c2pa-manifest";
        assert_eq!(one(h).uri, "https://a.example/café/münchen.c2pa");
    }
}
