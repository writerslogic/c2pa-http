// Copyright 2026 WritersLogic. All rights reserved.
// Licensed under the Apache License, Version 2.0 or the MIT license,
// at your option.

//! C2PA manifest discovery over HTTP: the `c2pa-manifest` `Link` header.
//!
//! When an asset is served over HTTP, a validator that finds no embedded
//! Manifest Store should look for a `Link` header carrying
//! `rel="c2pa-manifest"`. This crate parses and emits that header, and provides
//! a [Tower] middleware that attaches it to every response.
//!
//! # Scope
//!
//! - **[`link`]** — parse and serialise the header per {RFC 8288}. No
//!   dependencies, operates on `&str`, usable under any HTTP stack.
//! - **[`layer`]** — the Tower [`Layer`](tower_layer::Layer), behind the
//!   default `tower` feature.
//!
//! Retrieving the manifest is left to the caller: this crate performs no
//! network I/O, so it makes no decisions about timeouts, redirects, or trust.
//! [`Error::Inaccessible`] exists so a caller that does fetch can report
//! `manifest.inaccessible` through the same error type.
//!
//! # Examples
//!
//! Advertise a manifest on every response:
//!
//! ```
//! # #[cfg(feature = "tower")] {
//! use c2pa_http::ManifestLinkLayer;
//! use tower::ServiceBuilder;
//!
//! let layer = ManifestLinkLayer::new("https://fabrikam.example/m.c2pa").unwrap();
//! let _service = ServiceBuilder::new().layer(layer);
//! # }
//! ```
//!
//! Read one from a response you received:
//!
//! ```
//! use c2pa_http::link;
//!
//! let header = r#"</style.css>; rel=preload, <https://a.example/m.c2pa>; rel="c2pa-manifest""#;
//! let found = link::extract([header]).unwrap();
//! assert_eq!(found.uri, "https://a.example/m.c2pa");
//! ```
//!
//! A target may also name a Manifest Store already embedded in the asset,
//! through a JUMBF fragment. A reference to a specific manifest *inside* the
//! store is not permitted, and the `childlabel` portion is discarded:
//!
//! ```
//! use c2pa_http::link;
//!
//! let header = r#"<https://a.example/i.jpg#jumbf=c2pa/urn:uuid:1234>; rel="c2pa-manifest""#;
//! let found = link::extract([header]).unwrap();
//! assert!(found.is_embedded());
//! assert_eq!(found.uri, "https://a.example/i.jpg#jumbf=c2pa");
//! ```
//!
//! # Precedence
//!
//! For an HTML document that carries both an in-document manifest element and a
//! `Link` header, the embedded Manifest Store takes precedence and the remote
//! link is ignored. Discovery inside the document itself is [`c2pa-html`]'s
//! job.
//!
//! [Tower]: https://crates.io/crates/tower
//! [`c2pa-html`]: https://crates.io/crates/c2pa-html

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(missing_debug_implementations)]

/// Errors from locating a manifest link, and the C2PA status codes they map to.
pub mod error;
pub mod link;

#[cfg(feature = "tower")]
pub mod layer;

#[cfg(all(feature = "python", not(target_arch = "wasm32")))]
mod python;

#[cfg(target_arch = "wasm32")]
mod wasm;

pub use error::Error;
pub use link::{ManifestLink, REL};

#[cfg(feature = "tower")]
pub use layer::{append_to, extract_from, ManifestLinkLayer, ManifestLinkService};
