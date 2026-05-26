use std::collections::{HashMap, HashSet};

use crate::models::{IdentifiedMod, ModDiffEntry, ModDiffKind, ModDiffResult, ModDiffSummary};

pub fn mod_match_key(mod_info: &IdentifiedMod) -> String {
    if let Some(pid) = mod_info.project_id.as_ref().filter(|s| !s.is_empty()) {
        return format!("mr:{pid}");
    }
    if let Some(cf) = mod_info.curseforge_id {
        return format!("cf:{cf}");
    }
    if let Some(mid) = mod_info.mod_id.as_ref().filter(|s| !s.is_empty()) {
        return format!("id:{}", mid.to_lowercase());
    }
    format!("name:{}", normalize_name(&mod_info.name))
}

fn normalize_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn versions_differ(a: &IdentifiedMod, b: &IdentifiedMod) -> bool {
    if a.sha512 == b.sha512 {
        return false;
    }
    if let (Some(va), Some(vb)) = (&a.current_version, &b.current_version) {
        if va != vb {
            return true;
        }
    }
    a.file_name != b.file_name
}

pub fn compare_mod_lists(source: &[IdentifiedMod], target: &[IdentifiedMod]) -> ModDiffResult {
    let mut target_by_key: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, m) in target.iter().enumerate() {
        target_by_key.entry(mod_match_key(m)).or_default().push(i);
    }

    let mut matched_target = HashSet::new();
    let mut entries = Vec::new();

    for src in source {
        let key = mod_match_key(src);
        if let Some(indices) = target_by_key.get(&key) {
            if let Some(&ti) = indices.iter().find(|i| !matched_target.contains(*i)) {
                matched_target.insert(ti);
                let tgt = &target[ti];
                let kind = if versions_differ(src, tgt) {
                    ModDiffKind::VersionMismatch
                } else {
                    ModDiffKind::Matched
                };
                entries.push(ModDiffEntry {
                    kind,
                    match_key: key,
                    source: Some(src.clone()),
                    target: Some(tgt.clone()),
                });
                continue;
            }
        }
        entries.push(ModDiffEntry {
            kind: ModDiffKind::OnlyInSource,
            match_key: key,
            source: Some(src.clone()),
            target: None,
        });
    }

    for (i, tgt) in target.iter().enumerate() {
        if matched_target.contains(&i) {
            continue;
        }
        entries.push(ModDiffEntry {
            kind: ModDiffKind::OnlyInTarget,
            match_key: mod_match_key(tgt),
            source: None,
            target: Some(tgt.clone()),
        });
    }

    let summary = summarize(&entries);
    ModDiffResult { entries, summary }
}

fn summarize(entries: &[ModDiffEntry]) -> ModDiffSummary {
    let mut summary = ModDiffSummary::default();
    for e in entries {
        match e.kind {
            ModDiffKind::OnlyInSource => summary.only_in_source += 1,
            ModDiffKind::OnlyInTarget => summary.only_in_target += 1,
            ModDiffKind::VersionMismatch => summary.version_mismatch += 1,
            ModDiffKind::Matched => summary.matched += 1,
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ModSource;

    fn sample(name: &str, project_id: Option<&str>, version: &str) -> IdentifiedMod {
        IdentifiedMod {
            file_name: format!("{name}-{version}.jar"),
            file_path: format!("/mods/{name}-{version}.jar"),
            sha512: format!("sha-{name}-{version}"),
            sha1: String::new(),
            fingerprint: 0,
            source: ModSource::Modrinth,
            project_id: project_id.map(str::to_string),
            curseforge_id: None,
            name: name.to_string(),
            name_zh: None,
            mod_id: None,
            current_version: Some(version.to_string()),
            loaders: vec!["fabric".into()],
            game_versions: vec!["1.21.1".into()],
            icon_url: None,
            github_url: None,
            depends: vec![],
        }
    }

    #[test]
    fn detects_only_in_source_and_mismatch() {
        let source = vec![
            sample("sodium", Some("AANobbMI"), "0.5.0"),
            sample("lithium", Some("gvQqBUqZ"), "0.11.0"),
        ];
        let target = vec![
            sample("sodium", Some("AANobbMI"), "0.5.1"),
            sample("iris", Some("YL57xq9U"), "1.6.0"),
        ];
        let result = compare_mod_lists(&source, &target);
        assert_eq!(result.summary.only_in_source, 1);
        assert_eq!(result.summary.only_in_target, 1);
        assert_eq!(result.summary.version_mismatch, 1);
        assert_eq!(result.summary.matched, 0);
    }
}
