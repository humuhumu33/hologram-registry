//! Hologram Registry: the Docker Registry HTTP API v2 and the OCI Distribution
//! API, under `/v2/`, on the Kappa store (`apps/registry`).
//!
//! The module is opt-in, and it authenticates itself (ADR 026): the registry
//! protocol has its own challenge and its own error shape, so these routes are
//! mounted beside the server's bearer layer, not under it.
//!
//! No Kappa type is named here. Storage is `crate::oci_store`.

mod blobs;
mod body;
pub mod error;
mod manifests;
mod media;
pub mod path;
mod respond;
mod uploads;

use crate::app::AppState;
use crate::module::{LiveModule, ModuleDescriptor};
use crate::oci_store::OciStore;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header::{ALLOW, CONTENT_LENGTH, CONTENT_TYPE, LOCATION};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use error::{ErrorCode, OciError};
use path::{BlobVerb, ManifestVerb, Route, UploadVerb};
use std::sync::Arc;
use tracing::Instrument;

pub const MODULE_ID: &str = "dev.hologram.live.oci";

static DESCRIPTOR: ModuleDescriptor = ModuleDescriptor {
    id: MODULE_ID,
    name: "Hologram Registry",
    version: env!("CARGO_PKG_VERSION"),
    dependencies: &["dev.hologram.live.system"],
    // The registry API is HTTP only: it adds no operation to the RPC surface.
    operations: &[],
};

pub struct OciRegistryModule;

impl LiveModule for OciRegistryModule {
    fn descriptor(&self) -> &'static ModuleDescriptor {
        &DESCRIPTOR
    }

    fn router(&self) -> Router<AppState> {
        // No `.fallback(…)`: tonic's router carries one and the merge would
        // collide. The catch-all route makes it unnecessary.
        Router::new()
            .route("/v2", any(without_the_slash))
            .route("/v2/", any(dispatch))
            .route("/v2/{*rest}", any(dispatch))
            .layer(middleware::from_fn(envelope))
    }

    fn authenticates_itself(&self) -> bool {
        true
    }
}

/// What the server's `authenticate` layer does for every other module, minus
/// the bearer check: a request id and a request span. Then the header every
/// response under `/v2/` carries, errors included.
async fn envelope(request: Request, next: Next) -> Response {
    let span = tracing::info_span!(
        "live.server.request",
        request_id = crate::server::next_request_id(),
        method = %request.method(),
        path = %request.uri().path()
    );
    let mut response = next.run(request).instrument(span).await;
    respond::stamp_version(response.headers_mut());
    response
}

/// `/v2` is not the base route; the reference's router redirects to it.
async fn without_the_slash() -> Response {
    let mut response = StatusCode::MOVED_PERMANENTLY.into_response();
    response
        .headers_mut()
        .insert(LOCATION, HeaderValue::from_static("/v2/"));
    response
}

async fn dispatch(State(state): State<AppState>, request: Request) -> Response {
    match state.oci_store() {
        Some(store) => handle(store.clone(), request).await,
        // The volume is opened whenever the module is enabled, so this is a
        // defect, not a state a client can cause.
        None => OciError::internal(&"the registry module is enabled without a volume").into_response(),
    }
}

/// Answer one request under `/v2/` from `store`.
pub async fn handle(store: Arc<OciStore>, request: Request) -> Response {
    // The body is not `Sync`, so nothing borrowed from the whole request may
    // live across an await: the head is borrowed, the body is moved.
    let (head, body) = request.into_parts();
    let rest = head.uri.path().strip_prefix("/v2/").unwrap_or_default();
    let route = path::decode(rest)
        .ok_or_else(OciError::unknown_route)
        .and_then(|rest| path::parse(&head.method, &rest));
    match route {
        Ok(route) => serve(store, route, &head, body)
            .await
            .unwrap_or_else(IntoResponse::into_response),
        Err(error) => error.into_response(),
    }
}

async fn serve(
    store: Arc<OciStore>,
    route: Route,
    head: &axum::http::request::Parts,
    body: Body,
) -> Result<Response, OciError> {
    let (headers, query) = (&head.headers, head.uri.query());
    match route {
        Route::Base => Ok(json(StatusCode::OK, "{}")),
        Route::Options { allow } => {
            let mut response = Response::new(Body::empty());
            response
                .headers_mut()
                .insert(ALLOW, HeaderValue::from_static(allow));
            Ok(response)
        }
        Route::Blob {
            repo,
            digest,
            verb: verb @ (BlobVerb::Get | BlobVerb::Head),
        } => blobs::get(store, repo, digest, headers, verb == BlobVerb::Head).await,
        Route::Manifest {
            repo,
            reference,
            verb: verb @ (ManifestVerb::Get | ManifestVerb::Head),
        } => manifests::get(store, repo, reference, headers, verb == ManifestVerb::Head).await,
        Route::Manifest {
            repo,
            reference,
            verb: ManifestVerb::Put,
        } => manifests::put(store, repo, reference, headers, body).await,
        Route::UploadStart { repo } => uploads::start(store, repo, query, body).await,
        Route::Upload { repo, id, verb } => match verb {
            UploadVerb::Status => uploads::status(store, repo, id).await,
            UploadVerb::Patch => uploads::patch(store, repo, id, headers, body).await,
            UploadVerb::Put => uploads::put(store, repo, id, query, body).await,
            UploadVerb::Delete => uploads::cancel(store, repo, id).await,
        },
        // Delete, listing and referrers arrive with the phase that builds
        // them (P7). Until then the path is known and refused.
        Route::Catalog
        | Route::TagsList { .. }
        | Route::Manifest { .. }
        | Route::Blob { .. }
        | Route::Referrers { .. } => Err(OciError::new(ErrorCode::Unsupported)),
    }
}

fn json(status: StatusCode, body: &'static str) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(CONTENT_LENGTH, HeaderValue::from(body.len()));
    response
}
