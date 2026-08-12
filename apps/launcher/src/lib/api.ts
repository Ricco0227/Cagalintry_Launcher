/**
 * Typed wrappers over the Rust command surface.
 *
 * Every shape here mirrors a `#[derive(Serialize)]` type in `src-tauri`. They
 * are hand-written rather than generated, so when one changes on the Rust side
 * the matching change belongs here in the same commit.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type LoaderKind = "vanilla" | "fabric" | "quilt" | "neoforge";

export interface LoaderSpec {
  kind: LoaderKind;
  version?: string;
}

export interface PackLink {
  packId: string;
  installedRevision: number;
}

export interface Instance {
  id: string;
  name: string;
  mcVersion: string;
  loader: LoaderSpec;
  createdAt: string;
  lastPlayed?: string;
  maxMemoryMb: number;
  javaPath?: string;
  extraJvmArgs: string[];
  pack?: PackLink;
}

/**
 * What the primary button should do. Resolved in Rust so the ordering rules
 * (linking beats updating, busy beats everything) live in exactly one place.
 */
export type PrimaryAction =
  | { kind: "linkMinecraft" }
  | { kind: "busy" }
  | { kind: "running" }
  | { kind: "install" }
  | { kind: "update"; changes: number }
  | { kind: "play" };

export interface InstanceView extends Instance {
  action: PrimaryAction;
}

export interface VersionSummary {
  id: string;
  kind: string;
}

export interface InstallProgress {
  instanceId: string;
  stage: string;
  completedFiles: number;
  totalFiles: number;
  downloadedBytes: number;
  totalBytes: number;
}

export interface GameLogLine {
  instanceId: string;
  line: string;
  isStderr: boolean;
}

export interface GameExit {
  instanceId: string;
  code: number | null;
  crashed: boolean;
}

/** Errors carry a stable code so the UI can branch without matching prose. */
export interface CommandError {
  code: string;
  message: string;
}

export function isCommandError(error: unknown): error is CommandError {
  return typeof error === "object" && error !== null && "code" in error && "message" in error;
}

export function errorMessage(error: unknown): string {
  if (isCommandError(error)) return error.message;
  if (error instanceof Error) return error.message;
  return String(error);
}

// --- commands --------------------------------------------------------------

export const listInstances = () => invoke<InstanceView[]>("list_instances");

export const listMinecraftVersions = () =>
  invoke<VersionSummary[]>("list_minecraft_versions");

export const createInstance = (name: string, mcVersion: string) =>
  invoke<InstanceView>("create_instance", { name, mcVersion });

export const deleteInstance = (id: string) => invoke<void>("delete_instance", { id });

export const launchInstance = (id: string) => invoke<void>("launch_instance", { id });

export const killInstance = (id: string) => invoke<void>("kill_instance", { id });

export const instanceFolder = (id: string) => invoke<string>("open_instance_folder", { id });

// --- events ----------------------------------------------------------------

export const onInstallProgress = (handler: (progress: InstallProgress) => void): Promise<UnlistenFn> =>
  listen<InstallProgress>("install://progress", (event) => handler(event.payload));

export const onGameLog = (handler: (line: GameLogLine) => void): Promise<UnlistenFn> =>
  listen<GameLogLine>("game://log", (event) => handler(event.payload));

export const onGameExit = (handler: (exit: GameExit) => void): Promise<UnlistenFn> =>
  listen<GameExit>("game://exit", (event) => handler(event.payload));

// --- formatting ------------------------------------------------------------

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}
