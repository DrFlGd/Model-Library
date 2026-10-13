"""Package A acceptance checks; call on an open model with several files."""

async def panel_workspace_checks(pg, check, out=None):
    await pg.wait_for_selector('#file-panel-close')
    original_viewport = pg.viewport_size
    await pg.click('#parts-views [data-view="folders"]')
    panel = await pg.locator('#file-panel').bounding_box()
    stage = await pg.locator('.workspace-viewing-area').bounding_box()
    check('the file panel sits left of the viewing area', panel['x'] + panel['width'] <= stage['x'], (panel, stage))

    await pg.click('#file-panel-close')
    await pg.wait_for_selector('#file-panel-open')
    check('the file panel collapses to its rail', await pg.locator('#file-panel').evaluate('(el) => el.offsetWidth') == 36)
    await pg.wait_for_timeout(600)  # Preferences are persisted asynchronously.
    await pg.reload()
    await pg.wait_for_selector('#file-panel-open')
    check('the file panel keeps its closed state after reload', await pg.locator('#file-panel-close').count() == 0)
    await pg.locator('#file-panel-open').focus()
    await pg.keyboard.press('[')
    await pg.wait_for_selector('#file-panel-close')
    await pg.locator('.file-panel-resize').focus()
    await pg.keyboard.press('ArrowRight')
    width = await pg.locator('#file-panel').evaluate('(el) => el.offsetWidth')
    await pg.wait_for_timeout(600)  # Preferences are persisted asynchronously.
    await pg.reload()
    await pg.wait_for_selector('#file-panel-close')
    check('the file panel keeps its resized width after reload', await pg.locator('#file-panel').evaluate('(el) => el.offsetWidth') == width, width)
    await pg.locator('#file-panel-close').focus()
    await pg.keyboard.press('[')
    await pg.wait_for_selector('#file-panel-open')
    await pg.click('#file-panel-open')

    # A click shows a row without selecting it; its tick (or Ctrl-click) selects, and adds another row.
    root = pg.locator('#part-tree [data-key="d:"]')
    await root.click()
    shown_only = await pg.locator('#part-tree .tree-btn.shown[data-key="d:"]').count() == 1 and await pg.locator('#part-tree .tree-btn[aria-pressed="true"]').count() == 0
    check('a click shows a file row without selecting it, and the bar stays as it was', shown_only and await pg.locator('#mp-head.selecting').count() == 0)
    await pg.locator('#part-tree li:has(> .file-selection-row [data-key="d:"]) > .file-selection-row .file-tick').click()
    files = pg.locator('#part-tree [data-file]')
    if await files.count():
        await files.first.click(modifiers=['Control'])
        check('the file panel shares a multi-selection across rows', await pg.locator('#part-tree .tree-btn[aria-pressed="true"]').count() == 2)
        said = await pg.inner_text('#bar-count')
        acts = await pg.eval_on_selector_all('#mp-head.selecting button[data-action]', 'els => els.map(e => e.dataset.action)')
        check('selected files turn the command bar into their actions', said.endswith(' files selected') and acts[:1] == ['extract']
              and await pg.locator('#mp-head.selecting #bar-more, #bar-send-model-files').count() >= 1, (said, acts, pg.viewport_size))
        await files.first.press('Escape')
        check('Escape clears the file selection', await pg.locator('#part-tree .tree-btn[aria-pressed="true"]').count() == 0 and await pg.locator('#mp-head.selecting').count() == 0)
        await files.first.click(modifiers=['Control'])
        await pg.click('#bar-clear')
        check('✕ clears the file selection', await pg.locator('#part-tree .tree-btn[aria-pressed="true"]').count() == 0 and await pg.locator('#mp-head.selecting').count() == 0)
    search = pg.get_by_role('searchbox', name='Search files')
    await search.fill('no-such-file-workspace-check')
    check('file search filters the panel', await pg.locator('#part-tree [data-file]').count() == 0)
    await search.fill('')

    # Agent B: filtered selection must be explicit; whole-model extraction is allowed.
    await pg.click("#select-visible-files")
    check("Select visible files chooses only filtered files", await pg.locator("#bar-extract").is_enabled())
    await pg.click("#select-whole-model")
    check("Select entire model enables extraction without excluding complete selections", await pg.locator("#bar-extract").is_enabled())
    await pg.click("#bar-clear")
    await pg.click("#mp-more")
    check("Add files is discoverable on a model", await pg.locator('#context-menu [data-action="add-files"]').count() == 1)
    await pg.keyboard.press("Escape")
    await pg.click("#select-whole-model")
    await pg.click("#bar-send-model-files")
    await pg.wait_for_selector("#send-files-dialog")
    check("Send to another model has a searchable destination review", await pg.locator("#send-search").count() == 1)
    await pg.keyboard.press("Escape")
    await pg.wait_for_selector("#send-files-dialog", state="detached")
    await pg.set_viewport_size({'width': 700, 'height': 800})
    await pg.click("#bar-clear")
    await pg.wait_for_selector('#file-panel-open')
    check('the narrow file drawer starts closed, as its rail', await pg.locator('#file-panel').evaluate('(el) => el.offsetWidth') == 36)
    await pg.click('#file-panel-open')
    await pg.wait_for_selector('#file-panel-close')
    check('Files opens the narrow drawer', await pg.locator('#file-panel.drawer').is_visible())
    await pg.click('#file-panel-close')
    await pg.set_viewport_size(original_viewport or {'width': 1280, 'height': 900})
    await pg.wait_for_selector('#file-panel-close')

    # The navigation and both workspace ribbons are independent.
    await pg.wait_for_selector('#rail-details')
    async def settled_navigation(collapsed):
        # Click completion does not wait for Preact or the sidebar's width
        # transition. Observe its final state and attached edge, not a delay.
        await pg.wait_for_function("""collapsed => {
          const nav = document.querySelector('.sidebar');
          const files = document.querySelector('#model-page #file-panel');
          if (!nav || !files || nav.classList.contains('collapsed') !== collapsed) return false;
          const edge = nav.getBoundingClientRect().right;
          const left = files.getBoundingClientRect().left;
          return !nav.getAnimations().some(a => a.playState === 'running' || a.pending)
            && Math.abs(edge - left) <= 1;
        }""", arg=collapsed)

    await settled_navigation(False)
    before = await pg.locator('#file-panel').bounding_box()
    await pg.click('.nav-burger')
    await settled_navigation(True)
    after = await pg.locator('#file-panel').bounding_box()
    check('collapsing navigation moves Files with its attached edge',
          after['x'] < before['x'] and after['x'] >= 45, (before, after))
    await pg.click('.nav-burger')
    await settled_navigation(False)
    expanded = await pg.locator('#file-panel').bounding_box()
    check('expanding navigation restores the attached Files edge',
          abs(expanded['x'] - before['x']) <= 1, (before, expanded))

    await pg.click('#rail-details')
    await pg.wait_for_selector('#workspace-edit-name')
    initial = await pg.input_value('#workspace-edit-name')
    await pg.fill('#workspace-edit-name', initial + ' draft')
    await pg.press('#workspace-edit-name', 'Escape')
    check('Esc in a Details box puts it back, unsaved',
          await pg.input_value('#workspace-edit-name') == initial and
          await pg.get_attribute('#workspace-details-form', 'data-dirty') == 'false' and
          await pg.locator('#model-page').count() == 1)
    panel = await pg.locator('#file-panel').bounding_box()
    stage = await pg.locator('.workspace-viewing-area').bounding_box()
    details = await pg.locator('#workspace-details-panel').bounding_box()
    check('Files, stage and Details are docked in order',
          panel['x'] + panel['width'] <= stage['x'] + 2 and
          stage['x'] + stage['width'] <= details['x'] + 2)

    await pg.set_viewport_size({'width': 1920, 'height': 1080})
    wide = await pg.locator('.workspace-viewing-area').bounding_box()
    await pg.set_viewport_size({'width': 1024, 'height': 768})
    restored = await pg.locator('.workspace-viewing-area').bounding_box()
    check('workspace responds to maximise, restore and panel widths',
          wide['width'] > restored['width'] and
          restored['width'] > 100 and restored['height'] > 100,
          (wide, restored))

    async def fold():
        # when the 3D view would get too narrow the inspector folds by itself
        if await pg.locator('#inspector-fold').is_visible():
            await pg.click('#inspector-fold')

    await pg.wait_for_function("() => !document.querySelector('#inspector.open')")
    check('the inspector folds itself at 1024 so the 3D view keeps its room',
          not await pg.locator('#workspace-details-panel').is_visible())
    await fold()
    await pg.set_viewport_size({'width': 800, 'height': 600})
    await pg.click('#rail-details')
    check('Details remains reachable at 800 by 600',
          await pg.locator('#workspace-details-panel').is_visible())
    await fold()
    if out is not None:
        await pg.set_viewport_size({'width': 1280, 'height': 800})
        if not await pg.locator('#workspace-details-panel').is_visible():
            await pg.click('#rail-details')
        await pg.screenshot(path=str(out / 'agent-a-docked-workspace.png'))
        await fold()
    await pg.set_viewport_size(original_viewport or {'width': 1280, 'height': 900})
    # with room again the inspector comes back as it was left; fold it for what follows
    await pg.wait_for_timeout(300)
    await fold()


async def details_refresh_checks(pg, check, out=None):
    """Agent A-1: clean refresh, true drafts, concurrent edits and Undo."""
    await pg.wait_for_selector('#rail-details')
    if not await pg.locator('#workspace-edit-tags').is_visible():
        await pg.click('#rail-details')
    await pg.wait_for_selector('#workspace-edit-tags')
    tags_before = await pg.input_value('#workspace-edit-tags')
    notes_before = await pg.locator('#workspace-edit-notes').input_value()

    async def remote_update(patch, refresh=True):
        await pg.evaluate("""async ({patch, refresh}) => {
            const {api, loadOverview} = await import('./ui/library.js');
            const id = document.querySelector('#model-page').dataset.model;
            await api('model_update', {id, patch});
            if (refresh) await loadOverview();
        }""", {'patch': patch, 'refresh': refresh})

    # Dialog edit should refresh the untouched ribbon, including when its
    # component remains mounted inside the open panel.
    await pg.click('#mp-edit')
    await pg.wait_for_selector('#details-dialog')
    await pg.fill('#edit-tags', 'Agent A dialog change')
    await pg.click('#details-dialog button[type=submit]')
    await pg.wait_for_selector('#details-dialog', state='detached')
    await pg.wait_for_function("""() =>
      document.querySelector('#workspace-edit-tags')?.value === 'Agent A dialog change' &&
      document.querySelector('#workspace-details-form')?.dataset.dirty === 'false'
    """)
    clean = await pg.evaluate("""async () => {
      const {ui} = await import('./ui/state.js');
      return !ui.get().workspaceDirty;
    }""")
    check('clean Details refreshes after separate Edit details dialog', clean)

    # Undo is a model metadata refresh, not a new unsaved draft.
    await pg.evaluate("""async () => {
      const {undoLast} = await import('./ui/library.js');
      await undoLast();
    }""")
    await pg.wait_for_function("""value =>
      document.querySelector('#workspace-edit-tags')?.value === value &&
      document.querySelector('#workspace-details-form')?.dataset.dirty === 'false'
    """, arg=tags_before)
    check('Undo refreshes clean Details without false draft',
          not await pg.evaluate("""async () =>
            (await import('./ui/state.js')).ui.get().workspaceDirty"""))

    # A watcher/catalog refresh should also replace the clean baseline.
    await remote_update({'tags': 'Agent A external change'})
    await pg.wait_for_function("""() =>
      document.querySelector('#workspace-edit-tags')?.value === 'Agent A external change' &&
      document.querySelector('#workspace-details-form')?.dataset.dirty === 'false'
    """)
    check('external refresh replaces clean Details without overwrite risk',
          not await pg.evaluate("""async () =>
            (await import('./ui/state.js')).ui.get().workspaceDirty"""))

    # A genuine notes draft must survive a different external field edit.
    await pg.fill('#workspace-edit-notes', 'Agent A local notes draft')
    await remote_update({'tags': 'Agent A newer tags'})
    await pg.wait_for_selector('#details-concurrent')
    await pg.click('#workspace-edit-tags')  # leaving the box doesn't save over the newer change
    check('dirty Details retains draft and holds it back after refresh',
          await pg.input_value('#workspace-edit-notes') == 'Agent A local notes draft' and
          await pg.get_attribute('#workspace-details-form', 'data-blocked') == 'true')
    await pg.click('#details-keep-draft')
    check('rebasing retains local notes and newer remote tags',
          await pg.input_value('#workspace-edit-tags') == 'Agent A newer tags' and
          await pg.input_value('#workspace-edit-notes') == 'Agent A local notes draft')
    # Keep my edits saves them.
    await pg.wait_for_function("""() =>
      document.querySelector('#workspace-details-form')?.dataset.dirty === 'false' &&
      document.querySelector('#workspace-details-form')?.getAttribute('aria-busy') === 'false' &&
      !document.querySelector('#details-concurrent')
    """)
    current = await pg.evaluate("""async () => {
      const {api} = await import('./ui/library.js');
      return api('model_get', {id: document.querySelector('#model-page').dataset.model});
    }""")
    check('rebased Save preserves unrelated external metadata',
          current.get('tags') == ['Agent A newer tags'] and
          (current.get('details') or {}).get('notes') == 'Agent A local notes draft')

    # A simultaneous edit to the same field must be named as a conflict.
    await pg.fill('#workspace-edit-tags', 'Agent A conflicting draft')
    await remote_update({'tags': 'Agent A concurrent author'})
    await pg.wait_for_selector('#details-concurrent')
    warning = await pg.locator('#details-concurrent').inner_text()
    check('conflicting field is named before user resolves it',
          'tags' in warning and await pg.get_attribute('#workspace-details-form', 'data-blocked') == 'true')
    await pg.click('#details-load-latest')
    check('Use latest discards only deliberate draft state',
          await pg.input_value('#workspace-edit-tags') == 'Agent A concurrent author' and
          await pg.get_attribute('#workspace-details-form', 'data-dirty') == 'false')

    # Race after the form's render but before Save is caught by preflight.
    # (Leaving the tags box for Use latest queued a save; let it find nothing to save first.)
    await pg.evaluate("() => new Promise((r) => setTimeout(r, 50))")
    await pg.fill('#workspace-edit-notes', 'Unsaved preflight test')
    await remote_update({'tags': 'Agent A preflight update'}, refresh=False)
    await pg.click('#workspace-edit-tags')  # leaving the box saves, after checking for newer changes
    await pg.wait_for_selector('#details-concurrent')
    check('preflight rejects changed backend data rather than overwriting it',
          await pg.input_value('#workspace-edit-notes') == 'Unsaved preflight test' and
          await pg.get_attribute('#workspace-details-form', 'data-blocked') == 'true')
    await pg.click('#details-load-latest')

    # Undo from a separate editor while this ribbon has *real* local changes.
    await pg.click('#mp-edit')
    await pg.wait_for_selector('#details-dialog')
    await pg.fill('#edit-tags', 'Agent A Undo while dirty')
    await pg.click('#details-dialog button[type=submit]')
    await pg.wait_for_selector('#details-dialog', state='detached')
    await pg.wait_for_function("""() =>
      document.querySelector('#workspace-edit-tags')?.value === 'Agent A Undo while dirty'
    """)
    await pg.fill('#workspace-edit-notes', 'Agent A draft retained through Undo')
    await pg.evaluate("""async () => {
      const {undoLast} = await import('./ui/library.js');
      await undoLast();
    }""")
    await pg.wait_for_selector('#details-concurrent')
    check('Undo preserves genuine draft, identifies conflict and blocks Save',
          await pg.input_value('#workspace-edit-notes') == 'Agent A draft retained through Undo' and
          await pg.get_attribute('#workspace-details-form', 'data-blocked') == 'true')
    await pg.click('#details-load-latest')

    # Return the test model to its original metadata for the rest of acceptance.
    await remote_update({'tags': tags_before, 'notes': notes_before})
    await pg.wait_for_function("""value =>
      document.querySelector('#workspace-edit-tags')?.value === value &&
      document.querySelector('#workspace-details-form')?.dataset.dirty === 'false'
    """, arg=tags_before)
    if out is not None:
        await pg.screenshot(path=str(out / 'agent-a-details-refresh.png'))
    await pg.click('#inspector-fold')
