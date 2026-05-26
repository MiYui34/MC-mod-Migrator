import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import type { ConflictPolicy, FileAssetTransferItem } from "@/types";
import { useState } from "react";

interface AssetTransferPreviewProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  items: FileAssetTransferItem[];
  categoryLabel: string;
  onConfirm: (policy: ConflictPolicy) => void;
  transferring?: boolean;
}

export function AssetTransferPreview({
  open,
  onOpenChange,
  items,
  categoryLabel,
  onConfirm,
  transferring,
}: AssetTransferPreviewProps) {
  const [policy, setPolicy] = useState<ConflictPolicy>("overwrite");
  const selected = items.filter(
    (i) =>
      i.selected &&
      i.status !== "up_to_date" &&
      i.status !== "incompatible"
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>确认迁移 — {categoryLabel}</DialogTitle>
        </DialogHeader>
        <div className="min-h-0 space-y-3 overflow-y-auto overscroll-contain">
          <p className="text-sm text-[var(--color-muted-foreground)]">
            将迁移 {selected.length} 项
          </p>
          <div className="space-y-2 text-sm">
            <p className="font-medium">冲突策略</p>
            <label className="flex items-center gap-2 whitespace-nowrap">
              <input
                type="radio"
                checked={policy === "overwrite"}
                onChange={() => setPolicy("overwrite")}
              />
              覆盖目标已有文件
            </label>
            <label className="flex items-center gap-2 whitespace-nowrap">
              <input
                type="radio"
                checked={policy === "skip"}
                onChange={() => setPolicy("skip")}
              />
              跳过冲突项
            </label>
            <p className="pt-2 text-xs text-[var(--color-muted-foreground)]">
              若已在设置中开启「迁移前备份」，将被覆盖的文件会先备份。
            </p>
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button
            onClick={() => onConfirm(policy)}
            disabled={transferring || selected.length === 0}
          >
            {transferring ? "迁移中…" : "开始迁移"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
