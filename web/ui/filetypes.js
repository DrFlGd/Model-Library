// One mark per file type, the same everywhere a file is listed (docs/PLAN.md,
// "Import follow-ups design"): a coloured tag with the type's name. STL, 3MF and
// ZIP-like archives each have their own colour; other 3D files, slicer files,
// pictures, documents and videos one per kind. Folders keep the folder icon.
import { html } from "../lib/html.js";

const GROUPS = {
  stl: ["stl"],
  "3mf": ["3mf"],
  "3d": ["obj", "step", "stp", "ply", "amf", "iges", "igs", "f3d", "blend", "3ds", "max"],
  slicer: ["gcode", "bgcode", "lys", "lyt", "chitubox", "ctb", "cbddlp", "goo", "prz", "fabbproject", "3mfproject", "ufp"],
  archive: ["zip", "7z", "rar", "tar", "gz"],
  image: ["jpg", "jpeg", "png", "webp", "gif", "bmp", "avif"],
  doc: ["pdf", "md", "txt", "html", "htm", "rtf", "doc", "docx"],
  video: ["mp4", "webm", "mov", "m4v", "mkv", "avi"],
};
const GROUP_OF = new Map(Object.entries(GROUPS).flatMap(([g, exts]) => exts.map((e) => [e, g])));
const LABELS = { jpeg: "JPG", stp: "STEP", igs: "IGES", chitubox: "CHITU", fabbproject: "NETFABB", "3mfproject": "3MF PROJ", htm: "HTML" };
const WHAT = { stl: "STL 3D file", "3mf": "3MF 3D file", "3d": "3D file", slicer: "slicer file", archive: "archive", image: "picture", doc: "document", video: "video", other: "file" };

/** A file type by its extension: { ext, group, label }. */
export function typeOfExt(ext) {
  const e = (ext || "").toLowerCase();
  return { ext: e, group: GROUP_OF.get(e) || "other", label: LABELS[e] || (e ? e.toUpperCase() : "FILE") };
}

/** A file's type, by its name. A set's later files (name.7z.002, name.r00) are archives too. */
export const fileType = (name) => /\.7z\.\d{3}$/i.test(name || "") ? typeOfExt("7z") : /\.r\d\d$/i.test(name || "") ? typeOfExt("rar")
  : typeOfExt((/\.([^./\\]+)$/.exec(name || "") || [])[1] || "");

/** Whether the app can look inside a file (docs/PLAN.md, "Group 4 design"): a ZIP,
 *  7z or RAR, or the first file of a set (name.7z.001, name.part1.rar). */
export function isArchive(name) {
  const n = String(name || "").toLowerCase();
  if (n.endsWith(".zip") || n.endsWith(".7z") || n.endsWith(".7z.001")) return true;
  if (!n.endsWith(".rar")) return false;
  const part = /\.part(\d+)\.rar$/.exec(n);
  return !part || Number(part[1]) === 1;
}

/** The tag for one file (`name`) or one type (`ext`), with how many when more than one. */
export function TypeTag({ name, ext, count }) {
  const t = ext != null ? typeOfExt(ext) : fileType(name);
  const what = t.group === "stl" || t.group === "3mf" ? WHAT[t.group] : `${t.label} ${WHAT[t.group]}`;
  return html`<span class=${`ft ft-${t.group}`} data-type=${t.ext || "file"} title=${count > 1 ? `${count} ${what}s` : what}>${t.label}${count > 1 ? html`<span class="ft-n">${count}</span>` : null}</span>`;
}

/** The types a model holds (its summary's `exts`), most first. */
export function TypeTags({ exts, max = 3 }) {
  const list = Object.entries(exts || {}).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  if (!list.length) return null;
  return html`<span class="ft-tags">${list.slice(0, max).map(([e, n]) => html`<${TypeTag} key=${e} ext=${e} count=${n} />`)}${list.length > max ? html`<span class="ft-more muted" title=${list.slice(max).map(([e, n]) => `${n} ${typeOfExt(e).label}`).join(", ")}>+${list.length - max}</span>` : null}</span>`;
}
