use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::cancellation::CancelToken;
use crate::compat::{
    game_version_query_tags, pick_best_scored, score_release, CompatTarget, ModVersionPolicy,
    PickContext, ReleaseChannel,
};
use crate::http::build_http_client;
use crate::models::{AppSettings, FileHash, IdentifiedMod, ModFile, ModSource, ModVersionOption};
use crate::providers::endpoints::{mirrors_for_setting, mirrors_with_official_fallback, mirrors_with_official_first, ModrinthEndpoints};
use crate::version::{
    effective_game_versions, loader_query_tags, mod_version_at_most, normalize_mc_version,
    parse_mod_version_label, release_supports_target_mc,
};

#[derive(Clone)]
pub struct ProjectVersionCache {
    inner: Arc<VersionCacheInner>,
}

struct VersionCacheInner {
    data: Mutex<HashMap<String, Vec<ModrinthVersionFile>>>,
    inflight: Mutex<HashMap<String, Arc<tokio::sync::Notify>>>,
}

impl ProjectVersionCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(VersionCacheInner {
                data: Mutex::new(HashMap::new()),
                inflight: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub async fn get(&self, key: &str) -> Option<Vec<ModrinthVersionFile>> {
        self.inner.data.lock().await.get(key).cloned()
    }

    /// Deduplicate concurrent fetches for the same project + target + mirror.
    pub async fn get_or_fetch<F, Fut>(&self, key: String, fetch: F) -> anyhow::Result<Vec<ModrinthVersionFile>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<Vec<ModrinthVersionFile>>>,
    {
        loop {
            if let Some(cached) = self.inner.data.lock().await.get(&key).cloned() {
                return Ok(cached);
            }

            let waiter = {
                let mut inflight = self.inner.inflight.lock().await;
                if let Some(n) = inflight.get(&key) {
                    Some(n.clone())
                } else {
                    inflight.insert(key.clone(), Arc::new(tokio::sync::Notify::new()));
                    None
                }
            };

            if let Some(notify) = waiter {
                notify.notified().await;
                continue;
            }

            let result = fetch().await;
            {
                let mut data = self.inner.data.lock().await;
                if let Ok(ref versions) = result {
                    if !versions.is_empty() {
                        data.entry(key.clone()).or_insert_with(|| versions.clone());
                    }
                }
                if let Some(notify) = self.inner.inflight.lock().await.remove(&key) {
                    notify.notify_waiters();
                }
            }
            return result;
        }
    }
}

pub fn new_version_cache() -> ProjectVersionCache {
    ProjectVersionCache::new()
}

/// Per-mirror version list cache (filtering is client-side).
fn version_list_cache_key(project_id: &str, mirror: &str) -> String {
    format!("{mirror}:{project_id}:versions:v6")
}

fn version_picker_cache_key(project_id: &str, target: &CompatTarget) -> String {
    format!("{project_id}:picker:{}|{}:v1", target.loader, target.mc_version)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionFetchPolicy {
    /// Only use prefetched / in-memory lists; never hit the network.
    CacheOnly,
    AllowFetch,
}

pub struct ModrinthProvider {
    client: Client,
    endpoints: ModrinthEndpoints,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModrinthDependency {
    pub project_id: Option<String>,
    pub dependency_type: String,
}

#[derive(Debug, Clone)]
pub struct ModrinthProjectInfo {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub downloads: u64,
    pub icon_url: Option<String>,
}

impl ModrinthProvider {
    pub fn new() -> Self {
        Self::with_endpoints(ModrinthEndpoints::official())
    }

    pub fn with_endpoints(endpoints: ModrinthEndpoints) -> Self {
        Self {
            client: build_http_client(crate::http::APP_USER_AGENT),
            endpoints,
        }
    }

    pub fn for_settings(settings: &AppSettings) -> Self {
        mirrors_for_setting(&settings.mod_api_mirror)
            .into_iter()
            .next()
            .map(Self::with_endpoints)
            .unwrap_or_else(Self::new)
    }

    pub fn endpoints(&self) -> &ModrinthEndpoints {
        &self.endpoints
    }

    fn api(&self) -> &str {
        &self.endpoints.api_base
    }

    pub async fn project_id_from_hash(&self, sha512: &str) -> anyhow::Result<Option<String>> {
        let map = self.post_version_files_by_hashes(&[sha512], "sha512").await?;
        Ok(map.get(sha512).map(|v| v.project_id.clone()))
    }

    async fn project_id_from_file_hashes(&self, sha512: &str, sha1: &str) -> Option<String> {
        if !sha512.is_empty() {
            if let Ok(map) = self.post_version_files_by_hashes(&[sha512], "sha512").await {
                if let Some(vf) = map.get(sha512) {
                    return Some(vf.project_id.clone());
                }
            }
        }
        if !sha1.is_empty() {
            if let Ok(map) = self.post_version_files_by_hashes(&[sha1], "sha1").await {
                if let Some(vf) = map.get(sha1) {
                    return Some(vf.project_id.clone());
                }
            }
        }
        None
    }

    async fn post_version_files_by_hashes(
        &self,
        hashes: &[&str],
        algorithm: &str,
    ) -> anyhow::Result<HashMap<String, ModrinthVersionFile>> {
        let resp = self
            .client
            .post(format!("{}/version_files", self.api()))
            .json(&serde_json::json!({
                "hashes": hashes,
                "algorithm": algorithm
            }))
            .send()
            .await?
            .error_for_status()?;
        let body = resp.bytes().await?;
        parse_version_files_response(&body).map_err(|e| {
            anyhow::anyhow!("POST {}/version_files: {e}", self.api())
        })
    }

    /// PCL-style server-side update match: `POST /version_files/update`.
    async fn post_version_files_update(
        &self,
        hashes: &[&str],
        algorithm: &str,
        loaders: &[String],
        game_versions: &[String],
    ) -> anyhow::Result<HashMap<String, ModrinthVersionFile>> {
        if hashes.is_empty() {
            return Ok(HashMap::new());
        }
        let resp = self
            .client
            .post(format!("{}/version_files/update", self.api()))
            .json(&serde_json::json!({
                "hashes": hashes,
                "algorithm": algorithm,
                "loaders": loaders,
                "game_versions": game_versions,
            }))
            .send()
            .await?;
        let status = resp.status();
        let body = resp.bytes().await?;
        if !status.is_success() {
            if matches!(status.as_u16(), 400 | 404 | 502 | 503) {
                return Ok(HashMap::new());
            }
            return Err(anyhow::anyhow!(
                "POST {}/version_files/update returned {status}: {}",
                self.api(),
                String::from_utf8_lossy(&body[..body.len().min(240)])
            ));
        }
        parse_version_files_response(&body).map_err(|e| {
            anyhow::anyhow!("POST {}/version_files/update: {e}", self.api())
        })
    }

    /// Resolve compatible file via Modrinth update API (sha512 first, then sha1).
    pub async fn get_compatible_version_via_update(
        &self,
        sha512: &str,
        sha1: &str,
        target: &CompatTarget,
        source_game_versions: &[String],
        source_mod_version: Option<&str>,
        version_policy: ModVersionPolicy,
    ) -> anyhow::Result<Option<ModFile>> {
        let loaders = loader_query_tags(&target.loader);
        let game_versions = if target.mc_known() {
            vec![target.mc_version.clone()]
        } else {
            Vec::new()
        };

        let attempts: [(&str, &str); 2] = [
            (sha512, "sha512"),
            (sha1, "sha1"),
        ];
        for (hash, algorithm) in attempts {
            if hash.is_empty() {
                continue;
            }
            let map = self
                .post_version_files_update(&[hash], algorithm, &loaders, &game_versions)
                .await?;
            if let Some(vf) = map.get(hash) {
                if !update_result_matches_target(
                    vf,
                    target,
                    source_game_versions,
                    source_mod_version,
                    version_policy,
                ) {
                    continue;
                }
                return Ok(Some(version_file_to_mod_file(vf, &self.endpoints)));
            }
        }
        Ok(None)
    }

    pub async fn identify_by_hashes(
        &self,
        hashes: &[FileHash],
    ) -> anyhow::Result<HashMap<String, IdentifiedMod>> {
        if hashes.is_empty() {
            return Ok(HashMap::new());
        }

        let sha512_list: Vec<&str> = hashes.iter().map(|h| h.sha512.as_str()).collect();

        let resp = self.fetch_version_files_by_hashes(&sha512_list).await?;

        let project_ids: HashSet<String> = resp.values().map(|v| v.project_id.clone()).collect();
        let project_meta = self.fetch_projects_parallel(&project_ids, 8).await;

        let mut results = HashMap::new();
        for hash in hashes {
            if let Some(vf) = resp.get(&hash.sha512) {
                let project_id = vf.project_id.clone();
                let (name, icon_url) = project_meta
                    .get(&project_id)
                    .cloned()
                    .unwrap_or_else(|| (vf.filename.clone(), None));
                let depends = vf
                    .dependencies
                    .iter()
                    .filter(|d| d.dependency_type == "required")
                    .filter_map(|d| d.project_id.clone())
                    .collect();

                results.insert(
                    hash.sha512.clone(),
                    IdentifiedMod {
                        file_name: hash.file_name.clone(),
                        file_path: hash.path.clone(),
                        sha512: hash.sha512.clone(),
                        sha1: hash.sha1.clone(),
                        fingerprint: hash.fingerprint,
                        source: ModSource::Modrinth,
                        project_id: Some(project_id),
                        curseforge_id: None,
                        name,
                        name_zh: None,
                        mod_id: None,
                        current_version: Some(vf.version_number.clone()),
                        loaders: vf.loaders.clone(),
                        game_versions: vf.game_versions.clone(),
                        icon_url,
                        github_url: None,
                        depends,
                    },
                );
            }
        }
        Ok(results)
    }

    pub async fn get_version_dependencies_by_hash(
        &self,
        sha512: &str,
    ) -> anyhow::Result<Vec<ModrinthDependency>> {
        let resp = self.post_version_files_by_hashes(&[sha512], "sha512").await?;
        Ok(resp
            .get(sha512)
            .map(|v| v.dependencies.clone())
            .unwrap_or_default())
    }

    pub async fn get_project_version_dependencies(
        &self,
        project_id: &str,
        target: &CompatTarget,
    ) -> anyhow::Result<Vec<ModrinthDependency>> {
        let versions = self.fetch_project_versions(project_id).await?;

        Ok(pick_modrinth_version(&versions, target, &[], None, ModVersionPolicy::Auto)
            .map(|v| v.dependencies.clone())
            .unwrap_or_default())
    }

    pub async fn resolve_project_id(&self, slug_or_id: &str) -> anyhow::Result<Option<String>> {
        if let Ok(Some(info)) = self.get_project_info(slug_or_id).await {
            return Ok(Some(info.id));
        }

        let hits: ModrinthSearchResponse = self
            .client
            .get(format!("{}/search", self.api()))
            .query(&[
                ("query", slug_or_id),
                ("limit", "5"),
                ("index", "relevance"),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let hits = hits.hits;
        let lower = slug_or_id.to_lowercase();
        for hit in &hits {
            if hit.project_id.eq_ignore_ascii_case(slug_or_id)
                || hit.slug.eq_ignore_ascii_case(slug_or_id)
                || hit.slug.to_lowercase() == lower
            {
                return Ok(Some(hit.project_id.clone()));
            }
        }

        Ok(hits.into_iter().next().map(|h| h.project_id))
    }

    pub async fn get_project_info(
        &self,
        slug_or_id: &str,
    ) -> anyhow::Result<Option<ModrinthProjectInfo>> {
        let resp = self
            .client
            .get(format!("{}/project/{slug_or_id}", self.api()))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let body = resp.bytes().await?;
        let p: ModrinthProjectDetail = serde_json::from_slice(&body).map_err(|e| {
            anyhow::anyhow!(
                "GET {}/project/{slug_or_id}: {e}; body: {}",
                self.api(),
                String::from_utf8_lossy(&body[..body.len().min(240)])
            )
        })?;
        Ok(Some(ModrinthProjectInfo {
            id: p.id,
            slug: p.slug,
            title: p.title,
            description: p.description,
            downloads: p.downloads,
            icon_url: p.icon_url,
        }))
    }

    pub async fn get_project_by_slug_or_id(
        &self,
        slug_or_id: &str,
    ) -> anyhow::Result<Option<ModrinthProjectInfo>> {
        self.get_project_info(slug_or_id).await
    }

    /// Find shader/resource pack version for target MC (loader-agnostic).
    pub async fn find_pack_for_mc(
        &self,
        query: &str,
        project_type: &str,
        target_mc: &str,
    ) -> anyhow::Result<Option<ModFile>> {
        let facets = format!(r#"[["project_type:{project_type}"]]"#);
        let hits: ModrinthSearchResponse = self
            .client
            .get(format!("{}/search", self.api()))
            .query(&[
                ("query", query),
                ("limit", "5"),
                ("index", "relevance"),
                ("facets", facets.as_str()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let project_id = {
            let lower = query.to_lowercase();
            hits.hits
                .iter()
                .find(|h| h.slug.eq_ignore_ascii_case(query) || h.slug.to_lowercase() == lower)
                .or(hits.hits.first())
                .map(|h| h.project_id.clone())
        };

        let Some(project_id) = project_id else {
            return Ok(None);
        };

        let mc = normalize_mc_version(target_mc);
        let versions = self
            .fetch_version_page(&project_id, 0, None, Some(mc.as_str()))
            .await?;
        for vf in &versions {
            if release_supports_target_mc(&vf.game_versions, &mc) {
                return Ok(Some(version_file_to_mod_file(vf, &self.endpoints)));
            }
        }
        Ok(None)
    }

    /// Hash lookup → project list pick. Update API is handled by `find_compatible_on_provider`.
    pub async fn get_compatible_version(
        &self,
        sha512: &str,
        sha1: &str,
        target: &CompatTarget,
        source_game_versions: &[String],
        version_cache: Option<&ProjectVersionCache>,
        source_mod_version: Option<&str>,
        version_policy: ModVersionPolicy,
    ) -> anyhow::Result<Option<ModFile>> {
        let Some(project_id) = self.project_id_from_file_hashes(sha512, sha1).await else {
            return Ok(None);
        };
        self.get_project_version_flexible_cached(
            &project_id,
            target,
            source_game_versions,
            version_cache,
            source_mod_version,
            VersionFetchPolicy::AllowFetch,
            version_policy,
        )
        .await
    }

    pub async fn get_project_version_flexible_cached(
        &self,
        project_id: &str,
        target: &CompatTarget,
        source_game_versions: &[String],
        cache: Option<&ProjectVersionCache>,
        source_mod_version: Option<&str>,
        fetch_policy: VersionFetchPolicy,
        version_policy: ModVersionPolicy,
    ) -> anyhow::Result<Option<ModFile>> {
        let picker_key = version_picker_cache_key(project_id, target);
        let list_key = version_list_cache_key(project_id, self.api());
        let source_gvs = source_game_versions.to_vec();

        let versions = if let Some(cache) = cache {
            if let Some(cached) = cache.get(&picker_key).await {
                cached
            } else if let Some(cached) = cache.get(&list_key).await {
                cached
            } else if fetch_policy == VersionFetchPolicy::AllowFetch {
                cache
                    .get_or_fetch(picker_key, || {
                        self.fetch_project_versions_for_picker(
                            project_id,
                            target,
                            &source_gvs,
                        )
                    })
                    .await?
            } else {
                return Ok(None);
            }
        } else if fetch_policy == VersionFetchPolicy::AllowFetch {
            self.fetch_project_versions_for_picker(project_id, target, source_game_versions)
                .await?
        } else {
            return Ok(None);
        };

        Ok(
            pick_modrinth_version(
                versions.as_slice(),
                target,
                source_game_versions,
                source_mod_version,
                version_policy,
            )
            .map(|v| version_file_to_mod_file(v, &self.endpoints)),
        )
    }

    async fn fetch_project_versions(
        &self,
        project_id: &str,
    ) -> anyhow::Result<Vec<ModrinthVersionFile>> {
        // Compatibility check: cap pages so a huge project cannot block progress for minutes.
        self.fetch_all_project_versions(project_id, Some(3), None, None)
            .await
    }

    async fn fetch_project_versions_full(
        &self,
        project_id: &str,
    ) -> anyhow::Result<Vec<ModrinthVersionFile>> {
        self.fetch_all_project_versions(project_id, None, None, None).await
    }

    /// 市场选版：默认按目标 MC/加载器过滤（1–2 次请求）；expand 时再拉取更多页。
    pub async fn fetch_versions_for_market_picker(
        &self,
        project_id: &str,
        target: &CompatTarget,
        source_gvs: &[String],
        expand: bool,
        skip_loader_filter: bool,
    ) -> anyhow::Result<Vec<ModrinthVersionFile>> {
        if expand {
            return self
                .fetch_versions_for_market_picker_expanded(
                    project_id,
                    target,
                    source_gvs,
                    skip_loader_filter,
                )
                .await;
        }

        if target.mc_known() || (!skip_loader_filter && !loader_query_tags(&target.loader).is_empty())
        {
            let filtered = self
                .fetch_filtered_version_pages(project_id, target, source_gvs, skip_loader_filter)
                .await?;
            if !filtered.is_empty() {
                return Ok(filtered);
            }
        }

        self.fetch_all_project_versions(project_id, Some(1), None, None)
            .await
    }

    async fn fetch_versions_for_market_picker_expanded(
        &self,
        project_id: &str,
        target: &CompatTarget,
        source_gvs: &[String],
        skip_loader_filter: bool,
    ) -> anyhow::Result<Vec<ModrinthVersionFile>> {
        const EXPAND_MAX_PAGES: usize = 5;
        let mut all = self
            .fetch_all_project_versions(project_id, Some(EXPAND_MAX_PAGES), None, None)
            .await?;
        if target.mc_known() || (!skip_loader_filter && !loader_query_tags(&target.loader).is_empty())
        {
            if let Ok(filtered) = self
                .fetch_filtered_version_pages(project_id, target, source_gvs, skip_loader_filter)
                .await
            {
                for v in filtered {
                    if !all
                        .iter()
                        .any(|existing| existing.version_number == v.version_number)
                    {
                        all.push(v);
                    }
                }
            }
        }
        Ok(all)
    }

    /// PCL 风格：拉取项目版本供手动选择（带上限，防止无限分页）
    pub async fn fetch_all_versions_for_picker(
        &self,
        project_id: &str,
    ) -> anyhow::Result<Vec<ModrinthVersionFile>> {
        self.fetch_all_project_versions(project_id, Some(5), None, None)
            .await
    }

    /// Version picker: filtered API first (1–2 requests), then capped unfiltered fallback.
    async fn fetch_project_versions_for_picker(
        &self,
        project_id: &str,
        target: &CompatTarget,
        source_game_versions: &[String],
    ) -> anyhow::Result<Vec<ModrinthVersionFile>> {
        let filtered = self
            .fetch_filtered_version_pages(project_id, target, source_game_versions, false)
            .await?;
        if !filtered.is_empty() {
            return Ok(filtered);
        }
        self.fetch_all_project_versions(project_id, Some(3), None, None)
            .await
    }

    async fn fetch_filtered_version_pages(
        &self,
        project_id: &str,
        target: &CompatTarget,
        source_game_versions: &[String],
        skip_loader_filter: bool,
    ) -> anyhow::Result<Vec<ModrinthVersionFile>> {
        let loader_tags = if skip_loader_filter {
            Vec::new()
        } else {
            loader_query_tags(&target.loader)
        };
        let loaders_json = if loader_tags.is_empty() {
            None
        } else {
            Some(loader_tags_json(&loader_tags))
        };
        let mc_tags = game_version_query_tags(&target.mc_version, source_game_versions);

        if target.mc_known() {
            if let Some(mc) = mc_tags.first() {
                let primary = self
                    .fetch_version_page(
                        project_id,
                        0,
                        loaders_json.as_deref(),
                        Some(mc.as_str()),
                    )
                    .await?;
                if !primary.is_empty() {
                    return Ok(primary);
                }
                if let Some(broad) = mc_tags
                    .iter()
                    .skip(1)
                    .find(|t| t.matches('.').count() == 1)
                {
                    let broad = self
                        .fetch_version_page(
                            project_id,
                            0,
                            loaders_json.as_deref(),
                            Some(broad.as_str()),
                        )
                        .await?;
                    if !broad.is_empty() {
                        return Ok(broad);
                    }
                }
            }
        }

        if let Some(ref loaders) = loaders_json {
            let loader_only = self
                .fetch_version_page(project_id, 0, Some(loaders.as_str()), None)
                .await?;
            if !loader_only.is_empty() {
                return Ok(loader_only);
            }
        }
        Ok(Vec::new())
    }

    /// PCL-style unfiltered version list; client-side filtering via `pick_modrinth_version`.
    async fn fetch_all_project_versions(
        &self,
        project_id: &str,
        max_pages: Option<usize>,
        loaders_json: Option<&str>,
        game_version: Option<&str>,
    ) -> anyhow::Result<Vec<ModrinthVersionFile>> {
        let mut all = Vec::new();
        let mut offset = 0usize;
        let mut pages = 0usize;
        loop {
            let page = self
                .fetch_version_page(project_id, offset, loaders_json, game_version)
                .await?;
            let count = page.len();
            all.extend(page);
            pages += 1;
            if count < 100 {
                break;
            }
            if max_pages.is_some_and(|max| pages >= max) {
                break;
            }
            offset += 100;
        }
        Ok(all)
    }

    /// Prefetch: one page is enough to warm cache; update API handles most checks.
    async fn prefetch_version_list(
        &self,
        project_id: &str,
        _target: &CompatTarget,
    ) -> anyhow::Result<Vec<ModrinthVersionFile>> {
        self.fetch_all_project_versions(project_id, Some(1), None, None)
            .await
    }

    async fn fetch_version_page(
        &self,
        project_id: &str,
        offset: usize,
        loaders_json: Option<&str>,
        game_version: Option<&str>,
    ) -> anyhow::Result<Vec<ModrinthVersionFile>> {
        let url = format!("{}/project/{project_id}/version", self.api());
        let offset_str = offset.to_string();
        let mut req = self.client.get(&url).query(&[
            ("limit", "100"),
            ("offset", offset_str.as_str()),
            ("include_changelog", "false"),
        ]);
        if let Some(loaders) = loaders_json {
            req = req.query(&[("loaders", loaders)]);
        }
        if let Some(gv) = game_version {
            req = req.query(&[("game_versions", format!("[\"{gv}\"]"))]);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let body = resp.bytes().await?;

        if !status.is_success() {
            // MCIM mirror often 404/400 on filtered queries or stale project ids.
            if matches!(status.as_u16(), 400 | 404 | 502 | 503) {
                return Ok(Vec::new());
            }
            return Err(anyhow::anyhow!(
                "GET {url} returned {status}: {}",
                String::from_utf8_lossy(&body[..body.len().min(240)])
            ));
        }

        if let Ok(raw) = serde_json::from_slice::<Vec<ModrinthProjectVersion>>(&body) {
            return Ok(raw
                .into_iter()
                .filter_map(ModrinthProjectVersion::into_version_file)
                .collect());
        }
        if let Ok(flat) = serde_json::from_slice::<Vec<ModrinthVersionFile>>(&body) {
            return Ok(flat);
        }

        let url = format!("{}/project/{project_id}/version", self.api());
        Err(anyhow::anyhow!(
            "failed to parse Modrinth version list from {url}: {}",
            String::from_utf8_lossy(&body[..body.len().min(240)])
        ))
    }

    async fn get_project(&self, project_id: &str) -> anyhow::Result<ModrinthProject> {
        Ok(self
            .client
            .get(format!("{}/project/{project_id}", self.api()))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    async fn fetch_version_files_by_hashes(
        &self,
        sha512_list: &[&str],
    ) -> anyhow::Result<HashMap<String, ModrinthVersionFile>> {
        const CHUNK: usize = 96;
        let mut merged = HashMap::new();
        for chunk in sha512_list.chunks(CHUNK) {
            merged.extend(self.post_version_files_by_hashes(chunk, "sha512").await?);
        }
        Ok(merged)
    }

    async fn fetch_projects_parallel(
        &self,
        project_ids: &HashSet<String>,
        concurrency: usize,
    ) -> HashMap<String, (String, Option<String>)> {
        stream::iter(project_ids.iter().cloned())
            .map(|project_id| async move {
                let meta = self
                    .get_project(&project_id)
                    .await
                    .ok()
                    .map(|p| (p.title, p.icon_url))
                    .unwrap_or_else(|| (project_id.clone(), None));
                (project_id, meta)
            })
            .buffer_unordered(concurrency.max(1))
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect()
    }

    /// Batch identify via hash lookup, trying each configured mirror until one succeeds.
    pub async fn identify_by_hashes_with_mirrors(
        settings: &AppSettings,
        hashes: &[FileHash],
    ) -> HashMap<String, IdentifiedMod> {
        let mirrors = mirrors_with_official_fallback(&settings.mod_api_mirror);
        for (i, endpoints) in mirrors.iter().enumerate() {
            let provider = Self::with_endpoints(endpoints.clone());
            match provider.identify_by_hashes(hashes).await {
                Ok(map) if !map.is_empty() => return map,
                Ok(map) if i + 1 == mirrors.len() => return map,
                Ok(_) => {}
                Err(_) => {}
            }
        }
        HashMap::new()
    }
}

/// Preload Modrinth version lists for all known project IDs (all mirrors, parallel).
pub async fn prefetch_project_versions(
    settings: &AppSettings,
    mods: &[IdentifiedMod],
    target: &CompatTarget,
    cache: &ProjectVersionCache,
    concurrency: usize,
    cancel: Option<&CancelToken>,
) {
    let mut ids = HashSet::new();
    for m in mods {
        if let Some(pid) = &m.project_id {
            if !pid.is_empty() {
                ids.insert(pid.clone());
            }
        }
    }
    if ids.is_empty() {
        return;
    }

    let ids: Vec<String> = ids.into_iter().collect();
    let mirrors: Arc<Vec<ModrinthEndpoints>> = Arc::new({
        let list: Vec<_> = mirrors_with_official_fallback(&settings.mod_api_mirror);
        if list.is_empty() {
            vec![ModrinthEndpoints::official()]
        } else {
            list
        }
    });
    let cache = cache.clone();
    let target = target.clone();

    stream::iter(ids.into_iter())
        .map(|project_id| {
            let mirrors = Arc::clone(&mirrors);
            let cache = cache.clone();
            let target = target.clone();
            async move {
                if cancel.is_some_and(|c| c.is_cancelled()) {
                    return;
                }
                for endpoints in mirrors.iter() {
                    if cancel.is_some_and(|c| c.is_cancelled()) {
                        return;
                    }
                    let cache_key = version_list_cache_key(&project_id, &endpoints.api_base);
                    if cache.get(&cache_key).await.is_some() {
                        return;
                    }
                    let provider = ModrinthProvider::with_endpoints(endpoints.clone());
                    match provider.prefetch_version_list(&project_id, &target).await {
                        Ok(versions) if !versions.is_empty() => {
                            let _ = cache
                                .get_or_fetch(cache_key, || async move { Ok(versions) })
                                .await;
                            return;
                        }
                        _ => continue,
                    }
                }
            }
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<()>>()
        .await;
}

/// Find a compatible Modrinth file, trying MCIM / official mirrors per settings.
pub async fn find_compatible_with_mirrors(
    settings: &AppSettings,
    mod_info: &IdentifiedMod,
    target: &CompatTarget,
    version_cache: Option<&ProjectVersionCache>,
    fast_check: bool,
    version_policy: ModVersionPolicy,
) -> Option<ModFile> {
    let source_mod_version = mod_info.current_version.as_deref();
    let source_gvs = effective_game_versions(mod_info);

    let mirrors: Vec<_> = mirrors_with_official_fallback(&settings.mod_api_mirror);

    if fast_check {
        for endpoints in &mirrors {
            let provider = ModrinthProvider::with_endpoints(endpoints.clone());
            if let Some(file) = find_compatible_on_provider(
                &provider,
                mod_info,
                target,
                &source_gvs,
                version_cache,
                source_mod_version,
                true,
                VersionFetchPolicy::CacheOnly,
                version_policy,
            )
            .await
            {
                return Some(file);
            }
        }

        if mirrors.len() > 1 {
            let mod_info = mod_info.clone();
            let target = target.clone();
            let source_gvs = source_gvs.to_vec();
            let hits = futures::future::join_all(mirrors.iter().map(|endpoints| {
                let provider = ModrinthProvider::with_endpoints(endpoints.clone());
                let mod_info = mod_info.clone();
                let target = target.clone();
                let source_gvs = source_gvs.clone();
                let version_cache = version_cache.cloned();
                let version_policy = version_policy;
                async move {
                    find_compatible_on_provider(
                        &provider,
                        &mod_info,
                        &target,
                        &source_gvs,
                        version_cache.as_ref(),
                        source_mod_version,
                        true,
                        VersionFetchPolicy::AllowFetch,
                        version_policy,
                    )
                    .await
                }
            }))
            .await;
            return hits.into_iter().flatten().next();
        }

        if let Some(endpoints) = mirrors.first() {
            let provider = ModrinthProvider::with_endpoints(endpoints.clone());
            return find_compatible_on_provider(
                &provider,
                mod_info,
                target,
                &source_gvs,
                version_cache,
                source_mod_version,
                true,
                VersionFetchPolicy::AllowFetch,
                version_policy,
            )
            .await;
        }
        return None;
    }

    for (i, endpoints) in mirrors.iter().enumerate() {
        let provider = ModrinthProvider::with_endpoints(endpoints.clone());
        if let Some(file) = find_compatible_on_provider(
            &provider,
            mod_info,
            target,
            &source_gvs,
            version_cache,
            source_mod_version,
            false,
            VersionFetchPolicy::AllowFetch,
            version_policy,
        )
        .await
        {
            return Some(file);
        }
        if i + 1 == mirrors.len() {
            break;
        }
    }
    None
}

async fn find_compatible_on_provider(
    modrinth: &ModrinthProvider,
    mod_info: &IdentifiedMod,
    target: &CompatTarget,
    source_gvs: &[String],
    version_cache: Option<&ProjectVersionCache>,
    source_mod_version: Option<&str>,
    _fast_check: bool,
    fetch_policy: VersionFetchPolicy,
    version_policy: ModVersionPolicy,
) -> Option<ModFile> {
    use crate::compat::modrinth_lookup_keys;

    // Update API is a single fast POST — always try before version list lookup.
    if !mod_info.sha512.is_empty() || !mod_info.sha1.is_empty() {
        if let Ok(Some(f)) = modrinth
            .get_compatible_version_via_update(
                &mod_info.sha512,
                &mod_info.sha1,
                target,
                source_gvs,
                source_mod_version,
                version_policy,
            )
            .await
        {
            return Some(f);
        }
    }

    if let Some(pid) = &mod_info.project_id {
        if !pid.is_empty() {
            if let Ok(Some(f)) = modrinth
                .get_project_version_flexible_cached(
                    pid,
                    target,
                    source_gvs,
                    version_cache,
                    source_mod_version,
                    fetch_policy,
                    version_policy,
                )
                .await
            {
                return Some(f);
            }
        }
    }

    if mod_info.project_id.is_none() && !mod_info.sha512.is_empty() {
        if fetch_policy == VersionFetchPolicy::CacheOnly {
            return None;
        }
        if let Ok(Some(f)) = modrinth
            .get_compatible_version(
                &mod_info.sha512,
                &mod_info.sha1,
                target,
                source_gvs,
                version_cache,
                source_mod_version,
                version_policy,
            )
            .await
        {
            return Some(f);
        }
    }
    for search_key in modrinth_lookup_keys(mod_info) {
        if mod_info
            .project_id
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case(&search_key))
        {
            continue;
        }
        if let Ok(Some(pid)) = modrinth.resolve_project_id(&search_key).await {
            if let Ok(Some(f)) = modrinth
                .get_project_version_flexible_cached(
                    &pid,
                    target,
                    source_gvs,
                    version_cache,
                    source_mod_version,
                    VersionFetchPolicy::AllowFetch,
                    version_policy,
                )
                .await
            {
                return Some(f);
            }
        }
    }
    None
}

impl Default for ModrinthProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn loader_tags_json(tags: &[String]) -> String {
    format!(
        "[{}]",
        tags.iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn collect_modrinth_candidates<'a>(
    versions: &'a [ModrinthVersionFile],
    target: &CompatTarget,
    source_game_versions: &[String],
    source_mod_version: Option<&str>,
) -> Vec<(crate::compat::ReleaseScore, &'a ModrinthVersionFile)> {
    versions
        .iter()
        .enumerate()
        .filter_map(|(idx, v)| {
            let score = score_release(
                &v.game_versions,
                &v.loaders,
                &v.version_number,
                target,
                &PickContext {
                    source_mod_version,
                    source_game_versions,
                    list_index: idx,
                    channel: ReleaseChannel::from_modrinth(&v.version_type),
                },
            );
            if score.is_compatible() {
                Some((score, v))
            } else {
                None
            }
        })
        .collect()
}

fn pick_modrinth_version<'a>(
    versions: &'a [ModrinthVersionFile],
    target: &CompatTarget,
    source_game_versions: &[String],
    source_mod_version: Option<&str>,
    version_policy: ModVersionPolicy,
) -> Option<&'a ModrinthVersionFile> {
    let candidates = collect_modrinth_candidates(
        versions,
        target,
        source_game_versions,
        source_mod_version,
    );
    pick_best_scored(
        candidates,
        |v| &v.version_number,
        source_mod_version,
        version_policy,
    )
}

/// List Modrinth releases for manual version selection.
pub async fn list_modrinth_versions(
    settings: &AppSettings,
    mod_info: &IdentifiedMod,
    target: &CompatTarget,
    version_cache: Option<&ProjectVersionCache>,
    version_policy: ModVersionPolicy,
    expand: bool,
    skip_loader_filter: bool,
) -> anyhow::Result<Vec<ModVersionOption>> {
    let source_gvs = effective_game_versions(mod_info);
    let source_mod_version = mod_info.current_version.as_deref();
    let mirrors = mirrors_with_official_first(&settings.mod_api_mirror);
    let mut last_err: Option<anyhow::Error> = None;

    for endpoints in mirrors {
        match tokio::time::timeout(
            Duration::from_secs(12),
            list_modrinth_versions_on_mirror(
                &endpoints,
                mod_info,
                target,
                &source_gvs,
                source_mod_version,
                version_cache,
                version_policy,
                expand,
                skip_loader_filter,
            ),
        )
        .await
        {
            Ok(Ok(list)) if !list.is_empty() => return Ok(list),
            Ok(Ok(_)) => {}
            Ok(Err(e)) => last_err = Some(e),
            Err(_) => {
                last_err = Some(anyhow::anyhow!(
                    "Modrinth 镜像 {} 请求超时",
                    endpoints.api_base
                ));
            }
        }
    }

    if let Some(e) = last_err {
        Err(e)
    } else {
        Err(anyhow::anyhow!("未从 Modrinth 获取到任何版本，请检查网络或尝试切换 API 镜像为「仅官方源」"))
    }
}

async fn list_modrinth_versions_on_mirror(
    endpoints: &ModrinthEndpoints,
    mod_info: &IdentifiedMod,
    target: &CompatTarget,
    source_gvs: &[String],
    source_mod_version: Option<&str>,
    version_cache: Option<&ProjectVersionCache>,
    version_policy: ModVersionPolicy,
    expand: bool,
    skip_loader_filter: bool,
) -> anyhow::Result<Vec<ModVersionOption>> {
    let provider = ModrinthProvider::with_endpoints(endpoints.clone());
    let project_id = if let Some(pid) = mod_info
        .project_id
        .as_ref()
        .filter(|p| !p.is_empty())
        .cloned()
    {
        pid
    } else if !mod_info.sha512.is_empty() || !mod_info.sha1.is_empty() {
        provider
            .project_id_from_file_hashes(&mod_info.sha512, &mod_info.sha1)
            .await
            .ok_or_else(|| anyhow::anyhow!("hash not found on Modrinth"))?
    } else {
        return Ok(Vec::new());
    };

    let list_key = version_list_cache_key(&project_id, &endpoints.api_base);
    let cache_suffix = if expand { ":expand" } else { ":fast" };
    let cache_key = format!("{list_key}{cache_suffix}");
    let versions = if let Some(cache) = version_cache {
        cache
            .get_or_fetch(cache_key, || {
                provider.fetch_versions_for_market_picker(
                    &project_id,
                    target,
                    source_gvs,
                    expand,
                    skip_loader_filter,
                )
            })
            .await?
    } else {
        provider
            .fetch_versions_for_market_picker(
                &project_id,
                target,
                source_gvs,
                expand,
                skip_loader_filter,
            )
            .await?
    };

    if versions.is_empty() {
        return Ok(Vec::new());
    }

    let candidates = collect_modrinth_candidates(
        versions.as_slice(),
        target,
        source_gvs,
        source_mod_version,
    );
    let recommended = pick_best_scored(
        candidates.clone(),
        |v| &v.version_number,
        source_mod_version,
        version_policy,
    )
    .map(|v| v.version_number.clone());

    let compatible_versions: HashSet<String> = candidates
        .iter()
        .map(|(_, v)| v.version_number.clone())
        .collect();

    let mut options: Vec<ModVersionOption> = versions
        .iter()
        .map(|v| {
            let required_dependencies = v
                .dependencies
                .iter()
                .filter(|d| d.dependency_type == "required")
                .count() as u32;
            ModVersionOption {
                version: v.version_number.clone(),
                file_name: v.filename.clone(),
                download_url: endpoints.rewrite_download_url(&v.url),
                source: ModSource::Modrinth,
                game_versions: v.game_versions.clone(),
                recommended: recommended.as_deref() == Some(v.version_number.as_str()),
                loaders: v.loaders.clone(),
                version_type: v.version_type.clone(),
                required_dependencies,
            }
        })
        .collect();

    // 兼容版本排在前面，其余版本仍保留（PCL 手动选版）
    options.sort_by(|a, b| {
        let ac = compatible_versions.contains(&a.version);
        let bc = compatible_versions.contains(&b.version);
        bc.cmp(&ac).then_with(|| {
            crate::version::parse_mod_version_label(&b.version)
                .cmp(&crate::version::parse_mod_version_label(&a.version))
                .then_with(|| a.version.cmp(&b.version))
        })
    });
    Ok(options)
}

fn update_result_matches_target(
    vf: &ModrinthVersionFile,
    target: &CompatTarget,
    source_game_versions: &[String],
    source_mod_version: Option<&str>,
    version_policy: ModVersionPolicy,
) -> bool {
    if !update_result_acceptable(vf, source_mod_version, version_policy) {
        return false;
    }
    score_release(
        &vf.game_versions,
        &vf.loaders,
        &vf.version_number,
        target,
        &PickContext {
            source_mod_version,
            source_game_versions,
            list_index: 0,
            channel: ReleaseChannel::from_modrinth(&vf.version_type),
        },
    )
    .is_compatible()
}

fn update_result_acceptable(
    vf: &ModrinthVersionFile,
    source_mod_version: Option<&str>,
    version_policy: ModVersionPolicy,
) -> bool {
    if version_policy != ModVersionPolicy::Downgrade {
        return true;
    }
    if let Some(src) = source_mod_version.and_then(parse_mod_version_label) {
        return mod_version_at_most(&vf.version_number, &src);
    }
    true
}

fn version_file_to_mod_file(vf: &ModrinthVersionFile, endpoints: &ModrinthEndpoints) -> ModFile {
    ModFile {
        file_name: vf.filename.clone(),
        download_url: endpoints.rewrite_download_url(&vf.url),
        version: vf.version_number.clone(),
        source: ModSource::Modrinth,
    }
}

#[derive(Debug, Deserialize)]
struct ModrinthProject {
    title: String,
    icon_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthProjectDetail {
    id: String,
    slug: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    downloads: u64,
    icon_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthSearchResponse {
    hits: Vec<ModrinthSearchHit>,
}

#[derive(Debug, Deserialize)]
struct ModrinthSearchHit {
    project_id: String,
    slug: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ModrinthVersionFile {
    project_id: String,
    version_number: String,
    #[serde(alias = "file_name")]
    filename: String,
    url: String,
    loaders: Vec<String>,
    game_versions: Vec<String>,
    #[serde(default)]
    version_type: String,
    #[serde(default)]
    dependencies: Vec<ModrinthDependency>,
}

/// Response shape of `GET /project/{id}/version` (file metadata nested under `files`).
#[derive(Debug, Deserialize)]
struct ModrinthProjectVersion {
    project_id: String,
    version_number: String,
    loaders: Vec<String>,
    game_versions: Vec<String>,
    #[serde(default)]
    version_type: String,
    #[serde(default)]
    dependencies: Vec<ModrinthDependency>,
    #[serde(default)]
    files: Vec<ModrinthProjectVersionFile>,
}

#[derive(Debug, Deserialize)]
struct ModrinthProjectVersionFile {
    #[serde(alias = "file_name")]
    filename: String,
    url: String,
    #[serde(default)]
    primary: bool,
}

impl ModrinthProjectVersion {
    fn into_version_file(self) -> Option<ModrinthVersionFile> {
        let file = self
            .files
            .iter()
            .find(|f| f.primary)
            .or(self.files.first())?;
        Some(ModrinthVersionFile {
            project_id: self.project_id,
            version_number: self.version_number,
            filename: file.filename.clone(),
            url: file.url.clone(),
            loaders: self.loaders,
            game_versions: self.game_versions,
            version_type: self.version_type,
            dependencies: self.dependencies,
        })
    }
}

/// `POST /version_files` returns full Version objects (nested `files[]`), not flat file records.
fn parse_version_files_response(body: &[u8]) -> anyhow::Result<HashMap<String, ModrinthVersionFile>> {
    if let Ok(nested) = serde_json::from_slice::<HashMap<String, ModrinthProjectVersion>>(body) {
        let map: HashMap<_, _> = nested
            .into_iter()
            .filter_map(|(hash, entry)| entry.into_version_file().map(|vf| (hash, vf)))
            .collect();
        if !map.is_empty() {
            return Ok(map);
        }
    }
    if let Ok(flat) = serde_json::from_slice::<HashMap<String, ModrinthVersionFile>>(body) {
        if !flat.is_empty() {
            return Ok(flat);
        }
    }
    Err(anyhow::anyhow!(
        "failed to parse version_files response: {}",
        String::from_utf8_lossy(&body[..body.len().min(240)])
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_version_nested_files_parse() {
        let json = r#"{
            "project_id": "GcWjdA9I",
            "version_number": "0.27.12",
            "version_type": "release",
            "loaders": ["fabric"],
            "game_versions": ["1.21.11"],
            "dependencies": [],
            "files": [{
                "filename": "malilib-fabric-1.21.11-0.27.12.jar",
                "url": "https://cdn.modrinth.com/data/x.jar",
                "primary": true
            }]
        }"#;
        let raw: ModrinthProjectVersion = serde_json::from_str(json).unwrap();
        let vf = raw.into_version_file().unwrap();
        assert_eq!(vf.filename, "malilib-fabric-1.21.11-0.27.12.jar");
        assert!(vf.url.contains("cdn.modrinth.com"));
    }

    #[test]
    fn update_result_respects_downgrade_policy() {
        let vf = ModrinthVersionFile {
            project_id: "p".into(),
            version_number: "2.0.0".into(),
            filename: "a.jar".into(),
            url: "https://cdn.modrinth.com/a.jar".into(),
            loaders: vec!["fabric".into()],
            game_versions: vec!["1.21.1".into()],
            version_type: "release".into(),
            dependencies: vec![],
        };
        assert!(!update_result_acceptable(
            &vf,
            Some("1.0.0"),
            ModVersionPolicy::Downgrade
        ));
        assert!(update_result_acceptable(
            &vf,
            Some("3.0.0"),
            ModVersionPolicy::Downgrade
        ));
        assert!(update_result_acceptable(
            &vf,
            Some("1.0.0"),
            ModVersionPolicy::Auto
        ));
    }

    #[test]
    fn update_result_rejects_incompatible_loader() {
        let vf = ModrinthVersionFile {
            project_id: "p".into(),
            version_number: "1.0.0".into(),
            filename: "a.jar".into(),
            url: "https://cdn.modrinth.com/a.jar".into(),
            loaders: vec!["forge".into()],
            game_versions: vec!["1.21.1".into()],
            version_type: "release".into(),
            dependencies: vec![],
        };
        let target = CompatTarget {
            mc_version: "1.21.1".into(),
            loader: "fabric".into(),
            loader_version: String::new(),
        };
        assert!(!update_result_matches_target(
            &vf,
            &target,
            &[],
            None,
            ModVersionPolicy::Auto,
        ));
    }

    #[test]
    fn version_files_nested_map_parse() {
        let json = r#"{
            "abc123": {
                "project_id": "GcWjdA9I",
                "version_number": "0.27.12",
                "version_type": "release",
                "loaders": ["fabric"],
                "game_versions": ["1.21.11"],
                "dependencies": [],
                "files": [{
                    "filename": "malilib-fabric-1.21.11-0.27.12.jar",
                    "url": "https://cdn.modrinth.com/data/x.jar",
                    "primary": true
                }]
            }
        }"#;
        let map = parse_version_files_response(json.as_bytes()).unwrap();
        let vf = map.get("abc123").unwrap();
        assert_eq!(vf.project_id, "GcWjdA9I");
        assert_eq!(vf.filename, "malilib-fabric-1.21.11-0.27.12.jar");
    }
}
