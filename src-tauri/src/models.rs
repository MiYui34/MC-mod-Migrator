use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ModSource {
    Modrinth,
    Curseforge,
    Metadata,
    Github,
    Sgu,
    Unknown,
}

impl Default for ModSource {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TransferStatus {
    Transferable,
    UpToDate,
    Incompatible,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceInfo {
    pub name: String,
    pub mods_path: String,
    pub mc_version: String,
    pub loader: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub loader_version: String,
    pub game_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launcher: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentifiedMod {
    pub file_name: String,
    pub file_path: String,
    pub sha512: String,
    pub sha1: String,
    pub fingerprint: i64,
    pub source: ModSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curseforge_id: Option<i64>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_zh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mod_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_version: Option<String>,
    pub loaders: Vec<String>,
    pub game_versions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_url: Option<String>,
    #[serde(default)]
    pub depends: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModTransferItem {
    #[serde(rename = "mod")]
    pub mod_info: IdentifiedMod,
    pub status: TransferStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_source: Option<ModSource>,
    pub selected: bool,
    #[serde(default)]
    pub is_dependency: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModDiffKind {
    OnlyInSource,
    OnlyInTarget,
    VersionMismatch,
    Matched,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDiffEntry {
    pub kind: ModDiffKind,
    pub match_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<IdentifiedMod>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<IdentifiedMod>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModDiffSummary {
    pub only_in_source: u32,
    pub only_in_target: u32,
    pub version_mismatch: u32,
    pub matched: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDiffResult {
    pub entries: Vec<ModDiffEntry>,
    pub summary: ModDiffSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationWarningItem {
    pub context: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationWarning {
    pub code: String,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<MigrationWarningItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossVersionChecklistItem {
    pub id: String,
    pub title: String,
    pub description: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossVersionGuide {
    pub source_mc: String,
    pub target_mc: String,
    pub major_version_change: bool,
    pub incompatible_count: u32,
    pub transferable_count: u32,
    pub checklist: Vec<CrossVersionChecklistItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityCheckResponse {
    pub items: Vec<ModTransferItem>,
    pub warnings: Vec<MigrationWarning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cross_version_guide: Option<CrossVersionGuide>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPreset {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_mc: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_loader: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target_mc: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target_loader: String,
    #[serde(default = "default_true")]
    pub backup_before_transfer: bool,
    #[serde(default = "default_mod_report_format")]
    pub mod_report_format: String,
    #[serde(default = "default_mod_version_policy")]
    pub mod_version_policy: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetEnv {
    pub mods_path: String,
    pub mc_version: String,
    pub loader: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub loader_version: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FileAssetCategory {
    ShaderPack,
    ResourcePack,
    Datapack,
    Litematica,
    ModConfig,
    GameSettings,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConfigScanMode {
    #[default]
    Related,
    All,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    #[default]
    Skip,
    Overwrite,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FileAssetStatus {
    Transferable,
    UpToDate,
    Conflict,
    OnlineAvailable,
    Incompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAsset {
    pub name: String,
    pub relative_path: String,
    pub file_path: String,
    pub is_directory: bool,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_mod_id: Option<String>,
    /// When true, companion `.txt` settings in shaderpacks/ (not the zip/folder pack itself).
    #[serde(default)]
    pub settings_file: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAssetTransferItem {
    pub asset: FileAsset,
    pub status: FileAssetStatus,
    pub selected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online_source: Option<ModSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAssetScanResult {
    pub items: Vec<FileAssetTransferItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationRecord {
    pub id: String,
    pub timestamp: String,
    pub source_name: String,
    pub target_name: String,
    pub source_mc: String,
    pub target_mc: String,
    pub category: String,
    pub success: u32,
    pub failed: u32,
    pub skipped: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationManifest {
    pub schema_version: u32,
    pub exported_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_instance: Option<InstanceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_instance: Option<InstanceInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mods: Vec<ManifestModEntry>,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub file_assets: std::collections::HashMap<String, Vec<ManifestAssetEntry>>,
    #[serde(default)]
    pub config_scan_mode: ConfigScanMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestModEntry {
    pub file_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mod_id: Option<String>,
    pub selected: bool,
    pub status: TransferStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestAssetEntry {
    pub relative_path: String,
    pub selected: bool,
    pub status: FileAssetStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportManifestResult {
    pub session: AppSession,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AppSettings {
    pub curseforge_api_key: String,
    pub download_source_priority: Vec<String>,
    pub max_concurrent_downloads: u32,
    /// Modrinth API mirror: `official` | `mcim` | `auto` (MCIM first, same as PCL2/HMCL domestic path).
    #[serde(default = "default_mod_api_mirror")]
    pub mod_api_mirror: String,
    /// `auto` = pick best compatible; `downgrade` = prefer version <= source mod, never auto-upgrade.
    #[serde(default = "default_mod_version_policy")]
    pub mod_version_policy: String,
    #[serde(default = "default_true")]
    pub auto_check_online_packs: bool,
    #[serde(default = "default_true")]
    pub backup_before_transfer: bool,
    #[serde(default = "default_false")]
    pub auto_export_mod_report: bool,
    #[serde(default = "default_mod_report_format")]
    pub mod_report_format: String,
    /// HTTP URL or local path to `latest.json` update manifest.
    #[serde(default)]
    pub update_manifest_url: String,
    /// When true and `update_manifest_url` is empty, use the official default manifest URL.
    #[serde(default = "default_true")]
    pub update_use_default_source: bool,
    /// `manual` = popup only; `auto` = download in background when update is found.
    #[serde(default = "default_update_mode")]
    pub update_mode: String,
    #[serde(default = "default_update_check_hours")]
    pub update_check_interval_hours: u32,
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_mod_report_format() -> String {
    "md".into()
}

fn default_mod_api_mirror() -> String {
    "auto".into()
}

fn default_mod_version_policy() -> String {
    "auto".into()
}

fn default_update_mode() -> String {
    "manual".into()
}

fn default_update_check_hours() -> u32 {
    24
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            curseforge_api_key: String::new(),
            download_source_priority: vec![
                "modrinth".into(),
                "curseforge".into(),
                "mcmod".into(),
                "github".into(),
            ],
            max_concurrent_downloads: 6,
            mod_api_mirror: default_mod_api_mirror(),
            mod_version_policy: default_mod_version_policy(),
            auto_check_online_packs: true,
            backup_before_transfer: true,
            auto_export_mod_report: false,
            mod_report_format: default_mod_report_format(),
            update_manifest_url: String::new(),
            update_use_default_source: true,
            update_mode: default_update_mode(),
            update_check_interval_hours: default_update_check_hours(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateManifest {
    pub version: String,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub notes: String,
    pub download_url: String,
    pub file_name: String,
    #[serde(default)]
    pub mandatory: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_supported_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_notes_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub update_available: bool,
    pub manifest: Option<UpdateManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    #[serde(default)]
    pub dismissed_version: String,
    #[serde(default)]
    pub last_check_at: Option<String>,
    #[serde(default)]
    pub downloaded_path: Option<String>,
    #[serde(default)]
    pub downloaded_version: Option<String>,
    #[serde(default)]
    pub last_check_ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateServerStatus {
    pub running: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModVersionOption {
    pub version: String,
    pub file_name: String,
    pub download_url: String,
    pub source: ModSource,
    pub game_versions: Vec<String>,
    pub recommended: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loaders: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version_type: String,
    #[serde(default)]
    pub required_dependencies: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub current: u32,
    pub total: u32,
    pub file_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferResult {
    pub success: u32,
    pub failed: u32,
    pub skipped: u32,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModTransferResponse {
    pub result: TransferResult,
    pub transferred_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppSession {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_instance: Option<InstanceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_instance: Option<InstanceInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mods: Vec<IdentifiedMod>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transfer_items: Vec<ModTransferItem>,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub file_assets: std::collections::HashMap<String, Vec<FileAssetTransferItem>>,
    #[serde(default)]
    pub config_scan_mode: ConfigScanMode,
    #[serde(default = "default_true")]
    pub shader_include_settings: bool,
    #[serde(default = "default_true")]
    pub auto_check_online_packs: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_transfer_result: Option<TransferResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub last_transferred_mod_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ModFile {
    pub file_name: String,
    pub download_url: String,
    pub version: String,
    pub source: ModSource,
}

#[derive(Debug, Clone)]
pub struct FileHash {
    pub path: String,
    pub file_name: String,
    pub sha512: String,
    pub sha1: String,
    pub fingerprint: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MarketSourceFilter {
    #[default]
    All,
    Modrinth,
    Curseforge,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MarketSort {
    #[default]
    Relevance,
    Downloads,
    Updated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MarketCategory {
    ShaderPack,
    ResourcePack,
    Mod,
    Modpack,
    Datapack,
    Litematic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketSearchItem {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub source: ModSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_status: Option<MarketInstallStatusKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modpack_badge: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MarketInstallStatusKind {
    NotInstalled,
    Installed,
    Updatable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketItemInstallStatus {
    pub key: String,
    pub status: MarketInstallStatusKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketProjectDetail {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gallery: Vec<String>,
    pub project_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modpack_badge: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketDepPreviewItem {
    pub name: String,
    pub file_name: String,
    pub is_dependency: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketMissingDep {
    pub project_id: String,
    pub source: ModSource,
    pub title: String,
    pub dep_ref: String,
    pub version: String,
    pub file_name: String,
    pub download_url: String,
    pub required_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketMissingDepsScan {
    pub items: Vec<MarketMissingDep>,
    pub scanned_mods: u32,
    pub skipped_unidentified: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketUpdatableMod {
    pub project_id: String,
    pub source: ModSource,
    pub title: String,
    pub installed_version: String,
    pub installed_file: String,
    pub latest_version: String,
    pub download_url: String,
    pub latest_file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketSearchResponse {
    pub items: Vec<MarketSearchItem>,
    pub page: u32,
    pub page_size: u32,
    pub total_hits: u32,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketInstallJob {
    pub category: MarketCategory,
    pub download_url: String,
    pub file_name: String,
    #[serde(default)]
    pub is_dependency: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ModSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mod_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketInstallResult {
    pub file_path: String,
    pub file_name: String,
    #[serde(default)]
    pub needs_launcher_import: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub installed_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketInstallBatchResult {
    pub results: Vec<MarketInstallResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    pub record_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketInstallRecord {
    pub id: String,
    pub timestamp: String,
    pub target_name: String,
    pub summary: String,
    #[serde(default)]
    pub files_created: Vec<String>,
    #[serde(default)]
    pub backup_id: Option<String>,
    #[serde(default)]
    pub undone: bool,
}
