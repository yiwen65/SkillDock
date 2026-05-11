import React from "react";
import type { TaskRecord, UserConfig } from "./types";

export type ThemePreference = UserConfig["uiPreferences"]["theme"];

export const views = ["Skills", "Projects", "Agents", "Logs", "Settings"] as const;
export type ViewName = (typeof views)[number];

type ThemeTarget = {
  dataset: Record<string, string | undefined>;
  style: { colorScheme: string };
};

export function applyThemePreference(
  theme: ThemePreference,
  root: ThemeTarget = document.documentElement,
) {
  if (theme === "system") {
    delete root.dataset.theme;
    root.style.colorScheme = "";
    return;
  }
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
}

export function isTerminalTaskStatus(status: TaskRecord["status"]) {
  return !["queued", "running"].includes(status);
}

export function mergeTaskRecords(incoming: TaskRecord[], existing: TaskRecord[]) {
  const records = new Map<string, TaskRecord>();
  for (const task of incoming) {
    records.set(task.id, task);
  }
  for (const task of existing) {
    if (records.has(task.id)) {
      const next = records.get(task.id)!;
      records.set(task.id, {
        ...next,
        stdout: next.stdout || task.stdout,
        stderr: next.stderr || task.stderr,
      });
    } else {
      records.set(task.id, task);
    }
  }
  return Array.from(records.values()).slice(0, 100);
}

export function preserveLogs(current: TaskRecord, incoming: TaskRecord): TaskRecord {
  if (current.id !== incoming.id) {
    return incoming;
  }
  return {
    ...incoming,
    stdout: incoming.stdout || current.stdout,
    stderr: incoming.stderr || current.stderr,
  };
}

export function errorMessage(error: unknown) {
  if (error && typeof error === "object" && "message" in error) {
    return String(error.message);
  }
  return error instanceof Error ? error.message : String(error);
}

export function StatusPanel({
  body,
  title,
  tone = "neutral",
}: {
  body: string;
  title: string;
  tone?: "neutral" | "error";
}) {
  return (
    <article className={tone === "error" ? "status-panel error" : "status-panel"}>
      <h2>{title}</h2>
      <p>{body}</p>
    </article>
  );
}

export function EmptyState({ body, title }: { body: string; title: string }) {
  return <StatusPanel body={body} title={title} tone="neutral" />;
}

export function PanelHeader({ detail, title }: { detail: string; title: string }) {
  return (
    <header className="panel-header">
      <h2>{title}</h2>
      <span>{detail}</span>
    </header>
  );
}

export const SummaryMetric = React.memo(function SummaryMetric({
  label,
  value,
}: {
  label: string;
  value: number;
}) {
  return (
    <article className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </article>
  );
});
