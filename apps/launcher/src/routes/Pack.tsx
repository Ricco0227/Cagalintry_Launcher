import { useEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  ArrowLeft,
  FolderOpen,
  Globe,
  Package,
  Plus,
  ScrollText,
  Settings2,
  Square,
  Trash2,
} from "lucide-react";

import {
  deletePack,
  errorMessage,
  getPack,
  packFolder,
  isCommandError,
  killPack,
  launchPack,
  updatePack,
} from "@/lib/api";
import { useLauncherStore } from "@/lib/store";
import { cn } from "@/lib/cn";
import { Button } from "@/components/Button";
import { ContentBrowser } from "@/components/ContentBrowser";
import { Field, Section, inputClass } from "@/components/Field";
import { InstalledContent } from "@/components/InstalledContent";
import { EmptyState } from "@/components/Page";
import { PrimaryButton } from "@/components/PrimaryButton";

const TABS = [
  { id: "content", label: "Content", icon: Package },
  { id: "worlds", label: "Worlds", icon: Globe },
  { id: "logs", label: "Logs", icon: ScrollText },
  { id: "settings", label: "Settings", icon: Settings2 },
] as const;

type TabId = (typeof TABS)[number]["id"];

export function PackPage() {
  const { id = "" } = useParams();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [tab, setTab] = useState<TabId>("content");
  const [notice, setNotice] = useState<string | null>(null);
  const [browsing, setBrowsing] = useState(false);

  const pack = useQuery({
    queryKey: ["pack", id],
    queryFn: () => getPack(id),
    enabled: id !== "",
  });

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ["pack", id] });
    void queryClient.invalidateQueries({ queryKey: ["packs"] });
  };

  const launch = useMutation({
    mutationFn: () => launchPack(id),
    onSettled: refresh,
    onError: (err) => {
      if (isCommandError(err) && (err.code === "busy" || err.code === "running")) return;
      setNotice(errorMessage(err));
    },
  });

  const stop = useMutation({ mutationFn: () => killPack(id), onSettled: refresh });

  const remove = useMutation({
    mutationFn: () => deletePack(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["packs"] });
      void navigate("/");
    },
    onError: (err) => setNotice(errorMessage(err)),
  });

  if (!pack.data) return null;
  const data = pack.data;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* Plain background for now — pack artwork lands with pack icons, and a
          hashed gradient in the meantime was standing in for a picture that
          isn't there yet. */}
      <header className="relative shrink-0 border-b border-border">
        <div className="relative flex flex-col gap-4 px-8 pt-5 pb-5">
          <Link
            to="/"
            className="inline-flex w-fit items-center gap-1.5 rounded-lg bg-surface-2 px-2.5 py-1.5 text-[12.5px] font-medium text-text-muted transition-colors hover:bg-surface-3 hover:text-text"
          >
            <ArrowLeft size={14} />
            Library
          </Link>

          <div className="flex items-end justify-between gap-4">
            <div className="min-w-0">
              <h1 className="truncate text-[26px] leading-tight font-semibold">{data.name}</h1>
              <div className="mt-2 flex flex-wrap items-center gap-1.5 text-[12px]">
                <Chip>{data.mcVersion}</Chip>
                {data.loader.kind !== "vanilla" && (
                  <Chip className="capitalize">{data.loader.kind}</Chip>
                )}
                {data.lastPlayed && (
                  <span className="text-text-muted">
                    Last played {new Date(data.lastPlayed).toLocaleDateString()}
                  </span>
                )}
              </div>
            </div>

            <div className="flex shrink-0 items-center gap-2">
              <Button
                icon={<FolderOpen size={15} />}
                onClick={() => void packFolder(id).then(openPath)}
              >
                Folder
              </Button>
              {data.action.kind === "running" ? (
                <Button icon={<Square size={14} />} onClick={() => stop.mutate()}>
                  Stop
                </Button>
              ) : null}
              <PrimaryButton
                action={data.action}
                onClick={() => {
                  if (data.action.kind === "linkMinecraft") {
                    setNotice(
                      "Microsoft sign-in lands in the next phase. Once the Azure application is approved you'll link an account and this becomes Play.",
                    );
                    return;
                  }
                  launch.mutate();
                }}
              />
            </div>
          </div>
        </div>
      </header>

      <nav className="flex shrink-0 gap-1 border-b border-border px-8">
        {TABS.map(({ id: tabId, label, icon: Icon }) => (
          <button
            key={tabId}
            type="button"
            onClick={() => setTab(tabId)}
            className={cn(
              "relative flex items-center gap-1.5 px-3 py-2.5 text-[13px] font-medium transition-colors",
              tab === tabId ? "text-text" : "text-text-subtle hover:text-text-muted",
            )}
          >
            <Icon size={15} />
            {label}
            {tab === tabId && (
              <span className="absolute inset-x-2 -bottom-px h-[2px] rounded-full bg-accent" />
            )}
          </button>
        ))}
      </nav>

      <div className="min-h-0 flex-1 overflow-y-auto px-8 py-6">
        {notice && (
          <div className="mb-5 flex items-start gap-2.5 rounded-[12px] border border-accent/35 bg-accent-soft px-3.5 py-3 text-[13px]">
            <p data-selectable className="min-w-0 flex-1 leading-relaxed">
              {notice}
            </p>
            <button
              type="button"
              onClick={() => setNotice(null)}
              className="shrink-0 text-text-subtle hover:text-text"
            >
              Dismiss
            </button>
          </div>
        )}

        {tab === "content" &&
          (browsing ? (
            <div className="flex flex-col gap-4">
              <div className="flex items-center justify-between">
                <h2 className="text-[15px] font-semibold">Add mods</h2>
                <Button size="sm" onClick={() => setBrowsing(false)}>
                  Done
                </Button>
              </div>
              <ContentBrowser pack={data} onError={setNotice} />
            </div>
          ) : (
            <div className="flex flex-col gap-4">
              <div className="flex items-center justify-between">
                <h2 className="text-[15px] font-semibold">Installed</h2>
                <Button
                  size="sm"
                  variant="primary"
                  icon={<Plus size={14} />}
                  onClick={() => setBrowsing(true)}
                >
                  Add mods
                </Button>
              </div>
              <InstalledContent
                packId={id}
                onError={setNotice}
                onBrowse={() => setBrowsing(true)}
              />
            </div>
          ))}

        {tab === "worlds" && (
          <EmptyState
            icon={<Globe size={24} />}
            title="No worlds yet"
            description="Worlds live in this modpack's saves folder. They are never touched by an update and never synced."
          />
        )}

        {tab === "logs" && <LogsTab packId={id} />}

        {tab === "settings" && (
          <PackSettings
            packId={id}
            name={data.name}
            maxMemoryMb={data.maxMemoryMb}
            javaPath={data.javaPath ?? ""}
            extraJvmArgs={data.extraJvmArgs}
            onSaved={refresh}
            onDelete={() => remove.mutate()}
          />
        )}
      </div>
    </div>
  );
}

function Chip({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <span className={cn("rounded-md bg-surface-2 px-1.5 py-0.5 font-medium", className)}>
      {children}
    </span>
  );
}

/** Live game output. */
function LogsTab({ packId }: { packId: string }) {
  const lines = useLauncherStore((state) => state.logs[packId]);
  const clear = useLauncherStore((state) => state.clearLogs);
  const bottom = useRef<HTMLDivElement>(null);

  // Follow the tail, which is where anything interesting appears.
  useEffect(() => {
    bottom.current?.scrollIntoView({ block: "end" });
  }, [lines]);

  if (!lines || lines.length === 0) {
    return (
      <EmptyState
        icon={<ScrollText size={24} />}
        title="No output yet"
        description="Output from the game appears here while it runs, with errors highlighted."
      />
    );
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <p className="text-[12.5px] text-text-muted tabular-nums">{lines.length} lines</p>
        <Button size="sm" onClick={() => clear(packId)}>
          Clear
        </Button>
      </div>

      <div
        data-selectable
        className="overflow-x-auto rounded-[12px] border border-border bg-bg p-3 font-mono text-[11.5px] leading-relaxed"
      >
        {lines.map((line, index) => (
          <div
            key={index}
            className={cn("whitespace-pre", line.isStderr ? "text-danger" : "text-text-muted")}
          >
            {line.line}
          </div>
        ))}
        <div ref={bottom} />
      </div>
    </div>
  );
}

function PackSettings({
  packId,
  name,
  maxMemoryMb,
  javaPath,
  extraJvmArgs,
  onSaved,
  onDelete,
}: {
  packId: string;
  name: string;
  maxMemoryMb: number;
  javaPath: string;
  extraJvmArgs: string[];
  onSaved: () => void;
  onDelete: () => void;
}) {
  const [draftName, setDraftName] = useState(name);
  const [draftMemory, setDraftMemory] = useState(String(maxMemoryMb));
  const [draftJava, setDraftJava] = useState(javaPath);
  const [draftArgs, setDraftArgs] = useState(extraJvmArgs.join(" "));

  const save = useMutation({
    mutationFn: (patch: Parameters<typeof updatePack>[1]) => updatePack(packId, patch),
    onSuccess: onSaved,
  });

  return (
    <div className="flex max-w-[620px] flex-col gap-5">
      <Section title="General">
        <Field label="Name">
          <input
            value={draftName}
            onChange={(event) => setDraftName(event.target.value)}
            onBlur={() => draftName.trim() && save.mutate({ name: draftName.trim() })}
            className={inputClass}
          />
        </Field>
      </Section>

      <Section title="Java" description="Overrides the launcher-wide settings for this modpack.">
        <Field label="Maximum memory" hint="In megabytes.">
          <input
            type="number"
            min={512}
            max={65536}
            step={512}
            value={draftMemory}
            onChange={(event) => setDraftMemory(event.target.value)}
            onBlur={() => {
              const parsed = Number(draftMemory);
              if (Number.isFinite(parsed) && parsed > 0) save.mutate({ maxMemoryMb: parsed });
              else setDraftMemory(String(maxMemoryMb));
            }}
            className={cn(inputClass, "w-32")}
          />
        </Field>

        <Field label="Java executable" hint="Leave empty to use the launcher-wide setting.">
          <input
            value={draftJava}
            onChange={(event) => setDraftJava(event.target.value)}
            onBlur={() => save.mutate({ javaPath: draftJava })}
            placeholder="Use launcher setting"
            className={inputClass}
          />
        </Field>

        <Field
          label="Additional JVM arguments"
          hint="Space separated, applied after the version's own arguments so they take precedence."
        >
          <input
            value={draftArgs}
            onChange={(event) => setDraftArgs(event.target.value)}
            onBlur={() => save.mutate({ extraJvmArgs: draftArgs.split(/\s+/).filter(Boolean) })}
            placeholder="-XX:+UseZGC"
            className={cn(inputClass, "font-mono")}
          />
        </Field>
      </Section>

      <Section title="Danger zone">
        <div className="flex items-center justify-between gap-4">
          <p className="text-[12.5px] leading-relaxed text-text-muted">
            Deleting removes this modpack and everything in it, including its worlds. Shared
            libraries, assets and Java runtimes are left alone.
          </p>
          <Button
            icon={<Trash2 size={15} />}
            onClick={onDelete}
            className="shrink-0 border-danger/40 text-danger hover:bg-danger hover:text-white"
          >
            Delete
          </Button>
        </div>
      </Section>
    </div>
  );
}
