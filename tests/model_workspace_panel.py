"""Package A acceptance checks; call on an open model with several files."""

async def panel_workspace_checks(pg, check):
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
