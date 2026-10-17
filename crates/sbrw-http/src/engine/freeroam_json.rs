//! `api/GetFreeroamJson`: a JSON file the operator names, served as is.

use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use sbrw_core::Fault;

use super::Call;
use crate::AppState;

// port: api/GetFreeroamJson#getServerInformation()
/// The file `FREEROAM_JSON_PATH` names; 404 with no body when the parameter
/// is unset or the file can't be read.
pub(super) async fn get_freeroam_json(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let path = state.world().call(|w| Ok(w.params.get_str_param("FREEROAM_JSON_PATH"))).await?;
    let read = match path {
        Some(path) => tokio::fs::read(path).await.ok(),
        None => None,
    };
    Ok(match read {
        Some(bytes) => (StatusCode::OK, [(CONTENT_TYPE, call.content_type())], bytes).into_response(),
        None => {
            tracing::warn!("freeroam json file not found");
            StatusCode::NOT_FOUND.into_response()
        }
    })
}
