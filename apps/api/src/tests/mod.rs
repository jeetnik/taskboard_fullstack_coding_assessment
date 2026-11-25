mod authentication;
mod task_routes;

use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use tower::ServiceExt;

use super::{App, router};

const ADMIN_EMAIL: &str = "admin@example.com";
const ADMIN_PASSWORD: &str = "AdminPass123!";
const JAMES_EMAIL: &str = "jamesbond@example.com";
const JAMES_PASSWORD: &str = "JamesBond123!";
const TEST_SIGNING_KEY: &[u8; 32] = &[42; 32];

fn test_router(db: &SqlitePool, dev_mode: bool) -> Router {
    router(Arc::new(
        App::new(db.clone(), TEST_SIGNING_KEY, dev_mode).expect("create test app"),
    ))
}

async fn seeded_router(db: &SqlitePool) -> Router {
    let app = test_router(db, true);
    send(&app, Method::POST, "/seed/users", None, None)
        .await
        .expect(StatusCode::OK);
    app
}

struct TestResponse {
    status: StatusCode,
    text: String,
}

impl TestResponse {
    fn expect(self, status: StatusCode) -> Self {
        assert_eq!(self.status, status, "Response body: {}", self.text);
        self
    }

    fn json(self) -> Value {
        serde_json::from_str(&self.text).expect("JSON response")
    }
}

async fn send(
    app: &Router,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> TestResponse {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let body = match body {
        Some(body) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };

    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    TestResponse {
        status,
        text: String::from_utf8(bytes.to_vec()).unwrap(),
    }
}

async fn start_login(app: &Router, email: &str, password: &str) -> TestResponse {
    send(
        app,
        Method::POST,
        "/auth/login",
        None,
        Some(json!({ "email": email, "password": password })),
    )
    .await
}

async fn challenge(app: &Router, email: &str, password: &str) -> (String, String) {
    let login = start_login(app, email, password)
        .await
        .expect(StatusCode::OK)
        .json();
    assert_eq!(login["expires_in"], 300);
    assert_eq!(login.as_object().unwrap().len(), 2);
    let id = login["login_challenge_id"].as_str().unwrap().to_owned();
    let log = send(
        app,
        Method::GET,
        &format!("/dev/email-logs/latest?login_challenge_id={id}"),
        None,
        None,
    )
    .await
    .expect(StatusCode::OK)
    .json();
    assert_eq!(log["login_challenge_id"], id);
    let code = log["code"].as_str().unwrap().to_owned();
    assert_eq!(code.len(), 6);
    assert!(code.bytes().all(|digit| digit.is_ascii_digit()));
    (id, code)
}

async fn verify(app: &Router, id: &str, code: &str) -> TestResponse {
    send(
        app,
        Method::POST,
        "/auth/verify-2fa",
        None,
        Some(json!({ "login_challenge_id": id, "code": code })),
    )
    .await
}

async fn sign_in(app: &Router, email: &str, password: &str) -> String {
    let (id, code) = challenge(app, email, password).await;
    let session = verify(app, &id, &code).await.expect(StatusCode::OK).json();
    assert_eq!(session["user"]["email"], email);
    assert_eq!(session["token_type"], "Bearer");
    assert_eq!(session["expires_in"], 3_600);
    session["access_token"].as_str().unwrap().to_owned()
}
