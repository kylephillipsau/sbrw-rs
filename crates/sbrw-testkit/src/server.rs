//! Drives sbrw-rs the way the reference recorder drives the Java core.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use anyhow::{Context, anyhow, bail};
use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::http::header::CONTENT_TYPE;
use http_body_util::BodyExt;
use indexmap::IndexMap;
use sbrw_common::{CaptureSink, Config, EffectLog, Effects, Env};
use sbrw_core::{World, WorldHandle, WorldOptions};
use serde_json::Value;
use time::Duration;
use tower::ServiceExt;

use crate::scenario::{Db, HttpRequest, HttpResponse, Scenario};

/// Response headers worth comparing. The rest (`vary`, `content-length`, ...)
/// depend on the HTTP stack, not on the ported code.
const COMPARED_HEADERS: [&str; 2] = ["Connection", "Content-Type"];

pub struct SbrwServer {
    router: Router,
    env: Env,
    effects: EffectLog,
    world: WorldHandle,
    _data_dir: tempfile::TempDir,
}

impl SbrwServer {
    /// Start a fresh server under the scenario's seed and clock, returning the
    /// effects of booting.
    pub fn boot(scenario: &Scenario) -> anyhow::Result<(Self, Effects)> {
        let data_dir = tempfile::tempdir().context("creating a data directory")?;
        let mut config = Config::default();
        config.server.data_dir = data_dir.path().to_path_buf();

        let env = Env::deterministic(scenario.seed, scenario.clock);
        let effects = EffectLog::recording();
        let world = World::boot(WorldOptions {
            db_path: None,
            parameters: scenario.parameters.clone().unwrap_or_default(),
            env: env.clone(),
            effects: effects.clone(),
        })?;
        let world = WorldHandle::spawn(world);
        let state = sbrw_http::AppState::new(Arc::new(config), CaptureSink::disabled(), world.clone());
        let router = sbrw_http::router(state);
        let boot = effects.drain();
        Ok((Self { router, env, effects, world, _data_dir: data_dir }, boot))
    }

    pub async fn http(&mut self, r: &HttpRequest) -> anyhow::Result<HttpResponse> {
        let mut uri = r.path.clone();
        if let Some(query) = r.query.as_ref().filter(|q| !q.is_empty()) {
            uri.push('?');
            uri.push_str(&url_encode(query));
        }
        let mut builder = Request::builder().method(r.method.as_str()).uri(uri);
        for (name, value) in r.headers.iter().flatten() {
            builder = builder.header(name, value);
        }
        let body = if let Some(body) = &r.body {
            builder = builder.header(CONTENT_TYPE, r.content_type.as_deref().unwrap_or("application/xml"));
            Body::from(body.clone())
        } else if let Some(form) = &r.form {
            builder = builder.header(CONTENT_TYPE, "application/x-www-form-urlencoded");
            Body::from(url_encode(form))
        } else {
            Body::empty()
        };
        let mut request = builder.body(body).context("building the request")?;
        let ip: IpAddr = r.remote_addr.as_deref().unwrap_or("127.0.0.1").parse().context("remote_addr")?;
        request.extensions_mut().insert(ConnectInfo(SocketAddr::new(ip, 0)));

        let response = self.router.clone().oneshot(request).await?;
        let status = response.status().as_u16();
        let mut headers = IndexMap::new();
        for name in COMPARED_HEADERS {
            if let Some(value) = response.headers().get(name) {
                headers.insert(name.to_owned(), Value::String(String::from_utf8_lossy(value.as_bytes()).into_owned()));
            }
        }
        let body = response.into_body().collect().await.context("reading the response body")?.to_bytes();
        Ok(HttpResponse { status, headers, body: String::from_utf8_lossy(&body).into_owned() })
    }

    pub fn advance(&self, millis: i64) {
        self.env.advance(Duration::milliseconds(millis));
    }

    /// Fire every timer due at the current time; returns how many fired.
    pub async fn fire_timers(&mut self) -> anyhow::Result<i64> {
        // sbrw-rs schedules no timers yet, so none are ever due.
        Ok(0)
    }

    /// Run a scheduled job named after upstream's `@Schedule` method.
    pub async fn job(&mut self, name: &str) -> anyhow::Result<()> {
        bail!("sbrw-rs has no scheduled job {name}")
    }

    /// Effects since the last call.
    pub fn effects(&self) -> Effects {
        self.effects.drain()
    }

    /// Every runtime table with rows, in the recorder's format.
    pub async fn snapshot(&self) -> anyhow::Result<Db> {
        self.world
            .call(|w| crate::snapshot::sqlite(w.db(), &[]).map_err(|e| anyhow!(e).into()))
            .await
            .map_err(|fault| anyhow!("{fault}"))
    }
}

/// `application/x-www-form-urlencoded`, which matches Java's `URLEncoder` used
/// by the recorder: spaces as `+`, only `*-._` and alphanumerics left as is.
fn url_encode(params: &IndexMap<String, String>) -> String {
    form_urlencoded::Serializer::new(String::new()).extend_pairs(params).finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_like_java_url_encoder() {
        let params: IndexMap<String, String> =
            [("email".to_owned(), "a b+c@example.com".to_owned()), ("x".to_owned(), "*-._~".to_owned())].into();
        assert_eq!(url_encode(&params), "email=a+b%2Bc%40example.com&x=*-._%7E");
    }
}
