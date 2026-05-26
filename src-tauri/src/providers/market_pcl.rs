//! PCL-style market search: Chinese MCMod bridge, result ranking, and merge logic.
//!
//! Reference: PCL `ResourceSearcher.vb` (Meloong-Git/PCL).

use std::collections::{HashMap, HashSet};

use crate::models::{AppSettings, MarketCategory, MarketSearchItem, MarketSort, ModSource};
use crate::providers::curseforge::CurseForgeProvider;
use crate::providers::endpoints::mirrors_with_official_fallback;
use crate::providers::mcmod::{extract_curseforge_slug, extract_modrinth_slug, McModProvider};
use crate::providers::modrinth::ModrinthProvider;

const MCMOD_SEARCH_LIMIT: usize = 12;

pub fn is_chinese_query(query: &str) -> bool {
    query
        .chars()
        .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

/// MCMod 中文搜索 → Modrinth / CurseForge 工程（类似 PCL WikiEntry + 直接获取）
pub async fn chinese_mod_search(
    query: &str,
    category: MarketCategory,
    settings: &AppSettings,
) -> anyhow::Result<Vec<MarketSearchItem>> {
    if category != MarketCategory::Mod && category != MarketCategory::Datapack {
        return Ok(Vec::new());
    }
    if !is_chinese_query(query) {
        return Ok(Vec::new());
    }

    let provider = McModProvider::new();
    let hits = provider.search_list(query, MCMOD_SEARCH_LIMIT).await?;
    if hits.is_empty() {
        return Ok(Vec::new());
    }

    let mut slugs = Vec::new();
    let mut slug_titles: HashMap<String, String> = HashMap::new();
    for hit in &hits {
        if let Some(url) = &hit.modrinth_url {
            if let Some(slug) = extract_modrinth_slug(url) {
                slug_titles
                    .entry(slug.clone())
                    .or_insert_with(|| hit.name_zh.clone().unwrap_or_else(|| hit.title.clone()));
                if !slugs.contains(&slug) {
                    slugs.push(slug);
                }
            }
        }
    }

    let mut items = Vec::new();
    if !slugs.is_empty() {
        items.extend(fetch_modrinth_projects_by_slugs(settings, &slugs, &slug_titles).await);
    }

    for hit in hits {
        if let Some(url) = &hit.curseforge_url {
            if let Some(slug) = extract_curseforge_slug(url) {
                if items.iter().any(|i| i.slug.eq_ignore_ascii_case(&slug)) {
                    continue;
                }
                if let Some(item) = fetch_curseforge_by_slug(settings, &slug, &hit).await {
                    items.push(item);
                }
            }
        }
    }

    Ok(items)
}

async fn fetch_modrinth_projects_by_slugs(
    settings: &AppSettings,
    slugs: &[String],
    zh_titles: &HashMap<String, String>,
) -> Vec<MarketSearchItem> {
    let mut out = Vec::new();
    for endpoints in mirrors_with_official_fallback(&settings.mod_api_mirror) {
        let provider = ModrinthProvider::with_endpoints(endpoints);
        for slug in slugs {
            match provider.get_project_by_slug_or_id(slug).await {
                Ok(Some(p)) => {
                    let title = zh_titles
                        .get(slug)
                        .cloned()
                        .filter(|t| !t.is_empty())
                        .unwrap_or(p.title);
                    out.push(MarketSearchItem {
                        id: p.id,
                        slug: p.slug,
                        title,
                        description: p.description,
                        icon_url: p.icon_url,
                        downloads: p.downloads,
                        source: ModSource::Modrinth,
                        install_status: None,
                        installed_version: None,
                        modpack_badge: None,
                    });
                }
                Ok(None) => {}
                Err(_) => {}
            }
        }
        if !out.is_empty() {
            break;
        }
    }
    out
}

async fn fetch_curseforge_by_slug(
    settings: &AppSettings,
    slug: &str,
    hit: &crate::providers::mcmod::McModSearchResult,
) -> Option<MarketSearchItem> {
    let cf = CurseForgeProvider::from_settings(settings);
    let info = cf.search_mod_market(slug).await.ok()??;
    Some(MarketSearchItem {
        id: info.id.to_string(),
        slug: info.slug,
        title: hit
            .name_zh
            .clone()
            .filter(|t| !t.is_empty())
            .unwrap_or(info.name),
        description: info.summary,
        icon_url: info.logo_url,
        downloads: info.download_count as u64,
        source: ModSource::Curseforge,
        install_status: None,
        installed_version: None,
        modpack_badge: None,
    })
}

/// PCL 风格：CurseForge 优先，去重后按用户选择的排序方式排列
pub fn merge_and_rank_items(
    items: Vec<MarketSearchItem>,
    query: &str,
    sort: MarketSort,
    browse_mode: bool,
) -> Vec<MarketSearchItem> {
    let items = dedupe_for_sort(items, sort);
    match sort {
        MarketSort::Downloads => sort_by_downloads(items),
        MarketSort::Updated => items,
        MarketSort::Relevance => {
            if browse_mode || query.trim().is_empty() {
                sort_by_downloads(items)
            } else {
                rank_by_relevance(items, query.trim())
            }
        }
    }
}

fn dedupe_for_sort(items: Vec<MarketSearchItem>, sort: MarketSort) -> Vec<MarketSearchItem> {
    match sort {
        MarketSort::Downloads => dedupe_keep_highest_downloads(items),
        _ => dedupe_like_pcl(items),
    }
}

/// 下载量排序：同一 Mod 保留下载量更高的来源（避免 CF 覆盖 Modrinth 导致排序偏低）
fn dedupe_keep_highest_downloads(items: Vec<MarketSearchItem>) -> Vec<MarketSearchItem> {
    let mut out: Vec<MarketSearchItem> = Vec::new();
    for item in items {
        if let Some(idx) = out.iter().position(|e| projects_like(e, &item)) {
            let existing = &out[idx];
            let replace = item.downloads > existing.downloads
                || (item.downloads == existing.downloads
                    && item.source == ModSource::Curseforge
                    && existing.source != ModSource::Curseforge);
            if replace {
                out[idx] = item;
            }
        } else {
            out.push(item);
        }
    }
    out
}

fn dedupe_like_pcl(items: Vec<MarketSearchItem>) -> Vec<MarketSearchItem> {
    let mut cf: Vec<MarketSearchItem> = items
        .iter()
        .filter(|i| i.source == ModSource::Curseforge)
        .cloned()
        .collect();
    let mut mr: Vec<MarketSearchItem> = items
        .into_iter()
        .filter(|i| i.source != ModSource::Curseforge)
        .collect();
    cf.retain(|cf_item| !mr.iter().any(|mr_item| projects_like(cf_item, mr_item)));
    cf.append(&mut mr);
    cf
}

fn projects_like(a: &MarketSearchItem, b: &MarketSearchItem) -> bool {
    if a.id.eq_ignore_ascii_case(&b.id) {
        return true;
    }
    if !a.slug.is_empty() && a.slug.eq_ignore_ascii_case(&b.slug) {
        return true;
    }
    normalize_name(&a.title) == normalize_name(&b.title)
}

fn normalize_name(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn sort_by_downloads(mut items: Vec<MarketSearchItem>) -> Vec<MarketSearchItem> {
    items.sort_by(|a, b| b.downloads.cmp(&a.downloads));
    items
}

fn rank_by_relevance(mut items: Vec<MarketSearchItem>, query: &str) -> Vec<MarketSearchItem> {
    let q = query.to_lowercase();
    let q_norm = normalize_name(query);
    let mut seen = HashSet::new();

    items.sort_by(|a, b| {
        let sa = relevance_score(a, &q, &q_norm);
        let sb = relevance_score(b, &q, &q_norm);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    items.retain(|item| {
        let key = format!("{:?}:{}", item.source, item.id.to_lowercase());
        seen.insert(key)
    });
    items
}

fn relevance_score(item: &MarketSearchItem, q: &str, q_norm: &str) -> f64 {
    let title = item.title.to_lowercase();
    let title_norm = normalize_name(&item.title);
    let desc = item.description.to_lowercase();
    let mut score = 0.0f64;

    if title == *q || title_norm == *q_norm {
        score += 10.0;
    } else if title.contains(q) || title_norm.contains(q_norm) {
        score += 5.0;
    } else if desc.contains(q) {
        score += 1.0;
    }

    let dl_mult = if item.source == ModSource::Modrinth { 5.0 } else { 1.0 };
    score += (item.downloads.max(1) as f64 * dl_mult).log10() / 9.0;
    score
}
