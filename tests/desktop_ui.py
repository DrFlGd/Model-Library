#!/usr/bin/env python3
"""Drive the built desktop app through tauri-driver (WebDriver) and save screenshots.

    tauri-driver &                     # needs WebKitWebDriver (Linux) or msedgedriver (Windows)
    python3 tests/desktop_ui.py --app desktop/target/release/model-library --out shots

Checks: the interface loads in the real app (the desktop platform, not the plain
web page), the first start asks where the library goes and, taking the
suggestion, makes it at ~/Model Library (with _library/ and Unsorted/), the
settings page shows it, and the theme button reaches night. Exits non-zero on any failure.
"""
import argparse
import json
import sys
import time
from pathlib import Path

from selenium import webdriver
from selenium.webdriver.common.options import ArgOptions

ap = argparse.ArgumentParser()
ap.add_argument("--app", required=True)
ap.add_argument("--out", default="shots")
ap.add_argument("--library", help="expected library folder (default: ~/Model Library)")
ap.add_argument("--driver", default="http://127.0.0.1:4444")
a = ap.parse_args()
out = Path(a.out)
out.mkdir(parents=True, exist_ok=True)
library = Path(a.library) if a.library else Path.home() / "Model Library"

opts = ArgOptions()
opts.set_capability("browserName", "wry")
opts.set_capability("tauri:options", {"application": str(Path(a.app).resolve())})
print("starting a session…", flush=True)
d = webdriver.Remote(command_executor=a.driver, options=opts)
print("session started", flush=True)
d.set_script_timeout(30)
try:
    d.set_window_size(1400, 900)
except Exception as e:  # some drivers can't resize
    print("resize:", e, flush=True)
failures, report = [], {}


def shot(name):
    d.save_screenshot(str(out / f"{name}.png"))


def wait(js, timeout=60, what=""):
    print(f"waiting for {what or js[:60]}…", flush=True)
    end = time.time() + timeout
    while time.time() < end:
        try:
            if d.execute_script(f"return !!({js})"):
                return True
        except Exception:
            pass
        time.sleep(0.25)
    failures.append(f"timed out waiting for {what or js}")
    try:
        shot(f"timeout-{len(failures):02d}")
        print("page says:", d.execute_script("return document.body.innerText.slice(0, 400)"), flush=True)
        print("errors:", d.execute_script("return window.__errors || []"), flush=True)
    except Exception as e:
        print("couldn't inspect the page:", e, flush=True)
    return False


try:
    if not wait("document.querySelector('#first-run')", 90, "the first-start screen"):
        raise SystemExit("the interface never appeared; skipping the rest")
    time.sleep(0.5)
    shot("00-first-run")
    # the first start asks where the library goes: take the suggested folder
    d.execute_script("document.querySelector('#use-default').click()")
    if not wait("document.querySelector('.home h1') && !document.querySelector('#first-run')", 30, "the home page"):
        raise SystemExit("the library wasn't made; skipping the rest")
    time.sleep(0.5)
    shot("00-home")
    report["platform"] = d.execute_script("return window.__modlib?.platform?.kind || ''")
    report["title"] = d.execute_script("return document.querySelector('.home h1').textContent")
    print("platform:", report["platform"], "| library:", report["title"], flush=True)
    if report["platform"] != "desktop":
        failures.append(f"platform: {report['platform']}")

    # the library folder, made where the first start suggested
    made = {p: (library / p).is_dir() for p in ("_library", "_library/schemas", "Unsorted")}
    made["library.json"] = (library / "_library/library.json").is_file()
    report["library"] = {"path": str(library), **made}
    if not all(made.values()):
        failures.append(f"library folder not made: {made}")

    # settings show it
    d.execute_script("location.hash = '#/settings'")
    if wait("document.querySelector('#library-path')", 20, "the settings page"):
        report["settings_path"] = d.execute_script("return document.querySelector('#library-path').textContent")
        if Path(report["settings_path"]).resolve() != library.resolve():
            failures.append(f"settings show {report['settings_path']}")
    shot("01-settings")

    # theme button: light -> dark -> night
    for _ in range(3):
        if d.execute_script("return document.documentElement.dataset.theme") == "night":
            break
        d.execute_script("document.querySelector('#theme-toggle').click()")
    report["theme"] = d.execute_script("return document.documentElement.dataset.theme")
    if report["theme"] != "night":
        failures.append(f"theme button never reached night: {report['theme']}")
    time.sleep(0.5)
    shot("02-night")
    errors = d.execute_script("return (window.__errors || []).slice(0, 20)")
    if errors:
        failures.extend(errors)
finally:
    (out / "desktop-ui.json").write_text(json.dumps({"report": report, "failures": failures}, indent=1))
    d.quit()

for f in failures:
    print("FAILED:", f)
sys.exit(1 if failures else 0)
