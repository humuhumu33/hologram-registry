//! Routes 4 and 5: a manifest by tag or by digest.

use super::error::{Context, OciError};
use super::respond::{not_modified, stamp_digest};
use crate::oci_store::{OciStore, Reference, RepoName};
use axum::body::Body;
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use std::sync::Arc;

/// Serve what is stored, under the media type it was pushed with.
///
/// A tag is resolved to a digest once, inside the store, and that digest is
/// what is read: the answer is one whole manifest, the old or the new, never a
/// mix, however the tag moves meanwhile. What `Accept` changes is gate B
/// scenario `manifest-accept`; until it is recorded, `Accept` changes nothing.
///
/// # Errors
///
/// `MANIFEST_UNKNOWN` when the tag, the digest or the repository is not known;
/// `UNKNOWN` when the store fails.
pub async fn get(
    store: Arc<OciStore>,
    repo: RepoName,
    reference: Reference,
    headers: &HeaderMap,
    head: bool,
) -> Result<Response, OciError> {
    let manifest = tokio::task::spawn_blocking(move || store.manifest_get(&repo, &reference))
        .await
        .map_err(|error| OciError::internal(&error))?
        .map_err(|error| OciError::from_store(error, Context::Manifest))?;

    let mut response = Response::new(Body::empty());
    let out = response.headers_mut();
    stamp_digest(out, &manifest.digest);
    if not_modified(headers, &manifest.digest) {
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        return Ok(response);
    }
    out.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&manifest.media_type).map_err(|error| OciError::internal(&error))?,
    );
    out.insert(CONTENT_LENGTH, HeaderValue::from(manifest.bytes.len()));
    if !head {
        *response.body_mut() = Body::from(manifest.bytes);
    }
    Ok(response)
}
