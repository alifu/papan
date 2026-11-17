import { useEffect } from "react";
import { Dialog } from "../../ui/Dialog";
import { CertificateList } from "./CertificateList";
import { useNetworkStore } from "./networkStore";
import { ProxyForm } from "./ProxyForm";
import "../environments/environments.css";

export interface NetworkDialogProps {
  readonly onClose: () => void;
}

/** Edit › Network settings…: proxy and client certificates, both machine-local. */
export function NetworkDialog({ onClose }: NetworkDialogProps) {
  const settings = useNetworkStore((s) => s.settings);
  const { load, saveProxy, addCertificate, removeCertificate } = useNetworkStore.getState();
  useEffect(() => {
    void load();
  }, [load]);
  const storage = settings?.storage;
  return (
    <Dialog open title="Network settings" onClose={onClose}>
      {storage && storage.mode !== "keychain" ? (
        <div className="env-storage-warning" role="status">
          {storage.message ?? "Secrets are not stored in the system keychain."}
        </div>
      ) : null}
      {settings ? (
        <>
          <ProxyForm proxy={settings.proxy} onSave={saveProxy} />
          <CertificateList certificates={settings.certificates} onAdd={addCertificate} onRemove={removeCertificate} />
        </>
      ) : (
        <p className="hint">Loading...</p>
      )}
    </Dialog>
  );
}
