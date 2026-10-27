use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;

use ech_db_protocol::error::Reject;
use ech_db_protocol::id::{Root, Txid};
use ech_db_protocol::value::Request;

use crate::error::DbError;
use crate::log::{Dir, History};
use crate::server::Server;

const CONTENT_TYPE: &str = "application/octet-stream";

impl Server {
    pub const BODY_LIMIT: usize = 12 * 1024 * 1024;
    pub const MAX_LIMIT: u64 = 64;

    pub fn router(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/{root}/append", post(Server::append_request))
            .route("/{root}/query", post(Server::query_request))
            .route("/{root}/tx", get(Server::history_request))
            .layer(DefaultBodyLimit::max(Self::BODY_LIMIT))
            .with_state(self.clone())
    }

    async fn append_request(
        State(server): State<Arc<Server>>,
        Path(root): Path<String>,
        body: Bytes,
    ) -> Response {
        let root = match root.parse::<Root>() {
            Ok(root) => root,
            Err(_) => return ApiError::reject(Reject::BadRequest).into_response(),
        };
        let request = match bcs::from_bytes::<Request>(&body) {
            Ok(request) => request,
            Err(_) => return ApiError::reject(Reject::BadRequest).into_response(),
        };
        match server.append(root, request).await {
            Ok(outcome) => Self::encoded(&outcome),
            Err(error) => ApiError::from(error).into_response(),
        }
    }

    async fn query_request(
        State(server): State<Arc<Server>>,
        Path(root): Path<String>,
        body: Bytes,
    ) -> Response {
        let root = match root.parse::<Root>() {
            Ok(root) => root,
            Err(_) => return ApiError::reject(Reject::BadRequest).into_response(),
        };
        let request = match bcs::from_bytes::<Request>(&body) {
            Ok(request) => request,
            Err(_) => return ApiError::reject(Reject::BadRequest).into_response(),
        };
        match server.query(root, request).await {
            Ok(outcome) => Self::encoded(&outcome),
            Err(error) => ApiError::from(error).into_response(),
        }
    }

    async fn history_request(
        State(server): State<Arc<Server>>,
        Path(root): Path<String>,
        Query(params): Query<HistoryQuery>,
    ) -> Response {
        let root = match root.parse::<Root>() {
            Ok(root) => root,
            Err(_) => return ApiError::reject(Reject::BadRequest).into_response(),
        };
        let dir = match params.dir.as_deref().and_then(Dir::parse) {
            Some(dir) => dir,
            None => return ApiError::reject(Reject::BadRequest).into_response(),
        };
        let from = match params.from.as_deref() {
            None => None,
            Some(text) => match text.parse::<Txid>() {
                Ok(txid) => Some(txid),
                Err(_) => return ApiError::reject(Reject::BadRequest).into_response(),
            },
        };
        let limit = match params.limit.as_deref() {
            None => return ApiError::reject(Reject::BadRequest).into_response(),
            Some(text) => match text.parse::<u64>() {
                Ok(limit) if limit > 0 && limit <= Self::MAX_LIMIT => limit,
                _ => return ApiError::reject(Reject::BadRequest).into_response(),
            },
        };
        match server.history(root, History { dir, from, limit }).await {
            Ok(records) => Self::encoded(&records),
            Err(error) => ApiError::from(error).into_response(),
        }
    }

    fn encoded<T: serde::Serialize>(value: &T) -> Response {
        match bcs::to_bytes(value) {
            Ok(bytes) => ([(header::CONTENT_TYPE, CONTENT_TYPE)], bytes).into_response(),
            Err(_) => ApiError::reject(Reject::Integrity).into_response(),
        }
    }
}

#[derive(Deserialize)]
struct HistoryQuery {
    dir: Option<String>,
    from: Option<String>,
    limit: Option<String>,
}

struct ApiError(DbError);

impl ApiError {
    fn reject(reject: Reject) -> Self {
        Self(DbError::Reject(reject))
    }

    fn status(reject: &Reject) -> StatusCode {
        match reject {
            Reject::BadSignature => StatusCode::FORBIDDEN,
            Reject::Duplicate => StatusCode::CONFLICT,
            Reject::NotFound | Reject::ProgramNotFound | Reject::NotAProgram => {
                StatusCode::NOT_FOUND
            }
            Reject::Integrity => StatusCode::INTERNAL_SERVER_ERROR,
            Reject::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::BAD_REQUEST,
        }
    }
}

impl From<DbError> for ApiError {
    fn from(error: DbError) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, reject) = match self.0 {
            DbError::Reject(reject) => (Self::status(&reject), reject),
            DbError::Fdb(error) => {
                tracing::warn!(error = %error, "fdb failure");
                (StatusCode::SERVICE_UNAVAILABLE, Reject::Unavailable)
            }
        };
        let bytes = bcs::to_bytes(&reject).expect("reject encodes");
        (status, [(header::CONTENT_TYPE, CONTENT_TYPE)], bytes).into_response()
    }
}
