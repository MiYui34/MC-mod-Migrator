import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

export function useAppVersion() {
  const [version, setVersion] = useState("");

  useEffect(() => {
    invoke<string>("get_app_version_cmd")
      .then(setVersion)
      .catch(() => {});
  }, []);

  return version;
}
