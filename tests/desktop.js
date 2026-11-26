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
    await ipc('agent_tool', {ws,name:'write_file',args:{path:'index.html',content:'<h1>ono</h1>'}});
    const read = await ipc('agent_tool', {ws,name:'read_file',args:{path:'src/a.txt'}});
    assert(read.includes('alpha'), 'workspace read failed');
    const packed = await ipc('agent_tool', {ws,name:'package_files',args:{path:'.',output_path:'delivery.zip'}});
    assert(packed.includes('已打包'), 'workspace package failed');
    const archive = await ipc('agent_resolve', {ws,path:'delivery.zip'});
    assert(archive.inside && archive.exists, 'workspace archive missing');
    const tree = await ipc('agent_tool', {ws,name:'list_files',args:{path:'.',depth:2}});
    assert(tree.startsWith('delivery.zip\nexisting.txt\nindex.html\nsrc/'), 'workspace tree dropped its first sorted entries');
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
    const originalAgentState = { tasks: S.ag.tasks, id: S.ag.id, ws: S.ag.ws, page: S.page };
    const originalAskUser = window.askUser;
    let questionCount = 0;
    window.askUser = async () => { questionCount++; return '简洁风格'; };
    const regressionTask = {id:'desktop-agent-regression',title:'Agent 回归',ws,allow:{},messages:[],todos:[],changed:[],status:'',created:Date.now()};
    S.ag.tasks = [regressionTask]; S.ag.id = regressionTask.id; S.ag.ws = ws; S.page = 'agent';
    turn = 0;
    window.streamOnce = async () => {
      turn++;
      const call = (id, name, args) => ({id,name,args:JSON.stringify(args)});
      if (turn === 1) return {text:'',calls:[
        call('plan','update_plan',{items:[{text:'检查计划',status:'in_progress'},{text:'检查问答',status:'in_progress'}]}),
        call('ask','ask_user',{question:'要采用哪种样式？'})
      ]};
      if (turn === 2) {
        assert(regressionTask.todos.filter(x => x.status === 'in_progress').length === 1, 'plan has multiple active steps');
        return {text:'',calls:[
          call('ask-again','ask_user',{question:'要采用哪种样式'}),
          call('bad-edit','edit_file',{path:'src/a.txt',old_text:'missing',new_text:'x'}),
          call('bad-edit-again','edit_file',{path:'src/a.txt',old_text:'missing',new_text:'x'}),
          call('read','read_file',{path:'src/a.txt'}),
          call('good-edit','edit_file',{path:'src/a.txt',old_text:'alpha',new_text:'ALPHA'}),
          call('done','update_plan',{items:[{text:'检查计划',status:'done'},{text:'检查问答',status:'done'}]})
        ]};
      }
      return {text:'完成',calls:[]};
    };
    try { await runAgent(regressionTask); } finally { window.streamOnce = originalStreamOnce; window.askUser = originalAskUser; }
    assert(questionCount === 1, 'answered question was asked again');
    const editSteps = regressionTask.messages[0].steps.filter(x => x.name === 'edit_file');
    assert(editSteps.filter(x => x.result.includes('未找到原文')).length === 1, 'failed edit executed more than once');
    assert(editSteps.some(x => x.result.includes('相同工具调用已经失败过')), 'repeated failed edit was not intercepted');
    assert(regressionTask.todos.every(x => x.status === 'done'), 'completed plan has unfinished items');
    assert(regressionTask.status === 'done', 'recovered agent task did not complete');
    assert(taskHistory(regressionTask, regressionTask.messages.length).some(m => m.content.includes('简洁风格')), 'question answer missing from history');
    assert(!document.querySelector('.ag-plan .qp-spin'), 'completed plan is still spinning');
    result.steps.push('question deduplication, failed edit guard, plan completion');
    const unfinishedTask = {id:'desktop-unfinished-plan',title:'未完成计划',ws,allow:{},messages:[],todos:[],changed:[],status:'',created:Date.now()};
    S.ag.tasks = [unfinishedTask]; S.ag.id = unfinishedTask.id;
    window.streamOnce = async () => ({text:'',calls:[{id:'unfinished-plan',name:'update_plan',args:JSON.stringify({items:[{text:'验证文件',status:'in_progress'},{text:'后续步骤',status:'pending'}]})}]});
    try { await runAgent(unfinishedTask); } finally { window.streamOnce = originalStreamOnce; }
    assert(unfinishedTask.status === 'error', 'unfinished plan was marked complete');
    assert(unfinishedTask.todos.every(x => x.status !== 'in_progress'), 'unfinished plan retained in_progress');
    assert(!document.querySelector('.ag-plan .qp-spin'), 'unfinished plan is still spinning');
    result.steps.push('unfinished plans stop spinning and cannot complete');
    S.ag.tasks = originalAgentState.tasks; S.ag.id = originalAgentState.id; S.ag.ws = originalAgentState.ws; S.page = originalAgentState.page;
    saveTasks(); invalidateTree(); render();
    const originalRun = { ...S.run };
    const originalServer = S.server;
    const originalCrash = S.crash;
    const originalVision = S.vision;
    const originalLogs = S.logs;
    S.run.path = `${ws}\\missing-model.gguf`;
    S.run.ctx = null;
    S.run.starting = false;
    S.run.error = '';
    go('run');
    const started = await actions.startModel({ stay: true });
    assert(started === false, 'missing model was reported as started');
    assert(!S.run.starting, 'failed model start retained starting state');
    assert(S.run.error.length > 0, 'failed model start discarded its error');
    assert(document.querySelector('#main .alert')?.textContent.includes(S.run.error.split('\n')[0]), 'failed model error was not rendered');
    assert(document.querySelector('details[data-open="logsOpen"]')?.open, 'failed model logs were not expanded');
    Object.assign(S.run, originalRun);
    S.server = originalServer; S.crash = originalCrash; S.vision = originalVision; S.logs = originalLogs;
    go(originalAgentState.page); render();
    result.steps.push('failed model start resets state and renders error');
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
