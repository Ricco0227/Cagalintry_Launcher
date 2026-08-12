import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openPath } from "@tauri-apps/plugin-opener";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Monitor, Moon, Sun } from "lucide-react";

import {
  dataDirectory,
  getSettings,
  updateSettings,
  type Settings as SettingsData,
  type Theme,
} from "@/lib/api";
import { applyTheme } from "@/lib/theme";
import { cn } from "@/lib/cn";
import { Button } from "@/components/Button";
import { Field, Section, inputClass } from "@/components/Field";
import { Page } from "@/components/Page";

const THEMES: { value: Theme; label: string; icon: typeof Sun }[] = [
  { value: "system", label: "System", icon: Monitor },
  { value: "light", label: "Light", icon: Sun },
  { value: "dark", label: "Dark", icon: Moon },
];

export function Settings() {
  const queryClient = useQueryClient();
  const settings = useQuery({ queryKey: ["settings"], queryFn: getSettings });
  const dataDir = useQuery({ queryKey: ["data-directory"], queryFn: dataDirectory });

  const save = useMutation({
    mutationFn: updateSettings,
    onSuccess: (updated) => {
      queryClient.setQueryData(["settings"], updated);
      // Applied immediately rather than on next launch — a theme picker that
      // needs a restart to take effect is a broken theme picker.
      applyTheme(updated.theme);
    },
  });

  if (!settings.data) return <Page title="Settings" subtitle="Java, downloads and appearance" children={null} />;

  return (
    <Page title="Settings" subtitle="Java, downloads and appearance">
      <div className="flex max-w-[620px] flex-col gap-5">
        <Section title="Appearance">
          <Field label="Theme">
            <div className="flex gap-2">
              {THEMES.map(({ value, label, icon: Icon }) => (
                <button
                  key={value}
                  type="button"
                  onClick={() => save.mutate({ theme: value })}
                  className={cn(
                    "flex flex-1 items-center justify-center gap-2 rounded-[10px] border px-3 py-2.5 text-[13px] font-medium transition-colors",
                    settings.data.theme === value
                      ? "border-accent bg-accent-soft text-accent"
                      : "border-border bg-bg text-text-muted hover:border-border-strong hover:text-text",
                  )}
                >
                  <Icon size={15} />
                  {label}
                </button>
              ))}
            </div>
          </Field>
        </Section>

        <Section title="Java" description="Applies to every pack unless one overrides it.">
          <JavaPathField settings={settings.data} onSave={(javaPath) => save.mutate({ javaPath })} />

          <Field
            label="Default memory"
            hint="Maximum heap for new packs, in megabytes. More is not always better — beyond what the pack needs, a larger heap mostly makes garbage collection pauses longer."
          >
            <MemoryInput
              value={settings.data.defaultMaxMemoryMb}
              onCommit={(defaultMaxMemoryMb) => save.mutate({ defaultMaxMemoryMb })}
            />
          </Field>
        </Section>

        <Section title="Downloads">
          <Field
            label="Simultaneous downloads"
            hint="An install is thousands of small files. Past a point the limit is the disk and the CDN rather than your connection. Takes effect after a restart."
          >
            <input
              type="number"
              min={1}
              max={32}
              defaultValue={settings.data.downloadConcurrency}
              onBlur={(event) => {
                const downloadConcurrency = Number(event.target.value);
                if (Number.isFinite(downloadConcurrency)) save.mutate({ downloadConcurrency });
              }}
              className={cn(inputClass, "w-28")}
            />
          </Field>
        </Section>

        <Section title="Storage">
          <Field
            label="Data folder"
            hint="Packs, and the shared libraries, assets and Java runtimes every pack draws on."
          >
            <div className="flex gap-2">
              <input readOnly value={dataDir.data ?? ""} className={cn(inputClass, "flex-1")} data-selectable />
              <Button
                icon={<FolderOpen size={15} />}
                onClick={() => dataDir.data && void openPath(dataDir.data)}
              >
                Open
              </Button>
            </div>
          </Field>
        </Section>
      </div>
    </Page>
  );
}

function JavaPathField({
  settings,
  onSave,
}: {
  settings: SettingsData;
  onSave: (javaPath: string | null) => void;
}) {
  const [value, setValue] = useState(settings.javaPath ?? "");

  useEffect(() => setValue(settings.javaPath ?? ""), [settings.javaPath]);

  const browse = async () => {
    const picked = await openFileDialog({
      title: "Select a Java executable",
      multiple: false,
      directory: false,
    });
    if (typeof picked === "string") {
      setValue(picked);
      onSave(picked);
    }
  };

  return (
    <Field
      label="Java executable"
      hint="Leave empty to let the launcher download the runtime each version was tested with, falling back to a system JVM new enough to run it."
    >
      <div className="flex gap-2">
        <input
          value={value}
          onChange={(event) => setValue(event.target.value)}
          // An emptied field means "go back to automatic", which is why the
          // backend distinguishes an absent field from an explicit null.
          onBlur={() => onSave(value.trim() === "" ? null : value)}
          placeholder="Automatic"
          className={cn(inputClass, "flex-1")}
        />
        <Button onClick={() => void browse()}>Browse</Button>
      </div>
    </Field>
  );
}

/** Numeric input that only commits a sane value. */
function MemoryInput({
  value,
  onCommit,
}: {
  value: number;
  onCommit: (value: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));

  useEffect(() => setDraft(String(value)), [value]);

  return (
    <div className="flex items-center gap-2">
      <input
        type="number"
        min={512}
        max={65536}
        step={512}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => {
          const parsed = Number(draft);
          if (Number.isFinite(parsed) && parsed > 0) onCommit(parsed);
          else setDraft(String(value));
        }}
        className={cn(inputClass, "w-32")}
      />
      <span className="text-[12.5px] text-text-subtle">MB</span>
    </div>
  );
}
