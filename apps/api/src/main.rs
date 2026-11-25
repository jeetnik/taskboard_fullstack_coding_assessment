mod auth;
mod error;
mod model;
mod tasks;

#[cfg(test)]
mod tests;

use std::{
    collections::{HashMap, VecDeque},
    env, io,
    path::Path,
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderValue, header::CACHE_CONTROL},
    routing::{get, patch, post},
};
use rand::{RngCore, rngs::OsRng};
use serde::Serialize;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use tokio::sync::{RwLock, Semaphore};
use tower_http::{set_header::SetResponseHeaderLayer, trace::TraceLayer};

type AppState = Arc<App>;

struct App {
    db: SqlitePool,
    jwt: auth::Jwt,
    code_key: [u8; 32],
    dummy_password_hash: String,
    password_workers: Arc<Semaphore>,
    mailbox: Option<RwLock<VecDeque<auth::EmailLog>>>,
    task_cache: RwLock<HashMap<String, tasks::CachedTasks>>,
}

impl App {
    fn new(
        db: SqlitePool,
        jwt_key: &[u8],
        dev_mode: bool,
    ) -> Result<Self, argon2::password_hash::Error> {
        Ok(Self {
            db,
            jwt: auth::Jwt::new(jwt_key),
            code_key: random_key(),
            dummy_password_hash: auth::hash_password("taskboard-dummy-password")?,
            password_workers: Arc::new(Semaphore::new(4)),
            mailbox: dev_mode.then(RwLock::default),
            task_cache: RwLock::default(),
        })
    }
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    dev_mode: bool,
}

async fn health(State(app): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        dev_mode: app.mailbox.is_some(),
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "taskboard_api=info,tower_http=warn".into()),
        )
        .init();

    let dev_mode = env::var("APP_DEV_MODE").as_deref() == Ok("1");
    let bind: std::net::SocketAddr = env::var("API_BIND")
        .unwrap_or_else(|_| "127.0.0.1:4000".into())
        .parse()?;
    if dev_mode && !bind.ip().is_loopback() {
        return Err(io::Error::other("APP_DEV_MODE requires a loopback API_BIND address").into());
    }

    let jwt_key = match env::var("JWT_SECRET") {
        Ok(secret) if secret.len() >= 32 => secret.into_bytes(),
        Ok(_) => return Err(io::Error::other("JWT_SECRET must be at least 32 bytes").into()),
        Err(env::VarError::NotPresent) if dev_mode => random_key().to_vec(),
        Err(error) => return Err(error.into()),
    };

    let database_path =
        env::var("DATABASE_PATH").unwrap_or_else(|_| "data/taskboard.sqlite3".into());
    if let Some(parent) = Path::new(&database_path)
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let db = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&database_path)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .foreign_keys(true)
                .busy_timeout(Duration::from_secs(5)),
        )
        .await?;
    sqlx::migrate!().run(&db).await?;

    let app =
        App::new(db, &jwt_key, dev_mode).map_err(|error| io::Error::other(error.to_string()))?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(address = %listener.local_addr()?, dev_mode, "Taskboard API listening");
    axum::serve(listener, router(Arc::new(app)))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

fn router(app: AppState) -> Router {
    let mut router = Router::new()
        .route("/health", get(health))
        .route("/auth/login", post(auth::login))
        .route("/auth/verify-2fa", post(auth::verify))
        .route("/tasks", get(tasks::list).post(tasks::create))
        .route("/tasks/{id}", patch(tasks::update))
        .route("/tasks/assign", post(tasks::assign))
        .route("/tasks/view-my-tasks", get(tasks::view_my_tasks));

    if app.mailbox.is_some() {
        router = router
            .route("/seed/users", post(auth::seed_users))
            .route("/dev/email-logs/latest", get(auth::latest_email));
    }

    router
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(app)
}

fn random_key() -> [u8; 32] {
    let mut key = [0; 32];
    OsRng.fill_bytes(&mut key);
    key
}
