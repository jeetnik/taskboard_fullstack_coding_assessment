CREATE TABLE users (
    id TEXT PRIMARY KEY NOT NULL,
    full_name TEXT NOT NULL,
    email TEXT NOT NULL UNIQUE COLLATE NOCASE,
    hashed_password TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'staff')),
    task_list_version INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'todo' CHECK (status IN ('todo', 'in_progress', 'done')),
    priority TEXT NOT NULL CHECK (priority IN ('high', 'medium', 'low')),
    created_by_id TEXT NOT NULL REFERENCES users(id),
    assigned_to_id TEXT REFERENCES users(id),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX tasks_assignee ON tasks (assigned_to_id, created_at, id);

CREATE TABLE login_challenges (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id),
    code_digest BLOB NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at INTEGER,
    attempts INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX login_challenges_expiration ON login_challenges (expires_at);

CREATE TRIGGER invalidate_assignee_task_lists
AFTER UPDATE OF assigned_to_id, title, description, status, priority ON tasks
FOR EACH ROW
WHEN OLD.assigned_to_id IS NOT NEW.assigned_to_id
  OR OLD.title IS NOT NEW.title
  OR OLD.description IS NOT NEW.description
  OR OLD.status IS NOT NEW.status
  OR OLD.priority IS NOT NEW.priority
BEGIN
    UPDATE users
    SET task_list_version = task_list_version + 1
    WHERE id IN (OLD.assigned_to_id, NEW.assigned_to_id);
END;
