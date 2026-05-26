import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState } from "react";

import type {
  AppSettings,
  UpdateCheckResult,
  UpdateManifest,
  UpdateProgress,
  UpdateState,
} from "@/types";
import { effectiveManifestUrl } from "@/types";

export type UpdateStatus = "idle" | "checking" | "available" | "downloading" | "downloaded";

export function useAppUpdate(
  settings: AppSettings | null,
  settingsLoading: boolean,
  busy = false
) {
  const [currentVersion, setCurrentVersion] = useState("");
  const [manifest, setManifest] = useState<UpdateManifest | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [checking, setChecking] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [downloadedPath, setDownloadedPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>("idle");
  const autoStartedRef = useRef(false);
  const pendingManifestRef = useRef<UpdateManifest | null>(null);

  const manifestUrl = settings ? effectiveManifestUrl(settings) : null;
  const isAutoMode = settings?.update_mode === "auto";

  useEffect(() => {
    invoke<string>("get_app_version_cmd").then(setCurrentVersion).catch(() => {});
  }, []);

  useEffect(() => {
    const unlisten = listen<UpdateProgress>("app-update-progress", (event) => {
      setProgress(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    void invoke<UpdateState>("get_update_state_cmd").then((state) => {
      if (state.downloadedPath && state.downloadedVersion) {
        setDownloadedPath(state.downloadedPath);
        setUpdateStatus("downloaded");
      }
    });
  }, []);

  const shouldShowUpdate = useCallback(
    async (result: UpdateCheckResult, updateState: UpdateState) => {
      if (!result.updateAvailable || !result.manifest) return false;
      if (result.manifest.mandatory) return true;
      return updateState.dismissedVersion !== result.manifest.version;
    },
    []
  );

  const applyCheckResult = useCallback(
    async (result: UpdateCheckResult, silent = false) => {
      if (!result.updateAvailable || !result.manifest) {
        setUpdateStatus("idle");
        setManifest(null);
        return result;
      }

      const updateState = await invoke<UpdateState>("get_update_state_cmd");
      if (!(await shouldShowUpdate(result, updateState))) {
        setUpdateStatus("idle");
        return result;
      }

      setManifest(result.manifest);
      pendingManifestRef.current = result.manifest;

      if (
        updateState.downloadedVersion === result.manifest.version &&
        updateState.downloadedPath
      ) {
        setDownloadedPath(updateState.downloadedPath);
        setUpdateStatus("downloaded");
        if (!silent && !isAutoMode) {
          setDialogOpen(true);
        }
        return result;
      }

      setDownloadedPath(null);
      setUpdateStatus("available");

      if (isAutoMode || silent) {
        return result;
      }
      setDialogOpen(true);
      return result;
    },
    [shouldShowUpdate, isAutoMode]
  );

  const runCheck = useCallback(
    async (force = false) => {
      if (!manifestUrl) return null;
      if (!force) {
        const should = await invoke<boolean>("should_check_app_update_cmd");
        if (!should) return null;
      }
      setChecking(true);
      setUpdateStatus("checking");
      setError(null);
      let result: UpdateCheckResult | null = null;
      try {
        result = await invoke<UpdateCheckResult>("check_app_update_cmd", {
          manifestUrl: null,
        });
        await applyCheckResult(result, isAutoMode && !force);
      } catch (e) {
        setError(String(e));
        setUpdateStatus("idle");
        if (force) {
          window.dispatchEvent(
            new CustomEvent("app-update-result", { detail: { error: String(e) } })
          );
        }
        return null;
      } finally {
        setChecking(false);
      }
      if (force && result) {
        window.dispatchEvent(
          new CustomEvent("app-update-result", { detail: { result } })
        );
      }
      return result;
    },
    [manifestUrl, applyCheckResult, isAutoMode]
  );

  const downloadUpdate = useCallback(async (target: UpdateManifest) => {
    setDownloading(true);
    setUpdateStatus("downloading");
    setError(null);
    setProgress(null);
    try {
      const path = await invoke<string>("download_app_update_cmd", { manifest: target });
      setDownloadedPath(path);
      setUpdateStatus("downloaded");
      return path;
    } catch (e) {
      const msg = String(e);
      if (!msg.includes("操作已取消")) {
        setError(msg);
      }
      setUpdateStatus(manifest ? "available" : "idle");
      return null;
    } finally {
      setDownloading(false);
    }
  }, [manifest]);

  const cancelDownload = useCallback(async () => {
    await invoke("cancel_app_update_download_cmd");
    setDownloading(false);
    setProgress(null);
    setUpdateStatus(manifest ? "available" : "idle");
  }, [manifest]);

  const installUpdate = useCallback(async (path: string) => {
    await invoke("install_app_update_cmd", { path });
    await getCurrentWindow().close();
  }, []);

  const dismissUpdate = useCallback(async (version: string) => {
    const state = await invoke<UpdateState>("get_update_state_cmd");
    await invoke("save_update_state_cmd", {
      updateState: { ...state, dismissedVersion: version },
    });
    setDialogOpen(false);
    setUpdateStatus("idle");
    pendingManifestRef.current = null;
  }, []);

  const openUpdateDialog = useCallback(() => {
    if (manifest) {
      setDialogOpen(true);
    } else {
      void runCheck(true);
    }
  }, [manifest, runCheck]);

  const checkNow = useCallback(() => runCheck(true), [runCheck]);

  useEffect(() => {
    const handler = () => {
      void runCheck(true);
    };
    window.addEventListener("app-update-check", handler);
    return () => window.removeEventListener("app-update-check", handler);
  }, [runCheck]);

  useEffect(() => {
    if (settingsLoading || !settings || !manifestUrl) return;

    void (async () => {
      const result = await runCheck(false);
      if (
        isAutoMode &&
        result?.updateAvailable &&
        result.manifest &&
        !autoStartedRef.current
      ) {
        autoStartedRef.current = true;
      }
    })();
  }, [settingsLoading, settings, manifestUrl, runCheck, isAutoMode]);

  useEffect(() => {
    if (!isAutoMode || busy || downloading || !manifestUrl) return;
    const target = pendingManifestRef.current ?? manifest;
    if (!target || updateStatus !== "available") return;
    if (downloadedPath) return;

    void downloadUpdate(target);
  }, [isAutoMode, busy, downloading, manifestUrl, manifest, updateStatus, downloadedPath, downloadUpdate]);

  useEffect(() => {
    if (settingsLoading || !manifestUrl) return;
    const hours = Math.max(1, settings?.update_check_interval_hours || 24);
    const timer = window.setInterval(
      () => {
        void runCheck(false);
      },
      hours * 60 * 60 * 1000
    );
    return () => window.clearInterval(timer);
  }, [settingsLoading, settings, manifestUrl, runCheck]);

  useEffect(() => {
    if (!downloadedPath || !isAutoMode) return;
    let unlisten: (() => void) | undefined;
    void getCurrentWindow()
      .onCloseRequested(async (event) => {
        event.preventDefault();
        await installUpdate(downloadedPath);
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => {
      unlisten?.();
    };
  }, [downloadedPath, isAutoMode, installUpdate]);

  return {
    currentVersion,
    manifest,
    dialogOpen,
    setDialogOpen,
    checking,
    downloading,
    progress,
    downloadedPath,
    error,
    updateStatus,
    checkNow,
    downloadUpdate,
    cancelDownload,
    installUpdate,
    dismissUpdate,
    openUpdateDialog,
  };
}
