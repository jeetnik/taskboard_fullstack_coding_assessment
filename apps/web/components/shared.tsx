import type { ReactNode } from "react";
import type { Task, User } from "../lib/api";

export function Brand() {
  return <div className="brand"><span className="brand-icon" aria-hidden="true"><i /><i /><i /></span> Taskboard</div>;
}

export function Notice({ kind, children }: { kind: "error" | "success"; children: ReactNode }) {
  return <div className={`notice notice-${kind}`} role={kind === "error" ? "alert" : "status"}>{children}</div>;
}

export function EmptyState({ title, children }: { title: string; children: ReactNode }) {
  return <div className="empty-state"><div className="empty-icon" aria-hidden="true"><BrandGlyph /></div><strong>{title}</strong><p>{children}</p></div>;
}

function BrandGlyph() {
  return <span className="empty-lines"><i /><i /><i /></span>;
}

export function Workspace({ user, onSignOut, children }: { user: User; onSignOut: () => void; children: ReactNode }) {
  const initials = user.full_name.trim().split(/\s+/).slice(0, 2).map((part) => part[0]).join("").toUpperCase();
  return (
    <div className="workspace">
      <aside className="sidebar">
        <Brand />
        <div className="sidebar-nav">
          <span className="sidebar-label">Workspace</span>
          <span className="sidebar-current" aria-current="page"><span className="nav-glyph" aria-hidden="true" />{user.role === "admin" ? "Manage tasks" : "My tasks"}</span>
        </div>
        <div className="sidebar-account">
          <div className="account">
            <span className="avatar" aria-hidden="true">{initials}</span>
            <div><strong>{user.full_name}</strong><span>{user.role === "admin" ? "Administrator" : "Staff member"}</span></div>
          </div>
          <button className="button button-quiet signout" onClick={onSignOut}>Sign out</button>
        </div>
      </aside>
      <main className="workspace-main">{children}</main>
    </div>
  );
}

export function TaskTable({
  tasks,
  selection,
}: {
  tasks: Task[];
  selection?: { ids: Set<string>; toggle: (id: string) => void; disabled: boolean; assignee: string };
}) {
  return (
    <div className="table-scroll">
      <table className="task-table">
        <thead>
          <tr>{selection && <th scope="col" className="checkbox-cell"><span className="sr-only">Select task</span></th>}<th scope="col">Task</th><th scope="col">Priority</th><th scope="col">Status</th>{selection && <th scope="col">Assignee</th>}</tr>
        </thead>
        <tbody>
          {tasks.map((task) => (
            <tr key={task.id} className={selection?.ids.has(task.id) ? "selected-row" : undefined}>
              {selection && <td className="checkbox-cell"><input
                type="checkbox"
                aria-label={`Select ${task.title}`}
                checked={selection.ids.has(task.id)}
                disabled={selection.disabled || task.assigned_to === selection.assignee}
                onChange={() => selection.toggle(task.id)}
              /></td>}
              <th scope="row"><span className="task-title">{task.title}</span>{task.description && <span className="task-description">{task.description}</span>}</th>
              <td><span className={`priority priority-${task.priority}`}><span aria-hidden="true" />{task.priority}</span></td>
              <td><span className={`task-status status-${task.status}`}>{task.status === "in_progress" ? "In progress" : task.status === "done" ? "Done" : "To do"}</span></td>
              {selection && <td className="assignee">{task.assigned_to === selection.assignee ? "James Bond" : task.assigned_to ?? "Unassigned"}</td>}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
