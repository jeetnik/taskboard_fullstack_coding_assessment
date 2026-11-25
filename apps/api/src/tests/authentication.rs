use axum::http::{Method, StatusCode};
use serde_json::json;
use sqlx::SqlitePool;
use uuid::Uuid;

use super::{
    ADMIN_EMAIL, ADMIN_PASSWORD, JAMES_EMAIL, JAMES_PASSWORD, TEST_SIGNING_KEY, challenge,
    seeded_router, send, start_login, test_router, verify,
};

#[sqlx::test]
async fn seeding_is_idempotent_and_password_hashes_stay_private(db: SqlitePool) {
    let app = test_router(&db, true);
    let first = send(&app, Method::POST, "/seed/users", None, None)
        .await
        .expect(StatusCode::OK)
        .json();
    let second = send(&app, Method::POST, "/seed/users", None, None)
        .await
        .expect(StatusCode::OK)
        .json();
    assert_eq!(first, second);
    let users = first.as_array().unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0]["email"], ADMIN_EMAIL);
    assert_eq!(users[0]["role"], "admin");
    assert_eq!(users[1]["email"], JAMES_EMAIL);
    assert_eq!(users[1]["role"], "staff");
    assert!(
        users
            .iter()
            .all(|user| user.get("hashed_password").is_none())
    );

    let hashes: Vec<String> =
        sqlx::query_scalar("SELECT hashed_password FROM users ORDER BY email")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(hashes.len(), 2);
    assert!(hashes.iter().all(|hash| hash.starts_with("$argon2id$")));
    assert_ne!(hashes[0], hashes[1]);
}

#[sqlx::test]
async fn invalid_credentials_do_not_create_challenges_or_emails(db: SqlitePool) {
    let app = seeded_router(&db).await;
    let expected = start_login(&app, ADMIN_EMAIL, "wrong password")
        .await
        .expect(StatusCode::UNAUTHORIZED)
        .json();
    assert_eq!(expected["error"]["code"], "invalid_credentials");
    for (email, password) in [
        ("missing@example.com", ADMIN_PASSWORD),
        (ADMIN_EMAIL, ""),
        ("", ADMIN_PASSWORD),
    ] {
        let failure = start_login(&app, email, password)
            .await
            .expect(StatusCode::UNAUTHORIZED)
            .json();
        assert_eq!(failure, expected);
    }

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM login_challenges")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    send(&app, Method::GET, "/dev/email-logs/latest", None, None)
        .await
        .expect(StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn two_factor_rejects_wrong_codes_and_consumes_the_correct_code(db: SqlitePool) {
    let app = seeded_router(&db).await;
    let (id, code) = challenge(&app, " ADMIN@EXAMPLE.COM ", ADMIN_PASSWORD).await;
    let (digest, expires_at): (Vec<u8>, i64) =
        sqlx::query_as("SELECT code_digest, expires_at FROM login_challenges WHERE id = ?")
            .bind(&id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(digest.len(), 32);
    assert_ne!(digest.as_slice(), code.as_bytes());

    let email = send(
        &app,
        Method::GET,
        "/dev/email-logs/latest?email=ADMIN%40EXAMPLE.COM",
        None,
        None,
    )
    .await
    .expect(StatusCode::OK)
    .json();
    assert_eq!(email["email"], ADMIN_EMAIL);
    assert_eq!(email["login_challenge_id"], id);
    assert_eq!(email["expires_at"], expires_at);

    send(
        &app,
        Method::GET,
        &format!("/dev/email-logs/latest?email={JAMES_EMAIL}&login_challenge_id={id}"),
        None,
        None,
    )
    .await
    .expect(StatusCode::NOT_FOUND);
    verify(&app, "invalid-id", &code)
        .await
        .expect(StatusCode::UNAUTHORIZED);
    verify(&app, &Uuid::new_v4().to_string(), &code)
        .await
        .expect(StatusCode::UNAUTHORIZED);
    let wrong = if code == "000000" { "000001" } else { "000000" };
    let rejected = verify(&app, &id, wrong)
        .await
        .expect(StatusCode::UNAUTHORIZED)
        .json();
    assert_eq!(rejected["error"]["code"], "invalid_code");
    assert!(rejected.get("access_token").is_none());

    let session = verify(&app, &id, &code).await.expect(StatusCode::OK).json();
    assert_eq!(session["user"]["role"], "admin");
    assert_eq!(session["user"]["email"], ADMIN_EMAIL);
    assert_eq!(
        session["access_token"].as_str().unwrap().split('.').count(),
        3
    );
    assert!(session["user"].get("hashed_password").is_none());
    verify(&app, &id, &code)
        .await
        .expect(StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn a_correct_code_is_rejected_after_expiration(db: SqlitePool) {
    let app = seeded_router(&db).await;
    let (id, code) = challenge(&app, JAMES_EMAIL, JAMES_PASSWORD).await;
    sqlx::query("UPDATE login_challenges SET expires_at = 0 WHERE id = ?")
        .bind(&id)
        .execute(&db)
        .await
        .unwrap();
    verify(&app, &id, &code)
        .await
        .expect(StatusCode::UNAUTHORIZED);

    let (replacement, code) = challenge(&app, JAMES_EMAIL, JAMES_PASSWORD).await;
    assert_ne!(replacement, id);
    verify(&app, &replacement, &code)
        .await
        .expect(StatusCode::OK);
}

#[sqlx::test]
async fn five_failed_guesses_lock_a_challenge(db: SqlitePool) {
    let app = seeded_router(&db).await;
    let (id, code) = challenge(&app, JAMES_EMAIL, JAMES_PASSWORD).await;
    let wrong = if code == "000000" { "000001" } else { "000000" };
    for _ in 0..5 {
        verify(&app, &id, wrong)
            .await
            .expect(StatusCode::UNAUTHORIZED);
    }
    verify(&app, &id, &code)
        .await
        .expect(StatusCode::UNAUTHORIZED);

    let (attempts, used_at): (i64, Option<i64>) =
        sqlx::query_as("SELECT attempts, used_at FROM login_challenges WHERE id = ?")
            .bind(&id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!((attempts, used_at), (5, None));
}

#[sqlx::test]
async fn concurrent_verification_issues_only_one_token(db: SqlitePool) {
    let app = seeded_router(&db).await;
    let (id, code) = challenge(&app, JAMES_EMAIL, JAMES_PASSWORD).await;
    let (first, second) = tokio::join!(verify(&app, &id, &code), verify(&app, &id, &code));
    let mut statuses = [first.status, second.status];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::UNAUTHORIZED]);
}

#[sqlx::test]
async fn protected_routes_reject_missing_invalid_and_expired_tokens(db: SqlitePool) {
    let app = seeded_router(&db).await;
    let id: String = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(JAMES_EMAIL)
        .fetch_one(&db)
        .await
        .unwrap();
    let expired = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &json!({ "sub": id, "iss": "taskboard", "aud": "taskboard-api", "iat": 1, "exp": 2 }),
        &jsonwebtoken::EncodingKey::from_secret(TEST_SIGNING_KEY),
    )
    .unwrap();
    for token in [None, Some("malformed-token"), Some(expired.as_str())] {
        send(&app, Method::GET, "/tasks/view-my-tasks", token, None)
            .await
            .expect(StatusCode::UNAUTHORIZED);
    }
}

#[sqlx::test]
async fn local_seed_and_email_routes_are_disabled_outside_development(db: SqlitePool) {
    let app = test_router(&db, false);
    let health = send(&app, Method::GET, "/health", None, None)
        .await
        .expect(StatusCode::OK)
        .json();
    assert_eq!(health["dev_mode"], false);
    send(&app, Method::POST, "/seed/users", None, None)
        .await
        .expect(StatusCode::NOT_FOUND);
    send(&app, Method::GET, "/dev/email-logs/latest", None, None)
        .await
        .expect(StatusCode::NOT_FOUND);
    start_login(&app, ADMIN_EMAIL, ADMIN_PASSWORD)
        .await
        .expect(StatusCode::SERVICE_UNAVAILABLE);
}
