// The Keyboard shortcuts sheet: ? anywhere (not while typing in a box), or the link on
// About. Every key here is one the pages really handle; keep it in step with them
// (actions.js onWindowKey, each page's usePageKeys, the panels' [ and ]).
import { html } from "../lib/html.js";
import { ui } from "./state.js";
import { Dialog, close } from "./dialogs.js";

const GROUPS = [
  ["Everywhere", [
    [["?"], "Show these shortcuts"],
    [["/"], "Go to the search box"],
    [["Ctrl", "Z"], "Undo the last change"],
    [["Shift", "F10"], "Open the right-click menu for what's selected (or the menu key)"],
    [["Esc"], "Close a menu or dialog, or clear the selection"],
    [["]"], "Fold or unfold the panel on the right (Details, Media, Files)"],
  ]],
  ["Models (All models, Unsorted, Starred, categories, collections)", [
    [["←", "→", "↑", "↓"], "Move between models; hold Shift to select as you go"],
    [["Ctrl", "A"], "Select all"],
    [["Enter"], "Open"],
    [["E"], "Edit details…"],
    [["F2"], "Edit details… with the name ready to change"],
    [["M"], "Move to category…"],
    [["C"], "Add to collection…"],
    [["S"], "Star, or take the star off"],
    [["["], "Fold or unfold a category's subcategories"],
  ]],
  ["A model's page", [
    [["E"], "Edit details (F2 with the name ready to change)"],
    [["M", "C", "S"], "Move, add to a collection, star"],
    [["←", "→"], "The previous or next file"],
    [["Backspace"], "Up a folder"],
    [["Ctrl", "A"], "Select all the files in the list"],
    [["Enter"], "Show the selected file"],
    [["N"], "Make a model… from the selected files"],
    [["["], "Fold or unfold the file list"],
  ]],
  ["Import", [
    [["↑", "↓"], "Move through folders and models"],
    [["←", "→"], "Fold or unfold a folder (Enter too)"],
    [["Ctrl", "A"], "Select all"],
    [["E"], "Edit the model's details (F2 with the name ready to change)"],
    [["M"], "Set category…"],
  ]],
  ["Duplicates and Health check", [
    [["Enter", "E", "M", "C", "S"], "Work on the model picked, as in a list of models"],
  ]],
  ["Typing in a box", [
    [["Enter"], "Save (Details) or press the dialog's button"],
    [["Esc"], "Put the box back as it was"],
    [["↓", "↑"], "Pick a suggested author, tag or licence, then Enter"],
  ]],
];

const Keys = ({ keys }) => html`<span class="keys">${keys.map((k, i) => html`${i ? html`<span class="key-join">${keys[0] === "Ctrl" || keys[0] === "Shift" ? "+" : " "}</span>` : null}<kbd>${k}</kbd>`)}</span>`;

export function ShortcutsDialog() {
  return html`<${Dialog} title="Keyboard shortcuts" id="shortcuts-dialog" onSubmit=${close} submitLabel="Close" cancelLabel=${null}>
    <p class="muted">On a Mac, use Cmd where it says Ctrl. Menus show each action's key beside it too.</p>
    <div class="shortcut-groups">${GROUPS.map(([title, rows]) => html`<section class="shortcut-group" key=${title}><h3>${title}</h3>
      <dl>${rows.map(([keys, what]) => html`<div key=${keys.join("") + what}><dt><${Keys} keys=${keys} /></dt><dd>${what}</dd></div>`)}</dl></section>`)}</div>
  <//>`;
}

export const showShortcuts = () => ui.set({ dialog: { type: "shortcuts" }, menu: null });
