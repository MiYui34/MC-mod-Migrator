/// Modrinth / CDN endpoints (official or MCIM mirror used by PCL2).
#[derive(Debug, Clone)]
pub struct ModrinthEndpoints {
    pub api_base: String,
    cdn_from: Option<&'static str>,
    cdn_to: Option<&'static str>,
}

impl ModrinthEndpoints {
    pub fn official() -> Self {
        Self {
            api_base: "https://api.modrinth.com/v2".to_string(),
            cdn_from: None,
            cdn_to: None,
        }
    }

    /// MCIM — same API layout as Modrinth, used by PCL2 as domestic mirror.
    pub fn mcim() -> Self {
        Self {
            api_base: "https://mod.mcimirror.top/modrinth/v2".to_string(),
            cdn_from: Some("https://cdn.modrinth.com"),
            cdn_to: Some("https://mod.mcimirror.top"),
        }
    }

    pub fn rewrite_download_url(&self, url: &str) -> String {
        match (self.cdn_from, self.cdn_to) {
            (Some(from), Some(to)) => url.replace(from, to),
            _ => url.to_string(),
        }
    }
}

pub fn mirrors_for_setting(mirror: &str) -> Vec<ModrinthEndpoints> {
    match mirror {
        "mcim" => vec![ModrinthEndpoints::mcim()],
        "auto" => vec![ModrinthEndpoints::mcim(), ModrinthEndpoints::official()],
        _ => vec![ModrinthEndpoints::official()],
    }
}

/// Prefer configured mirrors, but always allow official Modrinth as last resort.
pub fn mirrors_with_official_fallback(mirror: &str) -> Vec<ModrinthEndpoints> {
    let mut mirrors = mirrors_for_setting(mirror);
    let has_official = mirrors
        .iter()
        .any(|m| m.api_base.contains("api.modrinth.com"));
    if !has_official {
        mirrors.push(ModrinthEndpoints::official());
    }
    mirrors
}

/// Latency-sensitive calls (version lists): official first in auto mode, MCIM as fallback.
pub fn mirrors_with_official_first(mirror: &str) -> Vec<ModrinthEndpoints> {
    match mirror {
        "auto" => vec![ModrinthEndpoints::official(), ModrinthEndpoints::mcim()],
        other => mirrors_with_official_fallback(other),
    }
}

/// CurseForge API / CDN endpoints (official or MCIM mirror).
#[derive(Debug, Clone)]
pub struct CurseForgeEndpoints {
    pub api_base: String,
    cdn_rewrites: Vec<(&'static str, &'static str)>,
    /// Official CF API requires a personal API key; MCIM mirror does not.
    pub needs_api_key: bool,
}

impl CurseForgeEndpoints {
    pub fn official() -> Self {
        Self {
            api_base: "https://api.curseforge.com/v1".to_string(),
            cdn_rewrites: vec![],
            needs_api_key: true,
        }
    }

    pub fn mcim() -> Self {
        Self {
            api_base: "https://mod.mcimirror.top/curseforge/v1".to_string(),
            cdn_rewrites: vec![
                ("https://edge.forgecdn.net", "https://mod.mcimirror.top"),
                ("https://mediafilez.forgecdn.net", "https://mod.mcimirror.top"),
            ],
            needs_api_key: false,
        }
    }

    pub fn rewrite_download_url(&self, url: &str) -> String {
        let mut out = url.to_string();
        if out.contains("https://api.curseforge.com/v1") {
            out = out.replace("https://api.curseforge.com/v1", &self.api_base);
        }
        for (from, to) in &self.cdn_rewrites {
            out = out.replace(from, *to);
        }
        out
    }
}

/// CurseForge mirrors that can be used without an API key (MCIM), plus official when a key is set.
pub fn cf_usable_mirrors(settings: &crate::models::AppSettings) -> Vec<CurseForgeEndpoints> {
    let has_key = !settings.curseforge_api_key.is_empty();
    match settings.mod_api_mirror.as_str() {
        "mcim" => vec![CurseForgeEndpoints::mcim()],
        "auto" => {
            let mut out = vec![CurseForgeEndpoints::mcim()];
            if has_key {
                out.push(CurseForgeEndpoints::official());
            }
            out
        }
        _ if has_key => vec![CurseForgeEndpoints::official()],
        // Official-only without key: still allow MCIM so mirror workflows keep working.
        _ => vec![CurseForgeEndpoints::mcim()],
    }
}

pub fn rewrite_cf_download_url(url: &str, settings: &crate::models::AppSettings) -> String {
    let mut out = url.to_string();
    for endpoints in cf_usable_mirrors(settings) {
        out = endpoints.rewrite_download_url(&out);
    }
    out
}

pub fn cf_mirrors_for_setting(mirror: &str) -> Vec<CurseForgeEndpoints> {
    match mirror {
        "mcim" => vec![CurseForgeEndpoints::mcim()],
        "auto" => vec![CurseForgeEndpoints::mcim(), CurseForgeEndpoints::official()],
        _ => vec![CurseForgeEndpoints::official()],
    }
}

/// Version / detail APIs: try official first when API key is available.
pub fn cf_mirrors_with_official_first(mirror: &str, has_api_key: bool) -> Vec<CurseForgeEndpoints> {
    match mirror {
        "auto" => {
            let mut out = Vec::new();
            if has_api_key {
                out.push(CurseForgeEndpoints::official());
            }
            out.push(CurseForgeEndpoints::mcim());
            out
        }
        other => cf_mirrors_for_setting(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcim_rewrites_cf_api_download_url() {
        let ep = CurseForgeEndpoints::mcim();
        let url = "https://api.curseforge.com/v1/mods/123/files/456/download";
        assert_eq!(
            ep.rewrite_download_url(url),
            "https://mod.mcimirror.top/curseforge/v1/mods/123/files/456/download"
        );
    }

    #[test]
    fn mcim_rewrites_cdn_url() {
        let ep = ModrinthEndpoints::mcim();
        let url = "https://cdn.modrinth.com/data/abc/versions/def/file.jar";
        assert_eq!(
            ep.rewrite_download_url(url),
            "https://mod.mcimirror.top/data/abc/versions/def/file.jar"
        );
    }

    #[test]
    fn mcim_only_gets_official_fallback() {
        let mirrors = mirrors_with_official_fallback("mcim");
        assert_eq!(mirrors.len(), 2);
        assert!(mirrors[0].api_base.contains("mcimirror"));
        assert!(mirrors[1].api_base.contains("api.modrinth.com"));
    }
}
