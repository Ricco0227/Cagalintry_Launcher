import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Boxes, Info, Plus, TriangleAlert } from "lucide-react";

import {
  createInstance,
  deleteInstance,
  errorMessage,
  isCommandError,
  launchInstance,
  listInstances,
  listMinecraftVersions,
  onGameExit,
  onInstallProgress,
  type InstallProgress,
  type InstanceView,
} from "@/lib/api";
import { cn } from "@/lib/cn";
import { Button } from "@/components/Button";
import { EmptyState, Page } from "@/components/Page";
import { InstanceCard } from "@/components/InstanceCard";
import { Modal } from "@/components/Modal";

/** A banner message. Not every interruption is a failure. */
interface Notice {
  tone: "error" | "info";
  message: string;
}

export function Library() {
  const queryClient = useQueryClient();
  const [creating, setCreating] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [progress, setProgress] = useState<Record<string, InstallProgress>>({});

  const setError = (message: string) => setNotice({ tone: "error", message });

  const instances = useQuery({ queryKey: ["instances"], queryFn: listInstances });

  // Progress and exit both change what the primary button should say, so both
  // invalidate the instance list rather than the UI trying to guess.
  useEffect(() => {
    const unlisten = [
      onInstallProgress((update) =>
        setProgress((current) => ({ ...current, [update.instanceId]: update })),
      ),
      onGameExit((exit) => {
        void queryClient.invalidateQueries({ queryKey: ["instances"] });
        if (exit.crashed) {
          setError(
            `The game exited unexpectedly${exit.code === null ? "" : ` with code ${exit.code}`}.`,
          );
        }
      }),
    ];
    return () => {
      for (const pending of unlisten) void pending.then((off) => off());
    };
  }, [queryClient]);

  const launch = useMutation({
    mutationFn: launchInstance,
    onMutate: () => setNotice(null),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: ["instances"] }),
    onError: (err) => {
      if (!isCommandError(err)) {
        setError(errorMessage(err));
        return;
      }
      // An impatient second click is not something to apologise for — the
      // guard did its job and the first launch is still running.
      if (err.code === "busy" || err.code === "running") return;
      // Not having signed in yet is a next step, not a fault.
      setNotice({ tone: err.code === "noAccount" ? "info" : "error", message: err.message });
    },
  });

  /**
   * Signing in is a different action from launching, so it is dispatched here
   * rather than sent to the backend only to come back as an error.
   */
  const handlePrimary = (instance: InstanceView) => {
    if (instance.action.kind === "linkMinecraft") {
      setNotice({
        tone: "info",
        message:
          "Microsoft sign-in lands in the next phase. Once your Azure application is approved for the Minecraft API, you'll link an account here and this becomes Play.",
      });
      return;
    }
    launch.mutate(instance.id);
  };

  const remove = useMutation({
    mutationFn: deleteInstance,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["instances"] }),
    onError: (err) => setError(errorMessage(err)),
  });

  const list = instances.data ?? [];

  return (
    <Page
      title="Library"
      subtitle={
        list.length === 0
          ? "Your Minecraft instances"
          : `${list.length} instance${list.length === 1 ? "" : "s"}`
      }
      actions={
        <Button variant="primary" icon={<Plus size={15} />} onClick={() => setCreating(true)}>
          New instance
        </Button>
      }
    >
      {notice && (
        <div
          className={cn(
            "mb-5 flex items-start gap-2.5 rounded-[12px] border px-3.5 py-3 text-[13px]",
            notice.tone === "error"
              ? "border-danger/35 bg-danger/10"
              : "border-accent/35 bg-accent-soft",
          )}
        >
          {notice.tone === "error" ? (
            <TriangleAlert size={16} className="mt-px shrink-0 text-danger" />
          ) : (
            <Info size={16} className="mt-px shrink-0 text-accent" />
          )}
          <p data-selectable className="min-w-0 flex-1 leading-relaxed">
            {notice.message}
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

      {instances.isPending ? null : list.length === 0 ? (
        <EmptyState
          icon={<Boxes size={24} />}
          title="No instances yet"
          description="Create one to download Minecraft and play. Everything is shared between instances, so a second one on the same version is nearly instant."
          action={
            <Button variant="primary" icon={<Plus size={15} />} onClick={() => setCreating(true)}>
              New instance
            </Button>
          }
        />
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(230px,1fr))] gap-4">
          {list.map((instance) => (
            <InstanceCard
              key={instance.id}
              instance={instance}
              progress={progress[instance.id]}
              onPrimary={() => handlePrimary(instance)}
              onDelete={() => remove.mutate(instance.id)}
            />
          ))}
        </div>
      )}

      <CreateInstanceDialog
        open={creating}
        onClose={() => setCreating(false)}
        onCreated={() => {
          setCreating(false);
          void queryClient.invalidateQueries({ queryKey: ["instances"] });
        }}
        onError={setError}
      />
    </Page>
  );
}

function CreateInstanceDialog({
  open,
  onClose,
  onCreated,
  onError,
}: {
  open: boolean;
  onClose: () => void;
  onCreated: () => void;
  onError: (message: string) => void;
}) {
  const [name, setName] = useState("");
  const [version, setVersion] = useState("");

  // Only fetched once the dialog is opened — the version manifest is a network
  // request and the Library shouldn't pay for it on every visit.
  const versions = useQuery({
    queryKey: ["minecraft-versions"],
    queryFn: listMinecraftVersions,
    enabled: open,
    staleTime: 60 * 60 * 1000,
  });

  const latest = versions.data?.[0]?.id ?? "";
  const selected = version || latest;

  const suggestedName = useMemo(
    () => (selected ? `Minecraft ${selected}` : ""),
    [selected],
  );

  const create = useMutation({
    mutationFn: () => createInstance(name.trim() || suggestedName, selected),
    onSuccess: () => {
      setName("");
      setVersion("");
      onCreated();
    },
    onError: (err) => onError(errorMessage(err)),
  });

  const canCreate = selected !== "" && !create.isPending;

  return (
    <Modal
      open={open}
      title="New instance"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!canCreate} onClick={() => create.mutate()}>
            {create.isPending ? "Creating…" : "Create"}
          </Button>
        </>
      }
    >
      <form
        className="flex flex-col gap-4"
        onSubmit={(event) => {
          event.preventDefault();
          if (canCreate) create.mutate();
        }}
      >
        <Field label="Name">
          <input
            autoFocus
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder={suggestedName || "My instance"}
            className="h-9 w-full rounded-[10px] border border-border bg-surface px-3 text-[13px] outline-none placeholder:text-text-subtle focus:border-accent"
          />
        </Field>

        <Field label="Minecraft version">
          {versions.isPending ? (
            <div className="flex h-9 items-center rounded-[10px] border border-border bg-surface px-3 text-[13px] text-text-subtle">
              Loading versions…
            </div>
          ) : versions.isError ? (
            <p className="text-[12.5px] text-danger">
              Could not reach Mojang: {errorMessage(versions.error)}
            </p>
          ) : (
            <select
              value={selected}
              onChange={(event) => setVersion(event.target.value)}
              className="h-9 w-full rounded-[10px] border border-border bg-surface px-2.5 text-[13px] outline-none focus:border-accent"
            >
              {versions.data?.map((entry) => (
                <option key={entry.id} value={entry.id}>
                  {entry.id}
                </option>
              ))}
            </select>
          )}
        </Field>
      </form>
    </Modal>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-[12px] font-medium text-text-muted">{label}</span>
      {children}
    </label>
  );
}
