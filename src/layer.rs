// Copyright 2026 WritersLogic. All rights reserved.
// Licensed under the Apache License, Version 2.0 or the MIT license,
// at your option.

//! A Tower [`Layer`] that advertises a C2PA Manifest Store on every response.
//!
//! The header is *appended*, never set: a response may already carry `Link`
//! fields for preload, canonical, or pagination hints, and replacing them would
//! break unrelated behaviour.
//!
//! # Scope
//!
//! This attaches and reads a header. It deliberately does not inspect request
//! or response *bodies* to detect embedded provenance: doing so means buffering
//! the entire body before it can be forwarded, which turns a streaming proxy
//! into an unbounded memory sink and hands any client a denial of service. A
//! body-inspecting middleware needs a mandatory size cap and a considered
//! failure mode, and belongs behind its own explicit opt-in rather than in the
//! layer that writes a header.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use http::header::HeaderValue;
use http::{HeaderMap, Response};
use tower_layer::Layer;
use tower_service::Service;

use crate::error::Error;
use crate::link::{self, ManifestLink};

/// The `Link` header name.
pub const LINK: http::header::HeaderName = http::header::LINK;

/// Append a `c2pa-manifest` link to a header map.
///
/// Appends rather than replaces, so existing `Link` fields survive. The target
/// is percent-encoded by [`link::encode_target`], so a hostile URI is rendered
/// inert rather than rejected — a CR/LF ends up inside the URI as `%0D%0A`
/// instead of starting a header of its own.
pub fn append_to(headers: &mut HeaderMap, uri: &str) -> Result<(), Error> {
    let value = link::format(uri)?;
    let header = HeaderValue::from_str(&value)
        .map_err(|_| Error::Malformed("target URI is not a valid header value"))?;
    headers.append(LINK, header);
    Ok(())
}

/// The `c2pa-manifest` link advertised by a header map, if exactly one is.
///
/// Header values that are not valid UTF-8 are skipped rather than failing the
/// lookup: a malformed unrelated `Link` field must not hide a good one.
pub fn extract_from(headers: &HeaderMap) -> Result<ManifestLink, Error> {
    link::extract(headers.get_all(LINK).iter().filter_map(|v| v.to_str().ok()))
}

/// A [`Layer`] that appends a `c2pa-manifest` link to every response.
///
/// The target is fixed for the life of the layer. For a target that varies per
/// request, call [`append_to`] from your own middleware instead.
#[derive(Debug, Clone)]
pub struct ManifestLinkLayer {
    header: HeaderValue,
}

impl ManifestLinkLayer {
    /// Build a layer advertising `uri`.
    ///
    /// The header value is rendered once, here, rather than per response. The
    /// target is percent-encoded, so any input produces a safe header.
    pub fn new(uri: &str) -> Result<Self, Error> {
        Self::from_value(link::format(uri)?)
    }

    /// As [`new`](Self::new), but fails if `uri` is not already a valid URI
    /// reference instead of repairing it.
    ///
    /// Prefer this when the target comes from configuration: a stray space in a
    /// deployment variable becomes a startup error rather than a silent `%20`
    /// and a 404 at validation time.
    pub fn new_strict(uri: &str) -> Result<Self, Error> {
        Self::from_value(link::format_strict(uri)?)
    }

    fn from_value(value: String) -> Result<Self, Error> {
        let header = HeaderValue::from_str(&value)
            .map_err(|_| Error::Malformed("target URI is not a valid header value"))?;
        Ok(Self { header })
    }
}

impl<S> Layer<S> for ManifestLinkLayer {
    type Service = ManifestLinkService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ManifestLinkService {
            inner,
            header: self.header.clone(),
        }
    }
}

/// The [`Service`] produced by [`ManifestLinkLayer`].
#[derive(Debug, Clone)]
pub struct ManifestLinkService<S> {
    inner: S,
    header: HeaderValue,
}

impl<S, Request, B> Service<Request> for ManifestLinkService<S>
where
    S: Service<Request, Response = Response<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        ResponseFuture {
            inner: self.inner.call(request),
            header: self.header.clone(),
        }
    }
}

pin_project_lite::pin_project! {
    /// Appends the header once the inner service resolves.
    #[derive(Debug)]
    pub struct ResponseFuture<F> {
        #[pin]
        inner: F,
        header: HeaderValue,
    }
}

impl<F, B, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<B>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut response = match this.inner.poll(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Ready(Ok(r)) => r,
        };
        response.headers_mut().append(LINK, this.header.clone());
        Poll::Ready(Ok(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::{ServiceBuilder, ServiceExt};

    const URI: &str = "https://a.example/m.c2pa";

    async fn ok(_: http::Request<()>) -> Result<Response<()>, std::convert::Infallible> {
        Ok(Response::new(()))
    }

    async fn with_existing_link(
        _: http::Request<()>,
    ) -> Result<Response<()>, std::convert::Infallible> {
        let mut r = Response::new(());
        r.headers_mut()
            .append(LINK, HeaderValue::from_static("</s.css>; rel=preload"));
        Ok(r)
    }

    #[tokio::test]
    async fn the_layer_advertises_the_manifest() {
        let svc = ServiceBuilder::new()
            .layer(ManifestLinkLayer::new(URI).unwrap())
            .service_fn(ok);
        let response = svc.oneshot(http::Request::new(())).await.unwrap();
        assert_eq!(extract_from(response.headers()).unwrap().uri, URI);
    }

    #[tokio::test]
    async fn an_existing_link_header_survives() {
        // Replacing rather than appending would silently drop the preload hint.
        let svc = ServiceBuilder::new()
            .layer(ManifestLinkLayer::new(URI).unwrap())
            .service_fn(with_existing_link);
        let response = svc.oneshot(http::Request::new(())).await.unwrap();
        assert_eq!(response.headers().get_all(LINK).iter().count(), 2);
        assert_eq!(extract_from(response.headers()).unwrap().uri, URI);
    }

    #[test]
    fn append_and_extract_round_trip() {
        let mut headers = HeaderMap::new();
        assert_eq!(extract_from(&headers), Err(Error::NotFound));
        append_to(&mut headers, URI).unwrap();
        assert_eq!(extract_from(&headers).unwrap().uri, URI);
    }

    #[test]
    fn append_preserves_unrelated_links() {
        let mut headers = HeaderMap::new();
        headers.append(LINK, HeaderValue::from_static("</a>; rel=next"));
        append_to(&mut headers, URI).unwrap();
        assert_eq!(headers.get_all(LINK).iter().count(), 2);
        assert_eq!(extract_from(&headers).unwrap().uri, URI);
    }

    #[test]
    fn a_hostile_target_cannot_inject_a_header() {
        // The CR/LF is percent-encoded, so the payload lands inside the URI
        // rather than becoming a header of its own.
        let mut headers = HeaderMap::new();
        append_to(&mut headers, "https://a.example/\r\nX-Evil: 1").unwrap();
        assert_eq!(headers.len(), 1, "a second header was injected");
        assert!(headers.get("x-evil").is_none());
        assert!(extract_from(&headers).unwrap().uri.contains("%0D%0A"));
    }

    #[tokio::test]
    async fn a_hostile_target_stays_inert_through_the_layer() {
        let svc = ServiceBuilder::new()
            .layer(ManifestLinkLayer::new("https://a.example/\r\nX-Evil: 1").unwrap())
            .service_fn(ok);
        let response = svc.oneshot(http::Request::new(())).await.unwrap();
        assert_eq!(response.headers().len(), 1);
        assert!(response.headers().get("x-evil").is_none());
    }

    #[test]
    fn strict_construction_rejects_what_lenient_repairs() {
        // For a target from configuration, a stray space should be a startup
        // error rather than a silent %20 and a 404 much later.
        assert!(ManifestLinkLayer::new_strict("https://a.example/a b").is_err());
        assert!(ManifestLinkLayer::new("https://a.example/a b").is_ok());
        assert!(ManifestLinkLayer::new_strict("https://a.example/m.c2pa").is_ok());
    }

    #[test]
    fn a_jumbf_target_survives_the_header_round_trip() {
        let mut headers = HeaderMap::new();
        append_to(&mut headers, "https://a.example/i.jpg#jumbf=c2pa").unwrap();
        let found = extract_from(&headers).unwrap();
        assert!(found.is_embedded());
        assert_eq!(found.jumbf.as_deref(), Some("c2pa"));
    }

    #[test]
    fn competing_targets_across_two_fields_are_rejected() {
        let mut headers = HeaderMap::new();
        append_to(&mut headers, "https://a.example/a.c2pa").unwrap();
        append_to(&mut headers, "https://a.example/b.c2pa").unwrap();
        assert_eq!(extract_from(&headers), Err(Error::MultipleLinks));
    }

    #[test]
    fn a_non_utf8_header_value_does_not_hide_a_good_one() {
        let mut headers = HeaderMap::new();
        headers.append(
            LINK,
            HeaderValue::from_bytes(b"</\xFF\xFE>; rel=next").unwrap(),
        );
        append_to(&mut headers, URI).unwrap();
        assert_eq!(extract_from(&headers).unwrap().uri, URI);
    }
}
