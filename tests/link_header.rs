//! End-to-end use of the public API, as a consumer sees it.
//!
//! The unit tests reach into private helpers; these exercise only what is
//! exported, which is what catches an accidental change to the public surface.

use c2pa_http::{link, Error, ManifestLink};

const URI: &str = "https://fabrikam.example/m.c2pa";

#[test]
fn format_and_extract_round_trip() {
    let header = link::format(URI).unwrap();
    assert_eq!(
        header,
        r#"<https://fabrikam.example/m.c2pa>; rel="c2pa-manifest""#
    );

    let found = link::extract([header.as_str()]).unwrap();
    assert_eq!(found.uri, URI);
    assert!(!found.is_embedded());
}

#[test]
fn a_c2pa_link_is_found_among_unrelated_ones() {
    let header = r#"</s.css>; rel=preload, <https://fabrikam.example/m.c2pa>; rel="c2pa-manifest", </p2>; rel=next"#;
    assert_eq!(link::extract([header]).unwrap().uri, URI);
}

#[test]
fn several_header_fields_are_all_searched() {
    let found = link::extract(["</a>; rel=preload", r#"<m.c2pa>; rel="c2pa-manifest""#]).unwrap();
    assert_eq!(found.uri, "m.c2pa");
}

#[test]
fn a_jumbf_target_names_an_embedded_store_and_drops_the_childlabel() {
    let header = r#"<https://a.example/i.jpg#jumbf=c2pa/urn:uuid:1234/c2pa.assertions>; rel="c2pa-manifest""#;
    let found: ManifestLink = link::extract([header]).unwrap();
    assert!(found.is_embedded());
    assert_eq!(found.jumbf.as_deref(), Some("c2pa"));
    assert_eq!(found.uri, "https://a.example/i.jpg#jumbf=c2pa");
}

#[test]
fn a_hostile_target_is_neutralised_not_rejected() {
    // The payload is preserved inside the URI, but can no longer terminate the
    // header field and start one of its own.
    let header = link::format("https://a.example/\r\nX-Injected: yes").unwrap();
    assert!(!header.contains('\r') && !header.contains('\n'));
    assert!(header.contains("%0D%0A"));
    assert_eq!(header.matches('<').count(), 1);
    assert_eq!(link::locate_all([header.as_str()]).len(), 1);
}

#[test]
fn strict_formatting_reports_what_lenient_repairs() {
    for needs_repair in ["https://a.example/a b", "https://a.example/\r\n", "café"] {
        assert!(
            link::format_strict(needs_repair).is_err(),
            "{needs_repair:?}"
        );
        assert!(link::format(needs_repair).is_ok(), "{needs_repair:?}");
    }
    assert!(link::format_strict(URI).is_ok());
}

#[test]
fn encoding_is_idempotent_and_preserves_valid_uris() {
    assert_eq!(link::encode_target(URI), URI);
    let once = link::encode_target("a b");
    assert_eq!(link::encode_target(&once), once);
    // Delimiters a URI needs must survive, or query strings break.
    let complex = "https://user@a.example:8443/p?x=1&y=2#frag";
    assert_eq!(link::encode_target(complex), complex);
}

#[test]
fn no_link_is_unsigned_rather_than_failed() {
    let err = link::extract(["</s.css>; rel=preload"]).unwrap_err();
    assert_eq!(err, Error::NotFound);
    assert_eq!(err.code(), None);
    assert!(err.is_no_manifest_located());
}

#[test]
fn competing_targets_are_rejected_rather_than_guessed() {
    let header = r#"<a.c2pa>; rel="c2pa-manifest", <b.c2pa>; rel="c2pa-manifest""#;
    assert_eq!(link::extract([header]), Err(Error::MultipleLinks));
    assert_eq!(link::locate_all([header]).len(), 2);
}

#[test]
fn only_inaccessible_carries_a_status_code() {
    // Raised by whoever fetches the manifest, not by this crate.
    assert_eq!(Error::Inaccessible.code(), Some("manifest.inaccessible"));
    assert!(!Error::Inaccessible.is_no_manifest_located());
}

#[cfg(feature = "tower")]
mod middleware {
    use super::URI;
    use c2pa_http::{append_to, extract_from, Error, ManifestLinkLayer};
    use http::{HeaderMap, HeaderValue};

    #[test]
    fn append_preserves_unrelated_link_headers() {
        let mut headers = HeaderMap::new();
        headers.append(
            http::header::LINK,
            HeaderValue::from_static("</a>; rel=next"),
        );
        append_to(&mut headers, URI).unwrap();
        assert_eq!(headers.get_all(http::header::LINK).iter().count(), 2);
        assert_eq!(extract_from(&headers).unwrap().uri, URI);
    }

    #[test]
    fn a_layer_can_be_built_and_rejects_a_bad_configured_target() {
        assert!(ManifestLinkLayer::new(URI).is_ok());
        assert!(ManifestLinkLayer::new_strict(URI).is_ok());
        assert!(matches!(
            ManifestLinkLayer::new_strict("https://a.example/a b"),
            Err(Error::Malformed(_))
        ));
    }
}
