use axum::{
    Router,
    http::{Method, StatusCode},
};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use uuid::Uuid;

use super::{
    ADMIN_EMAIL, ADMIN_PASSWORD, JAMES_EMAIL, JAMES_PASSWORD, seeded_router, send, sign_in,
};

const MY_TASKS: &str = "/tasks/view-my-tasks";

async fn create_task(app: &Router, admin: &str, title: &str, priority: &str) -> Value {
    send(
        app,
        Method::POST,
        "/tasks",
        Some(admin),
        Some(json!({ "title": title, "priority": priority })),
    )
    .await
    .expect(StatusCode::CREATED)
    .json()
}

async fn assign(
    app: &Router,
    admin: &str,
    task_ids: &[&str],
    assignee: &str,
    status: StatusCode,
) -> Value {
    send(
        app,
        Method::POST,
        "/tasks/assign",
        Some(admin),
        Some(json!({ "task_ids": task_ids, "assignee_email": assignee })),
    )
    .await
    .expect(status)
    .json()
}

async fn view(app: &Router, token: &str, hit: bool, total: usize) -> Value {
    let data = send(app, Method::GET, MY_TASKS, Some(token), None)
        .await
        .expect(StatusCode::OK)
        .json();
    assert_eq!(data["cache"], json!({ "hit": hit }));
    assert_eq!(data["summary"], json!({ "total_assigned_tasks": total }));
    assert_eq!(data["tasks"].as_array().unwrap().len(), total);
    data
}

#[sqlx::test]
async fn second_james_bond_request_returns_the_same_three_tasks_from_cache(db: SqlitePool) {
    let app = seeded_router(&db).await;
    let admin = sign_in(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let mut created = Vec::new();
    for (index, priority) in ["high", "medium", "low", "medium", "low"]
        .into_iter()
        .enumerate()
    {
        let title = format!("Task {}", index + 1);
        let task = create_task(&app, &admin, &title, priority).await;
        assert_eq!(task["title"], title);
        assert_eq!(task["priority"], priority);
        assert_eq!(task["status"], "todo");
        assert_eq!(task["description"], "");
        assert!(task["assigned_to"].is_null());
        created.push(task);
    }
    let mut expected = created[..3].to_vec();
    let ids = expected
        .iter()
        .map(|task| task["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    let result = assign(&app, &admin, &ids, JAMES_EMAIL, StatusCode::OK).await;
    assert_eq!(
        result,
        json!({ "assigned_count": 3, "assigned_to": JAMES_EMAIL })
    );

    let james = sign_in(&app, JAMES_EMAIL, JAMES_PASSWORD).await;
    let first = view(&app, &james, false, 3).await;
    let second = view(&app, &james, true, 3).await;
    for task in &mut expected {
        task["assigned_to"] = json!(JAMES_EMAIL);
    }
    assert_eq!(
        first["user"],
        json!({ "email": JAMES_EMAIL, "role": "staff" })
    );
    assert_eq!(first["tasks"], json!(expected));
    assert_eq!(second["tasks"], first["tasks"]);
    assert_eq!(second["user"], first["user"]);

    let all = send(&app, Method::GET, "/tasks", Some(&admin), None)
        .await
        .expect(StatusCode::OK)
        .json();
    assert_eq!(all.as_array().unwrap().len(), 5);
    assert_eq!(all[3], created[3]);
    assert_eq!(all[4], created[4]);
}

#[sqlx::test]
async fn staff_cannot_create_list_assign_or_edit_tasks(db: SqlitePool) {
    let app = seeded_router(&db).await;
    let admin = sign_in(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let james = sign_in(&app, JAMES_EMAIL, JAMES_PASSWORD).await;
    let original = create_task(&app, &admin, "Unassigned task", "high").await;
    let id = original["id"].as_str().unwrap();
    let task_path = format!("/tasks/{id}");
    for (method, path, body) in [
        (
            Method::POST,
            "/tasks",
            Some(json!({ "title": "Forbidden", "priority": "low" })),
        ),
        (Method::GET, "/tasks", None),
        (
            Method::POST,
            "/tasks/assign",
            Some(json!({ "task_ids": [id], "assignee_email": JAMES_EMAIL })),
        ),
        (
            Method::PATCH,
            task_path.as_str(),
            Some(json!({ "status": "done" })),
        ),
    ] {
        let result = send(&app, method, path, Some(&james), body)
            .await
            .expect(StatusCode::FORBIDDEN)
            .json();
        assert_eq!(result["error"]["code"], "forbidden");
    }

    view(&app, &james, false, 0).await;
    let admin_tasks = send(&app, Method::GET, "/tasks", Some(&admin), None)
        .await
        .expect(StatusCode::OK)
        .json();
    assert_eq!(admin_tasks, json!([original]));
}

#[sqlx::test]
async fn task_reads_use_the_current_database_role_and_account(db: SqlitePool) {
    let app = seeded_router(&db).await;
    let admin = sign_in(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    view(&app, &admin, false, 0).await;
    sqlx::query("UPDATE users SET role = 'staff' WHERE email = ?")
        .bind(ADMIN_EMAIL)
        .execute(&db)
        .await
        .unwrap();

    send(&app, Method::GET, "/tasks", Some(&admin), None)
        .await
        .expect(StatusCode::FORBIDDEN);
    let response = view(&app, &admin, true, 0).await;
    assert_eq!(response["user"]["role"], "staff");

    sqlx::query(
        "DELETE FROM login_challenges WHERE user_id = (SELECT id FROM users WHERE email = ?)",
    )
    .bind(ADMIN_EMAIL)
    .execute(&db)
    .await
    .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN_EMAIL)
        .execute(&db)
        .await
        .unwrap();
    send(&app, Method::GET, MY_TASKS, Some(&admin), None)
        .await
        .expect(StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn task_fields_are_validated_and_empty_patch_is_rejected(db: SqlitePool) {
    let app = seeded_router(&db).await;
    let admin = sign_in(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    for (body, status) in [
        (
            json!({ "title": "   ", "priority": "high" }),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({ "title": "x".repeat(121), "priority": "high" }),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({ "title": "Task", "description": "x".repeat(1001), "priority": "high" }),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({ "title": "Task", "priority": "urgent" }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (json!({ "title": "Task" }), StatusCode::UNPROCESSABLE_ENTITY),
    ] {
        send(&app, Method::POST, "/tasks", Some(&admin), Some(body))
            .await
            .expect(status);
    }
    let title = "📝".repeat(120);
    let task = create_task(&app, &admin, &format!(" {title} "), "medium").await;
    assert_eq!(task["title"], title);
    let path = format!("/tasks/{}", task["id"].as_str().unwrap());
    send(&app, Method::PATCH, &path, Some(&admin), Some(json!({})))
        .await
        .expect(StatusCode::BAD_REQUEST);
    send(
        &app,
        Method::PATCH,
        &path,
        Some(&admin),
        Some(json!({ "status": "closed" })),
    )
    .await
    .expect(StatusCode::UNPROCESSABLE_ENTITY);

    let missing = format!("/tasks/{}", Uuid::new_v4());
    send(
        &app,
        Method::PATCH,
        &missing,
        Some(&admin),
        Some(json!({ "title": "Missing" })),
    )
    .await
    .expect(StatusCode::NOT_FOUND);
    let tasks = send(&app, Method::GET, "/tasks", Some(&admin), None)
        .await
        .expect(StatusCode::OK)
        .json();
    assert_eq!(tasks, json!([task]));
}

#[sqlx::test]
async fn invalid_assignment_rolls_back_tasks_and_cache_invalidation(db: SqlitePool) {
    let app = seeded_router(&db).await;
    let admin = sign_in(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let james = sign_in(&app, JAMES_EMAIL, JAMES_PASSWORD).await;
    let original = create_task(&app, &admin, "Unassigned task", "low").await;
    let id = original["id"].as_str().unwrap();

    view(&app, &james, false, 0).await;
    for bad_ids in [vec![], vec![id, id], vec!["not-a-uuid"], vec![id; 101]] {
        assign(&app, &admin, &bad_ids, JAMES_EMAIL, StatusCode::BAD_REQUEST).await;
    }
    for assignee in [ADMIN_EMAIL, "unknown@example.com"] {
        assign(&app, &admin, &[id], assignee, StatusCode::NOT_FOUND).await;
    }
    let missing = Uuid::new_v4().to_string();
    assign(
        &app,
        &admin,
        &[id, &missing],
        JAMES_EMAIL,
        StatusCode::NOT_FOUND,
    )
    .await;

    view(&app, &james, true, 0).await;
    let after = send(&app, Method::GET, "/tasks", Some(&admin), None)
        .await
        .expect(StatusCode::OK)
        .json();
    assert_eq!(after, json!([original]));
}

#[sqlx::test]
async fn writes_invalidate_only_the_affected_assignees(db: SqlitePool) {
    let app = seeded_router(&db).await;
    let other_email = "other@example.com";
    sqlx::query(
        "INSERT INTO users (id, full_name, email, hashed_password, role)
         SELECT ?, 'Other Staff', ?, hashed_password, 'staff' FROM users WHERE email = ?",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(other_email)
    .bind(JAMES_EMAIL)
    .execute(&db)
    .await
    .unwrap();
    let admin = sign_in(&app, ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let james = sign_in(&app, JAMES_EMAIL, JAMES_PASSWORD).await;
    let other = sign_in(&app, other_email, JAMES_PASSWORD).await;
    for token in [&admin, &james, &other] {
        view(&app, token, false, 0).await;
    }

    let created = create_task(&app, &admin, "New assignment", "high").await;
    let id = created["id"].as_str().unwrap();
    view(&app, &james, true, 0).await;
    assign(
        &app,
        &admin,
        &[id],
        " JAMESBOND@EXAMPLE.COM ",
        StatusCode::OK,
    )
    .await;
    let assigned = view(&app, &james, false, 1).await;
    assert_eq!(assigned["tasks"][0]["assigned_to"], JAMES_EMAIL);
    view(&app, &other, true, 0).await;

    assign(&app, &admin, &[id], JAMES_EMAIL, StatusCode::OK).await;
    view(&app, &james, true, 1).await;
    let updated = send(
        &app,
        Method::PATCH,
        &format!("/tasks/{id}"),
        Some(&admin),
        Some(json!({ "title": "Updated assignment", "status": "in_progress" })),
    )
    .await
    .expect(StatusCode::OK)
    .json();
    let refreshed = view(&app, &james, false, 1).await;
    assert_eq!(updated["status"], "in_progress");
    assert_eq!(refreshed["tasks"], json!([updated]));

    assign(&app, &admin, &[id], other_email, StatusCode::OK).await;
    view(&app, &james, false, 0).await;
    let reassigned = view(&app, &other, false, 1).await;
    assert_eq!(reassigned["tasks"][0]["id"], id);
    assert_eq!(reassigned["tasks"][0]["assigned_to"], other_email);
    view(&app, &james, true, 0).await;
    view(&app, &other, true, 1).await;
    view(&app, &admin, true, 0).await;
}
