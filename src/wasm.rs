// Copyright 2026 WritersLogic. All rights reserved.
// Licensed under the Apache License, Version 2.0 or the MIT license,
// at your option.

//! WebAssembly bindings, built with `wasm-pack` and published to npm as
//! `c2pa-http`.
//!
//! Only the [`link`](crate::link) parser is exposed; the Tower layer has no
//! meaning off a Rust service stack. In JavaScript this is the piece that
//! matters — an Express or Fastify handler, a Cloudflare Worker, or a `fetch`
//! caller all deal in raw header strings:
//!
//! ```js
//! import { format, extract } from "c2pa-http";
//!
//! res.setHeader("Link", format("https://a.example/m.c2pa"));
//!
//! const found = extract(response.headers.getSetCookie ? [] : [response.headers.get("link")]);
//! if (found) console.log(found.uri, found.isEmbedded);
//! ```

use wasm_bindgen::prelude::*;

use crate::link;

fn to_js(l: &link::ManifestLink) -> JsValue {
    let out = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&out, &"uri".into(), &l.uri.as_str().into());
    let _ = js_sys::Reflect::set(
        &out,
        &"jumbf".into(),
        &match &l.jumbf {
            Some(j) => j.as_str().into(),
            None => JsValue::NULL,
        },
    );
    let _ = js_sys::Reflect::set(&out, &"isEmbedded".into(), &l.is_embedded().into());
    out.into()
}

/// Build a `Link` header value advertising `uri` as the C2PA Manifest Store.
///
/// The target is percent-encoded, so any input yields a safe header: a CR/LF
/// becomes `%0D%0A` and can no longer start a header of its own. Only an empty
/// target throws.
#[wasm_bindgen]
pub fn format(uri: &str) -> Result<String, JsError> {
    link::format(uri).map_err(|e| JsError::new(&e.to_string()))
}

/// As [`format`], but throws rather than repairing a target that is not already
/// a valid URI reference.
#[wasm_bindgen(js_name = formatStrict)]
pub fn format_strict(uri: &str) -> Result<String, JsError> {
    link::format_strict(uri).map_err(|e| JsError::new(&e.to_string()))
}

/// Percent-encode the bytes that cannot appear literally in a URI reference.
///
/// Idempotent: `%` is left alone, so an already-encoded URI is not
/// double-encoded.
#[wasm_bindgen(js_name = encodeTarget)]
pub fn encode_target(uri: &str) -> String {
    link::encode_target(uri)
}

/// The single `c2pa-manifest` link across the given `Link` header values, or
/// `null` when none is advertised.
///
/// Throws when genuinely competing targets are advertised.
#[wasm_bindgen]
pub fn extract(values: Vec<String>) -> Result<JsValue, JsError> {
    match link::extract(values.iter().map(String::as_str)) {
        Ok(found) => Ok(to_js(&found)),
        // Advertising nothing is not a failure.
        Err(crate::Error::NotFound) => Ok(JsValue::NULL),
        Err(e) => Err(JsError::new(&e.to_string())),
    }
}

/// Every `c2pa-manifest` link across the given `Link` header values, in order.
#[wasm_bindgen(js_name = locateAll)]
pub fn locate_all(values: Vec<String>) -> Vec<JsValue> {
    link::locate_all(values.iter().map(String::as_str))
        .iter()
        .map(to_js)
        .collect()
}

/// The IANA-registered link relation naming a C2PA Manifest Store.
#[wasm_bindgen]
pub fn rel() -> String {
    link::REL.to_string()
}
