"""Home acceptance checks for loose files in an existing category."""
import json
import zipfile


async def loose_workspace_checks(pg, check, library, base_url, api, cube, png, out):
    parent = library / 'Household/Kitchen'
    parent.mkdir(parents=True, exist_ok=True)
    originals = {
        'WorkspaceHook.stl': cube(9).encode(),
        'WorkspaceHook.png': png,
        'WorkspaceHook notes.txt': b'Keep the matching instructions with the model.',
        'WorkspaceHookLong.png': png,
        'WorkspaceUnmatched.txt': b'This document belongs to neither model.',
    }
    for name, body in originals.items():
        (parent / name).write_bytes(body)
    with zipfile.ZipFile(parent / 'WorkspaceHookLong.zip', 'w') as archive:
        archive.writestr('clip.stl', cube(7))
    originals['WorkspaceHookLong.zip'] = (parent / 'WorkspaceHookLong.zip').read_bytes()
    await api(pg, 'library_scan', {'full': True})
    await pg.goto(base_url + '#/')
    await pg.reload()
    one = '#home-loose-files .wrap-loose[data-file="Household/Kitchen/WorkspaceHook.stl"]'
    await pg.wait_for_selector(one)
    paths = await pg.locator('#home-loose-files .loose-file-path').all_text_contents()
    check('Home finds loose model files inside a category',
          'Household/Kitchen/WorkspaceHook.stl' in paths
          and 'Household/Kitchen/WorkspaceHookLong.zip' in paths
          and not any('WorkspaceUnmatched' in path for path in paths), paths)

    await pg.click(one)
    await pg.wait_for_selector(one, state='detached')
    wrapped = parent / 'WorkspaceHook'
    side = json.loads((wrapped / 'model.json').read_text())
    check('Put in a folder keeps the category and matching companions',
          (wrapped / 'WorkspaceHook.stl').read_bytes() == originals['WorkspaceHook.stl']
          and (wrapped / 'WorkspaceHook.png').read_bytes() == png
          and (wrapped / 'WorkspaceHook notes.txt').is_file()
          and (parent / 'WorkspaceHookLong.png').is_file()
          and (parent / 'WorkspaceUnmatched.txt').is_file()
          and side.get('schema') == 'household' and side.get('path') == ['Kitchen'], side)
    await pg.goto(base_url + '#/browse/schema/household/Kitchen')
    await pg.locator('.card:has(.card-name:text-is("WorkspaceHook"))').wait_for()
    check('the wrapped model appears in its original category',
          await pg.locator('.card:has(.card-name:text-is("WorkspaceHook"))').count() == 1)
    await pg.goto(base_url + '#/')
    undo = pg.locator('#recent-changes li').filter(has_text='Put WorkspaceHook in a folder').locator('button.undo-change')
    await undo.wait_for()
    await undo.click()
    await pg.wait_for_selector(one)
    check('Recent changes undo restores every original loose file exactly',
          not wrapped.exists() and all((parent / name).read_bytes() == body for name, body in originals.items()))

    # Put all is exercised through Home too; wrapping uses one undo entry per model.
    await pg.click('#wrap-all-loose')
    await pg.wait_for_selector(one, state='detached')
    await pg.wait_for_selector('#home-loose-files .wrap-loose[data-file="Household/Kitchen/WorkspaceHookLong.zip"]', state='detached')
    check('Put all in folders wraps each model without taking unmatched documents',
          (wrapped / 'model.json').is_file()
          and (parent / 'WorkspaceHookLong/model.json').is_file()
          and (parent / 'WorkspaceHookLong/WorkspaceHookLong.zip').read_bytes() == originals['WorkspaceHookLong.zip']
          and (parent / 'WorkspaceHookLong/WorkspaceHookLong.png').is_file()
          and (parent / 'WorkspaceUnmatched.txt').read_bytes() == originals['WorkspaceUnmatched.txt'])
    await pg.screenshot(path=str(out / '41-loose-files-wrapped.png'), full_page=True)
