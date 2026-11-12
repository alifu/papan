export const ICON_NAMES = [
  "folder", "folder-open", "request-http", "request-graphql", "request-grpc", "request-soap",
  "request-ws", "webhook", "environment", "git", "mock", "journey", "play", "stop", "plus",
  "close", "settings", "sun", "moon", "monitor", "chevron-left", "chevron-right", "chevron-down",
  "search", "history",
] as const;

export type IconName = (typeof ICON_NAMES)[number];
