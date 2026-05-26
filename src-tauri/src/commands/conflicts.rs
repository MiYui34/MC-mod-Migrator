use std::collections::HashMap;

use crate::commands::target_mods::build_target_mod_id_index;
use crate::jar::parse_jar_metadata;
use crate::models::{
    CrossVersionChecklistItem, CrossVersionGuide, IdentifiedMod, MigrationWarning,
    MigrationWarningItem, ModTransferItem, TargetEnv, TransferStatus,
};
use crate::version::{normalize_loader, normalize_mc_version, parse_mc_version};

pub fn detect_migration_warnings(
    source_mods: &[IdentifiedMod],
    target_mods_path: &str,
    target: &TargetEnv,
    items: &[ModTransferItem],
) -> Vec<MigrationWarning> {
    let mut warnings = Vec::new();

    if let Some(w) = detect_duplicate_mod_ids(target_mods_path) {
        warnings.push(w);
    }
    warnings.extend(detect_loader_mix(target_mods_path, &target.loader));
    if let Some(w) = detect_duplicate_project_ids(source_mods, "源实例") {
        warnings.push(w);
    }
    warnings.extend(detect_incompatible_summary(items));

    warnings
}

fn detect_duplicate_mod_ids(mods_path: &str) -> Option<MigrationWarning> {
    let index = build_target_mod_id_index(mods_path);
    let mut items = Vec::new();
    for (mod_id, files) in index {
        if files.len() > 1 {
            items.push(MigrationWarningItem {
                context: mod_id,
                title: None,
                files,
            });
        }
    }
    if items.is_empty() {
        return None;
    }
    let count = items.len() as u32;
    Some(MigrationWarning {
        code: "duplicate_mod_id".into(),
        severity: "warning".into(),
        message: format!("目标 mods 文件夹中有 {count} 个 Mod ID 存在重复 jar"),
        count: Some(count),
        items,
    })
}

fn detect_loader_mix(mods_path: &str, target_loader: &str) -> Vec<MigrationWarning> {
    let target_loader = normalize_loader(target_loader);
    if target_loader.is_empty() || target_loader == "unknown" {
        return Vec::new();
    }

    let mut fabric = 0u32;
    let mut forge = 0u32;
    let mut neo = 0u32;
    let dir = std::path::Path::new(mods_path);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !name.ends_with(".jar") {
            continue;
        }
        if let Ok(meta) = parse_jar_metadata(&path) {
            if let Some(hint) = meta.loader_hint.as_deref() {
                let l = normalize_loader(hint);
                if l.contains("fabric") {
                    fabric += 1;
                } else if l.contains("neoforge") {
                    neo += 1;
                } else if l.contains("forge") {
                    forge += 1;
                }
            }
        }
    }

    let mut present = Vec::new();
    if fabric > 0 {
        present.push(format!("Fabric ({fabric})"));
    }
    if forge > 0 {
        present.push(format!("Forge ({forge})"));
    }
    if neo > 0 {
        present.push(format!("NeoForge ({neo})"));
    }

    if present.len() <= 1 {
        return Vec::new();
    }

    vec![MigrationWarning {
        code: "loader_mix".into(),
        severity: "error".into(),
        message: format!(
            "目标 mods 文件夹检测到多种加载器混装：{}。目标加载器为 {target_loader}，可能导致崩溃",
            present.join("、")
        ),
        count: None,
        items: Vec::new(),
    }]
}

fn mod_display_name(m: &IdentifiedMod) -> Option<String> {
    if let Some(zh) = m.name_zh.as_ref().filter(|s| !s.is_empty()) {
        return Some(zh.clone());
    }
    if !m.name.trim().is_empty() {
        return Some(m.name.clone());
    }
    None
}

fn detect_duplicate_project_ids(mods: &[IdentifiedMod], label: &str) -> Option<MigrationWarning> {
    let mut by_project: HashMap<String, (Option<String>, Vec<String>)> = HashMap::new();
    for m in mods {
        let title = mod_display_name(m);
        if let Some(pid) = m.project_id.as_ref().filter(|s| !s.is_empty()) {
            let entry = by_project.entry(pid.clone()).or_default();
            if entry.0.is_none() {
                entry.0 = title.clone();
            }
            entry.1.push(m.file_name.clone());
        }
        if let Some(cf) = m.curseforge_id {
            let key = format!("cf:{cf}");
            let entry = by_project.entry(key).or_default();
            if entry.0.is_none() {
                entry.0 = title;
            }
            entry.1.push(m.file_name.clone());
        }
    }

    let mut items = Vec::new();
    for (key, (title, files)) in by_project {
        if files.len() > 1 {
            items.push(MigrationWarningItem {
                context: key,
                title,
                files,
            });
        }
    }
    if items.is_empty() {
        return None;
    }
    items.sort_by(|a, b| {
        a.title
            .as_deref()
            .unwrap_or(&a.context)
            .cmp(b.title.as_deref().unwrap_or(&b.context))
    });
    let count = items.len() as u32;
    Some(MigrationWarning {
        code: "duplicate_project".into(),
        severity: "warning".into(),
        message: format!(
            "{label} 中有 {count} 个 Mod 项目存在重复 jar（常见于中英文双文件名，迁移前建议只保留一份）"
        ),
        count: Some(count),
        items,
    })
}

fn detect_incompatible_summary(items: &[ModTransferItem]) -> Vec<MigrationWarning> {
    let incompatible = items
        .iter()
        .filter(|i| i.status == TransferStatus::Incompatible)
        .count();
    if incompatible == 0 {
        return Vec::new();
    }
    vec![MigrationWarning {
        code: "incompatible_mods".into(),
        severity: "warning".into(),
        message: format!("有 {incompatible} 个 Mod 在目标端不兼容，迁移前请检查加载器与 MC 版本"),
        count: Some(incompatible as u32),
        items: Vec::new(),
    }]
}

pub fn build_cross_version_guide(
    source_mc: &str,
    target_mc: &str,
    items: &[ModTransferItem],
) -> Option<CrossVersionGuide> {
    let source_mc = normalize_mc_version(source_mc);
    let target_mc = normalize_mc_version(target_mc);
    if source_mc.is_empty()
        || target_mc.is_empty()
        || source_mc == "unknown"
        || target_mc == "unknown"
    {
        return None;
    }

    let (Some((s_maj, s_min, _)), Some((t_maj, t_min, _))) =
        (parse_mc_version(&source_mc), parse_mc_version(&target_mc))
    else {
        return None;
    };

    let major_jump = s_maj != t_maj;
    let minor_jump = s_maj == t_maj && s_min != t_min;
    if !major_jump && !minor_jump && source_mc == target_mc {
        return None;
    }

    let incompatible = items
        .iter()
        .filter(|i| i.status == TransferStatus::Incompatible)
        .count();
    let unknown = items
        .iter()
        .filter(|i| i.status == TransferStatus::Unknown)
        .count();
    let transferable = items
        .iter()
        .filter(|i| i.status == TransferStatus::Transferable)
        .count();

    let mut checklist = vec![
        CrossVersionChecklistItem {
            id: "check_loaders".into(),
            title: "确认目标加载器".into(),
            description: "跨版本迁移时 Fabric/Forge/NeoForge 通常不能互换，请在左侧手动确认目标加载器".into(),
            required: true,
        },
        CrossVersionChecklistItem {
            id: "migrate_mods_first".into(),
            title: "先迁移 Mod，再迁移配置".into(),
            description: "建议先完成 Mod 迁移并通过兼容性检查，再迁移 Mod 配置与游戏设置".into(),
            required: major_jump,
        },
        CrossVersionChecklistItem {
            id: "review_incompatible".into(),
            title: format!("检查 {incompatible} 个不兼容 Mod"),
            description: "不兼容 Mod 需手动换版本或从市场重新安装".into(),
            required: incompatible > 0,
        },
    ];

    if unknown > 0 {
        checklist.push(CrossVersionChecklistItem {
            id: "review_unknown".into(),
            title: format!("处理 {unknown} 个未识别 Mod"),
            description: "未识别 Mod 无法自动查询远程版本，建议手动确认或移除".into(),
            required: false,
        });
    }

    if major_jump {
        checklist.push(CrossVersionChecklistItem {
            id: "backup_world".into(),
            title: "备份存档与世界".into(),
            description: "大版本跨越可能导致存档不兼容，请先备份 .minecraft/saves".into(),
            required: true,
        });
    }

    Some(CrossVersionGuide {
        source_mc: source_mc.clone(),
        target_mc: target_mc.clone(),
        major_version_change: major_jump,
        incompatible_count: incompatible as u32,
        transferable_count: transferable as u32,
        checklist,
    })
}

pub fn failure_suggestion(status: TransferStatus, has_download_url: bool) -> &'static str {
    match status {
        TransferStatus::Incompatible if !has_download_url => {
            "建议：在市场中搜索该 Mod 手动安装，或更换目标 MC/加载器"
        }
        TransferStatus::Incompatible => "建议：点击版本选择器尝试其他版本，或切换 Mod 版本策略为「匹配源版本」",
        TransferStatus::Unknown => "建议：确认 jar 完整且来源可识别，或从市场重新下载",
        TransferStatus::UpToDate => "目标端已有所需版本，无需操作",
        TransferStatus::Transferable => "可重新勾选并迁移",
    }
}
