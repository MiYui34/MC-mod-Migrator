import { invoke } from "@tauri-apps/api/core";
import { GitCompare, Download, FileText, History, Loader2, RefreshCw, Scan, Store, Upload, ArrowUpCircle } from "lucide-react";
import { useState, useEffect } from "react";
import { AssetTable } from "@/components/AssetTable";
import { AssetTransferPreview } from "@/components/AssetTransferPreview";
import { CategoryTabs } from "@/components/CategoryTabs";
import { InstancePicker } from "@/components/InstancePicker";
import { MigrationAlerts } from "@/components/MigrationAlerts";
import { MigrationHistoryDialog } from "@/components/MigrationHistoryDialog";
import { MigrationPresetsPanel } from "@/components/MigrationPresetsPanel";
import { ModDiffView } from "@/components/ModDiffView";
import { ModTable } from "@/components/ModTable";
import {
  TaskProgressBar,
  TransferProgressBar,
  ScanProgressBar,
  CheckProgressBar,
} from "@/components/ProgressBar";
import { TransferPreview } from "@/components/TransferPreview";
import { Button } from "@/components/ui/button";
import { useMods } from "@/contexts/ModsContext";
import { useAppUpdateContext } from "@/contexts/AppUpdateContext";
import { useSettings } from "@/hooks/useMods";
import { useAppVersion } from "@/hooks/useAppVersion";
import { Market } from "@/pages/Market";
import type { ConflictPolicy, FileAssetCategory, InstanceInfo, MarketCategory, MigrationCategory, MigrationRecord, ModDiffKind, OpenMarketOptions } from "@/types";
import { CATEGORY_LABELS } from "@/types";
import { cn } from "@/lib/utils";

interface HomeProps {
  onOpenSettings: () => void;
}

function toTargetEnv(instance: InstanceInfo) {
  return {
    modsPath: instance.modsPath,
    mcVersion: instance.mcVersion,
    loader: instance.loader,
    loaderVersion: instance.loaderVersion ?? "",
  };
}

function isAssetCategory(cat: MigrationCategory): cat is FileAssetCategory {
  return cat !== "mod";
}

const MIGRATION_CATEGORIES: MigrationCategory[] = [
  "mod",
  "shader_pack",
  "resource_pack",
  "datapack",
  "litematica",
  "mod_config",
  "game_settings",
];

function assetCategoryToMarket(cat: FileAssetCategory): MarketCategory | null {
  if (cat === "shader_pack" || cat === "resource_pack" || cat === "datapack") {
    return cat;
  }
  if (cat === "litematica") {
    return "litematic";
  }
  return null;
}

const ONBOARDING_KEY = "onboarding_dismissed";

function isMigrationCategory(value: string): value is MigrationCategory {
  return MIGRATION_CATEGORIES.includes(value as MigrationCategory);
}

export function Home({ onOpenSettings }: HomeProps) {
  const { settings } = useSettings();
  const appVersion = useAppVersion();
  const appUpdate = useAppUpdateContext();
  const {
    sourceInstance,
    targetInstance,
    transferItems,
    scanning,
    checking,
    transferring,
    progress,
    scanProgress,
    checkProgress,
    error,
    sessionLoading,
    launcherInstances,
    pickFolder,
    selectSource,
    selectTarget,
    updateSourceInstance,
    updateTargetInstance,
    selectInstanceDirect,
    scanMods,
    checkCompatibility,
    toggleSelect,
    toggleSelectAll,
    executeTransfer,
    cancelTask,
    loadModVersions,
    selectModVersion,
    mods,
    setError,
    activeCategory,
    setActiveCategory,
    fileAssets,
    configScanMode,
    setConfigScanMode,
    shaderIncludeSettings,
    setShaderIncludeSettings,
    assetScanning,
    assetTransferring,
    assetScanProgress,
    assetTransferProgress,
    assetHint,
    scanAssets,
    transferAssets,
    toggleAssetSelect,
    toggleAssetSelectAll,
    exportManifest,
    importManifest,
    exportModReport,
    loadMigrationHistory,
    deleteMigrationRecord,
    openBackupFolder,
    restoreFromBackup,
    migrationHistory,
    migrationWarnings,
    crossVersionGuide,
    modDiff,
    diffLoading,
    modViewMode,
    setModViewMode,
    compareModDiff,
    applyIncrementalSync,
    retryFailedTransfer,
    lastFailedFileNames,
    migrationPresets,
    saveMigrationPreset,
    deleteMigrationPreset,
    applyMigrationPreset,
  } = useMods();

  const [previewOpen, setPreviewOpen] = useState(false);
  const [assetPreviewOpen, setAssetPreviewOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [resultMsg, setResultMsg] = useState<string | null>(null);
  const [restoringId, setRestoringId] = useState<string | null>(null);
  const [homeMode, setHomeMode] = useState<"migration" | "market">("migration");
  const [marketOptions, setMarketOptions] = useState<OpenMarketOptions | null>(null);
  const [diffFilter, setDiffFilter] = useState<ModDiffKind | "all">("all");
  const [guideDismissed, setGuideDismissed] = useState(false);
  const [alertsDismissed, setAlertsDismissed] = useState(false);
  const [showOnboarding, setShowOnboarding] = useState(
    () => !localStorage.getItem(ONBOARDING_KEY)
  );

  useEffect(() => {
    setAlertsDismissed(false);
  }, [migrationWarnings, crossVersionGuide]);

  const openMarket = (options?: OpenMarketOptions) => {
    setMarketOptions(options ?? null);
    setHomeMode("market");
  };

  const dismissOnboarding = () => {
    localStorage.setItem(ONBOARDING_KEY, "1");
    setShowOnboarding(false);
  };

  const assetCategory = isAssetCategory(activeCategory) ? activeCategory : null;
  const assetItems = assetCategory ? fileAssets[assetCategory] ?? [] : [];

  const handlePickSource = async () => {
    const path = await pickFolder();
    if (path) await selectSource(path);
  };

  const handlePickTarget = async () => {
    const path = await pickFolder();
    if (path) await selectTarget(path);
  };

  const handleScan = async () => {
    if (!sourceInstance) return;
    setResultMsg(null);
    await scanMods(
      sourceInstance.modsPath,
      targetInstance ? toTargetEnv(targetInstance) : null
    );
  };

  const handleCheck = async () => {
    if (!targetInstance) return;
    setResultMsg(null);
    setGuideDismissed(false);
    await checkCompatibility(toTargetEnv(targetInstance));
    if (mods.length > 0) {
      await compareModDiff(mods, targetInstance.modsPath);
    }
  };

  const handleCompareDiff = async () => {
    if (!sourceInstance || !targetInstance || mods.length === 0) return;
    setModViewMode("diff");
    await compareModDiff(mods, targetInstance.modsPath);
  };

  const handleIncrementalSync = () => {
    applyIncrementalSync();
    setModViewMode("list");
    setResultMsg("已勾选目标端缺失且可迁移的 Mod");
  };

  const handleTransfer = async () => {
    if (!targetInstance) return;
    setPreviewOpen(false);
    setResultMsg(null);
    const result = await executeTransfer(toTargetEnv(targetInstance));
    if (result) {
      setResultMsg(
        `完成：成功 ${result.success}，失败 ${result.failed}，跳过 ${result.skipped}` +
          (result.errors.length ? `\n${result.errors.join("\n")}` : "")
      );
      if (settings.auto_export_mod_report) {
        try {
          await exportModReport(settings.mod_report_format);
        } catch {
          /* optional */
        }
      }
      await handleCheck();
    }
  };

  const handleAssetScan = async () => {
    if (!assetCategory || !sourceInstance) return;
    if (assetCategory === "mod_config" && configScanMode === "related" && mods.length === 0) {
      setError("请先在 Mod 分类扫描识别");
      return;
    }
    setResultMsg(null);
    await scanAssets(assetCategory);
  };

  const handleAssetTransfer = async (policy: ConflictPolicy) => {
    if (!assetCategory) return;
    setAssetPreviewOpen(false);
    setResultMsg(null);
    const result = await transferAssets(assetCategory, policy);
    if (result) {
      setResultMsg(
        `${CATEGORY_LABELS[assetCategory]}迁移：成功 ${result.success}，失败 ${result.failed}，跳过 ${result.skipped}` +
          (result.errors.length ? `\n${result.errors.join("\n")}` : "")
      );
      await scanAssets(assetCategory);
    }
  };

  const handleImportManifest = async () => {
    const result = await importManifest();
    if (result?.warnings.length) {
      setResultMsg(result.warnings.join("\n"));
    }
  };

  const openHistory = async () => {
    await loadMigrationHistory();
    setHistoryOpen(true);
  };

  const handleCategoryChange = (category: MigrationCategory) => {
    setResultMsg(null);
    setActiveCategory(category);
  };

  const handleRestoreFromHistory = async (record: MigrationRecord) => {
    if (!record.backupId) {
      setError("此记录无备份，无法撤销。请在设置中开启「迁移前自动备份」后重新迁移。");
      return;
    }
    if (
      !window.confirm(
        "撤销将把目标实例恢复为迁移前状态：已覆盖的文件从备份还原，本次新添加的文件将被删除。是否继续？"
      )
    ) {
      return;
    }
    setRestoringId(record.id);
    setError(null);
    try {
      const result = await restoreFromBackup(record.backupId);
      setHistoryOpen(false);
      if (isMigrationCategory(record.category)) {
        setActiveCategory(record.category);
        if (record.category === "mod") {
          if (targetInstance) {
            await checkCompatibility(toTargetEnv(targetInstance));
          }
        } else {
          await scanAssets(record.category);
        }
      }
      setResultMsg(
        `撤销完成：恢复 ${result.restored} 项` +
          (result.removed ? `，删除 ${result.removed} 项` : "") +
          (result.failed ? `，失败 ${result.failed}` : "") +
          (result.errors.length ? `\n${result.errors.join("\n")}` : "")
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setRestoringId(null);
    }
  };

  const handleReEditFromHistory = async (record: MigrationRecord) => {
    if (!isMigrationCategory(record.category)) {
      setError(`未知分类：${record.category}`);
      return;
    }
    setHistoryOpen(false);
    setResultMsg(null);
    setActiveCategory(record.category);
    if (record.category === "mod") {
      if (mods.length === 0 && sourceInstance) {
        await scanMods(
          sourceInstance.modsPath,
          targetInstance ? toTargetEnv(targetInstance) : null
        );
      } else if (targetInstance) {
        await checkCompatibility(toTargetEnv(targetInstance));
      }
      setResultMsg("已切换到 Mod 分类，请重新选择并迁移");
    } else {
      await scanAssets(record.category);
      setResultMsg(
        `已切换到「${CATEGORY_LABELS[record.category]}」，请重新选择并迁移`
      );
    }
  };

  const selectedCount = transferItems.filter(
    (i) => i.selected && i.status === "transferable"
  ).length;

  const selectedAssetCount = assetItems.filter(
    (i) =>
      i.selected &&
      i.status !== "up_to_date" &&
      i.status !== "incompatible"
  ).length;

  const mcVersionWarning =
    assetCategory === "game_settings" &&
    sourceInstance &&
    targetInstance &&
    sourceInstance.mcVersion !== "unknown" &&
    targetInstance.mcVersion !== "unknown" &&
    sourceInstance.mcVersion !== targetInstance.mcVersion;

  if (sessionLoading) {
    return (
      <div className="flex items-center justify-center h-screen text-[var(--color-muted-foreground)] gap-2">
        <Loader2 className="h-5 w-5 animate-spin" />
        正在恢复上次工作区...
      </div>
    );
  }

  return (
    <div className="flex flex-col h-screen">
      <header className="flex items-center justify-between border-b border-[var(--color-border)] px-6 py-3">
        <div>
          <h1 className="text-lg font-semibold flex items-center gap-2">
            MC 换端助手
            {appVersion ? (
              <span className="text-xs font-normal text-[var(--color-muted-foreground)] border border-[var(--color-border)] rounded px-1.5 py-0.5 tabular-nums">
                v{appVersion}
              </span>
            ) : null}
            {appUpdate.updateStatus === "downloaded" && appUpdate.downloadedPath ? (
              <button
                type="button"
                onClick={() => void appUpdate.installUpdate(appUpdate.downloadedPath!)}
                className="inline-flex items-center gap-1 text-xs font-normal text-emerald-400 border border-emerald-500/40 bg-emerald-500/10 rounded px-1.5 py-0.5 hover:bg-emerald-500/20"
              >
                <RefreshCw className="h-3 w-3" />
                重启安装
              </button>
            ) : appUpdate.updateStatus === "available" ||
              appUpdate.updateStatus === "downloading" ||
              (appUpdate.manifest && appUpdate.updateStatus !== "idle") ? (
              <button
                type="button"
                onClick={() => appUpdate.openUpdateDialog()}
                className="inline-flex items-center gap-1 text-xs font-normal text-amber-400 border border-amber-500/40 bg-amber-500/10 rounded px-1.5 py-0.5 hover:bg-amber-500/20"
              >
                {appUpdate.updateStatus === "downloading" ? (
                  <Loader2 className="h-3 w-3 animate-spin" />
                ) : (
                  <span className="h-1.5 w-1.5 rounded-full bg-amber-400" />
                )}
                {appUpdate.updateStatus === "downloading"
                  ? "更新下载中"
                  : appUpdate.manifest
                    ? `新版本 ${appUpdate.manifest.version}`
                    : "检查更新"}
              </button>
            ) : (
              <button
                type="button"
                onClick={() => void appUpdate.checkNow()}
                disabled={appUpdate.checking}
                className="inline-flex items-center gap-1 text-xs font-normal text-[var(--color-muted-foreground)] border border-[var(--color-border)] rounded px-1.5 py-0.5 hover:bg-[var(--color-muted)]/50 disabled:opacity-50"
              >
                {appUpdate.checking ? (
                  <Loader2 className="h-3 w-3 animate-spin" />
                ) : (
                  <ArrowUpCircle className="h-3 w-3" />
                )}
                检查更新
              </button>
            )}
          </h1>
          <p className="text-xs text-[var(--color-muted-foreground)]">
            {homeMode === "migration"
              ? "Mod / 光影 / 材质 / 数据包 / 投影 / 配置 / 游戏设置 一站式迁移"
              : "从 Modrinth / CurseForge 搜索并下载资源"}
          </p>
        </div>
        <div className="flex items-center gap-1 shrink-0 flex-nowrap">
          <div className="flex rounded-md border border-[var(--color-border)] mr-2 overflow-hidden">
            <button
              type="button"
              onClick={() => {
                setHomeMode("migration");
                setResultMsg(null);
              }}
              className={cn(
                "px-3 py-1.5 text-xs transition-colors",
                homeMode === "migration"
                  ? "bg-[var(--color-primary)] text-[var(--color-primary-foreground)]"
                  : "hover:bg-[var(--color-muted)] text-[var(--color-muted-foreground)]"
              )}
            >
              迁移
            </button>
            <button
              type="button"
              onClick={() => {
                setHomeMode("market");
                setResultMsg(null);
              }}
              className={cn(
                "px-3 py-1.5 text-xs transition-colors flex items-center gap-1",
                homeMode === "market"
                  ? "bg-[var(--color-primary)] text-[var(--color-primary-foreground)]"
                  : "hover:bg-[var(--color-muted)] text-[var(--color-muted-foreground)]"
              )}
            >
              <Store className="h-3.5 w-3.5" />
              市场
            </button>
          </div>
          <Button variant="ghost" size="sm" onClick={() => void exportManifest()} title="导出清单">
            <Download className="h-4 w-4" />
            导出清单
          </Button>
          <Button variant="ghost" size="sm" onClick={() => void handleImportManifest()} title="导入清单">
            <Upload className="h-4 w-4" />
            导入清单
          </Button>
          <Button variant="ghost" size="sm" onClick={() => void openHistory()} title="迁移历史">
            <History className="h-4 w-4" />
            历史
          </Button>
          <Button variant="ghost" size="sm" onClick={onOpenSettings}>
            设置
          </Button>
        </div>
      </header>

      {homeMode === "migration" && (
        <CategoryTabs active={activeCategory} onChange={handleCategoryChange} />
      )}

      <div className="flex flex-1 min-h-0 items-stretch">
        <aside className="w-[24rem] shrink-0 border-r border-[var(--color-border)] flex flex-col bg-[var(--color-background)]">
          <div className="px-4 pt-4 pb-2 space-y-3 shrink-0">
            {homeMode === "migration" && (
              <InstancePicker
                label="源实例"
                instance={sourceInstance}
                onPickFolder={handlePickSource}
                onSelectInstance={(inst) => selectInstanceDirect(inst, "source")}
                onUpdateInstance={updateSourceInstance}
                launcherInstances={launcherInstances}
                loading={scanning || assetScanning}
              />
            )}
            <InstancePicker
              label={homeMode === "market" ? "目标实例（安装到）" : "目标实例"}
              instance={targetInstance}
              onPickFolder={handlePickTarget}
              onSelectInstance={(inst) => selectInstanceDirect(inst, "target")}
              onUpdateInstance={updateTargetInstance}
              launcherInstances={launcherInstances}
              loading={scanning || assetScanning}
            />
          </div>

          {homeMode === "migration" && (
          <div className="shrink-0 px-4 pb-4 pt-2 space-y-2 border-t border-[var(--color-border)] bg-[var(--color-background)]">
            {activeCategory === "mod" ? (
              <>
                <Button
                  className="w-full"
                  onClick={handleScan}
                  disabled={!sourceInstance || scanning || checking}
                >
                  {scanning ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Scan className="h-4 w-4" />
                  )}
                  扫描识别
                </Button>
                <Button
                  variant="secondary"
                  className="w-full"
                  onClick={handleCheck}
                  disabled={!targetInstance || transferItems.length === 0 || checking}
                >
                  {checking ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <RefreshCw className="h-4 w-4" />
                  )}
                  检查兼容性
                </Button>
                <Button
                  variant="outline"
                  className="w-full"
                  onClick={() => void handleCompareDiff()}
                  disabled={!targetInstance || mods.length === 0 || diffLoading}
                >
                  {diffLoading ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <GitCompare className="h-4 w-4" />
                  )}
                  对比差异
                </Button>
                <Button
                  variant="outline"
                  className="w-full"
                  onClick={handleIncrementalSync}
                  disabled={!modDiff || modDiff.summary.onlyInSource === 0}
                >
                  轻量同步（仅缺失）
                </Button>
                {lastFailedFileNames.length > 0 && (
                  <Button variant="outline" className="w-full" onClick={retryFailedTransfer}>
                    重试失败项 ({lastFailedFileNames.length})
                  </Button>
                )}
                <Button
                  variant="secondary"
                  className="w-full"
                  onClick={() => void exportModReport("md")}
                  disabled={transferItems.length === 0}
                >
                  <FileText className="h-4 w-4" />
                  导出报告
                </Button>
                <Button
                  className="w-full"
                  onClick={() => setPreviewOpen(true)}
                  disabled={selectedCount === 0 || transferring}
                >
                  开始迁移 ({selectedCount})
                </Button>
                <MigrationPresetsPanel
                  presets={migrationPresets}
                  sourceInstance={sourceInstance}
                  targetInstance={targetInstance}
                  onSave={saveMigrationPreset}
                  onDelete={deleteMigrationPreset}
                  onApply={applyMigrationPreset}
                />
              </>
            ) : (
              <>
                <Button
                  className="w-full"
                  onClick={handleAssetScan}
                  disabled={
                    !sourceInstance ||
                    assetScanning ||
                    assetTransferring ||
                    (assetCategory === "mod_config" &&
                      configScanMode === "related" &&
                      mods.length === 0)
                  }
                >
                  {assetScanning ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Scan className="h-4 w-4" />
                  )}
                  扫描
                </Button>
                <Button
                  className="w-full"
                  onClick={() => setAssetPreviewOpen(true)}
                  disabled={selectedAssetCount === 0 || assetTransferring || !targetInstance}
                >
                  开始迁移 ({selectedAssetCount})
                </Button>
              </>
            )}
          </div>
          )}
        </aside>

        <main className="flex-1 flex flex-col min-w-0">
          {showOnboarding && (
            <div className="mx-4 mt-3 rounded-md bg-[var(--color-primary)]/10 border border-[var(--color-primary)]/30 px-3 py-2 text-sm flex justify-between gap-2 items-start">
              <span>
                使用提示：先在左侧选择<strong className="mx-1">目标实例</strong>，然后在「迁移」中换端复制资源，或在「市场」中从 Modrinth / CurseForge 下载安装。
              </span>
              <button type="button" onClick={dismissOnboarding} className="underline shrink-0 text-xs">
                知道了
              </button>
            </div>
          )}
          {error && (
            <div className="mx-4 mt-3 rounded-md bg-red-500/10 border border-red-500/30 px-3 py-2 text-sm text-red-400 flex justify-between">
              <span>{error}</span>
              <button type="button" onClick={() => setError(null)} className="underline">
                关闭
              </button>
            </div>
          )}
          {resultMsg && (
            <div className="mx-4 mt-3 rounded-md bg-emerald-500/10 border border-emerald-500/30 px-3 py-2 text-sm text-emerald-400 whitespace-pre-wrap flex justify-between gap-2">
              <span>{resultMsg}</span>
              <button type="button" onClick={() => setResultMsg(null)} className="underline shrink-0">
                关闭
              </button>
            </div>
          )}
          <div className="flex-1 min-h-0">
            {homeMode === "market" ? (
              <Market
                targetInstance={targetInstance}
                initialOptions={marketOptions}
                onError={setError}
                onSuccess={setResultMsg}
                onCancelTask={cancelTask}
              />
            ) : activeCategory === "mod" ? (
              <div className="flex flex-col h-full">
                <div className="flex gap-1 px-4 py-2 border-b border-[var(--color-border)]">
                  <button
                    type="button"
                    onClick={() => setModViewMode("list")}
                    className={cn(
                      "rounded-md px-3 py-1 text-xs",
                      modViewMode === "list"
                        ? "bg-[var(--color-primary)] text-[var(--color-primary-foreground)]"
                        : "text-[var(--color-muted-foreground)] hover:bg-[var(--color-muted)]"
                    )}
                  >
                    列表
                  </button>
                  <button
                    type="button"
                    onClick={() => setModViewMode("diff")}
                    className={cn(
                      "rounded-md px-3 py-1 text-xs flex items-center gap-1",
                      modViewMode === "diff"
                        ? "bg-[var(--color-primary)] text-[var(--color-primary-foreground)]"
                        : "text-[var(--color-muted-foreground)] hover:bg-[var(--color-muted)]"
                    )}
                  >
                    <GitCompare className="h-3 w-3" />
                    对比
                    {modDiff ? ` (${modDiff.summary.onlyInSource + modDiff.summary.versionMismatch})` : ""}
                  </button>
                </div>
                {!alertsDismissed && (migrationWarnings.length > 0 || crossVersionGuide) && (
                  <MigrationAlerts
                    warnings={migrationWarnings}
                    crossVersionGuide={guideDismissed ? null : crossVersionGuide}
                    onDismissGuide={() => setGuideDismissed(true)}
                    onDismiss={() => setAlertsDismissed(true)}
                  />
                )}
                {modViewMode === "diff" ? (
                  <ModDiffView
                    entries={modDiff?.entries ?? []}
                    summary={
                      modDiff?.summary ?? {
                        onlyInSource: 0,
                        onlyInTarget: 0,
                        versionMismatch: 0,
                        matched: 0,
                      }
                    }
                    loading={diffLoading}
                    filter={diffFilter}
                    onFilterChange={setDiffFilter}
                  />
                ) : (
                  <ModTable
                    items={transferItems}
                    onToggle={toggleSelect}
                    onToggleAll={toggleSelectAll}
                    scanning={scanning}
                    checking={checking}
                    target={targetInstance ? toTargetEnv(targetInstance) : null}
                    sourceMods={mods}
                    onLoadVersions={loadModVersions}
                    onVersionSelect={selectModVersion}
                    onSearchMarket={(name) => openMarket({ category: "mod", query: name })}
                  />
                )}
              </div>
            ) : assetCategory ? (
              <AssetTable
                category={assetCategory}
                items={assetItems}
                scanning={assetScanning}
                hint={assetHint}
                configScanMode={configScanMode}
                onConfigModeChange={setConfigScanMode}
                shaderIncludeSettings={shaderIncludeSettings}
                onShaderIncludeSettingsChange={setShaderIncludeSettings}
                modsScanned={mods.length > 0}
                mcVersionWarning={Boolean(mcVersionWarning)}
                onToggle={(idx, sel) => toggleAssetSelect(assetCategory, idx, sel)}
                onToggleAll={(sel) => toggleAssetSelectAll(assetCategory, sel)}
                targetInstance={targetInstance}
                onSearchMarket={(query) => {
                  const mc = assetCategoryToMarket(assetCategory);
                  if (mc) openMarket({ category: mc, query });
                }}
                onInstallFromMarket={async (downloadUrl, fileName) => {
                  const mc = assetCategoryToMarket(assetCategory);
                  if (!mc || !targetInstance) return;
                  await invoke("market_install_from_asset_cmd", {
                    category: mc,
                    downloadUrl,
                    fileName,
                    target: targetInstance,
                  });
                  setResultMsg(`已从市场安装 ${fileName}`);
                }}
              />
            ) : null}
          </div>
          {homeMode === "migration" && activeCategory === "mod" ? (
            <>
              <ScanProgressBar progress={scanProgress} scanning={scanning} onCancel={cancelTask} />
              <CheckProgressBar progress={checkProgress} checking={checking} onCancel={cancelTask} />
              <TransferProgressBar
                progress={progress}
                transferring={transferring}
                onCancel={cancelTask}
              />
            </>
          ) : homeMode === "migration" ? (
            <>
              <TaskProgressBar
                progress={assetScanProgress}
                active={assetScanning}
                idleMessage="正在扫描资源..."
                onCancel={cancelTask}
              />
              <TaskProgressBar
                progress={assetTransferProgress}
                active={assetTransferring}
                idleMessage="正在迁移资源..."
                onCancel={cancelTask}
              />
            </>
          ) : null}
        </main>
      </div>

      {homeMode === "migration" && targetInstance && (
        <TransferPreview
          open={previewOpen}
          onOpenChange={setPreviewOpen}
          items={transferItems}
          targetPath={targetInstance.modsPath}
          targetVersion={targetInstance.mcVersion}
          targetLoader={targetInstance.loader}
          targetLoaderVersion={targetInstance.loaderVersion}
          onConfirm={handleTransfer}
          transferring={transferring}
        />
      )}

      {homeMode === "migration" && assetCategory && (
        <AssetTransferPreview
          open={assetPreviewOpen}
          onOpenChange={setAssetPreviewOpen}
          items={assetItems}
          categoryLabel={CATEGORY_LABELS[assetCategory]}
          onConfirm={handleAssetTransfer}
          transferring={assetTransferring}
        />
      )}

      <MigrationHistoryDialog
        open={historyOpen}
        onOpenChange={setHistoryOpen}
        records={migrationHistory}
        onRefresh={() => void loadMigrationHistory()}
        onDelete={(id) => void deleteMigrationRecord(id)}
        onOpenBackup={(id) => void openBackupFolder(id)}
        onRestore={(record) => void handleRestoreFromHistory(record)}
        onReEdit={(record) => void handleReEditFromHistory(record)}
        restoringId={restoringId}
      />
    </div>
  );
}
