// Choosing where a model goes: a category (schema) or Unsorted, and a value for
// each of its levels, suggesting the values already in the library. Used by the
// Import page and the Move to category dialog.
import { html, useState } from "../lib/html.js";
import { valuesAt } from "./library.js";

let listSeq = 0;

/** `schema` (id or null), `values` (one per level); `onChange(schema, values)`. */
export function CategoryPicker({ overview, schema, values = [], onChange, idPrefix = "" }) {
  const schemas = overview?.schemas || [];
  const sc = schemas.find((s) => s.id === schema) || null;
  const levels = sc?.levels || [];
  const setLevel = (i, v) => onChange(schema, levels.map((_, j) => (j === i ? v : values[j] || "")));
  const [listBase] = useState(() => `cat-list-${++listSeq}`);
  return html`<div class="cat-picker">
    <select class="cat-schema" id=${idPrefix ? `${idPrefix}-schema` : null} aria-label="Category" value=${schema || ""}
      onChange=${(e) => onChange(e.target.value || null, [])}>
      <option value="">Unsorted</option>
      ${schemas.map((s) => html`<option value=${s.id} key=${s.id}>${s.name}</option>`)}
    </select>
    ${levels.map((l, i) => {
      const list = `${listBase}-${i}`;
      return html`<span class="cat-level" key=${l.key}>
        <input type="text" class="cat-value" data-level=${l.key} id=${idPrefix ? `${idPrefix}-${l.key}` : null} placeholder=${l.label} aria-label=${l.label}
          value=${values[i] || ""} list=${list} onInput=${(e) => setLevel(i, e.target.value)} />
        <datalist id=${list}>${valuesAt(sc, values, i).map((v) => html`<option value=${v} key=${v} />`)}</datalist>
      </span>`;
    })}
  </div>`;
}
