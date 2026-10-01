//! Content path validation. Every repo-relative path riki reads, indexes, or (later) writes goes
//! through [`validate`] first, so a bad path is a typed error and never reaches git2 (whose
//! `TreeUpdateBuilder::upsert` unwraps on a NUL).

use thiserror::Error;
use tracing::debug;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PathError {
    #[error("path is empty")]
    Empty,
    #[error("path {0:?} contains a NUL byte")]
    Nul(String),
    #[error("path {0:?} has an empty segment (leading, trailing, or doubled `/`)")]
    EmptySegment(String),
    #[error("path {0:?} contains a `..` segment")]
    DotDot(String),
    #[error("path {path:?} has segment {segment:?} starting with `.`")]
    LeadingDot { path: String, segment: String },
    #[error("path is not valid UTF-8: {0:?}")]
    NotUtf8(String),
}

/// Check a repo-relative, `/`-separated path: non-empty, no NUL, no empty segment, no `..`, and no
/// segment starting with `.`.
pub fn validate(path: &str) -> Result<(), PathError> {
    debug!("path::validate: path={path:?}");
    if path.is_empty() {
        return Err(PathError::Empty);
    }
    if path.contains('\0') {
        return Err(PathError::Nul(path.to_string()));
    }
    for segment in path.split('/') {
        if segment.is_empty() {
            return Err(PathError::EmptySegment(path.to_string()));
        }
        if segment == ".." {
            return Err(PathError::DotDot(path.to_string()));
        }
        if segment.starts_with('.') {
            return Err(PathError::LeadingDot {
                path: path.to_string(),
                segment: segment.to_string(),
            });
        }
    }
    Ok(())
}

/// Validate raw path bytes (git tree entry names are bytes, not strings).
pub fn validate_bytes(path: &[u8]) -> Result<&str, PathError> {
    let text = std::str::from_utf8(path).map_err(|_| PathError::NotUtf8(String::from_utf8_lossy(path).into_owned()))?;
    validate(text)?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_paths_pass() {
        assert_eq!(validate("README.md"), Ok(()));
        assert_eq!(validate("a/b/c.md"), Ok(()));
        assert_eq!(validate("a/b.c/d..e.md"), Ok(()));
    }

    #[test]
    fn dot_dot_is_typed() {
        assert_eq!(validate("a/../b.md"), Err(PathError::DotDot("a/../b.md".into())));
        assert_eq!(validate(".."), Err(PathError::DotDot("..".into())));
    }

    #[test]
    fn nul_is_typed() {
        assert_eq!(validate("a/b\0.md"), Err(PathError::Nul("a/b\0.md".into())));
    }

    #[test]
    fn leading_dot_segments_are_typed() {
        assert!(matches!(validate(".git/config"), Err(PathError::LeadingDot { segment, .. }) if segment == ".git"));
        assert!(matches!(validate("a/./b.md"), Err(PathError::LeadingDot { segment, .. }) if segment == "."));
        assert!(matches!(validate("a/.hidden.md"), Err(PathError::LeadingDot { .. })));
    }

    #[test]
    fn empty_and_empty_segments_are_typed() {
        assert_eq!(validate(""), Err(PathError::Empty));
        assert!(matches!(validate("/a.md"), Err(PathError::EmptySegment(_))));
        assert!(matches!(validate("a//b.md"), Err(PathError::EmptySegment(_))));
        assert!(matches!(validate("a/"), Err(PathError::EmptySegment(_))));
    }

    #[test]
    fn bytes_must_be_utf8() {
        assert_eq!(validate_bytes(b"a/b.md"), Ok("a/b.md"));
        assert!(matches!(validate_bytes(b"a/\xff.md"), Err(PathError::NotUtf8(_))));
        assert!(matches!(validate_bytes(b"../x"), Err(PathError::DotDot(_))));
    }
}
