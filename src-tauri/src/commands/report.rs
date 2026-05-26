use std::fs;
use std::path::Path;

use crate::commands::conflicts::failure_suggestion;
use crate::models::{
    ModTransferItem, ModSource, TransferResult, TransferStatus,
};

pub fn generate_mod_report_md(
    items: &[ModTransferItem],
    result: Option<&TransferResult>,
    source_label: &str,
    target_label: &str,
    transferred_names: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("# Mod 迁移报告\n\n");
    out.push_str(&format!("- 源：{source_label}\n"));
    out.push_str(&format!("- 目标：{target_label}\n"));
    if let Some(r) = result {
        out.push_str(&format!(
            "- 结果：成功 {} / 失败 {} / 跳过 {}\n\n",
            r.success, r.failed, r.skipped
        ));
    } else {
        out.push('\n');
    }

    append_section_md(&mut out, "已成功迁移", &group_transferred(items, transferred_names));
    if let Some(r) = result {
        if !r.errors.is_empty() {
            out.push_str("\n## 迁移失败\n\n");
            for e in &r.errors {
                out.push_str(&format!("- {e}\n"));
            }
        }
    }
    append_section_md(&mut out, "未迁移 - 不兼容", &group_by_status_with_hint(items, TransferStatus::Incompatible, false));
    append_section_md(&mut out, "未迁移 - 已最新", &group_by_status(items, TransferStatus::UpToDate, false));
    append_section_md(&mut out, "未迁移 - 未识别", &group_by_status_with_hint(items, TransferStatus::Unknown, false));
    append_section_md(
        &mut out,
        "未勾选可迁移",
        &items
            .iter()
            .filter(|i| i.status == TransferStatus::Transferable && !i.selected)
            .map(format_item)
            .collect::<Vec<_>>(),
    );

    out
}

pub fn generate_mod_report_txt(
    items: &[ModTransferItem],
    result: Option<&TransferResult>,
    source_label: &str,
    target_label: &str,
    transferred_names: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("Mod 迁移报告\n");
    out.push_str("============\n");
    out.push_str(&format!("源：{source_label}\n"));
    out.push_str(&format!("目标：{target_label}\n"));
    if let Some(r) = result {
        out.push_str(&format!(
            "结果：成功 {} / 失败 {} / 跳过 {}\n\n",
            r.success, r.failed, r.skipped
        ));
    }
    append_section_txt(&mut out, "已成功迁移", &group_transferred(items, transferred_names));
    append_section_txt(
        &mut out,
        "未迁移-不兼容",
        &group_by_status_with_hint(items, TransferStatus::Incompatible, false),
    );
    append_section_txt(
        &mut out,
        "未迁移-已最新",
        &group_by_status(items, TransferStatus::UpToDate, false),
    );
    append_section_txt(
        &mut out,
        "未迁移-未识别",
        &group_by_status_with_hint(items, TransferStatus::Unknown, false),
    );
    append_section_txt(
        &mut out,
        "未勾选可迁移",
        &items
            .iter()
            .filter(|i| i.status == TransferStatus::Transferable && !i.selected)
            .map(format_item)
            .collect::<Vec<_>>(),
    );
    out
}

pub fn export_mod_report(
    path: &str,
    format: &str,
    items: &[ModTransferItem],
    result: Option<&TransferResult>,
    source_label: &str,
    target_label: &str,
    transferred_names: &[String],
) -> anyhow::Result<()> {
    let content = if format == "txt" {
        generate_mod_report_txt(items, result, source_label, target_label, transferred_names)
    } else {
        generate_mod_report_md(items, result, source_label, target_label, transferred_names)
    };
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

fn format_item(item: &ModTransferItem) -> String {
    let src_v = item
        .mod_info
        .current_version
        .as_deref()
        .unwrap_or("-");
    let tgt_v = item.target_version.as_deref().unwrap_or("-");
    let src_name = source_label(&item.download_source);
    format!(
        "{} ({}) {} -> {} [{}]",
        item.mod_info.name, item.mod_info.file_name, src_v, tgt_v, src_name
    )
}

fn format_item_with_suggestion(item: &ModTransferItem) -> String {
    let base = format_item(item);
    let hint = failure_suggestion(item.status.clone(), item.download_url.is_some());
    format!("{base} — {hint}")
}

fn source_label(s: &Option<ModSource>) -> &'static str {
    match s {
        Some(ModSource::Modrinth) => "Modrinth",
        Some(ModSource::Curseforge) => "CurseForge",
        Some(ModSource::Github) => "GitHub",
        Some(ModSource::Sgu) => "SGU 投影站",
        Some(ModSource::Metadata) => "本地",
        _ => "未知",
    }
}

fn group_transferred(items: &[ModTransferItem], names: &[String]) -> Vec<String> {
    if !names.is_empty() {
        return items
            .iter()
            .filter(|i| names.iter().any(|n| n == &i.mod_info.file_name || n == &i.mod_info.name))
            .map(format_item)
            .collect();
    }
    items
        .iter()
        .filter(|i| i.selected && i.status == TransferStatus::Transferable)
        .map(format_item)
        .collect()
}

fn group_by_status(
    items: &[ModTransferItem],
    status: TransferStatus,
    selected_only: bool,
) -> Vec<String> {
    items
        .iter()
        .filter(|i| i.status == status && (!selected_only || i.selected))
        .map(format_item)
        .collect()
}

fn group_by_status_with_hint(
    items: &[ModTransferItem],
    status: TransferStatus,
    selected_only: bool,
) -> Vec<String> {
    items
        .iter()
        .filter(|i| i.status == status && (!selected_only || i.selected))
        .map(format_item_with_suggestion)
        .collect()
}

fn append_section_md(out: &mut String, title: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    out.push_str(&format!("\n## {title}\n\n"));
    out.push_str("| Mod | 详情 |\n|-----|------|\n");
    for line in lines {
        let parts: Vec<_> = line.splitn(2, " (").collect();
        let name = parts.first().map(|s| s.to_string()).unwrap_or_else(|| line.clone());
        out.push_str(&format!("| {name} | {line} |\n"));
    }
}

fn append_section_txt(out: &mut String, title: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    out.push_str(&format!("\n[{title}]\n"));
    for line in lines {
        out.push_str(&format!("  - {line}\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::IdentifiedMod;

    fn sample_item(status: TransferStatus, selected: bool) -> ModTransferItem {
        ModTransferItem {
            mod_info: IdentifiedMod {
                file_name: "a.jar".into(),
                file_path: String::new(),
                sha512: String::new(),
                sha1: String::new(),
                fingerprint: 0,
                source: ModSource::Unknown,
                project_id: None,
                curseforge_id: None,
                name: "TestMod".into(),
                name_zh: None,
                mod_id: None,
                current_version: Some("1.0".into()),
                loaders: vec![],
                game_versions: vec![],
                icon_url: None,
                github_url: None,
                depends: vec![],
            },
            status,
            target_file_name: None,
            target_version: Some("2.0".into()),
            download_url: None,
            download_source: Some(ModSource::Modrinth),
            selected,
            is_dependency: false,
            required_by: None,
        }
    }

    #[test]
    fn report_contains_sections() {
        let items = vec![
            sample_item(TransferStatus::Transferable, true),
            sample_item(TransferStatus::Incompatible, false),
        ];
        let md = generate_mod_report_md(&items, None, "src", "tgt", &[]);
        assert!(md.contains("# Mod 迁移报告"));
        assert!(md.contains("未迁移 - 不兼容"));
    }
}
