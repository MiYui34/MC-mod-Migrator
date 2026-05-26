import { useMemo, useState } from "react";
import {
  AlertTriangle,
  ChevronDown,
  ChevronRight,
  Copy,
  Info,
  ShieldAlert,
  X,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import type { CrossVersionGuide, MigrationWarning } from "@/types";
import { cn } from "@/lib/utils";

interface MigrationAlertsProps {
  warnings: MigrationWarning[];
  crossVersionGuide: CrossVersionGuide | null;
  onDismissGuide?: () => void;
  onDismiss?: () => void;
  className?: string;
}

const CODE_META: Record<
  string,
  { label: string; hint?: string }
> = {
  duplicate_project: {
    label: "重复项目",
    hint: "同一 Mod 的中英文文件名各存一份，建议迁移前删除多余 jar",
  },
  duplicate_mod_id: {
    label: "Mod ID 冲突",
    hint: "同一 Mod ID 对应多个 jar，可能导致加载异常",
  },
  incompatible_mods: {
    label: "不兼容",
    hint: "可在列表中筛选「不兼容」状态，尝试换版本或从市场安装",
  },
  loader_mix: {
    label: "加载器混装",
    hint: "请清理与目标加载器不符的 jar",
  },
};

function severityStyles(severity: string) {
  if (severity === "error") {
    return {
      panel: "border-red-500/30 bg-red-500/5",
      badge: "bg-red-500/15 text-red-300 border-red-500/30",
      icon: "text-red-400",
    };
  }
  return {
    panel: "border-amber-500/30 bg-amber-500/5",
    badge: "bg-amber-500/15 text-amber-200 border-amber-500/30",
    icon: "text-amber-400",
  };
}

function looksLikeMirrorPair(files: string[]): boolean {
  if (files.length !== 2) return false;
  const hasBracket = files.some((f) => /^\[.+?\]/.test(f));
  const hasPlain = files.some((f) => !/^\[.+?\]/.test(f));
  return hasBracket && hasPlain;
}

function WarningSection({
  warning,
  defaultOpen,
}: {
  warning: MigrationWarning;
  defaultOpen: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const meta = CODE_META[warning.code] ?? { label: warning.code };
  const styles = severityStyles(warning.severity);
  const items = warning.items ?? [];
  const hasItems = items.length > 0;

  return (
    <div className={cn("rounded-lg border overflow-hidden", styles.panel)}>
      <button
        type="button"
        onClick={() => hasItems && setOpen((v) => !v)}
        className={cn(
          "w-full flex items-start gap-2 px-3 py-2.5 text-left text-sm",
          hasItems && "hover:bg-black/10 cursor-pointer",
          !hasItems && "cursor-default"
        )}
      >
        {hasItems ? (
          open ? (
            <ChevronDown className={cn("h-4 w-4 shrink-0 mt-0.5", styles.icon)} />
          ) : (
            <ChevronRight className={cn("h-4 w-4 shrink-0 mt-0.5", styles.icon)} />
          )
        ) : (
          <AlertTriangle className={cn("h-4 w-4 shrink-0 mt-0.5", styles.icon)} />
        )}
        <div className="flex-1 min-w-0 space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <span
              className={cn(
                "inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-medium",
                styles.badge
              )}
            >
              {meta.label}
              {warning.count != null ? ` · ${warning.count}` : ""}
            </span>
            <span className="text-[var(--color-foreground)] leading-snug">{warning.message}</span>
          </div>
          {meta.hint && (
            <p className="text-xs text-[var(--color-muted-foreground)] leading-relaxed">
              {meta.hint}
            </p>
          )}
        </div>
      </button>

      {hasItems && open && (
        <div className="border-t border-[var(--color-border)]/60 max-h-52 overflow-y-auto">
          <table className="w-full text-xs">
            <thead className="sticky top-0 bg-[var(--color-background)]/95 backdrop-blur-sm">
              <tr className="text-[var(--color-muted-foreground)]">
                <th className="text-left font-medium px-3 py-1.5 w-[40%]">Mod</th>
                <th className="text-left font-medium px-3 py-1.5">重复文件</th>
              </tr>
            </thead>
            <tbody>
              {items.map((item) => (
                <tr
                  key={`${item.context}-${item.files.join("|")}`}
                  className="border-t border-[var(--color-border)]/40 align-top"
                >
                  <td className="px-3 py-2">
                    <div className="font-medium text-[var(--color-foreground)] truncate max-w-[14rem]">
                      {item.title ?? item.context}
                    </div>
                    {item.title && (
                      <div className="text-[10px] text-[var(--color-muted-foreground)] truncate max-w-[14rem]">
                        {item.context}
                      </div>
                    )}
                  </td>
                  <td className="px-3 py-2">
                    <ul className="space-y-1">
                      {item.files.map((file) => (
                        <li
                          key={file}
                          className="flex items-start gap-1.5 text-[var(--color-muted-foreground)]"
                        >
                          <Copy className="h-3 w-3 shrink-0 mt-0.5 opacity-60" />
                          <span className="break-all leading-relaxed">{file}</span>
                        </li>
                      ))}
                    </ul>
                    {looksLikeMirrorPair(item.files) && (
                      <span className="inline-block mt-1 rounded bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-300">
                        疑似中英文镜像重复
                      </span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

export function MigrationAlerts({
  warnings,
  crossVersionGuide,
  onDismissGuide,
  onDismiss,
  className,
}: MigrationAlertsProps) {
  const [collapsed, setCollapsed] = useState(false);

  const summary = useMemo(() => {
    const errorCount = warnings.filter((w) => w.severity === "error").length;
    const warningCount = warnings.filter((w) => w.severity !== "error").length;
    const duplicateProjects = warnings.find((w) => w.code === "duplicate_project")?.count ?? 0;
    const incompatible = warnings.find((w) => w.code === "incompatible_mods")?.count ?? 0;
    return { errorCount, warningCount, duplicateProjects, incompatible };
  }, [warnings]);

  if (warnings.length === 0 && !crossVersionGuide) return null;

  const hasMany = warnings.length > 1 || (warnings[0]?.items?.length ?? 0) > 3;

  return (
    <div className={cn("shrink-0 border-b border-[var(--color-border)] bg-[var(--color-muted)]/20", className)}>
      <div className="flex items-center justify-between gap-2 px-4 py-2">
        <div className="flex items-center gap-2 min-w-0">
          <ShieldAlert className="h-4 w-4 shrink-0 text-amber-400" />
          <span className="text-sm font-medium shrink-0">迁移检查</span>
          <div className="flex flex-wrap items-center gap-1.5 min-w-0">
            {summary.duplicateProjects > 0 && (
              <span className="rounded-full bg-amber-500/15 border border-amber-500/25 px-2 py-0.5 text-[11px] text-amber-200">
                {summary.duplicateProjects} 重复项目
              </span>
            )}
            {summary.incompatible > 0 && (
              <span className="rounded-full bg-amber-500/15 border border-amber-500/25 px-2 py-0.5 text-[11px] text-amber-200">
                {summary.incompatible} 不兼容
              </span>
            )}
            {summary.errorCount > 0 && (
              <span className="rounded-full bg-red-500/15 border border-red-500/25 px-2 py-0.5 text-[11px] text-red-300">
                {summary.errorCount} 严重
              </span>
            )}
            {summary.warningCount > 0 && summary.duplicateProjects === 0 && summary.incompatible === 0 && (
              <span className="text-xs text-[var(--color-muted-foreground)]">
                {summary.warningCount} 项提醒
              </span>
            )}
          </div>
        </div>
        <div className="flex items-center gap-1 shrink-0">
          {hasMany && (
            <Button
              variant="ghost"
              size="sm"
              className="h-7 text-xs px-2"
              onClick={() => setCollapsed((v) => !v)}
            >
              {collapsed ? "展开详情" : "收起"}
            </Button>
          )}
          {onDismiss && (
            <Button
              variant="ghost"
              size="sm"
              className="h-7 w-7 p-0"
              onClick={onDismiss}
              title="关闭提醒"
            >
              <X className="h-3.5 w-3.5" />
            </Button>
          )}
        </div>
      </div>

      {!collapsed && (
        <div className="px-4 pb-3 space-y-2">
          {warnings.map((w) => (
            <WarningSection
              key={w.code}
              warning={w}
              defaultOpen={w.severity === "error" || w.code === "incompatible_mods"}
            />
          ))}

          {crossVersionGuide && (
            <div className="rounded-lg border border-sky-500/30 bg-sky-500/5 overflow-hidden">
              <div className="flex items-start justify-between gap-2 px-3 py-2.5">
                <div className="flex gap-2 items-start min-w-0">
                  <Info className="h-4 w-4 shrink-0 mt-0.5 text-sky-300" />
                  <div className="min-w-0">
                    <p className="text-sm font-medium text-sky-200">
                      跨版本向导 · {crossVersionGuide.sourceMc} → {crossVersionGuide.targetMc}
                      {crossVersionGuide.majorVersionChange ? "（大版本跨越）" : ""}
                    </p>
                    <p className="text-xs text-[var(--color-muted-foreground)] mt-0.5">
                      可迁移 {crossVersionGuide.transferableCount} · 不兼容{" "}
                      {crossVersionGuide.incompatibleCount}
                    </p>
                  </div>
                </div>
                {onDismissGuide && (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 text-xs shrink-0"
                    onClick={onDismissGuide}
                  >
                    关闭
                  </Button>
                )}
              </div>
              <ul className="border-t border-sky-500/20 px-3 py-2 space-y-1 text-xs text-[var(--color-muted-foreground)] max-h-32 overflow-y-auto">
                {crossVersionGuide.checklist.map((item) => (
                  <li key={item.id} className="flex gap-1.5 leading-relaxed">
                    <span className={item.required ? "text-sky-200" : ""}>
                      {item.required ? "●" : "○"}
                    </span>
                    <span>
                      <span className={cn("font-medium", item.required && "text-sky-100")}>
                        {item.title}
                      </span>
                      {" — "}
                      {item.description}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
