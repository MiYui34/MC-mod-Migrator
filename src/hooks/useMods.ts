import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState, startTransition } from "react";
import type {
  AppSession,
  AppSettings,
  ConfigScanMode,
  ConflictPolicy,
  FileAssetCategory,
  FileAssetScanResult,
  FileAssetTransferItem,
  IdentifiedMod,
  ImportManifestResult,
  InstanceInfo,
  MigrationCategory,
  MigrationRecord,
  CompatibilityCheckResponse,
  CrossVersionGuide,
  MigrationPreset,
  MigrationWarning,
  ModDiffResult,
  ModTransferItem,
  ModTransferResponse,
  ModVersionOption,
  RestoreResult,
  TargetEnv,
  TransferProgress,
  TransferResult,
} from "@/types";
import {
  instancesSameGameFolder,
  SAME_GAME_VERSION_FOLDER_ERROR,
} from "@/lib/instances";

const CANCELLED_MSG = "操作已取消";

function isCancelledError(e: unknown): boolean {
  return String(e).includes(CANCELLED_MSG);
}

/** 忽略乱序到达的旧进度，避免并行检查时进度条回退 */
function mergeMonotonicProgress(
  prev: TransferProgress | null,
  next: TransferProgress
): TransferProgress {
  if (next.message.includes("完成")) {
    return next;
  }
  // Dependency resolution / prefetch phases use total=0 status-only updates.
  if (next.total === 0) {
    return next;
  }
  if (next.current >= next.total) {
    return next;
  }
  const minCurrent = prev?.total === next.total ? (prev?.current ?? 0) : 0;
  if (next.current >= minCurrent) {
    return next;
  }
  return { ...next, current: minCurrent };
}

export function useModsState() {
  const [sourceInstance, setSourceInstance] = useState<InstanceInfo | null>(null);
  const [targetInstance, setTargetInstance] = useState<InstanceInfo | null>(null);
  const [mods, setMods] = useState<IdentifiedMod[]>([]);
  const [transferItems, setTransferItems] = useState<ModTransferItem[]>([]);
  const [sessionLoading, setSessionLoading] = useState(true);
  const sessionReady = useRef(false);
  const skipNextPersist = useRef(true);
  const [launcherLoading, setLauncherLoading] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [transferring, setTransferring] = useState(false);
  const [progress, setProgress] = useState<TransferProgress | null>(null);
  const [scanProgress, setScanProgress] = useState<TransferProgress | null>(null);
  const [checkProgress, setCheckProgress] = useState<TransferProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [launcherInstances, setLauncherInstances] = useState<InstanceInfo[]>([]);
  const [activeCategory, setActiveCategory] = useState<MigrationCategory>("mod");
  const [fileAssets, setFileAssets] = useState<
    Partial<Record<FileAssetCategory, FileAssetTransferItem[]>>
  >({});
  const [configScanMode, setConfigScanMode] = useState<ConfigScanMode>("related");
  const [shaderIncludeSettings, setShaderIncludeSettings] = useState(true);
  const [assetScanning, setAssetScanning] = useState(false);
  const [assetTransferring, setAssetTransferring] = useState(false);
  const [assetScanProgress, setAssetScanProgress] = useState<TransferProgress | null>(null);
  const [assetTransferProgress, setAssetTransferProgress] = useState<TransferProgress | null>(null);
  const [assetHint, setAssetHint] = useState<string | null>(null);
  const [lastTransferResult, setLastTransferResult] = useState<TransferResult | null>(null);
  const [lastTransferredModNames, setLastTransferredModNames] = useState<string[]>([]);
  const [migrationHistory, setMigrationHistory] = useState<MigrationRecord[]>([]);
  const [migrationWarnings, setMigrationWarnings] = useState<MigrationWarning[]>([]);
  const [crossVersionGuide, setCrossVersionGuide] = useState<CrossVersionGuide | null>(null);
  const [modDiff, setModDiff] = useState<ModDiffResult | null>(null);
  const [diffLoading, setDiffLoading] = useState(false);
  const [modViewMode, setModViewMode] = useState<"list" | "diff">("list");
  const [migrationPresets, setMigrationPresets] = useState<MigrationPreset[]>([]);
  const [lastFailedFileNames, setLastFailedFileNames] = useState<string[]>([]);

  const pickFolder = useCallback(async () => {
    return invoke<string | null>("pick_folder");
  }, []);

  const detectInstance = useCallback(async (path: string) => {
    return invoke<InstanceInfo>("detect_instance", { path });
  }, []);

  const loadLauncherInstances = useCallback(async () => {
    setLauncherLoading(true);
    try {
      const instances = await invoke<InstanceInfo[]>("scan_instances");
      setLauncherInstances(instances);
    } catch {
      setLauncherInstances([]);
    } finally {
      setLauncherLoading(false);
    }
  }, []);

  const selectSource = useCallback(
    async (path: string) => {
      setError(null);
      try {
        const instance = await detectInstance(path);
        if (targetInstance && instancesSameGameFolder(instance, targetInstance)) {
          setError(SAME_GAME_VERSION_FOLDER_ERROR);
          return null;
        }
        setSourceInstance(instance);
        return instance;
      } catch (e) {
        setError(String(e));
        return null;
      }
    },
    [detectInstance, targetInstance]
  );

  const selectTarget = useCallback(
    async (path: string) => {
      setError(null);
      try {
        const instance = await detectInstance(path);
        if (sourceInstance && instancesSameGameFolder(sourceInstance, instance)) {
          setError(SAME_GAME_VERSION_FOLDER_ERROR);
          return null;
        }
        setTargetInstance(instance);
        return instance;
      } catch (e) {
        setError(String(e));
        return null;
      }
    },
    [detectInstance, sourceInstance]
  );

  const updateSourceInstance = useCallback(
    (patch: Partial<Pick<InstanceInfo, "mcVersion" | "loader" | "loaderVersion">>) => {
      setSourceInstance((prev) => (prev ? { ...prev, ...patch } : prev));
    },
    []
  );

  const updateTargetInstance = useCallback(
    (patch: Partial<Pick<InstanceInfo, "mcVersion" | "loader" | "loaderVersion">>) => {
      setTargetInstance((prev) => (prev ? { ...prev, ...patch } : prev));
    },
    []
  );

  const selectInstanceDirect = useCallback(
    (instance: InstanceInfo, role: "source" | "target") => {
      if (role === "source") {
        if (targetInstance && instancesSameGameFolder(instance, targetInstance)) {
          setError(SAME_GAME_VERSION_FOLDER_ERROR);
          return;
        }
        setSourceInstance(instance);
      } else {
        if (sourceInstance && instancesSameGameFolder(sourceInstance, instance)) {
          setError(SAME_GAME_VERSION_FOLDER_ERROR);
          return;
        }
        setTargetInstance(instance);
      }
    },
    [sourceInstance, targetInstance]
  );

  const [checking, setChecking] = useState(false);

  const modsToTransferItems = useCallback(
    (list: IdentifiedMod[]): ModTransferItem[] =>
      list.map((m) => ({
        mod: m,
        status: "unknown" as const,
        selected: false,
        isDependency: false,
      })),
    []
  );

  const cancelTask = useCallback(async () => {
    try {
      await invoke("cancel_task");
    } catch {
      /* ignore */
    }
    setScanning(false);
    setChecking(false);
    setTransferring(false);
    setScanProgress(null);
    setCheckProgress(null);
    setProgress(null);
  }, []);

  const runCompatibilityCheck = useCallback(
    async (modList: IdentifiedMod[], target: TargetEnv, errorPrefix?: string) => {
      setChecking(true);
      setCheckProgress(null);
      try {
        const response = await invoke<CompatibilityCheckResponse>("check_mods_compatibility", {
          mods: modList,
          target,
        });
        setTransferItems(response.items);
        setMigrationWarnings(response.warnings ?? []);
        setCrossVersionGuide(response.crossVersionGuide ?? null);
        return response;
      } catch (e) {
        if (!isCancelledError(e)) {
          const msg = String(e);
          setError(errorPrefix ? `${errorPrefix}${msg}` : msg);
        }
        return null;
      } finally {
        setChecking(false);
        setCheckProgress(null);
      }
    },
    []
  );

  const scanMods = useCallback(
    async (modsPath: string, target?: TargetEnv | null) => {
      setScanning(true);
      setError(null);
      setScanProgress(null);
      let result: IdentifiedMod[] = [];
      try {
        result = await invoke<IdentifiedMod[]>("scan_and_identify", {
          modsPath,
        });
        setMods(result);
        setTransferItems(modsToTransferItems(result));
      } catch (e) {
        if (!isCancelledError(e)) {
          setError(String(e));
        }
        return [];
      } finally {
        setScanning(false);
        setScanProgress(null);
      }

      if (target && result.length > 0) {
        await runCompatibilityCheck(result, target, "兼容检查失败，已显示扫描结果：");
      }

      return result;
    },
    [modsToTransferItems, runCompatibilityCheck]
  );

  const checkCompatibility = useCallback(
    async (target: TargetEnv) => {
      if (mods.length === 0) return null;
      return runCompatibilityCheck(mods, target);
    },
    [mods, runCompatibilityCheck]
  );

  const compareModDiff = useCallback(
    async (sourceMods: IdentifiedMod[], targetModsPath: string) => {
      setDiffLoading(true);
      try {
        const result = await invoke<ModDiffResult>("compare_mod_diff_cmd", {
          sourceMods,
          targetModsPath,
        });
        setModDiff(result);
        return result;
      } catch (e) {
        if (!isCancelledError(e)) {
          setError(String(e));
        }
        return null;
      } finally {
        setDiffLoading(false);
      }
    },
    []
  );

  const applyIncrementalSync = useCallback(() => {
    if (!modDiff) return;
    const missingFiles = new Set(
      modDiff.entries
        .filter((e) => e.kind === "only_in_source" && e.source)
        .map((e) => e.source!.fileName)
    );
    setTransferItems((prev) =>
      prev.map((item) => ({
        ...item,
        selected:
          item.status === "transferable" &&
          !item.isDependency &&
          missingFiles.has(item.mod.fileName),
      }))
    );
  }, [modDiff]);

  const retryFailedTransfer = useCallback(() => {
    if (lastFailedFileNames.length === 0) return;
    const failed = new Set(lastFailedFileNames);
    setTransferItems((prev) =>
      prev.map((item) => ({
        ...item,
        selected:
          item.status === "transferable" &&
          !item.isDependency &&
          failed.has(item.mod.name),
      }))
    );
  }, [lastFailedFileNames]);

  const loadMigrationPresets = useCallback(async () => {
    try {
      const presets = await invoke<MigrationPreset[]>("list_migration_presets_cmd");
      setMigrationPresets(presets);
      return presets;
    } catch {
      setMigrationPresets([]);
      return [];
    }
  }, []);

  const saveMigrationPreset = useCallback(async (preset: MigrationPreset) => {
    const saved = await invoke<MigrationPreset>("save_migration_preset_cmd", { preset });
    await loadMigrationPresets();
    return saved;
  }, [loadMigrationPresets]);

  const deleteMigrationPreset = useCallback(
    async (id: string) => {
      await invoke("delete_migration_preset_cmd", { id });
      await loadMigrationPresets();
    },
    [loadMigrationPresets]
  );

  const applyMigrationPreset = useCallback(
    (preset: MigrationPreset) => {
      if (preset.sourceMc || preset.sourceLoader) {
        updateSourceInstance({
          mcVersion: preset.sourceMc || undefined,
          loader: preset.sourceLoader || undefined,
        });
      }
      if (preset.targetMc || preset.targetLoader) {
        updateTargetInstance({
          mcVersion: preset.targetMc || undefined,
          loader: preset.targetLoader || undefined,
        });
      }
    },
    [updateSourceInstance, updateTargetInstance]
  );

  const toggleSelect = useCallback((index: number, selected: boolean) => {
    setTransferItems((prev) =>
      prev.map((item, i) => (i === index ? { ...item, selected } : item))
    );
  }, []);

  const toggleSelectAll = useCallback((selected: boolean) => {
    setTransferItems((prev) =>
      prev.map((item) =>
        item.status === "transferable" && !item.isDependency
          ? { ...item, selected }
          : item
      )
    );
  }, []);

  const executeTransfer = useCallback(
    async (target: TargetEnv) => {
      setTransferring(true);
      setError(null);
      try {
        const response = await invoke<ModTransferResponse>("execute_transfer", {
          items: transferItems,
          sourceMods: mods,
          target,
          sourceInstance: sourceInstance ?? null,
          targetInstanceName: targetInstance?.name ?? null,
        });
        setLastTransferResult(response.result);
        setLastTransferredModNames(response.transferredNames);
        if (response.result.failed > 0) {
          setLastFailedFileNames(
            response.result.errors.map((e) => e.split(":")[0]?.trim()).filter(Boolean)
          );
        } else {
          setLastFailedFileNames([]);
        }
        return response.result;
      } catch (e) {
        if (!isCancelledError(e)) {
          setError(String(e));
        }
        return null;
      } finally {
        setTransferring(false);
        setProgress(null);
      }
    },
    [transferItems, mods, sourceInstance, targetInstance]
  );

  const currentAssetItems = fileAssets[activeCategory as FileAssetCategory] ?? [];

  const scanAssets = useCallback(
    async (category: FileAssetCategory) => {
      if (!sourceInstance) return [];
      setAssetScanning(true);
      setError(null);
      setAssetHint(null);
      setAssetScanProgress(null);
      try {
        const knownModIds = mods
          .map((m) => m.modId)
          .filter((id): id is string => Boolean(id));
        const result = await invoke<FileAssetScanResult>("scan_file_assets_cmd", {
          category,
          source: sourceInstance,
          target: targetInstance ?? null,
          configMode: configScanMode,
          knownModIds,
          autoCheckOnline: false,
          includeShaderSettings: shaderIncludeSettings,
        });
        setFileAssets((prev) => ({ ...prev, [category]: result.items }));
        if (result.hint) setAssetHint(result.hint);
        return result.items;
      } catch (e) {
        if (!isCancelledError(e)) setError(String(e));
        return [];
      } finally {
        setAssetScanning(false);
        setAssetScanProgress(null);
      }
    },
    [sourceInstance, targetInstance, mods, configScanMode, shaderIncludeSettings]
  );

  const transferAssets = useCallback(
    async (category: FileAssetCategory, conflictPolicy: ConflictPolicy = "overwrite") => {
      if (!targetInstance) return null;
      const items = fileAssets[category] ?? [];
      setAssetTransferring(true);
      setError(null);
      try {
        return await invoke<TransferResult>("transfer_file_assets_cmd", {
          category,
          items,
          target: targetInstance,
          source: sourceInstance ?? null,
          conflictPolicy,
          backupEnabled: true,
        });
      } catch (e) {
        if (!isCancelledError(e)) setError(String(e));
        return null;
      } finally {
        setAssetTransferring(false);
        setAssetTransferProgress(null);
      }
    },
    [fileAssets, targetInstance, sourceInstance]
  );

  const toggleAssetSelect = useCallback(
    (category: FileAssetCategory, index: number, selected: boolean) => {
      setFileAssets((prev) => {
        const list = [...(prev[category] ?? [])];
        if (!list[index]) return prev;
        list[index] = { ...list[index], selected };
        return { ...prev, [category]: list };
      });
    },
    []
  );

  const toggleAssetSelectAll = useCallback(
    (category: FileAssetCategory, selected: boolean) => {
      setFileAssets((prev) => {
        const list = (prev[category] ?? []).map((item) =>
          item.status === "up_to_date" || item.status === "incompatible"
            ? item
            : { ...item, selected }
        );
        return { ...prev, [category]: list };
      });
    },
    []
  );

  const exportManifest = useCallback(async () => {
    const path = await invoke<string | null>("pick_save_file", {
      defaultName: "migration-manifest.json",
    });
    if (!path) return;
    const session: AppSession = {
      sourceInstance: sourceInstance ?? undefined,
      targetInstance: targetInstance ?? undefined,
      mods,
      transferItems,
      fileAssets,
      configScanMode,
      shaderIncludeSettings,
      activeCategory,
    };
    await invoke("export_migration_manifest", { session, path });
  }, [
    sourceInstance,
    targetInstance,
    mods,
    transferItems,
    fileAssets,
    configScanMode,
    shaderIncludeSettings,
    activeCategory,
  ]);

  const importManifest = useCallback(async () => {
    const path = await invoke<string | null>("pick_open_file");
    if (!path) return null;
    const session: AppSession = {
      sourceInstance: sourceInstance ?? undefined,
      targetInstance: targetInstance ?? undefined,
      mods,
      transferItems,
      fileAssets,
      configScanMode,
      shaderIncludeSettings,
      activeCategory,
    };
    const result = await invoke<ImportManifestResult>("import_migration_manifest", {
      path,
      session,
    });
    if (result.session.sourceInstance) setSourceInstance(result.session.sourceInstance);
    if (result.session.targetInstance) setTargetInstance(result.session.targetInstance);
    if (result.session.mods?.length) setMods(result.session.mods);
    if (result.session.transferItems?.length) setTransferItems(result.session.transferItems);
    if (result.session.fileAssets) setFileAssets(result.session.fileAssets);
    if (result.session.configScanMode) setConfigScanMode(result.session.configScanMode);
    if (result.session.shaderIncludeSettings !== undefined) {
      setShaderIncludeSettings(result.session.shaderIncludeSettings);
    }
    if (result.session.activeCategory) setActiveCategory(result.session.activeCategory);
    return result;
  }, [
    sourceInstance,
    targetInstance,
    mods,
    transferItems,
    fileAssets,
    configScanMode,
    shaderIncludeSettings,
    activeCategory,
  ]);

  const exportModReport = useCallback(
    async (format: "md" | "txt" = "md") => {
      const path = await invoke<string | null>("pick_save_file", {
        defaultName: `mod-report.${format}`,
      });
      if (!path) return;
      const srcLabel = sourceInstance
        ? `${sourceInstance.mcVersion} (${sourceInstance.name})`
        : "未知";
      const tgtLabel = targetInstance
        ? `${targetInstance.mcVersion} (${targetInstance.name})`
        : "未知";
      await invoke("export_mod_report_cmd", {
        path,
        format,
        items: transferItems,
        result: lastTransferResult,
        sourceLabel: srcLabel,
        targetLabel: tgtLabel,
        transferredNames: lastTransferredModNames,
      });
    },
    [
      sourceInstance,
      targetInstance,
      transferItems,
      lastTransferResult,
      lastTransferredModNames,
    ]
  );

  const loadMigrationHistory = useCallback(async () => {
    const list = await invoke<MigrationRecord[]>("list_migration_history_cmd");
    setMigrationHistory(list);
    return list;
  }, []);

  const deleteMigrationRecord = useCallback(async (id: string) => {
    await invoke("delete_migration_record_cmd", { id });
    setMigrationHistory((prev) => prev.filter((r) => r.id !== id));
  }, []);

  const openBackupFolder = useCallback(async (backupId: string) => {
    await invoke("open_backup_folder", { backupId });
  }, []);

  const restoreFromBackup = useCallback(async (backupId: string) => {
    return invoke<RestoreResult>("restore_from_backup_cmd", { backupId });
  }, []);

  const persistSession = useCallback(async (session: AppSession) => {
    try {
      await invoke("save_app_session", { session });
    } catch {
      /* ignore persistence errors */
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const session = await invoke<AppSession>("get_session");
        if (cancelled) return;

        if (session.sourceInstance) setSourceInstance(session.sourceInstance);
        if (session.targetInstance) setTargetInstance(session.targetInstance);
        if (
          session.sourceInstance &&
          session.targetInstance &&
          instancesSameGameFolder(session.sourceInstance, session.targetInstance)
        ) {
          setError(SAME_GAME_VERSION_FOLDER_ERROR);
        }
        sessionReady.current = true;
        setSessionLoading(false);

        if (session.mods?.length || session.transferItems?.length) {
          startTransition(() => {
            if (session.mods?.length) setMods(session.mods);
            if (session.transferItems?.length) setTransferItems(session.transferItems);
            if (session.fileAssets) setFileAssets(session.fileAssets);
            if (session.configScanMode) setConfigScanMode(session.configScanMode);
            if (session.shaderIncludeSettings !== undefined) {
              setShaderIncludeSettings(session.shaderIncludeSettings);
            }
            if (session.activeCategory) setActiveCategory(session.activeCategory);
            if (session.lastTransferResult) setLastTransferResult(session.lastTransferResult);
            if (session.lastTransferredModNames)
              setLastTransferredModNames(session.lastTransferredModNames);
          });
        }
      } finally {
        if (!cancelled && !sessionReady.current) {
          sessionReady.current = true;
          setSessionLoading(false);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (sessionLoading) return;
    const timer = window.setTimeout(() => {
      void loadLauncherInstances();
    }, 300);
    return () => window.clearTimeout(timer);
  }, [sessionLoading, loadLauncherInstances]);

  useEffect(() => {
    if (!sessionReady.current || sessionLoading) return;
    if (skipNextPersist.current) {
      skipNextPersist.current = false;
      return;
    }
    const timer = window.setTimeout(() => {
      persistSession({
        sourceInstance: sourceInstance ?? undefined,
        targetInstance: targetInstance ?? undefined,
        mods,
        transferItems,
        fileAssets,
        configScanMode,
        shaderIncludeSettings,
        activeCategory,
        lastTransferResult: lastTransferResult ?? undefined,
        lastTransferredModNames,
      });
    }, 1500);
    return () => window.clearTimeout(timer);
  }, [
    sourceInstance,
    targetInstance,
    mods,
    transferItems,
    sessionLoading,
    persistSession,
    fileAssets,
    configScanMode,
    shaderIncludeSettings,
    activeCategory,
    lastTransferResult,
    lastTransferredModNames,
  ]);

  useEffect(() => {
    const unlistenTransfer = listen<TransferProgress>("transfer-progress", (event) => {
      setProgress((prev) => mergeMonotonicProgress(prev, event.payload));
    });
    const unlistenScan = listen<TransferProgress>("scan-progress", (event) => {
      setScanProgress(event.payload);
    });
    const unlistenCheck = listen<TransferProgress>("check-progress", (event) => {
      setCheckProgress((prev) => mergeMonotonicProgress(prev, event.payload));
    });
    const unlistenAssetScan = listen<TransferProgress>("asset-scan-progress", (event) => {
      setAssetScanProgress(event.payload);
    });
    const unlistenAssetTransfer = listen<TransferProgress>("asset-transfer-progress", (event) => {
      setAssetTransferProgress((prev) => mergeMonotonicProgress(prev, event.payload));
    });
    return () => {
      unlistenTransfer.then((fn) => fn());
      unlistenScan.then((fn) => fn());
      unlistenCheck.then((fn) => fn());
      unlistenAssetScan.then((fn) => fn());
      unlistenAssetTransfer.then((fn) => fn());
    };
  }, []);

  const selectModVersion = useCallback((index: number, option: ModVersionOption) => {
    setTransferItems((prev) =>
      prev.map((item, i) =>
        i === index
          ? {
              ...item,
              status: "transferable" as const,
              targetVersion: option.version,
              targetFileName: option.fileName,
              downloadUrl: option.downloadUrl,
              downloadSource: option.source,
              selected: true,
            }
          : item
      )
    );
  }, []);

  const loadModVersions = useCallback(
    async (item: ModTransferItem, target: TargetEnv, sourceModList: IdentifiedMod[]) => {
      return invoke<ModVersionOption[]>("list_mod_version_options", {
        modInfo: item.mod,
        target,
        sourceMods: sourceModList,
      });
    },
    []
  );

  useEffect(() => {
    void loadMigrationPresets();
  }, [loadMigrationPresets]);

  return {
    sourceInstance,
    targetInstance,
    mods,
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
    launcherLoading,
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
    setTransferItems,
    setError,
    activeCategory,
    setActiveCategory,
    fileAssets,
    currentAssetItems,
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
    lastTransferResult,
    lastTransferredModNames,
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
    loadMigrationPresets,
    saveMigrationPreset,
    deleteMigrationPreset,
    applyMigrationPreset,
  };
}

export function useSettings() {
  const [settings, setSettings] = useState<AppSettings>({
    curseforge_api_key: "",
    download_source_priority: ["modrinth", "curseforge", "mcmod", "github"],
    max_concurrent_downloads: 6,
    mod_api_mirror: "auto",
    mod_version_policy: "auto",
    auto_check_online_packs: true,
    backup_before_transfer: true,
    auto_export_mod_report: false,
    mod_report_format: "md",
    update_manifest_url: "",
    update_mode: "manual",
    update_check_interval_hours: 24,
  });
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const s = await invoke<AppSettings>("get_settings");
      setSettings(s);
    } finally {
      setLoading(false);
    }
  }, []);

  const save = useCallback(async (s: AppSettings) => {
    await invoke("save_app_settings", { settings: s });
    setSettings(s);
    window.dispatchEvent(new CustomEvent("settings-reloaded"));
  }, []);

  const clearCache = useCallback(async () => {
    await invoke("clear_cache");
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    const handler = () => {
      void load();
    };
    window.addEventListener("settings-reloaded", handler);
    return () => window.removeEventListener("settings-reloaded", handler);
  }, [load]);

  return { settings, loading, save, clearCache, reload: load };
}
