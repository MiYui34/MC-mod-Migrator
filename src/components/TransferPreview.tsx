import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import type { ModTransferItem } from "@/types";

interface TransferPreviewProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  items: ModTransferItem[];
  targetPath: string;
  targetVersion: string;
  targetLoader: string;
  targetLoaderVersion?: string;
  onConfirm: () => void;
  transferring?: boolean;
}

export function TransferPreview({
  open,
  onOpenChange,
  items,
  targetPath,
  targetVersion,
  targetLoader,
  targetLoaderVersion,
  onConfirm,
  transferring,
}: TransferPreviewProps) {
  const selected = items.filter((i) => i.selected && i.status === "transferable");
  const primary = selected.filter((i) => !i.isDependency);
  const dependencies = selected.filter((i) => i.isDependency);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[min(85dvh,calc(100vh-2rem))] max-w-lg flex-col">
        <DialogHeader>
          <DialogTitle>转移预览</DialogTitle>
        </DialogHeader>
        <DialogBody className="space-y-2">
        <div className="space-y-1 text-sm text-[var(--color-muted-foreground)]">
          <p className="truncate">
            目标：MC {targetVersion} · {targetLoader}
            {targetLoaderVersion ? ` ${targetLoaderVersion}` : ""}
          </p>
          <p className="truncate">路径：{targetPath}</p>
          <p>
            将下载 {selected.length} 个 Mod
            {dependencies.length > 0
              ? `（${primary.length} 个主 Mod + ${dependencies.length} 个依赖）`
              : ""}
            ：
          </p>
        </div>
        <ul className="space-y-2">
          {selected.map((item) => (
            <li
              key={item.mod.sha512 + item.mod.fileName + (item.isDependency ? "-dep" : "")}
              className="flex items-center justify-between rounded-md border border-[var(--color-border)] p-2 text-sm"
            >
              <div>
                <div className="font-medium flex items-center gap-2">
                  {item.mod.name}
                  {item.isDependency && (
                    <Badge variant="warning" className="text-[10px] px-1.5 py-0">
                      依赖
                    </Badge>
                  )}
                </div>
                <div className="text-xs text-[var(--color-muted-foreground)]">
                  {item.targetFileName}
                </div>
                {item.requiredBy && (
                  <div className="text-xs text-amber-500/80">
                    被 {item.requiredBy} 需要
                  </div>
                )}
              </div>
              <Badge variant="secondary">
                {item.downloadSource ?? item.mod.source}
              </Badge>
            </li>
          ))}
        </ul>
        </DialogBody>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={onConfirm} disabled={transferring || selected.length === 0}>
            确认迁移
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
