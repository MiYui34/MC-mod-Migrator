import { createContext, useContext, type ReactNode } from "react";
import { useModsState } from "@/hooks/useMods";

type ModsContextValue = ReturnType<typeof useModsState>;

const ModsContext = createContext<ModsContextValue | null>(null);

export function ModsProvider({ children }: { children: ReactNode }) {
  const value = useModsState();
  return <ModsContext.Provider value={value}>{children}</ModsContext.Provider>;
}

export function useMods() {
  const ctx = useContext(ModsContext);
  if (!ctx) {
    throw new Error("useMods must be used within ModsProvider");
  }
  return ctx;
}
