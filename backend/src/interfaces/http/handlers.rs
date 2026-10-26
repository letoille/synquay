use super::dto::{HealthResponse, ServiceInfoResponse};
use crate::AppState;
use axum::{Json, extract::State};

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

pub async fn info(State(state): State<AppState>) -> Json<ServiceInfoResponse> {
    Json(state.system.info().into())
}
