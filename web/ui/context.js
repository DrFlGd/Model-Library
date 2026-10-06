// What the interface needs from the rest of the app, set once at start-up by
// app.js: { platform }.
export const ctx = {};
export function setContext(values) { Object.assign(ctx, values); }

/** Where a route lives in the URL, and back. */
export function routeHash(route) {
  if (route === "home") return "#/";
  if (route.startsWith("browse:")) return `#/browse/${route.slice(7)}`;
  return `#/${route}`;
}

export function routeFromHash(hash) {
  const path = hash.replace(/^#\/?/, "").split("?")[0];
  if (!path) return "home";
  const [first, rest] = path.split("/");
  if (first === "browse") return `browse:${rest || "all"}`;
  return first === "settings" ? "settings" : "home";
}
