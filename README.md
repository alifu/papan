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
