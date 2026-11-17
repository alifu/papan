import { useEffect, useState } from "react";
import { commands, type AppInfo } from "../ipc/bindings";
import { toUserMessage } from "../ipc/errors";

export type AppInfoState =
  | { readonly status: "loading" }
  | { readonly status: "ready"; readonly info: AppInfo }
  | { readonly status: "error"; readonly message: string };

export function useAppInfo(): AppInfoState {
  const [state, setState] = useState<AppInfoState>({ status: "loading" });

  useEffect(() => {
    let active = true;
    commands
      .appInfo()
      .then((info) => active && setState({ status: "ready", info }))
      .catch(
        (err: unknown) =>
          active && setState({ status: "error", message: toUserMessage(err) }),
      );
    return () => {
      active = false;
    };
  }, []);

  return state;
}
