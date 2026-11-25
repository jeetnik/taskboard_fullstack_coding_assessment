use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Deserialize, PartialEq, Serialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Staff,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub full_name: String,
    pub email: String,
    pub role: Role,
    #[serde(skip)]
    pub task_list_version: i64,
}

#[derive(Clone, Copy, Deserialize, Serialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Priority {
    High,
    Medium,
    Low,
}

#[derive(Clone, Copy, Deserialize, Serialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Status {
    Todo,
    InProgress,
    Done,
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: Status,
    pub priority: Priority,
    pub assigned_to: Option<String>,
}

pub const USER_FIELDS: &str = "id, full_name, email, role, task_list_version";

pub const TASK_SELECT: &str = "
    SELECT t.id, t.title, t.description, t.status, t.priority, u.email AS assigned_to
    FROM tasks t
    LEFT JOIN users u ON u.id = t.assigned_to_id
";
