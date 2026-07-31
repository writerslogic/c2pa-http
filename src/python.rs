//! Python bindings, built with [maturin]/[PyO3] behind the `python` feature and
//! published to PyPI as `c2pa-http`.
//!
//! The Tower [`Layer`](crate::layer::ManifestLinkLayer) is *not* bound: a
//! `Layer` composes into a Rust service stack and has no meaning outside one.
//! What is bound is the [`link`](crate::link) parser, which is what a Python
//! web application needs — Django, Flask, and FastAPI all hand you the raw
//! `Link` header and expect you to emit your own.
//!
//! ```python
//! import c2pa_http
//!
//! # Serving: attach the header.
//! response["Link"] = c2pa_http.format("https://a.example/m.c2pa")
//!
//! # Consuming: read it back, tolerating unrelated Link values.
//! found = c2pa_http.extract(response.headers.get_list("Link"))
//! ```
//!
//! [maturin]: https://www.maturin.rs/
//! [PyO3]: https://pyo3.rs/

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::link::{self, ManifestLink};

/// Map a crate error to `ValueError`, naming the C2PA status code when the
/// specification defines one so a caller can branch on it.
fn map_err(e: crate::Error) -> PyErr {
    match e.code() {
        Some(code) => PyValueError::new_err(format!("{e} [{code}]")),
        None => PyValueError::new_err(e.to_string()),
    }
}

fn to_dict<'py>(py: Python<'py>, l: &ManifestLink) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("uri", l.uri.as_str())?;
    out.set_item("jumbf", l.jumbf.as_deref())?;
    out.set_item("is_embedded", l.is_embedded())?;
    Ok(out)
}

/// Build a `Link` header value advertising `uri` as the C2PA Manifest Store.
///
/// The target is percent-encoded, so any input yields a safe header: a CR/LF
/// becomes `%0D%0A` and can no longer start a header of its own. Only an empty
/// target raises.
#[pyfunction]
fn format(uri: &str) -> PyResult<String> {
    link::format(uri).map_err(map_err)
}

/// As `format`, but raises rather than repairing a target that is not already a
/// valid URI reference.
///
/// Use this when the target comes from configuration, where a silent `%20`
/// would hide a typo until it surfaced as a 404.
#[pyfunction]
fn format_strict(uri: &str) -> PyResult<String> {
    link::format_strict(uri).map_err(map_err)
}

/// Percent-encode the bytes that cannot appear literally in a URI reference.
///
/// Idempotent: `%` is left alone, so an already-encoded URI is not
/// double-encoded.
#[pyfunction]
fn encode_target(uri: &str) -> String {
    link::encode_target(uri)
}

/// The single `c2pa-manifest` link across the given `Link` header values, or
/// `None` when none is advertised.
///
/// Accepts a list of header values, since a response may carry several `Link`
/// fields and each may hold several comma-separated links. Returns a dict with
/// `uri`, `jumbf`, and `is_embedded`.
///
/// Raises `ValueError` when genuinely competing targets are advertised: the
/// specification defines no precedence between them, so choosing one would
/// invent a rule.
#[pyfunction]
fn extract<'py>(py: Python<'py>, values: Vec<String>) -> PyResult<Option<Bound<'py, PyDict>>> {
    match link::extract(values.iter().map(String::as_str)) {
        Ok(found) => Ok(Some(to_dict(py, &found)?)),
        // Advertising nothing is not a failure.
        Err(crate::Error::NotFound) => Ok(None),
        Err(e) => Err(map_err(e)),
    }
}

/// Every `c2pa-manifest` link across the given `Link` header values, in order.
#[pyfunction]
fn locate_all<'py>(py: Python<'py>, values: Vec<String>) -> PyResult<Vec<Bound<'py, PyDict>>> {
    link::locate_all(values.iter().map(String::as_str))
        .iter()
        .map(|l| to_dict(py, l))
        .collect()
}

#[pymodule]
fn c2pa_http(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(format, m)?)?;
    m.add_function(wrap_pyfunction!(format_strict, m)?)?;
    m.add_function(wrap_pyfunction!(encode_target, m)?)?;
    m.add_function(wrap_pyfunction!(extract, m)?)?;
    m.add_function(wrap_pyfunction!(locate_all, m)?)?;
    m.add("REL", crate::link::REL)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
