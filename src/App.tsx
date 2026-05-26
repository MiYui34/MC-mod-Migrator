import type { ReactNode } from "react";
import { useState } from "react";
import { Home } from "@/pages/Home";
import { SettingsPage } from "@/pages/Settings";
import { ModsProvider } from "@/contexts/ModsContext";
import { AppUpdateProvider, useAppUpdateContext } from "@/contexts/AppUpdateContext";
import { UpdateDialog } from "@/components/UpdateDialog";

function UpdateDialogLayer() {
  const update = useAppUpdateContext();
  return (
    <UpdateDialog
      open={update.dialogOpen}
      onOpenChange={update.setDialogOpen}
      manifest={update.manifest}
      currentVersion={update.currentVersion}
      downloading={update.downloading}
      progress={update.progress}
      downloadedPath={update.downloadedPath}
      error={update.error}
      onDownload={async (m) => {
        await update.downloadUpdate(m);
      }}
      onInstall={update.installUpdate}
      onDismiss={update.dismissUpdate}
      onCancelDownload={update.cancelDownload}
    />
  );
}

function AppShell({ children }: { children: ReactNode }) {
  return (
    <>
      {children}
      <UpdateDialogLayer />
    </>
  );
}

function App() {
  const [page, setPage] = useState<"home" | "settings">("home");

  return (
    <ModsProvider>
      <AppUpdateProvider>
        <AppShell>
          {page === "settings" ? (
            <SettingsPage onBack={() => setPage("home")} />
          ) : (
            <Home onOpenSettings={() => setPage("settings")} />
          )}
        </AppShell>
      </AppUpdateProvider>
    </ModsProvider>
  );
}

export default App;
