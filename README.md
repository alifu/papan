# Tauri + React + Typescript

This template should help get you started developing with Tauri, React and Typescript in Vite.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## Webhooks

Papan can receive webhooks. Create a listener with a name, port and path, start it, and every request sent to that path is captured and shown in the list with its headers and body. You can set the status code and response headers the listener returns, and replay a capture as a scratch request to send it on to your own server.

- **Loopback by default.** A listener binds to `127.0.0.1`, so only programs on your machine can reach it. Port `0` picks any free port.
- **LAN warning.** Choosing "all interfaces" binds to `0.0.0.0`, which makes the listener reachable from your local network. Anyone on that network can then send requests to the listener, so use it only on networks you trust.
- **Exposing a listener.** To receive calls from the internet, tunnel the loopback port with [cloudflared](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/):

  ```sh
  cloudflared tunnel --url http://127.0.0.1:<port>
  ```

  or with [ngrok](https://ngrok.com/):

  ```sh
  ngrok http <port>
  ```

  Only the path you configured is captured. Other paths get `404`.
- **Memory only.** Captures are kept in memory and are cleared when Papan quits. Listener settings are saved, captures are not.

## gRPC

Papan sends gRPC calls from `kind: grpc` requests. A request names a service and a method, and its message is JSON in the proto3 mapping.

- **Protos folder.** A collection keeps its `.proto` files in `protos/` at the collection root. Requests refer to them by relative path, for example `greeter/v1/greeter.proto`. Scratch requests keep their files in the app's data folder. Only `.proto` files inside the folder are read.
- **Import .proto.** Importing a file copies it and the files it imports into the protos folder under the same relative names. A file that already exists with different content stops the import, and nothing is copied.
- **Import paths.** An `import` is looked up in the protos folder and then in each import path. Import paths are relative to the protos folder, and the default is `.`. Well-known types such as `google/protobuf/timestamp.proto` are built in.
- **Path rules.** Paths must be relative, must not contain `..`, and must not go through a symlink. Anything outside the protos folder is refused. A compile is limited to 200 files and 1 MiB per file.
- **Server reflection.** Instead of proto files, a request can ask the server for its definitions with reflection. Papan tries reflection v1 and falls back to v1alpha. Fetched definitions are cached on this machine, not in the collection. Refresh fetches them again.
- **TLS or plaintext.** The URL scheme decides. `https://` uses TLS and checks the server certificate unless "verify TLS" is off in Settings. `http://` sends gRPC without TLS, which suits local servers. Client certificates and HTTP CONNECT proxies work as they do for HTTP requests.
- **Metadata.** Headers are sent as gRPC metadata. Names ending in `-bin` take base64 values. Reserved names such as `content-type` and `grpc-*` are refused.
- **Streaming.** There are four method kinds: unary (one message in, one out), server streaming (one in, many out), client streaming (many in, one out) and bidirectional. A streaming call stays open until you send End to close the request side or Cancel to stop the call. Messages in both directions appear in the log in order.
- **Deadline.** An optional deadline in milliseconds ends the call with `DEADLINE_EXCEEDED` when it passes.
