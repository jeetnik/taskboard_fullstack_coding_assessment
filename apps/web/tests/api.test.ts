import assert from "node:assert/strict";
import test from "node:test";
import { api, ApiError, errorMessage, type MyTasks, type TaskDraft } from "../lib/api";

test("login submits credentials and returns a challenge without a token", async (t) => {
  const challenge = { login_challenge_id: "challenge-id", expires_in: 300 };
  const request = t.mock.method(globalThis, "fetch", async (url: RequestInfo | URL, options?: RequestInit) => {
    assert.equal(url, "/api/auth/login");
    assert.equal(options?.method, "POST");
    const headers = new Headers(options?.headers);
    assert.equal(headers.get("Content-Type"), "application/json");
    assert.equal(headers.get("Authorization"), null);
    assert.deepEqual(JSON.parse(String(options?.body)), { email: "admin@example.com", password: "demo-password" });
    return Response.json(challenge);
  });

  assert.deepEqual(await api.login("admin@example.com", "demo-password"), challenge);
  assert.equal(request.mock.callCount(), 1);
});

test("the email lookup encodes its filter and verification preserves a leading zero", async (t) => {
  const id = "challenge+id&test";
  const session = {
    access_token: "test-token",
    user: { id: "user-id", email: "admin@example.com", full_name: "Admin", role: "admin" },
  };
  const request = t.mock.method(globalThis, "fetch", async (url: RequestInfo | URL, options?: RequestInit) => {
    if (options?.method === "GET") {
      assert.equal(url, "/api/dev/email-logs/latest?login_challenge_id=challenge%2Bid%26test");
      return Response.json({ code: "012345" });
    }
    assert.equal(url, "/api/auth/verify-2fa");
    assert.equal(options?.method, "POST");
    assert.deepEqual(JSON.parse(String(options?.body)), { login_challenge_id: id, code: "012345" });
    return Response.json(session);
  });

  const { code } = await api.latestCode(id);
  assert.deepEqual(await api.verify(id, code), session);
  assert.equal(request.mock.callCount(), 2);
});

test("assigned-task refreshes reach the API and retain server cache metadata", async (t) => {
  const controller = new AbortController();
  const base: MyTasks = {
    user: { email: "jamesbond@example.com", role: "staff" },
    tasks: [],
    summary: { total_assigned_tasks: 0 },
    cache: { hit: false },
  };
  let responses = 0;
  const request = t.mock.method(globalThis, "fetch", async (url: RequestInfo | URL, options?: RequestInit) => {
    assert.equal(url, "/api/tasks/view-my-tasks");
    assert.equal(options?.method, "GET");
    assert.equal(options?.cache, "no-store");
    assert.equal(options?.signal, controller.signal);
    assert.equal(new Headers(options?.headers).get("Authorization"), "Bearer staff-token");
    const hit = responses > 0;
    responses += 1;
    return Response.json({ ...base, cache: { hit } });
  });

  assert.deepEqual(await api.myTasks("staff-token", controller.signal), base);
  assert.deepEqual(await api.myTasks("staff-token", controller.signal), { ...base, cache: { hit: true } });
  assert.equal(request.mock.callCount(), 2);
});

test("a forbidden create attempt preserves the API status and message", async (t) => {
  const draft: TaskDraft = { title: "Permission check", description: "", priority: "low" };
  const request = t.mock.method(globalThis, "fetch", async (url: RequestInfo | URL, options?: RequestInit) => {
    assert.equal(url, "/api/tasks");
    assert.equal(options?.method, "POST");
    assert.equal(new Headers(options?.headers).get("Authorization"), "Bearer staff-token");
    assert.deepEqual(JSON.parse(String(options?.body)), draft);
    return Response.json({ error: { code: "forbidden", message: "Only admins can create, assign, or update tasks." } }, { status: 403 });
  });
  await assert.rejects(api.createTask("staff-token", draft), (error) => {
    assert.ok(error instanceof ApiError);
    assert.equal(error.status, 403);
    assert.equal(errorMessage(error), "Only admins can create, assign, or update tasks.");
    return true;
  });
  assert.equal(request.mock.callCount(), 1);
});

test("assignment sends the selected IDs and the assignee's email", async (t) => {
  const assignee = "jamesbond@example.com";
  const taskIds = ["task-1", "task-2", "task-3"];
  const request = t.mock.method(globalThis, "fetch", async (url: RequestInfo | URL, options?: RequestInit) => {
    assert.equal(url, "/api/tasks/assign");
    assert.equal(options?.method, "POST");
    assert.equal(new Headers(options?.headers).get("Authorization"), "Bearer admin-token");
    assert.deepEqual(JSON.parse(String(options?.body)), { task_ids: taskIds, assignee_email: assignee });
    return Response.json({ assigned_count: 3, assigned_to: assignee });
  });
  assert.deepEqual(await api.assignTasks("admin-token", taskIds, assignee), { assigned_count: 3, assigned_to: assignee });
  assert.equal(request.mock.callCount(), 1);
});

test("an HTML proxy error and a connection failure get readable messages", async (t) => {
  const fetch = t.mock.method(globalThis, "fetch", async () => new Response("<h1>Bad gateway</h1>", { status: 502 }));
  await assert.rejects(api.health(), (error) => {
    assert.ok(error instanceof ApiError);
    assert.equal(error.status, 502);
    assert.equal(errorMessage(error), "The API is unavailable. Try again.");
    return true;
  });

  fetch.mock.mockImplementation(async () => { throw new TypeError("Failed to fetch"); });
  await assert.rejects(api.health(), (error) => {
    assert.match(errorMessage(error), /Check that it’s running/);
    return true;
  });
});
