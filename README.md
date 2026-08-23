# c2pa-http

_C2PA manifest discovery over HTTP: the `c2pa-manifest` Link header (RFC 8288), with a Tower middleware._

<p align="center">
  <a href="https://crates.io/crates/c2pa-http"><img src="https://img.shields.io/crates/v/c2pa-http.svg" alt="crates.io"></a>
  <a href="https://docs.rs/c2pa-http"><img src="https://docs.rs/c2pa-http/badge.svg" alt="docs.rs"></a>
  <a href="https://github.com/writerslogic/c2pa-http/actions/workflows/ci.yml"><img src="https://github.com/writerslogic/c2pa-http/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://scorecard.dev/viewer/?uri=github.com/writerslogic/c2pa-http"><img src="https://api.securityscorecards.dev/projects/github.com/writerslogic/c2pa-http/badge" alt="OpenSSF Scorecard"></a>
  <a href="#license"><img src="https://img.shields.io/crates/l/c2pa-http.svg" alt="License"></a>
</p>

## Overview

Implements C2PA manifest discovery over HTTP: the `c2pa-manifest` `Link` header, per [RFC 8288](https://www.rfc-editor.org/rfc/rfc8288), with a [Tower](https://crates.io/crates/tower) middleware.

When an asset is served over HTTP and carries no embedded Manifest Store, a validator should look for a `Link` header carrying `rel="c2pa-manifest"`. Its target is where the C2PA Manifest Store can be retrieved.

```http
Link: <https://fabrikam.example/m.c2pa>; rel="c2pa-manifest"
```

```toml
[dependencies]
c2pa-http = "0.1"
```

The same crate is published for JavaScript/WebAssembly and Python, built from this source:

```bash
npm install c2pa-http   # wasm-bindgen build
pip install c2pa-http   # PyO3 abi3 wheel, CPython 3.9+
```

This is the one discovery method that is not a file format, so it composes with
all the others: a document embedding its manifest via
[`c2pa-html`](https://crates.io/crates/c2pa-html) can *also* advertise one over
HTTP, and the specification gives the header precedence.

This crate owns two things:

1. **The header** — parse and serialise it, with no dependencies, under any HTTP stack.
2. **The middleware** — a Tower `Layer` that attaches it to every response.

Retrieving the manifest is left to the caller: the crate performs no network
I/O, so it makes no decisions about timeouts, redirects, or trust.

> [!NOTE]
> Not certified or conformance-tested by the C2PA. It implements the discovery method as specified.

## What it does

| | |
|---|---|
| `link` | parse and serialise the header; no dependencies, `&str` in and out |
| `layer` | the Tower `Layer` that attaches it to every response |

Retrieving the manifest is left to you: the crate performs no network I/O, so it
makes no decisions about timeouts, redirects, or trust. `Error::Inaccessible`
exists so a caller that does fetch can report `manifest.inaccessible` through
the same type.

## Serve it

```rust
use c2pa_http::ManifestLinkLayer;
use tower::ServiceBuilder;

let layer = ManifestLinkLayer::new("https://fabrikam.example/m.c2pa")?;
let service = ServiceBuilder::new().layer(layer);
# Ok::<(), c2pa_http::Error>(())
```

The header is **appended, never set**. A response may already carry `Link`
fields for preload, canonical, or pagination hints; replacing them would break
unrelated behaviour.

## Read it

```rust
use c2pa_http::link;

let header = r#"</style.css>; rel=preload, <https://a.example/m.c2pa>; rel="c2pa-manifest""#;
let found = link::extract([header])?;
assert_eq!(found.uri, "https://a.example/m.c2pa");
# Ok::<(), c2pa_http::Error>(())
```

## Embedded manifests, and the childlabel rule

A target may name a Manifest Store *already embedded* in the asset, via a JUMBF
URI fragment. Referencing a specific manifest inside the store is not permitted,
and a validator must ignore the `childlabel` portion — so it is discarded:

```rust
use c2pa_http::link;

let header = r#"<https://a.example/i.jpg#jumbf=c2pa/urn:uuid:1234>; rel="c2pa-manifest""#;
let found = link::extract([header])?;
assert!(found.is_embedded());
assert_eq!(found.uri, "https://a.example/i.jpg#jumbf=c2pa");
# Ok::<(), c2pa_http::Error>(())
```

## Parsing that holds up

Commas separate link-values and semicolons separate parameters — but both are
legal inside a `<target>` or a quoted string. A query string of `?ids=1,2,3` is
ordinary, and splitting naively on those characters is the classic way to
mis-parse this header. The scanner tracks both contexts.

Also handled: quoted and unquoted `rel`, case-insensitive matching, `rel` as a
space-separated token list, several `Link` fields on one response, backslash
escapes inside quoted parameters, and RFC 8288's rule that only the **first**
`rel` parameter counts.

Competing targets are rejected rather than guessed at: the specification defines
no precedence between two different `c2pa-manifest` links, so choosing one would
be inventing a rule. Duplicate links naming the *same* target are fine.

## Header injection is impossible, without rejecting anything

A raw CR, LF, space, or angle bracket cannot legally appear in a URI at all —
[RFC 3986](https://www.rfc-editor.org/rfc/rfc3986) excludes them. So a string
carrying one is not a URI to be rejected; it is a URI that has not been encoded
yet. `link::format` percent-encodes it, which is both the spec-correct repair
and what makes injection impossible:

```rust
use c2pa_http::link;

// A CR/LF payload lands inside the URI instead of starting a new header.
let header = link::format("https://a.example/\r\nX-Injected: yes")?;
assert!(header.contains("%0D%0A"));
assert!(!header.contains('\n'));
# Ok::<(), c2pa_http::Error>(())
```

`>` becomes `%3E` and can no longer close the target early; non-ASCII travels as
percent-encoded UTF-8. Encoding is **idempotent** — `%` is left untouched, so an
already-encoded URI is not double-encoded into `%2520` — and every delimiter a
URI needs (`? # / : @ & = +` and the sub-delims) is preserved, so query strings
and fragments survive intact.

When you would rather be *told* that your input needed repairing, use
`link::format_strict` or `ManifestLinkLayer::new_strict`. For a target read from
configuration that is usually the better choice: a stray space becomes a startup
error instead of a silent `%20` and a 404 at validation time.

## Scope

This crate attaches and reads a header. It deliberately does **not** inspect
request or response bodies to detect embedded provenance: that means buffering
the whole body before forwarding it, which turns a streaming proxy into an
unbounded memory sink and hands any client a denial of service. That belongs
behind its own explicit opt-in with a mandatory size cap, not in the layer that
writes a header.

## Other languages

Python and JavaScript get the `link` parser — the Tower layer has no meaning off
a Rust service stack, but emitting and reading the header is exactly what a web
framework needs.

```python
import c2pa_http
response["Link"] = c2pa_http.format("https://a.example/m.c2pa")
found = c2pa_http.extract([incoming.headers.get("link")])
```

```js
import { format, extract } from "c2pa-http";
res.setHeader("Link", format("https://a.example/m.c2pa"));
```

## Features

| feature | default | adds |
|---|---|---|
| `tower` | yes | the `Layer`/`Service` (`http`, `tower-layer`, `tower-service`, `pin-project-lite`) |
| `python` | no | PyO3 bindings for the PyPI distribution |

`default-features = false` leaves a dependency-free RFC 8288 parser usable under
any HTTP stack — hyper, axum, a Cloudflare Worker, or a hand-rolled server.

## Related Crates

Part of a family of single-purpose crates, one per C2PA embedding method. Each
is standalone and independently versioned.

| Crate | Description |
|---|---|
| [c2pa-structured-text](https://crates.io/crates/c2pa-structured-text) | Structured text: ASCII-armoured manifest in a comment or front matter |
| [c2pa-unstructured-text](https://crates.io/crates/c2pa-unstructured-text) | Unstructured text: invisible Unicode variation-selector run |
| [c2pa-html](https://crates.io/crates/c2pa-html) | HTML: `script` and `link` elements in the document head |
| [c2pa-text-binding](https://crates.io/crates/c2pa-text-binding) | Soft binding and content fingerprinting for text assets |
| [c2pa-vtt](https://crates.io/crates/c2pa-vtt) | WebVTT caption and subtitle embedding |
| [c2pa-zip](https://crates.io/crates/c2pa-zip) | ZIP-based documents: EPUB, DOCX, ODT, OXPS |
| [c2pa-warc](https://crates.io/crates/c2pa-warc) | WARC web archive embedding (ISO 28500) |
| [c2pa-fonts](https://crates.io/crates/c2pa-fonts) | OpenType/TrueType (SFNT) font embedding |
| [c2pa-ml](https://crates.io/crates/c2pa-ml) | ML model containers: GGUF, SafeTensors, ONNX |
| [c2pa](https://crates.io/crates/c2pa) | Official C2PA SDK |

## Security

Found a vulnerability? Please report it privately — see [SECURITY.md](./SECURITY.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.

Built by [WritersLogic](https://writerslogic.com)
