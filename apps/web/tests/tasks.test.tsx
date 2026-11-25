import assert from "node:assert/strict";
import test from "node:test";
import { renderToStaticMarkup } from "react-dom/server";
import { EmptyState, TaskTable } from "../components/shared";
import type { Task } from "../lib/api";

const assignee = "jamesbond@example.com";
const tasks: Task[] = [
  { id: "first", title: "Review the brief", description: "Draft notes", status: "todo", priority: "high", assigned_to: assignee },
  { id: "second", title: "Confirm owners", description: "", status: "in_progress", priority: "medium", assigned_to: assignee },
  { id: "third", title: "Archive <draft>", description: "", status: "done", priority: "low", assigned_to: assignee },
];

test("the staff table renders exactly the supplied tasks without admin controls", () => {
  const markup = renderToStaticMarkup(<TaskTable tasks={tasks} />);
  assert.equal((markup.match(/scope="row"/g) ?? []).length, 3);
  for (const text of ["Review the brief", "Draft notes", "Confirm owners", "Archive &lt;draft&gt;", "In progress", "Done"]) {
    assert.ok(markup.includes(text), text);
  }
  assert.doesNotMatch(markup, /type="checkbox"|>Assignee</);
});

test("the admin table keeps assigned rows disabled and selected rows checked", () => {
  const rows = [tasks[0], { ...tasks[1], assigned_to: null }];
  const markup = renderToStaticMarkup(<TaskTable
    tasks={rows}
    selection={{ ids: new Set(["second"]), assignee, disabled: false, toggle: () => {} }}
  />);
  assert.match(markup, /aria-label="Select Review the brief" disabled=""/);
  assert.match(markup, /aria-label="Select Confirm owners" checked=""/);
  assert.match(markup, />James Bond</);
  assert.match(markup, />Unassigned</);
});

test("an empty result can display the empty state without task rows", () => {
  const markup = renderToStaticMarkup(<EmptyState title="No assignments yet">Tasks will appear here once assigned.</EmptyState>);
  assert.match(markup, /No assignments yet/);
  assert.match(markup, /Tasks will appear here once assigned/);
  assert.doesNotMatch(markup, /<tr[ >]/);
});
