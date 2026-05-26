import { ChevronDown, ExternalLink, Info, Loader2 } from "lucide-react";
import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { IdentifiedMod, ModVersionOption } from "@/types";

interface ModVersionPickerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  modInfo: IdentifiedMod;
  currentVersion?: string;
  selectedVersion?: string;
  loading: boolean;
  options: ModVersionOption[];
  error?: string | null;
  onSelect: (option: ModVersionOption) => void;
}

const SOURCE_PAGE: Record<string, string> = {
  modrinth: "Modrinth",
  curseforge: "CurseForge",
};

const VERSION_TYPE_LABEL: Record<string, string> = {
  release: "正式版",
  beta: "测试版",
  alpha: "内测版",
};

function modPageUrl(mod: IdentifiedMod): string | null {
  if (mod.projectId) {
    return `https://modrinth.com/mod/${mod.projectId}`;
  }
  if (mod.curseforgeId) {
    return `https://www.curseforge.com/minecraft/mc-mods/${mod.curseforgeId}`;
  }
  return null;
}

function VersionDetail({ opt }: { opt: ModVersionOption }) {
  const channel = opt.versionType
    ? VERSION_TYPE_LABEL[opt.versionType] ?? opt.versionType
    : null;

  return (
    <div className="mt-2 rounded-md bg-[var(--color-muted)]/30 px-3 py-2 text-xs space-y-1.5">
      <div className="text-[var(--color-muted-foreground)] break-all">
        文件名：{opt.fileName}
      </div>
      <div className="text-[var(--color-muted-foreground)]">
        来源：{SOURCE_PAGE[opt.source] ?? opt.source}
      </div>
      {channel && (
        <div className="text-[var(--color-muted-foreground)]">通道：{channel}</div>
      )}
      {opt.loaders && opt.loaders.length > 0 && (
        <div className="text-[var(--color-muted-foreground)]">
          加载器：{opt.loaders.join(", ")}
        </div>
      )}
      {opt.gameVersions.length > 0 && (
        <div className="text-[var(--color-muted-foreground)] break-words">
          支持 MC：{opt.gameVersions.join(", ")}
        </div>
      )}
      {opt.requiredDependencies != null && opt.requiredDependencies > 0 && (
        <div className="text-[var(--color-muted-foreground)]">
          必需依赖：{opt.requiredDependencies} 个
        </div>
      )}
    </div>
  );
}

export function ModVersionPicker({
  open,
  onOpenChange,
  modInfo,
  currentVersion,
  selectedVersion,
  loading,
  options,
  error,
  onSelect,
}: ModVersionPickerProps) {
  const [query, setQuery] = useState("");
  const [expandedKey, setExpandedKey] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      setQuery("");
      setExpandedKey(null);
    }
  }, [open]);

  const filtered = options.filter((opt) => {
    const q = query.trim().toLowerCase();
    if (!q) return true;
    return (
      opt.version.toLowerCase().includes(q) ||
      opt.fileName.toLowerCase().includes(q)
    );
  });

  const pageUrl = modPageUrl(modInfo);
  const modLabel = modInfo.nameZh ?? modInfo.name;

  const openModPage = async () => {
    if (pageUrl) {
      await openUrl(pageUrl);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[min(85dvh,calc(100vh-2rem))] max-w-lg flex-col">
        <DialogHeader>
          <DialogTitle>选择 Mod 版本</DialogTitle>
          <div className="flex items-start gap-3 pt-1">
            {modInfo.iconUrl && (
              <img
                src={modInfo.iconUrl}
                alt=""
                className="h-10 w-10 rounded-md object-cover shrink-0"
              />
            )}
            <div className="min-w-0 flex-1">
              <p className="text-sm font-medium">{modLabel}</p>
              {modInfo.nameZh && modInfo.name !== modInfo.nameZh && (
                <p className="text-xs text-[var(--color-muted-foreground)]">
                  {modInfo.name}
                </p>
              )}
              {currentVersion && (
                <p className="text-xs text-[var(--color-muted-foreground)] mt-0.5">
                  源端版本：{currentVersion}
                </p>
              )}
            </div>
            {pageUrl && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="shrink-0 text-xs h-8"
                onClick={() => void openModPage()}
              >
                <ExternalLink className="h-3.5 w-3.5 mr-1" />
                查看 Mod
              </Button>
            )}
          </div>
        </DialogHeader>

        <input
          type="text"
          placeholder="搜索版本..."
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="w-full shrink-0 rounded-md border border-[var(--color-border)] bg-transparent px-3 py-2 text-sm"
        />

        <DialogBody className="rounded-md border border-[var(--color-border)]">
          {loading ? (
            <div className="flex items-center justify-center gap-2 py-12 text-sm text-[var(--color-muted-foreground)]">
              <Loader2 className="h-4 w-4 animate-spin" />
              正在加载可用版本...
            </div>
          ) : error ? (
            <p className="p-4 text-sm text-red-400">{error}</p>
          ) : filtered.length === 0 ? (
            <p className="p-4 text-sm text-[var(--color-muted-foreground)]">
              未找到可安装的兼容版本
            </p>
          ) : (
            <ul className="divide-y divide-[var(--color-border)]">
              {filtered.map((opt) => {
                const key = `${opt.version}-${opt.fileName}`;
                const active = selectedVersion === opt.version;
                const expanded = expandedKey === key;
                return (
                  <li key={key}>
                    <div
                      className={cn(
                        "flex items-start gap-1 px-2 py-2 hover:bg-[var(--color-accent)]/40 transition-colors",
                        active && "bg-[var(--color-primary)]/10"
                      )}
                    >
                      <button
                        type="button"
                        onClick={() => onSelect(opt)}
                        className="flex-1 min-w-0 text-left px-1 py-0.5"
                      >
                        <div className="flex items-center gap-2 flex-nowrap">
                          <span className="font-medium text-sm">{opt.version}</span>
                          {opt.recommended && (
                            <Badge variant="success" className="text-[10px] px-1.5 py-0">
                              推荐
                            </Badge>
                          )}
                          {opt.versionType && opt.versionType !== "release" && (
                            <Badge variant="secondary" className="text-[10px] px-1.5 py-0">
                              {VERSION_TYPE_LABEL[opt.versionType] ?? opt.versionType}
                            </Badge>
                          )}
                        </div>
                        <div className="text-xs text-[var(--color-muted-foreground)] truncate mt-0.5">
                          {opt.fileName}
                        </div>
                        {opt.gameVersions.length > 0 && (
                          <div className="text-[10px] text-[var(--color-muted-foreground)] mt-1 truncate">
                            MC: {opt.gameVersions.slice(0, 4).join(", ")}
                            {opt.gameVersions.length > 4 ? "…" : ""}
                          </div>
                        )}
                      </button>
                      <button
                        type="button"
                        title="查看版本详情"
                        aria-expanded={expanded}
                        onClick={() => setExpandedKey(expanded ? null : key)}
                        className={cn(
                          "shrink-0 p-2 rounded-md text-[var(--color-muted-foreground)] hover:text-[var(--color-foreground)] hover:bg-[var(--color-accent)]/60 transition-colors",
                          expanded && "text-[var(--color-primary)]"
                        )}
                      >
                        <ChevronDown
                          className={cn(
                            "h-4 w-4 transition-transform",
                            expanded && "rotate-180"
                          )}
                        />
                      </button>
                    </div>
                    {expanded && (
                      <div className="px-3 pb-3">
                        <VersionDetail opt={opt} />
                        <button
                          type="button"
                          onClick={() => onSelect(opt)}
                          className="mt-2 w-full text-xs py-1.5 rounded-md border border-[var(--color-border)] hover:bg-[var(--color-accent)]/40 transition-colors flex items-center justify-center gap-1"
                        >
                          <Info className="h-3 w-3" />
                          选择此版本
                        </button>
                      </div>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </DialogBody>
      </DialogContent>
    </Dialog>
  );
}
