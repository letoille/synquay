use super::middleware::RequestContext;
use axum::{
    Extension, Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}
#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
    request_id: String,
}

pub(super) fn response(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    context: RequestContext,
) -> Response {
    (
        status,
        Json(ErrorEnvelope {
            error: ErrorBody {
                code,
                message,
                request_id: context.request_id,
            },
        }),
    )
        .into_response()
}

pub async fn not_found(Extension(context): Extension<RequestContext>) -> Response {
    response(
        StatusCode::NOT_FOUND,
        "not_found",
        "The requested resource was not found.",
        context,
    )
}

pub async fn method_not_allowed(Extension(context): Extension<RequestContext>) -> Response {
    response(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "The request method is not allowed.",
        context,
    )
}
