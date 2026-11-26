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
    const normalized = normalizeServerStatus({running:true,ready:true,model_path:'C:\\models\\Qwen.gguf',model_name:{OsString:'Windows'}});
    assert(normalized.model_name === 'Qwen', 'model label normalization');
    const ws = __WORKSPACE_FIXTURE__;
    assert(ws, 'missing workspace fixture');
    const written = await ipc('agent_tool', {ws,name:'write_file',args:{path:'src/a.txt',content:'alpha'}});
    assert(written.includes('已写入'), 'workspace write failed');
    const read = await ipc('agent_tool', {ws,name:'read_file',args:{path:'src/a.txt'}});
    assert(read.includes('alpha'), 'workspace read failed');
    const packed = await ipc('agent_tool', {ws,name:'package_files',args:{path:'.',output_path:'delivery.zip'}});
    assert(packed.includes('已打包'), 'workspace package failed');
    const archive = await ipc('agent_resolve', {ws,path:'delivery.zip'});
    assert(archive.inside && archive.exists, 'workspace archive missing');
    const tree = await ipc('agent_tool', {ws,name:'list_files',args:{path:'.',depth:2}});
    assert(tree.includes('src/a.txt') && tree.includes('delivery.zip'), 'workspace tree stale');
    result.steps.push('model label, workspace write/read/package');
    const originalStreamOnce = window.streamOnce;
    let turn = 0;
    window.streamOnce = async (_convo, _tools, ai) => {
      if (++turn === 1) return {text:'',calls:[{id:'desktop-failure',name:'write_file',args:JSON.stringify({path:'src',content:'not a file'})}]};
      ai.content += '所有文件已完成。';
      return {text:ai.content,calls:[]};
    };
    const task = {id:'desktop-failure-test',title:'失败状态测试',ws,messages:[],todos:[],changed:[],status:'',created:Date.now()};
    try { await runAgent(task); } finally { window.streamOnce = originalStreamOnce; }
    assert(task.status === 'error', 'failed write was marked complete');
    assert(task.messages[0].content.includes('任务未完成'), 'missing failure correction');
    result.steps.push('failed mutation cannot complete a task');
    const path = __MODEL_FIXTURE__;
    if (path) {
      const imported = await ipc('import_path', {path});
      assert((await ipc('list_local')).some(m => m.path === imported.path), 'import missing');
      S.run.path = imported.path; S.run.ctx = 256;
      await actions.startModel();
      const status = await ipc('server_status');
      assert(status.ready, 'model not ready');
      assert(typeof status.model_name === 'string' && status.model_name.length > 0, 'model name is not text');
      go('agent');
      assert(document.querySelector('.src-btn')?.textContent.includes(status.model_name), 'agent model label missing');
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
