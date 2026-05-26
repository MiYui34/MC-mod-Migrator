import { ASSET_STATUS_LABELS, formatBytes, type FileAssetCategory, type FileAssetTransferItem, type InstanceInfo } from "@/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Download, Search } from "lucide-react";
import { useState } from "react";

interface AssetTableProps {
  category: FileAssetCategory;
  items: FileAssetTransferItem[];
  scanning?: boolean;
  hint?: string | null;
  configScanMode?: "related" | "all";
  onConfigModeChange?: (mode: "related" | "all") => void;
  shaderIncludeSettings?: boolean;
  onShaderIncludeSettingsChange?: (include: boolean) => void;
  modsScanned?: boolean;
  mcVersionWarning?: boolean;
  onToggle: (index: number, selected: boolean) => void;
  onToggleAll: (selected: boolean) => void;
  targetInstance?: InstanceInfo | null;
  onSearchMarket?: (query: string) => void;
  onInstallFromMarket?: (downloadUrl: string, fileName: string) => Promise<void>;
}

function statusVariant(status: FileAssetTransferItem["status"]) {
  switch (status) {
    case "transferable":
      return "default";
    case "up_to_date":
      return "secondary";
    case "conflict":
      return "destructive";
    case "online_available":
      return "default";
    case "incompatible":
      return "secondary";
    default:
      return "secondary";
  }
}

export function AssetTable({
  category,
  items,
  scanning,
  hint,
  configScanMode,
  onConfigModeChange,
  shaderIncludeSettings,
  onShaderIncludeSettingsChange,
  modsScanned,
  mcVersionWarning,
  onToggle,
  onToggleAll,
  targetInstance,
  onSearchMarket,
  onInstallFromMarket,
}: AssetTableProps) {
  const [installingIdx, setInstallingIdx] = useState<number | null>(null);
  const marketSupported =
    category === "shader_pack" || category === "resource_pack" || category === "datapack";

  const handleInstall = async (idx: number) => {
    const item = items[idx];
    if (!item.downloadUrl || !onInstallFromMarket) return;
    setInstallingIdx(idx);
    try {
      await onInstallFromMarket(item.downloadUrl, item.asset.name);
    } finally {
      setInstallingIdx(null);
    }
  };
  const selectable = items.filter(
    (i) => i.status !== "up_to_date" && i.status !== "incompatible"
  );
  const allSelected = selectable.length > 0 && selectable.every((i) => i.selected);

  return (
    <div className="flex flex-col h-full">
      {category === "shader_pack" && onShaderIncludeSettingsChange && (
        <div className="px-4 py-2 flex items-center gap-2 border-b border-[var(--color-border)] text-sm">
          <span className="text-[var(--color-muted-foreground)]">同时扫描：</span>
          <button
            type="button"
            className={shaderIncludeSettings ? "font-semibold underline" : ""}
            onClick={() => onShaderIncludeSettingsChange(true)}
          >
            光影包 + 设置
          </button>
          <span>/</span>
          <button
            type="button"
            className={!shaderIncludeSettings ? "font-semibold underline" : ""}
            onClick={() => onShaderIncludeSettingsChange(false)}
          >
            仅光影包
          </button>
          <span className="text-xs text-[var(--color-muted-foreground)] ml-1">
            设置指 shaderpacks 目录下与光影包对应的 .txt（如同名 .txt、文件夹内 .txt）
          </span>
        </div>
      )}
      {category === "mod_config" && onConfigModeChange && (
        <div className="px-4 py-2 flex items-center gap-2 border-b border-[var(--color-border)] text-sm">
          <span className="text-[var(--color-muted-foreground)]">扫描模式：</span>
          <button
            type="button"
            className={configScanMode === "related" ? "font-semibold underline" : ""}
            onClick={() => onConfigModeChange("related")}
          >
            相关配置
          </button>
          <span>/</span>
          <button
            type="button"
            className={configScanMode === "all" ? "font-semibold underline" : ""}
            onClick={() => onConfigModeChange("all")}
          >
            全部配置
          </button>
          {configScanMode === "related" && !modsScanned && (
            <span className="text-amber-500 text-xs ml-2">请先在 Mod 分类扫描识别</span>
          )}
        </div>
      )}
      {mcVersionWarning && (
        <div className="px-4 py-2 text-xs text-amber-500 border-b border-[var(--color-border)]">
          源/目标 MC 版本不同，部分游戏设置换版本后可能无效，进游戏后请检查。
        </div>
      )}
      {hint && items.length === 0 && !scanning && (
        <div className="p-4 text-sm text-[var(--color-muted-foreground)]">{hint}</div>
      )}
      {items.length > 0 && (
        <div className="overflow-auto flex-1 min-w-0">
          <table className="w-max min-w-full text-sm">
            <thead className="sticky top-0 bg-[var(--color-background)] border-b border-[var(--color-border)]">
              <tr>
                <th className="w-10 p-2">
                  <Checkbox
                    checked={allSelected}
                    onCheckedChange={(v) => onToggleAll(Boolean(v))}
                  />
                </th>
                <th className="text-left p-2 whitespace-nowrap">类型</th>
                <th className="text-left p-2 whitespace-nowrap">名称</th>
                <th className="text-left p-2 whitespace-nowrap">路径</th>
                <th className="text-right p-2 whitespace-nowrap">大小</th>
                <th className="text-left p-2 whitespace-nowrap">状态</th>
                {marketSupported && onSearchMarket && (
                  <th className="text-left p-2 whitespace-nowrap">市场</th>
                )}
              </tr>
            </thead>
            <tbody>
              {items.map((item, idx) => (
                <tr
                  key={`${item.asset.relativePath}-${idx}`}
                  className="border-b border-[var(--color-border)]/50 hover:bg-[var(--color-muted)]/30"
                >
                  <td className="p-2">
                    <Checkbox
                      checked={item.selected}
                      disabled={item.status === "up_to_date" || item.status === "incompatible"}
                      onCheckedChange={(v) => onToggle(idx, Boolean(v))}
                    />
                  </td>
                  <td className="p-2 whitespace-nowrap">
                    <Badge variant={item.asset.settingsFile ? "warning" : "secondary"}>
                      {item.asset.settingsFile ? "设置" : category === "shader_pack" ? "光影包" : "文件"}
                    </Badge>
                  </td>
                  <td className="p-2 font-medium whitespace-nowrap">{item.asset.name}</td>
                  <td className="p-2 text-[var(--color-muted-foreground)] whitespace-nowrap">
                    {item.asset.relativePath}
                  </td>
                  <td className="p-2 text-right text-[var(--color-muted-foreground)] whitespace-nowrap">
                    {formatBytes(item.asset.size)}
                  </td>
                  <td className="p-2 whitespace-nowrap">
                    <Badge variant={statusVariant(item.status)}>
                      {ASSET_STATUS_LABELS[item.status]}
                    </Badge>
                  </td>
                  {marketSupported && onSearchMarket && (
                    <td className="p-2 whitespace-nowrap">
                      <div className="flex gap-1">
                        {item.status === "online_available" && item.downloadUrl && onInstallFromMarket && (
                          <Button
                            variant="secondary"
                            size="sm"
                            className="h-7 text-xs"
                            disabled={!targetInstance || installingIdx === idx}
                            onClick={() => void handleInstall(idx)}
                          >
                            <Download className="h-3 w-3" />
                            从市场安装
                          </Button>
                        )}
                        {(item.status === "transferable" ||
                          item.status === "incompatible" ||
                          item.status === "online_available") && (
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-7 text-xs"
                            onClick={() => onSearchMarket(item.asset.name)}
                          >
                            <Search className="h-3 w-3" />
                            搜索
                          </Button>
                        )}
                      </div>
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {scanning && items.length === 0 && (
        <div className="p-8 text-center text-[var(--color-muted-foreground)]">正在扫描...</div>
      )}
    </div>
  );
}
