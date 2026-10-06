// What the interface needs from the rest of the app, set once at start-up by
// app.js: { platform }.
export const ctx = {};
export function setContext(values) { Object.assign(ctx, values); }

/** Where a route lives in the URL, and back. Browse places: all, unsorted, favs,
 *  schema:<id>[/<category value>…] (values URI-encoded). */
export function routeHash(route) {
  if (route === "home") return "#/";
  if (route.startsWith("browse:")) return `#/browse/${route.slice(7).replace(/^schema:/, "schema/")}`;
  return `#/${route}`;
}

export function routeFromHash(hash) {
  const path = hash.replace(/^#\/?/, "").split("?")[0];
  if (!path) return "home";
  const [first, ...rest] = path.split("/");
  if (first === "browse") {
    if (rest[0] === "schema" && rest[1]) return `browse:schema:${rest.slice(1).join("/")}`;
    return `browse:${rest[0] || "all"}`;
  }
  return first === "settings" || first === "import" ? first : "home";
}

/** A browse place for a schema and some of its category values. */
export const schemaScope = (id, values = []) => ["schema:" + id, ...values.map(encodeURIComponent)].join("/");
