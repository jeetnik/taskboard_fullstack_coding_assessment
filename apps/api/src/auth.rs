use std::time::{SystemTime, UNIX_EPOCH};

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{Error as PasswordError, SaltString},
};
use axum::{
    Json,
    extract::{FromRequestParts, Query, State},
    http::{StatusCode, header::AUTHORIZATION, request::Parts},
};
use hmac::{Hmac, Mac};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::{Rng, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

use crate::{
    AppState,
    error::ApiError,
    model::{Role, USER_FIELDS, User},
};

const JAMES_EMAIL: &str = "jamesbond@example.com";
const ADMIN_EMAIL: &str = "admin@example.com";
const CODE_LIFETIME: u64 = 300;
const TOKEN_LIFETIME: u64 = 3_600;
const MAX_ATTEMPTS: i64 = 5;
const MAILBOX_LIMIT: usize = 128;
const ISSUER: &str = "taskboard";
const AUDIENCE: &str = "taskboard-api";

#[derive(Serialize, Deserialize)]
struct Claims {
    sub: String,
    iss: String,
    aud: String,
    iat: u64,
    exp: u64,
}

pub struct Jwt {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
}

impl Jwt {
    pub fn new(secret: &[u8]) -> Self {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[ISSUER]);
        validation.set_audience(&[AUDIENCE]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = 0;
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            validation,
        }
    }

    fn issue(&self, user: &User, now: u64) -> Result<String, ApiError> {
        let claims = Claims {
            sub: user.id.clone(),
            iss: ISSUER.into(),
            aud: AUDIENCE.into(),
            iat: now,
            exp: now + TOKEN_LIFETIME,
        };
        jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &self.encoding)
            .map_err(ApiError::internal)
    }

    fn subject(&self, token: &str) -> Result<String, ApiError> {
        jsonwebtoken::decode::<Claims>(token, &self.decoding, &self.validation)
            .map(|data| data.claims.sub)
            .map_err(|_| ApiError::unauthorized("invalid_token", "Sign in to continue."))
    }
}

pub struct CurrentUser(pub User);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        app: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let (scheme, token) = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split_once(' '))
            .ok_or_else(|| ApiError::unauthorized("missing_token", "Sign in to continue."))?;
        if !scheme.eq_ignore_ascii_case("Bearer") {
            return Err(ApiError::unauthorized(
                "invalid_token",
                "Sign in to continue.",
            ));
        }
        let id = app.jwt.subject(token)?;
        let user =
            sqlx::query_as::<_, User>(&format!("SELECT {USER_FIELDS} FROM users WHERE id = ?"))
                .bind(id)
                .fetch_optional(&app.db)
                .await?
                .ok_or_else(|| ApiError::unauthorized("invalid_token", "Sign in to continue."))?;
        Ok(Self(user))
    }
}

pub struct Admin(pub User);

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        app: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let CurrentUser(user) = CurrentUser::from_request_parts(parts, app).await?;
        if user.role != Role::Admin {
            return Err(ApiError::forbidden(
                "Only admins can create, assign, or update tasks.",
            ));
        }
        Ok(Self(user))
    }
}

#[derive(Clone, Serialize)]
pub struct EmailLog {
    email: String,
    login_challenge_id: String,
    code: String,
    expires_at: u64,
}

#[derive(Deserialize)]
pub struct EmailFilter {
    email: Option<String>,
    login_challenge_id: Option<String>,
}

pub async fn latest_email(
    State(app): State<AppState>,
    Query(filter): Query<EmailFilter>,
) -> Result<Json<EmailLog>, ApiError> {
    let mailbox = app
        .mailbox
        .as_ref()
        .ok_or_else(|| ApiError::not_found("Email log unavailable."))?;
    let now = unix_time();
    let messages = mailbox.read().await;
    messages
        .iter()
        .rev()
        .find(|message| {
            message.expires_at > now
                && filter
                    .email
                    .as_ref()
                    .is_none_or(|email| message.email.eq_ignore_ascii_case(email.trim()))
                && filter
                    .login_challenge_id
                    .as_ref()
                    .is_none_or(|id| id == &message.login_challenge_id)
        })
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::not_found("No current verification email was found."))
}

#[derive(Deserialize)]
pub struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    login_challenge_id: String,
    expires_in: u64,
}

#[derive(sqlx::FromRow)]
struct LoginUser {
    id: String,
    email: String,
    hashed_password: String,
}

pub async fn login(
    State(app): State<AppState>,
    Json(input): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    let mailbox = app.mailbox.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "email_unavailable",
            "Email delivery is unavailable.",
        )
    })?;
    let invalid_credentials =
        || ApiError::unauthorized("invalid_credentials", "Incorrect email or password.");
    let email = input.email.trim();
    if email.is_empty()
        || email.len() > 254
        || input.password.is_empty()
        || input.password.len() > 256
    {
        return Err(invalid_credentials());
    }

    let user = sqlx::query_as::<_, LoginUser>(
        "SELECT id, email, hashed_password FROM users WHERE email = ?",
    )
    .bind(email)
    .fetch_optional(&app.db)
    .await?;
    let hash = user
        .as_ref()
        .map_or(&app.dummy_password_hash, |user| &user.hashed_password)
        .clone();
    let permit = password_permit(&app)?;
    let password_matches = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        check_password(&input.password, &hash)
    })
    .await
    .map_err(ApiError::internal)?
    .map_err(ApiError::internal)?;
    let user = user
        .filter(|_| password_matches)
        .ok_or_else(invalid_credentials)?;

    let id = Uuid::new_v4().to_string();
    let code = format!("{:06}", OsRng.gen_range(0..1_000_000u32));
    let digest = code_mac(&app.code_key, &id, &code).finalize().into_bytes();
    let mut transaction = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let now = unix_time();
    let expires_at = now + CODE_LIFETIME;
    sqlx::query("DELETE FROM login_challenges WHERE expires_at <= ?")
        .bind(now as i64)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "INSERT INTO login_challenges (id, user_id, code_digest, expires_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user.id)
    .bind(&digest[..])
    .bind(expires_at as i64)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let mut messages = mailbox.write().await;
    messages.retain(|message| message.expires_at > now);
    if messages.len() == MAILBOX_LIMIT {
        messages.pop_front();
    }
    messages.push_back(EmailLog {
        email: user.email,
        login_challenge_id: id.clone(),
        code,
        expires_at,
    });
    Ok(Json(LoginResponse {
        login_challenge_id: id,
        expires_in: CODE_LIFETIME,
    }))
}

#[derive(Deserialize)]
pub struct VerifyRequest {
    login_challenge_id: String,
    code: String,
}

#[derive(sqlx::FromRow)]
struct Challenge {
    user_id: String,
    code_digest: Vec<u8>,
    expires_at: i64,
    used_at: Option<i64>,
    attempts: i64,
}

#[derive(Serialize)]
pub struct VerifyResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: u64,
    user: User,
}

pub async fn verify(
    State(app): State<AppState>,
    Json(input): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, ApiError> {
    let invalid_code = || {
        ApiError::unauthorized(
            "invalid_code",
            "That code is incorrect, expired, or already used. Try again or start a new login.",
        )
    };
    if Uuid::parse_str(&input.login_challenge_id).is_err() {
        return Err(invalid_code());
    }

    let mut transaction = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let challenge = sqlx::query_as::<_, Challenge>(
        "SELECT user_id, code_digest, expires_at, used_at, attempts FROM login_challenges WHERE id = ?",
    )
    .bind(&input.login_challenge_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(invalid_code)?;

    let now = unix_time();
    if challenge.used_at.is_some()
        || challenge.expires_at <= now as i64
        || challenge.attempts >= MAX_ATTEMPTS
    {
        return Err(invalid_code());
    }
    let code = input.code.trim();
    if code.len() != 6
        || code_mac(&app.code_key, &input.login_challenge_id, code)
            .verify_slice(&challenge.code_digest)
            .is_err()
    {
        sqlx::query("UPDATE login_challenges SET attempts = attempts + 1 WHERE id = ?")
            .bind(&input.login_challenge_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        return Err(invalid_code());
    }

    let user = sqlx::query_as::<_, User>(&format!("SELECT {USER_FIELDS} FROM users WHERE id = ?"))
        .bind(challenge.user_id)
        .fetch_one(&mut *transaction)
        .await?;
    let access_token = app.jwt.issue(&user, now)?;
    sqlx::query("UPDATE login_challenges SET used_at = ? WHERE id = ?")
        .bind(now as i64)
        .bind(input.login_challenge_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;

    Ok(Json(VerifyResponse {
        access_token,
        token_type: "Bearer",
        expires_in: TOKEN_LIFETIME,
        user,
    }))
}

pub async fn seed_users(State(app): State<AppState>) -> Result<Json<Vec<User>>, ApiError> {
    let users = [
        ("Admin", ADMIN_EMAIL, "AdminPass123!", Role::Admin),
        ("James Bond", JAMES_EMAIL, "JamesBond123!", Role::Staff),
    ];
    let permit = password_permit(&app)?;
    let seeded = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        users.map(|(name, email, password, role)| {
            hash_password(password).map(|hash| (name, email, hash, role))
        })
    })
    .await
    .map_err(ApiError::internal)?;

    let mut transaction = app.db.begin().await?;
    for seed in seeded {
        let (name, email, hash, role) = seed.map_err(ApiError::internal)?;
        sqlx::query("INSERT INTO users (id, full_name, email, hashed_password, role) VALUES (?, ?, ?, ?, ?) ON CONFLICT(email) DO NOTHING")
            .bind(Uuid::new_v4().to_string())
            .bind(name)
            .bind(email)
            .bind(hash)
            .bind(role)
            .execute(&mut *transaction)
            .await?;
    }
    let users = sqlx::query_as::<_, User>(&format!(
        "SELECT {USER_FIELDS} FROM users WHERE email IN (?, ?) ORDER BY email"
    ))
    .bind(ADMIN_EMAIL)
    .bind(JAMES_EMAIL)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(users))
}

fn password_permit(app: &AppState) -> Result<tokio::sync::OwnedSemaphorePermit, ApiError> {
    app.password_workers
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "auth_busy",
                "Too many authentication requests. Try again in a moment.",
            )
        })
}

pub fn hash_password(password: &str) -> Result<String, PasswordError> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

fn check_password(password: &str, hash: &str) -> Result<bool, PasswordError> {
    let hash = PasswordHash::new(hash)?;
    match Argon2::default().verify_password(password.as_bytes(), &hash) {
        Ok(()) => Ok(true),
        Err(PasswordError::Password) => Ok(false),
        Err(error) => Err(error),
    }
}

fn code_mac(key: &[u8], challenge_id: &str, code: &str) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts every key length");
    mac.update(challenge_id.as_bytes());
    mac.update(b":");
    mac.update(code.as_bytes());
    mac
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid system time")
        .as_secs()
}
