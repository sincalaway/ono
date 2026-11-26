// Browser-only preview runtime. It never reads or writes the local file system.
(function () {
  'use strict';
  if (window.__TAURI__) return;

  const GIB = 1024 ** 3;
  const PREVIEW_WS = 'D:\\ono-preview-workspace';
  const clone = (value) => JSON.parse(JSON.stringify(value));
  const workspaces = new Map();
  const commandLog = [];
  const browserErrors = [];
  let autostart = false;
  let lastSmokeTest = null;
  let smokeRunning = false;
  let modelStartError = '';

  window.addEventListener('error', (event) => {
    browserErrors.push(event.message || String(event.error || '未知脚本错误'));
  });
  window.addEventListener('unhandledrejection', (event) => {
    browserErrors.push(String(event.reason?.message || event.reason || '未处理的 Promise 错误'));
  });

  const sampleModel = {
    name: '示例对话模型 · 3B Q4_K_M',
    path: '/ono-preview-models/Example-3B-Q4_K_M.gguf',
    imported: false,
    info: {
      arch: 'qwen2', block_count: 36, expert_count: 0, expert_used_count: 0,
      files: ['Example-3B-Q4_K_M.gguf'], file_type: 'GGUF', total_bytes: 2.1 * GIB,
      is_diffusion: false
    },
    plan: {
      tier: 'fast', tier_label: '流畅', gpu_bytes: 2.5 * GIB, ram_bytes: 0,
      disk_bytes: 0, kv_bytes: 0.35 * GIB, ctx: 32768, est_tps: 42,
      summary: '示例规划：模型与缓存可放入显存。',
      gpu_note: '全部放在显卡', ram_note: '不需要额外内存', disk_note: '不需要硬盘读取'
    }
  };
  const samplePlan = () => ({
    model: clone(sampleModel), plan: clone(sampleModel.plan), custom_args: '',
    command: 'llama-server -m Example-3B-Q4_K_M.gguf -ngl 99 -c 32768 --host 127.0.0.1 --port 8686'
  });
  const recommendation = (id, name, bytes, installed) => ({
    installed_path: installed ? sampleModel.path : null,
    bytes,
    quant: 'Q4_K_M',
    model: {
      id, name, org: 'ono 预览', desc: '仅用于展示界面与交互的示例数据。',
      repo: `ono-preview/${id}`, vision: false, params_b: 3, tags: ['对话', '编程'],
      quants: ['Q4_K_M', 'Q8_0'], size_gb: { Q4_K_M: bytes / GIB, Q8_0: bytes * 1.7 / GIB },
      release: '2026-01'
    },
    rec: {
      model: {
        id, name, org: 'ono 预览', desc: '仅用于展示界面与交互的示例数据。',
        repo: `ono-preview/${id}`, vision: false, params_b: 3, tags: ['对话', '编程'],
        quants: ['Q4_K_M', 'Q8_0'], size_gb: { Q4_K_M: bytes / GIB, Q8_0: bytes * 1.7 / GIB },
        release: '2026-01'
      },
      quant: 'Q4_K_M', bytes, plan: clone(sampleModel.plan)
    }
  });
  const recommendations = () => [
    recommendation('example-3b', '示例对话模型 · 3B', 2.1 * GIB, true),
    recommendation('example-7b', '示例编程模型 · 7B', 4.6 * GIB, false),
    recommendation('example-14b', '示例推理模型 · 14B', 9.2 * GIB, false)
  ];
  const imageCatalog = () => [
    {
      id: 'preview-flux', kind: 'image', name: '示例画图模型', org: 'ono 预览',
      desc: '用于预览画图页面的模拟模型。', released: '2026-01', rec_quant: 'Q4_K_M',
      steps: 20, cfg: 3.5, edit: false, edit_only: false, refs: 0,
      quants: [{ quant: 'Q4_K_M', bytes: 6.8 * GIB, need_bytes: 6.8 * GIB, installed: true, has_main: true, tier: 'fast', tier_label: '流畅' }]
    },
    {
      id: 'preview-wan', kind: 'video', name: '示例视频模型', org: 'ono 预览',
      desc: '用于预览视频页面的模拟模型。', released: '2026-01', rec_quant: 'Q4_K_M',
      steps: 20, fps: 16, hd: true, i2v: true, refs: 0,
      quants: [{ quant: 'Q4_K_M', bytes: 8.4 * GIB, need_bytes: 8.4 * GIB, installed: true, has_main: true, tier: 'ok', tier_label: '能跑' }]
    }
  ];
  const hardware = () => ({
    cpu: { name: '示例处理器', logical_cores: 16, physical_cores: 8 },
    memory: { available_bytes: 24 * GIB, total_bytes: 32 * GIB, kind: 'DDR5' },
    gpus: [{
      name: '示例显卡 · 12 GB', vendor: 'nvidia', integrated: false, shared: false,
      backend: 'cuda', vram_bytes: 12 * GIB, vram_used_bytes: 2 * GIB, util: 8,
      temp: 46, power: 35, power_limit: 250
    }],
    primary_gpu: 0, backend: 'cuda',
    disks: [{ mount: '/', available_bytes: 400 * GIB, total_bytes: 512 * GIB, is_ssd: true }]
  });
  const settings = {
    models_dir: '/ono-preview-models', images_dir: '', port: 8686,
    gpu_override: null, engine_cpu: false, perf_fa: 'on', perf_kv: 'q8_0',
    auto_restart: true, lan_access: false, gpu_devices: [], hf_token: '',
    custom_mirror: '', github_mirror: ''
  };
  const sampleSkill = {
    name: 'preview-guide', description: '浏览器预览模式的示例 Skill。', enabled: true,
    content: '---\nname: preview-guide\ndescription: 浏览器预览模式的示例 Skill。\n---\n\n# 预览指南\n\n这里只使用虚拟文件，不会访问真实磁盘。\n'
  };
  let skills = [clone(sampleSkill)];
  const memories = [];

  function seedWorkspace(ws) {
    if (workspaces.has(ws)) return workspaces.get(ws);
    const files = new Map([
      ['index.html', { type: 'file', content: '<!doctype html>\n<h1>ono 浏览器预览</h1>\n' }],
      ['README.md', { type: 'file', content: '# ono 预览工作区\n\n这里的文件只存在于当前页面内存中。\n' }],
      ['src', { type: 'dir', content: '' }],
      ['src/app.js', { type: 'file', content: 'console.log("preview");\n' }],
      ['src/style.css', { type: 'file', content: 'body { color: #333; }\n' }]
    ]);
    workspaces.set(ws, files);
    return files;
  }
  function relativePath(raw) {
    const path = String(raw || '.').replace(/\\/g, '/').replace(/^\.\//, '');
    if (!path || path === '.') return '';
    if (path.startsWith('/') || /^[A-Za-z]:/.test(path)) throw new Error('预览虚拟工作区只接受相对路径');
    const parts = path.split('/');
    if (parts.some((part) => !part || part === '..')) throw new Error('预览虚拟工作区不接受空路径或 ..');
    return parts.join('/');
  }
  function node(files, path) {
    return files.get(path);
  }
  function isDirectory(files, path) {
    return path === '' || files.get(path)?.type === 'dir' || [...files.keys()].some((key) => key.startsWith(`${path}/`));
  }
  function listFiles(files, base, depth) {
    const childrenOf = (dir) => {
      const prefix = dir ? `${dir}/` : '';
      const children = new Map();
      for (const key of files.keys()) {
        if (!key.startsWith(prefix) || key === dir) continue;
        const rest = key.slice(prefix.length);
        const first = rest.split('/')[0];
        const path = prefix + first;
        children.set(path, isDirectory(files, path));
      }
      return children;
    };
    const lines = [];
    const visit = (dir, level) => {
      const children = [...childrenOf(dir).keys()].sort((a, b) => a < b ? -1 : a > b ? 1 : 0);
      for (const path of children) {
        const directory = isDirectory(files, path);
        lines.push(directory ? `${path}/` : path);
        if (directory && level > 1) visit(path, level - 1);
      }
    };
    if (isDirectory(files, base)) visit(base, depth);
    else if (files.has(base)) lines.push(base);
    return lines.join('\n');
  }
  function searchFiles(files, base, args) {
    const pattern = String(args.pattern || '').toLowerCase();
    const types = String(args.file_types || '').split(',').map((x) => x.trim().replace(/^\./, '').toLowerCase()).filter(Boolean);
    const prefix = base ? `${base}/` : '';
    const lines = [];
    for (const [path, entry] of [...files.entries()].sort(([a], [b]) => a.localeCompare(b))) {
      if (!path.startsWith(prefix) || entry.type !== 'file') continue;
      const extension = path.includes('.') ? path.split('.').pop().toLowerCase() : '';
      if (types.length && !types.includes(extension)) continue;
      const relative = path.slice(prefix.length);
      if (relative.toLowerCase().includes(pattern)) lines.push(relative);
      entry.content.split('\n').forEach((line, index) => {
        if (line.toLowerCase().includes(pattern)) lines.push(`${relative}:${index + 1}: ${line.slice(0, 500)}`);
      });
    }
    return lines.slice(0, 2000).join('\n');
  }
  function virtualTool(ws, name, args) {
    const files = seedWorkspace(ws);
    const path = relativePath(args.path);
    if (name === 'list_files') return listFiles(files, path, Math.max(1, Math.min(6, +args.depth || 3)));
    if (name === 'search_files') return searchFiles(files, path, args);
    if (name === 'read_file') {
      const entry = node(files, path);
      if (!entry || entry.type !== 'file') throw new Error(`虚拟文件不存在：${path}`);
      const start = Math.max(1, +args.start_line || 1);
      const count = Math.max(1, Math.min(20000, +args.max_lines || 2000));
      return entry.content.split('\n').map((line, index) => `${index + 1}\t${line}`).slice(start - 1, start - 1 + count).join('\n');
    }
    if (name === 'write_file') {
      if (!path) throw new Error('write_file 需要文件路径');
      files.set(path, { type: 'file', content: String(args.content ?? '') });
      return `已写入 ${path}（浏览器预览虚拟文件）`;
    }
    if (name === 'edit_file') {
      const entry = node(files, path);
      if (!entry || entry.type !== 'file') throw new Error(`虚拟文件不存在：${path}`);
      const oldText = String(args.old_text ?? '');
      const newText = String(args.new_text ?? '');
      if (!oldText) throw new Error('old_text 不能为空。请先 read_file 查看当前内容。');
      const matches = entry.content.split(oldText).length - 1;
      if (!matches) throw new Error(`未找到原文：${path}。old_text 必须与当前文件完全一致。请先调用 read_file 重新读取，再逐字复制 old_text。`);
      if (matches > 1 && !args.replace_all) throw new Error(`原文出现多次：${path}。请提供更多上下文，或明确 replace_all。`);
      entry.content = args.replace_all ? entry.content.split(oldText).join(newText) : entry.content.replace(oldText, newText);
      return `已修改 ${path}（浏览器预览虚拟文件）`;
    }
    if (name === 'make_dir') {
      if (!path) throw new Error('make_dir 需要文件夹路径');
      files.set(path, { type: 'dir', content: '' });
      return `已创建文件夹 ${path}（浏览器预览）`;
    }
    if (name === 'delete_path') {
      if (!path) throw new Error('不能删除虚拟工作区根目录');
      for (const key of [...files.keys()]) if (key === path || key.startsWith(`${path}/`)) files.delete(key);
      return `已删除 ${path}（浏览器预览虚拟文件）`;
    }
    if (name === 'package_files') {
      const source = path || '.';
      const output = relativePath(args.output_path);
      if (!output) throw new Error('package_files 需要 output_path');
      const contents = [...files.entries()].filter(([key, entry]) => entry.type === 'file' && key !== output && (!source || source === '.' || key === source || key.startsWith(`${source}/`))).map(([key]) => key);
      files.set(output, { type: 'file', content: JSON.stringify({ preview: true, files: contents }, null, 2) });
      return `已打包 ${output}（预览模拟，不生成真实 ZIP）`;
    }
    throw new Error(`浏览器预览不支持工作区工具：${name}`);
  }
  function unsupported(operation) {
    throw new Error(`浏览器预览不支持${operation}。请在 ono 桌面应用中使用真实功能。`);
  }

  async function invoke(command, args = {}) {
    commandLog.push({ command, args });
    if (commandLog.length > 500) commandLog.shift();
    await Promise.resolve();
    switch (command) {
      case 'get_settings': return clone(settings);
      case 'save_settings': Object.assign(settings, clone(args.settings || {})); return clone(settings);
      case 'get_overview': return {
        hardware: hardware(), models_dir: settings.models_dir, recs: recommendations(),
        engine: { server_path: '/ono-preview/llama-server', variant: 'vulkan', tag: '浏览器预览模拟' },
        conclusion: { headline: '浏览器预览 · 模拟数据', detail: '可浏览页面并体验虚拟工作区；不会检测真实硬件或启动模型。' }
      };
      case 'get_catalog': return recommendations();
      case 'list_local': return [clone(sampleModel)];
      case 'plan_model': return samplePlan();
      case 'server_status': return { running: false, ready: false, port: settings.port, model_path: null, model_name: null };
      case 'stop_model': return null;
      case 'engine_check': return { bad_arch: [], gpu_ok: false, unknown: true, gpus: [], devices: '浏览器预览：未检测真实推理引擎。' };
      case 'live_stats': return { vram_used: 2 * GIB, vram_total: 12 * GIB, ram_used: 10 * GIB, ram_total: 32 * GIB };
      case 'sys_monitor': {
        const gpu = hardware().gpus[0];
        return { cores: 16, cpu_name: '示例处理器', cpu: 12, gpus: [{ ...gpu, vram_total: gpu.vram_bytes, vram_used: gpu.vram_used_bytes }], procs: [], mem_used: 10 * GIB, mem_total: 32 * GIB, ram_used: 10 * GIB, ram_total: 32 * GIB };
      }
      case 'load_progress': return { running: false, stage: '', fraction: 0, read_bytes: 0, total_bytes: 0, speed_bps: 0, elapsed_s: 0, last_log: '' };
      case 'last_crash': return '';
      case 'image_catalog': return imageCatalog();
      case 'image_engine_status': return { server_path: '/ono-preview/sd-server', variant: 'vulkan', tag: '浏览器预览模拟' };
      case 'image_history': return [];
      case 'image_status': return { running: false, ready: false };
      case 'list_loras': return [];
      case 'stop_image': case 'cancel_video': return null;
      case 'images_dir_path': return '/ono-preview-images';
      case 'lora_dir_path': return '/ono-preview-loras';
      case 'auto_adopt_image_models': return [];
      case 'get_autostart': return autostart;
      case 'set_autostart': autostart = !!args.on; return autostart;
      case 'set_window_theme': return null;
      case 'agent_resolve': {
        const ws = String(args.ws || PREVIEW_WS);
        seedWorkspace(ws);
        const path = relativePath(args.path);
        const files = workspaces.get(ws);
        return { path: path ? `${ws}\\${path.replace(/\//g, '\\')}` : ws, inside: true, exists: !path || files.has(path) || isDirectory(files, path) };
      }
      case 'agent_tool': return virtualTool(String(args.ws || PREVIEW_WS), String(args.name || ''), args.args || {});
      case 'agent_abort_task': return 0;
      case 'agent_pick_folder': return PREVIEW_WS;
      case 'list_skills': return clone(skills);
      case 'read_skill': {
        const skill = skills.find((item) => item.name === args.name);
        if (!skill) throw new Error(`Skill 不存在：${args.name}`);
        if (args.enabledOnly && !skill.enabled) throw new Error(`Skill ${args.name} 已停用`);
        if (String(args.path || 'SKILL.md') !== 'SKILL.md') throw new Error('预览 Skill 只有示例 SKILL.md');
        return { name: skill.name, path: 'SKILL.md', content: skill.content };
      }
      case 'set_skill_enabled': {
        const skill = skills.find((item) => item.name === args.name);
        if (!skill) throw new Error(`Skill 不存在：${args.name}`);
        skill.enabled = !!args.enabled;
        return clone(skill);
      }
      case 'delete_skill': skills = skills.filter((item) => item.name !== args.name); return null;
      case 'memory_search': return memories.filter((item) => String(item.content).toLowerCase().includes(String(args.query || '').toLowerCase())).slice(0, +args.limit || 5);
      case 'memory_add': {
        const item = { id: `preview-${Date.now()}`, content: String(args.content || ''), tags: Array.isArray(args.tags) ? args.tags : [] };
        memories.push(item); return item;
      }
      case 'memory_update': {
        const item = memories.find((entry) => entry.id === args.id);
        if (!item) throw new Error(`记忆不存在：${args.id}`);
        if (args.content !== undefined) item.content = String(args.content);
        if (args.tags !== undefined) item.tags = args.tags;
        return item;
      }
      case 'api_list': return [];
      case 'api_save': return null;
      case 'api_abort': return null;
      case 'start_model': {
        const error = modelStartError;
        modelStartError = '';
        if (error) throw error;
        return unsupported(command);
      }
      case 'generate_image': case 'generate_video': case 'start_image':
      case 'download_model': case 'download_image_model': case 'install_engine': case 'install_image_engine':
      case 'api_stream': case 'api_models': case 'tool_web_search': case 'tool_web_fetch':
      case 'import_path': case 'import_skill': case 'read_image': case 'pick_gguf': case 'pick_skill_file':
      case 'pick_attachments': case 'pick_image_files': case 'import_image_file': case 'import_lora':
        return unsupported(command);
      case 'run_command': case 'run_code': return unsupported('在浏览器中执行命令或代码');
      case 'open_path': case 'open_images_dir': case 'open_url': case 'save_text_file': case 'save_image_file':
        return unsupported('原生文件或桌面操作');
      default: return unsupported(`IPC 命令 ${command}`);
    }
  }

  const eventListeners = new Map();
  window.__TAURI__ = {
    core: { invoke },
    event: {
      listen: async (name, handler) => {
        const listeners = eventListeners.get(name) || new Set();
        listeners.add(handler);
        eventListeners.set(name, listeners);
        return () => listeners.delete(handler);
      },
      emit: async (name, payload) => {
        for (const handler of eventListeners.get(name) || []) handler({ event: name, id: 0, payload });
      }
    },
    app: { getVersion: async () => '0.0.0-preview' },
    window: {
      getCurrentWindow: () => ({
        isFullscreen: async () => !!document.fullscreenElement,
        setFullscreen: async (on) => {
          if (on && !document.fullscreenElement) await document.documentElement.requestFullscreen();
          if (!on && document.fullscreenElement) await document.exitFullscreen();
        }
      })
    }
  };

  function mountPreviewUi() {
    document.body.classList.add('ono-preview');
    const bar = document.createElement('div');
    bar.className = 'ono-preview-bar';
    bar.dataset.noi18n = '';
    bar.innerHTML = '<strong>浏览器预览 · 模拟数据</strong><span>不访问真实文件、不启动模型、不执行命令</span><button type="button" id="onoSmokeButton">运行 Smoke Test</button>';
    document.body.prepend(bar);
    const panel = document.createElement('section');
    panel.className = 'ono-preview-panel hidden';
    panel.id = 'onoSmokePanel';
    panel.dataset.noi18n = '';
    panel.innerHTML = '<div class="ono-preview-head"><b>Smoke Test</b><button type="button" id="onoSmokeClose" aria-label="关闭自检面板">×</button></div><p>检查主要页面、设置读写、虚拟工作区、Agent 问答去重、失败调用拦截、计划收尾，以及模型加载失败恢复。</p><div id="onoSmokeSummary">尚未运行</div><ol id="onoSmokeSteps"></ol>';
    document.body.appendChild(panel);
    document.getElementById('onoSmokeButton').addEventListener('click', () => {
      panel.classList.remove('hidden');
      runSmokeTest();
    });
    document.getElementById('onoSmokeClose').addEventListener('click', () => panel.classList.add('hidden'));
  }
  function renderSmokeResult(result) {
    const panel = document.getElementById('onoSmokePanel');
    if (!panel) return;
    panel.classList.remove('hidden');
    document.getElementById('onoSmokeSummary').textContent = result.ok ? `通过 · ${result.steps.length} 项` : `失败 · ${result.error || '检查未通过'}`;
    document.getElementById('onoSmokeSteps').replaceChildren(...result.steps.map((step) => {
      const item = document.createElement('li');
      item.className = step.ok ? 'ok' : 'failed';
      item.textContent = `${step.ok ? '✓' : '✕'} ${step.name}${step.ok ? '' : `：${step.error}`}`;
      return item;
    }));
  }
  const wait = (ms = 30) => new Promise((resolve) => setTimeout(resolve, ms));
  async function waitForApp() {
    for (let i = 0; i < 150; i++) {
      if (typeof S !== 'undefined' && S.overview && S.local && S.settings && document.readyState === 'complete') return;
      await wait(100);
    }
    throw new Error('应用初始化超时');
  }
  async function smokeStep(result, name, fn) {
    try {
      await fn();
      result.steps.push({ name, ok: true });
    } catch (error) {
      result.steps.push({ name, ok: false, error: String(error?.message || error) });
      throw error;
    }
  }
  async function checkAgent(result) {
    const ws = PREVIEW_WS;
    await invoke('agent_tool', { ws, name: 'write_file', args: { path: 'smoke/agent.txt', content: 'alpha\nbeta\n' } });
    const originalStream = window.streamOnce;
    const originalAsk = window.askUser;
    const originalTasks = S.ag.tasks;
    const originalId = S.ag.id;
    const originalPage = S.page;
    const failedEditsBefore = commandLog.filter((entry) => entry.command === 'agent_tool' && entry.args.name === 'edit_file' && entry.args.args?.old_text === 'missing').length;
    let asks = 0;
    let turn = 0;
    const task = { id: 'preview-smoke-agent', title: '预览自检任务', ws, allow: {}, messages: [], todos: [], changed: [], status: '', created: Date.now() };
    const call = (id, name, args) => ({ id, name, args: JSON.stringify(args) });
    window.askUser = async () => { asks++; return '简洁风格'; };
    window.streamOnce = async () => {
      turn++;
      if (turn === 1) return { text: '', calls: [
        call('plan', 'update_plan', { items: [
          { text: '检查计划状态', status: 'in_progress' },
          { text: '检查问答去重', status: 'in_progress' },
          { text: '检查编辑失败恢复', status: 'pending' }
        ] }),
        call('ask', 'ask_user', { question: '要采用哪种样式？' })
      ] };
      if (turn === 2) {
        if (task.todos.filter((item) => item.status === 'in_progress').length !== 1) throw new Error('计划同时存在多个 in_progress');
        return { text: '', calls: [
          call('ask-again', 'ask_user', { question: '要采用哪种样式' }),
          call('read', 'read_file', { path: 'smoke/agent.txt' }),
          call('bad-edit', 'edit_file', { path: 'smoke/agent.txt', old_text: 'missing', new_text: 'x' }),
          call('bad-edit-again', 'edit_file', { path: 'smoke/agent.txt', old_text: 'missing', new_text: 'x' }),
          call('good-edit', 'edit_file', { path: 'smoke/agent.txt', old_text: 'alpha', new_text: 'ALPHA' }),
          call('done', 'update_plan', { items: [
            { text: '检查计划状态', status: 'done' },
            { text: '检查问答去重', status: 'done' },
            { text: '检查编辑失败恢复', status: 'done' }
          ] })
        ] };
      }
      return { text: '自检完成', calls: [] };
    };
    S.ag.tasks = [task];
    S.ag.id = task.id;
    S.ag.ws = ws;
    S.page = 'agent';
    try {
      await runAgent(task);
    } finally {
      window.streamOnce = originalStream;
      window.askUser = originalAsk;
      S.ag.tasks = originalTasks;
      S.ag.id = originalId;
      S.page = originalPage;
      saveTasks();
      invalidateTree();
      render();
      await invoke('agent_tool', { ws, name: 'delete_path', args: { path: 'smoke/agent.txt' } });
    }
    if (asks !== 1) throw new Error(`重复问题弹出了 ${asks} 次`);
    if (!Object.values(task.questionAnswers || {}).includes('简洁风格')) throw new Error('问题答案没有缓存');
    if (!taskHistory(task, task.messages.length).some((message) => String(message.content).includes('简洁风格'))) throw new Error('历史上下文没有带回问题答案');
    const failedEdits = commandLog.filter((entry) => entry.command === 'agent_tool' && entry.args.name === 'edit_file' && entry.args.args?.old_text === 'missing').length - failedEditsBefore;
    if (failedEdits !== 1) throw new Error(`相同失败编辑实际执行了 ${failedEdits} 次`);
    if (task.status !== 'done') throw new Error(`修正后任务状态仍为 ${task.status}`);
    if (document.querySelector('.ag-plan .qp-spin')) throw new Error('任务结束后计划仍在转圈');
  }

  async function checkModelStartFailure() {
    const originalRun = { ...S.run };
    const originalServer = S.server;
    const originalCrash = S.crash;
    const originalVision = S.vision;
    const originalLogs = S.logs;
    const originalPage = S.page;
    const failure = '加载模型失败（exit code: 1）\n原因：显存不足。\n\n引擎日志：\nunknown model architecture: example\nllama_server: exiting due to model loading error';
    try {
      S.run.path = S.local[0].path;
      S.run.starting = false;
      S.run.error = '';
      go('run');
      window.__ONO_PREVIEW__.failNextModelStart(failure);
      const started = await actions.startModel({ stay: true });
      if (started !== false) throw new Error('模拟加载失败后仍返回成功');
      if (S.run.starting) throw new Error('加载失败后 starting 没有复位');
      if (S.run.error !== failure) throw new Error('完整错误日志没有保留');
      if (S.server.ready) throw new Error('加载失败后服务仍显示就绪');
      const alert = document.querySelector('#main .alert');
      if (!alert?.textContent.includes('unknown model architecture: example')) throw new Error('页面没有显示完整引擎日志');
      if (!document.querySelector('details[data-open="logsOpen"]')?.open) throw new Error('加载失败后运行日志没有展开');
      const retry = alert.querySelector('[data-act="retryModelCpu"]');
      if (!retry) throw new Error('缺少本次 CPU 重试入口');
      const before = commandLog.filter((entry) => entry.command === 'start_model').length;
      window.__ONO_PREVIEW__.failNextModelStart(failure);
      retry.click();
      for (let i = 0; i < 100 && (S.run.starting || commandLog.filter((entry) => entry.command === 'start_model').length === before); i++) await wait(30);
      const starts = commandLog.filter((entry) => entry.command === 'start_model');
      if (starts.length !== before + 1) throw new Error('CPU 重试没有发起启动请求');
      if (starts.at(-1).args.cpu !== true) throw new Error('CPU 重试没有传递 cpu 参数');
      if (S.run.starting || S.run.error !== failure) throw new Error('CPU 重试失败后状态没有正确收尾');
    } finally {
      Object.assign(S.run, originalRun);
      S.server = originalServer;
      S.crash = originalCrash;
      S.vision = originalVision;
      S.logs = originalLogs;
      go(originalPage);
      render();
    }
  }

  async function runSmokeTest() {
    if (smokeRunning) return lastSmokeTest;
    smokeRunning = true;
    const result = { ok: false, steps: [], error: '', at: new Date().toISOString() };
    lastSmokeTest = result;
    renderSmokeResult(result);
    const originalPage = typeof S !== 'undefined' ? S.page : null;
    try {
      if (typeof S === 'undefined' || !S.ag) throw new Error('应用仍在初始化，请稍后再运行 Smoke Test');
      if (S.ag.running || S.streaming) throw new Error('请先停止正在运行的任务或对话，再运行 Smoke Test');
      await smokeStep(result, '应用初始化', waitForApp);
      await smokeStep(result, '主要页面导航', async () => {
        const selectors = { overview: '#main h1', models: '#main h1', run: '#runSel', chat: '#msgs', agent: '#msgInput', image: '#main h1', video: '#main h1', monitor: '#main h1', settings: '#main h1' };
        for (const page of Object.keys(selectors)) {
          go(page);
          if (page === 'image' || page === 'video') await loadImageData(true);
          await wait();
          if (!document.querySelector(selectors[page])) throw new Error(`${page} 页缺少 ${selectors[page]}`);
        }
        if (S.mon.timer) monStop();
      });
      await smokeStep(result, '设置保存与读取', async () => {
        const before = await invoke('get_settings');
        try {
          const saved = await invoke('save_settings', { settings: { ...before, port: 18687 } });
          if (saved.port !== 18687 || (await invoke('get_settings')).port !== 18687) throw new Error('设置未往返一致');
        } finally {
          await invoke('save_settings', { settings: before });
        }
      });
      await smokeStep(result, '虚拟工作区写读列包', async () => {
        const ws = PREVIEW_WS;
        const tree = await invoke('agent_tool', { ws, name: 'list_files', args: { path: '.', depth: 2 } });
        if (!tree.startsWith('README.md\nindex.html\n')) throw new Error('文件树漏掉排序靠前的 README.md 或 index.html');
        await invoke('agent_tool', { ws, name: 'write_file', args: { path: 'smoke/hello.txt', content: 'hello ono' } });
        if (!(await invoke('agent_tool', { ws, name: 'read_file', args: { path: 'smoke/hello.txt' } })).includes('hello ono')) throw new Error('虚拟文件读取失败');
        if (!(await invoke('agent_tool', { ws, name: 'list_files', args: { path: '.', depth: 2 } })).includes('smoke/hello.txt')) throw new Error('虚拟文件树未刷新');
        await invoke('agent_tool', { ws, name: 'package_files', args: { path: 'smoke', output_path: 'smoke.zip' } });
        if (!(await invoke('agent_resolve', { ws, path: 'smoke.zip' })).exists) throw new Error('虚拟打包结果不存在');
        await invoke('agent_tool', { ws, name: 'delete_path', args: { path: 'smoke' } });
        await invoke('agent_tool', { ws, name: 'delete_path', args: { path: 'smoke.zip' } });
      });
      await smokeStep(result, 'Agent 回归检查', () => checkAgent(result));
      await smokeStep(result, '模型加载失败恢复与 CPU 重试', checkModelStartFailure);
      await smokeStep(result, '浏览器运行错误检查', async () => {
        if (browserErrors.length) throw new Error(browserErrors.join('；'));
      });
      result.ok = true;
    } catch (error) {
      result.error = String(error?.message || error);
    } finally {
      smokeRunning = false;
      if (originalPage && typeof S !== 'undefined') go(originalPage);
      renderSmokeResult(result);
    }
    return result;
  }

  window.__ONO_PREVIEW__ = {
    active: true,
    runSmokeTest,
    getLastResult: () => lastSmokeTest,
    getCalls: () => clone(commandLog),
    getErrors: () => [...browserErrors],
    failNextModelStart: (message) => { modelStartError = String(message || '模拟模型加载失败'); }
  };
  const start = () => {
    mountPreviewUi();
    waitForApp().then(runSmokeTest).catch((error) => {
      lastSmokeTest = { ok: false, steps: [], error: String(error), at: new Date().toISOString() };
      renderSmokeResult(lastSmokeTest);
    });
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start, { once: true });
  else start();
})();
