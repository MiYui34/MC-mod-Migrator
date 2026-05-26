use reqwest::Client;
use scraper::{Html, Selector};

pub struct McModProvider {
    client: Client,
}

#[derive(Debug, Clone)]
pub struct McModSearchResult {
    pub mcmod_id: String,
    pub title: String,
    pub name_zh: Option<String>,
    pub page_url: String,
    pub modrinth_url: Option<String>,
    pub curseforge_url: Option<String>,
    pub github_url: Option<String>,
}

impl McModProvider {
    pub fn new() -> Self {
        Self {
            client: crate::http::build_http_client(crate::http::APP_USER_AGENT_MOD),
        }
    }

    pub async fn search(&self, query: &str) -> anyhow::Result<Option<McModSearchResult>> {
        Ok(self.search_list(query, 1).await?.into_iter().next())
    }

    /// PCL 风格：返回 MCMod 搜索页上的多个匹配项（中文 Mod 名搜索）
    pub async fn search_list(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<McModSearchResult>> {
        let limit = limit.max(1);
        let url = format!(
            "https://search.mcmod.cn/s?keyword={}",
            urlencoding::encode(query)
        );
        let html = self.client.get(&url).send().await?.text().await?;

        let mut results = Vec::new();
        for (page_url, title) in parse_search_page_all(&html).into_iter().take(limit) {
            if let Ok(html) = self.client.get(&page_url).send().await?.text().await {
                results.push(parse_mod_page(&html, &page_url, &title));
            }
        }

        if results.len() < limit {
            let search_url = format!(
                "https://www.mcmod.cn/modlist.html?key={}",
                urlencoding::encode(query)
            );
            let html = self.client.get(&search_url).send().await?.text().await?;
            for (page_url, title) in parse_modlist_page_all(&html, query) {
                if results.len() >= limit {
                    break;
                }
                if results.iter().any(|r| r.page_url == page_url) {
                    continue;
                }
                if let Ok(html) = self.client.get(&page_url).send().await?.text().await {
                    results.push(parse_mod_page(&html, &page_url, &title));
                }
            }
        }

        Ok(results)
    }
}

impl Default for McModProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_search_page_all(html: &str) -> Vec<(String, String)> {
    let document = Html::parse_document(html);
    let item_sel = Selector::parse(".result-item, .search-result-item, a[href*='/class/']")
        .unwrap_or_else(|_| Selector::parse("a").unwrap());

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for element in document.select(&item_sel) {
        if let Some(href) = element.value().attr("href") {
            if href.contains("/class/") {
                let page_url = normalize_mcmod_url(href);
                if !seen.insert(page_url.clone()) {
                    continue;
                }
                let title = element.text().collect::<String>().trim().to_string();
                if !title.is_empty() {
                    out.push((page_url, title));
                }
            }
        }
    }
    out
}

fn parse_search_page(html: &str) -> Option<(String, String)> {
    parse_search_page_all(html).into_iter().next()
}

fn parse_modlist_page_all(html: &str, query: &str) -> Vec<(String, String)> {
    let document = Html::parse_document(html);
    let link_sel = Selector::parse("a[href*='/class/']").unwrap();
    let query_lower = query.to_lowercase();
    let mut out = Vec::new();

    for element in document.select(&link_sel) {
        if let Some(href) = element.value().attr("href") {
            let title = element.text().collect::<String>().trim().to_string();
            if title.is_empty() {
                continue;
            }
            let title_lower = title.to_lowercase();
            if title_lower.contains(&query_lower) || query_lower.contains(&title_lower) {
                out.push((normalize_mcmod_url(href), title));
            }
        }
    }
    out
}

fn parse_modlist_page(html: &str, query: &str) -> Option<(String, String)> {
    parse_modlist_page_all(html, query).into_iter().next()
}

fn parse_mod_page(html: &str, page_url: &str, title: &str) -> McModSearchResult {
    let document = Html::parse_document(html);

    let mcmod_id = page_url
        .split("/class/")
        .nth(1)
        .and_then(|s| s.split('.').next())
        .unwrap_or("")
        .to_string();

    let link_sel = Selector::parse("a[href]").unwrap();
    let mut modrinth_url = None;
    let mut curseforge_url = None;
    let mut github_url = None;

    for element in document.select(&link_sel) {
        if let Some(href) = element.value().attr("href") {
            let lower = href.to_lowercase();
            if lower.contains("modrinth.com") {
                modrinth_url = Some(href.to_string());
            } else if lower.contains("curseforge.com") {
                curseforge_url = Some(href.to_string());
            } else if lower.contains("github.com") {
                github_url = Some(normalize_github_url(href));
            }
        }
    }

    let name_zh = title
        .split('(')
        .next()
        .map(|s| s.trim().trim_start_matches('[').to_string());

    McModSearchResult {
        mcmod_id,
        title: title.to_string(),
        name_zh,
        page_url: page_url.to_string(),
        modrinth_url,
        curseforge_url,
        github_url,
    }
}

fn normalize_mcmod_url(href: &str) -> String {
    if href.starts_with("http") {
        href.to_string()
    } else {
        format!("https://www.mcmod.cn{href}")
    }
}

fn normalize_github_url(url: &str) -> String {
    let url = url.trim_end_matches('/');
    if url.contains("/releases") {
        url.split("/releases").next().unwrap_or(url).to_string()
    } else {
        url.to_string()
    }
}

pub fn extract_modrinth_slug(url: &str) -> Option<String> {
    url.split("modrinth.com/mod/")
        .nth(1)
        .map(|s| {
            s.trim_end_matches('/')
                .split('/')
                .next()
                .unwrap_or(s)
                .to_string()
        })
}

pub fn extract_curseforge_slug(url: &str) -> Option<String> {
    url.split("/projects/")
        .nth(1)
        .map(|s| {
            s.trim_end_matches('/')
                .split('/')
                .next()
                .unwrap_or(s)
                .to_string()
        })
}
