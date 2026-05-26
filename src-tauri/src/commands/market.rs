use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use tauri::AppHandle;

use crate::cancellation::CancelToken;
use crate::commands::deps::{resolve_for_transfer, sort_download_order};
use crate::commands::market_undo::{self, MarketInstallSession};
use crate::commands::modpack;
use crate::commands::progress::MonotonicEmitter;
use crate::commands::transfer::resolve_target_env;
use crate::compat::{CompatTarget, ModVersionPolicy};
use crate::http::{build_http_client, download_raw_file, download_zip_file_validated};
use crate::instance::{enrich_target_env, resolve_instance_paths, resolve_schematic_roots};
use crate::models::{
    AppSettings, IdentifiedMod, InstanceInfo, MarketCategory, MarketInstallBatchResult,
    MarketInstallJob, MarketInstallResult, MarketSearchItem, MarketSearchResponse, MarketSort,
    MarketSourceFilter, ModSource, ModTransferItem, ModVersionOption, TargetEnv, TransferStatus,
};
use crate::providers::curseforge::{
    list_curseforge_versions, parse_cf_mod_file_ids, resolve_cf_download_url,
};
use crate::providers::endpoints::{
    cf_usable_mirrors, mirrors_with_official_fallback, rewrite_cf_download_url, CurseForgeEndpoints,
    ModrinthEndpoints,
};
use crate::providers::market_pcl::{chinese_mod_search, is_chinese_query, merge_and_rank_items};
use crate::providers::modrinth::{list_modrinth_versions, ProjectVersionCache};
use crate::providers::packs::{lookup_online_pack, PackKind};
use crate::providers::sgu_litematic;
use crate::version::{loader_query_tags, normalize_mc_version};

const PAGE_SIZE: u32 = 20;
const MINECRAFT_GAME_ID: i64 = 432;

struct MarketSearchPage {
    items: Vec<MarketSearchItem>,
    total_hits: u32,
    page_count: u32,
}

impl MarketSearchPage {
    fn empty() -> Self {
        Self {
            items: vec![],
            total_hits: 0,
            page_count: 0,
        }
    }
}

fn provider_has_more(page_count: u32, page_size: u32, page_index: u32, total_hits: u32) -> bool {
    if page_count == 0 {
        return false;
    }
    if page_count >= page_size {
        return true;
    }
    if total_hits == 0 {
        return false;
    }
    page_index.saturating_add(1).saturating_mul(page_size) < total_hits
}

pub struct MarketInstallContext<'a> {
    pub target: &'a InstanceInfo,
    pub settings: &'a AppSettings,
    pub cancel: &'a CancelToken,
    pub client: &'a Client,
}

struct CategoryParams {
    modrinth_type: &'static str,
    cf_class_id: i64,
}

/// Mod 与整合包版本才按加载器筛选；光影/材质/数据包在 Modrinth/CF 上通常无加载器标签。
fn category_uses_loader_filter(category: MarketCategory) -> bool {
    matches!(category, MarketCategory::Mod | MarketCategory::Modpack)
}

fn category_params(category: MarketCategory) -> CategoryParams {
    match category {
        MarketCategory::ShaderPack => CategoryParams {
            modrinth_type: "shader",
            cf_class_id: 6552,
        },
        MarketCategory::ResourcePack => CategoryParams {
            modrinth_type: "resourcepack",
            cf_class_id: 12,
        },
        MarketCategory::Mod => CategoryParams {
            modrinth_type: "mod",
            cf_class_id: 6,
        },
        MarketCategory::Modpack => CategoryParams {
            modrinth_type: "modpack",
            cf_class_id: 4471,
        },
        MarketCategory::Datapack => CategoryParams {
            modrinth_type: "datapack",
            cf_class_id: 6945,
        },
        MarketCategory::Litematic => CategoryParams {
            modrinth_type: "",
            cf_class_id: 0,
        },
    }
}

fn cf_sort_field(sort: MarketSort) -> &'static str {
    match sort {
        MarketSort::Relevance => "2",
        MarketSort::Downloads => "6",
        MarketSort::Updated => "3",
    }
}

fn modrinth_index(sort: MarketSort) -> &'static str {
    match sort {
        MarketSort::Relevance => "relevance",
        MarketSort::Downloads => "downloads",
        MarketSort::Updated => "updated",
    }
}

pub async fn market_search(
    category: MarketCategory,
    query: String,
    mc_version: String,
    loader: String,
    page: u32,
    source_filter: MarketSourceFilter,
    sort: MarketSort,
    relax_filters: bool,
    compatible_only: bool,
    settings: &AppSettings,
) -> anyhow::Result<MarketSearchResponse> {
    if category == MarketCategory::Litematic {
        return sgu_litematic::search_litematics(&query, page, PAGE_SIZE, sort).await;
    }

    let params = category_params(category);
    let page = page.max(0);
    let offset = page * PAGE_SIZE;
    let client = build_http_client(crate::http::APP_USER_AGENT);
    let effective_relax = relax_filters && !compatible_only;

    let should_search_modrinth = source_filter == MarketSourceFilter::All
        || source_filter == MarketSourceFilter::Modrinth;
    let should_search_cf = source_filter == MarketSourceFilter::All
        || source_filter == MarketSourceFilter::Curseforge;

    let modrinth_fut = async {
        if !should_search_modrinth {
            return MarketSearchPage::empty();
        }
        for endpoints in mirrors_with_official_fallback(&settings.mod_api_mirror) {
            match tokio::time::timeout(
                Duration::from_secs(10),
                search_modrinth(
                    &client,
                    &endpoints,
                    &query,
                    params.modrinth_type,
                    &mc_version,
                    &loader,
                    category,
                    offset,
                    PAGE_SIZE,
                    sort,
                    effective_relax,
                ),
            )
            .await
            {
                Ok(Ok(page)) if !page.items.is_empty() => return page,
                Ok(Ok(_)) => {}
                Ok(Err(_)) => {}
                Err(_) => {}
            }
        }
        MarketSearchPage::empty()
    };

    let cf_fut = async {
        if !should_search_cf {
            return MarketSearchPage::empty();
        }
        for endpoints in cf_usable_mirrors(settings) {
            let search = tokio::time::timeout(
                Duration::from_secs(10),
                search_curseforge(
                    &client,
                    &endpoints,
                    &settings.curseforge_api_key,
                    &query,
                    params.cf_class_id,
                    page,
                    PAGE_SIZE,
                    sort,
                    category,
                    &mc_version,
                    &loader,
                    compatible_only,
                ),
            )
            .await;
            if let Ok(Ok(page)) = search {
                if !page.items.is_empty() {
                    return page;
                }
            }
        }
        MarketSearchPage::empty()
    };

    let browse_mode = query.trim().is_empty();
    let zh_fut = async {
        if page == 0
            && !browse_mode
            && (category == MarketCategory::Mod || category == MarketCategory::Datapack)
            && is_chinese_query(&query)
        {
            tokio::time::timeout(
                Duration::from_secs(8),
                chinese_mod_search(&query, category, settings),
            )
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default()
        } else {
            Vec::new()
        }
    };

    let (modrinth_page, cf_page, zh_items) = tokio::join!(modrinth_fut, cf_fut, zh_fut);

    let mut items = modrinth_page.items;
    items.extend(cf_page.items);
    items.extend(zh_items);

    items = merge_and_rank_items(items, &query, sort, browse_mode);
    crate::commands::market_extra::annotate_modpack_badges(category, &mut items);

    let modrinth_has_more = should_search_modrinth
        && provider_has_more(
            modrinth_page.page_count,
            PAGE_SIZE,
            page,
            modrinth_page.total_hits,
        );
    let cf_has_more = should_search_cf
        && provider_has_more(cf_page.page_count, PAGE_SIZE, page, cf_page.total_hits);
    let has_more = modrinth_has_more || cf_has_more;

    let total_hits = if should_search_modrinth && !should_search_cf {
        modrinth_page.total_hits.max(items.len() as u32)
    } else if should_search_cf && !should_search_modrinth {
        cf_page.total_hits.max(items.len() as u32)
    } else if should_search_modrinth && should_search_cf {
        modrinth_page
            .total_hits
            .saturating_add(cf_page.total_hits)
            .max(items.len() as u32)
    } else {
        items.len() as u32
    };

    Ok(MarketSearchResponse {
        items,
        page,
        page_size: PAGE_SIZE,
        total_hits,
        has_more,
    })
}

fn dedupe_search_items(items: Vec<MarketSearchItem>) -> Vec<MarketSearchItem> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| {
            let key = format!("{:?}:{}", item.source, item.id.to_lowercase());
            seen.insert(key)
        })
        .collect()
}

async fn search_modrinth(
    client: &Client,
    endpoints: &ModrinthEndpoints,
    query: &str,
    project_type: &str,
    mc_version: &str,
    loader: &str,
    category: MarketCategory,
    offset: u32,
    limit: u32,
    sort: MarketSort,
    relax_filters: bool,
) -> anyhow::Result<MarketSearchPage> {
    let hits = modrinth_search_once(
        client,
        endpoints,
        query,
        project_type,
        mc_version,
        loader,
        category,
        offset,
        limit,
        sort,
        true,
    )
    .await?;
    if hits.items.is_empty() && relax_filters && category != MarketCategory::Litematic {
        return modrinth_search_once(
            client,
            endpoints,
            query,
            project_type,
            mc_version,
            loader,
            category,
            offset,
            limit,
            sort,
            false,
        )
        .await;
    }
    Ok(hits)
}

async fn modrinth_search_once(
    client: &Client,
    endpoints: &ModrinthEndpoints,
    query: &str,
    project_type: &str,
    mc_version: &str,
    loader: &str,
    category: MarketCategory,
    offset: u32,
    limit: u32,
    sort: MarketSort,
    with_facets: bool,
) -> anyhow::Result<MarketSearchPage> {
    let mut facet_parts = vec![format!(r#"["project_type:{project_type}"]"#)];
    if with_facets {
        let mc = normalize_mc_version(mc_version);
        if !mc.is_empty() && mc != "unknown" {
            facet_parts.push(format!(r#"["versions:{mc}"]"#));
        }
        if category == MarketCategory::Mod {
            for tag in loader_query_tags(loader) {
                facet_parts.push(format!(r#"["categories:{tag}"]"#));
            }
        }
    }
    let facets = format!("[{}]", facet_parts.join(","));

    let resp: ModrinthSearchResponse = client
        .get(format!("{}/search", endpoints.api_base))
        .query(&[
            ("query", query),
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
            ("index", modrinth_index(sort)),
            ("facets", facets.as_str()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(MarketSearchPage {
        page_count: resp.hits.len() as u32,
        total_hits: resp.total_hits,
        items: resp
            .hits
            .into_iter()
            .map(|h| MarketSearchItem {
                id: h.project_id,
                slug: h.slug,
                title: h.title,
                description: h.description,
                icon_url: h.icon_url,
                downloads: h.downloads,
                source: ModSource::Modrinth,
                install_status: None,
                installed_version: None,
                modpack_badge: None,
            })
            .collect(),
    })
}

fn cf_loader_type_id(loader: &str) -> i32 {
    match loader.to_lowercase().as_str() {
        "fabric" => 4,
        "quilt" => 5,
        "neoforge" => 6,
        _ => 1,
    }
}

async fn search_curseforge(
    client: &Client,
    endpoints: &CurseForgeEndpoints,
    api_key: &str,
    query: &str,
    class_id: i64,
    page: u32,
    page_size: u32,
    sort: MarketSort,
    category: MarketCategory,
    mc_version: &str,
    loader: &str,
    compatible_only: bool,
) -> anyhow::Result<MarketSearchPage> {
    if endpoints.needs_api_key && api_key.is_empty() {
        return Ok(MarketSearchPage::empty());
    }

    let mut req = client.get(format!("{}/mods/search", endpoints.api_base));
    if !api_key.is_empty() {
        req = req.header("x-api-key", api_key);
    }

    let mut query_params = vec![
        ("gameId", MINECRAFT_GAME_ID.to_string()),
        ("classId", class_id.to_string()),
        ("searchFilter", query.to_string()),
        ("pageSize", page_size.to_string()),
        ("index", page.to_string()),
        ("sortField", cf_sort_field(sort).to_string()),
        ("sortOrder", "desc".to_string()),
    ];
    if compatible_only && category == MarketCategory::Mod {
        let mc = normalize_mc_version(mc_version);
        if !mc.is_empty() && mc != "unknown" {
            query_params.push(("gameVersion", mc));
        }
        if !loader.trim().is_empty() {
            query_params.push(("modLoaderType", cf_loader_type_id(loader).to_string()));
        }
    }

    let resp: CfSearchResponse = req
        .query(&query_params)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(MarketSearchPage {
        page_count: resp.data.len() as u32,
        total_hits: resp.pagination.total_count,
        items: resp
            .data
            .into_iter()
            .map(|h| MarketSearchItem {
                id: h.id.to_string(),
                slug: h.slug,
                title: h.name,
                description: h.summary,
                icon_url: h.logo.map(|l| l.url),
                downloads: h.download_count as u64,
                source: ModSource::Curseforge,
                install_status: None,
                installed_version: None,
                modpack_badge: None,
            })
            .collect(),
    })
}

pub async fn market_lookup_by_name(
    category: MarketCategory,
    file_name: String,
    mc_version: String,
    loader: String,
    settings: &AppSettings,
) -> anyhow::Result<Option<MarketSearchItem>> {
    match category {
        MarketCategory::ShaderPack | MarketCategory::ResourcePack => {
            let kind = if category == MarketCategory::ShaderPack {
                PackKind::Shader
            } else {
                PackKind::ResourcePack
            };
            let mc = normalize_mc_version(&mc_version);
            if let Some(hit) = lookup_online_pack(&file_name, kind, &mc, settings).await {
                let stem = file_name
                    .trim_end_matches(".zip")
                    .trim_end_matches(".rar");
                return Ok(Some(MarketSearchItem {
                    id: stem.to_string(),
                    slug: stem.to_string(),
                    title: stem.to_string(),
                    description: format!("在线版本 {}", hit.version),
                    icon_url: None,
                    downloads: 0,
                    source: hit.source,
                    install_status: None,
                    installed_version: None,
                    modpack_badge: None,
                }));
            }
            Ok(None)
        }
        _ => {
            let resp = market_search(
                category,
                file_name.clone(),
                mc_version,
                loader,
                0,
                MarketSourceFilter::All,
                MarketSort::Relevance,
                true,
                false,
                settings,
            )
            .await?;
            Ok(resp.items.into_iter().next())
        }
    }
}

pub async fn market_list_versions(
    category: MarketCategory,
    source: ModSource,
    project_id: String,
    target: TargetEnv,
    settings: &AppSettings,
    version_cache: &ProjectVersionCache,
    expand: bool,
) -> anyhow::Result<Vec<ModVersionOption>> {
    let skip_loader_filter = !category_uses_loader_filter(category);
    let mod_info = identified_from_market(&source, &project_id);
    let target_env = resolve_target_env(enrich_target_env(target), &[]);
    let compat = CompatTarget::from_target(&target_env);
    let policy = ModVersionPolicy::from_setting(&settings.mod_version_policy);

    match source {
        ModSource::Modrinth => {
            list_modrinth_versions(
                settings,
                &mod_info,
                &compat,
                Some(version_cache),
                policy,
                expand,
                skip_loader_filter,
            )
            .await
        }
        ModSource::Curseforge => {
            list_curseforge_versions(
                settings,
                &mod_info,
                &compat,
                policy,
                expand,
                skip_loader_filter,
            )
            .await
        }
        ModSource::Sgu if category == MarketCategory::Litematic => {
            sgu_litematic::list_litematic_versions(&project_id).await
        }
        _ => Ok(vec![]),
    }
}

pub async fn market_install_batch(
    app: Option<AppHandle>,
    mut jobs: Vec<MarketInstallJob>,
    target: InstanceInfo,
    resolve_deps: bool,
    settings: &AppSettings,
    cancel: &CancelToken,
    version_cache: Option<&ProjectVersionCache>,
    data_dir: Option<&Path>,
) -> anyhow::Result<MarketInstallBatchResult> {
    let mut warnings = Vec::new();

    if resolve_deps {
        if let Ok(extra_warnings) =
            expand_jobs_with_deps(&mut jobs, &target, settings, version_cache, cancel, app.as_ref())
                .await
        {
            warnings.extend(extra_warnings);
        }
    }

    let total = jobs.len().max(1) as u32;
    let progress = app
        .as_ref()
        .map(|a| MonotonicEmitter::new(a.clone(), "market-install-progress", total));

    let client = build_http_client(crate::http::APP_USER_AGENT);
    let ctx = MarketInstallContext {
        target: &target,
        settings,
        cancel,
        client: &client,
    };

    let record_id = market_undo::new_record_id();
    let mut session = MarketInstallSession::new(record_id.clone(), data_dir, &target.name)?;
    let mut results = Vec::new();
    let summary_names: Vec<String> = jobs.iter().map(|j| j.file_name.clone()).collect();
    let summary = summary_names.join(", ");
    let mut install_error: Option<anyhow::Error> = None;

    for job in &jobs {
        cancel.ensure_running()?;
        if let Some(ref p) = progress {
            p.step(&job.file_name, |cur, tot| format!("市场安装 ({cur}/{tot})"))
                .await;
        }

        match install_job(job, &ctx, &mut session, progress.as_ref()).await {
            Ok(result) => results.push(result),
            Err(e) => {
                install_error = Some(e);
                break;
            }
        }
    }

    let completed_summary = if results.is_empty() {
        summary
    } else {
        results.iter().map(|r| r.file_name.clone()).collect::<Vec<_>>().join(", ")
    };

    let mut final_record_id = record_id;
    if let Some(record) = session.finalize(&target.name, &completed_summary)? {
        for r in &mut results {
            r.record_id = Some(record.id.clone());
        }
        final_record_id = record.id;
    }

    if let Some(e) = install_error {
        if results.is_empty() {
            return Err(e);
        }
        warnings.push(format!(
            "安装未完成（已完成 {} 项，可在「最近安装」中撤销）: {}",
            results.len(),
            e
        ));
    }

    Ok(MarketInstallBatchResult {
        results,
        warnings,
        record_id: final_record_id,
    })
}

async fn expand_jobs_with_deps(
    jobs: &mut Vec<MarketInstallJob>,
    target: &InstanceInfo,
    settings: &AppSettings,
    version_cache: Option<&ProjectVersionCache>,
    cancel: &CancelToken,
    app: Option<&AppHandle>,
) -> anyhow::Result<Vec<String>> {
    let target_env = resolve_target_env(
        TargetEnv {
            mods_path: target.mods_path.clone(),
            mc_version: target.mc_version.clone(),
            loader: target.loader.clone(),
            loader_version: target.loader_version.clone(),
        },
        &[],
    );

    let mut mod_items: Vec<ModTransferItem> = jobs
        .iter()
        .filter(|j| j.category == MarketCategory::Mod && !j.is_dependency)
        .map(job_to_transfer_item)
        .collect();

    if mod_items.is_empty() {
        return Ok(vec![]);
    }

    let warnings = match resolve_for_transfer(
        app,
        &mut mod_items,
        &[],
        &target_env,
        settings,
        version_cache,
        cancel,
    )
    .await
    {
        Ok(()) => vec![],
        Err(e) => vec![e.to_string()],
    };

    let sorted = sort_download_order(mod_items);
    let main_urls: std::collections::HashSet<String> = jobs
        .iter()
        .map(|j| j.download_url.clone())
        .collect();

    let mut dep_jobs = Vec::new();
    for item in sorted {
        if item.is_dependency {
            if let Some(url) = item.download_url.clone() {
                if !main_urls.contains(&url) {
                    dep_jobs.push(MarketInstallJob {
                        category: MarketCategory::Mod,
                        download_url: url,
                        file_name: item
                            .target_file_name
                            .unwrap_or_else(|| item.mod_info.file_name.clone()),
                        is_dependency: true,
                        project_id: item.mod_info.project_id.clone(),
                        source: Some(item.mod_info.source.clone()),
                        mod_name: Some(item.mod_info.name.clone()),
                    });
                }
            }
        }
    }

    jobs.splice(0..0, dep_jobs);
    Ok(warnings)
}

fn job_to_transfer_item(job: &MarketInstallJob) -> ModTransferItem {
    let mod_info = if let (Some(source), Some(project_id)) = (&job.source, &job.project_id) {
        identified_from_market(source, project_id)
    } else {
        IdentifiedMod {
            file_name: job.file_name.clone(),
            file_path: String::new(),
            sha512: String::new(),
            sha1: String::new(),
            fingerprint: 0,
            source: ModSource::Unknown,
            project_id: None,
            curseforge_id: None,
            name: job.mod_name.clone().unwrap_or_else(|| job.file_name.clone()),
            name_zh: None,
            mod_id: None,
            current_version: None,
            loaders: vec![],
            game_versions: vec![],
            icon_url: None,
            github_url: None,
            depends: vec![],
        }
    };

    ModTransferItem {
        mod_info,
        status: TransferStatus::Transferable,
        target_file_name: Some(job.file_name.clone()),
        target_version: None,
        download_url: Some(job.download_url.clone()),
        download_source: job.source.clone(),
        selected: true,
        is_dependency: job.is_dependency,
        required_by: None,
    }
}

async fn install_job(
    job: &MarketInstallJob,
    ctx: &MarketInstallContext<'_>,
    session: &mut MarketInstallSession,
    progress: Option<&Arc<MonotonicEmitter>>,
) -> anyhow::Result<MarketInstallResult> {
    match job.category {
        MarketCategory::Modpack => {
            let p = progress.cloned();
            modpack::install_modpack(ctx, &job.download_url, &job.file_name, session, p.as_ref()).await
        }
        MarketCategory::Datapack => install_datapack(ctx, job, session, progress).await,
        MarketCategory::Litematic => install_litematic(ctx, job, session, progress).await,
        _ => install_file(ctx, job, job.category, &job.file_name, session, progress).await,
    }
}

async fn install_file(
    ctx: &MarketInstallContext<'_>,
    job: &MarketInstallJob,
    category: MarketCategory,
    file_name: &str,
    session: &mut MarketInstallSession,
    progress: Option<&Arc<MonotonicEmitter>>,
) -> anyhow::Result<MarketInstallResult> {
    let dest_dir = install_dest_dir(category, ctx.target);
    std::fs::create_dir_all(&dest_dir)?;

    let safe_name = sanitize_file_name(file_name);
    let dest = dest_dir.join(&safe_name);

    if dest.is_file() || dest.is_dir() {
        session.track_overwrite(dest.clone())?;
    }

    if let Some(p) = progress {
        p.emit_status(&safe_name, "下载中…").await;
    }

    let url = resolve_market_download_url(job, ctx.settings).await?;
    download_zip_file_validated(ctx.client, &url, &dest, ctx.cancel).await?;

    session.track_created(dest.clone());

    Ok(MarketInstallResult {
        file_path: dest.to_string_lossy().to_string(),
        file_name: safe_name,
        needs_launcher_import: false,
        hint: String::new(),
        record_id: None,
        installed_files: vec![dest.to_string_lossy().replace('\\', "/")],
    })
}

async fn install_datapack(
    ctx: &MarketInstallContext<'_>,
    job: &MarketInstallJob,
    session: &mut MarketInstallSession,
    progress: Option<&Arc<MonotonicEmitter>>,
) -> anyhow::Result<MarketInstallResult> {
    let paths = resolve_instance_paths(ctx.target);
    std::fs::create_dir_all(&paths.datapacks)?;

    let safe_name = sanitize_file_name(&job.file_name);
    let dest = paths.datapacks.join(&safe_name);

    if dest.is_file() || dest.is_dir() {
        session.track_overwrite(dest.clone())?;
    }

    if let Some(p) = progress {
        p.emit_status(&safe_name, "下载数据包…").await;
    }

    let url = resolve_market_download_url(job, ctx.settings).await?;
    download_zip_file_validated(ctx.client, &url, &dest, ctx.cancel).await?;

    session.track_created(dest.clone());

    Ok(MarketInstallResult {
        file_path: dest.to_string_lossy().to_string(),
        file_name: safe_name,
        needs_launcher_import: false,
        hint: String::new(),
        record_id: None,
        installed_files: vec![dest.to_string_lossy().replace('\\', "/")],
    })
}

async fn install_litematic(
    ctx: &MarketInstallContext<'_>,
    job: &MarketInstallJob,
    session: &mut MarketInstallSession,
    progress: Option<&Arc<MonotonicEmitter>>,
) -> anyhow::Result<MarketInstallResult> {
    let dest_dir = resolve_schematic_roots(ctx.target)
        .into_iter()
        .next()
        .unwrap_or_else(|| resolve_instance_paths(ctx.target).schematics);
    std::fs::create_dir_all(&dest_dir)?;

    let safe_name = sanitize_file_name(&job.file_name);
    let dest = dest_dir.join(&safe_name);

    if dest.is_file() || dest.is_dir() {
        session.track_overwrite(dest.clone())?;
    }

    if let Some(p) = progress {
        p.emit_status(&safe_name, "下载投影…").await;
    }

    let url = rewrite_download_url(&job.download_url, ctx.settings);
    download_raw_file(ctx.client, &url, &dest, ctx.cancel).await?;

    session.track_created(dest.clone());

    Ok(MarketInstallResult {
        file_path: dest.to_string_lossy().to_string(),
        file_name: safe_name,
        needs_launcher_import: false,
        hint: String::new(),
        record_id: None,
        installed_files: vec![dest.to_string_lossy().replace('\\', "/")],
    })
}

fn install_dest_dir(category: MarketCategory, instance: &InstanceInfo) -> PathBuf {
    let paths = resolve_instance_paths(instance);
    match category {
        MarketCategory::ShaderPack => paths.shaderpacks,
        MarketCategory::ResourcePack => paths.resourcepacks,
        MarketCategory::Mod => PathBuf::from(&instance.mods_path),
        MarketCategory::Modpack => paths.game_dir,
        MarketCategory::Datapack => paths.datapacks,
        MarketCategory::Litematic => paths.schematics,
    }
}

fn rewrite_download_url(url: &str, settings: &AppSettings) -> String {
    let mut out = url.to_string();
    for endpoints in mirrors_with_official_fallback(&settings.mod_api_mirror) {
        out = endpoints.rewrite_download_url(&out);
    }
    out = rewrite_cf_download_url(&out, settings);
    out
}

async fn resolve_market_download_url(
    job: &MarketInstallJob,
    settings: &AppSettings,
) -> anyhow::Result<String> {
    if job.source.as_ref() == Some(&ModSource::Curseforge) {
        let mod_id = job
            .project_id
            .as_ref()
            .and_then(|id| id.parse::<i64>().ok())
            .or_else(|| parse_cf_mod_file_ids(&job.download_url).map(|(id, _)| id));
        let file_id = parse_cf_mod_file_ids(&job.download_url).map(|(_, id)| id);
        if let (Some(mod_id), Some(file_id)) = (mod_id, file_id) {
            if let Ok(url) =
                resolve_cf_download_url(settings, mod_id, file_id, &job.download_url).await
            {
                return Ok(url);
            }
        }
    }
    Ok(rewrite_download_url(&job.download_url, settings))
}

pub fn identified_from_market(source: &ModSource, project_id: &str) -> IdentifiedMod {
    match source {
        ModSource::Modrinth => IdentifiedMod {
            file_name: String::new(),
            file_path: String::new(),
            sha512: String::new(),
            sha1: String::new(),
            fingerprint: 0,
            source: ModSource::Modrinth,
            project_id: Some(project_id.to_string()),
            curseforge_id: None,
            name: String::new(),
            name_zh: None,
            mod_id: None,
            current_version: None,
            loaders: vec![],
            game_versions: vec![],
            icon_url: None,
            github_url: None,
            depends: vec![],
        },
        ModSource::Curseforge => {
            let cf_id = project_id.parse().ok();
            IdentifiedMod {
                file_name: String::new(),
                file_path: String::new(),
                sha512: String::new(),
                sha1: String::new(),
                fingerprint: 0,
                source: ModSource::Curseforge,
                project_id: None,
                curseforge_id: cf_id,
                name: String::new(),
                name_zh: None,
                mod_id: None,
                current_version: None,
                loaders: vec![],
                game_versions: vec![],
                icon_url: None,
                github_url: None,
                depends: vec![],
            }
        }
        _ => IdentifiedMod {
            file_name: String::new(),
            file_path: String::new(),
            sha512: String::new(),
            sha1: String::new(),
            fingerprint: 0,
            source: source.clone(),
            project_id: None,
            curseforge_id: None,
            name: String::new(),
            name_zh: None,
            mod_id: None,
            current_version: None,
            loaders: vec![],
            game_versions: vec![],
            icon_url: None,
            github_url: None,
            depends: vec![],
        },
    }
}

fn sanitize_file_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "download.zip".to_string();
    }
    let mut out = String::new();
    for ch in trimmed.chars() {
        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct ModrinthSearchResponse {
    hits: Vec<ModrinthSearchHit>,
    #[serde(default)]
    total_hits: u32,
}

#[derive(Debug, Deserialize)]
struct ModrinthSearchHit {
    project_id: String,
    slug: String,
    title: String,
    description: String,
    downloads: u64,
    icon_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CfSearchResponse {
    data: Vec<CfSearchHit>,
    #[serde(default)]
    pagination: CfPagination,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CfPagination {
    #[serde(default)]
    total_count: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfSearchHit {
    id: i64,
    slug: String,
    name: String,
    summary: String,
    download_count: u32,
    logo: Option<CfLogo>,
}

#[derive(Debug, Deserialize)]
struct CfLogo {
    url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_file_name_strips_invalid_chars() {
        assert_eq!(sanitize_file_name("foo/bar.zip"), "foo_bar.zip");
        assert_eq!(sanitize_file_name(""), "download.zip");
    }

    #[test]
    fn category_params_maps_modpack_and_datapack() {
        let p = category_params(MarketCategory::Modpack);
        assert_eq!(p.modrinth_type, "modpack");
        assert_eq!(p.cf_class_id, 4471);
        let d = category_params(MarketCategory::Datapack);
        assert_eq!(d.modrinth_type, "datapack");
        assert_eq!(d.cf_class_id, 6945);
    }

    #[tokio::test]
    async fn market_search_resource_pack_hits_modrinth() {
        let settings = AppSettings::default();
        let resp = market_search(
            MarketCategory::ResourcePack,
            "faithful".into(),
            "1.21.1".into(),
            "fabric".into(),
            0,
            MarketSourceFilter::Modrinth,
            MarketSort::Relevance,
            true,
            false,
            &settings,
        )
        .await
        .expect("search should succeed");
        assert!(!resp.items.is_empty(), "expected Modrinth resource pack hits");
        assert!(
            resp.items.iter().all(|i| i.source == ModSource::Modrinth),
            "all hits should be Modrinth when filtered"
        );
    }

    #[tokio::test]
    async fn market_search_shader_hits_modrinth() {
        let settings = AppSettings::default();
        let resp = market_search(
            MarketCategory::ShaderPack,
            "complementary".into(),
            "1.21.1".into(),
            "iris".into(),
            0,
            MarketSourceFilter::All,
            MarketSort::Relevance,
            false,
            false,
            &settings,
        )
        .await
        .expect("search should succeed");
        assert!(!resp.items.is_empty(), "expected at least one shader hit");
        assert!(resp.items.iter().any(|i| i.source == ModSource::Modrinth));
    }
}
