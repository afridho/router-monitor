use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
mod history;
mod labels;
mod telemetry;

static ROUTER_URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
fn router_url() -> &'static str {
    ROUTER_URL
        .get()
        .map(String::as_str)
        .unwrap_or("https://192.168.100.1:80")
}
#[derive(Clone)]
struct App {
    session: Arc<Mutex<Option<reqwest::Client>>>,
}
#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
}
#[derive(Serialize)]
struct Snapshot {
    reachable: bool,
    authenticated: bool,
    message: String,
    pages: Vec<Page>,
}
#[derive(Serialize)]
struct Page {
    name: &'static str,
    values: Vec<Vec<String>>,
}
type ApiError = (StatusCode, Json<serde_json::Value>);
fn error(message: &str) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error":message})),
    )
}
fn client() -> Result<reqwest::Client, reqwest::Error> {
    // This exception is scoped to the fixed LAN router, never arbitrary URLs.
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .cookie_store(true)
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
}
fn local_request(headers: &HeaderMap) -> bool {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let expected = "127.0.0.1:8787";
    host == Some(expected)
        && headers
            .get(header::ORIGIN)
            .is_none_or(|v| v == "http://127.0.0.1:8787")
}
async fn login(
    State(app): State<App>,
    headers: HeaderMap,
    Json(input): Json<Login>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let router = router_url();
    if !local_request(&headers) {
        return Err(error("Origin not allowed."));
    }
    if input.username.is_empty() || input.password.is_empty() {
        return Err(error("Username and password are required."));
    }
    let mut session = app.session.lock().await;
    let jar = Arc::new(reqwest::cookie::Jar::default());
    jar.add_cookie_str(
        "Cookie=body:Language:english:id=-1; Path=/",
        &router.parse().unwrap(),
    );
    let c = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .cookie_provider(jar)
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|_| error("Unable to initialize the router connection."))?;
    c.get(format!("{router}/"))
        .send()
        .await
        .map_err(|_| error("Router unreachable. Check your network connection."))?;
    let token = c
        .post(format!("{router}/asp/GetRandCount.asp"))
        .send()
        .await
        .map_err(|_| error("Unable to retrieve the router login token."))?
        .text()
        .await
        .map_err(|_| error("Invalid router token response."))?;
    let response = c
        .post(format!("{router}/login.cgi"))
        .header(header::REFERER, format!("{router}/"))
        .header(header::ORIGIN, router)
        .form(&[
            ("UserName", input.username.as_str()),
            (
                "PassWord",
                STANDARD.encode(input.password.as_bytes()).as_str(),
            ),
            ("Language", "english"),
            ("x.X_HW_Token", token.trim_start_matches('\u{feff}').trim()),
        ])
        .send()
        .await
        .map_err(|_| error("Login failed: the router did not respond."))?;
    if !response.status().is_success() {
        return Err(error("The router rejected the login request."));
    }
    let body = c
        .get(format!("{router}/index.asp"))
        .send()
        .await
        .map_err(|_| error("Unable to verify the router session."))?
        .text()
        .await
        .map_err(|_| error("Invalid router response."))?;
    if !is_authenticated_index(&body) {
        return Err(error(
            "The router session could not be verified. Check your account before retrying to avoid lockout.",
        ));
    }
    *session = Some(c);
    Ok(Json(serde_json::json!({"ok":true})))
}
fn is_device_page(body: &str) -> bool {
    body.contains("DevInfo") || body.contains("devinfo001") || body.contains("GetDeviceInfo")
}
fn is_authenticated_index(body: &str) -> bool {
    body.contains("new stNormalUserInfo(") && !body.contains("id=\"txt_Password\"")
}
fn parse_arrays(body: &str) -> Vec<Vec<String>> {
    let constructors = regex::Regex::new(r"new\s+\w+\s*\(([^\r\n;]*)\)").unwrap();
    let strings = regex::Regex::new(r#""((?:\\.|[^"\\])*)"|'((?:\\.|[^'\\])*)'"#).unwrap();
    constructors
        .captures_iter(body)
        .filter_map(|c| {
            let fields: Vec<String> = strings
                .captures_iter(&c[1])
                .map(|v| decode(v.get(1).or_else(|| v.get(2)).unwrap().as_str()))
                .collect();
            if fields.len() >= 2
                && !fields
                    .iter()
                    .any(|v| v.contains("Password") || v.contains("PassWord"))
            {
                Some(fields)
            } else {
                None
            }
        })
        .take(100)
        .collect()
}
fn decode(s: &str) -> String {
    regex::Regex::new(r"\\x([0-9a-fA-F]{2})")
        .unwrap()
        .replace_all(s, |c: &regex::Captures| {
            (u8::from_str_radix(&c[1], 16).unwrap_or(b'?') as char).to_string()
        })
        .into_owned()
}
async fn status(State(app): State<App>, headers: HeaderMap) -> Result<Json<Snapshot>, ApiError> {
    let router = router_url();
    if !local_request(&headers) {
        return Err(error("Host not allowed."));
    }
    let session = app.session.lock().await.clone();
    let c = session
        .clone()
        .unwrap_or(client().map_err(|_| error("Connection failed."))?);
    let reachable = c.get(format!("{router}/")).send().await.is_ok();
    let mut pages = Vec::new();
    let mut authenticated = false;
    if session.is_some() {
        for (name, path) in [
            ("Informasi perangkat", "/index.asp"),
            ("Optik GPON", "/html/amp/opticinfo/opticinfo.asp"),
            (
                "Perangkat terhubung",
                "/html/bbsp/userdevinfo/userdevinfo.asp",
            ),
        ] {
            if let Ok(r) = c.get(format!("{router}{path}")).send().await
                && r.status().is_success()
                && let Ok(body) = r.text().await
            {
                if name == "Informasi perangkat" {
                    authenticated = is_device_page(&body) || is_authenticated_index(&body);
                }
                if authenticated {
                    pages.push(Page {
                        name,
                        values: parse_arrays(&body),
                    });
                }
            }
        }
    }
    Ok(Json(Snapshot {
        reachable,
        authenticated,
        message: if authenticated {
            "Router session active. Read-only telemetry available."
        } else if reachable {
            "Router reachable. Connect your account to load readings."
        } else {
            "Router unreachable. Check the laptop network connection."
        }
        .into(),
        pages,
    }))
}
async fn disconnect(
    State(app): State<App>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !local_request(&headers) {
        return Err(error("Origin not allowed."));
    }
    *app.session.lock().await = None;
    Ok(Json(serde_json::json!({"ok":true})))
}
async fn telemetry_api(
    State(app): State<App>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !local_request(&headers) {
        return Err(error("Host not allowed"));
    }
    let client = app
        .session
        .lock()
        .await
        .clone()
        .ok_or_else(|| error("Connect to the router to load live measurements."))?;
    Ok(Json(telemetry::collect(&client).await))
}
async fn security(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    for (k, v) in [
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
    ] {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(k),
            axum::http::HeaderValue::from_static(v),
        );
    }
    response
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let configured =
        std::env::var("ROUTER_URL").unwrap_or_else(|_| "https://192.168.100.1:80".into());
    let url = reqwest::Url::parse(&configured)?;
    if !["http", "https"].contains(&url.scheme())
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "ROUTER_URL must be an HTTP(S) origin without credentials, path, query or fragment"
                .into(),
        );
    }
    ROUTER_URL
        .set(configured.trim_end_matches('/').to_owned())
        .unwrap();
    history::initialize()?;
    labels::initialize()?;
    let state = App {
        session: Arc::new(Mutex::new(None)),
    };
    let collector = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let client = collector.session.lock().await.clone();
            if let Some(client) = client {
                let sample = telemetry::collect(&client).await;
                match tokio::task::spawn_blocking(move || history::save(&sample)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => eprintln!("History save failed: {e}"),
                    Err(e) => eprintln!("History worker failed: {e}"),
                }
            }
        }
    });
    let app = Router::new()
        .fallback_service(tower_http::services::ServeDir::new(
            std::env::var("STATIC_DIR").unwrap_or_else(|_| "frontend/dist".into()),
        ))
        .route("/api/telemetry", get(telemetry_api))
        .route("/api/history", get(history::read))
        .route("/api/labels", get(labels::read).post(labels::save))
        .route("/api/status", get(status))
        .route("/api/login", post(login))
        .route("/api/disconnect", post(disconnect))
        .with_state(state)
        .layer(axum::middleware::from_fn(security));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8787").await?;
    println!("RouterMonitor → http://127.0.0.1:8787 (read-only)");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verifies_index_session() {
        assert!(is_authenticated_index(
            "var UserInfo = new Array(new stNormalUserInfo(\"root\", \"1\"),null)"
        ));
        assert!(!is_authenticated_index(
            "var pageName = '/'; top.location.replace(pageName)"
        ));
    }
    #[test]
    fn parses_huawei_arrays() {
        assert_eq!(
            parse_arrays(r#"var x = new DevInfo("HG8145V6", "192\x2e168\x2e100\x2e1");"#)[0],
            vec!["HG8145V6", "192.168.100.1"]
        );
    }
    #[test]
    fn rejects_login_page() {
        assert!(!is_device_page("<input id=txt_Password>"));
    }
    #[test]
    fn checks_origin() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, "127.0.0.1:8787".parse().unwrap());
        assert!(local_request(&h));
        h.insert(header::ORIGIN, "https://evil.test".parse().unwrap());
        assert!(!local_request(&h));
    }
}
