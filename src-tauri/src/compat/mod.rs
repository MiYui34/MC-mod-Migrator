//! Unified compatibility target + mod release scoring for download selection.

use crate::models::{IdentifiedMod, ModSource, TargetEnv};
use crate::version::{
    best_version_pick_score, effective_game_versions, game_version_exactly_matches_target,
    is_known_loader, loader_matches_target, mod_version_at_most, normalize_loader,
    normalize_mc_version, parse_mc_version, parse_mod_version_label, release_supports_target_mc,
    VersionPickScore,
};

/// Resolved instance the user wants to migrate mods into.
#[derive(Debug, Clone)]
pub struct CompatTarget {
    pub mc_version: String,
    pub loader: String,
    pub loader_version: String,
}

impl CompatTarget {
    pub fn from_target(env: &TargetEnv) -> Self {
        Self {
            mc_version: normalize_mc_version(&env.mc_version),
            loader: normalize_loader(&env.loader),
            loader_version: env.loader_version.clone(),
        }
    }

    pub fn from_env(env: &TargetEnv, mod_info: &IdentifiedMod) -> Self {
        let normalized = normalize_loader(&env.loader);
        let loader = if is_known_loader(&normalized) {
            normalized
        } else if let Some(l) = mod_info.loaders.iter().find(|l| is_known_loader(l)) {
            normalize_loader(l)
        } else {
            normalized
        };

        Self {
            mc_version: normalize_mc_version(&env.mc_version),
            loader,
            loader_version: env.loader_version.clone(),
        }
    }

    pub fn mc_known(&self) -> bool {
        !self.mc_version.is_empty() && self.mc_version != "unknown"
    }
}

/// Source mod already declares support for the target instance (same MC line + loader).
pub fn mod_locally_compatible(mod_info: &IdentifiedMod, target: &CompatTarget) -> bool {
    let gvs = effective_game_versions(mod_info);
    if !target.mc_known() || gvs.is_empty() {
        return false;
    }
    let mc_ok = gvs
        .iter()
        .any(|gv| game_version_exactly_matches_target(gv, &target.mc_version));
    if !mc_ok || !loader_matches_target(&mod_info.loaders, &target.loader) {
        return false;
    }
    mod_info.source != ModSource::Unknown
        || mod_info.project_id.is_some()
        || mod_info.curseforge_id.is_some()
}

pub fn local_mod_file(mod_info: &IdentifiedMod) -> crate::models::ModFile {
    crate::models::ModFile {
        file_name: mod_info.file_name.clone(),
        download_url: String::new(),
        version: mod_info
            .current_version
            .clone()
            .unwrap_or_else(|| "current".into()),
        source: mod_info.source.clone(),
    }
}

/// Try primary target loader first; if mod metadata conflicts, also try the mod's loader
/// (handles mis-detected instance loader while MC version is correct).
pub fn compat_lookup_targets(env: &TargetEnv, mod_info: &IdentifiedMod) -> Vec<CompatTarget> {
    let primary = CompatTarget::from_env(env, mod_info);
    let mut out = vec![primary.clone()];
    let target_loader = normalize_loader(&env.loader);
    if is_known_loader(&target_loader)
        && !mod_info.loaders.is_empty()
        && !loader_matches_target(&mod_info.loaders, &target_loader)
    {
        for l in &mod_info.loaders {
            let nl = normalize_loader(l);
            if is_known_loader(&nl) && !out.iter().any(|t| t.loader == nl) {
                out.push(CompatTarget {
                    loader: nl,
                    ..primary.clone()
                });
            }
        }
    }
    out
}

/// How to pick a mod release relative to the source instance version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModVersionPolicy {
    #[default]
    Auto,
    /// Prefer highest compatible version that is <= source mod version (no auto-upgrade).
    Downgrade,
}

impl ModVersionPolicy {
    pub fn from_setting(value: &str) -> Self {
        match value {
            "downgrade" => Self::Downgrade,
            _ => Self::Auto,
        }
    }
}

/// Pick the best scored candidate, optionally capping at the source mod version.
pub fn pick_best_scored<T: Clone>(
    candidates: Vec<(ReleaseScore, T)>,
    label: impl Fn(&T) -> &str,
    source_mod_version: Option<&str>,
    policy: ModVersionPolicy,
) -> Option<T> {
    if candidates.is_empty() {
        return None;
    }

    let pool = match policy {
        ModVersionPolicy::Auto => candidates,
        ModVersionPolicy::Downgrade => {
            if let Some(src) = source_mod_version.and_then(parse_mod_version_label) {
                let filtered: Vec<_> = candidates
                    .iter()
                    .filter(|(_, item)| mod_version_at_most(label(item), &src))
                    .cloned()
                    .collect();
                if filtered.is_empty() {
                    candidates
                } else {
                    filtered
                }
            } else {
                candidates
            }
        }
    };

    pool.into_iter()
        .min_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, item)| item)
}

/// Extra context from the source mod when picking a target release.
#[derive(Debug, Clone, Default)]
pub struct PickContext<'a> {
    pub source_mod_version: Option<&'a str>,
    pub source_game_versions: &'a [String],
    /// Position in provider list (0 = newest on Modrinth).
    pub list_index: usize,
    pub channel: ReleaseChannel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReleaseChannel {
    #[default]
    Release,
    Beta,
    Alpha,
}

impl ReleaseChannel {
    pub fn from_modrinth(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "beta" => Self::Beta,
            "alpha" => Self::Alpha,
            _ => Self::Release,
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::Release => 0,
            Self::Beta => 1,
            Self::Alpha => 2,
        }
    }
}

/// Lower is better. Used to pick the most suitable mod file for the target instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseScore {
    pub mc: VersionPickScore,
    pub loader_tier: u8,
    pub channel: u8,
    pub source_line_bonus: u8,
    pub version_label_tier: u8,
    /// Modrinth list position (0 = newest). Higher index is preferred when tied.
    pub list_index: usize,
}

impl PartialOrd for ReleaseScore {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ReleaseScore {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.mc
            .tier
            .cmp(&other.mc.tier)
            .then(self.mc.patch_distance.cmp(&other.mc.patch_distance))
            .then(self.loader_tier.cmp(&other.loader_tier))
            .then(self.channel.cmp(&other.channel))
            .then(self.source_line_bonus.cmp(&other.source_line_bonus))
            .then(self.version_label_tier.cmp(&other.version_label_tier))
            .then(other.list_index.cmp(&self.list_index))
    }
}

impl ReleaseScore {
    pub fn incompatible() -> Self {
        Self {
            mc: VersionPickScore {
                tier: u8::MAX,
                patch_distance: u32::MAX,
            },
            loader_tier: u8::MAX,
            channel: u8::MAX,
            source_line_bonus: u8::MAX,
            version_label_tier: u8::MAX,
            list_index: 0,
        }
    }

    pub fn is_compatible(&self) -> bool {
        self.mc.tier != u8::MAX
    }
}

pub fn score_release(
    game_versions: &[String],
    version_loaders: &[String],
    version_label: &str,
    target: &CompatTarget,
    ctx: &PickContext<'_>,
) -> ReleaseScore {
    if target.mc_known() {
        if !release_supports_target_mc(game_versions, &target.mc_version) {
            return ReleaseScore::incompatible();
        }
    }

    let mc = if target.mc_known() {
        best_version_pick_score(game_versions, &target.mc_version)
    } else {
        VersionPickScore {
            tier: 0,
            patch_distance: 0,
        }
    };

    if mc.tier == u8::MAX {
        return ReleaseScore::incompatible();
    }

    if !loader_matches_target(version_loaders, &target.loader) {
        return ReleaseScore::incompatible();
    }

    ReleaseScore {
        mc,
        loader_tier: loader_match_tier(version_loaders, &target.loader),
        channel: ctx.channel.rank(),
        source_line_bonus: source_mc_line_bonus(game_versions, ctx.source_game_versions),
        version_label_tier: version_label_tier(version_label, ctx.source_mod_version),
        list_index: ctx.list_index,
    }
}

pub fn compare_release_scores(a: ReleaseScore, b: ReleaseScore) -> std::cmp::Ordering {
    a.cmp(&b)
}

/// Game version filters for Modrinth API — exact first, minor line, plus source mod tags.
pub fn game_version_query_tags(mc_version: &str, extra: &[String]) -> Vec<String> {
    let mut tags = Vec::new();
    let mut push = |raw: &str| {
        let n = normalize_mc_version(raw);
        if n.is_empty() || n == "unknown" {
            return;
        }
        tags.push(n.clone());
        if let Some((major, minor, _)) = parse_mc_version(&n) {
            tags.push(format!("{major}.{minor}"));
        }
    };
    push(mc_version);
    for gv in extra {
        push(gv);
    }
    tags.sort();
    tags.dedup();
    tags
}

/// Search keys to resolve a mod on Modrinth, most specific first.
pub fn modrinth_lookup_keys(mod_info: &IdentifiedMod) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(mod_id) = &mod_info.mod_id {
        if !mod_id.is_empty() {
            keys.push(mod_id.clone());
        }
    }
    if let Some(project_id) = &mod_info.project_id {
        if !project_id.is_empty() && !keys.iter().any(|k| k == project_id) {
            keys.push(project_id.clone());
        }
    }
    if !mod_info.name.is_empty() {
        keys.push(mod_info.name.clone());
    }
    let stem = filename_stem(&mod_info.file_name);
    if !stem.is_empty() && !keys.iter().any(|k| k.eq_ignore_ascii_case(&stem)) {
        keys.push(stem);
    }
    keys
}

pub fn filename_stem(file_name: &str) -> String {
    let stem = file_name.trim_end_matches(".jar");
    if stem.is_empty() {
        return String::new();
    }
    // Drop common trailing MC / loader segments: mod-1.2.3+mc1.21.4, mod-fabric-1.21.4
    let trimmed = if let Ok(re) = regex::Regex::new(
        r"(?i)(\+mc[\d.]+|[-_](?:fabric|forge|neoforge|quilt)[-_]?[\d.]+|[-_]?mc?\d+\.\d+(?:\.\d+)?(?:\.\d+)?)$",
    ) {
        if let Some(m) = re.find(stem) {
            let t = stem[..m.start()].trim_end_matches('-').trim_end_matches('_');
            if !t.is_empty() {
                t.to_string()
            } else {
                stem.to_string()
            }
        } else {
            stem.to_string()
        }
    } else {
        stem.to_string()
    };
    // Drop trailing mod semver: carpet-1.4.141+v251113 -> carpet
    if let Ok(re) = regex::Regex::new(r"^(.+?)-\d[\d.]*(?:\+[^\s]+)?$") {
        if let Some(caps) = re.captures(&trimmed) {
            if let Some(name) = caps.get(1) {
                let candidate = name.as_str();
                if !candidate.is_empty() && candidate.len() < trimmed.len() {
                    return candidate.to_string();
                }
            }
        }
    }
    trimmed
}

fn loader_match_tier(version_loaders: &[String], target_loader: &str) -> u8 {
    let target = normalize_loader(target_loader);
    let mod_loaders: Vec<String> = version_loaders
        .iter()
        .map(|l| normalize_loader(l))
        .filter(|l| is_known_loader(l))
        .collect();
    if mod_loaders.is_empty() {
        return 2;
    }
    for vl in &mod_loaders {
        if *vl == target {
            return 0;
        }
    }
    if target == "quilt" && mod_loaders.iter().any(|vl| vl == "fabric") {
        return 1;
    }
    0
}

fn source_mc_line_bonus(game_versions: &[String], source_game_versions: &[String]) -> u8 {
    if source_game_versions.is_empty() || game_versions.is_empty() {
        return 1;
    }
    for sgv in source_game_versions {
        for gv in game_versions {
            if gv == sgv {
                return 0;
            }
            if crate::version::same_release_line(gv, sgv) {
                return 0;
            }
        }
    }
    1
}

fn version_label_tier(label: &str, source: Option<&str>) -> u8 {
    match source {
        Some(s) if labels_similar(label, s) => 0,
        Some(_) => 1,
        None => 1,
    }
}

fn labels_similar(a: &str, b: &str) -> bool {
    let a = a.trim().to_lowercase();
    let b = b.trim().to_lowercase();
    a == b || a.starts_with(&b) || b.starts_with(&a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_stem_strips_mc_suffix() {
        assert_eq!(
            filename_stem("sodium-fabric-mc1.21.4.jar"),
            "sodium-fabric"
        );
    }

    #[test]
    fn filename_stem_strips_mod_version_suffix() {
        assert_eq!(
            filename_stem("carpet-1.4.141+v251113.jar"),
            "carpet"
        );
    }

    #[test]
    fn older_listing_preferred_at_equal_mc() {
        let target = CompatTarget {
            mc_version: "1.21.4".into(),
            loader: "fabric".into(),
            loader_version: String::new(),
        };
        let gvs = vec!["1.21.4".into()];
        let loaders = vec!["fabric".into()];
        let newer = score_release(
            &gvs,
            &loaders,
            "1.0.0",
            &target,
            &PickContext {
                list_index: 0,
                channel: ReleaseChannel::Release,
                ..Default::default()
            },
        );
        let older = score_release(
            &gvs,
            &loaders,
            "1.0.0",
            &target,
            &PickContext {
                list_index: 5,
                channel: ReleaseChannel::Release,
                ..Default::default()
            },
        );
        assert!(older < newer);
    }

    #[test]
    fn downgrade_policy_caps_at_source_version() {
        use crate::version::parse_mod_version_label;

        let target = CompatTarget {
            mc_version: "1.21.4".into(),
            loader: "fabric".into(),
            loader_version: String::new(),
        };
        let gvs = vec!["1.21.4".into()];
        let loaders = vec!["fabric".into()];
        let source = parse_mod_version_label("1.2.0").unwrap();

        let newer = score_release(
            &gvs,
            &loaders,
            "1.3.0",
            &target,
            &PickContext {
                list_index: 0,
                channel: ReleaseChannel::Release,
                ..Default::default()
            },
        );
        let older = score_release(
            &gvs,
            &loaders,
            "1.1.0",
            &target,
            &PickContext {
                list_index: 1,
                channel: ReleaseChannel::Release,
                ..Default::default()
            },
        );

        let picked = pick_best_scored(
            vec![(newer, "1.3.0"), (older, "1.1.0")],
            |v| *v,
            Some("1.2.0"),
            ModVersionPolicy::Downgrade,
        );
        assert_eq!(picked, Some("1.1.0"));
        assert!(parse_mod_version_label(picked.unwrap()).unwrap() <= source);
    }

    #[test]
    fn exact_mc_beats_same_line() {
        let target = CompatTarget {
            mc_version: "1.21.4".into(),
            loader: "fabric".into(),
            loader_version: String::new(),
        };
        let loaders = vec!["fabric".into()];
        let exact = score_release(
            &["1.21.4".into()],
            &loaders,
            "v1",
            &target,
            &Default::default(),
        );
        let same_line = score_release(
            &["1.21.11".into()],
            &loaders,
            "v1",
            &target,
            &Default::default(),
        );
        assert!(exact < same_line);
    }

    #[test]
    fn local_compat_rejects_cross_patch_same_line() {
        let target = CompatTarget {
            mc_version: "1.21.11".into(),
            loader: "fabric".into(),
            loader_version: "0.19.2".into(),
        };
        let m = IdentifiedMod {
            file_name: "mod-1.21.6.jar".into(),
            file_path: "/mods/mod.jar".into(),
            sha512: String::new(),
            sha1: String::new(),
            fingerprint: 0,
            source: ModSource::Modrinth,
            project_id: Some("abc".into()),
            curseforge_id: None,
            name: "Test Mod".into(),
            name_zh: None,
            mod_id: None,
            current_version: Some("1.0.0+1.21.6".into()),
            loaders: vec!["fabric".into()],
            game_versions: vec!["1.21.6".into()],
            icon_url: None,
            github_url: None,
            depends: vec![],
        };
        assert!(!mod_locally_compatible(&m, &target));
    }

    #[test]
    fn local_compat_same_mc_and_loader() {
        let target = CompatTarget {
            mc_version: "1.21.4".into(),
            loader: "fabric".into(),
            loader_version: String::new(),
        };
        let m = IdentifiedMod {
            file_name: "sodium.jar".into(),
            file_path: "/mods/sodium.jar".into(),
            sha512: String::new(),
            sha1: String::new(),
            fingerprint: 0,
            source: ModSource::Modrinth,
            project_id: Some("AANobbMI".into()),
            curseforge_id: None,
            name: "Sodium".into(),
            name_zh: None,
            mod_id: None,
            current_version: Some("0.6.0".into()),
            loaders: vec!["fabric".into()],
            game_versions: vec!["1.21.4".into()],
            icon_url: None,
            github_url: None,
            depends: vec![],
        };
        assert!(mod_locally_compatible(&m, &target));
    }
}
