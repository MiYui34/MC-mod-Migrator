use std::collections::HashMap;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use reqwest::Client;
use reqwest::RequestBuilder;
use serde::Deserialize;

use crate::compat::{
    pick_best_scored, score_release, CompatTarget, ModVersionPolicy, PickContext, ReleaseChannel,
};
use crate::models::{AppSettings, FileHash, IdentifiedMod, ModFile, ModSource, ModVersionOption};
use crate::providers::endpoints::{cf_usable_mirrors, rewrite_cf_download_url, CurseForgeEndpoints};
use crate::version::{
    effective_game_versions, mod_version_at_most, normalize_mc_version, parse_mod_version_label,
    release_supports_target_mc, is_known_loader,
};

const MINECRAFT_GAME_ID: i64 = 432;

pub struct CurseForgeProvider {
    client: Client,
    api_key: String,
    endpoints: CurseForgeEndpoints,
}

#[derive(Debug, Clone)]
pub struct CfModMarketInfo {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub download_count: u32,
    pub logo_url: Option<String>,
}

impl CurseForgeProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_endpoints(api_key, CurseForgeEndpoints::official())
    }

    pub fn with_endpoints(api_key: String, endpoints: CurseForgeEndpoints) -> Self {
        Self {
            client: crate::http::build_http_client(crate::http::APP_USER_AGENT),
            api_key,
            endpoints,
        }
    }

    pub fn from_settings(settings: &AppSettings) -> Self {
        cf_usable_mirrors(settings)
            .into_iter()
            .next()
            .map(|endpoints| Self::with_endpoints(settings.curseforge_api_key.clone(), endpoints))
            .unwrap_or_else(|| Self::with_endpoints(String::new(), CurseForgeEndpoints::mcim()))
    }

    fn has_key(&self) -> bool {
        !self.api_key.is_empty()
    }

    fn can_query(&self) -> bool {
        !self.endpoints.needs_api_key || self.has_key()
    }

    fn authed(&self, req: RequestBuilder) -> RequestBuilder {
        if self.has_key() {
            req.header("x-api-key", &self.api_key)
        } else {
            req
        }
    }

    fn api(&self, path: &str) -> String {
        format!("{}{path}", self.endpoints.api_base)
    }

    pub async fn identify_by_fingerprints(
        &self,
        hashes: &[FileHash],
    ) -> anyhow::Result<HashMap<i64, IdentifiedMod>> {
        if !self.can_query() || hashes.is_empty() {
            return Ok(HashMap::new());
        }

        let fingerprints: Vec<i64> = hashes.iter().map(|h| h.fingerprint).collect();
        let body = serde_json::json!({ "fingerprints": fingerprints });

        let resp: CfApiResponse<CfFingerprintResponse> = self
            .authed(
                self.client
                    .post(self.api(&format!("/fingerprints/{MINECRAFT_GAME_ID}"))),
            )
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let fp_to_hash: HashMap<i64, &FileHash> =
            hashes.iter().map(|h| (h.fingerprint, h)).collect();

        let mut results = HashMap::new();
        for m in resp.data.exact_matches {
            if let Some(hash) = fp_to_hash.get(&m.fingerprint) {
                let mod_info = self.get_mod(m.id).await.ok();
                let file_info = self.get_file(m.id, m.file.id).await.ok();
                let name = mod_info
                    .as_ref()
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| hash.file_name.clone());

                results.insert(
                    m.fingerprint,
                    IdentifiedMod {
                        file_name: hash.file_name.clone(),
                        file_path: hash.path.clone(),
                        sha512: hash.sha512.clone(),
                        sha1: hash.sha1.clone(),
                        fingerprint: hash.fingerprint,
                        source: ModSource::Curseforge,
                        project_id: None,
                        curseforge_id: Some(m.id),
                        name,
                        name_zh: None,
                        mod_id: None,
                        current_version: file_info.as_ref().map(|f| f.display_name.clone()),
                        loaders: file_info
                            .as_ref()
                            .map(|f| f.mod_loaders.clone())
                            .unwrap_or_default(),
                        game_versions: file_info
                            .as_ref()
                            .map(|f| f.game_versions.clone())
                            .unwrap_or_default(),
                        icon_url: mod_info
                            .as_ref()
                            .and_then(|m| m.logo.as_ref().map(|l| l.url.clone())),
                        github_url: None,
                        depends: vec![],
                    },
                );
            }
        }
        Ok(results)
    }

    pub async fn identify_by_fingerprints_with_mirrors(
        settings: &AppSettings,
        hashes: &[FileHash],
    ) -> HashMap<i64, IdentifiedMod> {
        let mirrors = cf_usable_mirrors(settings);
        for (i, endpoints) in mirrors.iter().enumerate() {
            let provider =
                Self::with_endpoints(settings.curseforge_api_key.clone(), endpoints.clone());
            match provider.identify_by_fingerprints(hashes).await {
                Ok(map) if !map.is_empty() => return map,
                Ok(map) if i + 1 == mirrors.len() => return map,
                Ok(_) => {}
                Err(_) => {}
            }
        }
        HashMap::new()
    }

    pub async fn search_mod_id(&self, slug: &str) -> anyhow::Result<Option<i64>> {
        if !self.can_query() {
            return Ok(None);
        }

        let resp: CfApiResponse<Vec<CfModSearchHit>> = self
            .authed(self.client.get(self.api("/mods/search")))
            .query(&[
                ("gameId", MINECRAFT_GAME_ID.to_string()),
                ("searchFilter", slug.to_string()),
                ("pageSize", "5".to_string()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let hits = resp.data;
        let slug_lower = slug.to_lowercase();
        for hit in &hits {
            if hit.slug.eq_ignore_ascii_case(slug) || hit.slug.to_lowercase() == slug_lower {
                return Ok(Some(hit.id));
            }
        }

        Ok(hits.into_iter().next().map(|hit| hit.id))
    }

    /// 按 slug 搜索 Mod 市场摘要（PCL 中文桥接用）
    pub async fn search_mod_market(&self, slug: &str) -> anyhow::Result<Option<CfModMarketInfo>> {
        if !self.can_query() {
            return Ok(None);
        }

        let resp: CfApiResponse<Vec<CfModSearchHitMarket>> = self
            .authed(self.client.get(self.api("/mods/search")))
            .query(&[
                ("gameId", MINECRAFT_GAME_ID.to_string()),
                ("searchFilter", slug.to_string()),
                ("pageSize", "5".to_string()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let hits = resp.data;
        let slug_lower = slug.to_lowercase();
        let hit = hits
            .iter()
            .find(|h| h.slug.eq_ignore_ascii_case(slug) || h.slug.to_lowercase() == slug_lower)
            .or(hits.first());

        Ok(hit.map(|h| CfModMarketInfo {
            id: h.id,
            slug: h.slug.clone(),
            name: h.name.clone(),
            summary: h.summary.clone(),
            download_count: h.download_count,
            logo_url: h.logo.as_ref().map(|l| l.url.clone()),
        }))
    }

    pub async fn get_mod_info(&self, mod_id: i64) -> anyhow::Result<CfModMarketInfo> {
        let detail = self.get_mod_detail(mod_id).await?;
        Ok(CfModMarketInfo {
            id: mod_id,
            slug: detail.slug,
            name: detail.name,
            summary: detail.summary,
            download_count: detail.download_count,
            logo_url: detail.logo.map(|l| l.url),
        })
    }

    /// Find shader/resource pack for target MC via classId search.
    pub async fn find_pack_for_mc(
        &self,
        name: &str,
        class_id: i64,
        target_mc: &str,
    ) -> anyhow::Result<Option<ModFile>> {
        if !self.can_query() {
            return Ok(None);
        }

        let resp: CfApiResponse<Vec<CfModSearchHit>> = self
            .authed(self.client.get(self.api("/mods/search")))
            .query(&[
                ("gameId", MINECRAFT_GAME_ID.to_string()),
                ("classId", class_id.to_string()),
                ("searchFilter", name.to_string()),
                ("pageSize", "5".to_string()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let mod_id = resp.data.into_iter().next().map(|h| h.id);
        let Some(mod_id) = mod_id else {
            return Ok(None);
        };

        let target = CompatTarget {
            mc_version: normalize_mc_version(target_mc),
            loader: "fabric".into(),
            loader_version: String::new(),
        };
        self.get_compatible_version(mod_id, &target, &[], None, ModVersionPolicy::Auto)
            .await
    }

    pub async fn get_compatible_version(
        &self,
        mod_id: i64,
        target: &CompatTarget,
        source_game_versions: &[String],
        source_mod_version: Option<&str>,
        version_policy: ModVersionPolicy,
    ) -> anyhow::Result<Option<ModFile>> {
        if !self.can_query() {
            return Ok(None);
        }

        if let Some(file) = self
            .get_compatible_via_latest_index(
                mod_id,
                target,
                source_game_versions,
                source_mod_version,
                version_policy,
            )
            .await?
        {
            return Ok(Some(file));
        }

        let loader_type = map_loader_type(&target.loader);
        let files = self
            .fetch_mod_files(mod_id, &target.mc_version, Some(loader_type))
            .await?;

        let candidates: Vec<_> = files
            .iter()
            .enumerate()
            .filter(|(_, file)| file_matches_loader(file, &target.loader))
            .filter(|(_, file)| release_supports_target_mc(&file.game_versions, &target.mc_version))
            .filter_map(|(idx, file)| {
                let score = score_release(
                    &file.game_versions,
                    &file.mod_loaders,
                    &file.display_name,
                    target,
                    &PickContext {
                        source_mod_version,
                        source_game_versions,
                        list_index: idx,
                        channel: ReleaseChannel::Release,
                    },
                );
                if score.is_compatible() {
                    Some((score, file))
                } else {
                    None
                }
            })
            .collect();

        let best = pick_best_scored(
            candidates,
            |file| &file.display_name,
            source_mod_version,
            version_policy,
        );

        if let Some(file) = best {
            let url = self.resolve_file_download_url(mod_id, file).await?;
            return Ok(Some(ModFile {
                file_name: file.file_name.clone(),
                download_url: url,
                version: file.display_name.clone(),
                source: ModSource::Curseforge,
            }));
        }
        Ok(None)
    }

    async fn resolve_file_download_url(
        &self,
        mod_id: i64,
        file: &CfModFile,
    ) -> anyhow::Result<String> {
        if let Some(url) = file.download_url.as_ref().filter(|u| !u.is_empty()) {
            return Ok(self.endpoints.rewrite_download_url(url));
        }
        let url = self.get_download_url(mod_id, file.id).await?;
        Ok(self.endpoints.rewrite_download_url(&url))
    }

    async fn get_mod(&self, mod_id: i64) -> anyhow::Result<CfMod> {
        let resp: CfApiResponse<CfMod> = self
            .authed(self.client.get(self.api(&format!("/mods/{mod_id}"))))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp.data)
    }

    async fn get_mod_detail(&self, mod_id: i64) -> anyhow::Result<CfModDetail> {
        let resp: CfApiResponse<CfModDetail> = self
            .authed(self.client.get(self.api(&format!("/mods/{mod_id}"))))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp.data)
    }

    async fn get_file(&self, mod_id: i64, file_id: i64) -> anyhow::Result<CfModFile> {
        let resp: CfApiResponse<CfModFile> = self
            .authed(
                self.client
                    .get(self.api(&format!("/mods/{mod_id}/files/{file_id}"))),
            )
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp.data)
    }

    async fn get_download_url(&self, mod_id: i64, file_id: i64) -> anyhow::Result<String> {
        use crate::http::decode_json_response;

        let resp = self
            .authed(
                self.client.get(self.api(&format!(
                    "/mods/{mod_id}/files/{file_id}/download-url"
                ))),
            )
            .send()
            .await?
            .error_for_status()?;
        let payload: CfApiResponse<CfDownloadUrlPayload> = decode_json_response(resp).await?;
        Ok(payload.data.into_url())
    }

    pub async fn fetch_file_download_url(&self, mod_id: i64, file_id: i64) -> anyhow::Result<String> {
        let url = self.get_download_url(mod_id, file_id).await?;
        Ok(self.endpoints.rewrite_download_url(&url))
    }

    pub async fn get_mod_file_for_install(
        &self,
        mod_id: i64,
        file_id: i64,
    ) -> anyhow::Result<(String, String)> {
        let file = self.get_file(mod_id, file_id).await?;
        let url = if let Some(u) = file.download_url.filter(|s| !s.is_empty()) {
            self.endpoints.rewrite_download_url(&u)
        } else {
            self.fetch_file_download_url(mod_id, file_id).await?
        };
        Ok((file.file_name, url))
    }

    async fn fetch_mod_files(
        &self,
        mod_id: i64,
        mc_version: &str,
        loader_type: Option<i32>,
    ) -> anyhow::Result<Vec<CfModFile>> {
        use crate::http::decode_json_response;

        let mut exact = self.authed(
            self.client
                .get(self.api(&format!("/mods/{mod_id}/files"))),
        );
        exact = exact.query(&[("pageSize", "50")]);
        if !mc_version.is_empty() && mc_version != "unknown" {
            exact = exact.query(&[("gameVersion", mc_version)]);
        }
        if let Some(loader_type) = loader_type {
            exact = exact.query(&[("modLoaderType", loader_type.to_string())]);
        }
        let exact = exact.send().await?.error_for_status()?;
        let exact: CfApiResponse<Vec<CfModFile>> = decode_json_response(exact).await?;
        if !exact.data.is_empty() {
            return Ok(exact.data);
        }

        let mut broad = self.authed(
            self.client
                .get(self.api(&format!("/mods/{mod_id}/files"))),
        );
        broad = broad.query(&[("pageSize", "100")]);
        if let Some(loader_type) = loader_type {
            broad = broad.query(&[("modLoaderType", loader_type.to_string())]);
        }
        let broad = broad.send().await?.error_for_status()?;
        let broad: CfApiResponse<Vec<CfModFile>> = decode_json_response(broad).await?;
        Ok(broad.data)
    }

    async fn fetch_mod_files_for_list(
        &self,
        mod_id: i64,
        mc_version: &str,
        loader_type: Option<i32>,
    ) -> anyhow::Result<Vec<CfModFile>> {
        use crate::http::decode_json_response;

        let mut exact = self.authed(
            self.client
                .get(self.api(&format!("/mods/{mod_id}/files"))),
        );
        exact = exact.query(&[("pageSize", "100")]);
        if !mc_version.is_empty() && mc_version != "unknown" {
            exact = exact.query(&[("gameVersion", mc_version)]);
        }
        if let Some(loader_type) = loader_type {
            exact = exact.query(&[("modLoaderType", loader_type.to_string())]);
        }
        let exact = exact.send().await?.error_for_status()?;
        let exact: CfApiResponse<Vec<CfModFile>> = decode_json_response(exact).await?;
        if !exact.data.is_empty() {
            return Ok(exact.data);
        }

        let mut broad = self.authed(
            self.client
                .get(self.api(&format!("/mods/{mod_id}/files"))),
        );
        broad = broad.query(&[("pageSize", "100")]);
        if let Some(loader_type) = loader_type {
            broad = broad.query(&[("modLoaderType", loader_type.to_string())]);
        }
        let broad = broad.send().await?.error_for_status()?;
        let broad: CfApiResponse<Vec<CfModFile>> = decode_json_response(broad).await?;
        Ok(broad.data)
    }

    /// PCL 风格：拉取 Mod 全部文件（分页上限，不做 MC/加载器过滤）
    async fn fetch_all_mod_files(&self, mod_id: i64) -> anyhow::Result<Vec<CfModFile>> {
        use crate::http::decode_json_response;

        const MAX_PAGES: i32 = 10;
        let page_size = "100";
        let mut all = Vec::new();
        let mut index = 0i32;
        loop {
            let resp = self
                .authed(
                    self.client
                        .get(self.api(&format!("/mods/{mod_id}/files"))),
                )
                .query(&[
                    ("pageSize", page_size),
                    ("index", &index.to_string()),
                ])
                .send()
                .await?
                .error_for_status()?;
            let page: CfApiResponse<Vec<CfModFile>> = decode_json_response(resp).await?;
            let count = page.data.len();
            all.extend(page.data);
            if count < 100 {
                break;
            }
            index += 1;
            if index >= MAX_PAGES {
                break;
            }
        }
        Ok(all)
    }

    /// PCL-style fast path: `latestFilesIndexes` on mod detail → single file fetch.
    async fn get_compatible_via_latest_index(
        &self,
        mod_id: i64,
        target: &CompatTarget,
        source_game_versions: &[String],
        source_mod_version: Option<&str>,
        version_policy: ModVersionPolicy,
    ) -> anyhow::Result<Option<ModFile>> {
        let detail = self.get_mod(mod_id).await?;
        let loader_type = map_loader_type(&target.loader);
        let file_id = pick_cf_latest_file_index(
            &detail.latest_files_indexes,
            loader_type,
            &target.mc_version,
            true,
        )
        .or_else(|| {
            pick_cf_latest_file_index(
                &detail.latest_files_indexes,
                loader_type,
                &target.mc_version,
                false,
            )
        })
        .map(|idx| idx.file_id);

        let Some(file_id) = file_id else {
            return Ok(None);
        };

        let file = self.get_file(mod_id, file_id).await?;
        if !file_matches_loader(&file, &target.loader) {
            return Ok(None);
        }
        let score = score_release(
            &file.game_versions,
            &file.mod_loaders,
            &file.display_name,
            target,
            &PickContext {
                source_mod_version,
                source_game_versions,
                list_index: 0,
                channel: ReleaseChannel::Release,
            },
        );
        if !score.is_compatible() {
            return Ok(None);
        }
        if version_policy == ModVersionPolicy::Downgrade {
            if let Some(src) = source_mod_version.and_then(parse_mod_version_label) {
                if !mod_version_at_most(&file.display_name, &src) {
                    return Ok(None);
                }
            }
        }

        let url = self.resolve_file_download_url(mod_id, &file).await?;
        Ok(Some(ModFile {
            file_name: file.file_name.clone(),
            download_url: url,
            version: file.display_name.clone(),
            source: ModSource::Curseforge,
        }))
    }
}

/// Parse CurseForge mod/file ids from API or CDN URLs.
pub fn parse_cf_mod_file_ids(url: &str) -> Option<(i64, i64)> {
    let lower = url.to_lowercase();
    let marker = "/mods/";
    let mods_idx = lower.find(marker)?;
    let after_mods = &url[mods_idx + marker.len()..];
    let (mod_part, rest) = after_mods.split_once('/')?;
    let mod_id = mod_part.parse().ok()?;
    let files_marker = "files/";
    let files_idx = rest.to_lowercase().find(files_marker)?;
    let after_files = &rest[files_idx + files_marker.len()..];
    let file_part = after_files
        .split(&['/', '?', '#'][..])
        .next()?;
    let file_id = file_part.parse().ok()?;
    Some((mod_id, file_id))
}

/// Resolve a CurseForge file download URL via configured mirrors (MCIM works without API key).
pub async fn resolve_cf_download_url(
    settings: &AppSettings,
    mod_id: i64,
    file_id: i64,
    fallback_url: &str,
) -> anyhow::Result<String> {
    let mut last_err: Option<anyhow::Error> = None;
    for endpoints in cf_usable_mirrors(settings) {
        let provider = CurseForgeProvider::with_endpoints(
            settings.curseforge_api_key.clone(),
            endpoints,
        );
        match provider.fetch_file_download_url(mod_id, file_id).await {
            Ok(url) if !url.is_empty() => return Ok(url),
            Ok(_) => {}
            Err(e) => last_err = Some(e),
        }
    }
    if !fallback_url.trim().is_empty() {
        return Ok(rewrite_cf_download_url(fallback_url, settings));
    }
    Err(last_err.unwrap_or_else(|| {
        anyhow::anyhow!("无法解析 CurseForge 下载链接，请检查网络或 Mod API 镜像设置")
    }))
}

/// Find a compatible CurseForge file across configured mirrors.
pub async fn find_compatible_with_mirrors(
    settings: &AppSettings,
    mod_info: &IdentifiedMod,
    target: &CompatTarget,
    version_policy: ModVersionPolicy,
) -> Option<ModFile> {
    let source_mod_version = mod_info.current_version.as_deref();
    let source_gvs = effective_game_versions(mod_info);

    for endpoints in cf_usable_mirrors(settings) {
        let provider = CurseForgeProvider::with_endpoints(
            settings.curseforge_api_key.clone(),
            endpoints,
        );
        if let Some(file) = find_compatible_on_provider(
            &provider,
            mod_info,
            target,
            &source_gvs,
            source_mod_version,
            version_policy,
        )
        .await
        {
            return Some(file);
        }
    }
    None
}

/// List CurseForge files for manual version selection.
pub async fn list_curseforge_versions(
    settings: &AppSettings,
    mod_info: &IdentifiedMod,
    target: &CompatTarget,
    version_policy: ModVersionPolicy,
    expand: bool,
    skip_loader_filter: bool,
) -> anyhow::Result<Vec<ModVersionOption>> {
    let source_mod_version = mod_info.current_version.as_deref();
    let source_gvs = effective_game_versions(mod_info);
    let has_key = !settings.curseforge_api_key.is_empty();

    for endpoints in cf_usable_mirrors(settings) {
        let provider = CurseForgeProvider::with_endpoints(
            settings.curseforge_api_key.clone(),
            endpoints.clone(),
        );
        let mod_id = if let Some(id) = mod_info.curseforge_id {
            Some(id)
        } else {
            let mut resolved = None;
            for key in curseforge_lookup_keys(mod_info) {
                if let Ok(Some(id)) = provider.search_mod_id(&key).await {
                    resolved = Some(id);
                    break;
                }
            }
            resolved
        };

        let Some(mod_id) = mod_id else {
            continue;
        };

        let loader_type = if skip_loader_filter {
            None
        } else {
            Some(map_loader_type(&target.loader))
        };
        let files = if expand {
            match tokio::time::timeout(
                Duration::from_secs(15),
                provider.fetch_all_mod_files(mod_id),
            )
            .await
            {
                Ok(Ok(files)) if !files.is_empty() => files,
                _ => Vec::new(),
            }
        } else {
            match tokio::time::timeout(
                Duration::from_secs(12),
                provider.fetch_mod_files_for_list(mod_id, &target.mc_version, loader_type),
            )
            .await
            {
                Ok(Ok(files)) => files,
                _ => Vec::new(),
            }
        };

        if files.is_empty() {
            continue;
        }

        let candidates: Vec<_> = files
            .iter()
            .enumerate()
            .filter(|(_, file)| file_matches_loader(file, &target.loader))
            .filter(|(_, file)| {
                release_supports_target_mc(&file.game_versions, &target.mc_version)
            })
            .filter_map(|(idx, file)| {
                let score = score_release(
                    &file.game_versions,
                    &file.mod_loaders,
                    &file.display_name,
                    target,
                    &PickContext {
                        source_mod_version,
                        source_game_versions: &source_gvs,
                        list_index: idx,
                        channel: ReleaseChannel::Release,
                    },
                );
                if score.is_compatible() {
                    Some((score, file))
                } else {
                    None
                }
            })
            .collect();

        let recommended = pick_best_scored(
            candidates.clone(),
            |file| &file.display_name,
            source_mod_version,
            version_policy,
        )
        .map(|f| f.display_name.clone());

        let recommended_name = recommended.clone();
        let compatible_names: std::collections::HashSet<String> = candidates
            .iter()
            .map(|(_, f)| f.display_name.clone())
            .collect();
        let mut options: Vec<ModVersionOption> = Vec::new();
        let mut pending: Vec<CfModFile> = Vec::new();

        for file in &files {
            if file
                .download_url
                .as_ref()
                .is_some_and(|u| !u.is_empty())
            {
                let url = provider.endpoints.rewrite_download_url(
                    file.download_url.as_ref().unwrap(),
                );
                options.push(ModVersionOption {
                    version: file.display_name.clone(),
                    file_name: file.file_name.clone(),
                    download_url: url,
                    source: ModSource::Curseforge,
                    game_versions: file.game_versions.clone(),
                    recommended: recommended_name.as_deref() == Some(file.display_name.as_str()),
                    loaders: file.mod_loaders.clone(),
                    version_type: String::new(),
                    required_dependencies: 0,
                });
            } else {
                pending.push((*file).clone());
            }
        }

        let api_key = settings.curseforge_api_key.clone();
        let endpoints = endpoints.clone();
        let resolved = stream::iter(pending)
            .map(|file| {
                let api_key = api_key.clone();
                let endpoints = endpoints.clone();
                let recommended_name = recommended_name.clone();
                async move {
                    let p = CurseForgeProvider::with_endpoints(api_key, endpoints);
                    let url = p.resolve_file_download_url(mod_id, &file).await.ok()?;
                    Some(ModVersionOption {
                        version: file.display_name.clone(),
                        file_name: file.file_name.clone(),
                        download_url: url,
                        source: ModSource::Curseforge,
                        game_versions: file.game_versions.clone(),
                        recommended: recommended_name.as_deref()
                            == Some(file.display_name.as_str()),
                        loaders: file.mod_loaders.clone(),
                        version_type: String::new(),
                        required_dependencies: 0,
                    })
                }
            })
            .buffer_unordered(8)
            .filter_map(|x| async move { x })
            .collect::<Vec<_>>()
            .await;
        options.extend(resolved);

        options.sort_by(|a, b| {
            let ac = compatible_names.contains(&a.version);
            let bc = compatible_names.contains(&b.version);
            bc.cmp(&ac).then_with(|| {
                parse_mod_version_label(&b.version)
                    .cmp(&parse_mod_version_label(&a.version))
                    .then_with(|| a.version.cmp(&b.version))
            })
        });
        if !options.is_empty() {
            return Ok(options);
        }
    }

    Err(anyhow::anyhow!(
        "未从 CurseForge 获取到任何版本，请检查网络或 Mod API 镜像设置{}",
        if has_key {
            String::new()
        } else {
            "（镜像模式下通常无需 API Key）".to_string()
        }
    ))
}

async fn find_compatible_on_provider(
    cf: &CurseForgeProvider,
    mod_info: &IdentifiedMod,
    target: &CompatTarget,
    source_gvs: &[String],
    source_mod_version: Option<&str>,
    version_policy: ModVersionPolicy,
) -> Option<ModFile> {
    if let Some(id) = mod_info.curseforge_id {
        if let Ok(Some(f)) = cf
            .get_compatible_version(id, target, source_gvs, source_mod_version, version_policy)
            .await
        {
            return Some(f);
        }
    }

    for key in curseforge_lookup_keys(mod_info) {
        if let Ok(Some(id)) = cf.search_mod_id(&key).await {
            if mod_info.curseforge_id == Some(id) {
                continue;
            }
            if let Ok(Some(f)) = cf
                .get_compatible_version(id, target, source_gvs, source_mod_version, version_policy)
                .await
            {
                return Some(f);
            }
        }
    }
    None
}

pub fn curseforge_lookup_keys(mod_info: &IdentifiedMod) -> Vec<String> {
    let mut keys = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let mut push = |s: String| {
        let t = s.trim().to_string();
        if !t.is_empty() && seen.insert(t.to_lowercase()) {
            keys.push(t);
        }
    };

    if let Some(id) = &mod_info.mod_id {
        push(id.clone());
    }
    if !mod_info.name.is_empty() && mod_info.name != mod_info.file_name {
        push(mod_info.name.clone());
    }
    let stem = mod_info
        .file_name
        .trim_end_matches(".jar")
        .trim_end_matches(".JAR");
    push(stem.to_string());
    keys
}

fn file_matches_loader(file: &CfModFile, loader: &str) -> bool {
    if !is_known_loader(loader) {
        return true;
    }
    if file.mod_loaders.is_empty() {
        return true;
    }
    file.mod_loaders
        .iter()
        .any(|l| l.eq_ignore_ascii_case(loader))
}

fn map_loader_type(loader: &str) -> i32 {
    match loader.to_lowercase().as_str() {
        "fabric" => 4,
        "quilt" => 5,
        "neoforge" => 6,
        _ => 1, // Forge
    }
}

#[derive(Debug, Deserialize)]
struct CfModSearchHit {
    id: i64,
    slug: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfModSearchHitMarket {
    id: i64,
    slug: String,
    name: String,
    summary: String,
    download_count: u32,
    logo: Option<CfLogo>,
}

#[derive(Debug, Deserialize)]
struct CfApiResponse<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct CfFingerprintResponse {
    exact_matches: Vec<CfExactMatch>,
}

#[derive(Debug, Deserialize)]
struct CfExactMatch {
    id: i64,
    fingerprint: i64,
    file: CfFileRef,
}

#[derive(Debug, Deserialize)]
struct CfFileRef {
    id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfLatestFileIndex {
    game_version: String,
    file_id: i64,
    mod_loader: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfMod {
    name: String,
    logo: Option<CfLogo>,
    #[serde(default)]
    latest_files_indexes: Vec<CfLatestFileIndex>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfModDetail {
    slug: String,
    name: String,
    summary: String,
    download_count: u32,
    logo: Option<CfLogo>,
}

fn pick_cf_latest_file_index<'a>(
    indexes: &'a [CfLatestFileIndex],
    loader_type: i32,
    target_mc: &str,
    exact_mc: bool,
) -> Option<&'a CfLatestFileIndex> {
    let target_n = normalize_mc_version(target_mc);
    indexes
        .iter()
        .filter(|idx| idx.mod_loader == loader_type)
        .filter(|idx| {
            if exact_mc {
                normalize_mc_version(&idx.game_version) == target_n
            } else {
                release_supports_target_mc(&[idx.game_version.clone()], target_mc)
            }
        })
        .max_by(|a, b| {
            parse_mod_version_label(&a.game_version)
                .cmp(&parse_mod_version_label(&b.game_version))
        })
}

#[derive(Debug, Deserialize)]
struct CfLogo {
    url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfModFile {
    id: i64,
    file_name: String,
    display_name: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    mod_loaders: Vec<String>,
    #[serde(default)]
    download_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CfDownloadUrlPayload {
    Direct(String),
    Wrapped { url: String },
}

impl CfDownloadUrlPayload {
    fn into_url(self) -> String {
        match self {
            Self::Direct(url) => url,
            Self::Wrapped { url } => url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cf_mod_file_ids_from_api_url() {
        let url = "https://api.curseforge.com/v1/mods/12345/files/67890/download";
        assert_eq!(parse_cf_mod_file_ids(url), Some((12345, 67890)));
    }

    #[test]
    fn pick_cf_latest_file_index_prefers_exact_mc() {
        let indexes = vec![
            CfLatestFileIndex {
                game_version: "1.21".into(),
                file_id: 1,
                mod_loader: 4,
            },
            CfLatestFileIndex {
                game_version: "1.21.4".into(),
                file_id: 2,
                mod_loader: 4,
            },
        ];
        let exact = pick_cf_latest_file_index(&indexes, 4, "1.21.4", true).unwrap();
        assert_eq!(exact.file_id, 2);
        let relaxed = pick_cf_latest_file_index(&indexes, 4, "1.21.4", false).unwrap();
        assert_eq!(relaxed.file_id, 2);
    }

    #[test]
    fn cf_mod_parses_latest_files_indexes() {
        let json = r#"{
            "name": "JEI",
            "latestFilesIndexes": [{
                "gameVersion": "1.21.4",
                "fileId": 12345,
                "modLoader": 4
            }]
        }"#;
        let m: CfMod = serde_json::from_str(json).unwrap();
        assert_eq!(m.latest_files_indexes.len(), 1);
        assert_eq!(m.latest_files_indexes[0].file_id, 12345);
        assert_eq!(m.latest_files_indexes[0].mod_loader, 4);
    }

    #[test]
    fn cf_mod_file_parses_camel_case() {
        let json = r#"{
            "id": 1,
            "fileName": "mod.jar",
            "displayName": "1.0.0",
            "gameVersions": ["1.21.4"],
            "modLoaders": ["Fabric"],
            "downloadUrl": "https://edge.forgecdn.net/files/1/mod.jar"
        }"#;
        let file: CfModFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.file_name, "mod.jar");
        assert_eq!(file.display_name, "1.0.0");
        assert_eq!(
            file.download_url.as_deref(),
            Some("https://edge.forgecdn.net/files/1/mod.jar")
        );
    }

    #[test]
    fn cf_download_url_accepts_string_or_object() {
        let direct: CfApiResponse<CfDownloadUrlPayload> =
            serde_json::from_str(r#"{"data":"https://example.com/a.jar"}"#).unwrap();
        assert_eq!(
            direct.data.into_url(),
            "https://example.com/a.jar"
        );

        let wrapped: CfApiResponse<CfDownloadUrlPayload> =
            serde_json::from_str(r#"{"data":{"url":"https://example.com/b.jar"}}"#).unwrap();
        assert_eq!(
            wrapped.data.into_url(),
            "https://example.com/b.jar"
        );
    }

    #[test]
    fn lookup_keys_include_mod_id_and_stem() {
        let m = IdentifiedMod {
            file_name: "jei-1.20.jar".into(),
            file_path: String::new(),
            sha512: String::new(),
            sha1: String::new(),
            fingerprint: 0,
            source: ModSource::Unknown,
            project_id: None,
            curseforge_id: None,
            name: "Just Enough Items".into(),
            name_zh: None,
            mod_id: Some("jei".into()),
            current_version: None,
            loaders: vec![],
            game_versions: vec![],
            icon_url: None,
            github_url: None,
            depends: vec![],
        };
        let keys = curseforge_lookup_keys(&m);
        assert!(keys.iter().any(|k| k == "jei"));
    }
}
