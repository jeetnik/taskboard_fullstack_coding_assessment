"use client";

import { useCallback, useEffect, useState } from "react";
import { api, ApiError, errorMessage, type MyTasks } from "../lib/api";
import { EmptyState, Notice, TaskTable } from "./shared";

export function StaffDashboard({ token, onExpired }: { token: string; onExpired: () => void }) {
  const [data, setData] = useState<MyTasks | null>(null);
  const [loading, setLoading] = useState(true);
  const [checking, setChecking] = useState(false);
  const [loadError, setLoadError] = useState("");
  const [permissionError, setPermissionError] = useState("");

  const loadTasks = useCallback(async (signal?: AbortSignal) => {
    setLoading(true);
    setLoadError("");
    try {
      const result = await api.myTasks(token, signal);
      if (!signal?.aborted) setData(result);
    } catch (error) {
      if (!signal?.aborted) {
        if (error instanceof ApiError && error.status === 401) onExpired();
        else setLoadError(errorMessage(error));
      }
    } finally {
      if (!signal?.aborted) setLoading(false);
    }
  }, [token, onExpired]);

  useEffect(() => {
    const controller = new AbortController();
    void loadTasks(controller.signal);
    return () => controller.abort();
  }, [loadTasks]);

  async function checkCreatePermission() {
    setChecking(true);
    setPermissionError("");
    try {
      await api.createTask(token, { title: "Staff permission check", description: "", priority: "low" });
      setPermissionError("The API unexpectedly allowed a staff account to create a task.");
    } catch (error) {
      if (error instanceof ApiError && error.status === 403) {
        setPermissionError("403 Forbidden. Staff members cannot create tasks. Only administrators can create them.");
      } else if (error instanceof ApiError && error.status === 401) {
        onExpired();
      } else {
        setPermissionError(errorMessage(error));
      }
    } finally {
      setChecking(false);
    }
  }

  return (
    <div className="dashboard staff-dashboard">
      <header className="dashboard-header">
        <div><h1>My tasks</h1><p>View the tasks assigned to you.</p></div>
        <button className="button button-secondary" disabled={checking} onClick={checkCreatePermission}>
          {checking ? "Checking…" : "Try creating a task"}
        </button>
      </header>
      {permissionError && <Notice kind="error">{permissionError}</Notice>}
      {loadError && <Notice kind="error">{loadError}</Notice>}
      <section className="card list-card" aria-labelledby="assigned-heading">
        <div className="card-heading card-toolbar">
          <div>
            <h2 id="assigned-heading">Assigned to me</h2>
            {data && <p>{data.summary.total_assigned_tasks} assigned {data.summary.total_assigned_tasks === 1 ? "task" : "tasks"}</p>}
          </div>
          <div className="refresh-group">
            {data && !loadError && <span className={`cache-pill ${data.cache.hit ? "cache-hit" : ""}`} role="status">
              {data.cache.hit ? "Cache hit" : "Loaded from database"}
            </span>}
            <button className="button button-secondary" disabled={loading} onClick={() => void loadTasks()}>{loading ? "Loading…" : "Refresh"}</button>
          </div>
        </div>
        {loading && !data && <div className="loading-state" role="status">Loading your tasks…</div>}
        {data?.tasks.length === 0 && <EmptyState title="No assignments yet">Tasks will appear here when an administrator assigns them to you.</EmptyState>}
        {!!data?.tasks.length && <TaskTable tasks={data.tasks} />}
      </section>
    </div>
  );
}
