import { X } from "lucide-react";
import { Progress } from "@/components/ui/progress";
import { Button } from "@/components/ui/button";
import type { TransferProgress } from "@/types";

interface TaskProgressBarProps {
  progress: TransferProgress | null;
  active: boolean;
  idleMessage?: string;
  onCancel?: () => void;
}

export function TaskProgressBar({
  progress,
  active,
  idleMessage = "处理中...",
  onCancel,
}: TaskProgressBarProps) {
  if (!active) return null;

  const percent = progress
    ? Math.round((progress.current / Math.max(progress.total, 1)) * 100)
    : 0;
    const indeterminate = Boolean(
    progress &&
      active &&
      (progress.total === 0 ||
        (progress.total > 0 &&
          progress.current === 0 &&
          !progress.message.includes("完成")))
  );

  return (
    <div className="border-t border-[var(--color-border)] bg-[var(--color-card)] p-4 space-y-2">
      <div className="flex justify-between items-center gap-3 text-sm">
        <span className="truncate">{progress?.message ?? (active ? idleMessage : "")}</span>
        <div className="flex items-center gap-2 shrink-0">
          {progress && progress.total > 0 && (
            <span className="text-[var(--color-muted-foreground)]">
              {progress.current}/{progress.total}
            </span>
          )}
          {onCancel && (
            <Button variant="ghost" size="sm" className="h-7 px-2" onClick={onCancel}>
              <X className="h-3.5 w-3.5" />
              取消
            </Button>
          )}
        </div>
      </div>
      {(progress && progress.total > 0) || indeterminate ? (
        indeterminate ? (
          <div className="relative h-2 w-full overflow-hidden rounded-full bg-[var(--color-secondary)]">
            <div className="absolute inset-y-0 w-1/3 bg-[var(--color-primary)] animate-pulse" />
          </div>
        ) : (
          progress && progress.total > 0 && <Progress value={percent} />
        )
      ) : null}
      {progress?.fileName && (
        <div className="text-xs text-[var(--color-muted-foreground)] truncate">
          {progress.fileName}
        </div>
      )}
    </div>
  );
}

interface TransferProgressBarProps {
  progress: TransferProgress | null;
  transferring: boolean;
  onCancel?: () => void;
}

export function TransferProgressBar({
  progress,
  transferring,
  onCancel,
}: TransferProgressBarProps) {
  return (
    <TaskProgressBar
      progress={progress}
      active={transferring}
      idleMessage="正在准备迁移..."
      onCancel={onCancel}
    />
  );
}

interface ScanProgressBarProps {
  progress: TransferProgress | null;
  scanning: boolean;
  onCancel?: () => void;
}

export function ScanProgressBar({ progress, scanning, onCancel }: ScanProgressBarProps) {
  return (
    <TaskProgressBar
      progress={progress}
      active={scanning}
      idleMessage="正在扫描..."
      onCancel={onCancel}
    />
  );
}

interface CheckProgressBarProps {
  progress: TransferProgress | null;
  checking: boolean;
  onCancel?: () => void;
}

export function CheckProgressBar({ progress, checking, onCancel }: CheckProgressBarProps) {
  return (
    <TaskProgressBar
      progress={progress}
      active={checking}
      idleMessage="正在检查兼容性..."
      onCancel={onCancel}
    />
  );
}
