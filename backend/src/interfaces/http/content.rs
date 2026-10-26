use super::{error, middleware::RequestContext};
use crate::{AppState, application::ports::content::ContentStore};
use axum::{
    Extension, Json,
    body::Body,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures_util::TryStreamExt;
use tokio_util::{
    io::{ReaderStream, StreamReader},
    sync::CancellationToken,
};
use uuid::Uuid;
/// Upload is explicitly opt-in through pipeline.content. Authentication precedes streaming.
pub async fn upload(
    State(state): State<AppState>,
    Path(connection): Path<Uuid>,
    Extension(ctx): Extension<RequestContext>,
    request: Request,
) -> Response {
    let Some(p) = state.pipeline else {
        return error::response(StatusCode::NOT_FOUND, "not_found", "Not configured.", ctx);
    };
    if connection != p.source.connection_id || p.content.is_none() {
        return error::response(StatusCode::NOT_FOUND, "not_found", "Not configured.", ctx);
    }
    if !p.authenticates(
        request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok()),
    ) {
        return error::response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Valid source credentials required.",
            ctx,
        );
    }
    let Some(size) = request
        .headers()
        .get("content-length")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse().ok())
    else {
        return error::response(
            StatusCode::LENGTH_REQUIRED,
            "length_required",
            "Content-Length is required.",
            ctx,
        );
    };
    let media = request
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_owned();
    let input = StreamReader::new(
        request
            .into_body()
            .into_data_stream()
            .map_err(std::io::Error::other),
    );
    let operation = p
        .context(Uuid::new_v4(), CancellationToken::new())
        .operation;
    match p
        .content
        .as_ref()
        .unwrap()
        .upload(operation, size, media, Box::pin(input))
        .await
    {
        Ok(reference) => (StatusCode::CREATED, Json(reference)).into_response(),
        Err(crate::application::ports::PortError::PayloadTooLarge) => error::response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            "Content exceeds limit.",
            ctx,
        ),
        Err(crate::application::ports::PortError::InvalidInput { .. }) => error::response(
            StatusCode::BAD_REQUEST,
            "invalid_content",
            "Invalid content metadata or length.",
            ctx,
        ),
        Err(_) => error::response(
            StatusCode::SERVICE_UNAVAILABLE,
            "content_unconfirmed",
            "Upload was not confirmed; retry creates a new reference.",
            ctx,
        ),
    }
}
pub async fn download(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(ctx): Extension<RequestContext>,
    request: Request,
) -> Response {
    let Some(p) = state.pipeline else {
        return error::response(StatusCode::NOT_FOUND, "not_found", "Not configured.", ctx);
    };
    if !p.authenticates(
        request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok()),
    ) {
        return error::response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Valid source credentials required.",
            ctx,
        );
    }
    let Ok(permit) = p.content_downloads.clone().try_acquire_owned() else {
        return error::response(
            StatusCode::SERVICE_UNAVAILABLE,
            "content_busy",
            "Try later.",
            ctx,
        );
    };
    let Some(store) = &p.content else {
        return error::response(StatusCode::NOT_FOUND, "not_found", "Not configured.", ctx);
    };
    match store
        .download(
            p.context(Uuid::new_v4(), CancellationToken::new())
                .operation,
            id,
        )
        .await
    {
        Ok((reference, input)) => {
            use futures_util::StreamExt;
            let stream = ReaderStream::new(input).map(move |chunk| {
                let _ = &permit;
                chunk
            });
            let mut response = Body::from_stream(stream).into_response();
            response
                .headers_mut()
                .insert("Content-Type", "application/octet-stream".parse().unwrap());
            response
                .headers_mut()
                .insert("Content-Disposition", "attachment".parse().unwrap());
            response
                .headers_mut()
                .insert("X-Content-Type-Options", "nosniff".parse().unwrap());
            response
                .headers_mut()
                .insert("Cache-Control", "no-store".parse().unwrap());
            response
                .headers_mut()
                .insert("Content-Length", reference.size.into());
            response
        }
        Err(_) => error::response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Content unavailable.",
            ctx,
        ),
    }
}
