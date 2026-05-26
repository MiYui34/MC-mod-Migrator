import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import {
  Download,
  ExternalLink,
  Loader2,
  Link2,
  Package,
  RefreshCw,
  RotateCcw,
  Search,
  Sparkles,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { MarketProjectBody } from "@/components/MarketProjectBody";
import { TaskProgressBar } from "@/components/ProgressBar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import type {
  InstanceInfo,
  MarketCategory,
  MarketDepPreviewItem,
  MarketInstallBatchResult,
  MarketInstallJob,
  MarketInstallRecord,
  MarketItemInstallStatus,
  MarketMissingDep,
  MarketMissingDepsScan,
  MarketProjectDetail,
  MarketSearchItem,
  MarketSearchResponse,
  MarketSort,
  MarketSourceFilter,
  MarketUpdatableMod,
  ModVersionOption,
  OpenMarketOptions,
  TransferProgress,
} from "@/types";
import {
  MARKET_CATEGORY_LABELS,
  MARKET_INSTALL_STATUS_LABELS,
  MARKET_SORT_LABELS,
  MARKET_SOURCE_FILTER_LABELS,
  MARKET_SOURCE_LABELS,
  MODPACK_BADGE_LABELS,
} from "@/types";

const MARKET_CATEGORIES: MarketCategory[] = [
  "shader_pack",
  "resource_pack",
  "mod",
  "modpack",
  "datapack",
  "litematic",
];

type MarketViewTab = "discover" | "search" | "updates" | "missing_deps";

interface MarketProps {
  targetInstance: InstanceInfo | null;
  initialOptions?: OpenMarketOptions | null;
  onError: (msg: string | null) => void;
  onSuccess: (msg: string) => void;
  onCancelTask?: () => void;
}

interface QueueItem {
  id: string;
  title: string;
  job: MarketInstallJob;
}

function toTargetEnv(instance: InstanceInfo) {
  return {
    modsPath: instance.modsPath,
    mcVersion: instance.mcVersion,
    loader: instance.loader,
    loaderVersion: instance.loaderVersion ?? "",
  };
}

function formatDownloads(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function marketItemKey(source: MarketSearchItem["source"], id: string): string {
  return `${source}:${id}`;
}

function normalizeMarketTitle(title: string): string {
  return title.toLowerCase().replace(/[^a-z0-9\u4e00-\u9fff]+/g, "");
}

function marketItemsLike(a: MarketSearchItem, b: MarketSearchItem): boolean {
  if (a.id.toLowerCase() === b.id.toLowerCase() && a.source === b.source) return true;
  if (a.slug && b.slug && a.slug.toLowerCase() === b.slug.toLowerCase()) return true;
  return normalizeMarketTitle(a.title) === normalizeMarketTitle(b.title);
}

function dedupeMarketItems(items: MarketSearchItem[], sort: MarketSort): MarketSearchItem[] {
  const out: MarketSearchItem[] = [];
  for (const item of items) {
    const idx = out.findIndex((e) => marketItemsLike(e, item));
    if (idx < 0) {
      out.push(item);
      continue;
    }
    if (sort === "downloads") {
      if (
        item.downloads > out[idx].downloads ||
        (item.downloads === out[idx].downloads &&
          item.source === "curseforge" &&
          out[idx].source !== "curseforge")
      ) {
        out[idx] = item;
      }
    } else if (item.source === "curseforge" && out[idx].source !== "curseforge") {
      out[idx] = item;
    }
  }
  return out;
}

function orderMarketItems(items: MarketSearchItem[], sort: MarketSort): MarketSearchItem[] {
  const deduped = dedupeMarketItems(items, sort);
  if (sort === "downloads") {
    return [...deduped].sort((a, b) => b.downloads - a.downloads);
  }
  return deduped;
}

function versionOptionKey(v: ModVersionOption): string {
  return `${v.source}:${v.version}:${v.fileName}:${v.downloadUrl}`;
}

function versionRowKey(v: ModVersionOption, index: number): string {
  return `${index}:${versionOptionKey(v)}`;
}

const VERSION_LIST_INITIAL_CAP = 80;

type VersionEntry = { version: ModVersionOption; index: number };

function versionMatchesSearch(v: ModVersionOption, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  if (v.version.toLowerCase().includes(q)) return true;
  if (v.fileName.toLowerCase().includes(q)) return true;
  if (v.versionType?.toLowerCase().includes(q)) return true;
  if (v.loaders?.some((loader) => loader.toLowerCase().includes(q))) return true;
  if (v.gameVersions?.some((gv) => gv.toLowerCase().includes(q))) return true;
  return false;
}

function buildVersionEntries(versions: ModVersionOption[]): VersionEntry[] {
  return versions.map((version, index) => ({ version, index }));
}

function installStatusBadgeVariant(
  status: MarketSearchItem["installStatus"]
): "default" | "secondary" | "warning" | "success" {
  if (status === "updatable") return "warning";
  if (status === "installed") return "success";
  return "secondary";
}

function ItemBadges({ item }: { item: MarketSearchItem }) {
  return (
    <div className="flex gap-1 flex-wrap">
      <Badge variant="secondary" className="text-[10px] px-1.5 py-0">
        {MARKET_SOURCE_LABELS[item.source]}
      </Badge>
      {item.modpackBadge && (
        <Badge variant="secondary" className="text-[10px] px-1.5 py-0">
          {MODPACK_BADGE_LABELS[item.modpackBadge] ?? item.modpackBadge}
        </Badge>
      )}
      {item.installStatus && item.installStatus !== "not_installed" && (
        <Badge
          variant={installStatusBadgeVariant(item.installStatus)}
          className="text-[10px] px-1.5 py-0"
        >
          {MARKET_INSTALL_STATUS_LABELS[item.installStatus]}
          {item.installedVersion ? ` · ${item.installedVersion}` : ""}
        </Badge>
      )}
      <span className="text-[10px] text-[var(--color-muted-foreground)]">
        {formatDownloads(item.downloads)} 下载
      </span>
    </div>
  );
}

function MarketItemCard({
  item,
  onClick,
}: {
  item: MarketSearchItem;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="text-left rounded-lg border border-[var(--color-border)] p-3 hover:bg-[var(--color-muted)]/30 transition-colors"
    >
      <div className="flex gap-3">
        {item.iconUrl ? (
          <img
            src={item.iconUrl}
            alt=""
            className="h-10 w-10 rounded shrink-0 object-cover"
            loading="lazy"
          />
        ) : (
          <div className="h-10 w-10 rounded bg-[var(--color-muted)] shrink-0 flex items-center justify-center">
            <Package className="h-5 w-5 opacity-50" />
          </div>
        )}
        <div className="min-w-0 flex-1">
          <div className="font-medium text-sm truncate">{item.title}</div>
          <div className="mt-1">
            <ItemBadges item={item} />
          </div>
        </div>
      </div>
      {item.description && (
        <p className="mt-2 text-xs text-[var(--color-muted-foreground)] line-clamp-2">
          {item.description}
        </p>
      )}
    </button>
  );
}

export function Market({
  targetInstance,
  initialOptions,
  onError,
  onSuccess,
  onCancelTask,
}: MarketProps) {
  const [viewTab, setViewTab] = useState<MarketViewTab>(
    initialOptions?.query ? "search" : "discover"
  );
  const [category, setCategory] = useState<MarketCategory>(
    initialOptions?.category ?? "shader_pack"
  );
  const [query, setQuery] = useState(initialOptions?.query ?? "");
  const [searching, setSearching] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [results, setResults] = useState<MarketSearchItem[]>([]);
  const [page, setPage] = useState(0);
  const [hasMore, setHasMore] = useState(false);

  const [sourceFilter, setSourceFilter] = useState<MarketSourceFilter>("all");
  const [sort, setSort] = useState<MarketSort>("downloads");
  const [relaxFilters, setRelaxFilters] = useState(true);
  const [compatibleOnly, setCompatibleOnly] = useState(false);

  const [detailOpen, setDetailOpen] = useState(false);
  const [selectedItem, setSelectedItem] = useState<MarketSearchItem | null>(null);
  const [projectDetail, setProjectDetail] = useState<MarketProjectDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [versions, setVersions] = useState<ModVersionOption[]>([]);
  const [versionsError, setVersionsError] = useState<string | null>(null);
  const [versionsLoading, setVersionsLoading] = useState(false);
  const [versionsExpanding, setVersionsExpanding] = useState(false);
  const [versionSearchQuery, setVersionSearchQuery] = useState("");
  const [showAllVersions, setShowAllVersions] = useState(false);
  const [selectedVersionKey, setSelectedVersionKey] = useState<string | null>(null);
  const selectedVersion = useMemo(
    () => {
      if (selectedVersionKey == null) return null;
      const idx = Number.parseInt(selectedVersionKey.split(":")[0] ?? "", 10);
      if (!Number.isNaN(idx) && versions[idx]) {
        return versions[idx];
      }
      return versions.find((v) => versionOptionKey(v) === selectedVersionKey) ?? null;
    },
    [versions, selectedVersionKey]
  );
  const versionSearchActive = versionSearchQuery.trim().length > 0;
  const filteredVersionEntries = useMemo(() => {
    const entries = buildVersionEntries(versions);
    if (!versionSearchActive) return entries;
    return entries.filter(({ version }) =>
      versionMatchesSearch(version, versionSearchQuery)
    );
  }, [versions, versionSearchQuery, versionSearchActive]);
  const visibleVersionEntries = useMemo(() => {
    if (versionSearchActive || showAllVersions) return filteredVersionEntries;
    return filteredVersionEntries.slice(0, VERSION_LIST_INITIAL_CAP);
  }, [filteredVersionEntries, versionSearchActive, showAllVersions]);
  const [resolveDeps, setResolveDeps] = useState(true);
  const [depPreview, setDepPreview] = useState<MarketDepPreviewItem[]>([]);
  const [depPreviewLoading, setDepPreviewLoading] = useState(false);

  const [queue, setQueue] = useState<QueueItem[]>([]);
  const [installing, setInstalling] = useState(false);
  const [installProgress, setInstallProgress] = useState<TransferProgress | null>(null);

  const [recentInstalls, setRecentInstalls] = useState<MarketInstallRecord[]>([]);
  const [undoingId, setUndoingId] = useState<string | null>(null);
  const [launcherImportPath, setLauncherImportPath] = useState<string | null>(null);

  const [updatableMods, setUpdatableMods] = useState<MarketUpdatableMod[]>([]);
  const [updatableLoading, setUpdatableLoading] = useState(false);
  const [selectedUpdates, setSelectedUpdates] = useState<Set<string>>(new Set());

  const [missingDeps, setMissingDeps] = useState<MarketMissingDep[]>([]);
  const [missingDepsMeta, setMissingDepsMeta] = useState<{
    scannedMods: number;
    skippedUnidentified: number;
  }>({ scannedMods: 0, skippedUnidentified: 0 });
  const [missingDepsLoading, setMissingDepsLoading] = useState(false);
  const [selectedMissingDeps, setSelectedMissingDeps] = useState<Set<string>>(new Set());

  const requestIdRef = useRef(0);
  const loadMoreInFlightRef = useRef(false);
  const resultsScrollRef = useRef<HTMLDivElement>(null);
  const loadMoreSentinelRef = useRef<HTMLDivElement>(null);
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;

  const mcVersion = targetInstance?.mcVersion ?? "";
  const loader = targetInstance?.loader ?? "";
  const needsTargetEnv = category !== "litematic";

  useEffect(() => {
    if (initialOptions?.category) setCategory(initialOptions.category);
    if (initialOptions?.query) {
      setQuery(initialOptions.query);
      setViewTab("search");
    }
  }, [initialOptions]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<TransferProgress>("market-install-progress", (e) => {
      setInstallProgress(e.payload);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

  const loadRecentInstalls = useCallback(async () => {
    try {
      const list = await invoke<MarketInstallRecord[]>(
        "market_list_recent_installs_cmd",
        { limit: 10 }
      );
      setRecentInstalls(list);
    } catch {
      /* ignore */
    }
  }, []);

  useEffect(() => {
    void loadRecentInstalls();
  }, [loadRecentInstalls]);

  const effectiveSort = viewTab === "discover" && sort === "relevance" ? "downloads" : sort;

  const applyInstallBadges = useCallback(
    async (items: MarketSearchItem[], requestId: number, replace: boolean) => {
      if (!targetInstance || items.length === 0) return;
      try {
        const statuses = await invoke<MarketItemInstallStatus[]>(
          "market_check_installed_cmd",
          {
            category,
            items,
            target: targetInstance,
            quickCheck: true,
          }
        );
        if (requestId !== requestIdRef.current) return;
        const statusMap = new Map(statuses.map((s) => [s.key, s]));
        setResults((prev) => {
          const base = replace ? items : prev;
          return base.map((item) => {
            const hit = statusMap.get(marketItemKey(item.source, item.id));
            if (!hit) return item;
            return {
              ...item,
              installStatus: hit.status,
              installedVersion: hit.installedVersion,
            };
          });
        });
      } catch {
        /* 角标失败不影响列表展示 */
      }
    },
    [category, targetInstance]
  );

  const fetchResults = useCallback(
    async (pageIndex: number, replace: boolean, browseMode: boolean) => {
      const q = browseMode ? "" : query.trim();
      if (!browseMode && !q) {
        onError("请输入搜索关键词");
        return;
      }
      if (needsTargetEnv && !targetInstance) {
        onError("请先选择目标实例，将自动使用其 MC 版本与加载器筛选");
        return;
      }

      if (replace) {
        requestIdRef.current += 1;
        loadMoreInFlightRef.current = false;
        setSearching(true);
        setLoadingMore(false);
      } else {
        if (loadMoreInFlightRef.current) return;
        loadMoreInFlightRef.current = true;
        setLoadingMore(true);
      }

      const requestId = requestIdRef.current;
      onError(null);
      const apiSort =
        browseMode && effectiveSort === "relevance" ? "downloads" : effectiveSort;
      try {
        const resp = await invoke<MarketSearchResponse>("market_search_cmd", {
          category,
          query: q,
          mcVersion,
          loader,
          page: pageIndex,
          sourceFilter,
          sort: apiSort,
          relaxFilters,
          compatibleOnly: category === "mod" ? compatibleOnly : false,
        });
        if (requestId !== requestIdRef.current) return;
        setResults((prev) =>
          orderMarketItems(
            replace ? resp.items : [...prev, ...resp.items],
            apiSort
          )
        );
        setPage(pageIndex);
        setHasMore(resp.hasMore);
        window.setTimeout(() => {
          if (requestId === requestIdRef.current) {
            void applyInstallBadges(resp.items, requestId, replace);
          }
        }, 0);
      } catch (e) {
        if (requestId !== requestIdRef.current) return;
        onError(String(e));
        if (replace)         setResults([]);
        setHasMore(false);
      } finally {
        if (requestId === requestIdRef.current) {
          setSearching(false);
          setLoadingMore(false);
          loadMoreInFlightRef.current = false;
        }
      }
    },
    [
      category,
      query,
      mcVersion,
      loader,
      sourceFilter,
      sort,
      effectiveSort,
      relaxFilters,
      compatibleOnly,
      needsTargetEnv,
      targetInstance,
      applyInstallBadges,
      onError,
    ]
  );

  const loadUpdatableMods = useCallback(async () => {
    if (!targetInstance) {
      setUpdatableMods([]);
      return;
    }
    setUpdatableLoading(true);
    onError(null);
    try {
      const list = await invoke<MarketUpdatableMod[]>(
        "market_list_updatable_mods_cmd",
        { target: targetInstance }
      );
      setUpdatableMods(list);
      setSelectedUpdates(new Set(list.map((m) => m.projectId)));
    } catch (e) {
      onError(String(e));
      setUpdatableMods([]);
    } finally {
      setUpdatableLoading(false);
    }
  }, [targetInstance, onError]);

  const loadMissingDeps = useCallback(async () => {
    if (!targetInstance) {
      setMissingDeps([]);
      setMissingDepsMeta({ scannedMods: 0, skippedUnidentified: 0 });
      return;
    }
    setMissingDepsLoading(true);
    onError(null);
    try {
      const scan = await invoke<MarketMissingDepsScan>("market_list_missing_deps_cmd", {
        target: targetInstance,
      });
      setMissingDeps(scan.items);
      setMissingDepsMeta({
        scannedMods: scan.scannedMods,
        skippedUnidentified: scan.skippedUnidentified,
      });
      setSelectedMissingDeps(new Set(scan.items.map((m) => m.projectId)));
    } catch (e) {
      onError(String(e));
      setMissingDeps([]);
      setMissingDepsMeta({ scannedMods: 0, skippedUnidentified: 0 });
    } finally {
      setMissingDepsLoading(false);
    }
  }, [targetInstance, onError]);

  useEffect(() => {
    if (viewTab === "discover") {
      if (needsTargetEnv && !targetInstance) {
        setResults([]);
        setPage(0);
        setHasMore(false);
        return;
      }
      setResults([]);
      setPage(0);
      setHasMore(false);
      void fetchResults(0, true, true);
    }
  }, [viewTab, category, sourceFilter, effectiveSort, relaxFilters, compatibleOnly, targetInstance?.name, needsTargetEnv, fetchResults]);

  useEffect(() => {
    if (category !== "mod" && (viewTab === "updates" || viewTab === "missing_deps")) {
      setViewTab("discover");
    }
  }, [category, viewTab]);

  useEffect(() => {
    if (viewTab !== "discover" && viewTab !== "search") return;
    if (results.length === 0 || !hasMore) return;

    const root = resultsScrollRef.current;
    const sentinel = loadMoreSentinelRef.current;
    if (!root || !sentinel) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries[0]?.isIntersecting) return;
        if (searching || loadingMore || loadMoreInFlightRef.current || !hasMore) return;
        void fetchResults(page + 1, false, viewTab === "discover");
      },
      { root, rootMargin: "320px", threshold: 0 }
    );

    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [
    viewTab,
    searching,
    loadingMore,
    results.length,
    hasMore,
    page,
    fetchResults,
  ]);

  useEffect(() => {
    if (viewTab === "updates") {
      void loadUpdatableMods();
    }
  }, [viewTab, loadUpdatableMods]);

  useEffect(() => {
    if (viewTab === "missing_deps") {
      void loadMissingDeps();
    }
  }, [viewTab, loadMissingDeps]);

  const handleSearch = () => {
    setViewTab("search");
    void fetchResults(0, true, false);
  };

  const applyVersionList = useCallback((list: ModVersionOption[]) => {
    setVersions(list);
    const pickIdx = list.findIndex((v) => v.recommended);
    const idx = pickIdx >= 0 ? pickIdx : 0;
    setShowAllVersions(
      list.length <= VERSION_LIST_INITIAL_CAP || idx >= VERSION_LIST_INITIAL_CAP
    );
    setSelectedVersionKey((prev) => {
      if (prev) {
        const prevIdx = Number.parseInt(prev.split(":")[0] ?? "", 10);
        if (!Number.isNaN(prevIdx) && list[prevIdx]) {
          return versionRowKey(list[prevIdx], prevIdx);
        }
      }
      const pick = list[idx];
      return pick ? versionRowKey(pick, idx) : null;
    });
  }, []);

  const loadDetailVersions = useCallback(
    async (item: MarketSearchItem, expand = false) => {
      if (category !== "litematic" && !targetInstance) {
        setVersions([]);
        setSelectedVersionKey(null);
        return;
      }
      if (expand) {
        setVersionsExpanding(true);
      } else {
        setVersionsLoading(true);
        setVersionsError(null);
      }
      try {
        const list = await invoke<ModVersionOption[]>("market_list_versions_cmd", {
          category,
          source: item.source,
          projectId: item.id,
          expand,
          target: targetInstance
            ? toTargetEnv(targetInstance)
            : {
                modsPath: "",
                mcVersion: "",
                loader: "",
                loaderVersion: "",
              },
        });
        applyVersionList(list);
        if (expand) {
          setShowAllVersions(true);
        }
      } catch (e) {
        const msg = String(e);
        setVersionsError(msg);
        onErrorRef.current(msg);
        if (!expand) {
          setVersions([]);
          setSelectedVersionKey(null);
        }
      } finally {
        if (expand) {
          setVersionsExpanding(false);
        } else {
          setVersionsLoading(false);
        }
      }
    },
    [category, targetInstance, applyVersionList]
  );

  const openDetail = async (item: MarketSearchItem) => {
    setSelectedItem(item);
    setDetailOpen(true);
    setProjectDetail(null);
    setVersions([]);
    setVersionsError(null);
    setVersionSearchQuery("");
    setShowAllVersions(false);
    setSelectedVersionKey(null);
    setDepPreview([]);
    setDetailLoading(true);
    onError(null);
    try {
      const detail = await invoke<MarketProjectDetail>("market_get_project_detail_cmd", {
        category,
        source: item.source,
        projectId: item.id,
      });
      setProjectDetail(detail);
    } catch (e) {
      onError(String(e));
    } finally {
      setDetailLoading(false);
    }
  };

  useEffect(() => {
    if (!detailOpen || !selectedItem) return;
    if (category !== "litematic" && !targetInstance) {
      setVersions([]);
      setSelectedVersionKey(null);
      return;
    }

    let cancelled = false;
    void loadDetailVersions(selectedItem, false).then(() => {
      if (cancelled) return;
    });
    return () => {
      cancelled = true;
    };
  }, [
    detailOpen,
    selectedItem?.id,
    selectedItem?.source,
    category,
    targetInstance?.name,
    targetInstance?.modsPath,
    targetInstance?.mcVersion,
    targetInstance?.loader,
    loadDetailVersions,
  ]);

  useEffect(() => {
    if (
      !selectedVersionKey ||
      !selectedItem ||
      category !== "mod" ||
      !targetInstance ||
      !resolveDeps
    ) {
      setDepPreview([]);
      return;
    }
    const version = selectedVersion;
    if (!version?.downloadUrl || !version.fileName) {
      setDepPreview([]);
      return;
    }
    let cancelled = false;
    setDepPreviewLoading(true);
    const timer = window.setTimeout(() => {
      void invoke<MarketDepPreviewItem[]>("market_preview_deps_cmd", {
        downloadUrl: version.downloadUrl,
        fileName: version.fileName,
        projectId: selectedItem.id,
        source: selectedItem.source,
        modName: selectedItem.title,
        target: targetInstance,
      })
        .then((items) => {
          if (!cancelled) setDepPreview(items);
        })
        .catch(() => {
          if (!cancelled) setDepPreview([]);
        })
        .finally(() => {
          if (!cancelled) setDepPreviewLoading(false);
        });
    }, 300);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [selectedVersionKey, selectedVersion, selectedItem, category, targetInstance, resolveDeps]);

  const addToQueue = () => {
    if (!selectedVersion || !selectedItem) return;
    const job: MarketInstallJob = {
      category,
      downloadUrl: selectedVersion.downloadUrl,
      fileName: selectedVersion.fileName,
      projectId: selectedItem.id,
      source: selectedItem.source,
      modName: selectedItem.title,
    };
    setQueue((prev) => [
      ...prev,
      {
        id: `${selectedItem.id}-${selectedVersion.fileName}-${Date.now()}`,
        title: `${selectedItem.title} · ${selectedVersion.version}`,
        job,
      },
    ]);
    setDetailOpen(false);
    onSuccess(`已加入队列：${selectedItem.title}`);
  };

  const removeFromQueue = (id: string) => {
    setQueue((prev) => prev.filter((q) => q.id !== id));
  };

  const runInstall = async (jobs: MarketInstallJob[], withDeps: boolean) => {
    if (!targetInstance || jobs.length === 0) return;
    setInstalling(true);
    setInstallProgress(null);
    setLauncherImportPath(null);
    onError(null);
    const shouldResolveDeps = withDeps && jobs.some((j) => j.category === "mod");
    try {
      const batch = await invoke<MarketInstallBatchResult>("market_install_batch_cmd", {
        jobs: jobs.map((j) => ({
          category: j.category,
          downloadUrl: j.downloadUrl,
          fileName: j.fileName,
          isDependency: j.isDependency ?? false,
          projectId: j.projectId ?? null,
          source: j.source ?? null,
          modName: j.modName ?? null,
        })),
        target: targetInstance,
        resolveDeps: shouldResolveDeps,
      });

      const launcher = batch.results.find((r) => r.needsLauncherImport);
      if (launcher?.filePath) {
        setLauncherImportPath(launcher.filePath);
        onSuccess(`${launcher.hint ?? "整合包已下载"}\n${launcher.filePath}`);
      } else {
        const names = batch.results.map((r) => r.fileName).join(", ");
        onSuccess(`已安装：${names}`);
      }

      if (batch.warnings?.length) {
        onError(batch.warnings.join("\n"));
      }

      setQueue([]);
      setDetailOpen(false);
      void loadRecentInstalls();
      if (viewTab === "discover") {
        void fetchResults(page, true, true);
      } else if (viewTab === "search" && query.trim()) {
        void fetchResults(page, true, false);
      }
      if (viewTab === "updates") {
        void loadUpdatableMods();
      }
      if (viewTab === "missing_deps") {
        void loadMissingDeps();
      }
    } catch (e) {
      onError(String(e));
    } finally {
      setInstalling(false);
      setInstallProgress(null);
    }
  };

  const handleInstallNow = () => {
    if (!selectedVersion || !selectedItem) return;
    const job: MarketInstallJob = {
      category,
      downloadUrl: selectedVersion.downloadUrl,
      fileName: selectedVersion.fileName,
      projectId: selectedItem.id,
      source: selectedItem.source,
      modName: selectedItem.title,
    };
    void runInstall([job], category === "mod" && resolveDeps);
  };

  const handleInstallQueue = () => {
    void runInstall(queue.map((q) => q.job), resolveDeps);
  };

  const handleBatchUpdate = () => {
    const jobs: MarketInstallJob[] = updatableMods
      .filter((m) => selectedUpdates.has(m.projectId))
      .map((m) => ({
        category: "mod" as MarketCategory,
        downloadUrl: m.downloadUrl,
        fileName: m.latestFileName,
        projectId: m.projectId,
        source: m.source,
        modName: m.title,
      }));
    void runInstall(jobs, true);
  };

  const handleBatchInstallMissing = () => {
    const jobs: MarketInstallJob[] = missingDeps
      .filter((m) => selectedMissingDeps.has(m.projectId))
      .map((m) => ({
        category: "mod" as MarketCategory,
        downloadUrl: m.downloadUrl,
        fileName: m.fileName,
        projectId: m.projectId,
        source: m.source,
        modName: m.title,
        isDependency: true,
      }));
    void runInstall(jobs, true);
  };

  const handleUndo = async (recordId: string) => {
    if (!confirm("确定撤销此次安装？新建文件将被删除，被覆盖的文件将从备份恢复。")) return;
    setUndoingId(recordId);
    onError(null);
    try {
      const result = await invoke<{ restored: number; removed: number; failed: number }>(
        "market_undo_install_cmd",
        { recordId }
      );
      onSuccess(
        `撤销完成：恢复 ${result.restored} 个文件，删除 ${result.removed} 个新建文件`
      );
      void loadRecentInstalls();
    } catch (e) {
      onError(String(e));
    } finally {
      setUndoingId(null);
    }
  };

  const filterBar =
    category === "litematic" ? (
      <div className="flex flex-wrap gap-2 items-center text-xs">
        <select
          value={sort}
          onChange={(e) => setSort(e.target.value as MarketSort)}
          className="rounded border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1"
        >
          {(Object.keys(MARKET_SORT_LABELS) as MarketSort[]).map((k) => (
            <option key={k} value={k}>
              {MARKET_SORT_LABELS[k]}
            </option>
          ))}
        </select>
        <span className="text-[var(--color-muted-foreground)]">
          数据来源：SGU 投影站（litematic.sgu-server.xin）
        </span>
      </div>
    ) : (
    <div className="flex flex-wrap gap-2 items-center text-xs">
      <select
        value={sourceFilter}
        onChange={(e) => setSourceFilter(e.target.value as MarketSourceFilter)}
        className="rounded border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1"
      >
        {(Object.keys(MARKET_SOURCE_FILTER_LABELS) as MarketSourceFilter[]).map((k) => (
          <option key={k} value={k}>
            {MARKET_SOURCE_FILTER_LABELS[k]}
          </option>
        ))}
      </select>
      <select
        value={sort}
        onChange={(e) => setSort(e.target.value as MarketSort)}
        className="rounded border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1"
      >
        {(Object.keys(MARKET_SORT_LABELS) as MarketSort[]).map((k) => (
          <option key={k} value={k}>
            {MARKET_SORT_LABELS[k]}
          </option>
        ))}
      </select>
      <label className="flex items-center gap-1 text-[var(--color-muted-foreground)]">
        <Checkbox checked={relaxFilters} onCheckedChange={(v) => setRelaxFilters(Boolean(v))} />
        无结果时放宽筛选
      </label>
      {category === "mod" && targetInstance && (
        <Button
          type="button"
          size="sm"
          variant={compatibleOnly ? "default" : "outline"}
          className="h-7 text-xs"
          onClick={() => setCompatibleOnly((v) => !v)}
          title={`仅显示兼容 MC ${mcVersion} · ${loader} 的 Mod`}
        >
          目标端可用
        </Button>
      )}
    </div>
    );

  return (
    <div className="flex flex-col h-full min-h-0">
      <div className="flex flex-nowrap gap-1 px-4 py-2 border-b border-[var(--color-border)] bg-[var(--color-muted)]/20 overflow-x-auto shrink-0">
        {MARKET_CATEGORIES.map((cat) => (
          <button
            key={cat}
            type="button"
            onClick={() => {
              requestIdRef.current += 1;
              loadMoreInFlightRef.current = false;
              setSearching(false);
              setLoadingMore(false);
              setDetailOpen(false);
              setCategory(cat);
              setResults([]);
              setPage(0);
              setHasMore(false);
              if (cat !== "mod") {
                setCompatibleOnly(false);
                if (viewTab === "updates" || viewTab === "missing_deps") {
                  setViewTab("discover");
                }
              }
            }}
            className={cn(
              "px-3 py-1.5 text-xs rounded-md transition-colors whitespace-nowrap",
              category === cat
                ? "bg-[var(--color-primary)] text-[var(--color-primary-foreground)]"
                : "hover:bg-[var(--color-muted)] text-[var(--color-muted-foreground)]"
            )}
          >
            {MARKET_CATEGORY_LABELS[cat]}
          </button>
        ))}
      </div>

      <div className="flex gap-1 px-4 pt-2 shrink-0">
        {(
          [
            { id: "discover" as const, label: "发现", icon: Sparkles },
            { id: "search" as const, label: "搜索", icon: Search },
            ...(category === "mod"
              ? ([
                  { id: "updates" as const, label: "Mod 更新", icon: RefreshCw },
                  { id: "missing_deps" as const, label: "缺失依赖", icon: Link2 },
                ] as const)
              : []),
          ] as const
        ).map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            onClick={() => setViewTab(id)}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md transition-colors",
              viewTab === id
                ? "bg-[var(--color-muted)] text-[var(--color-foreground)] font-medium"
                : "text-[var(--color-muted-foreground)] hover:bg-[var(--color-muted)]/50"
            )}
          >
            <Icon className="h-3.5 w-3.5" />
            {label}
          </button>
        ))}
      </div>

      <div className="px-4 py-3 border-b border-[var(--color-border)] shrink-0 space-y-2">
        {viewTab === "search" && (
          <div className="flex gap-2">
            <input
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleSearch()}
              placeholder={`搜索${MARKET_CATEGORY_LABELS[category]}…`}
              className="flex-1 min-w-0 rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-3 py-2 text-sm"
            />
            <Button onClick={handleSearch} disabled={searching}>
              {searching ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Search className="h-4 w-4" />
              )}
              搜索
            </Button>
          </div>
        )}

        {viewTab === "discover" && (
          <p className="text-xs text-[var(--color-muted-foreground)]">
            {category === "litematic"
              ? "浏览 SGU 投影站公开投影，无需输入关键词"
              : `浏览热门与最近更新的 ${MARKET_CATEGORY_LABELS[category]}，无需输入关键词`}
          </p>
        )}

        {viewTab === "updates" && (
          <p className="text-xs text-[var(--color-muted-foreground)]">
            列出目标实例中已识别且存在新版本的 Mod（需曾在迁移页扫描过 Mod 以建立索引）
          </p>
        )}

        {viewTab === "missing_deps" && (
          <p className="text-xs text-[var(--color-muted-foreground)]">
            扫描已装 Mod 的前置依赖，列出目标实例中缺失且可在线安装的库（需先在迁移页扫描 Mod）
          </p>
        )}

        {(viewTab === "discover" || viewTab === "search") && filterBar}

        {targetInstance ? (
          <p className="text-xs text-[var(--color-muted-foreground)]">
            目标：{targetInstance.name}
            {needsTargetEnv && (
              <> · 自动匹配 MC {mcVersion} · {loader}</>
            )}
          </p>
        ) : (
          <p className="text-xs text-amber-500">
            {needsTargetEnv
              ? "请在左侧选择目标实例，搜索与版本将自动使用该实例的 MC 版本与加载器"
              : "请在左侧选择目标实例后再安装"}
          </p>
        )}

        {queue.length > 0 && (
          <div className="rounded-md border border-[var(--color-border)] p-2 space-y-1">
            <div className="flex items-center justify-between text-xs font-medium">
              <span>安装队列 ({queue.length})</span>
              <Button
                size="sm"
                disabled={installing || !targetInstance}
                onClick={() => handleInstallQueue()}
              >
                安装全部
              </Button>
            </div>
            {queue.map((q) => (
              <div key={q.id} className="flex items-center justify-between text-xs gap-2">
                <span className="truncate">{q.title}</span>
                <button
                  type="button"
                  onClick={() => removeFromQueue(q.id)}
                  className="text-[var(--color-muted-foreground)] hover:text-red-400 shrink-0"
                >
                  <Trash2 className="h-3 w-3" />
                </button>
              </div>
            ))}
          </div>
        )}

        {launcherImportPath && (
          <div className="flex items-center gap-2 text-xs">
            <span className="text-[var(--color-muted-foreground)] truncate">
              整合包已保存，请在启动器中导入
            </span>
            <Button
              variant="secondary"
              size="sm"
              className="h-7 shrink-0"
              onClick={() => void openDownloadFolder(launcherImportPath)}
            >
              打开文件夹
            </Button>
          </div>
        )}

        {recentInstalls.length > 0 && (
          <div className="rounded-md border border-[var(--color-border)] p-2 space-y-1">
            <div className="text-xs font-medium">最近安装</div>
            {recentInstalls.slice(0, 5).map((rec) => (
              <div key={rec.id} className="flex items-center justify-between text-xs gap-2">
                <span className="truncate text-[var(--color-muted-foreground)]">
                  {rec.summary} → {rec.targetName}
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 px-2 shrink-0"
                  disabled={undoingId === rec.id}
                  onClick={() => void handleUndo(rec.id)}
                >
                  {undoingId === rec.id ? (
                    <Loader2 className="h-3 w-3 animate-spin" />
                  ) : (
                    <RotateCcw className="h-3 w-3" />
                  )}
                  撤销
                </Button>
              </div>
            ))}
          </div>
        )}
      </div>

      <div ref={resultsScrollRef} className="flex-1 min-h-0 overflow-y-auto p-4">
        {viewTab === "updates" ? (
          <>
            {!targetInstance && (
              <div className="flex flex-col items-center justify-center h-full text-[var(--color-muted-foreground)] gap-2 py-12">
                <RefreshCw className="h-10 w-10 opacity-40" />
                <p className="text-sm">请先选择目标实例</p>
              </div>
            )}
            {targetInstance && updatableLoading && (
              <div className="flex items-center justify-center py-12 gap-2 text-sm text-[var(--color-muted-foreground)]">
                <Loader2 className="h-4 w-4 animate-spin" />
                扫描可更新 Mod…
              </div>
            )}
            {targetInstance && !updatableLoading && updatableMods.length === 0 && (
              <div className="flex flex-col items-center justify-center h-full text-[var(--color-muted-foreground)] gap-2 py-12">
                <RefreshCw className="h-10 w-10 opacity-40" />
                <p className="text-sm">未发现可更新的 Mod</p>
                <p className="text-xs">请先在迁移页扫描 Mod 以识别项目来源</p>
              </div>
            )}
            {targetInstance && updatableMods.length > 0 && (
              <div className="space-y-3">
                <div className="flex items-center justify-between gap-2">
                  <label className="flex items-center gap-2 text-sm">
                    <Checkbox
                      checked={selectedUpdates.size === updatableMods.length}
                      onCheckedChange={(v) => {
                        if (v) {
                          setSelectedUpdates(new Set(updatableMods.map((m) => m.projectId)));
                        } else {
                          setSelectedUpdates(new Set());
                        }
                      }}
                    />
                    全选 ({selectedUpdates.size}/{updatableMods.length})
                  </label>
                  <div className="flex gap-2">
                    <Button
                      variant="secondary"
                      size="sm"
                      disabled={updatableLoading}
                      onClick={() => void loadUpdatableMods()}
                    >
                      <RefreshCw className="h-3.5 w-3.5" />
                      刷新
                    </Button>
                    <Button
                      size="sm"
                      disabled={installing || selectedUpdates.size === 0}
                      onClick={() => void handleBatchUpdate()}
                    >
                      更新所选
                    </Button>
                  </div>
                </div>
                <div className="space-y-2">
                  {updatableMods.map((mod) => (
                    <div
                      key={mod.projectId}
                      className="flex items-center gap-3 rounded-lg border border-[var(--color-border)] p-3"
                    >
                      <Checkbox
                        checked={selectedUpdates.has(mod.projectId)}
                        onCheckedChange={(v) => {
                          setSelectedUpdates((prev) => {
                            const next = new Set(prev);
                            if (v) next.add(mod.projectId);
                            else next.delete(mod.projectId);
                            return next;
                          });
                        }}
                      />
                      <div className="min-w-0 flex-1">
                        <div className="font-medium text-sm truncate">{mod.title}</div>
                        <div className="text-xs text-[var(--color-muted-foreground)] mt-0.5">
                          {mod.installedFile} · {mod.installedVersion} → {mod.latestVersion}
                        </div>
                      </div>
                      <Badge variant="secondary" className="text-[10px] shrink-0">
                        {MARKET_SOURCE_LABELS[mod.source]}
                      </Badge>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </>
        ) : viewTab === "missing_deps" ? (
          <>
            {!targetInstance && (
              <div className="flex flex-col items-center justify-center h-full text-[var(--color-muted-foreground)] gap-2 py-12">
                <Link2 className="h-10 w-10 opacity-40" />
                <p className="text-sm">请先选择目标实例</p>
              </div>
            )}
            {targetInstance && missingDepsLoading && (
              <div className="flex items-center justify-center py-12 gap-2 text-sm text-[var(--color-muted-foreground)]">
                <Loader2 className="h-4 w-4 animate-spin" />
                扫描缺失依赖…
              </div>
            )}
            {targetInstance && !missingDepsLoading && missingDeps.length === 0 && (
              <div className="flex flex-col items-center justify-center h-full text-[var(--color-muted-foreground)] gap-2 py-12">
                <Link2 className="h-10 w-10 opacity-40" />
                <p className="text-sm">未发现缺失的前置依赖</p>
                <p className="text-xs text-center max-w-sm">
                  已扫描 {missingDepsMeta.scannedMods} 个已识别 Mod
                  {missingDepsMeta.skippedUnidentified > 0
                    ? `，${missingDepsMeta.skippedUnidentified} 个 jar 未在缓存中（请先在迁移页扫描）`
                    : ""}
                </p>
              </div>
            )}
            {targetInstance && missingDeps.length > 0 && (
              <div className="space-y-3">
                <div className="flex items-center justify-between gap-2 flex-wrap">
                  <label className="flex items-center gap-2 text-sm">
                    <Checkbox
                      checked={selectedMissingDeps.size === missingDeps.length}
                      onCheckedChange={(v) => {
                        if (v) {
                          setSelectedMissingDeps(new Set(missingDeps.map((m) => m.projectId)));
                        } else {
                          setSelectedMissingDeps(new Set());
                        }
                      }}
                    />
                    全选 ({selectedMissingDeps.size}/{missingDeps.length})
                  </label>
                  <div className="flex gap-2 items-center">
                    <span className="text-xs text-[var(--color-muted-foreground)]">
                      已扫描 {missingDepsMeta.scannedMods} 个 Mod
                    </span>
                    <Button
                      variant="secondary"
                      size="sm"
                      disabled={missingDepsLoading}
                      onClick={() => void loadMissingDeps()}
                    >
                      <RefreshCw className="h-3.5 w-3.5" />
                      刷新
                    </Button>
                    <Button
                      size="sm"
                      disabled={installing || selectedMissingDeps.size === 0}
                      onClick={() => void handleBatchInstallMissing()}
                    >
                      安装所选
                    </Button>
                  </div>
                </div>
                <div className="space-y-2">
                  {missingDeps.map((dep) => (
                    <div
                      key={dep.projectId}
                      className="flex items-start gap-3 rounded-lg border border-[var(--color-border)] p-3"
                    >
                      <Checkbox
                        className="mt-0.5"
                        checked={selectedMissingDeps.has(dep.projectId)}
                        onCheckedChange={(v) => {
                          setSelectedMissingDeps((prev) => {
                            const next = new Set(prev);
                            if (v) next.add(dep.projectId);
                            else next.delete(dep.projectId);
                            return next;
                          });
                        }}
                      />
                      <div className="min-w-0 flex-1">
                        <div className="font-medium text-sm truncate">{dep.title}</div>
                        <div className="text-xs text-[var(--color-muted-foreground)] mt-0.5 truncate">
                          {dep.fileName} · {dep.version}
                        </div>
                        {dep.requiredBy.length > 0 && (
                          <div className="text-xs text-[var(--color-muted-foreground)] mt-1">
                            被需要于：{dep.requiredBy.join("、")}
                          </div>
                        )}
                      </div>
                      <Badge variant="secondary" className="text-[10px] shrink-0">
                        {MARKET_SOURCE_LABELS[dep.source]}
                      </Badge>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </>
        ) : (
          <>
            {results.length === 0 && !searching && (
              <div className="flex flex-col items-center justify-center h-full text-[var(--color-muted-foreground)] gap-2 py-12">
                <Package className="h-10 w-10 opacity-40" />
                <p className="text-sm">
                  {viewTab === "discover"
                    ? "暂无浏览结果，请调整筛选条件"
                    : category === "litematic"
                      ? "输入关键词搜索 SGU 投影站"
                      : "输入关键词搜索 Modrinth / CurseForge 资源"}
                </p>
              </div>
            )}

            {searching && results.length === 0 && (
              <div className="flex items-center justify-center py-12 gap-2 text-sm text-[var(--color-muted-foreground)]">
                <Loader2 className="h-4 w-4 animate-spin" />
                加载中…
              </div>
            )}

            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
              {results.map((item) => (
                <MarketItemCard
                  key={`${item.source}-${item.id}`}
                  item={item}
                  onClick={() => void openDetail(item)}
                />
              ))}
            </div>

            {loadingMore && (
              <div className="flex items-center justify-center py-4 gap-2 text-sm text-[var(--color-muted-foreground)]">
                <Loader2 className="h-4 w-4 animate-spin" />
                加载更多…
              </div>
            )}

            {results.length > 0 && hasMore && (
              <div ref={loadMoreSentinelRef} className="h-8 shrink-0" aria-hidden />
            )}
          </>
        )}
      </div>

      <TaskProgressBar
        progress={installProgress}
        active={installing}
        idleMessage="市场安装中…"
        onCancel={onCancelTask}
      />

      <Dialog open={detailOpen} onOpenChange={setDetailOpen}>
        <DialogContent className="flex h-[min(90dvh,calc(100vh-2rem))] w-[min(calc(100vw-2rem),48rem)] max-w-3xl flex-col gap-0 overflow-hidden p-0">
          <DialogBody className="flex-1">
            <div className="border-b border-[var(--color-border)] p-6 pb-4 pr-14">
              <DialogHeader className="pr-0">
                <DialogTitle className="flex items-start gap-3">
                  {(projectDetail?.iconUrl ?? selectedItem?.iconUrl) && (
                    <img
                      src={projectDetail?.iconUrl ?? selectedItem?.iconUrl}
                      alt=""
                      className="h-12 w-12 rounded shrink-0 object-cover"
                    />
                  )}
                  <div className="min-w-0 flex-1">
                    <span className="block truncate">
                      {projectDetail?.title ?? selectedItem?.title ?? "资源详情"}
                    </span>
                    {selectedItem && (
                      <div className="mt-2">
                        <ItemBadges item={selectedItem} />
                      </div>
                    )}
                  </div>
                </DialogTitle>
              </DialogHeader>

              {detailLoading ? (
                <div className="flex items-center gap-2 py-4 text-sm text-[var(--color-muted-foreground)]">
                  <Loader2 className="h-4 w-4 animate-spin" />
                  加载详情…
                </div>
              ) : (
                <>
                  {(projectDetail?.description || selectedItem?.description) && (
                    <p className="mt-4 text-sm text-[var(--color-foreground)]">
                      {projectDetail?.description ?? selectedItem?.description}
                    </p>
                  )}

                  {projectDetail?.body ? (
                    <MarketProjectBody body={projectDetail.body} />
                  ) : null}

                  <div className="mt-3 flex flex-wrap gap-2 items-center">
                    {Array.isArray(projectDetail?.categories) &&
                      projectDetail.categories.map((cat) => (
                      <Badge key={cat} variant="secondary" className="text-[10px]">
                        {cat}
                      </Badge>
                    ))}
                    {projectDetail?.license && (
                      <span className="text-xs text-[var(--color-muted-foreground)]">
                        许可证：{projectDetail.license}
                      </span>
                    )}
                    {projectDetail?.projectUrl && (
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-7 text-xs"
                        onClick={() => void openUrl(projectDetail.projectUrl)}
                      >
                        <ExternalLink className="h-3 w-3" />
                        在网站上查看
                      </Button>
                    )}
                  </div>
                </>
              )}
            </div>

            <div className="space-y-3 p-6">
              <h3 className="text-sm font-medium">
                {category === "litematic" ? "下载文件" : "选择版本"}
              </h3>
              {versionsLoading ? (
                <div className="flex items-center justify-center py-6 gap-2 text-sm text-[var(--color-muted-foreground)]">
                  <Loader2 className="h-4 w-4 animate-spin" />
                  加载版本列表…
                </div>
              ) : versions.length === 0 ? (
                <p className="text-sm text-[var(--color-muted-foreground)] py-2">
                  {versionsError
                    ? versionsError
                    : !targetInstance && category !== "litematic"
                      ? "请先选择目标实例，将自动匹配其 MC 版本与加载器"
                      : category === "litematic"
                        ? "无法获取投影下载信息"
                        : "未找到可用版本，可在设置中切换 Mod API 镜像后重试"}
                </p>
              ) : category === "litematic" && versions.length === 1 ? (
                <div className="rounded-md border border-[var(--color-border)] px-3 py-2 text-sm">
                  <div className="font-medium truncate">{versions[0].fileName}</div>
                  <div className="text-xs text-[var(--color-muted-foreground)] mt-1">
                    将安装到实例 schematics 目录
                  </div>
                </div>
              ) : (
                <>
                  <div className="flex flex-wrap items-center gap-2">
                    <div className="relative flex-1 min-w-[12rem]">
                      <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[var(--color-muted-foreground)]" />
                      <Input
                        value={versionSearchQuery}
                        onChange={(e) => setVersionSearchQuery(e.target.value)}
                        placeholder="搜索版本号、MC 版本、加载器…"
                        className="h-8 pl-8 text-xs"
                      />
                    </div>
                    {versionSearchActive && (
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="h-8 text-xs"
                        onClick={() => setVersionSearchQuery("")}
                      >
                        清除
                      </Button>
                    )}
                    <span className="text-xs text-[var(--color-muted-foreground)]">
                      {versionSearchActive
                        ? `${filteredVersionEntries.length} / ${versions.length} 个版本`
                        : `${versions.length} 个版本`}
                    </span>
                  </div>

                  {filteredVersionEntries.length === 0 ? (
                    <p className="text-sm text-[var(--color-muted-foreground)] py-2">
                      无匹配版本，请尝试其他关键词（如 1.20.1、forge、neoforge）
                    </p>
                  ) : (
                  <div
                    role="radiogroup"
                    aria-label="选择版本"
                    className="max-h-48 overflow-y-auto overscroll-contain space-y-2 pr-1"
                  >
                    {visibleVersionEntries.map(({ version: v, index }) => {
                      const rowKey = versionRowKey(v, index);
                      const checked = selectedVersionKey === rowKey;
                      return (
                        <button
                          key={rowKey}
                          type="button"
                          role="radio"
                          aria-checked={checked}
                          className={cn(
                            "flex w-full cursor-pointer text-left rounded-md border px-3 py-2 text-sm transition-colors",
                            checked
                              ? "border-[var(--color-primary)] bg-[var(--color-primary)]/10"
                              : "border-[var(--color-border)] hover:bg-[var(--color-muted)]/30"
                          )}
                          onClick={() => setSelectedVersionKey(rowKey)}
                        >
                          <div className="min-w-0 flex-1">
                            <div className="flex items-center justify-between gap-2">
                              <span className="font-medium">{v.version || "未知版本"}</span>
                              <div className="flex gap-1 shrink-0">
                                {selectedItem?.installedVersion === v.version && (
                                  <Badge variant="secondary" className="text-[10px]">
                                    当前已装
                                  </Badge>
                                )}
                                {v.recommended && (
                                  <Badge className="text-[10px]">推荐</Badge>
                                )}
                              </div>
                            </div>
                            <div className="text-xs text-[var(--color-muted-foreground)] mt-1 truncate">
                              {v.fileName || "未知文件"}
                            </div>
                            {v.gameVersions?.length > 0 && (
                              <div className="text-[10px] text-[var(--color-muted-foreground)] mt-1 truncate">
                                {v.gameVersions.slice(0, 4).join(", ")}
                                {v.gameVersions.length > 4 ? "…" : ""}
                              </div>
                            )}
                          </div>
                        </button>
                      );
                    })}
                  </div>
                  )}
                  {!versionSearchActive &&
                    !showAllVersions &&
                    filteredVersionEntries.length > VERSION_LIST_INITIAL_CAP && (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-7 text-xs"
                      disabled={versionsExpanding}
                      onClick={() => {
                        if (selectedItem) {
                          void loadDetailVersions(selectedItem, true);
                        } else {
                          setShowAllVersions(true);
                        }
                      }}
                    >
                      {versionsExpanding ? (
                        <>
                          <Loader2 className="h-3 w-3 animate-spin mr-1 inline" />
                          加载全部版本…
                        </>
                      ) : (
                        `加载全部版本（当前 ${versions.length} 条）`
                      )}
                    </Button>
                  )}
                  {!versionSearchActive &&
                    showAllVersions &&
                    filteredVersionEntries.length > VERSION_LIST_INITIAL_CAP && (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-7 text-xs"
                      onClick={() => setShowAllVersions(false)}
                    >
                      收起列表
                    </Button>
                  )}
                </>
              )}

              {category === "mod" && (
                <>
                  <label className="flex items-center gap-2 text-sm">
                    <Checkbox
                      checked={resolveDeps}
                      onCheckedChange={(v) => setResolveDeps(Boolean(v))}
                    />
                    同时安装必需依赖（推荐）
                  </label>
                  {resolveDeps && selectedVersion && (
                    <div className="rounded-md border border-[var(--color-border)] p-3 space-y-1">
                      <div className="text-xs font-medium">将安装的内容</div>
                      {depPreviewLoading ? (
                        <div className="flex items-center gap-2 text-xs text-[var(--color-muted-foreground)]">
                          <Loader2 className="h-3 w-3 animate-spin" />
                          解析依赖…
                        </div>
                      ) : depPreview.length === 0 ? (
                        <p className="text-xs text-[var(--color-muted-foreground)]">
                          无额外依赖
                        </p>
                      ) : (
                        depPreview.map((d) => (
                          <div
                            key={d.fileName}
                            className="flex items-center justify-between text-xs gap-2"
                          >
                            <span className="truncate">
                              {d.name}
                              {d.isDependency && (
                                <span className="text-[var(--color-muted-foreground)]">
                                  {" "}
                                  (依赖)
                                </span>
                              )}
                            </span>
                            <span className="text-[var(--color-muted-foreground)] shrink-0 truncate max-w-[40%]">
                              {d.fileName}
                            </span>
                          </div>
                        ))
                      )}
                    </div>
                  )}
                </>
              )}
            </div>
          </DialogBody>

          <div className="flex shrink-0 flex-nowrap gap-2 border-t border-[var(--color-border)] bg-[var(--color-background)] p-4 [&_button]:min-w-0 [&_button]:flex-1 [&_button]:whitespace-nowrap">
            <Button
              variant="secondary"
              className="flex-1"
              disabled={!selectedVersion || installing || !targetInstance}
              onClick={addToQueue}
            >
              加入队列
            </Button>
            <Button
              className="flex-1"
              disabled={!selectedVersion || installing || !targetInstance}
              onClick={() => void handleInstallNow()}
            >
              {installing ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Download className="h-4 w-4" />
              )}
              立即安装
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}

export async function openDownloadFolder(filePath: string) {
  const dir = filePath.replace(/[/\\][^/\\]+$/, "");
  await openPath(dir);
}
