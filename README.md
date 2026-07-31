<p align="center">
  <a href="https://crates.io/crates/c2pa-http"><img src="https://img.shields.io/crates/v/c2pa-http.svg" alt="crates.io"></a>
  <a href="https://docs.rs/c2pa-http"><img src="https://docs.rs/c2pa-http/badge.svg" alt="docs.rs"></a>
  <a href="https://pypi.org/project/c2pa-http/"><img src="https://img.shields.io/pypi/v/c2pa-http.svg" alt="PyPI"></a>
  <a href="https://www.npmjs.com/package/c2pa-http"><img src="https://img.shields.io/npm/v/c2pa-http.svg" alt="npm"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="license"></a>
</p>

# c2pa-http

C2PA manifest discovery over HTTP: the `c2pa-manifest` `Link` header, per
[RFC 8288](https://www.rfc-editor.org/rfc/rfc8288), with a
[Tower](https://crates.io/crates/tower) middleware.

When an asset is served over HTTP and carries no embedded Manifest Store, a
validator should look for a `Link` header carrying `rel="c2pa-manifest"`. Its
target is where the C2PA Manifest Store can be retrieved.

```toml
[dependencies]
c2pa-http = "0.1"
```

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

## Related

| crate | method |
|---|---|
| [`c2pa-html`](https://crates.io/crates/c2pa-html) | HTML: `script` and `link` elements in the document head |
| [`c2pa-structured-text`](https://crates.io/crates/c2pa-structured-text) | structured text: ASCII-armoured manifest in a comment |
| [`c2pa-unstructured-text`](https://crates.io/crates/c2pa-unstructured-text) | unstructured text: Unicode variation selectors |

For an HTML document carrying both an in-document manifest element and a `Link`
header, the specification gives the header precedence.

## License

MIT OR Apache-2.0.
