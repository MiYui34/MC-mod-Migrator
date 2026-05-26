use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use futures::future::join_all;

use crate::commands::deps::{
    resolve_for_transfer, resolve_missing_deps_for_installed, sort_download_order,
};
use crate::commands::market::{identified_from_market, market_list_versions};
use crate::commands::transfer::resolve_target_env;
use crate::compat::filename_stem;
use crate::db::AppDatabase;
use crate::hash::compute_file_hashes;
use crate::http::build_http_client;
use crate::instance::{resolve_instance_paths, resolve_schematic_roots};
use crate::jar::parse_jar_metadata;
use crate::models::{
    AppSettings, IdentifiedMod, InstanceInfo, MarketCategory, MarketDepPreviewItem,
    MarketInstallStatusKind, MarketItemInstallStatus, MarketMissingDep, MarketMissingDepsScan,
    MarketProjectDetail, MarketSearchItem, MarketUpdatableMod, ModSource, ModTransferItem,
    ModVersionOption, TargetEnv, TransferStatus,
};
use crate::providers::endpoints::{cf_usable_mirrors, mirrors_with_official_first};
use crate::providers::modrinth::ProjectVersionCache;
use crate::providers::sgu_litematic;

struct TargetAssetIndex {
    modrinth_projects: HashMap<String, (String, Option<String>)>,
    cf_projects: HashMap<String, (String, Option<String>)>,
    mod_ids: HashMap<String, (String, Option<String>)>,
    file_stems: HashMap<String, (String, Option<String>)>,
    pack_files: HashSet<String>,
}

fn normalize_key(s: &str) -> String {
    s.to_lowercase()
}

fn pack_stem(name: &str) -> String {
    normalize_key(&filename_stem(name))
}

fn build_target_asset_index(
    instance: &InstanceInfo,
    category: MarketCategory,
    db: Option<&AppDatabase>,
) -> TargetAssetIndex {
    let mut index = TargetAssetIndex {
        modrinth_projects: HashMap::new(),
        cf_projects: HashMap::new(),
        mod_ids: HashMap::new(),
        file_stems: HashMap::new(),
        pack_files: HashSet::new(),
    };

    match category {
        MarketCategory::Mod => {
            let mods_path = Path::new(&instance.mods_path);
            if let Ok(entries) = std::fs::read_dir(mods_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if !name.to_lowercase().ends_with(".jar") {
                        continue;
                    }
                    let version = parse_jar_metadata(&path)
                        .ok()
                        .and_then(|m| m.version);
                    index
                        .file_stems
                        .insert(pack_stem(&name), (name.clone(), version.clone()));

                    if let Ok(meta) = parse_jar_metadata(&path) {
                        if let Some(mod_id) = meta.mod_id {
                            index
                                .mod_ids
                                .insert(normalize_key(&mod_id), (name.clone(), version.clone()));
                        }
                    }

                    if let Some(db) = db {
                        if let Ok(hash) = compute_file_hashes(&path) {
                            if let Ok(Some(cached)) = db.get_cached_mod(&hash.sha512) {
                                let ver = cached.current_version.clone();
                                if let Some(pid) = cached.project_id {
                                    index.modrinth_projects.insert(
                                        normalize_key(&pid),
                                        (name.clone(), ver.clone()),
                                    );
                                }
                                if let Some(cf_id) = cached.curseforge_id {
                                    index
                                        .cf_projects
                                        .insert(cf_id.to_string(), (name.clone(), ver.clone()));
                                }
                                if let Some(mod_id) = cached.mod_id {
                                    index
                                        .mod_ids
                                        .entry(normalize_key(&mod_id))
                                        .or_insert((name.clone(), ver.clone()));
                                }
                            }
                        }
                    }
                }
            }
        }
        MarketCategory::ShaderPack | MarketCategory::ResourcePack | MarketCategory::Datapack => {
            let paths = resolve_instance_paths(instance);
            let dir = match category {
                MarketCategory::ShaderPack => paths.shaderpacks,
                MarketCategory::ResourcePack => paths.resourcepacks,
                MarketCategory::Datapack => paths.datapacks,
                _ => return index,
            };
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    index.pack_files.insert(pack_stem(&name));
                }
            }
        }
        MarketCategory::Litematic => {
            for dir in resolve_schematic_roots(instance) {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        index.pack_files.insert(pack_stem(&name));
                    }
                }
            }
        }
        MarketCategory::Modpack => {}
    }

    index
}

fn modpack_badge_for(source: ModSource) -> Option<String> {
    match source {
        ModSource::Modrinth => Some("mrpack".to_string()),
        ModSource::Curseforge => Some("launcher_import".to_string()),
        _ => None,
    }
}

pub fn annotate_modpack_badges(category: MarketCategory, items: &mut [MarketSearchItem]) {
    if category != MarketCategory::Modpack {
        return;
    }
    for item in items.iter_mut() {
        item.modpack_badge = modpack_badge_for(item.source.clone());
    }
}

fn market_item_key(source: &ModSource, id: &str) -> String {
    let tag = match source {
        ModSource::Modrinth => "modrinth",
        ModSource::Curseforge => "curseforge",
        ModSource::Metadata => "metadata",
        ModSource::Github => "github",
        ModSource::Sgu => "sgu",
        ModSource::Unknown => "unknown",
    };
    format!("{tag}:{id}")
}

pub async fn annotate_install_status(
    category: MarketCategory,
    items: &mut [MarketSearchItem],
    target: &InstanceInfo,
    settings: &AppSettings,
    version_cache: &ProjectVersionCache,
    db: Option<&AppDatabase>,
    quick_check: bool,
) -> anyhow::Result<()> {
    let index = build_target_asset_index(target, category, db);
    let checks: Vec<_> = items
        .iter()
        .map(|item| {
            check_item_status(
                category,
                item,
                target,
                &index,
                settings,
                version_cache,
                quick_check,
            )
        })
        .collect();
    let statuses = join_all(checks).await;
    for (item, status) in items.iter_mut().zip(statuses) {
        let (kind, installed_version, _) = status?;
        item.install_status = Some(kind);
        item.installed_version = installed_version;
    }
    Ok(())
}

async fn check_item_status(
    category: MarketCategory,
    item: &MarketSearchItem,
    target: &InstanceInfo,
    index: &TargetAssetIndex,
    settings: &AppSettings,
    version_cache: &ProjectVersionCache,
    quick_check: bool,
) -> anyhow::Result<(MarketInstallStatusKind, Option<String>, Option<String>)> {
    match category {
        MarketCategory::Mod => {
            let hit = match item.source {
                ModSource::Modrinth => index
                    .modrinth_projects
                    .get(&normalize_key(&item.id))
                    .or_else(|| index.mod_ids.get(&normalize_key(&item.slug)))
                    .or_else(|| index.file_stems.get(&pack_stem(&item.slug))),
                ModSource::Curseforge => index
                    .cf_projects
                    .get(&item.id)
                    .or_else(|| index.file_stems.get(&pack_stem(&item.slug))),
                _ => index.file_stems.get(&pack_stem(&item.slug)),
            };
            if let Some((file, ver)) = hit {
                if quick_check {
                    return Ok((
                        MarketInstallStatusKind::Installed,
                        ver.clone(),
                        Some(file.clone()),
                    ));
                }
                let latest = fetch_recommended_version(
                    category,
                    item.source.clone(),
                    &item.id,
                    target,
                    settings,
                    version_cache,
                )
                .await?;
                if let Some(latest) = latest {
                    if ver.as_deref() != Some(latest.version.as_str()) {
                        return Ok((
                            MarketInstallStatusKind::Updatable,
                            ver.clone(),
                            Some(file.clone()),
                        ));
                    }
                }
                return Ok((
                    MarketInstallStatusKind::Installed,
                    ver.clone(),
                    Some(file.clone()),
                ));
            }
            Ok((MarketInstallStatusKind::NotInstalled, None, None))
        }
        MarketCategory::ShaderPack | MarketCategory::ResourcePack | MarketCategory::Datapack => {
            let stem = pack_stem(&item.slug);
            let title_stem = pack_stem(&item.title);
            if index.pack_files.contains(&stem) || index.pack_files.contains(&title_stem) {
                Ok((MarketInstallStatusKind::Installed, None, None))
            } else {
                Ok((MarketInstallStatusKind::NotInstalled, None, None))
            }
        }
        MarketCategory::Litematic => {
            let file_stem = pack_stem(&sgu_litematic::schematic_file_name(&item.title));
            let title_stem = pack_stem(&item.title);
            if index.pack_files.contains(&file_stem) || index.pack_files.contains(&title_stem) {
                Ok((MarketInstallStatusKind::Installed, None, None))
            } else {
                Ok((MarketInstallStatusKind::NotInstalled, None, None))
            }
        }
        MarketCategory::Modpack => Ok((MarketInstallStatusKind::NotInstalled, None, None)),
    }
}

async fn fetch_recommended_version(
    category: MarketCategory,
    source: ModSource,
    project_id: &str,
    target: &InstanceInfo,
    settings: &AppSettings,
    version_cache: &ProjectVersionCache,
) -> anyhow::Result<Option<ModVersionOption>> {
    let target_env = resolve_target_env(
        TargetEnv {
            mods_path: target.mods_path.clone(),
            mc_version: target.mc_version.clone(),
            loader: target.loader.clone(),
            loader_version: target.loader_version.clone(),
        },
        &[],
    );
    let versions = market_list_versions(
        category,
        source,
        project_id.to_string(),
        target_env,
        settings,
        version_cache,
        false,
    )
    .await?;
    Ok(versions
        .iter()
        .find(|v| v.recommended)
        .cloned()
        .or_else(|| versions.first().cloned()))
}

pub async fn market_check_installed(
    category: MarketCategory,
    items: Vec<MarketSearchItem>,
    target: InstanceInfo,
    settings: &AppSettings,
    version_cache: &ProjectVersionCache,
    db: Option<&AppDatabase>,
    quick_check: bool,
) -> anyhow::Result<Vec<MarketItemInstallStatus>> {
    let index = build_target_asset_index(&target, category, db);
    let checks: Vec<_> = items
        .iter()
        .map(|item| {
            check_item_status(
                category,
                item,
                &target,
                &index,
                settings,
                version_cache,
                quick_check,
            )
        })
        .collect();
    let statuses = join_all(checks).await;
    let mut out = Vec::with_capacity(items.len());
    for (item, status) in items.iter().zip(statuses) {
        let (status, installed_version, installed_file) = status?;
        out.push(MarketItemInstallStatus {
            key: market_item_key(&item.source, &item.id),
            status,
            installed_version,
            installed_file,
        });
    }
    Ok(out)
}

pub async fn market_get_project_detail(
    category: MarketCategory,
    source: ModSource,
    project_id: String,
    settings: &AppSettings,
) -> anyhow::Result<MarketProjectDetail> {
    match source {
        ModSource::Modrinth => fetch_modrinth_detail(&project_id, category, settings).await,
        ModSource::Curseforge => {
            fetch_cf_detail(project_id.parse().unwrap_or(0), category, settings).await
        }
        ModSource::Sgu if category == MarketCategory::Litematic => {
            sgu_litematic::fetch_schematic_detail(&project_id).await
        }
        _ => anyhow::bail!("不支持的项目来源"),
    }
}

async fn fetch_modrinth_detail(
    project_id: &str,
    category: MarketCategory,
    settings: &AppSettings,
) -> anyhow::Result<MarketProjectDetail> {
    for endpoints in mirrors_with_official_first(&settings.mod_api_mirror) {
        let client = build_http_client(crate::http::APP_USER_AGENT);
        let url = format!("{}/project/{project_id}", endpoints.api_base);
        let resp = match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            client.get(&url).send(),
        )
        .await
        {
            Ok(Ok(r)) => r,
            _ => continue,
        };
        if !resp.status().is_success() {
            continue;
        }
        let body = resp.bytes().await?;
        let p: ModrinthProjectFull = match serde_json::from_slice(&body) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let project_url = modrinth_project_url(&p.slug, category);
        return Ok(MarketProjectDetail {
            id: p.id,
            slug: p.slug,
            title: p.title,
            description: p.description.unwrap_or_default(),
            body: p.body.unwrap_or_default(),
            icon_url: p.icon_url,
            gallery: modrinth_gallery_urls(p.gallery),
            project_url,
            license: modrinth_license_label(p.license),
            categories: p.categories.unwrap_or_default(),
            modpack_badge: if category == MarketCategory::Modpack {
                Some("mrpack".to_string())
            } else {
                None
            },
        });
    }
    anyhow::bail!("无法获取 Modrinth 项目详情")
}

async fn fetch_cf_detail(
    mod_id: i64,
    category: MarketCategory,
    settings: &AppSettings,
) -> anyhow::Result<MarketProjectDetail> {
    if mod_id <= 0 {
        anyhow::bail!("无效的 CurseForge 项目 ID");
    }
    for endpoints in cf_usable_mirrors(settings) {
        let client = build_http_client(crate::http::APP_USER_AGENT);
        let mut req = client.get(format!("{}/mods/{mod_id}", endpoints.api_base));
        if endpoints.needs_api_key && !settings.curseforge_api_key.is_empty() {
            req = req.header("x-api-key", &settings.curseforge_api_key);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            continue;
        }
        let body = resp.bytes().await?;
        let wrapper: CfDetailResponse = serde_json::from_slice(&body)?;
        let p = wrapper.data;
        let slug = p.slug.clone().unwrap_or_else(|| mod_id.to_string());
        let gallery: Vec<String> = p
            .screenshots
            .unwrap_or_default()
            .into_iter()
            .filter_map(|s| s.url)
            .collect();
        return Ok(MarketProjectDetail {
            id: mod_id.to_string(),
            slug: slug.clone(),
            title: p.name,
            description: p.summary.unwrap_or_default(),
            body: p.description.unwrap_or_default(),
            icon_url: p.logo.and_then(|l| l.url),
            gallery,
            project_url: cf_project_url(category, mod_id, &slug),
            license: None,
            categories: Vec::new(),
            modpack_badge: if category == MarketCategory::Modpack {
                Some("launcher_import".to_string())
            } else {
                None
            },
        });
    }
    anyhow::bail!("无法获取 CurseForge 项目详情（请检查网络或 Mod API 镜像设置）")
}

pub async fn market_preview_deps(
    download_url: String,
    file_name: String,
    project_id: String,
    source: ModSource,
    _mod_name: String,
    target: InstanceInfo,
    settings: &AppSettings,
    version_cache: &ProjectVersionCache,
    cancel: &crate::cancellation::CancelToken,
) -> anyhow::Result<Vec<MarketDepPreviewItem>> {
    let source_ref = source.clone();
    let mut items = vec![ModTransferItem {
        mod_info: identified_from_market(&source_ref, &project_id),
        status: TransferStatus::Transferable,
        target_file_name: Some(file_name),
        target_version: None,
        download_url: Some(download_url),
        download_source: Some(source),
        selected: true,
        is_dependency: false,
        required_by: None,
    }];
    let target_env = resolve_target_env(
        TargetEnv {
            mods_path: target.mods_path.clone(),
            mc_version: target.mc_version.clone(),
            loader: target.loader.clone(),
            loader_version: target.loader_version.clone(),
        },
        &[],
    );
    resolve_for_transfer(
        None,
        &mut items,
        &[],
        &target_env,
        settings,
        Some(version_cache),
        cancel,
    )
    .await?;

    let sorted = sort_download_order(items);
    Ok(sorted
        .into_iter()
        .map(|i| MarketDepPreviewItem {
            name: if i.mod_info.name.is_empty() {
                i.mod_info.file_name.clone()
            } else {
                i.mod_info.name
            },
            file_name: i
                .target_file_name
                .unwrap_or_else(|| i.mod_info.file_name),
            is_dependency: i.is_dependency,
        })
        .collect())
}

pub async fn market_list_updatable_mods(
    target: InstanceInfo,
    settings: &AppSettings,
    version_cache: &ProjectVersionCache,
    db: &AppDatabase,
    cancel: &crate::cancellation::CancelToken,
) -> anyhow::Result<Vec<MarketUpdatableMod>> {
    cancel.ensure_running()?;
    let mods_path = Path::new(&target.mods_path);
    let Ok(entries) = std::fs::read_dir(mods_path) else {
        return Ok(vec![]);
    };

    let mut updatable = Vec::new();
    for entry in entries.flatten() {
        cancel.ensure_running()?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if !file_name.to_lowercase().ends_with(".jar") {
            continue;
        }
        let hash = match compute_file_hashes(&path) {
            Ok(h) => h,
            Err(_) => continue,
        };
        let cached = match db.get_cached_mod(&hash.sha512)? {
            Some(c) => c,
            None => continue,
        };
        let (source, project_id) = if let Some(pid) = cached.project_id.clone() {
            (ModSource::Modrinth, pid)
        } else if let Some(cf_id) = cached.curseforge_id {
            (ModSource::Curseforge, cf_id.to_string())
        } else {
            continue;
        };

        let latest = match fetch_recommended_version(
            MarketCategory::Mod,
            source.clone(),
            &project_id,
            &target,
            settings,
            version_cache,
        )
        .await
        {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(latest) = latest else {
            continue;
        };
        let installed_version = cached
            .current_version
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        if installed_version == latest.version {
            continue;
        }

        updatable.push(MarketUpdatableMod {
            project_id,
            source,
            title: if cached.name.is_empty() {
                file_name.clone()
            } else {
                cached.name
            },
            installed_version,
            installed_file: file_name,
            latest_version: latest.version,
            download_url: latest.download_url,
            latest_file_name: latest.file_name,
        });
    }
    Ok(updatable)
}

fn load_target_cached_mods(
    target: &InstanceInfo,
    db: &AppDatabase,
    cancel: &crate::cancellation::CancelToken,
) -> anyhow::Result<(Vec<IdentifiedMod>, u32)> {
    cancel.ensure_running()?;
    let mods_path = Path::new(&target.mods_path);
    let Ok(entries) = std::fs::read_dir(mods_path) else {
        return Ok((vec![], 0));
    };

    let mut mods = Vec::new();
    let mut skipped = 0u32;
    for entry in entries.flatten() {
        cancel.ensure_running()?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if !file_name.to_lowercase().ends_with(".jar") {
            continue;
        }
        let hash = match compute_file_hashes(&path) {
            Ok(h) => h,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let Some(mut cached) = db.get_cached_mod(&hash.sha512)? else {
            skipped += 1;
            continue;
        };
        cached.file_path = path.to_string_lossy().into_owned();
        cached.file_name = file_name;
        cached.sha512 = hash.sha512;
        cached.sha1 = hash.sha1;
        cached.fingerprint = hash.fingerprint;
        if cached.depends.is_empty() {
            if let Ok(meta) = parse_jar_metadata(&path) {
                if !meta.depends.is_empty() {
                    cached.depends = meta.depends;
                }
            }
        }
        mods.push(cached);
    }
    Ok((mods, skipped))
}

fn merge_missing_dep_items(items: Vec<ModTransferItem>) -> Vec<MarketMissingDep> {
    let mut merged: HashMap<String, MarketMissingDep> = HashMap::new();
    for item in items {
        let Some(download_url) = item.download_url.clone() else {
            continue;
        };
        let file_name = item
            .target_file_name
            .clone()
            .unwrap_or_else(|| item.mod_info.file_name.clone());
        let version = item
            .target_version
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let project_id = item
            .mod_info
            .project_id
            .clone()
            .unwrap_or_else(|| item.mod_info.mod_id.clone().unwrap_or_else(|| file_name.clone()));
        let dep_ref = item
            .mod_info
            .mod_id
            .clone()
            .unwrap_or_else(|| project_id.clone());
        let key = project_id.to_lowercase();
        let title = if item.mod_info.name.is_empty() {
            dep_ref.clone()
        } else {
            item.mod_info.name.clone()
        };
        let source = item.download_source.clone().unwrap_or(ModSource::Modrinth);
        let required_by = item.required_by.clone().unwrap_or_default();

        merged
            .entry(key)
            .and_modify(|existing| {
                if !required_by.is_empty() && !existing.required_by.contains(&required_by) {
                    existing.required_by.push(required_by.clone());
                }
            })
            .or_insert(MarketMissingDep {
                project_id,
                source,
                title,
                dep_ref,
                version,
                file_name,
                download_url,
                required_by: if required_by.is_empty() {
                    vec![]
                } else {
                    vec![required_by]
                },
            });
    }
    let mut out: Vec<_> = merged.into_values().collect();
    out.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    out
}

pub async fn market_list_missing_deps(
    target: InstanceInfo,
    settings: &AppSettings,
    version_cache: &ProjectVersionCache,
    db: &AppDatabase,
    cancel: &crate::cancellation::CancelToken,
) -> anyhow::Result<MarketMissingDepsScan> {
    let (installed_mods, skipped_unidentified) = load_target_cached_mods(&target, db, cancel)?;
    let scanned_mods = installed_mods.len() as u32;
    if installed_mods.is_empty() {
        return Ok(MarketMissingDepsScan {
            items: vec![],
            scanned_mods: 0,
            skipped_unidentified,
        });
    }

    let target_env = resolve_target_env(
        TargetEnv {
            mods_path: target.mods_path.clone(),
            mc_version: target.mc_version.clone(),
            loader: target.loader.clone(),
            loader_version: target.loader_version.clone(),
        },
        &installed_mods,
    );

    let dep_items = resolve_missing_deps_for_installed(
        None,
        &target_env,
        &installed_mods,
        settings,
        Some(version_cache),
        cancel,
    )
    .await?;

    let sorted = sort_download_order(dep_items);
    Ok(MarketMissingDepsScan {
        items: merge_missing_dep_items(sorted),
        scanned_mods,
        skipped_unidentified,
    })
}

fn modrinth_project_url(slug: &str, category: MarketCategory) -> String {
    let segment = match category {
        MarketCategory::ShaderPack => "shader",
        MarketCategory::ResourcePack => "resourcepack",
        MarketCategory::Datapack => "datapack",
        MarketCategory::Modpack => "modpack",
        MarketCategory::Mod | MarketCategory::Litematic => "mod",
    };
    format!("https://modrinth.com/{segment}/{slug}")
}

fn cf_project_url(category: MarketCategory, mod_id: i64, slug: &str) -> String {
    let path = match category {
        MarketCategory::ShaderPack => "shaders",
        MarketCategory::ResourcePack => "texture-packs",
        MarketCategory::Datapack => "data-packs",
        MarketCategory::Modpack => "modpacks",
        MarketCategory::Mod | MarketCategory::Litematic => "mc-mods",
    };
    if slug.is_empty() {
        format!("https://www.curseforge.com/minecraft/{path}/{mod_id}")
    } else {
        format!("https://www.curseforge.com/minecraft/{path}/{slug}")
    }
}

#[derive(Debug, Deserialize)]
struct ModrinthGalleryEntry {
    url: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ModrinthGalleryWire {
    Url(String),
    Item(ModrinthGalleryEntry),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ModrinthLicenseWire {
    Text(String),
    Object {
        #[serde(default)]
        id: String,
        #[serde(default)]
        name: Option<String>,
    },
}

fn modrinth_gallery_urls(items: Option<Vec<ModrinthGalleryWire>>) -> Vec<String> {
    items
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| match entry {
            ModrinthGalleryWire::Url(url) => Some(url),
            ModrinthGalleryWire::Item(item) => Some(item.url),
        })
        .collect()
}

fn modrinth_license_label(license: Option<ModrinthLicenseWire>) -> Option<String> {
    license.map(|l| match l {
        ModrinthLicenseWire::Text(text) => text,
        ModrinthLicenseWire::Object { id, name } => name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(id),
    })
}

#[derive(Debug, Deserialize)]
struct ModrinthProjectFull {
    id: String,
    slug: String,
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    body: Option<String>,
    icon_url: Option<String>,
    #[serde(default)]
    gallery: Option<Vec<ModrinthGalleryWire>>,
    #[serde(default)]
    license: Option<ModrinthLicenseWire>,
    #[serde(default)]
    categories: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct CfDetailResponse {
    data: CfModDetail,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfModDetail {
    name: String,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    description: Option<String>,
    logo: Option<CfLogoDetail>,
    #[serde(default)]
    screenshots: Option<Vec<CfScreenshot>>,
}

#[derive(Debug, Deserialize)]
struct CfLogoDetail {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CfScreenshot {
    url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modrinth_project_full_parses_object_gallery_and_license() {
        let json = r#"{
            "id": "abc",
            "slug": "demo",
            "title": "Demo",
            "description": "desc",
            "gallery": [{"url": "https://cdn.example/a.png"}],
            "license": {"id": "MIT", "name": "MIT License"}
        }"#;
        let p: ModrinthProjectFull = serde_json::from_str(json).expect("parse");
        assert_eq!(modrinth_gallery_urls(p.gallery), vec!["https://cdn.example/a.png".to_string()]);
        assert_eq!(
            modrinth_license_label(p.license).as_deref(),
            Some("MIT License")
        );
    }
}
