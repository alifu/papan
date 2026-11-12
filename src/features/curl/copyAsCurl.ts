import { currentContent, useRequestStore } from "../request/requestStore";
import { copyText } from "../response/clipboard";
import { reportError, showToast } from "../../ui/toastStore";
import { useTabsStore } from "../../shell/tabsStore";
import * as api from "./api";

export const COPIED_MESSAGE = "Copied. Secret values are left as {{name}}.";

/** Copies the active request, drafts included, as a cURL command. */
export async function copyAsCurl(): Promise<void> {
  const { tabs, activeTabId } = useTabsStore.getState();
  const tab = tabs.find((t) => t.id === activeTabId);
  const content = tab?.location ? currentContent(useRequestStore.getState().byTab[tab.id]) : null;
  if (!tab?.location || !content) return;
  try {
    await copyText(await api.exportCurl(tab.location, content));
    showToast({ kind: "info", message: COPIED_MESSAGE });
  } catch (err) {
    reportError(err);
  }
}
