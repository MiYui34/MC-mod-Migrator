import { createContext, useContext, type ReactNode } from "react";
import { useMods } from "@/contexts/ModsContext";
import { useSettings } from "@/hooks/useMods";
import { useAppUpdate, type UpdateStatus } from "@/hooks/useAppUpdate";
import type { UpdateManifest, UpdateProgress } from "@/types";

type AppUpdateContextValue = ReturnType<typeof useAppUpdate>;

const AppUpdateContext = createContext<AppUpdateContextValue | null>(null);

export function AppUpdateProvider({ children }: { children: ReactNode }) {
  const { settings, loading } = useSettings();
  const { scanning, transferring, checking } = useMods();
  const busy = scanning || transferring || checking;
  const value = useAppUpdate(settings, loading, busy);
  return <AppUpdateContext.Provider value={value}>{children}</AppUpdateContext.Provider>;
}

export function useAppUpdateContext() {
  const ctx = useContext(AppUpdateContext);
  if (!ctx) {
    throw new Error("useAppUpdateContext must be used within AppUpdateProvider");
  }
  return ctx;
}

export type { UpdateStatus, UpdateManifest, UpdateProgress };
