import { AppShell } from "./shell/AppShell";
import { useApplyTheme } from "./theme/useApplyTheme";

function App() {
  useApplyTheme();
  return <AppShell />;
}

export default App;
