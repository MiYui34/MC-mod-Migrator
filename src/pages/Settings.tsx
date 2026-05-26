import { ArrowLeft, Save, Trash2, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { useAppVersion } from "@/hooks/useAppVersion";
import { useSettings } from "@/hooks/useMods";
import type { AppSettings, UpdateCheckResult, UpdateState } from "@/types";
import { DEFAULT_UPDATE_MANIFEST_URL, effectiveManifestUrl } from "@/types";

const SOURCE_LABELS: Record<string, string> = {
  modrinth: "Modrinth",
  curseforge: "CurseForge",
  mcmod: "MC百科",
  github: "GitHub",
};

interface SettingsPageProps {
  onBack: () => void;
}

export function SettingsPage({ onBack }: SettingsPageProps) {
  const { settings, loading, save, clearCache } = useSettings();
  const [draft, setDraft] = useState<AppSettings | null>(null);
  const [saved, setSaved] = useState(false);
  const [cacheCleared, setCacheCleared] = useState(false);
  const appVersion = useAppVersion();
  const [updateChecking, setUpdateChecking] = useState(false);
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const [updateState, setUpdateState] = useState<UpdateState | null>(null);

  const current = draft ?? settings;

  useEffect(() => {
    void invoke<UpdateState>("get_update_state_cmd").then(setUpdateState).catch(() => {});
  }, [updateChecking, updateMessage]);

  useEffect(() => {
    const handler = (event: Event) => {
      setUpdateChecking(false);
      const detail = (event as CustomEvent<{ result?: UpdateCheckResult; error?: string }>)
        .detail;
      if (detail?.error) {
        setUpdateMessage(detail.error);
        return;
      }
      if (detail?.result?.updateAvailable && detail.result.manifest) {
        setUpdateMessage(`发现新版本 ${detail.result.manifest.version}`);
      } else if (detail?.result) {
        setUpdateMessage(`当前已是最新版本（${detail.result.currentVersion}）`);
      }
    };
    window.addEventListener("app-update-result", handler);
    return () => window.removeEventListener("app-update-result", handler);
  }, []);

  const handleSave = async () => {
    await save(current);
    setDraft(null);
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  const handleClearCache = async () => {
    await clearCache();
    setCacheCleared(true);
    setTimeout(() => setCacheCleared(false), 2000);
  };

  const handleCheckUpdate = () => {
    if (!effectiveManifestUrl(current)) {
      setUpdateMessage("未配置更新源。请填写清单地址或启用官方默认源。");
      return;
    }
    setUpdateChecking(true);
    setUpdateMessage(null);
    window.dispatchEvent(new CustomEvent("app-update-check"));
  };

  const handleRestoreDefaultSource = () => {
    setDraft({
      ...current,
      update_manifest_url: "",
      update_use_default_source: true,
    });
    setUpdateMessage(`已恢复官方默认源：${DEFAULT_UPDATE_MANIFEST_URL}`);
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-screen text-[var(--color-muted-foreground)]">
        加载设置...
      </div>
    );
  }

  return (
    <div className="max-w-2xl mx-auto p-6 space-y-6">
      <div className="flex items-center gap-3">
        <Button variant="ghost" size="icon" onClick={onBack}>
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <h1 className="text-xl font-semibold">设置</h1>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>下载源</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <p className="text-sm text-[var(--color-muted-foreground)]">
            按顺序尝试：Modrinth → CurseForge → MC百科 → GitHub。启用 MCIM
            镜像时，Modrinth 与 CurseForge 均可走国内加速，通常无需 API Key。
          </p>
          <div className="flex gap-2 flex-wrap">
            {current.download_source_priority.map((src, i) => (
              <span
                key={src}
                className="rounded-md border border-[var(--color-border)] px-3 py-1 text-sm"
              >
                {i + 1}. {SOURCE_LABELS[src] ?? src}
              </span>
            ))}
          </div>

          <div className="space-y-2 pt-2 border-t border-[var(--color-border)]">
            <p className="text-sm font-medium">API 镜像（Modrinth + CurseForge）</p>
            <p className="text-xs text-[var(--color-muted-foreground)]">
              PCL2 / HMCL 在国内使用 MCIM 镜像（mod.mcimirror.top）；失败时自动回退官方源。
            </p>
            <div className="flex gap-2 flex-wrap">
              {(
                [
                  ["auto", "自动（MCIM → 官方）"],
                  ["mcim", "仅 MCIM 国内镜像"],
                  ["official", "仅官方 API"],
                ] as const
              ).map(([value, label]) => (
                <button
                  key={value}
                  type="button"
                  onClick={() =>
                    setDraft({ ...current, mod_api_mirror: value })
                  }
                  className={`rounded-md border px-3 py-1.5 text-sm transition-colors ${
                    (current.mod_api_mirror || "auto") === value
                      ? "border-emerald-500 bg-emerald-500/10"
                      : "border-[var(--color-border)] hover:bg-[var(--color-muted)]"
                  }`}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>CurseForge API Key（可选）</CardTitle>
        </CardHeader>
        <CardContent className="space-y-2">
          <Input
            type="password"
            placeholder="留空则仅通过 MCIM 镜像使用 CurseForge"
            value={current.curseforge_api_key}
            onChange={(e) =>
              setDraft({ ...current, curseforge_api_key: e.target.value })
            }
          />
          <p className="text-xs text-[var(--color-muted-foreground)]">
            使用官方 CurseForge API 时需要 Key（在{" "}
            <a
              href="https://console.curseforge.com/"
              className="underline"
              target="_blank"
              rel="noreferrer"
            >
              CurseForge Console
            </a>{" "}
            申请）。镜像模式下可不填。
          </p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Mod 版本策略</CardTitle>
        </CardHeader>
        <CardContent className="space-y-2">
          <p className="text-xs text-[var(--color-muted-foreground)]">
            「匹配源版本（允许降级）」会在兼容前提下优先选择不高于源端 Mod 版本的构建，避免自动升级到更新版本。
            仍可在检查结果中手动点选其他版本。
          </p>
          <div className="flex gap-2 flex-wrap">
            {(
              [
                ["auto", "自动（推荐最新兼容版）"],
                ["downgrade", "匹配源版本（允许降级）"],
              ] as const
            ).map(([value, label]) => (
              <button
                key={value}
                type="button"
                onClick={() =>
                  setDraft({ ...current, mod_version_policy: value })
                }
                className={`rounded-md border px-3 py-1.5 text-sm transition-colors ${
                  (current.mod_version_policy || "auto") === value
                    ? "border-emerald-500 bg-emerald-500/10"
                    : "border-[var(--color-border)] hover:bg-[var(--color-muted)]"
                }`}
              >
                {label}
              </button>
            ))}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>并发数</CardTitle>
        </CardHeader>
        <CardContent className="space-y-2">
          <Input
            type="number"
            min={2}
            max={32}
            value={current.max_concurrent_downloads}
            onChange={(e) =>
              setDraft({
                ...current,
                max_concurrent_downloads: parseInt(e.target.value, 10) || 6,
              })
            }
          />
          <p className="text-xs text-[var(--color-muted-foreground)]">
            并行任务数，影响扫描、兼容性检查与迁移下载/复制速度（建议 8–16，网络好可调至 24–32）
          </p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>迁移与报告</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={current.backup_before_transfer ?? true}
              onChange={(e) =>
                setDraft({ ...current, backup_before_transfer: e.target.checked })
              }
            />
            迁移前自动备份将被覆盖的目标文件
          </label>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={current.auto_export_mod_report ?? false}
              onChange={(e) =>
                setDraft({ ...current, auto_export_mod_report: e.target.checked })
              }
            />
            Mod 迁移完成后自动导出报告
          </label>
          <div className="flex gap-2 flex-wrap items-center text-sm">
            <span className="text-[var(--color-muted-foreground)]">报告格式：</span>
            {(["md", "txt"] as const).map((fmt) => (
              <button
                key={fmt}
                type="button"
                onClick={() => setDraft({ ...current, mod_report_format: fmt })}
                className={`rounded-md border px-3 py-1 text-sm ${
                  (current.mod_report_format || "md") === fmt
                    ? "border-emerald-500 bg-emerald-500/10"
                    : "border-[var(--color-border)]"
                }`}
              >
                .{fmt}
              </button>
            ))}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>软件更新</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <p className="text-sm text-[var(--color-muted-foreground)]">
            当前版本：{appVersion || "…"}。启用官方默认源或填写清单地址后，启动时会自动检查新版本。
          </p>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={current.update_use_default_source !== false}
              onChange={(e) =>
                setDraft({ ...current, update_use_default_source: e.target.checked })
              }
            />
            使用官方默认更新源（{DEFAULT_UPDATE_MANIFEST_URL}）
          </label>
          <div className="space-y-2">
            <label className="text-sm font-medium">更新清单地址（latest.json，留空则使用官方源）</label>
            <Input
              placeholder={DEFAULT_UPDATE_MANIFEST_URL}
              value={current.update_manifest_url ?? ""}
              onChange={(e) =>
                setDraft({ ...current, update_manifest_url: e.target.value })
              }
            />
            <div className="flex gap-2 flex-wrap">
              <Button type="button" variant="outline" size="sm" onClick={handleRestoreDefaultSource}>
                恢复默认
              </Button>
            </div>
            <p className="text-xs text-[var(--color-muted-foreground)]">
              远程源须使用 HTTPS；本地文件路径可用于开发测试。留空且关闭官方源 = 禁用更新检查。
              发版请使用独立安装的「MC换端助手更新发布器」。
            </p>
          </div>
          <div className="space-y-2">
            <p className="text-sm font-medium">更新方式</p>
            <div className="flex gap-2 flex-wrap">
              {(["manual", "auto"] as const).map((mode) => (
                <button
                  key={mode}
                  type="button"
                  onClick={() => setDraft({ ...current, update_mode: mode })}
                  className={`rounded-md border px-3 py-1 text-sm ${
                    (current.update_mode || "manual") === mode
                      ? "border-emerald-500 bg-emerald-500/10"
                      : "border-[var(--color-border)]"
                  }`}
                >
                  {mode === "manual" ? "手动更新（弹窗提示）" : "自动下载（完成后提示安装）"}
                </button>
              ))}
            </div>
          </div>
          <div className="space-y-2">
            <label className="text-sm font-medium">检查间隔（小时）</label>
            <Input
              type="number"
              min={1}
              max={168}
              value={current.update_check_interval_hours ?? 24}
              onChange={(e) =>
                setDraft({
                  ...current,
                  update_check_interval_hours: Math.max(1, Number(e.target.value) || 24),
                })
              }
            />
          </div>
          <Button variant="outline" onClick={handleCheckUpdate} disabled={updateChecking}>
            <RefreshCw className={`h-4 w-4 ${updateChecking ? "animate-spin" : ""}`} />
            立即检查更新
          </Button>
          {updateState?.lastCheckAt ? (
            <div className="rounded-md border border-[var(--color-border)] p-3 text-xs space-y-1">
              <p className="text-[var(--color-muted-foreground)]">
                上次检查：{new Date(Number(updateState.lastCheckAt) * 1000).toLocaleString()}
              </p>
              <p className={updateState.lastCheckOk ? "text-emerald-400" : "text-red-400"}>
                {updateState.lastCheckOk
                  ? updateState.lastCheckVersion
                    ? `成功，发现版本 ${updateState.lastCheckVersion}`
                    : "成功，当前已是最新版本"
                  : `失败：${updateState.lastCheckError ?? "未知错误"}`}
              </p>
            </div>
          ) : null}
          {updateMessage && (
            <p className="text-xs text-[var(--color-muted-foreground)]">{updateMessage}</p>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>缓存</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-xs text-[var(--color-muted-foreground)] mb-3">
            Mod 识别结果会缓存在本地，加速重复扫描。源/目标实例与 Mod 列表会在关闭软件后自动恢复。
          </p>
          <Button variant="outline" onClick={handleClearCache}>
            <Trash2 className="h-4 w-4" />
            清除识别缓存
            {cacheCleared && <span className="text-emerald-400 ml-2">已清除</span>}
          </Button>
        </CardContent>
      </Card>

      <div className="flex gap-2">
        <Button onClick={handleSave}>
          <Save className="h-4 w-4" />
          保存设置
          {saved && <span className="text-emerald-300 ml-1">✓</span>}
        </Button>
        <Button variant="outline" onClick={onBack}>
          返回
        </Button>
      </div>
    </div>
  );
}
