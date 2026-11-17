# Papan code map

This doc maps each Papan screen and shared piece to the files that implement it, for developers new to the codebase.
Paths are relative to the repo root. FE = `src/`, BE = `src-tauri/src/`.
To edit something, find it in the quick lookup table, then read its screen section for the wiring (components, state, IPC, Rust logic, tests).

## Architecture in 10 lines

1. Papan is a Tauri 2 desktop API client: React/TypeScript frontend in `src/`, Rust backend in `src-tauri/src/`.
2. Frontend code is organised as feature folders in `src/features/<x>/`, with shared widgets in `src/ui/` and the app shell in `src/shell/`.
3. Each feature has an `api.ts` that wraps the typed `commands.*` object from `src/ipc/bindings.ts`, a zustand `*Store.ts`, a `*.css` file and colocated tests.
4. The frontend calls the backend over Tauri IPC. Backend commands live in `<module>/commands.rs`; the real logic sits in sibling `service.rs` or `*_model.rs` files.
5. Backend commands, events and error types are registered in `src-tauri/src/ipc.rs` (`builder()`).
6. `src/ipc/bindings.ts` is generated from that registry. Never hand-edit it. Regenerate with a debug app start or `UPDATE_BINDINGS=1 cargo test bindings`.
7. A plain `cargo test` fails if the committed bindings are stale (`sync_bindings` in `src-tauri/src/ipc.rs`).
8. Backend events flow to the frontend (for example `CollectionChanged`, `WsEvent`, `GrpcEvent`, `JourneyRunEvent`). Errors are `AppError` in `src-tauri/src/error.rs`, mirrored in `src/ipc/errors.ts`.
9. Persistent data is plain files on disk (YAML requests, collections, environments, mocks, journeys; JSON registries and history). The format table is in [Shared pieces](#shared-pieces).
10. Startup flows `src/main.tsx` -> `src/App.tsx` -> `src/shell/AppShell.tsx`; Tauri state, plugins and setup are in `src-tauri/src/lib.rs` (`run()`).

## Quick lookup: I want to change X

| Change | Open these files |
|---|---|
| App icon (bundle) | `design/app-icon.svg` (source), `src-tauri/icons/*` (icon.icns, icon.ico, icon.png, 32x32.png...), icons list in `src-tauri/tauri.conf.json` |
| Window title / product name / identifier | `src-tauri/tauri.conf.json` |
| Colours / theme palette | `src/theme/tokens.css` (check `src/theme/contrast.test.ts`) |
| Default font / sizes / bar heights | `src/theme/tokens.css` `:root` vars |
| Theme mode logic (system/light/dark) | `src/theme/themeStore.ts`, `src/theme/useApplyTheme.ts`, `src/shell/ThemeSwitcher.tsx` |
| A button's look | `src/ui/Button.tsx` + `src/ui/ui.css` (global look: `src/theme/base.css`) |
| An icon glyph / add icon | `src/ui/icons/glyphs/*.tsx`, `src/ui/icons/names.ts`, `src/ui/icons/Icon.tsx` |
| Dialog chrome / modal behaviour | `src/ui/Dialog.tsx`, `src/ui/ui.css` |
| Toast behaviour | `src/ui/Toast.tsx`, `src/ui/toastStore.ts` |
| Code editor look / `{{var}}` highlight | `src/ui/code/theme.ts`, `src/ui/code/setup.ts`, `src/ui/code/varHighlight.ts` |
| Menu bar items (File/Edit/Request/View) | `src/shell/MenuBar.tsx`; macOS native menu `src-tauri/src/menu.rs` |
| Keyboard shortcuts | `src/shell/useShortcuts.ts`, `src/ui/platform.ts` (labels) |
| Toolbar buttons | `src/shell/Toolbar.tsx` |
| Sidebar sections (add/reorder/rename) | `src/shell/layoutStore.ts` (`NAV_SECTIONS`), `src/shell/navSections.ts`, `src/shell/Sidebar.tsx` `renderSection` |
| Welcome screen text | `src/shell/TabPanel.tsx` `Welcome` |
| Status bar text | `src/shell/StatusBar.tsx`, `src/shell/useAppInfo.ts`, `src-tauri/src/commands.rs` `app_info` |
| Add a header (or other row) editor to the request editor | `src/features/request/RequestEditor.tsx`, `src/features/request/KeyValueTable.tsx`, `src/features/request/sections.ts`; default headers on send: `src-tauri/src/engine/http/build.rs` |
| Add a new request sub-tab | `src/features/request/sections.ts` + `src/features/request/RequestEditor.tsx` |
| Change how an HTTP request is sent | `src-tauri/src/engine/http/send.rs`, `build.rs`, `client.rs`, `service.rs` (`HttpEngine::send`), `redirect.rs`, `commands.rs` `send_request` |
| Variable `{{ }}` resolution / env var handling | `src-tauri/src/vars/resolve.rs`, `template.rs`, `dynamic.rs`, `service.rs`; FE `src/features/environments/template.ts`, `src/features/environments/envStore.ts` |
| Environment manager UI | `src/features/environments/EnvManager.tsx`, `src/features/environments/VariableTable.tsx` |
| Secret storage / keychain | `src-tauri/src/vars/vault/*`, `src-tauri/src/store/secrets.rs`, FE `src/features/environments/SaveSecretDialog.tsx` |
| Auth types (basic/bearer/api key/digest/OAuth2) | FE `src/features/request/AuthEditor.tsx`, `AuthFields.tsx`, `OAuthFields.tsx`; BE `src-tauri/src/store/auth_model.rs`, `src-tauri/src/engine/http/digest.rs`, `src-tauri/src/engine/http/oauth/*` |
| Response body viewer / JSON tree / HTML preview | `src/features/response/BodyView.tsx`, `JsonTree.tsx`, `HtmlPreview.tsx`, `detect.ts`, `format.ts` |
| Response timing bar / status line | `src/features/response/TimingBar.tsx`, `src/features/response/ResponseViewer.tsx`; BE `src-tauri/src/engine/http/types.rs` |
| Pre-request/post-response script API (`pm.*`) | `src-tauri/src/script/pm.rs`, `context.rs`, `sandbox.rs`; limits `src-tauri/src/script/watchdog.rs`; snippets `src/features/request/snippets.ts` |
| Collection tree / row context menu | `src/features/collections/rowMenu.ts`, `TreeRow.tsx`, `CollectionsPanel.tsx` |
| Request file format (YAML fields) | `src-tauri/src/store/request_model.rs`, `request_emit.rs`, `model.rs`, `paths.rs`; then regenerate bindings (`UPDATE_BINDINGS=1 cargo test bindings`) |
| Add/modify a Tauri command | write fn in `<module>/commands.rs`, register in `src-tauri/src/ipc.rs` `builder()`, regenerate `src/ipc/bindings.ts`, wrap in `src/features/<x>/api.ts` |
| Error messages / codes | `src-tauri/src/error.rs`, `src/ipc/errors.ts` |
| GraphQL schema explorer / introspection | `src/features/graphql/SchemaExplorer.tsx`, `explorerModel.ts`; BE `src-tauri/src/engine/graphql/introspection.rs` |
| GraphQL subscriptions | `src/features/graphql/Subscription*.tsx`; BE `src-tauri/src/engine/graphql/subscription*.rs`, `gql_ws.rs` |
| SOAP / WSDL import and envelope generation | `src/features/soap/*`; BE `src-tauri/src/engine/soap/wsdl.rs`, `envelope.rs`, `xsd.rs` |
| gRPC calls, reflection, proto import | `src/features/grpc/*`; BE `src-tauri/src/engine/grpc/{call,reflection,protos}.rs` |
| WebSocket client | `src/features/ws/*`; BE `src-tauri/src/engine/ws/{session,transport}.rs` |
| Git dialog (commit/pull/push/branches) | `src/features/git/GitDialog.tsx`, `gitModel.ts`; BE `src-tauri/src/git/service.rs`, `runner.rs` |
| Postman import mapping | BE `src-tauri/src/interop/postman/import/*.rs`, `stage.rs`; UI `src/features/postman/ImportPostmanDialog.tsx` |
| Postman export mapping | BE `src-tauri/src/interop/postman/export/*.rs`, `export_flow.rs`; UI `src/features/postman/ExportPostmanDialog.tsx` |
| cURL import/export | BE `src-tauri/src/interop/curl/{import,export,build}.rs`; UI `src/features/curl/` |
| Proxy / client certs / TLS settings | `src/features/network/*`; BE `src-tauri/src/engine/http/network.rs`, `proxy.rs`, `identity.rs` |
| Cookie handling | `src/features/cookies/*`; BE `src-tauri/src/engine/http/cookies.rs`, `cookie_jars.rs` |
| Mock route rule (operators/targets/response modes) | BE `src-tauri/src/mock/rules.rs`, `matcher.rs`, `engine.rs`, `types.rs`; FE `src/features/mocks/RulesEditor.tsx`, `mockModel.ts` |
| Mock route kinds (http/crud/ws), CRUD data, templating, TLS | BE `src-tauri/src/mock/crud.rs`, `ws.rs`, `template.rs`, `tls.rs`; FE `RouteEditor.tsx`, `WsRouteEditor.tsx`, `DataEditor.tsx` |
| Mock callbacks / fallback / proxy | BE `src-tauri/src/mock/proxy.rs`, `engine.rs`; FE `CallbacksEditor.tsx`, `FallbackEditor.tsx` |
| Mock file format | `src-tauri/src/mock/model.rs`, `disk.rs` |
| Webhook listener config / response / replay | BE `src-tauri/src/webhook/{config,respond,replay,server}.rs`; FE `src/features/webhooks/ListenerEditor.tsx`, `ResponseEditor.tsx` |
| Journey node types | BE `src-tauri/src/journey/model.rs` (`NodeType`, validation), `runner.rs`; FE `src/features/journeys/journeyModel.ts` (NODE_LABELS, PALETTE, defaults), `StepCard.tsx`, `NodeInspector.tsx`; regenerate bindings |
| Journey edge mapping / checks / data | BE `src-tauri/src/journey/check.rs`, `data.rs`; FE `EdgeInspector.tsx`, `flowMap.ts` |
| Journey run display | FE `src/features/journeys/RunPanel.tsx`, `runState.ts`; BE `src-tauri/src/journey/runner.rs`, `types.rs` |
| History list behaviour | `src/features/history/HistoryPanel.tsx`; BE `src-tauri/src/store/history.rs` |
| Tab behaviour (open/close/dirty) | `src/shell/tabsStore.ts`, `src/shell/TabStrip.tsx`, `src/shell/DiscardDialog.tsx` |
| Sidebar width/collapse persistence | `src/shell/layoutStore.ts` |
| Logging / redaction | `src-tauri/src/logging/{mod,redact}.rs` |
| Collection file-watch / external change reload | `src-tauri/src/store/watcher.rs`, `fingerprint.rs`; FE `src/features/collections/collectionsStore.ts` (`subscribe`) |
| Script trust prompt for imported collections | `src-tauri/src/store/trust.rs`; FE `src/features/collections/ScriptsDialog.tsx` |

## Screens

Each screen lists where it is opened from, its components, state, IPC calls (frontend `api.ts` -> Rust), Rust logic and tests.

### 1.1 App shell (window)

- **Opened from:** always (app start).
- **Components:** `src/shell/AppShell.tsx` `AppShell`. Children: `MenuBar`, `Toolbar`, `Sidebar`, `Splitter` (`src/ui`), `TabStrip`, `TabPanel`, `StatusBar`, `ShellDialogs`, `ToastHost`.
- **State:** `useLayoutStore`, `useTabsStore`, `useDialogsStore`; store subscriptions (collections, env, webhook, mock, journey).
- **IPC at start:** collections, scratch, network `loadProxyMode`, webhook list (see each feature).
- **Rust:** `src-tauri/src/lib.rs`.
- **Tests:** `src/App.test.tsx`, `src/shell/*.test.tsx`, `e2e/smoke.spec.ts`.
- **Native menu:** `src-tauri/src/menu.rs` (`build`, Cmd+W rebinding), macOS only (`#[cfg(target_os="macos")]`).

### 1.2 Menu bar (File / Edit / Request / View)

- **Opened from:** the top menu bar.
- **Components:** `src/shell/MenuBar.tsx` `MenuBar` (uses `src/ui/Menu.tsx`).
- **Menu items:**
  - File: New Tab, Save, Import cURL, Import Postman, Export to Postman, Clone Repository, Close Tab.
  - Edit: Network settings.
  - Request: Send, Copy as cURL, Manage cookies.
  - View: Toggle Sidebar.
- **Dialogs:** opened via `dialogsStore` (`ShellDialog`: network, cookies, import-curl, import-postman, export-postman, git, clone-git), rendered in `src/shell/ShellDialogs.tsx`.
- **Shortcuts:** `src/shell/useShortcuts.ts`.
- **Tests:** `src/shell/MenuBar.test.tsx`.

### 1.3 Toolbar

- **Opened from:** top of the window, below the menu bar.
- **Components:** `src/shell/Toolbar.tsx` `Toolbar`: New Tab, sidebar toggle, `EnvSwitcher`, `ThemeSwitcher`.
- **Tests:** `src/shell/Toolbar.test.tsx`.

### 1.4 Sidebar (accordion: Collections, Scratch, Webhooks, Mocks, Journeys, History)

- **Opened from:** the left sidebar, one panel per section.
- **Components:** `src/shell/Sidebar.tsx` `Sidebar`. Section order: `NAV_SECTIONS` in `src/shell/layoutStore.ts`. Labels and icons: `src/shell/navSections.ts`.

**Collections**
- **Component:** `src/features/collections/CollectionsPanel.tsx` `CollectionsPanel`.
- **Children:** `TreeView.tsx`, `TreeRow.tsx`, `NameDialog`, `ConfirmDialog`, `ScriptsDialog`.
- **Helpers:** `treeRows.ts`, `rowMenu.ts`, `requestKind.ts`, `nameSchema.ts`.
- **State:** `collectionsStore.ts`.
- **IPC** (`src/features/collections/api.ts`): list/create/open/closeCollection, readTree, createFolder, createRequest, renameNode, deleteNode, moveNode, readRequest, writeRequest, readScripts, writeScripts, setScriptsTrusted -> `src-tauri/src/store/commands.rs` (same-named functions).
- **Rust logic:** `store/service.rs`, `store/tree_ops.rs`, `store/collection.rs`.
- **Tests:** `CollectionsPanel.test.tsx`, `dialogs.test.tsx`, `e2e/storage.spec.ts`.

**Scratch**
- **Component:** `src/features/scratch/ScratchPanel.tsx` `ScratchPanel`; children `MoveToCollectionDialog`; state `scratchStore.ts`.
- **IPC:** listScratch, createScratch, renameScratch, deleteScratch, moveScratchToCollection -> `src-tauri/src/store/commands.rs`; logic `store/scratch.rs`.

**Webhooks:** see 1.12. **Mocks:** see 1.11. **Journeys:** see 1.13.

**History**
- **Component:** `src/features/history/HistoryPanel.tsx` `HistoryPanel`; state `historyStore.ts`; helper `relativeTime.ts`.
- **IPC:** listHistory, clearHistory, restoreHistoryEntry -> `src-tauri/src/store/commands.rs`; logic `store/history.rs`.
- **Tests:** `HistoryPanel.test.tsx`.

### 1.5 Tab strip, tab panel and Welcome

- **Opened from:** the tab strip under the toolbar.
- **Components:** `src/shell/TabStrip.tsx` `TabStrip`; `src/shell/TabPanel.tsx` `TabPanel` (`renderTab` dispatches journey, mock, webhook, welcome, request; inline `Welcome` component, "Welcome to Papan"); `src/shell/DiscardDialog.tsx` (unsaved changes).
- **State:** `src/shell/tabsStore.ts` (`Tab`, `TabKind`: welcome, request, webhook, mock, journey).
- **Tests:** `TabStrip.test.tsx`, `TabPanel.test.tsx`, `tabsStore.test.ts`.

### 1.6 Request editor (all kinds)

- **Opened from:** a request tab (kind `request`).
- **Component:** `src/features/request/RequestEditor.tsx` `RequestEditor`.
- **Children:** `UrlBar.tsx`; `ParamsEditor`; `KeyValueTable` (headers); `BodyEditor` (+ `MultipartEditor`, `xmlFormat.ts`); `AuthEditor` / `AuthFields` / `OAuthFields` / `OAuthTokenPanel`; `ScriptsEditor`; `SettingsEditor`; response pane = `ResponseViewer` (1.7) split by `src/ui/Splitter.tsx`.
- **Helpers:** `sections.ts` (`sectionsFor(kind)`, sub-tab list per kind), `snippets.ts`, `content.ts`, `params.ts`.
- **State:** `requestStore.ts` (per-tab draft); `oauthStore.ts`, `oauthApi.ts`.
- **Sub-tabs per kind** (`sections.ts`):
  - http: Params, Headers, Body, Auth, Scripts, Settings.
  - graphql: adds Query, Variables, Subscription.
  - soap: adds Body, SOAP.
  - ws: Params, Headers, Messages, Subprotocols, Auth, Settings.
  - grpc: Message, Metadata, Protos, Auth, Settings.
- **Kind selector options:** `src/features/collections/requestKind.ts` (HTTP, GraphQL, SOAP, WebSocket, gRPC).
- **IPC** (`src/features/request/api.ts`): sendRequest, cancelRequest, pickBodyFile, readResponseBody, saveResponseBody, clearCookies -> `src-tauri/src/engine/http/commands.rs` (`send_request` etc.).
- **Rust logic:** `engine/http/service.rs` (`HttpEngine::send`), `build.rs`, `resolve.rs`, `send.rs`, `client.rs`, `redirect.rs`, `follow.rs`, `digest.rs`, `proxy.rs`, `variables.rs`, `mask.rs`, `inflight.rs`.
- **OAuth IPC** (`oauthApi.ts`): oauthStatus, oauthAuthorize, oauthCancel, oauthClear -> `src-tauri/src/engine/http/oauth/commands.rs`; logic `oauth/{flow,authorize,listener,pkce,token,tokens}.rs`.
- **Tests:** `RequestEditor.test.tsx`, `RequestEditor.kinds.test.tsx`, `UrlBar`/`BodyEditor`/`AuthEditor` tests; `e2e/http.spec.ts`, `e2e/advanced.spec.ts`. Rust: `tests/http_send.rs`, `http_advanced.rs`, `http_oauth.rs`, `oauth.rs`, `http_cookies.rs`.

### 1.6a GraphQL editor

- **Opened from:** a request tab of kind GraphQL; rendered by `src/features/request/RequestEditor.tsx`.
- **Component:** `src/features/graphql/GraphqlEditor.tsx` `GraphqlEditor` (Query and Variables sub-tabs). Children: `SchemaExplorer.tsx`, `VariablesEditor.tsx`.
- **Subscription:** `SubscriptionPanel.tsx`, `SubscriptionBar.tsx`, `SubscriptionSettings.tsx`, `ResultLog.tsx`.
- **Helpers / state:** `schema.ts`, `operations.ts`, `explorerModel.ts`, `subscriptionModel.ts`, `graphqlStore.ts`.
- **IPC** (`src/features/graphql/api.ts`): introspectGraphql, readCachedSchema, loadSchemaFile, clearCachedSchema, cancelRequest -> `src-tauri/src/engine/graphql/commands.rs`; logic `engine/graphql/{service,introspection,driver,cache}.rs`.
- **Subscriptions** (`src/features/ws/api.ts`): graphqlSubscribe, graphqlUnsubscribe -> `engine/graphql/subscription_commands.rs`; logic `subscription_service.rs`, `subscription.rs`, `gql_ws.rs`.
- **Storage:** schema is cached; models `store/protocol_model.rs`, `store/subscription_model.rs`.
- **Tests:** `GraphqlEditor*.test.tsx`, `SchemaExplorer.test.tsx`; `e2e/graphql.spec.ts`, `e2e/graphqlSubscription.spec.ts`; `tests/graphql_*.rs`. Fixtures: `src/test/graphql/`.

### 1.6b SOAP panel

- **Opened from:** the SOAP sub-tab of a SOAP request; rendered by `src/features/request/RequestEditor.tsx`.
- **Component:** `src/features/soap/SoapPanel.tsx` `SoapPanel` (default export). Children: `OperationList.tsx`, `WsdlImportDialog.tsx`. Helpers: `applyTemplate.ts`; styles `soap.css`.
- **IPC** (`src/features/soap/api.ts`): importWsdl, readCachedWsdl, soapEnvelope -> `src-tauri/src/engine/soap/commands.rs`.
- **Rust logic:** `engine/soap/{service,wsdl,wsdl_read,xsd,walk,envelope,rules,fetch,cache}.rs`.
- **Storage model:** `store/protocol_model.rs` (`soap`).
- **Tests:** `SoapPanel.test.tsx`; `e2e/soap.spec.ts`; `tests/soap_import.rs`, `soap_fetch_rules.rs`.

### 1.6c gRPC

- **Opened from:** a request tab of kind gRPC; `GrpcBar` and `CallPanel` are rendered by `src/features/request/RequestEditor.tsx`.
- **Components:** `src/features/grpc/CallPanel.tsx` `CallPanel` (message and log view); `GrpcBar.tsx` (connect and send bar); `MessageEditor.tsx`; `ProtoSourcesEditor.tsx` ("Protos" tab); `DeadlineField.tsx`. The log reuses `src/features/ws/MessageLog.tsx`.
- **Helpers / state:** `useGrpcSchema.ts`, `grpcStore.ts`, `grpcModel.ts`; styles `grpc.css`.
- **IPC** (`src/features/grpc/api.ts`): grpcSchema, grpcReflect, grpcImportProto, grpcStart, grpcSend, grpcEnd, grpcCancel, grpcLog, grpcClearLog -> `src-tauri/src/engine/grpc/commands.rs`.
- **Rust logic:** `engine/grpc/{service,call,channel,connect,reflection,protos,import,schema,codec,metadata,registry,cache,sink}.rs`.
- **Event:** `GrpcEvent` (`engine/grpc/types.rs`).
- **Storage:** `store/grpc_model.rs`, `store/grpc_protos.rs`.
- **Tests:** `CallPanel`/`GrpcBar`/`MessageEditor`/`ProtoSourcesEditor` `.test.tsx`; `e2e/grpc.spec.ts`; `tests/grpc_*.rs`.

### 1.6d WebSocket

- **Opened from:** a request tab of kind WebSocket; `ConnectBar` and `WsPanel` are rendered by `src/features/request/RequestEditor.tsx`.
- **Component:** `src/features/ws/WsPanel.tsx` `WsPanel`. Children: `ConnectBar.tsx`, `MessageComposer.tsx`, `MessageLog.tsx`, `SubprotocolsEditor.tsx`, `TemplatesEditor.tsx` ("Messages" tab).
- **Helpers / state:** `wsStore.ts`, `wsModel.ts`; styles `ws.css`.
- **IPC** (`src/features/ws/api.ts`): wsConnect, wsSend, wsDisconnect, wsLog, wsClearLog -> `src-tauri/src/engine/ws/commands.rs`.
- **Rust logic:** `engine/ws/{session,transport,handshake,messages,log,sink,types}.rs`.
- **Event:** `WsEvent`. **Storage:** `store/ws_model.rs`.
- **Tests:** `WsPanel.test.tsx` etc.; `e2e/ws.spec.ts`; `tests/ws_echo.rs`, `ws_transport.rs`.

### 1.7 Response viewer

- **Opened from:** the response pane of the request editor (1.6).
- **Component:** `src/features/response/ResponseViewer.tsx` `ResponseViewer`. Children:
  - `BodyView.tsx` (with `JsonTree.tsx`, `HtmlPreview.tsx`);
  - `TimingBar.tsx`;
  - `SearchBar.tsx` + `search.ts`;
  - `SaveToVariableDialog.tsx` + `saveToVariable.ts`;
  - `scriptReports.tsx` (script console and tests).
- **Helpers:** `detect.ts` (content sniff), `format.ts`, `size.ts`, `clipboard.ts`; styles `response.css`, `viewer.css`.
- **State:** opens `dialogsStore` entries (for example cookies).
- **IPC** (`src/features/response/api.ts`): readResponseBody, saveResponseBody -> `src-tauri/src/engine/http/commands.rs`. The body file is written by `engine/http/send.rs` / `responses.rs`.
- **Tests:** `ResponseViewer.test.tsx`, `JsonTree.test.tsx`, etc.; fixtures `src/test/responses/`.

### 1.8 Environments, variables, secrets

- **Opened from:** `EnvSwitcher` in the toolbar.
- **Components:** `src/features/environments/EnvSwitcher.tsx` `EnvSwitcher` -> `EnvManager.tsx` `EnvManager` (dialog). Children: `VariableTable.tsx`, `SaveSecretDialog.tsx` (also opened from `src/features/request/AuthFields.tsx`).
- **Helpers / hooks:** `useScopeRows.ts`, `variableRows.ts`, `template.ts` (`{{var}}` parse), `useEffectiveVariables.ts`, `useActiveLocation.ts`, `unresolvedAttrs.ts`.
- **State:** `envStore.ts`; styles `environments.css`.
- **IPC** (`src/features/environments/api.ts`): listEnvironments, createEnvironment, renameEnvironment, deleteEnvironment, selectEnvironment, readVariables, writeVariables, effectiveVariables, previewTemplate, secretStorageStatus, resetSecretStorage -> `src-tauri/src/vars/commands.rs`.
- **Rust logic:** `vars/{service,resolve,template,dynamic,scopes,local,write,persist,types}.rs`; secrets in `vars/vault/`; env files `store/environments.rs`, `store/env_ops.rs`.
- **Tests:** `EnvManager`/`EnvSwitcher`/`SaveSecretDialog` `.test.tsx`, `template.test.ts`; `e2e/env.spec.ts`; `tests/vars_*.rs`, `vault.rs`, `http_vars.rs`.

### 1.9 Scripts editor

- **Request level:** `src/features/request/ScriptsEditor.tsx` `ScriptsEditor` (the "Scripts" sub-tab; snippets in `snippets.ts`).
- **Collection / folder level:** `src/features/collections/ScriptsDialog.tsx` `ScriptsDialog` (row menu "Scripts..." in `rowMenu.ts` / `CollectionsPanel.tsx`).
- **IPC:** readScripts, writeScripts, setScriptsTrusted -> `src-tauri/src/store/commands.rs`; logic `store/script_ops.rs`, `script_model.rs`, `trust.rs`.
- **Runtime:** `src-tauri/src/script/` (see [Shared pieces](#shared-pieces)).
- **Tests:** `ScriptsEditor.test.tsx`, `ScriptsDialog.test.tsx`; `tests/script_*.rs`.

### 1.10 Git

- **Opened from:** row menu in `CollectionsPanel.tsx`, which calls `dialogsStore.show("git", id)`. Also File > Clone Repository.
- **Components:** `src/features/git/GitDialog.tsx` `GitDialog` (`gitModel.ts`); `src/features/git/CloneDialog.tsx` `CloneDialog`.
- **IPC** (`src/features/git/api.ts`): gitStatus, gitInit, gitClone, gitDiff, gitCommit, gitPull, gitPush, gitBranches, gitSwitch, gitCreateBranch, gitResolve -> `src-tauri/src/git/commands.rs`.
- **Rust logic:** `git/{service,runner,parse,types}.rs` (shells out to git).
- **Note:** cloned collections start untrusted (`store/trust.rs`).
- **Tests:** `GitDialog.test.tsx`, `CloneDialog.test.tsx`, `gitModel.test.ts`; `git/service_tests.rs`.

### 1.10b Postman import/export, cURL, network, cookies (menu dialogs)

- **Postman import:** `src/features/postman/ImportPostmanDialog.tsx` + `ImportReport.tsx`. **Export:** `ExportPostmanDialog.tsx`. Model: `postmanModel.ts`.
  - IPC (`src/features/postman/api.ts`): postmanImportPick, postmanImportCommit, postmanImportDiscard, postmanExport -> `src-tauri/src/interop/postman/commands.rs`.
  - Logic: `interop/postman/{flow,stage,export_flow,scripts,limits,report,types}.rs`, `import/*.rs`, `export/*.rs`.
  - Tests: `tests/postman_*.rs`; `e2e/postman.spec.ts`.
- **cURL:** `src/features/curl/ImportCurlDialog.tsx`, `copyAsCurl.ts`.
  - IPC: importCurl, exportCurl -> `src-tauri/src/engine/http/net_commands.rs`; logic `interop/curl/*.rs`.
  - Tests: `tests/curl.rs`.
- **Network settings** (Edit > Network settings): `src/features/network/NetworkDialog.tsx` (`ProxyForm.tsx`, `CertificateList.tsx`, `networkStore.ts`).
  - IPC: readNetworkSettings, readProxyMode, writeProxySettings, addClientCertificate, removeClientCertificate -> `engine/http/net_commands.rs`.
  - Logic: `engine/http/{network,proxy,identity}.rs`.
  - Tests: `network.test.tsx`; `tests/http_network.rs`.
- **Cookies** (Request > Manage cookies): `src/features/cookies/CookieManager.tsx` (`CookieForm.tsx`, `cookieRows.ts`).
  - IPC: listCookies, setCookie, deleteCookies, clearCookies -> `engine/http/net_commands.rs` / http commands.
  - Logic: `engine/http/cookies.rs`, `cookie_jars.rs`.

### 1.11 Mock server designer

- **Opened from:** sidebar Mocks -> `src/features/mocks/MocksPanel.tsx` `MocksPanel`. Opens a tab (kind `mock`).
- **Components:** `src/features/mocks/MockTab.tsx` `MockTab`, with `RouteEditor.tsx` (`RulesEditor.tsx`, `HeadersEditor.tsx`, `CallbacksEditor.tsx`, `WsRouteEditor.tsx`), `FallbackEditor.tsx`, `DataEditor.tsx`, `MockSettings.tsx`, `MockLog.tsx`.
- **State / helpers:** `mockStore.ts`, `mockModel.ts`; styles `mocks.css`.
- **IPC** (`src/features/mocks/api.ts`): listMocks, readMock, saveMock, deleteMock, startMock, stopMock, mockLog, clearMockLog, mockFromCollection, importMock, exportMock -> `src-tauri/src/mock/commands.rs`.
- **Rust logic:** `mock/{engine,matcher,rules,server,crud,ws,ws_upgrade,template,proxy,tls,openapi,mockers,from_collection,files,request,types,model,disk}.rs`.
- **Events:** MockLogged, MockStopped.
- **Tests:** `MockTab`, `MocksPanel`, `mockStore`, `mockModel` tests; `mock/service_tests.rs`, `ws_tests.rs`.

### 1.12 Webhook receiver

- **Opened from:** sidebar Webhooks -> `src/features/webhooks/WebhooksPanel.tsx` `WebhooksPanel`. Opens a tab (kind `webhook`).
- **Components:** `src/features/webhooks/WebhookTab.tsx` `WebhookTab`, with `ListenerEditor.tsx`, `ResponseEditor.tsx`, `CaptureList.tsx`, `CaptureDetail.tsx`.
- **Helpers / state:** `schema.ts`, `curl.ts`, `webhookStore.ts`; styles `webhooks.css`.
- **IPC** (`src/features/webhooks/api.ts`): listWebhooks, saveWebhook, deleteWebhook, startWebhook, stopWebhook, listCaptures, readCapture, clearCaptures, replayCapture -> `src-tauri/src/webhook/commands.rs`.
- **Rust logic:** `webhook/{server,capture,respond,replay,config,types}.rs`.
- **Events:** WebhookCaptured, WebhookStopped.
- **Tests:** `webhooks.test.tsx`, `webhookStore.test.ts`; `e2e/webhooks.spec.ts`; `tests/webhook_*.rs`.

### 1.13 Journey canvas

- **Opened from:** sidebar Journeys -> `src/features/journeys/JourneysPanel.tsx` `JourneysPanel`. Opens a tab (kind `journey`).
- **Components:** `src/features/journeys/JourneyTab.tsx` `JourneyTab` -> `JourneyCanvas.tsx` `JourneyCanvas`. Children: `StepCard.tsx` (nodes), `NodeInspector.tsx`, `EdgeInspector.tsx`, `RunPanel.tsx`.
- **Helpers / state:** `flowMap.ts` (graph mapping), `runState.ts`, `journeyStore.ts`, `journeyModel.ts` (`NODE_LABELS`, `PALETTE`, defaults per node type); styles `journeys.css`.
- **Node types** (BE enum `NodeType` in `journey/model.rs`; TS type from bindings): start, end, request, script, condition, delay, assert, loop.
- **IPC** (`src/features/journeys/api.ts`): listJourneys, readJourney, saveJourney, deleteJourney, runJourney, cancelJourney -> `src-tauri/src/journey/commands.rs`.
- **Rust logic:** `journey/{runner,check,data,host,model,disk,types}.rs`.
- **Event:** JourneyRunEvent.
- **Tests:** `JourneyTab`, `NodeInspector`, `EdgeInspector`, `RunPanel`, `flowMap` tests; `e2e/journeys.spec.ts`; `tests/journey_run.rs`, `journey/runner_tests.rs`.

### 1.14 Settings / About / Theme

- **There is no dedicated Settings or About screen.**
- **Theme:** `ThemeSwitcher` in the Toolbar (see [Shared pieces](#shared-pieces)).
- **App name and version:** shown in `src/shell/StatusBar.tsx` via `src/shell/useAppInfo.ts` -> IPC `appInfo` (bindings) -> `src-tauri/src/commands.rs` `app_info`; type `app_info.rs` `AppInfo`.
- **Network / proxy:** Edit > Network settings (see 1.10b).
- **Per-request settings:** `src/features/request/SettingsEditor.tsx`.

### 1.15 Toasts

- **Component:** `src/ui/Toast.tsx` `ToastHost` (mounted in `AppShell`).
- **State:** `src/ui/toastStore.ts` (`useToastStore`).

## Shared pieces

### Theme and styles

- Tokens: `src/theme/tokens.css` (all CSS variables: fonts, sizes, `:root[data-theme="light"]` and `[data-theme="dark"]` colours). Contrast checked by `src/theme/contrast.test.ts`.
- Global element styles: `src/theme/base.css`.
- Mode: `src/theme/themeStore.ts` (zustand; `ThemeMode` = system | light | dark). `src/theme/useApplyTheme.ts` sets `<html data-theme>` and is called in `src/App.tsx`.
- Switcher UI: `src/shell/ThemeSwitcher.tsx`.
- Per-feature CSS: `src/shell/shell.css`, `src/ui/ui.css`, and `src/features/*/` (`request`, `response`, `viewer`, `collections`, `environments`, `ws`, `grpc`, `soap`, `mocks`, `webhooks`, `journeys`) `.css` files.
- Code editor (CodeMirror) theme: `src/ui/code/theme.ts`.

### src/ui (shared widgets)

- `Button.tsx`, `Dialog.tsx`, `Menu.tsx`, `Tabs.tsx`, `Splitter.tsx`, `Toast.tsx` + `toastStore.ts`.
- `icons/`: `Icon.tsx`, `names.ts`, `glyphs/{actions,files,protocols,chrome}.tsx`.
- `code/`: `CodeEditor.tsx`, `setup.ts`, `theme.ts`, `varHighlight.ts` (`{{var}}` highlighting), `searchMarks.ts`, `CodeEditor.mock.tsx`.
- `safeStorage.ts`, `memoryStorage.ts`, `platform.ts` (`shortcutLabel`), `ui.css`.
- Tests sit alongside (for example `Button.test.tsx`).

### src/shell (app shell)

- `AppShell.tsx` (layout, store loading and subscriptions), `MenuBar.tsx`, `Toolbar.tsx`, `Sidebar.tsx`, `navSections.ts`, `layoutStore.ts`, `TabStrip.tsx`, `TabPanel.tsx`, `tabsStore.ts`, `DiscardDialog.tsx`, `ShellDialogs.tsx` + `dialogsStore.ts`, `StatusBar.tsx` + `useAppInfo.ts`, `useShortcuts.ts`, `activeRequest.ts` (save and send the active request), `ThemeSwitcher.tsx`, `shell.css`.
- Entry: `src/main.tsx` -> `src/App.tsx`.
- `src/features/locations.ts`: request location helpers.

### IPC and binding generation

- Registry: `src-tauri/src/ipc.rs` `builder()` (`collect_commands!`, `collect_events!`, `.typ::<AppError>()`). A new command, event or type is added here.
- Output: `src/ipc/bindings.ts` (generated; exports `commands`, `events` and types). Never hand-edit.
- Regenerate: start the app in debug (`export_bindings` in `src-tauri/src/lib.rs`), or run `UPDATE_BINDINGS=1 cargo test bindings`.
- Check: plain `cargo test` fails if the committed file is stale (`bindings_path()`, `render_bindings()`, `sync_bindings` in `src-tauri/src/ipc.rs`).
- Errors: `src-tauri/src/error.rs` (`AppError`, `ErrorCode`); frontend `src/ipc/errors.ts`.
- Tauri setup: `src-tauri/src/lib.rs` `run()`; config `src-tauri/tauri.conf.json` (productName Papan, icons list); capabilities `src-tauri/capabilities/default.json`; build script `src-tauri/build.rs`.

### Scripting runtime (pre-request and post-response, `pm.*`)

- `src-tauri/src/script/{sandbox,pm,context,chain,watchdog,report}.rs`.
- HTTP bridge: `src-tauri/src/engine/http/{scripted,script_bridge,script_map}.rs`.
- Variable writes: `src-tauri/src/vars/persist.rs`.
- Tests: `src-tauri/tests/script_*.rs`.

### On-disk storage formats

| What | Defined in |
|---|---|
| Request file `<name>.http.yaml`, `_folder.yaml` (suffix consts) | `src-tauri/src/store/paths.rs` (`REQUEST_SUFFIX`, `FOLDER_FILE`); schemas `store/model.rs`, `request_model.rs`, `request_emit.rs`, `auth_model.rs`, `script_model.rs`, `protocol_model.rs` (kind/graphql/soap), `ws_model.rs`, `grpc_model.rs`, `subscription_model.rs`; YAML I/O `store/yaml.rs` |
| `papan.collection.yaml` | `store/model.rs`, `store/collection.rs` |
| Environments `environments/<stem>.env.yaml`, collection vars, `<appdata>/globals.yaml` | `store/env_model.rs`, `store/environments.rs` (`ENVIRONMENTS_DIR`, `ENV_SUFFIX`), `store/env_ops.rs` |
| Open-collection registry `collections.json` | `store/registry.rs` |
| Scratch requests `scratch/`, history `history.json` | `store/scratch.rs` (`SCRATCH_DIR`), `store/history.rs` |
| Untrusted collections `untrusted-collections.json` | `store/trust.rs` |
| Secrets (memory-only literals) / vault | `store/secrets.rs`; `vars/vault/{mod,keys,os_keys,file_vault,sealed,crypto}.rs` |
| gRPC protos storage | `store/grpc_protos.rs` (`SCRATCH_PROTOS_DIR`) |
| Mock `mocks/<slug>.mock.yaml` | `mock/model.rs`, `mock/disk.rs` |
| Journey `journeys/<slug>.journey.yaml` | `journey/model.rs`, `journey/disk.rs` |
| Webhook listeners `webhooks.json` | `webhook/config.rs` |
| `network.json` (proxy, client certs) | `engine/http/network.rs` (+ `identity.rs`) |
| Cookie jars | `engine/http/cookie_jars.rs`, `cookies.rs` |
| File watcher / change fingerprint | `store/watcher.rs`, `store/fingerprint.rs` |
| UI prefs (localStorage) | theme `papan.theme`, layout `papan.layout` |

Roundtrip tests: `src-tauri/tests/store_roundtrip.rs`, `store_paths.rs`, `store_scripts.rs`, `store_secrets.rs`, `store_watcher.rs`, `vars_store.rs`, `vault.rs`.

### Test commands

- Unit tests: `npm test` (vitest; setup `src/test/setup.ts`; fixtures `src/test/fixtures.ts`).
- E2E: `e2e/*.spec.ts` with mocks `e2e/tauriMock.ts` and `e2e/*Mock.ts` (`playwright.config.ts`, `testDir` "e2e").
- Rust integration tests: `src-tauri/tests/*.rs` (+ `tests/support/`).

## Known gaps

- No Settings or About screen. Only the theme switcher (Toolbar), the app name and version (StatusBar), Edit > Network settings and the per-request Settings tab exist.
- Some on-disk storage is described by module only, with no file name in these notes: cookie jars (`engine/http/cookie_jars.rs`, `cookies.rs`), secrets (`store/secrets.rs`, vault modules), and gRPC protos (`store/grpc_protos.rs`).
- The `src/features/graphql/Subscription*.tsx` and `engine/graphql/subscription*.rs` entries are glob shorthand, not exact file lists.
- Test lists are partial: several entries end in "etc." or "...", so they are not exhaustive.

Regenerate or update this file when screens move; paths were checked on 2026-11-17.
