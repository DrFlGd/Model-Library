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

    # The model root is selectable, and Ctrl-click adds another row.
    root = pg.locator('#part-tree [data-key="d:"]')
    await root.click()
    files = pg.locator('#part-tree [data-file]')
    if await files.count():
        await files.first.click(modifiers=['Control'])
        check('the file panel shares a multi-selection across rows', await pg.locator('#part-tree .tree-btn[aria-pressed="true"]').count() == 2)
        await files.first.press('Escape')
        check('Escape clears the file selection', await pg.locator('#part-tree .tree-btn[aria-pressed="true"]').count() == 0)
    search = pg.get_by_role('searchbox', name='Search files')
    await search.fill('no-such-file-workspace-check')
    check('file search filters the panel', await pg.locator('#part-tree [data-file]').count() == 0)
    await search.fill('')

    await pg.set_viewport_size({'width': 700, 'height': 800})
    await pg.wait_for_selector('#files-drawer-button')
    check('the narrow file drawer starts closed', not await pg.locator('#file-panel').is_visible())
    await pg.click('#files-drawer-button')
    await pg.wait_for_selector('#file-panel-close')
    check('Files opens the narrow drawer', await pg.locator('#file-panel.drawer').is_visible())
    await pg.click('#file-panel-close')
    await pg.set_viewport_size(original_viewport or {'width': 1280, 'height': 900})
    await pg.wait_for_selector('#file-panel-close')

    # The navigation and both workspace ribbons are independent.
    await pg.wait_for_selector('#details-panel-open')
    before = await pg.locator('#file-panel').bounding_box()
    await pg.click('.nav-burger')
    after = await pg.locator('#file-panel').bounding_box()
    check('collapsing navigation moves Files with its attached edge',
          after['x'] < before['x'] and after['x'] >= 45, (before, after))
    await pg.click('.nav-burger')

    await pg.click('#details-panel-open')
    await pg.wait_for_selector('#workspace-edit-name')
    initial = await pg.input_value('#workspace-edit-name')
    await pg.fill('#workspace-edit-name', initial + ' draft')
    await pg.click('#details-panel-close')
    await pg.click('#details-panel-open')
    check('draft stays intact after collapsing Details',
          await pg.input_value('#workspace-edit-name') == initial + ' draft')
    await pg.click('.workspace-details-actions button[type=button]')
    check('Cancel leaves the model name unchanged',
          await pg.input_value('#workspace-edit-name') == initial)
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
    await pg.click('#details-panel-close')
    await pg.set_viewport_size({'width': 800, 'height': 600})
    await pg.click('#details-drawer-button')
    check('Details remains reachable at 800 by 600',
          await pg.locator('#workspace-details-panel.drawer').is_visible())
    await pg.click('#details-panel-close')
    if out is not None:
        await pg.set_viewport_size({'width': 1280, 'height': 800})
        await pg.click('#details-panel-open')
        await pg.screenshot(path=str(out / 'agent-a-docked-workspace.png'))
        await pg.click('#details-panel-close')
    await pg.set_viewport_size(original_viewport or {'width': 1280, 'height': 900})


async def details_refresh_checks(pg, check, out=None):
    """Agent A-1: clean refresh, true drafts, concurrent edits and Undo."""
    await pg.wait_for_selector('#details-panel-open')
    await pg.click('#details-panel-open')
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
      document.querySelector('#workspace-details-form button[type=submit]')?.disabled
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
      document.querySelector('#workspace-details-form button[type=submit]')?.disabled
    """, arg=tags_before)
    check('Undo refreshes clean Details without false draft',
          not await pg.evaluate("""async () =>
            (await import('./ui/state.js')).ui.get().workspaceDirty"""))

    # A watcher/catalog refresh should also replace the clean baseline.
    await remote_update({'tags': 'Agent A external change'})
    await pg.wait_for_function("""() =>
      document.querySelector('#workspace-edit-tags')?.value === 'Agent A external change' &&
      document.querySelector('#workspace-details-form button[type=submit]')?.disabled
    """)
    check('external refresh replaces clean Details without overwrite risk',
          not await pg.evaluate("""async () =>
            (await import('./ui/state.js')).ui.get().workspaceDirty"""))

    # A genuine notes draft must survive a different external field edit.
    await pg.fill('#workspace-edit-notes', 'Agent A local notes draft')
    await remote_update({'tags': 'Agent A newer tags'})
    await pg.wait_for_selector('#details-concurrent')
    check('dirty Details retains draft and blocks Save after refresh',
          await pg.input_value('#workspace-edit-notes') == 'Agent A local notes draft' and
          await pg.locator('#workspace-details-form button[type=submit]').is_disabled())
    await pg.click('#details-keep-draft')
    check('rebasing retains local notes and newer remote tags',
          await pg.input_value('#workspace-edit-tags') == 'Agent A newer tags' and
          await pg.input_value('#workspace-edit-notes') == 'Agent A local notes draft')
    await pg.click('#workspace-details-form button[type=submit]')
    await pg.wait_for_function("""() =>
      document.querySelector('#workspace-details-form button[type=submit]')?.disabled &&
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
          'tags' in warning and await pg.locator('#workspace-details-form button[type=submit]').is_disabled())
    await pg.click('#details-load-latest')
    check('Use latest discards only deliberate draft state',
          await pg.input_value('#workspace-edit-tags') == 'Agent A concurrent author' and
          await pg.locator('#workspace-details-form button[type=submit]').is_disabled())

    # Race after the form's render but before Save is caught by preflight.
    await pg.fill('#workspace-edit-notes', 'Unsaved preflight test')
    await remote_update({'tags': 'Agent A preflight update'}, refresh=False)
    await pg.click('#workspace-details-form button[type=submit]')
    await pg.wait_for_selector('#details-concurrent')
    check('preflight rejects changed backend data rather than overwriting it',
          await pg.input_value('#workspace-edit-notes') == 'Unsaved preflight test' and
          await pg.locator('#workspace-details-form button[type=submit]').is_disabled())
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
          await pg.locator('#workspace-details-form button[type=submit]').is_disabled())
    await pg.click('#details-load-latest')

    # Return the test model to its original metadata for the rest of acceptance.
    await remote_update({'tags': tags_before, 'notes': notes_before})
    await pg.wait_for_function("""value =>
      document.querySelector('#workspace-edit-tags')?.value === value &&
      document.querySelector('#workspace-details-form button[type=submit]')?.disabled
    """, arg=tags_before)
    if out is not None:
        await pg.screenshot(path=str(out / 'agent-a-details-refresh.png'))
    await pg.click('#details-panel-close')
