import { ChevronDown, FolderOpen, Loader2 } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import type { InstanceInfo } from "@/types";
import { cn } from "@/lib/utils";

const LOADERS = ["fabric", "forge", "neoforge", "quilt", "unknown"] as const;

const LOADER_VERSION_PLACEHOLDER: Record<string, string> = {
  fabric: "0.17.2",
  forge: "54.0.0",
  neoforge: "21.4.123",
  quilt: "0.26.0",
};

interface InstancePickerProps {
  label: string;
  instance: InstanceInfo | null;
  onPickFolder: () => void;
  onSelectInstance?: (instance: InstanceInfo) => void;
  onUpdateInstance?: (
    patch: Partial<Pick<InstanceInfo, "mcVersion" | "loader" | "loaderVersion">>
  ) => void;
  launcherInstances?: InstanceInfo[];
  loading?: boolean;
  compact?: boolean;
}

function shortPath(path: string) {
  const normalized = path.replace(/\\/g, "/").replace(/\/mods\/?$/i, "");
  const parts = normalized.split("/");
  if (parts.length <= 4) return normalized;
  return `…/${parts.slice(-4).join("/")}`;
}

function instanceDisplayPath(instance: InstanceInfo) {
  return instance.gameDir?.trim() || instance.modsPath.replace(/[/\\]mods\/?$/i, "");
}

export function InstancePicker({
  label,
  instance,
  onPickFolder,
  onSelectInstance,
  onUpdateInstance,
  launcherInstances = [],
  loading,
  compact = false,
}: InstancePickerProps) {
  const [launcherOpen, setLauncherOpen] = useState(!instance);
  const needsManual =
    instance &&
    (instance.mcVersion === "unknown" || instance.loader === "unknown");

  useEffect(() => {
    if (instance) {
      setLauncherOpen(false);
    }
  }, [instance?.modsPath]);

  return (
    <Card className={cn("shrink-0", compact && "shadow-none")}>
      <CardHeader className={cn("pb-1 pt-3 px-3", compact && "py-2 px-2")}>
        <CardTitle className={cn(compact ? "text-sm" : "text-base")}>{label}</CardTitle>
      </CardHeader>
      <CardContent className={cn("space-y-2 px-3 pb-3", compact && "px-2 pb-2 space-y-1.5")}>
        <Button
          variant="outline"
          size={compact ? "sm" : "default"}
          className={cn(
            "w-full justify-start",
            compact ? "h-8 text-xs" : "h-9 text-sm"
          )}
          onClick={onPickFolder}
          disabled={loading}
        >
          {loading ? (
            <Loader2 className={cn("animate-spin shrink-0", compact ? "h-3.5 w-3.5" : "h-4 w-4")} />
          ) : (
            <FolderOpen className={cn("shrink-0", compact ? "h-3.5 w-3.5" : "h-4 w-4")} />
          )}
          选择版本文件夹
        </Button>
        <p className="text-[10px] text-[var(--color-muted-foreground)] leading-snug">
          例如 .minecraft/versions/1.21.4-Fabric（无需选到 mods 子文件夹）
        </p>

        {launcherInstances.length > 0 && onSelectInstance && (
          <div className="rounded-md border border-[var(--color-border)]">
            <button
              type="button"
              onClick={() => setLauncherOpen((v) => !v)}
              className="flex w-full items-center justify-between gap-2 px-2 py-1.5 text-xs text-[var(--color-muted-foreground)] hover:bg-[var(--color-accent)]/50 transition-colors"
            >
              <span>已发现 {launcherInstances.length} 个实例</span>
              <ChevronDown
                className={cn(
                  "h-3.5 w-3.5 shrink-0 transition-transform",
                  launcherOpen && "rotate-180"
                )}
              />
            </button>
            {launcherOpen && (
              <div className="max-h-28 overflow-y-auto border-t border-[var(--color-border)] p-1 space-y-0.5">
                {launcherInstances.map((inst) => (
                  <button
                    key={`${inst.launcher}-${inst.modsPath}`}
                    type="button"
                    title={inst.modsPath}
                    onClick={() => {
                      onSelectInstance(inst);
                      setLauncherOpen(false);
                    }}
                    className={cn(
                      "w-full text-left rounded px-2 py-1 text-xs truncate border transition-colors",
                      instance?.modsPath === inst.modsPath
                        ? "border-[var(--color-primary)] bg-[var(--color-primary)]/15"
                        : "border-transparent hover:bg-[var(--color-accent)]"
                    )}
                  >
                    {inst.launcher ? `[${inst.launcher}] ` : ""}
                    {inst.name}
                    <span className="text-[var(--color-muted-foreground)]">
                      {" "}
                      · {inst.mcVersion} {inst.loader}
                    </span>
                  </button>
                ))}
              </div>
            )}
          </div>
        )}

        {instance && (
          <div
            className={cn(
              "rounded-md border border-[var(--color-border)] bg-[var(--color-background)] space-y-1.5",
              compact ? "p-2 text-xs" : "p-3 text-sm"
            )}
            title={instanceDisplayPath(instance)}
          >
            <div className="font-medium truncate leading-snug">{instance.name}</div>
            <div className="text-[var(--color-muted-foreground)] truncate text-xs leading-snug">
              {shortPath(instanceDisplayPath(instance))}
            </div>
            <div className="flex flex-nowrap gap-1.5 pt-0.5 overflow-hidden">
              <Badge
                variant={instance.mcVersion === "unknown" ? "warning" : "secondary"}
                className={cn("shrink-0", compact ? "text-[10px] px-1.5 py-0" : "text-xs")}
              >
                MC {instance.mcVersion}
              </Badge>
              <Badge
                variant={instance.loader === "unknown" ? "warning" : "secondary"}
                className={cn("shrink-0 truncate", compact ? "text-[10px] px-1.5 py-0" : "text-xs")}
              >
                {instance.loader}
                {instance.loaderVersion ? ` ${instance.loaderVersion}` : ""}
              </Badge>
            </div>

            {onUpdateInstance && needsManual && (
              <div className="space-y-1.5 pt-1 border-t border-[var(--color-border)]">
                <div className="text-[10px] text-amber-400">未能自动识别，请手动填写</div>
                <Input
                  placeholder="MC 版本，如 1.21.4"
                  value={instance.mcVersion === "unknown" ? "" : instance.mcVersion}
                  onChange={(e) =>
                    onUpdateInstance({
                      mcVersion: e.target.value.trim() || "unknown",
                    })
                  }
                  className="h-7 text-xs"
                />
                {instance.loader !== "unknown" && (
                  <Input
                    placeholder={`加载器 ${
                      LOADER_VERSION_PLACEHOLDER[instance.loader] ?? "0.17.2"
                    }`}
                    value={instance.loaderVersion ?? ""}
                    onChange={(e) =>
                      onUpdateInstance({
                        loaderVersion: e.target.value.trim(),
                      })
                    }
                    className="h-7 text-xs"
                  />
                )}
                <div className="flex flex-wrap gap-1">
                  {LOADERS.filter((l) => l !== "unknown").map((loader) => (
                    <button
                      key={loader}
                      type="button"
                      onClick={() => onUpdateInstance({ loader })}
                      className={cn(
                        "rounded px-1.5 py-0.5 text-[10px] border transition-colors",
                        instance.loader === loader
                          ? "border-[var(--color-primary)] bg-[var(--color-primary)]/15"
                          : "border-[var(--color-border)] hover:bg-[var(--color-accent)]"
                      )}
                    >
                      {loader}
                    </button>
                  ))}
                </div>
              </div>
            )}

            {needsManual && !onUpdateInstance && (
              <p className="text-[10px] text-amber-400 pt-0.5 leading-tight">
                版本/加载器未识别，兼容检查可能不准确
              </p>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
