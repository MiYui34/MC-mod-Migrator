import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import type { MigrationRecord } from "@/types";
import { CATEGORY_LABELS, type MigrationCategory } from "@/types";

interface MigrationHistoryDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  records: MigrationRecord[];
  onRefresh: () => void;
  onDelete: (id: string) => void;
  onOpenBackup: (backupId: string) => void;
  onRestore: (record: MigrationRecord) => void;
  onReEdit: (record: MigrationRecord) => void;
  restoringId?: string | null;
}

function formatTime(ts: string) {
  const n = Number(ts);
  if (!Number.isNaN(n) && n > 1_000_000_000) {
    return new Date(n * 1000).toLocaleString();
  }
  return ts;
}

function categoryLabel(category: string) {
  if (category in CATEGORY_LABELS) {
    return CATEGORY_LABELS[category as MigrationCategory];
  }
  return category;
}

export function MigrationHistoryDialog({
  open,
  onOpenChange,
  records,
  onRefresh,
  onDelete,
  onOpenBackup,
  onRestore,
  onReEdit,
  restoringId,
}: MigrationHistoryDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[min(85dvh,calc(100vh-2rem))] max-w-2xl flex-col">
        <DialogHeader>
          <DialogTitle>迁移历史</DialogTitle>
          <p className="text-xs text-[var(--color-muted-foreground)]">
            可撤销已开启「迁移前备份」的记录：还原被覆盖文件，并删除本次新添加的文件。
          </p>
        </DialogHeader>
        <DialogBody className="space-y-2 py-1">
          {records.length === 0 && (
            <p className="text-sm text-[var(--color-muted-foreground)] text-center py-8">
              暂无迁移记录
            </p>
          )}
          {records.map((r) => (
            <div
              key={r.id}
              className="rounded-md border border-[var(--color-border)] p-3 text-sm space-y-1"
            >
              <div className="flex justify-between gap-2">
                <span className="font-medium">
                  {r.sourceName} → {r.targetName}
                </span>
                <span className="text-[var(--color-muted-foreground)] text-xs">
                  {formatTime(r.timestamp)}
                </span>
              </div>
              <div className="text-[var(--color-muted-foreground)]">
                {r.sourceMc} → {r.targetMc} · {categoryLabel(r.category)}
              </div>
              <div>
                成功 {r.success} / 失败 {r.failed} / 跳过 {r.skipped}
              </div>
              <div className="flex flex-nowrap gap-2 overflow-x-auto pt-1 [&_button]:shrink-0">
                <Button
                  variant="outline"
                  size="sm"
                  className="whitespace-nowrap"
                  disabled={!r.backupId || restoringId === r.id}
                  title={
                    r.backupId
                      ? "撤销此次迁移"
                      : "未开启迁移前备份，无法撤销"
                  }
                  onClick={() => onRestore(r)}
                >
                  {restoringId === r.id ? "撤销中…" : "撤销操作"}
                </Button>
                {r.backupId && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="whitespace-nowrap"
                    onClick={() => onOpenBackup(r.backupId!)}
                  >
                    打开备份
                  </Button>
                )}
                <Button
                  variant="outline"
                  size="sm"
                  className="whitespace-nowrap"
                  onClick={() => onReEdit(r)}
                >
                  重新编辑
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  className="whitespace-nowrap"
                  onClick={() => onDelete(r.id)}
                >
                  删除记录
                </Button>
              </div>
            </div>
          ))}
        </DialogBody>
        <DialogFooter>
          <Button variant="outline" size="sm" onClick={onRefresh}>
            刷新
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
