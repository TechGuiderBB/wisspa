export const PROMPT_ROUTE_EVENT = "wisspa://prompt-route";

export type PromptRoute = {
  session: number;
  app: string;
  host: string | null;
  branch: "prompt" | "content" | "unknown";
  source: "auto" | "manual";
};

export type RouteBadge = {
  text: string;
  tone: "ai" | "content" | "neutral";
};

// Prefer the sanitised host when present (strongest signal); fall back to app name.
export function routeLabel(route: PromptRoute): string {
  return route.host ?? route.app;
}

// Maps the resolved branch to a short display badge and a tone token that
// components translate into colours. "unknown" means the branch has not yet
// been classified (Sonnet has not returned, or the host was unrecognised).
export function routeBadge(route: PromptRoute): RouteBadge {
  switch (route.branch) {
    case "prompt":
      return { text: "Prompt", tone: "ai" };
    case "content":
      return { text: "Content", tone: "content" };
    case "unknown":
      return { text: "Detecting", tone: "neutral" };
    default:
      return { text: "Detecting", tone: "neutral" };
  }
}
