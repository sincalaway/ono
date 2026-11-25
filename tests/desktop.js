(async () => {
  if (window.__testingDesktop || !window.__TAURI__ || document.readyState !== 'complete') return;
  window.__testingDesktop = true;
  const result = { ok: false, steps: [] };
  const ipc = window.__TAURI__.core.invoke;
  const assert = (ok, reason) => { if (!ok) throw Error(reason); };
  try {
    for (let i = 0; i < 100 && !S.overview; i++) await new Promise(r => setTimeout(r, 100));
    assert(S.overview?.hardware.memory.total_bytes > 0, 'missing hardware data');
    assert(document.querySelector('#main h1')?.textContent === S.overview.conclusion.headline, 'overview failed to render');
    result.steps.push('overview rendered');
    const settings = await ipc('get_settings');
    settings.port = 18686;
    await ipc('save_settings', {settings});
    assert((await ipc('get_settings')).port === 18686, 'settings round trip');
    const catalog = await ipc('get_catalog');
    assert(catalog.length > 0, 'empty catalog');
    go('models');
    await loadCatalog();
    assert(document.querySelector('#main').textContent.includes('导入本地 GGUF'), 'model page');
    S.settings = await ipc('get_settings');
    go('settings');
    assert(document.getElementById('st-port')?.value === '18686', 'settings page');
    go('agent');
    go('image');
    await loadImageData(true);
    assert(S.img.catalog.length === 12, 'image catalog');
    go('video');
    go('overview');
    result.steps.push('settings, catalog and navigation');
    const path = __MODEL_FIXTURE__;
    if (path) {
      const imported = await ipc('import_path', {path});
      assert((await ipc('list_local')).some(m => m.path === imported.path), 'import missing');
      S.run.path = imported.path; S.run.ctx = 256;
      await actions.startModel();
      const status = await ipc('server_status');
      assert(status.ready, 'model not ready');
      const response = await llmFetch({messages:[{role:'user',content:'Once upon a time'}],max_tokens:8,temperature:0},new AbortController().signal);
      const answer = await response.json();
      assert(response.ok && answer.choices?.length && answer.usage?.completion_tokens > 0, JSON.stringify(answer));
      await ipc('stop_model');
      assert(!(await ipc('server_status')).running, 'engine did not stop');
      result.steps.push('GGUF import, real chat completion, stop');
    }
    result.ok = true;
  } catch (e) { result.error = String(e); result.stack = e?.stack; }
  await window.__TAURI__.event.emit('desktop-test-result', result);
})();
