use std::collections::HashSet;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{Admin, CurrentUser},
    error::ApiError,
    model::{Priority, Role, Status, TASK_SELECT, Task},
};

#[derive(Deserialize)]
pub struct CreateTask {
    title: String,
    #[serde(default)]
    description: String,
    priority: Priority,
}

#[derive(Deserialize)]
pub struct UpdateTask {
    title: Option<String>,
    description: Option<String>,
    status: Option<Status>,
    priority: Option<Priority>,
}

#[derive(Deserialize)]
pub struct AssignTasks {
    task_ids: Vec<String>,
    assignee_email: String,
}

#[derive(Serialize)]
pub struct Assignment {
    assigned_count: usize,
    assigned_to: String,
}

pub struct CachedTasks {
    version: i64,
    tasks: Vec<Task>,
}

#[derive(Serialize)]
pub struct MyTasks {
    user: UserSummary,
    tasks: Vec<Task>,
    summary: TaskSummary,
    cache: CacheMetadata,
}

#[derive(Serialize)]
struct UserSummary {
    email: String,
    role: Role,
}

#[derive(Serialize)]
struct TaskSummary {
    total_assigned_tasks: usize,
}

#[derive(Serialize)]
struct CacheMetadata {
    hit: bool,
}

pub async fn list(
    State(app): State<AppState>,
    Admin(_): Admin,
) -> Result<Json<Vec<Task>>, ApiError> {
    let tasks = sqlx::query_as::<_, Task>(&format!("{TASK_SELECT} ORDER BY t.created_at, t.rowid"))
        .fetch_all(&app.db)
        .await?;
    Ok(Json(tasks))
}

pub async fn create(
    State(app): State<AppState>,
    Admin(user): Admin,
    Json(input): Json<CreateTask>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    let title = valid_title(&input.title)?;
    let description = valid_description(&input.description)?;
    let id = Uuid::new_v4().to_string();

    let task = sqlx::query_as::<_, Task>(
        "INSERT INTO tasks (id, title, description, priority, created_by_id) VALUES (?, ?, ?, ?, ?)
         RETURNING id, title, description, status, priority, NULL AS assigned_to",
    )
    .bind(id)
    .bind(title)
    .bind(description)
    .bind(input.priority)
    .bind(user.id)
    .fetch_one(&app.db)
    .await?;
    Ok((StatusCode::CREATED, Json(task)))
}

pub async fn update(
    State(app): State<AppState>,
    Admin(_): Admin,
    Path(id): Path<String>,
    Json(input): Json<UpdateTask>,
) -> Result<Json<Task>, ApiError> {
    if input.title.is_none()
        && input.description.is_none()
        && input.status.is_none()
        && input.priority.is_none()
    {
        return Err(ApiError::bad_request(
            "Provide at least one task field to update.",
        ));
    }
    let title = input.title.as_deref().map(valid_title).transpose()?;
    let description = input
        .description
        .as_deref()
        .map(valid_description)
        .transpose()?;

    let task = sqlx::query_as::<_, Task>(
        "UPDATE tasks SET title = COALESCE(?, title), description = COALESCE(?, description),
         status = COALESCE(?, status), priority = COALESCE(?, priority), updated_at = CURRENT_TIMESTAMP
         WHERE id = ?
         RETURNING id, title, description, status, priority,
             (SELECT email FROM users WHERE id = tasks.assigned_to_id) AS assigned_to",
    )
    .bind(title)
    .bind(description)
    .bind(input.status)
    .bind(input.priority)
    .bind(id)
    .fetch_optional(&app.db)
    .await?
    .ok_or_else(|| ApiError::not_found("Task not found."))?;

    Ok(Json(task))
}

pub async fn assign(
    State(app): State<AppState>,
    Admin(_): Admin,
    Json(input): Json<AssignTasks>,
) -> Result<Json<Assignment>, ApiError> {
    let task_count = input.task_ids.len();
    if !(1..=100).contains(&task_count) {
        return Err(ApiError::bad_request("Select between 1 and 100 tasks."));
    }
    let unique_ids: HashSet<_> = input.task_ids.iter().collect();
    if unique_ids.len() != task_count
        || input.task_ids.iter().any(|id| Uuid::parse_str(id).is_err())
    {
        return Err(ApiError::bad_request("Task IDs must be valid and unique."));
    }

    let mut transaction = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let (assignee_id, email) = sqlx::query_as::<_, (String, String)>(
        "SELECT id, email FROM users WHERE email = ? AND role = 'staff'",
    )
    .bind(input.assignee_email.trim())
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| ApiError::not_found("Staff assignee not found."))?;

    for id in input.task_ids {
        let result = sqlx::query(
            "UPDATE tasks SET assigned_to_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(&assignee_id)
        .bind(id)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 0 {
            return Err(ApiError::not_found(
                "One of the selected tasks was not found.",
            ));
        }
    }
    transaction.commit().await?;

    Ok(Json(Assignment {
        assigned_count: task_count,
        assigned_to: email,
    }))
}

pub async fn view_my_tasks(
    State(app): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> Result<Json<MyTasks>, ApiError> {
    let version = user.task_list_version;
    let cached = app
        .task_cache
        .read()
        .await
        .get(&user.id)
        .filter(|entry| entry.version == version)
        .map(|entry| entry.tasks.clone());
    let hit = cached.is_some();
    let tasks = match cached {
        Some(tasks) => tasks,
        None => {
            let tasks = sqlx::query_as::<_, Task>(&format!(
                "{TASK_SELECT} WHERE t.assigned_to_id = ? ORDER BY t.created_at, t.rowid"
            ))
            .bind(&user.id)
            .fetch_all(&app.db)
            .await?;
            let mut cache = app.task_cache.write().await;
            // A read that overlaps an update must never replace a newer cached snapshot.
            if cache
                .get(&user.id)
                .is_none_or(|entry| entry.version <= version)
            {
                cache.insert(
                    user.id,
                    CachedTasks {
                        version,
                        tasks: tasks.clone(),
                    },
                );
            }
            tasks
        }
    };

    Ok(Json(MyTasks {
        user: UserSummary {
            email: user.email,
            role: user.role,
        },
        summary: TaskSummary {
            total_assigned_tasks: tasks.len(),
        },
        cache: CacheMetadata { hit },
        tasks,
    }))
}

fn valid_title(title: &str) -> Result<&str, ApiError> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 120 {
        return Err(ApiError::bad_request(
            "Task title must be between 1 and 120 characters.",
        ));
    }
    Ok(title)
}

fn valid_description(description: &str) -> Result<&str, ApiError> {
    let description = description.trim();
    if description.chars().count() > 1_000 {
        return Err(ApiError::bad_request(
            "Task description must be at most 1,000 characters.",
        ));
    }
    Ok(description)
}
