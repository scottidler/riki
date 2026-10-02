//! `GET /_riki/assets/{name}`: the page stylesheet and script, the editor bundle, its
//! stylesheet, and the bundled fonts, compiled into the binary. The four code assets are built
//! from `editor/` and committed under `server/assets/`; otto's `editor` task fails when the
//! committed files differ from a fresh build. The fonts (`server/assets/fonts/`) are Inter and
//! JetBrains Mono variable woff2 subsets under the SIL Open Font License; each license sits beside
//! its files.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::OnceLock;

use axum::extract::Path;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tracing::debug;

pub struct Asset {
    pub name: &'static str,
    pub content_type: &'static str,
    pub bytes: &'static [u8],
}

/// A `static`, not a `const`: a `const` is instantiated at each use, and the 942755c release
/// binary carried every embedded asset twice that way.
pub static ASSETS: &[Asset] = &[
    Asset {
        name: "riki.css",
        content_type: "text/css; charset=utf-8",
        bytes: include_bytes!("../assets/riki.css"),
    },
    Asset {
        name: "riki.js",
        content_type: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../assets/riki.js"),
    },
    Asset {
        name: "editor.js",
        content_type: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../assets/editor.js"),
    },
    Asset {
        name: "editor.css",
        content_type: "text/css; charset=utf-8",
        bytes: include_bytes!("../assets/editor.css"),
    },
    Asset {
        name: "inter-latin-wght-normal.woff2",
        content_type: "font/woff2",
        bytes: include_bytes!("../assets/fonts/inter-latin-wght-normal.woff2"),
    },
    Asset {
        name: "inter-latin-wght-italic.woff2",
        content_type: "font/woff2",
        bytes: include_bytes!("../assets/fonts/inter-latin-wght-italic.woff2"),
    },
    Asset {
        name: "inter-latin-ext-wght-normal.woff2",
        content_type: "font/woff2",
        bytes: include_bytes!("../assets/fonts/inter-latin-ext-wght-normal.woff2"),
    },
    Asset {
        name: "jetbrains-mono-latin-wght-normal.woff2",
        content_type: "font/woff2",
        bytes: include_bytes!("../assets/fonts/jetbrains-mono-latin-wght-normal.woff2"),
    },
    Asset {
        name: "jetbrains-mono-latin-ext-wght-normal.woff2",
        content_type: "font/woff2",
        bytes: include_bytes!("../assets/fonts/jetbrains-mono-latin-ext-wght-normal.woff2"),
    },
];

pub fn find(name: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|asset| asset.name == name)
}

/// A strong validator per asset, so a browser revalidates (`no-cache`) without re-downloading
/// until the binary changes.
fn etag(asset: &Asset) -> &'static str {
    static TAGS: OnceLock<Vec<String>> = OnceLock::new();
    let tags = TAGS.get_or_init(|| {
        ASSETS
            .iter()
            .map(|asset| {
                let mut hasher = DefaultHasher::new();
                asset.bytes.hash(&mut hasher);
                format!("\"{:016x}\"", hasher.finish())
            })
            .collect()
    });
    let index = ASSETS
        .iter()
        .position(|known| known.name == asset.name)
        .expect("etag(): asset comes from ASSETS");
    &tags[index]
}

pub async fn asset(Path(name): Path<String>, headers: HeaderMap) -> Response {
    debug!("asset: name={name:?}");
    let Some(asset) = find(&name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let tag = etag(asset);
    let common = [
        (header::ETAG, HeaderValue::from_static(tag)),
        (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        (header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
    ];
    let fresh = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == tag));
    if fresh {
        return (StatusCode::NOT_MODIFIED, common).into_response();
    }
    (
        common,
        [(header::CONTENT_TYPE, HeaderValue::from_static(asset.content_type))],
        asset.bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests;
