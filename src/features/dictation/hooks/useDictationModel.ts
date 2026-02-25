import { useCallback, useEffect, useState } from "react";
import type { DictationModelStatus } from "../../../types";
import {
  cancelDictationDownload,
  downloadDictationModel,
  getDictationModelStatus,
  removeDictationModel,
} from "../../../services/tauri";
import { subscribeDictationDownload } from "../../../services/events";

type UseDictationModelResult = {
  status: DictationModelStatus | null;
  refresh: () => Promise<void>;
  download: () => Promise<void>;
  cancel: () => Promise<void>;
  remove: () => Promise<void>;
};

export function useDictationModel(modelId: string | null): UseDictationModelResult {
  const [status, setStatus] = useState<DictationModelStatus | null>(null);

  const refresh = useCallback(async () => {
    const next = await getDictationModelStatus(modelId);
    setStatus(next);
  }, [modelId]);

  useEffect(() => {
    let active = true;

    void (async () => {
      try {
        const next = await getDictationModelStatus(modelId);
        if (active) {
          setStatus(next);
        }
      } catch {
        // Ignore dictation status errors during startup.
      }
    })();

    const unlisten = subscribeDictationDownload((event) => {
      console.log("[useDictationModel] received dictation-download event:", event);
      if (!active) {
        console.log("[useDictationModel] ignoring event - component not active");
        return;
      }
      if (!modelId || event.modelId === modelId) {
        console.log("[useDictationModel] updating status from event");
        setStatus(event);
      } else {
        console.log("[useDictationModel] ignoring event - modelId mismatch:", {
          expected: modelId,
          received: event.modelId,
        });
      }
    });

    return () => {
      active = false;
      unlisten();
    };
  }, [modelId]);

  const download = useCallback(async () => {
    console.log("[useDictationModel] download() called with modelId:", modelId);
    try {
      const next = await downloadDictationModel(modelId);
      console.log("[useDictationModel] download() returned:", next);
      setStatus(next);
    } catch (error) {
      console.error("[useDictationModel] download() error:", error);
      throw error;
    }
  }, [modelId]);

  const cancel = useCallback(async () => {
    const next = await cancelDictationDownload(modelId);
    setStatus(next);
  }, [modelId]);

  const remove = useCallback(async () => {
    const next = await removeDictationModel(modelId);
    setStatus(next);
  }, [modelId]);

  return {
    status,
    refresh,
    download,
    cancel,
    remove,
  };
}
