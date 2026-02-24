import { readGlobalOpenCodeConfig, writeGlobalOpenCodeConfig } from "@services/tauri";
import { useFileEditor } from "@/features/shared/hooks/useFileEditor";

export function useGlobalOpenCodeConfig() {
  return useFileEditor({
    key: "global-config",
    read: readGlobalOpenCodeConfig,
    write: writeGlobalOpenCodeConfig,
    readErrorTitle: "Couldn't load global opencode.json",
    writeErrorTitle: "Couldn't save global opencode.json",
  });
}

/** @deprecated Use useGlobalOpenCodeConfig instead */
export const useGlobalCodexConfigToml = useGlobalOpenCodeConfig;
