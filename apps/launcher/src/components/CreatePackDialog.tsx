import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  createPack,
  errorMessage,
  listLoaderVersions,
  listMinecraftVersions,
  type LoaderKind,
} from "@/lib/api";
import { useLauncherStore } from "@/lib/store";
import { cn } from "@/lib/cn";
import { Button } from "./Button";
import { Modal } from "./Modal";
import { Select } from "./Select";

const LOADER_KINDS: LoaderKind[] = ["vanilla", "fabric", "quilt", "neoforge"];

const LOADER_LABELS: Record<LoaderKind, string> = {
  vanilla: "Vanilla",
  fabric: "Fabric",
  quilt: "Quilt",
  neoforge: "NeoForge",
};

/**
 * Creating a modpack: a name, a Minecraft version and a loader.
 *
 * Mounted once at the app level and driven by the store, because the button
 * that opens it lives in the rail and has to work from whichever page you
 * happen to be on. A newly created pack becomes the selected one — you almost
 * certainly made it to play it.
 */
export function CreatePackDialog({ onError }: { onError: (message: string) => void }) {
  const queryClient = useQueryClient();
  const open = useLauncherStore((state) => state.creating);
  const setCreating = useLauncherStore((state) => state.setCreating);
  const selectPack = useLauncherStore((state) => state.selectPack);

  const [name, setName] = useState("");
  const [version, setVersion] = useState("");
  const [loaderKind, setLoaderKind] = useState<LoaderKind>("vanilla");
  const [loaderVersion, setLoaderVersion] = useState("");

  // Only fetched once the dialog is opened — the version manifest is a network
  // request and the launcher shouldn't pay for it on every visit.
  const versions = useQuery({
    queryKey: ["minecraft-versions"],
    queryFn: listMinecraftVersions,
    enabled: open,
    staleTime: 60 * 60 * 1000,
  });

  const latest = versions.data?.[0]?.id ?? "";
  const selected = version || latest;

  const loaders = useQuery({
    queryKey: ["loader-versions", loaderKind, selected],
    queryFn: () => listLoaderVersions(loaderKind, selected),
    enabled: open && loaderKind !== "vanilla" && selected !== "",
    staleTime: 60 * 60 * 1000,
  });

  // Default to the newest stable build, falling back to the newest of any kind
  // when a version only has prereleases — which is normal soon after a
  // Minecraft release.
  const defaultLoaderVersion =
    loaders.data?.find((entry) => entry.stable)?.version ?? loaders.data?.[0]?.version ?? "";
  const selectedLoaderVersion = loaderVersion || defaultLoaderVersion;

  const suggestedName = useMemo(() => {
    if (!selected) return "";
    return loaderKind === "vanilla"
      ? `Minecraft ${selected}`
      : `${LOADER_LABELS[loaderKind]} ${selected}`;
  }, [selected, loaderKind]);

  const close = () => setCreating(false);

  const create = useMutation({
    mutationFn: () =>
      createPack(name.trim() || suggestedName, selected, {
        kind: loaderKind,
        ...(loaderKind === "vanilla" ? {} : { version: selectedLoaderVersion }),
      }),
    onSuccess: (pack) => {
      setName("");
      setVersion("");
      setLoaderKind("vanilla");
      setLoaderVersion("");
      selectPack(pack.id);
      void queryClient.invalidateQueries({ queryKey: ["packs"] });
      close();
    },
    onError: (err) => onError(errorMessage(err)),
  });

  // A modded pack without a loader build would silently launch vanilla.
  const loaderReady = loaderKind === "vanilla" || selectedLoaderVersion !== "";
  const canCreate = selected !== "" && loaderReady && !create.isPending;

  return (
    <Modal
      open={open}
      title="New modpack"
      onClose={close}
      footer={
        <>
          <Button onClick={close}>Cancel</Button>
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
            placeholder={suggestedName || "My modpack"}
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
            <Select
              value={selected}
              options={(versions.data ?? []).map((entry) => ({
                value: entry.id,
                label: entry.id,
              }))}
              onChange={(next) => {
                setVersion(next);
                // Loader builds are per Minecraft version; keeping the old
                // choice would pin a build that doesn't exist for the new one.
                setLoaderVersion("");
              }}
              ariaLabel="Minecraft version"
              className="w-full"
            />
          )}
        </Field>

        <Field label="Mod loader">
          <div className="grid grid-cols-4 gap-1.5">
            {LOADER_KINDS.map((kind) => (
              <button
                key={kind}
                type="button"
                onClick={() => {
                  setLoaderKind(kind);
                  setLoaderVersion("");
                }}
                className={cn(
                  "rounded-[9px] border px-2 py-2 text-[12.5px] font-medium transition-colors",
                  loaderKind === kind
                    ? "border-accent bg-accent-soft text-accent"
                    : "border-border bg-surface text-text-muted hover:border-border-strong hover:text-text",
                )}
              >
                {LOADER_LABELS[kind]}
              </button>
            ))}
          </div>
        </Field>

        {loaderKind !== "vanilla" && (
          <Field label={`${LOADER_LABELS[loaderKind]} version`}>
            {loaders.isPending ? (
              <div className="flex h-9 items-center rounded-[10px] border border-border bg-surface px-3 text-[13px] text-text-subtle">
                Loading builds…
              </div>
            ) : loaders.isError ? (
              <p className="text-[12.5px] text-danger">{errorMessage(loaders.error)}</p>
            ) : (
              <Select
                value={selectedLoaderVersion}
                options={(loaders.data ?? []).map((entry) => ({
                  value: entry.version,
                  label: entry.version,
                  ...(entry.stable ? {} : { hint: "prerelease" }),
                }))}
                onChange={setLoaderVersion}
                ariaLabel={`${LOADER_LABELS[loaderKind]} version`}
                className="w-full"
              />
            )}
          </Field>
        )}
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
