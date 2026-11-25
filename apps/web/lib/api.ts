export type Role = "admin" | "staff";
export type Priority = "high" | "medium" | "low";
export type TaskStatus = "todo" | "in_progress" | "done";

export interface User {
  id: string;
  full_name: string;
  email: string;
  role: Role;
}

export interface Task {
  id: string;
  title: string;
  description: string;
  status: TaskStatus;
  priority: Priority;
  assigned_to: string | null;
}

export interface TaskDraft {
  title: string;
  description: string;
  priority: Priority;
}

export interface MyTasks {
  user: Pick<User, "email" | "role">;
  tasks: Task[];
  summary: { total_assigned_tasks: number };
  cache: { hit: boolean };
}

export interface Session {
  access_token: string;
  user: User;
}

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
    this.name = "ApiError";
  }
}

interface RequestOptions {
  method?: "GET" | "POST";
  body?: unknown;
  token?: string;
  signal?: AbortSignal;
}

async function request<T>(path: string, { method = "GET", body, token, signal }: RequestOptions = {}): Promise<T> {
  const headers = new Headers({ Accept: "application/json" });
  if (body !== undefined) headers.set("Content-Type", "application/json");
  if (token) headers.set("Authorization", `Bearer ${token}`);

  const response = await fetch(`/api${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    signal,
    cache: "no-store",
  });

  if (!response.ok) {
    const fallback = response.status >= 500 ? "The API is unavailable. Try again." : `Request failed (${response.status}).`;
    const json = response.headers.get("content-type")?.includes("application/json")
      ? await response.json().catch(() => null)
      : null;
    throw new ApiError(response.status, json?.error?.message ?? fallback);
  }
  return response.json();
}

export const api = {
  health: (signal?: AbortSignal) => request<{ dev_mode: boolean }>("/health", { signal }),
  seedUsers: () => request<User[]>("/seed/users", { method: "POST" }),
  login: (email: string, password: string) =>
    request<{ login_challenge_id: string; expires_in: number }>("/auth/login", {
      method: "POST",
      body: { email, password },
    }),
  latestCode: (challengeId: string) =>
    request<{ code: string }>(`/dev/email-logs/latest?login_challenge_id=${encodeURIComponent(challengeId)}`),
  verify: (challengeId: string, code: string) =>
    request<Session>("/auth/verify-2fa", { method: "POST", body: { login_challenge_id: challengeId, code } }),
  listTasks: (token: string, signal?: AbortSignal) => request<Task[]>("/tasks", { token, signal }),
  createTask: (token: string, draft: TaskDraft) => request<Task>("/tasks", { method: "POST", body: draft, token }),
  assignTasks: (token: string, taskIds: string[], assigneeEmail: string) =>
    request<{ assigned_count: number; assigned_to: string }>("/tasks/assign", {
      method: "POST",
      body: { task_ids: taskIds, assignee_email: assigneeEmail },
      token,
    }),
  myTasks: (token: string, signal?: AbortSignal) => request<MyTasks>("/tasks/view-my-tasks", { token, signal }),
};

export function errorMessage(error: unknown): string {
  return error instanceof ApiError ? error.message : "Couldn’t reach the API. Check that it’s running and try again.";
}
