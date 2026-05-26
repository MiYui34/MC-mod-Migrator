import { Badge } from "@/components/ui/badge";
import type { ModDiffEntry, ModDiffKind, ModDiffSummary } from "@/types";
import { cn } from "@/lib/utils";

const KIND_LABEL: Record<ModDiffKind, string> = {
  only_in_source: "仅源端",
  only_in_target: "仅目标端",
  version_mismatch: "版本不同",
  matched: "一致",
};

const KIND_VARIANT: Record<
  ModDiffKind,
  "success" | "secondary" | "destructive" | "warning"
> = {
  only_in_source: "warning",
  only_in_target: "secondary",
  version_mismatch: "warning",
  matched: "success",
};

interface ModDiffViewProps {
  entries: ModDiffEntry[];
  summary: ModDiffSummary;
  loading?: boolean;
  filter?: ModDiffKind | "all";
  onFilterChange?: (filter: ModDiffKind | "all") => void;
}

export function ModDiffView({
  entries,
  summary,
  loading,
  filter = "all",
  onFilterChange,
}: ModDiffViewProps) {
  const filtered =
    filter === "all" ? entries : entries.filter((e) => e.kind === filter);

  return (
    <div className="flex flex-col h-full">
      <div className="px-4 py-3 border-b border-[var(--color-border)] space-y-3">
        <div className="flex flex-wrap gap-2 text-xs">
          <SummaryChip label="仅源端" count={summary.onlyInSource} />
          <SummaryChip label="仅目标" count={summary.onlyInTarget} />
          <SummaryChip label="版本不同" count={summary.versionMismatch} />
          <SummaryChip label="一致" count={summary.matched} />
        </div>
        {onFilterChange && (
          <div className="flex gap-1 flex-wrap">
            {(["all", "only_in_source", "only_in_target", "version_mismatch", "matched"] as const).map(
              (f) => (
                <button
                  key={f}
                  type="button"
                  onClick={() => onFilterChange(f)}
                  className={cn(
                    "rounded-md border px-2 py-0.5 text-xs",
                    filter === f
                      ? "border-emerald-500 bg-emerald-500/10"
                      : "border-[var(--color-border)]"
                  )}
                >
                  {f === "all" ? "全部" : KIND_LABEL[f]}
                </button>
              )
            )}
          </div>
        )}
      </div>
      <div className="flex-1 overflow-y-auto">
        {loading ? (
          <p className="p-4 text-sm text-[var(--color-muted-foreground)]">正在对比…</p>
        ) : filtered.length === 0 ? (
          <p className="p-4 text-sm text-[var(--color-muted-foreground)]">暂无差异数据，请先扫描并对比</p>
        ) : (
          <table className="w-full text-sm">
            <thead className="sticky top-0 bg-[var(--color-card)] border-b border-[var(--color-border)]">
              <tr className="text-left text-xs text-[var(--color-muted-foreground)]">
                <th className="p-2 font-medium">类型</th>
                <th className="p-2 font-medium">Mod</th>
                <th className="p-2 font-medium">源版本</th>
                <th className="p-2 font-medium">目标版本</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((entry) => (
                <tr
                  key={`${entry.kind}-${entry.matchKey}-${entry.source?.fileName ?? entry.target?.fileName}`}
                  className="border-b border-[var(--color-border)]/50 hover:bg-[var(--color-muted)]/30"
                >
                  <td className="p-2">
                    <Badge variant={KIND_VARIANT[entry.kind]} className="text-[10px]">
                      {KIND_LABEL[entry.kind]}
                    </Badge>
                  </td>
                  <td className="p-2">
                    <div className="font-medium">
                      {entry.source?.name ?? entry.target?.name ?? "—"}
                    </div>
                    <div className="text-xs text-[var(--color-muted-foreground)] truncate max-w-[200px]">
                      {entry.source?.fileName ?? entry.target?.fileName}
                    </div>
                  </td>
                  <td className="p-2 text-xs text-[var(--color-muted-foreground)]">
                    {entry.source?.currentVersion ?? "—"}
                  </td>
                  <td className="p-2 text-xs text-[var(--color-muted-foreground)]">
                    {entry.target?.currentVersion ?? "—"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}

function SummaryChip({ label, count }: { label: string; count: number }) {
  return (
    <span className="rounded-md border border-[var(--color-border)] px-2 py-0.5">
      {label}: <strong>{count}</strong>
    </span>
  );
}
