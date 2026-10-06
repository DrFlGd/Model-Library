# Third-party code

| What | Used for | License |
| --- | --- | --- |
| [Preact](https://github.com/preactjs/preact) 10.27.2 @ `0dbe636` | Interface components (`web/vendor/preact`; copied by `tools/vendor_preact.py`, only import paths changed) | MIT |
| [htm](https://github.com/developit/htm) @ `d62dcfd` | JSX-like templates without a build step (`web/vendor/htm`) | Apache-2.0 |
| [three.js](https://github.com/mrdoob/three.js) r186 | 3D viewer (`web/vendor/three`: three.module.js, three.core.js, OrbitControls, STLLoader; unmodified, as in Grid Workshop) | MIT |
| [zip](https://crates.io/crates/zip), [flate2](https://crates.io/crates/flate2), [crc32fast](https://crates.io/crates/crc32fast), [pulldown-cmark](https://crates.io/crates/pulldown-cmark), [base64](https://crates.io/crates/base64) | Reading inside ZIPs and 3MF, writing thumbnail PNGs, showing Markdown readmes, cover snapshots | MIT or Apache-2.0 |
| [Archivo](https://github.com/google/fonts/tree/main/ofl/archivo) | Interface font (bundled into the app by `tools/build_desktop.py --fetch-fonts`) | SIL OFL 1.1 |
| [Tauri](https://tauri.app) 2 and its dialog and opener plugins | The desktop app's window | MIT or Apache-2.0 |

The app's shell, interface pieces and tests started as copies from [Claude Grid Workshop](https://github.com/DrFlGd/Claude-Grid-Workshop) (same owner).
