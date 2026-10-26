//! Packaged console assets. API misses keep their original JSON response.
use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::Response,
};
use std::path::Path;
use tower::ServiceExt;
use tower_http::services::ServeDir;

pub fn with_console(app: Router, directory: &Path) -> Router {
    let files = ServeDir::new(directory);
    // Frontend navigation is state-based; there is no catch-all HTML fallback.
    app.layer(middleware::from_fn(move |request: Request, next: Next| {
        let files = files.clone();
        async move {
            let path = request.uri().path();
            let asset = path == "/"
                || path == "/index.html"
                || path.starts_with("/assets/")
                || path.starts_with("/brand/")
                || path == "/favicon.ico";
            if !asset || !matches!(*request.method(), Method::GET | Method::HEAD) {
                return next.run(request).await;
            }
            let mut response: Response = files.oneshot(request).await.unwrap().map(Body::new);
            response.headers_mut().insert(
                "x-content-type-options",
                HeaderValue::from_static("nosniff"),
            );
            response
                .headers_mut()
                .insert("cache-control", HeaderValue::from_static("no-cache"));
            if response.status() == StatusCode::OK {
                response
                    .headers_mut()
                    .insert("referrer-policy", HeaderValue::from_static("same-origin"));
            }
            response
        }
    }))
}
