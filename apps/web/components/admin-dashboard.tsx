"use client";

import { useCallback, useEffect, useState, type FormEvent } from "react";
import { api, ApiError, errorMessage, type Priority, type Task, type TaskDraft } from "../lib/api";
import { EmptyState, Notice, TaskTable } from "./shared";

const assignee = "jamesbond@example.com";
const blank: TaskDraft = { title: "", description: "", priority: "medium" };

export function AdminDashboard({ token, onExpired }: { token: string; onExpired: () => void }) {
  const [tasks, setTasks] = useState<Task[] | null>(null);
  const [draft, setDraft] = useState<TaskDraft>(blank);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(true);
  const [mutation, setMutation] = useState<"create" | "assign" | null>(null);
  const [error, setError] = useState("");
  const [feedback, setFeedback] = useState("");

  const reportError = useCallback((error: unknown) => {
    if (error instanceof ApiError && error.status === 401) onExpired();
    else setError(errorMessage(error));
  }, [onExpired]);

  const loadTasks = useCallback(async (signal?: AbortSignal) => {
    setLoading(true);
    setError("");
    try {
      const result = await api.listTasks(token, signal);
      if (signal?.aborted) return;
      setTasks(result);
      const assignable = new Set(result.filter((task) => task.assigned_to !== assignee).map((task) => task.id));
      setSelected((previous) => new Set([...previous].filter((id) => assignable.has(id))));
    } catch (error) {
      if (!signal?.aborted) reportError(error);
    } finally {
      if (!signal?.aborted) setLoading(false);
    }
  }, [token, reportError]);

  useEffect(() => {
    const controller = new AbortController();
    void loadTasks(controller.signal);
    return () => controller.abort();
  }, [loadTasks]);

  async function createTask(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setMutation("create");
    setError("");
    setFeedback("");
    try {
      const task = await api.createTask(token, draft);
      setTasks((current) => [...(current ?? []), task]);
      setDraft(blank);
      setFeedback(`Created “${task.title}”.`);
    } catch (error) {
      reportError(error);
    } finally {
      setMutation(null);
    }
  }

  function toggleTask(id: string) {
    setSelected((previous) => {
      const next = new Set(previous);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  async function assignSelected() {
    setMutation("assign");
    setError("");
    setFeedback("");
    try {
      const result = await api.assignTasks(token, [...selected], assignee);
      setTasks((current) => current?.map((task) => selected.has(task.id) ? { ...task, assigned_to: result.assigned_to } : task) ?? null);
      setSelected(new Set());
      setFeedback(`Assigned ${result.assigned_count} ${result.assigned_count === 1 ? "task" : "tasks"} to James Bond.`);
    } catch (error) {
      reportError(error);
    } finally {
      setMutation(null);
    }
  }

  const busy = loading || mutation !== null;
  const assignedCount = tasks?.filter((task) => task.assigned_to === assignee).length;

  return (
    <div className="dashboard">
      <header className="dashboard-header">
        <div><h1>Manage tasks</h1><p>Create tasks and assign work to James Bond.</p></div>
      </header>
      <div className="metrics">
        <div className="metric"><span>Total tasks</span><strong>{tasks?.length ?? "–"}</strong></div>
        <div className="metric"><span>Assigned to James Bond</span><strong>{assignedCount ?? "–"}</strong></div>
        <div className="metric"><span>Unassigned</span><strong>{tasks ? tasks.filter((task) => !task.assigned_to).length : "–"}</strong></div>
      </div>
      {error && <Notice kind="error">{error}</Notice>}
      {feedback && <Notice kind="success">{feedback}</Notice>}
      <div className="admin-grid">
        <section className="card create-card" aria-labelledby="create-heading">
          <div className="card-heading"><h2 id="create-heading">New task</h2><p>Choose a title and priority.</p></div>
          <form className="form-stack" onSubmit={createTask}>
            <label className="field">Title
              <input required maxLength={120} disabled={mutation === "create"} value={draft.title}
                onChange={(event) => setDraft({ ...draft, title: event.target.value })} placeholder="e.g. Review project brief" />
            </label>
            <label className="field">Description <span className="optional">Optional</span>
              <textarea maxLength={1000} disabled={mutation === "create"} value={draft.description}
                onChange={(event) => setDraft({ ...draft, description: event.target.value })} placeholder="Add details for the assignee" rows={3} />
            </label>
            <label className="field">Priority
              <select value={draft.priority} disabled={mutation === "create"} onChange={(event) => setDraft({ ...draft, priority: event.target.value as Priority })}>
                <option value="high">High</option><option value="medium">Medium</option><option value="low">Low</option>
              </select>
            </label>
            <button className="button button-primary" disabled={busy || tasks === null || !draft.title.trim()}>
              {mutation === "create" ? "Creating…" : "Create task"}
            </button>
          </form>
        </section>
        <section className="card list-card" aria-labelledby="all-tasks-heading">
          <div className="card-heading card-toolbar">
            <div><h2 id="all-tasks-heading">All tasks</h2><p>Select tasks to assign them to James Bond.</p></div>
            <button className="button button-quiet" disabled={busy} onClick={() => void loadTasks()}>{loading ? "Loading…" : "Refresh"}</button>
          </div>
          {loading && tasks === null && <div className="loading-state" role="status">Loading tasks…</div>}
          {tasks?.length === 0 && <EmptyState title="No tasks yet">Create your first task using the form.</EmptyState>}
          {!!tasks?.length && <TaskTable tasks={tasks} selection={{ ids: selected, toggle: toggleTask, disabled: busy, assignee }} />}
          <div className="card-footer">
            <span aria-live="polite">{selected.size} selected</span>
            <button className="button button-secondary" disabled={busy || selected.size === 0 || selected.size > 100} onClick={assignSelected}>
              {mutation === "assign" ? "Assigning…" : "Assign to James Bond"}
            </button>
          </div>
          {selected.size > 100 && <p className="selection-limit" role="status">Select up to 100 tasks per assignment.</p>}
        </section>
      </div>
    </div>
  );
}
