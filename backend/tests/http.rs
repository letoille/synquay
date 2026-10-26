use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn health_and_info_contract() {
    for (path, expected) in [
        ("/api/health", json!({"status": "ok"})),
        (
            "/api/info",
            json!({"name": "Synquay", "version": env!("CARGO_PKG_VERSION"), "stage": "scaffold"}),
        ),
    ] {
        let response = synquay_server::app()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert!(Uuid::parse_str(response.headers()["x-request-id"].to_str().unwrap()).is_ok());
        let body = to_bytes(response.into_body(), 2048).await.unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), expected);
    }
}

#[tokio::test]
async fn errors_include_the_response_correlation_id() {
    for (method, path, status, code) in [
        ("GET", "/api/missing", StatusCode::NOT_FOUND, "not_found"),
        (
            "POST",
            "/api/health",
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ),
    ] {
        let response = synquay_server::app()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        let request_id = response.headers()["x-request-id"]
            .to_str()
            .unwrap()
            .to_owned();
        let body = to_bytes(response.into_body(), 2048).await.unwrap();
        let error: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(error["error"]["code"], code);
        assert_eq!(error["error"]["request_id"], request_id);
    }
}

#[tokio::test]
async fn caller_cannot_reuse_or_spoof_server_request_ids() {
    let mut previous = String::new();
    for _ in 0..2 {
        let response = synquay_server::app()
            .oneshot(
                Request::get("/api/health")
                    .header("x-request-id", "caller-controlled-id")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let id = response.headers()["x-request-id"].to_str().unwrap();
        assert!(Uuid::parse_str(id).is_ok());
        assert_ne!(id, "caller-controlled-id");
        assert_ne!(id, previous);
        previous = id.to_owned();
    }
}
