import { useEffect } from "react";
import { CookieManager } from "../features/cookies/CookieManager";
import { useActiveLocation } from "../features/environments/useActiveLocation";
import { ImportCurlDialog } from "../features/curl/ImportCurlDialog";
import { NetworkDialog } from "../features/network/NetworkDialog";
import { useDialogsStore } from "./dialogsStore";

/** Renders the dialog chosen in `useDialogsStore`. */
export function ShellDialogs() {
  const open = useDialogsStore((s) => s.open);
  const close = useDialogsStore((s) => s.close);
  const location = useActiveLocation();
  const needsLocation = open === "cookies" && !location;
  // No request to take a jar from: drop the request so it cannot pop up on the next tab.
  useEffect(() => {
    if (needsLocation) close();
  }, [needsLocation, close]);
  if (open === "network") return <NetworkDialog onClose={close} />;
  if (open === "import-curl") return <ImportCurlDialog onClose={close} />;
  if (open === "cookies" && location) return <CookieManager location={location} onClose={close} />;
  return null;
}
