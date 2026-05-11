import type { Project } from "./types";

export function statusLabel(status: Project["gitStatus"]) {
  return status.replace(/_/g, " ");
}

export function divergenceLabel(project: Project) {
  return `${project.aheadCount} ahead / ${project.behindCount} behind`;
}

export function projectUpdateTitle(project: Project) {
  switch (project.gitStatus) {
    case "up_to_date":
      return "Up to date";
    case "behind":
      return project.behindCount > 0 ? `${project.behindCount} behind` : "Behind";
    case "ahead":
      return project.aheadCount > 0 ? `${project.aheadCount} ahead` : "Ahead";
    case "diverged":
      return "Diverged";
    case "dirty":
      return "Dirty worktree";
    case "no_upstream":
      return "No upstream";
    case "detached":
      return "Detached head";
    case "fetch_failed":
      return "Fetch failed";
    default:
      return "Unknown";
  }
}

export function projectUpdateDetail(project: Project) {
  switch (project.gitStatus) {
    case "up_to_date":
      return "Synced with remote";
    case "behind":
      return "Pull available";
    case "ahead":
      return "Local commits pending";
    case "diverged":
      return divergenceLabel(project);
    case "dirty":
      return "Local changes present";
    case "no_upstream":
      return "Tracking branch missing";
    case "detached":
      return "Branch not checked out";
    case "fetch_failed":
      return "Refresh failed";
    default:
      return "Status not checked";
  }
}

let markedParse: ((source: string) => string) | null = null;

export async function renderMarkdown(source: string): Promise<string> {
  if (!markedParse) {
    const m = await import("marked");
    markedParse = (s) => m.marked.parse(s, { async: false }) as string;
  }
  return markedParse(source);
}
