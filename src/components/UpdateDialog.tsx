import { Download, ExternalLink, Loader2, RefreshCw, X } from "lucide-react";
import ReactMarkdown from "react-markdown";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { UpdateManifest, UpdateProgress } from "@/types";

interface UpdateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  manifest: UpdateManifest | null;
  currentVersion: string;
  downloading: boolean;
  progress: UpdateProgress | null;
  downloadedPath: string | null;
  error: string | null;
  onDownload: (manifest: UpdateManifest) => Promise<void>;
  onInstall: (path: string) => Promise<void>;
  onDismiss: (version: string) => Promise<void>;
  onCancelDownload?: () => Promise<void>;
}

function formatBytes(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

export function UpdateDialog({
  open,
  onOpenChange,
  manifest,
  currentVersion,
  downloading,
  progress,
  downloadedPath,
  error,
  onDownload,
  onInstall,
  onDismiss,
  onCancelDownload,
}: UpdateDialogProps) {
  if (!manifest) return null;

  const pct =
    progress?.total && progress.total > 0
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : null;

  const readyToInstall = Boolean(downloadedPath);
  const forced = manifest.mandatory;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-w-md"
        showClose={!forced}
        onPointerDownOutside={(e) => forced && e.preventDefault()}
        onEscapeKeyDown={(e) => forced && e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle>
            {forced && manifest.minSupportedVersion
              ? `需要升级至 ${manifest.version}`
              : `发现新版本 ${manifest.version}`}
          </DialogTitle>
          <DialogDescription>
            当前版本 {currentVersion}
            {manifest.releaseDate ? ` · 发布于 ${manifest.releaseDate}` : ""}
            {forced && manifest.minSupportedVersion ? (
              <span className="block mt-1 text-amber-400">
                当前版本低于最低支持版本 {manifest.minSupportedVersion}，必须更新后才能继续使用。
              </span>
            ) : null}
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 max-h-[min(40vh,16rem)] space-y-3 overflow-y-auto overscroll-contain">
          {manifest.notes ? (
            <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-muted)]/30 p-3 text-sm prose prose-sm prose-invert max-w-none">
              <ReactMarkdown>{manifest.notes}</ReactMarkdown>
            </div>
          ) : (
            <p className="text-sm text-[var(--color-muted-foreground)]">暂无更新说明。</p>
          )}

          {manifest.releaseNotesUrl ? (
            <a
              href={manifest.releaseNotesUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1 whitespace-nowrap text-sm text-emerald-400 hover:underline"
            >
              <ExternalLink className="h-3.5 w-3.5 shrink-0" />
              查看完整更新日志
            </a>
          ) : null}

          {downloading && (
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-sm text-[var(--color-muted-foreground)]">
                <Loader2 className="h-4 w-4 shrink-0 animate-spin" />
                <span className="truncate">{progress?.message ?? "正在下载…"}</span>
              </div>
              {pct != null && (
                <div className="space-y-1">
                  <div className="h-2 overflow-hidden rounded-full bg-[var(--color-muted)]">
                    <div
                      className="h-full bg-emerald-500 transition-all"
                      style={{ width: `${pct}%` }}
                    />
                  </div>
                  <p className="text-xs text-[var(--color-muted-foreground)]">
                    {formatBytes(progress?.downloaded ?? 0)}
                    {progress?.total ? ` / ${formatBytes(progress.total)} (${pct}%)` : ""}
                  </p>
                </div>
              )}
            </div>
          )}

          {readyToInstall && !downloading && (
            <p className="text-sm text-emerald-400">
              更新包已下载，点击「立即安装」启动安装程序。
            </p>
          )}

          {error && <p className="text-sm text-red-400 break-words">{error}</p>}
        </div>

        <DialogFooter>
          {!forced && (
            <>
              <Button variant="ghost" disabled={downloading} onClick={() => onOpenChange(false)}>
                稍后提醒
              </Button>
              <Button
                variant="outline"
                disabled={downloading}
                onClick={() => void onDismiss(manifest.version)}
              >
                跳过此版本
              </Button>
            </>
          )}
          {downloading && onCancelDownload ? (
            <Button variant="outline" onClick={() => void onCancelDownload()}>
              <X className="h-4 w-4" />
              取消下载
            </Button>
          ) : null}
          {readyToInstall ? (
            <Button onClick={() => void onInstall(downloadedPath!)}>
              <RefreshCw className="h-4 w-4" />
              立即安装
            </Button>
          ) : (
            <Button disabled={downloading} onClick={() => void onDownload(manifest)}>
              {downloading ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Download className="h-4 w-4" />
              )}
              {downloading ? "下载中…" : "下载更新"}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
