/// Whether `target_mc` satisfies a Fabric-style dependency string (e.g. `>=1.21-`, `26.1-`).
pub fn game_version_meets_constraint(constraint: &str, target_mc: &str) -> bool {
    let target_mc = normalize_mc_version(target_mc);
    if target_mc.is_empty() || target_mc == "unknown" {
        return true;
    }
    let constraint = constraint.trim();
    if constraint.is_empty() || constraint == "*" {
        return true;
    }

    let (op, raw_ver) = parse_constraint_op(constraint);
    let required = fabric_version_to_semver(raw_ver.trim_end_matches('-').trim());
    let target = fabric_version_to_semver(&target_mc);

    let Ok(req_sem) = semver::Version::parse(&required) else {
        return true;
    };
    let Ok(tgt_sem) = semver::Version::parse(&target) else {
        return true;
    };

    match op {
        ">=" => tgt_sem >= req_sem,
        ">" => tgt_sem > req_sem,
        "<=" => tgt_sem <= req_sem,
        "<" => tgt_sem < req_sem,
        "=" | "==" => tgt_sem == req_sem,
        _ => true,
    }
}

fn parse_constraint_op(s: &str) -> (&'static str, &str) {
    for (op, len) in [(">=", 2), ("<=", 2), ("==", 2), (">", 1), ("<", 1), ("=", 1)] {
        if s.starts_with(op) {
            return (op, s[len..].trim());
        }
    }
    ("=", s.trim())
}

fn fabric_version_to_semver(v: &str) -> String {
    let v = normalize_mc_version(v);
    if let Some((a, b, c)) = parse_mc_version(&v) {
        return format!("{a}.{b}.{c}");
    }
    v
}

#[cfg(test)]
mod constraint_tests {
    use super::*;

    #[test]
    fn rejects_future_snapshot_on_12111() {
        assert!(!game_version_meets_constraint("26.1-", "1.21.11"));
    }

    #[test]
    fn accepts_same_line() {
        assert!(game_version_meets_constraint(">=1.21-", "1.21.11"));
    }
}

/// Parse "1.21.4" -> (1, 21, 4). Non-numeric suffixes are ignored.
pub fn parse_mc_version(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch_str = parts.next().unwrap_or("0");
    let patch: String = patch_str.chars().take_while(|c| c.is_ascii_digit()).collect();
    let patch = if patch.is_empty() {
        0
    } else {
        patch.parse().ok()?
    };
    Some((major, minor, patch))
}

/// Extract the last `1.x` or `1.x.y` substring from folder names / labels.
pub fn extract_mc_version(text: &str) -> Option<String> {
    let re = regex::Regex::new(r"1\.\d+(?:\.\d+)?").ok()?;
    re.find_iter(text).map(|m| m.as_str().to_string()).last()
}

/// Normalize user input or folder names like `1.21.11-Fabric 0.19.2` → `1.21.11`.
pub fn normalize_mc_version(raw: &str) -> String {
    if raw.is_empty() || raw == "unknown" {
        return raw.to_string();
    }
    extract_mc_version(raw).unwrap_or_else(|| raw.trim().to_string())
}

/// Same release line, e.g. 1.21.4 and 1.21.11 both belong to 1.21.x
pub fn same_release_line(a: &str, b: &str) -> bool {
    let a = normalize_mc_version(a);
    let b = normalize_mc_version(b);
    match (parse_mc_version(&a), parse_mc_version(&b)) {
        (Some((ma, mi, _)), Some((mb, mi2, _))) => ma == mb && mi == mi2,
        _ => false,
    }
}

/// Absolute patch distance within the same release line.
pub fn patch_distance(a: &str, b: &str) -> u32 {
    let a = normalize_mc_version(a);
    let b = normalize_mc_version(b);
    match (parse_mc_version(&a), parse_mc_version(&b)) {
        (Some((ma, mi, pa)), Some((mb, mi2, pb))) if ma == mb && mi == mi2 => pa.abs_diff(pb),
        _ => u32::MAX,
    }
}

/// How well a declared MC version matches the target instance. Lower is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionPickScore {
    pub tier: u8,
    pub patch_distance: u32,
}

impl PartialOrd for VersionPickScore {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for VersionPickScore {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.tier
            .cmp(&other.tier)
            .then(self.patch_distance.cmp(&other.patch_distance))
    }
}

/// Relaxed matching: same 1.x.y line is acceptable as long as the game can run.
///
/// Tier 0: exact MC version
/// Tier 1: same release line (any patch in 1.21.x)
/// Tier 2: broad minor tag like "1.21"
pub fn score_game_version_for_target(game_version: &str, target: &str) -> Option<VersionPickScore> {
    let target = normalize_mc_version(target);
    let game_version = normalize_mc_version(game_version);
    if target.is_empty() || target == "unknown" {
        return Some(VersionPickScore {
            tier: 0,
            patch_distance: 0,
        });
    }
    if game_version == target {
        return Some(VersionPickScore {
            tier: 0,
            patch_distance: 0,
        });
    }
    if same_release_line(&game_version, &target) {
        return Some(VersionPickScore {
            tier: 1,
            patch_distance: patch_distance(&game_version, &target),
        });
    }
    if parse_mc_version(&game_version).is_some() && game_version.matches('.').count() == 1 {
        let padded = format!("{game_version}.0");
        if same_release_line(&padded, &target) {
            return Some(VersionPickScore {
                tier: 2,
                patch_distance: patch_distance(&padded, &target),
            });
        }
    }
    None
}

pub fn best_version_pick_score(game_versions: &[String], target: &str) -> VersionPickScore {
    game_versions
        .iter()
        .filter_map(|gv| score_game_version_for_target(gv, target))
        .min()
        .unwrap_or(VersionPickScore {
            tier: u8::MAX,
            patch_distance: u32::MAX,
        })
}

use crate::models::IdentifiedMod;

/// Collect MC version hints from mod metadata, filename, and declared game versions.
pub fn effective_game_versions(mod_info: &IdentifiedMod) -> Vec<String> {
    if !mod_info.game_versions.is_empty() {
        return mod_info.game_versions.clone();
    }
    let mut gvs = Vec::new();
    if let Some(v) = &mod_info.current_version {
        if let Some(mc) = extract_mc_version(v) {
            gvs.push(mc);
        }
    }
    if let Some(mc) = extract_mc_version(&mod_info.file_name) {
        if !gvs.iter().any(|g| g == &mc) {
            gvs.push(mc);
        }
    }
    gvs
}

/// Whether a mod version's declared game version supports the target MC version.
pub fn game_version_supports_target(game_version: &str, target: &str) -> bool {
    score_game_version_for_target(game_version, target).is_some()
}

/// Exact MC version match (safe to copy a local jar without re-downloading).
pub fn game_version_exactly_matches_target(game_version: &str, target: &str) -> bool {
    normalize_mc_version(game_version) == normalize_mc_version(target)
}

/// Strict release compatibility for download selection.
///
/// Modrinth/CurseForge broad tags like `1.21` do **not** imply every 1.21.x patch
/// (Fabric `fabric.mod.json` often caps at 1.21.2 while the store tag says `1.21`).
/// Accept when:
/// - exact target patch tag present, or
/// - same release line and the highest declared patch is >= target patch.
pub fn release_supports_target_mc(game_versions: &[String], target: &str) -> bool {
    let target = normalize_mc_version(target);
    if target.is_empty() || target == "unknown" {
        return true;
    }
    if game_versions.is_empty() {
        return false;
    }

    for gv in game_versions {
        if normalize_mc_version(gv) == target {
            return true;
        }
    }

    let Some((_, _, target_patch)) = parse_mc_version(&target) else {
        return false;
    };
    let max_patch = game_versions
        .iter()
        .map(|gv| normalize_mc_version(gv))
        .filter(|gv| same_release_line(gv, &target))
        .filter_map(|gv| parse_mc_version(&gv).map(|(_, _, p)| p))
        .max();
    max_patch.is_some_and(|p| p >= target_patch)
}

pub fn is_known_loader(loader: &str) -> bool {
    matches!(
        normalize_loader(loader).as_str(),
        "fabric" | "forge" | "neoforge" | "quilt"
    )
}

/// Normalize loader strings like "Fabric 0.17.2" → "fabric".
pub fn normalize_loader(loader: &str) -> String {
    let lower = loader.trim().to_lowercase();
    if lower.is_empty() || lower == "unknown" {
        return lower;
    }
    if lower.contains("neoforge") {
        return "neoforge".to_string();
    }
    if lower.contains("fabric") {
        return "fabric".to_string();
    }
    if lower.contains("quilt") {
        return "quilt".to_string();
    }
    if lower.contains("forge") {
        return "forge".to_string();
    }
    lower
        .split_whitespace()
        .next()
        .unwrap_or(lower.as_str())
        .to_string()
}

/// Loaders to query on Modrinth when resolving versions for a target instance.
pub fn loader_query_tags(loader: &str) -> Vec<String> {
    let base = normalize_loader(loader);
    if !is_known_loader(&base) {
        return Vec::new();
    }
    let mut tags = vec![base.clone()];
    // Quilt can run Fabric mods; query both when targeting Quilt.
    if base == "quilt" {
        tags.push("fabric".into());
    }
    tags.sort();
    tags.dedup();
    tags
}

/// Whether a mod release's declared loaders fit the target instance loader.
/// Loader *versions* (e.g. Fabric Loader 0.16 vs 0.17) are not compared — only the family.
/// Tags like `minecraft` / `java` are ignored; only fabric/forge/neoforge/quilt count.
pub fn loader_matches_target(version_loaders: &[String], target_loader: &str) -> bool {
    let target = normalize_loader(target_loader);
    if !is_known_loader(&target) {
        return true;
    }
    let mod_loaders: Vec<String> = version_loaders
        .iter()
        .map(|l| normalize_loader(l))
        .filter(|l| is_known_loader(l))
        .collect();
    if mod_loaders.is_empty() {
        return true;
    }
    mod_loaders
        .iter()
        .any(|vl| loaders_compatible(vl, &target))
}

fn loaders_compatible(version_loader: &str, target: &str) -> bool {
    let vl = normalize_loader(version_loader);
    if vl == target {
        return true;
    }
    // Quilt instances can use Fabric-tagged mod builds.
    if target == "quilt" && vl == "fabric" {
        return true;
    }
    false
}

/// Substrings to look for in GitHub release asset names.
pub fn loader_name_tokens(loader: &str) -> Vec<String> {
    let base = normalize_loader(loader);
    if !is_known_loader(&base) {
        return Vec::new();
    }
    let mut tokens = vec![base.clone()];
    if base == "quilt" {
        tokens.push("fabric".into());
    }
    if base == "neoforge" {
        tokens.push("neoforge".into());
    }
    if base == "forge" {
        tokens.push("forge".into());
    }
    tokens.sort();
    tokens.dedup();
    tokens
}

pub fn asset_name_matches_loader(name_lower: &str, target_loader: &str) -> bool {
    let target = normalize_loader(target_loader);
    if !is_known_loader(&target) {
        return true;
    }
    loader_name_tokens(&target)
        .iter()
        .any(|token| name_lower.contains(token.as_str()))
}

pub fn compare_mod_release_candidates(
    score_a: VersionPickScore,
    score_b: VersionPickScore,
    label_a: &str,
    label_b: &str,
    source_label: Option<&str>,
) -> std::cmp::Ordering {
    score_a.cmp(&score_b).then_with(|| {
        prefer_same_mod_release(label_b, label_a, source_label)
    })
}

fn prefer_same_mod_release(a: &str, b: &str, source_label: Option<&str>) -> std::cmp::Ordering {
    let rank = |label: &str| {
        if source_label.is_some_and(|s| s == label) {
            0u8
        } else {
            1u8
        }
    };
    rank(a).cmp(&rank(b))
}

/// Parse mod semver from labels like `0.6.0+1.21.4`, `v1.2.3-mc1.21.1`.
pub fn parse_mod_version_label(label: &str) -> Option<semver::Version> {
    let trimmed = label.trim().trim_start_matches(['v', 'V']);
    let head = trimmed.split('+').next()?.split_whitespace().next()?;
    if let Ok(v) = semver::Version::parse(head) {
        return Some(v);
    }
    let core = head
        .split("-mc")
        .next()
        .and_then(|s| s.split("_mc").next())
        .unwrap_or(head);
    semver::Version::parse(core).ok()
}

pub fn mod_version_at_most(label: &str, source: &semver::Version) -> bool {
    parse_mod_version_label(label)
        .map(|v| v <= *source)
        .unwrap_or(true)
}

pub fn best_matching_game_version(candidates: &[String], target: &str) -> Option<String> {
    let mut best: Option<(VersionPickScore, String)> = None;
    for gv in candidates {
        if let Some(score) = score_game_version_for_target(gv, target) {
            if best.as_ref().is_none_or(|(s, _)| score < *s) {
                best = Some((score, gv.clone()));
            }
        }
    }
    best.map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_mc_from_folder_name() {
        assert_eq!(
            normalize_mc_version("1.21.11-Fabric 0.19.2"),
            "1.21.11"
        );
        assert_eq!(normalize_mc_version("1.21.6-Fabric 0.18.4"), "1.21.6");
    }

    #[test]
    fn same_line_1_21_6_to_1_21_11_relaxed_for_scoring() {
        assert!(game_version_supports_target("1.21.6", "1.21.11"));
        assert!(game_version_supports_target("1.21.6", "1.21.11-Fabric 0.19.2"));
    }

    #[test]
    fn release_support_strict_for_cross_patch() {
        assert!(!release_supports_target_mc(&["1.21.6".into()], "1.21.11"));
        assert!(!release_supports_target_mc(&["1.21.1".into()], "1.21.11"));
        assert!(!release_supports_target_mc(&["1.21".into()], "1.21.11"));
        assert!(release_supports_target_mc(&["1.21.11".into()], "1.21.11"));
        assert!(release_supports_target_mc(&["1.21.6".into()], "1.21.6"));
        assert!(release_supports_target_mc(
            &["1.21.6".into(), "1.21.11".into()],
            "1.21.11"
        ));
        assert!(release_supports_target_mc(
            &["1.21.10".into(), "1.21.11".into()],
            "1.21.11"
        ));
    }

    #[test]
    fn exact_match_for_local_copy() {
        assert!(!game_version_exactly_matches_target("1.21.6", "1.21.11"));
        assert!(game_version_exactly_matches_target("1.21.11", "1.21.11-Fabric 0.19.2"));
    }

    #[test]
    fn loader_ignores_minecraft_tag() {
        assert!(loader_matches_target(
            &["minecraft".into(), "java".into()],
            "fabric"
        ));
        assert!(loader_matches_target(&["fabric".into(), "minecraft".into()], "fabric"));
    }

    #[test]
    fn same_release_line_works() {
        assert!(same_release_line("1.21.4", "1.21.11"));
        assert!(!same_release_line("1.20.4", "1.21.4"));
    }

    #[test]
    fn exact_beats_same_line() {
        let exact = score_game_version_for_target("1.21.4", "1.21.4").unwrap();
        let same_line = score_game_version_for_target("1.21.11", "1.21.4").unwrap();
        assert!(exact < same_line);
        assert_eq!(same_line.tier, 1);
    }

    #[test]
    fn normalize_loader_strips_version() {
        assert_eq!(normalize_loader("Fabric 0.17.2"), "fabric");
        assert_eq!(normalize_loader("1.21.8-Fabric 0.17.2"), "fabric");
    }

    #[test]
    fn quilt_accepts_fabric_mod_loaders() {
        assert!(loader_matches_target(&["fabric".into()], "quilt"));
        assert!(!loader_matches_target(&["quilt".into()], "fabric"));
    }
}
