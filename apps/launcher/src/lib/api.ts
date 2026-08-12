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

/** What kind of content an entry is. Mirrors `EntryKind` in the proto crate. */
export type EntryKind = "mod" | "resourcepack" | "shaderpack";

export interface PackEntry {
  kind: EntryKind;
  source: { type: "modrinth"; projectId: string; versionId: string };
  path: string;
  hashes: { sha1: string; sha512: string };
  size: number;
  downloads: string[];
  side: "client" | "both";
  enabled: boolean;
  name?: string;
  versionNumber?: string;
}

export interface SearchHit {
  projectId: string;
  slug: string;
  title: string;
  description: string;
  author?: string;
  iconUrl?: string;
  downloads: number;
  follows: number;
  categories: string[];
  clientSide?: string;
  serverSide?: string;
}

export interface SearchResults {
  hits: SearchHit[];
  offset: number;
  limit: number;
  totalHits: number;
}

export interface GalleryImage {
  url: string;
  title?: string;
  description?: string;
  featured: boolean;
}

/**
 * A project's full page. `bodyHtml` is the description rendered from Markdown
 * and sanitised in Rust — scripts, event handlers and inline styles are already
 * gone by the time it arrives here.
 */
export interface ProjectPage {
  id: string;
  slug: string;
  title: string;
  description: string;
  bodyHtml: string;
  iconUrl?: string;
  downloads: number;
  followers: number;
  categories: string[];
  clientSide?: string;
  serverSide?: string;
  sourceUrl?: string;
  issuesUrl?: string;
  wikiUrl?: string;
  discordUrl?: string;
  gallery: GalleryImage[];
}

export interface ModrinthVersion {
  id: string;
  projectId: string;
  name: string;
  versionNumber: string;
  versionType: "release" | "beta" | "alpha";
  gameVersions: string[];
  loaders: string[];
  downloads: number;
}

export type Theme = "system" | "light" | "dark";

export interface Settings {
  theme: Theme;
  downloadConcurrency: number;
  defaultMaxMemoryMb: number;
  javaPath?: string;
}

/**
 * Only the fields being changed are sent. `javaPath: null` clears the override,
 * while omitting it leaves the current value alone — the two are different.
 */
export interface SettingsPatch {
  theme?: Theme;
  downloadConcurrency?: number;
  defaultMaxMemoryMb?: number;
  javaPath?: string | null;
}

export interface InstancePatch {
  name?: string;
  maxMemoryMb?: number;
  javaPath?: string;
  extraJvmArgs?: string[];
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

export interface LoaderVersion {
  version: string;
  /** Prereleases are still offered, just never preselected. */
  stable: boolean;
}

export const listLoaderVersions = (kind: LoaderKind, mcVersion: string) =>
  invoke<LoaderVersion[]>("list_loader_versions", { kind, mcVersion });

export const createInstance = (name: string, mcVersion: string, loader: LoaderSpec) =>
  invoke<InstanceView>("create_instance", { name, mcVersion, loader });

export const getInstance = (id: string) => invoke<InstanceView>("get_instance", { id });

export const updateInstance = (id: string, patch: InstancePatch) =>
  invoke<InstanceView>("update_instance", { id, patch });

export const deleteInstance = (id: string) => invoke<void>("delete_instance", { id });

export const searchContent = (params: {
  kind: EntryKind;
  query: string;
  mcVersion?: string;
  loader?: LoaderKind;
  offset?: number;
  limit?: number;
}) => invoke<SearchResults>("search_content", params);

export const getProject = (projectId: string) =>
  invoke<ProjectPage>("get_project", { projectId });

export const listProjectVersions = (id: string, projectId: string, kind: EntryKind) =>
  invoke<ModrinthVersion[]>("list_project_versions", { id, projectId, kind });

export const listContent = (id: string) => invoke<PackEntry[]>("list_content", { id });

export const installContent = (
  id: string,
  projectId: string,
  kind: EntryKind,
  versionId?: string,
) => invoke<PackEntry[]>("install_content", { id, projectId, kind, versionId });

export const removeContent = (id: string, path: string) =>
  invoke<PackEntry[]>("remove_content", { id, path });

export const setContentEnabled = (id: string, path: string, enabled: boolean) =>
  invoke<PackEntry[]>("set_content_enabled", { id, path, enabled });

export const getSettings = () => invoke<Settings>("get_settings");

export const updateSettings = (patch: SettingsPatch) =>
  invoke<Settings>("update_settings", { patch });

export const dataDirectory = () => invoke<string>("data_directory");

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
