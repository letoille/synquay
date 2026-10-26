use axum::{
    extract::{MatchedPath, Request},
    http::HeaderValue,
    middleware::Next,
    response::Response,
};
use std::time::Instant;
use tracing::Instrument;
use uuid::Uuid;

#[derive(Clone)]
pub struct RequestContext {
    pub request_id: String,
}

pub async fn observe_request(mut request: Request, next: Next) -> Response {
    // Server-owned IDs prevent callers from spoofing correlation IDs in our logs.
    let request_id = Uuid::new_v4().to_string();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("<unmatched>", MatchedPath::as_str);
    let span = tracing::info_span!("http.request", request_id = %request_id, method = %request.method(), route);
    request.extensions_mut().insert(RequestContext {
        request_id: request_id.clone(),
    });
    async move {
        let started = Instant::now();
        tracing::debug!(event = "http.request.started", "Request started");
        let mut response = next.run(request).await;
        let status = response.status().as_u16();
        let duration_ms = started.elapsed().as_secs_f64() * 1000.0;
        response.headers_mut().insert(
            "x-request-id",
            HeaderValue::from_str(&request_id).expect("UUID is a valid header"),
        );
        // Duration ends when response headers are ready, not when a streamed body finishes.
        if response.status().is_server_error() {
            tracing::error!(
                event = "http.request.completed",
                status,
                duration_ms,
                "Request failed"
            );
        } else if response.status().is_client_error() {
            tracing::warn!(
                event = "http.request.completed",
                status,
                duration_ms,
                "Request rejected"
            );
        } else {
            tracing::info!(
                event = "http.request.completed",
                status,
                duration_ms,
                "Request completed"
            );
        }
        response
    }
    .instrument(span)
    .await
}
