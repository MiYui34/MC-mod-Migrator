export type ModSource = "modrinth" | "curseforge" | "metadata" | "github" | "sgu" | "unknown";

export type TransferStatus =
  | "transferable"
  | "up_to_date"
  | "incompatible"
  | "unknown";

export type FileAssetCategory =
  | "shader_pack"
  | "resource_pack"
  | "datapack"
  | "litematica"
  | "mod_config"
  | "game_settings";

export type MigrationCategory = "mod" | FileAssetCategory;

export type FileAssetStatus =
  | "transferable"
  | "up_to_date"
  | "conflict"
  | "online_available"
  | "incompatible";

export type ConfigScanMode = "related" | "all";

export type ConflictPolicy = "skip" | "overwrite";

export interface InstanceInfo {
  name: string;
  modsPath: string;
  mcVersion: string;
  loader: string;
  loaderVersion?: string;
  gameDir: string;
  launcher?: string;
}

export interface IdentifiedMod {
  fileName: string;
  filePath: string;
  sha512: string;
  sha1: string;
  fingerprint: number;
  source: ModSource;
  projectId?: string;
  curseforgeId?: number;
  name: string;
  nameZh?: string;
  modId?: string;
  currentVersion?: string;
  loaders: string[];
  gameVersions: string[];
  iconUrl?: string;
  githubUrl?: string;
  depends: string[];
}

export interface ModTransferItem {
  mod: IdentifiedMod;
  status: TransferStatus;
  targetFileName?: string;
  targetVersion?: string;
  downloadUrl?: string;
  downloadSource?: ModSource;
  selected: boolean;
  isDependency?: boolean;
  requiredBy?: string;
}

export type ModDiffKind = "only_in_source" | "only_in_target" | "version_mismatch" | "matched";

export interface ModDiffEntry {
  kind: ModDiffKind;
  matchKey: string;
  source?: IdentifiedMod;
  target?: IdentifiedMod;
}

export interface ModDiffSummary {
  onlyInSource: number;
  onlyInTarget: number;
  versionMismatch: number;
  matched: number;
}

export interface ModDiffResult {
  entries: ModDiffEntry[];
  summary: ModDiffSummary;
}

export interface MigrationWarningItem {
  context: string;
  title?: string;
  files: string[];
}

export interface MigrationWarning {
  code: string;
  severity: string;
  message: string;
  count?: number;
  items?: MigrationWarningItem[];
}

export interface CrossVersionChecklistItem {
  id: string;
  title: string;
  description: string;
  required: boolean;
}

export interface CrossVersionGuide {
  sourceMc: string;
  targetMc: string;
  majorVersionChange: boolean;
  incompatibleCount: number;
  transferableCount: number;
  checklist: CrossVersionChecklistItem[];
}

export interface CompatibilityCheckResponse {
  items: ModTransferItem[];
  warnings: MigrationWarning[];
  crossVersionGuide?: CrossVersionGuide;
}

export interface MigrationPreset {
  id: string;
  name: string;
  sourceMc?: string;
  sourceLoader?: string;
  targetMc?: string;
  targetLoader?: string;
  backupBeforeTransfer?: boolean;
  modReportFormat?: "md" | "txt";
  modVersionPolicy?: string;
  createdAt?: string;
}

export interface FileAsset {
  name: string;
  relativePath: string;
  filePath: string;
  isDirectory: boolean;
  size: number;
  relatedModId?: string;
  /** Companion .txt settings under shaderpacks/ */
  settingsFile?: boolean;
}

export interface FileAssetTransferItem {
  asset: FileAsset;
  status: FileAssetStatus;
  selected: boolean;
  downloadUrl?: string;
  onlineVersion?: string;
  onlineSource?: ModSource;
}

export interface FileAssetScanResult {
  items: FileAssetTransferItem[];
  hint?: string;
}

export interface TargetEnv {
  modsPath: string;
  mcVersion: string;
  loader: string;
  loaderVersion?: string;
}

export interface AppSettings {
  curseforge_api_key: string;
  download_source_priority: string[];
  max_concurrent_downloads: number;
  mod_api_mirror: string;
  mod_version_policy: string;
  auto_check_online_packs: boolean;
  backup_before_transfer: boolean;
  auto_export_mod_report: boolean;
  mod_report_format: "md" | "txt";
  update_manifest_url: string;
  update_use_default_source?: boolean;
  update_mode: "manual" | "auto";
  update_check_interval_hours: number;
}

export interface UpdateManifest {
  version: string;
  releaseDate?: string;
  notes: string;
  downloadUrl: string;
  fileName: string;
  mandatory?: boolean;
  sha256?: string;
  fileSize?: number;
  minSupportedVersion?: string;
  releaseNotesUrl?: string;
}

export interface UpdateCheckResult {
  currentVersion: string;
  updateAvailable: boolean;
  manifest?: UpdateManifest;
}

export interface UpdateProgress {
  downloaded: number;
  total?: number;
  message: string;
}

export interface UpdateState {
  dismissedVersion: string;
  lastCheckAt?: string;
  downloadedPath?: string;
  downloadedVersion?: string;
  lastCheckOk?: boolean;
  lastCheckError?: string;
  lastCheckVersion?: string;
}

export const DEFAULT_UPDATE_MANIFEST_URL =
  "https://www.sgu-server.xin/updates/latest.json";

export function effectiveManifestUrl(settings: AppSettings): string | null {
  const custom = settings.update_manifest_url?.trim();
  if (custom) return custom;
  if (settings.update_use_default_source !== false) {
    return DEFAULT_UPDATE_MANIFEST_URL;
  }
  return null;
}

export interface ModVersionOption {
  version: string;
  fileName: string;
  downloadUrl: string;
  source: ModSource;
  gameVersions: string[];
  recommended: boolean;
  loaders?: string[];
  versionType?: string;
  requiredDependencies?: number;
}

export interface TransferProgress {
  current: number;
  total: number;
  fileName: string;
  message: string;
}

export interface TransferResult {
  success: number;
  failed: number;
  skipped: number;
  errors: string[];
}

export interface ModTransferResponse {
  result: TransferResult;
  transferredNames: string[];
}

export interface MigrationRecord {
  id: string;
  timestamp: string;
  sourceName: string;
  targetName: string;
  sourceMc: string;
  targetMc: string;
  category: string;
  success: number;
  failed: number;
  skipped: number;
  backupId?: string;
  manifestPath?: string;
  reportPath?: string;
}

export interface RestoreResult {
  restored: number;
  removed: number;
  failed: number;
  errors: string[];
}

export interface ImportManifestResult {
  session: AppSession;
  warnings: string[];
}

export interface AppSession {
  sourceInstance?: InstanceInfo | null;
  targetInstance?: InstanceInfo | null;
  mods?: IdentifiedMod[];
  transferItems?: ModTransferItem[];
  fileAssets?: Partial<Record<string, FileAssetTransferItem[]>>;
  configScanMode?: ConfigScanMode;
  shaderIncludeSettings?: boolean;
  autoCheckOnlinePacks?: boolean;
  activeCategory?: MigrationCategory;
  lastTransferResult?: TransferResult;
  lastTransferredModNames?: string[];
}

export const CATEGORY_LABELS: Record<MigrationCategory, string> = {
  mod: "Mod",
  shader_pack: "光影包",
  resource_pack: "材质包",
  datapack: "数据包",
  litematica: "投影文件",
  mod_config: "Mod 配置",
  game_settings: "游戏设置",
};

export const ASSET_STATUS_LABELS: Record<FileAssetStatus, string> = {
  transferable: "可迁移",
  up_to_date: "已存在",
  conflict: "冲突",
  online_available: "可在线下载",
  incompatible: "不兼容",
};

export function formatBytes(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}

export type MarketCategory =
  | "shader_pack"
  | "resource_pack"
  | "mod"
  | "modpack"
  | "datapack"
  | "litematic";

export type MarketSourceFilter = "all" | "modrinth" | "curseforge";
export type MarketSort = "relevance" | "downloads" | "updated";

export type MarketInstallStatus = "not_installed" | "installed" | "updatable";

export interface MarketSearchItem {
  id: string;
  slug: string;
  title: string;
  description: string;
  iconUrl?: string;
  downloads: number;
  source: ModSource;
  installStatus?: MarketInstallStatus;
  installedVersion?: string;
  modpackBadge?: string;
}

export interface MarketItemInstallStatus {
  key: string;
  status: MarketInstallStatus;
  installedVersion?: string;
  installedFile?: string;
}

export interface MarketProjectDetail {
  id: string;
  slug: string;
  title: string;
  description: string;
  body: string;
  iconUrl?: string;
  gallery: string[];
  projectUrl: string;
  license?: string;
  categories: string[];
  modpackBadge?: string;
}

export interface MarketDepPreviewItem {
  name: string;
  fileName: string;
  isDependency: boolean;
}

export interface MarketUpdatableMod {
  projectId: string;
  source: ModSource;
  title: string;
  installedVersion: string;
  installedFile: string;
  latestVersion: string;
  downloadUrl: string;
  latestFileName: string;
}

export interface MarketMissingDep {
  projectId: string;
  source: ModSource;
  title: string;
  depRef: string;
  version: string;
  fileName: string;
  downloadUrl: string;
  requiredBy: string[];
}

export interface MarketMissingDepsScan {
  items: MarketMissingDep[];
  scannedMods: number;
  skippedUnidentified: number;
}

export interface MarketSearchResponse {
  items: MarketSearchItem[];
  page: number;
  pageSize: number;
  totalHits: number;
  hasMore: boolean;
}

export interface MarketInstallJob {
  category: MarketCategory;
  downloadUrl: string;
  fileName: string;
  isDependency?: boolean;
  projectId?: string;
  source?: ModSource;
  modName?: string;
}

export interface MarketInstallResult {
  filePath: string;
  fileName: string;
  needsLauncherImport?: boolean;
  hint?: string;
  recordId?: string;
  installedFiles?: string[];
}

export interface MarketInstallBatchResult {
  results: MarketInstallResult[];
  warnings?: string[];
  recordId: string;
}

export interface MarketInstallRecord {
  id: string;
  timestamp: string;
  targetName: string;
  summary: string;
  filesCreated?: string[];
  backupId?: string;
  undone?: boolean;
}

export interface OpenMarketOptions {
  category?: MarketCategory;
  query?: string;
}

export const MARKET_CATEGORY_LABELS: Record<MarketCategory, string> = {
  shader_pack: "光影包",
  resource_pack: "资源包",
  mod: "Mod",
  modpack: "整合包",
  datapack: "数据包",
  litematic: "投影",
};

export const MARKET_SOURCE_FILTER_LABELS: Record<MarketSourceFilter, string> = {
  all: "全部来源",
  modrinth: "Modrinth",
  curseforge: "CurseForge",
};

export const MARKET_SORT_LABELS: Record<MarketSort, string> = {
  relevance: "相关度",
  downloads: "下载量",
  updated: "最近更新",
};

export const MARKET_SOURCE_LABELS: Record<ModSource, string> = {
  modrinth: "Modrinth",
  curseforge: "CurseForge",
  metadata: "元数据",
  github: "GitHub",
  sgu: "SGU 投影站",
  unknown: "未知",
};

export const MARKET_INSTALL_STATUS_LABELS: Record<MarketInstallStatus, string> = {
  not_installed: "未安装",
  installed: "已安装",
  updatable: "可更新",
};

export const MODPACK_BADGE_LABELS: Record<string, string> = {
  mrpack: "可一键装",
  launcher_import: "需启动器导入",
};
