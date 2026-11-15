import { useAppInfo } from "./useAppInfo";

export function StatusBar() {
  const appInfo = useAppInfo();
  const right =
    appInfo.status === "ready"
      ? `${appInfo.info.name} ${appInfo.info.version}`
      : "";

  return (
    <footer className="statusbar">
      {appInfo.status === "error" ? (
        <span role="alert" className="statusbar-error">
          {appInfo.message}
        </span>
      ) : (
        <span role="status">Ready</span>
      )}
      <span className="statusbar-info">{right}</span>
    </footer>
  );
}
