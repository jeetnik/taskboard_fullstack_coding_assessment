import assert from "node:assert/strict";

const origin = (process.env.VALIDATION_ORIGIN ?? "http://127.0.0.1:4000").replace(/\/$/, "");
const adminEmail = "admin@example.com";
const jamesEmail = "jamesbond@example.com";

async function request(path, { method = "GET", body, token, expect = 200 } = {}) {
  const headers = new Headers({ Accept: "application/json" });
  if (body !== undefined) headers.set("Content-Type", "application/json");
  if (token) headers.set("Authorization", `Bearer ${token}`);

  const response = await fetch(`${origin}${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(10_000),
  });
  const text = await response.text();
  assert.equal(response.status, expect, `${method} ${path} returned ${response.status}: ${text}`);
  return JSON.parse(text);
}

async function signIn(email, password, role) {
  const login = await request("/auth/login", { method: "POST", body: { email, password } });
  assert.deepEqual(Object.keys(login).sort(), ["expires_in", "login_challenge_id"]);
  assert.equal(login.expires_in, 300);
  assert.ok(login.login_challenge_id);

  const { code } = await request(`/dev/email-logs/latest?login_challenge_id=${encodeURIComponent(login.login_challenge_id)}`);
  assert.match(code, /^\d{6}$/);

  const verification = { login_challenge_id: login.login_challenge_id, code };
  const wrong = { ...verification, code: code === "000000" ? "111111" : "000000" };
  await request("/auth/verify-2fa", { method: "POST", body: wrong, expect: 401 });
  const { access_token, user } = await request("/auth/verify-2fa", { method: "POST", body: verification });
  assert.equal(user.email, email);
  assert.equal(user.role, role);
  assert.match(access_token, /^[\w-]+\.[\w-]+\.[\w-]+$/);
  await request("/auth/verify-2fa", { method: "POST", body: verification, expect: 401 });
  return access_token;
}

async function main() {
  const health = await request("/health");
  assert.equal(health.status, "ok");
  assert.equal(health.dev_mode, true, "Run the API in development mode.");
  const users = await request("/seed/users", { method: "POST" });
  assert.deepEqual(users.map(({ email, role }) => ({ email, role })), [
    { email: adminEmail, role: "admin" },
    { email: jamesEmail, role: "staff" },
  ]);

  await request("/tasks/view-my-tasks", { expect: 401 });
  await request("/auth/login", { method: "POST", body: { email: adminEmail, password: "wrong-password" }, expect: 401 });
  const admin = await signIn(adminEmail, "AdminPass123!", "admin");
  assert.equal((await request("/tasks", { token: admin })).length, 0, "Use a fresh SQLite database for this validation run.");

  const drafts = [
    { title: "Review project brief", priority: "high" },
    { title: "Confirm delivery owners", priority: "medium" },
    { title: "Organize reference notes", priority: "low" },
    { title: "Schedule the review", priority: "medium" },
    { title: "Prepare status report", priority: "low" },
  ];
  const created = [];
  for (const draft of drafts) {
    const task = await request("/tasks", { method: "POST", body: draft, token: admin, expect: 201 });
    assert.equal(task.title, draft.title);
    assert.equal(task.priority, draft.priority);
    assert.equal(task.status, "todo");
    assert.equal(task.assigned_to, null);
    created.push(task);
  }

  const assigned = created.slice(0, 3);
  const assignment = { task_ids: assigned.map((task) => task.id), assignee_email: jamesEmail };
  assert.deepEqual(await request("/tasks/assign", { method: "POST", body: assignment, token: admin }), {
    assigned_count: 3,
    assigned_to: jamesEmail,
  });

  const james = await signIn(jamesEmail, "JamesBond123!", "staff");
  const forbidden = await request("/tasks", { method: "POST", body: { title: "Forbidden test", priority: "low" }, token: james, expect: 403 });
  assert.equal(forbidden.error.code, "forbidden");
  await request("/tasks/assign", { method: "POST", body: assignment, token: james, expect: 403 });
  await request("/tasks", { token: james, expect: 403 });

  const first = await request("/tasks/view-my-tasks", { token: james });
  assert.deepEqual(first, {
    user: { email: jamesEmail, role: "staff" },
    tasks: assigned.map((task) => ({ ...task, assigned_to: jamesEmail })),
    summary: { total_assigned_tasks: 3 },
    cache: { hit: false },
  });
  const second = await request("/tasks/view-my-tasks", { token: james });
  assert.deepEqual(second, { ...first, cache: { hit: true } });
  assert.equal((await request("/tasks", { token: admin })).length, 5);

  console.log(JSON.stringify({ first, second }, null, 2));
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
