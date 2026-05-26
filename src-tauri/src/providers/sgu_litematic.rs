use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use reqwest::Client;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::http::{build_http_client, decode_json_response};
use crate::models::{
    MarketProjectDetail, MarketSearchItem, MarketSearchResponse, MarketSort, ModSource,
    ModVersionOption,
};

pub const BASE_URL: &str = "https://litematic.sgu-server.xin";
const SCHEMATIC_CACHE_TTL: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
struct SguEndpoints {
    api_base: String,
    public_base: String,
    /// When set, `/uploads/...` paths map to `{public_base}{prefix}...` (www reverse proxy).
    uploads_prefix: Option<String>,
}

fn endpoint_candidates() -> Vec<SguEndpoints> {
    vec![
        SguEndpoints {
            api_base: "https://litematic.sgu-server.xin/api".into(),
            public_base: "https://litematic.sgu-server.xin".into(),
            uploads_prefix: None,
        },
        SguEndpoints {
            api_base: "https://www.sgu-server.xin/litematic-api".into(),
            public_base: "https://www.sgu-server.xin".into(),
            uploads_prefix: Some("/litematic-uploads/".into()),
        },
    ]
}

fn endpoints_store() -> &'static Mutex<Option<SguEndpoints>> {
    static STORE: OnceLock<Mutex<Option<SguEndpoints>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(None))
}

fn remember_endpoints(endpoints: SguEndpoints) {
    if let Ok(mut guard) = endpoints_store().lock() {
        *guard = Some(endpoints);
    }
}

fn current_endpoints() -> SguEndpoints {
    endpoints_store()
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(|| endpoint_candidates().into_iter().next().expect("endpoints"))
}

struct SchematicCacheEntry {
    fetched_at: Instant,
    items: Vec<SguSchematic>,
}

fn schematic_cache() -> &'static Mutex<HashMap<String, SchematicCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, SchematicCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn fetch_schematics_cached(client: &Client, query: &str) -> anyhow::Result<Vec<SguSchematic>> {
    let key = query.trim().to_lowercase();
    if let Ok(cache) = schematic_cache().lock() {
        if let Some(entry) = cache.get(&key) {
            if entry.fetched_at.elapsed() < SCHEMATIC_CACHE_TTL {
                return Ok(entry.items.clone());
            }
        }
    }

    let items = fetch_all_schematics(client, query).await?;
    if let Ok(mut cache) = schematic_cache().lock() {
        cache.insert(
            key,
            SchematicCacheEntry {
                fetched_at: Instant::now(),
                items: items.clone(),
            },
        );
    }
    Ok(items)
}

#[derive(Debug, Clone, Deserialize)]
struct SguSchematic {
    id: i64,
    name: String,
    #[serde(default, deserialize_with = "deserialize_optional_flex_string")]
    description: Option<String>,
    file_path: String,
    download_count: u64,
    is_pinned: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_optional_flex_string")]
    creator_name: Option<String>,
    updated_at: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_flex_string")]
    readme: Option<String>,
}

fn deserialize_optional_flex_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|v| match v {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Object(obj) => obj
            .get("name")
            .or_else(|| obj.get("text"))
            .or_else(|| obj.get("id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| serde_json::to_string(&serde_json::Value::Object(obj)).ok()),
        other => other.as_str().map(|s| s.to_string()),
    }))
}

fn abs_url_with(endpoints: &SguEndpoints, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        return path.to_string();
    }
    if let Some(prefix) = &endpoints.uploads_prefix {
        if let Some(rest) = path.strip_prefix("/uploads/") {
            return format!("{}{prefix}{rest}", endpoints.public_base.trim_end_matches('/'));
        }
    }
    format!("{}{path}", endpoints.public_base.trim_end_matches('/'))
}

fn format_transport_error(err: &reqwest::Error) -> String {
    let detail = err.to_string();
    if err.is_connect()
        || detail.contains("certificate")
        || detail.contains("tls")
        || detail.contains("ssl")
        || detail.contains("WRONG_PRINCIPAL")
    {
        "SSL/连接失败（HTTPS 证书可能未覆盖该子域名）".into()
    } else if err.is_timeout() {
        "请求超时".into()
    } else {
        detail
    }
}

fn summarize_endpoint_errors(errors: &[String]) -> String {
    format!(
        "无法访问 SGU 投影站 API（{}）。\n\
请检查服务器：\n\
· litematic.sgu-server.xin 的 HTTPS 证书需包含该子域名\n\
· Nginx 需将 /api/ 反代到 Litematic 后端（勿回落到 SPA 首页）\n\
· 或在 www.sgu-server.xin 配置 /litematic-api/ 与 /litematic-uploads/ 反代",
        errors.join("；")
    )
}

async fn get_json<T: DeserializeOwned>(client: &Client, path: &str) -> anyhow::Result<T> {
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let mut errors = Vec::new();

    for endpoints in endpoint_candidates() {
        let url = format!("{}{path}", endpoints.api_base.trim_end_matches('/'));
        let resp = match client.get(&url).send().await {
            Ok(resp) => resp,
            Err(err) => {
                errors.push(format!("{url}: {}", format_transport_error(&err)));
                continue;
            }
        };
        let status = resp.status();
        let resp = match resp.error_for_status() {
            Ok(resp) => resp,
            Err(err) => {
                errors.push(format!("{url}: HTTP {status} ({err})"));
                continue;
            }
        };
        match decode_json_response(resp).await {
            Ok(data) => {
                remember_endpoints(endpoints);
                return Ok(data);
            }
            Err(err) => errors.push(format!("{url}: {err}")),
        }
    }

    anyhow::bail!(summarize_endpoint_errors(&errors))
}

pub fn schematic_file_name(name: &str) -> String {
    let mut safe = String::new();
    for ch in name.trim().chars() {
        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            safe.push('_');
        } else {
            safe.push(ch);
        }
    }
    if safe.is_empty() {
        safe = "schematic".to_string();
    }
    if !safe.to_lowercase().ends_with(".litematic") {
        safe.push_str(".litematic");
    }
    safe
}

fn map_schematic(s: &SguSchematic) -> MarketSearchItem {
    let desc = s
        .description
        .clone()
        .filter(|d| !d.trim().is_empty())
        .or_else(|| {
            s.creator_name
                .as_ref()
                .map(|c| format!("作者：{c}"))
        })
        .unwrap_or_default();
    MarketSearchItem {
        id: s.id.to_string(),
        slug: s.id.to_string(),
        title: s.name.clone(),
        description: desc,
        icon_url: None,
        downloads: s.download_count,
        source: ModSource::Sgu,
        install_status: None,
        installed_version: None,
        modpack_badge: None,
    }
}

fn sort_schematics(items: &mut [SguSchematic], sort: MarketSort, has_query: bool) {
    match sort {
        MarketSort::Downloads => {
            items.sort_by(|a, b| {
                b.is_pinned
                    .unwrap_or(0)
                    .cmp(&a.is_pinned.unwrap_or(0))
                    .then(b.download_count.cmp(&a.download_count))
            });
        }
        MarketSort::Updated => {
            items.sort_by(|a, b| {
                b.updated_at
                    .as_deref()
                    .unwrap_or("")
                    .cmp(a.updated_at.as_deref().unwrap_or(""))
            });
        }
        MarketSort::Relevance => {
            if !has_query {
                items.sort_by(|a, b| {
                    b.is_pinned
                        .unwrap_or(0)
                        .cmp(&a.is_pinned.unwrap_or(0))
                        .then(b.download_count.cmp(&a.download_count))
                });
            }
        }
    }
}

async fn fetch_all_schematics(client: &Client, query: &str) -> anyhow::Result<Vec<SguSchematic>> {
    let path = if query.trim().is_empty() {
        "/schematics".to_string()
    } else {
        format!(
            "/schematics/search?q={}",
            urlencoding::encode(query.trim())
        )
    };
    get_json(client, &path).await
}

pub async fn search_litematics(
    query: &str,
    page: u32,
    page_size: u32,
    sort: MarketSort,
) -> anyhow::Result<MarketSearchResponse> {
    let client = build_http_client(crate::http::APP_USER_AGENT);
    let has_query = !query.trim().is_empty();
    let mut all = fetch_schematics_cached(&client, query).await?;
    sort_schematics(&mut all, sort, has_query);

    let total_hits = all.len() as u32;
    let start = (page * page_size) as usize;
    let end = start.saturating_add(page_size as usize).min(all.len());
    let slice = if start >= all.len() {
        &[][..]
    } else {
        &all[start..end]
    };

    Ok(MarketSearchResponse {
        items: slice.iter().map(map_schematic).collect(),
        page,
        page_size,
        total_hits,
        has_more: end < all.len(),
    })
}

pub async fn fetch_schematic_detail(project_id: &str) -> anyhow::Result<MarketProjectDetail> {
    let client = build_http_client(crate::http::APP_USER_AGENT);
    let s: SguSchematic = get_json(&client, &format!("/schematics/{project_id}")).await?;
    let endpoints = current_endpoints();

    let mut categories = Vec::new();
    if let Some(creator) = &s.creator_name {
        categories.push(format!("作者：{creator}"));
    }
    if s.is_pinned.unwrap_or(0) != 0 {
        categories.push("置顶".to_string());
    }

    let body = s.readme.unwrap_or_default();
    let description = s
        .description
        .clone()
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_default();

    Ok(MarketProjectDetail {
        id: s.id.to_string(),
        slug: s.id.to_string(),
        title: s.name.clone(),
        description,
        body,
        icon_url: None,
        gallery: vec![],
        project_url: endpoints.public_base.clone(),
        license: None,
        categories,
        modpack_badge: None,
    })
}

pub async fn list_litematic_versions(project_id: &str) -> anyhow::Result<Vec<ModVersionOption>> {
    let client = build_http_client(crate::http::APP_USER_AGENT);
    let s: SguSchematic = get_json(&client, &format!("/schematics/{project_id}")).await?;
    let endpoints = current_endpoints();

    let file_name = schematic_file_name(&s.name);
    let download_url = abs_url_with(&endpoints, &s.file_path);

    Ok(vec![ModVersionOption {
        version: "latest".to_string(),
        file_name,
        download_url,
        source: ModSource::Sgu,
        recommended: true,
        game_versions: vec![],
        loaders: vec![],
        version_type: String::new(),
        required_dependencies: 0,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schematic_file_name_adds_extension() {
        assert_eq!(
            schematic_file_name("测试投影"),
            "测试投影.litematic"
        );
        assert_eq!(
            schematic_file_name("foo.litematic"),
            "foo.litematic"
        );
    }

    #[test]
    fn abs_url_supports_www_uploads_proxy() {
        let ep = SguEndpoints {
            api_base: "https://www.sgu-server.xin/litematic-api".into(),
            public_base: "https://www.sgu-server.xin".into(),
            uploads_prefix: Some("/litematic-uploads/".into()),
        };
        assert_eq!(
            abs_url_with(&ep, "/uploads/123/foo.litematic"),
            "https://www.sgu-server.xin/litematic-uploads/123/foo.litematic"
        );
    }
}
