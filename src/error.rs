// Copyright 2026 WritersLogic. All rights reserved.
// Licensed under the Apache License, Version 2.0 or the MIT license,
// at your option.

use std::fmt;

/// Errors from locating a C2PA Manifest Store via an HTTP `Link` header.
///
/// # What carries a status code
///
/// Locating a manifest by reference is a prerequisite to validation. A response
/// with no `c2pa-manifest` link simply has no provenance to check, which is not
/// a failure.
///
/// The one registered code that belongs to this crate is
/// `manifest.inaccessible`: the specification requires it when a manifest "was
/// documented to exist in a remote location, but is not present there, or the
/// location is not currently available (such as in an offline scenario)". This
/// crate performs no network I/O, so it never raises that itself — it is
/// exposed as [`Error::Inaccessible`] for the caller that does the fetching to
/// report through the same type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No `Link` header with `rel="c2pa-manifest"` was present.
    NotFound,
    /// More than one distinct `c2pa-manifest` target was advertised.
    ///
    /// The specification describes retrieving "a" Manifest Store from "that URI
    /// reference" and defines no precedence between competing links, so picking
    /// one would be inventing a rule. Duplicate links naming the *same* target
    /// are not an error.
    MultipleLinks,
    /// A `Link` field value could not be parsed as RFC 8288.
    Malformed(&'static str),
    /// A manifest was advertised but could not be retrieved.
    ///
    /// Reported as `manifest.inaccessible`. Raised by the caller performing the
    /// fetch, not by this crate.
    Inaccessible,
}

impl Error {
    /// The registered C2PA validation status code for this error, or `None`
    /// when the condition carries no status code.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::Inaccessible => Some("manifest.inaccessible"),
            Self::NotFound | Self::MultipleLinks | Self::Malformed(_) => None,
        }
    }

    /// Whether this means the response advertised no provenance at all, as
    /// opposed to provenance that was advertised and could not be used.
    pub fn is_no_manifest_located(&self) -> bool {
        matches!(
            self,
            Self::NotFound | Self::MultipleLinks | Self::Malformed(_)
        )
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "no c2pa-manifest Link header found"),
            Self::MultipleLinks => {
                write!(f, "more than one c2pa-manifest target was advertised")
            }
            Self::Malformed(why) => write!(f, "malformed Link header: {why}"),
            Self::Inaccessible => {
                write!(f, "the advertised manifest could not be retrieved")
            }
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<Error> {
        vec![
            Error::NotFound,
            Error::MultipleLinks,
            Error::Malformed("unterminated target"),
            Error::Inaccessible,
        ]
    }

    #[test]
    fn display_composes_into_a_sentence_for_every_variant() {
        for e in all() {
            let s = e.to_string();
            assert!(!s.is_empty(), "{e:?} rendered empty");
            assert!(!s.ends_with('.'), "{e:?} ends with a period: {s}");
            let first = s.chars().next().expect("checked non-empty above");
            assert!(!first.is_uppercase(), "{e:?} starts uppercase: {s}");
        }
    }

    #[test]
    fn only_inaccessible_carries_a_code() {
        assert_eq!(Error::Inaccessible.code(), Some("manifest.inaccessible"));
        for e in [Error::NotFound, Error::MultipleLinks, Error::Malformed("x")] {
            assert_eq!(e.code(), None, "{e:?} must not report a status code");
            assert!(
                e.is_no_manifest_located(),
                "{e:?} must classify as unsigned"
            );
        }
    }

    #[test]
    fn inaccessible_is_not_an_absence_of_provenance() {
        // Something was advertised; it just could not be fetched.
        assert!(!Error::Inaccessible.is_no_manifest_located());
    }

    #[test]
    fn every_code_is_a_registered_identifier() {
        for e in all() {
            if let Some(code) = e.code() {
                assert_eq!(code, "manifest.inaccessible", "{e:?} invented a code");
            }
        }
    }
}
