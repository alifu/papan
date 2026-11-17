import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { applyTheme } from "./theme/useApplyTheme";
import { readSystemTheme, resolveTheme, useThemeStore } from "./theme/themeStore";
import "./theme/tokens.css";
import "./theme/base.css";
import "./ui/ui.css";
import "./shell/shell.css";

// Set the theme before the first render to avoid a wrong-theme flash.
applyTheme(resolveTheme(useThemeStore.getState().mode, readSystemTheme()));

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
