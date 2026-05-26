import { Search } from "lucide-react";
import { ModVersionPicker } from "@/components/ModVersionPicker";
import { useMemo, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import type { IdentifiedMod, ModTransferItem, ModVersionOption, TargetEnv, TransferStatus } from "@/types";
import { cn } from "@/lib/utils";

interface ModTableProps {
  items: ModTransferItem[];
  onToggle: (index: number, selected: boolean) => void;
  onToggleAll: (selected: boolean) => void;
  showCheckboxes?: boolean;
  scanning?: boolean;
  checking?: boolean;
  target?: TargetEnv | null;
  sourceMods?: IdentifiedMod[];
  onVersionSelect?: (index: number, option: ModVersionOption) => void;
  onLoadVersions?: (
    item: ModTransferItem,
    target: TargetEnv,
    sourceMods: IdentifiedMod[]
  ) => Promise<ModVersionOption[]>;
  onSearchMarket?: (modName: string) => void;
}

const STATUS_LABEL: Record<TransferStatus, string> = {
  transferable: "可迁移",
  up_to_date: "已最新",
  incompatible: "不兼容",
  unknown: "未识别",
};

const STATUS_HINT: Record<TransferStatus, string> = {
  transferable: "已找到目标端适配版（优先精确 MC + 稳定正式版，同版本线内均可）",
  up_to_date: "目标 mods 文件夹中已有该 Mod 的适配版本，无需重复下载",
  incompatible:
    "未找到适配文件。常见原因：① 目标加载器与 Mod 不一致（如 Fabric Mod 迁到 Forge）；② 目标 MC/加载器识别有误，请在左侧手动修正；③ 网络或镜像源异常",
  unknown: "未能识别 Mod 来源，无法查询远程仓库",
};

const STATUS_VARIANT: Record<
  TransferStatus,
  "success" | "secondary" | "destructive" | "warning"
> = {
  transferable: "success",
  up_to_date: "secondary",
  incompatible: "destructive",
  unknown: "warning",
};

const SOURCE_LABEL: Record<string, string> = {
  modrinth: "Modrinth",
  curseforge: "CurseForge",
  metadata: "本地",
  github: "GitHub",
  unknown: "未知",
};

const SOURCE_HINT: Record<string, string> = {
  modrinth: "Modrinth",
  curseforge: "CurseForge",
  metadata: "本地元数据（从 jar 内识别）",
  github: "GitHub",
  unknown: "未知来源",
};

export function ModTable({
  items,
  onToggle,
  onToggleAll,
  showCheckboxes = true,
  scanning = false,
  checking = false,
  target = null,
  sourceMods = [],
  onVersionSelect,
  onLoadVersions,
  onSearchMarket,
}: ModTableProps) {
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<"all" | "transferable" | "hide_unknown">(
    "all"
  );
  const [pickerIndex, setPickerIndex] = useState<number | null>(null);
  const [versionOptions, setVersionOptions] = useState<ModVersionOption[]>([]);
  const [versionsLoading, setVersionsLoading] = useState(false);
  const [versionsError, setVersionsError] = useState<string | null>(null);

  const openVersionPicker = async (globalIndex: number) => {
    if (!target || !onLoadVersions || !onVersionSelect) return;
    const item = items[globalIndex];
    if (item.status !== "transferable" && item.status !== "up_to_date") return;

    setPickerIndex(globalIndex);
    setVersionsLoading(true);
    setVersionsError(null);
    setVersionOptions([]);
    try {
      const list = await onLoadVersions(item, target, sourceMods);
      setVersionOptions(list);
    } catch (e) {
      setVersionsError(String(e));
    } finally {
      setVersionsLoading(false);
    }
  };

  const filtered = useMemo(() => {
    return items.filter((item) => {
      const name = item.mod.name.toLowerCase();
      const q = search.toLowerCase();
      if (q && !name.includes(q) && !item.mod.fileName.toLowerCase().includes(q)) {
        return false;
      }
      if (filter === "transferable" && item.status !== "transferable") {
        return false;
      }
      if (filter === "hide_unknown" && item.status === "unknown") {
        return false;
      }
      return true;
    });
  }, [items, search, filter]);

  const transferableCount = items.filter((i) => i.status === "transferable").length;
  const allSelected = items
    .filter((i) => i.status === "transferable")
    .every((i) => i.selected);

  if (items.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-64 text-[var(--color-muted-foreground)] gap-2">
        {scanning ? (
          <p>正在扫描识别 Mod...</p>
        ) : checking ? (
          <p>正在检查兼容性...</p>
        ) : (
          <>
            <p>选择源实例版本文件夹并点击「扫描识别」</p>
            <p className="text-xs opacity-70">
              例如 .minecraft/versions/1.21.x-Fabric（会自动使用其中的 mods 目录）
            </p>
          </>
        )}
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 p-3 border-b border-[var(--color-border)]">
        <div className="relative flex-1">
          <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-[var(--color-muted-foreground)]" />
          <Input
            placeholder="搜索 Mod..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="pl-8"
          />
        </div>
        <Button
          variant={filter === "all" ? "secondary" : "ghost"}
          size="sm"
          onClick={() => setFilter("all")}
        >
          全部
        </Button>
        <Button
          variant={filter === "transferable" ? "secondary" : "ghost"}
          size="sm"
          onClick={() => setFilter("transferable")}
        >
          可迁移 ({transferableCount})
        </Button>
        <Button
          variant={filter === "hide_unknown" ? "secondary" : "ghost"}
          size="sm"
          onClick={() => setFilter("hide_unknown")}
        >
          隐藏未识别
        </Button>
      </div>

      <div className="px-3 pb-2 text-[11px] text-[var(--color-muted-foreground)] leading-relaxed border-b border-[var(--color-border)]">
        状态说明：
        <span className="text-emerald-400/90">可迁移</span>=找到目标端适配版 ·
        <span className="mx-1">已最新</span>=目标目录已有 ·
        <span className="text-red-400/90">不兼容</span>=未找到目标版本/加载器的下载 ·
        <span className="text-amber-400/90">未识别</span>=无法识别 Mod 来源
      </div>

      {showCheckboxes && (
        <div className="flex items-center gap-2 px-3 py-2 border-b border-[var(--color-border)] text-sm">
          <Checkbox
            checked={allSelected && transferableCount > 0}
            onCheckedChange={(v) => onToggleAll(!!v)}
          />
          <span className="text-[var(--color-muted-foreground)]">全选可迁移项</span>
        </div>
      )}

      <div className="flex-1 overflow-auto min-w-0">
        <table className="w-max min-w-full text-sm">
          <thead className="sticky top-0 bg-[var(--color-card)] border-b border-[var(--color-border)]">
            <tr className="text-left text-[var(--color-muted-foreground)]">
              {showCheckboxes && <th className="p-3 w-10" />}
              <th className="p-3 whitespace-nowrap">Mod 名称</th>
              <th className="p-3 w-24 whitespace-nowrap">来源</th>
              <th className="p-3 w-24 whitespace-nowrap">状态</th>
              <th className="p-3 w-32 whitespace-nowrap">目标版本</th>
              {onSearchMarket && <th className="p-3 w-24 whitespace-nowrap">市场</th>}
            </tr>
          </thead>
          <tbody>
            {filtered.map((item) => {
              const globalIndex = items.indexOf(item);
              return (
                <tr
                  key={item.mod.sha512 + item.mod.fileName}
                  className={cn(
                    "border-b border-[var(--color-border)]/50 hover:bg-[var(--color-accent)]/30",
                    item.status === "transferable" && item.selected && "bg-[var(--color-primary)]/5"
                  )}
                >
                  {showCheckboxes && (
                    <td className="p-3">
                      <Checkbox
                        checked={item.selected}
                        disabled={item.status !== "transferable"}
                        onCheckedChange={(v) => onToggle(globalIndex, !!v)}
                      />
                    </td>
                  )}
                  <td className="p-3 whitespace-nowrap">
                    <div className="font-medium inline-flex items-center gap-2 max-w-none">
                      {item.mod.name}
                      {item.isDependency && (
                        <Badge variant="warning" className="text-[10px] px-1.5 py-0 shrink-0">
                          依赖
                        </Badge>
                      )}
                    </div>
                    {item.mod.nameZh && (
                      <div className="text-xs text-[var(--color-muted-foreground)]">
                        {item.mod.nameZh}
                      </div>
                    )}
                    {item.requiredBy && (
                      <div className="text-xs text-amber-500/80">
                        被 {item.requiredBy} 需要
                      </div>
                    )}
                    <div className="text-xs text-[var(--color-muted-foreground)]">
                      {item.mod.fileName}
                    </div>
                  </td>
                  <td className="p-3 whitespace-nowrap">
                    <Badge
                      variant="secondary"
                      title={SOURCE_HINT[item.mod.source] ?? item.mod.source}
                    >
                      {SOURCE_LABEL[item.mod.source] ?? item.mod.source}
                    </Badge>
                  </td>
                  <td className="p-3 whitespace-nowrap">
                    <Badge
                      variant={STATUS_VARIANT[item.status]}
                      title={STATUS_HINT[item.status]}
                    >
                      {STATUS_LABEL[item.status]}
                    </Badge>
                  </td>
                  <td className="p-3 text-xs text-[var(--color-muted-foreground)] whitespace-nowrap">
                    {item.mod.currentVersion && item.targetVersion ? (
                      <button
                        type="button"
                        disabled={!onLoadVersions || !target || checking}
                        onClick={() => openVersionPicker(globalIndex)}
                        className={cn(
                          "text-left hover:text-[var(--color-primary)] transition-colors",
                          onLoadVersions && target && (item.status === "transferable" || item.status === "up_to_date")
                            ? "underline-offset-2 hover:underline cursor-pointer"
                            : "cursor-default"
                        )}
                        title={
                          onLoadVersions && target
                            ? "点击选择其他兼容版本"
                            : undefined
                        }
                      >
                        {item.mod.currentVersion}
                        <span className="mx-1 text-[var(--color-primary)]">→</span>
                        {item.targetVersion}
                      </button>
                    ) : (
                      item.targetVersion ?? "—"
                    )}
                  </td>
                  {onSearchMarket && (
                    <td className="p-3 whitespace-nowrap">
                      {(item.status === "incompatible" || !item.downloadUrl) && (
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-7 text-xs"
                          onClick={() => onSearchMarket(item.mod.name || item.mod.fileName)}
                        >
                          <Search className="h-3 w-3" />
                          搜索
                        </Button>
                      )}
                    </td>
                  )}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      {pickerIndex !== null && items[pickerIndex] && (
        <ModVersionPicker
          open
          onOpenChange={(open) => {
            if (!open) setPickerIndex(null);
          }}
          modInfo={items[pickerIndex].mod}
          currentVersion={items[pickerIndex].mod.currentVersion}
          selectedVersion={items[pickerIndex].targetVersion}
          loading={versionsLoading}
          options={versionOptions}
          error={versionsError}
          onSelect={(option) => {
            onVersionSelect?.(pickerIndex, option);
            setPickerIndex(null);
          }}
        />
      )}
    </div>
  );
}
