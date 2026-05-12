import { openWorkspacePath } from "./commands";
import { errorMessage } from "./shared";

export type CopyPathResult = "copied" | "prompted";

export async function copyTextWithFallback(text: string): Promise<CopyPathResult> {
  if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return "copied";
  }

  if (typeof window !== "undefined" && window.prompt) {
    window.prompt("Copy path", text);
    return "prompted";
  }

  throw new Error("Clipboard is unavailable.");
}

export async function openWorkspacePathWithCopyFallback({
  copyText = copyTextWithFallback,
  label,
  openPath = openWorkspacePath,
  path,
  workspaceRoot,
}: {
  copyText?: (text: string) => Promise<CopyPathResult>;
  label: string;
  openPath?: (workspaceRoot: string, path: string) => Promise<void>;
  path: string;
  workspaceRoot: string;
}): Promise<string> {
  try {
    await openPath(workspaceRoot, path);
    return `Opening ${label}.`;
  } catch (openError) {
    try {
      const copyResult = await copyText(path);
      const fallback =
        copyResult === "copied" ? "path copied instead" : "copy path fallback opened";
      return `Could not open ${label}; ${fallback}. ${errorMessage(openError)}`;
    } catch {
      return errorMessage(openError);
    }
  }
}
