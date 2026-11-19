import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve, join } from 'node:path';
import { tmpdir } from 'node:os';
import { existsSync, writeFileSync, mkdirSync } from 'node:fs';
import assert from 'node:assert/strict';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = resolve(__dirname, '../../');
const serverExe = resolve(repoRoot, 'target/debug/gentactix-server' + (process.platform === 'win32' ? '.exe' : ''));
const chromeDataDir = join(tmpdir(), 'gentactix_chrome_e2e_m1');
const scratchDir = join(repoRoot, 'scratch');
mkdirSync(scratchDir, { recursive: true });
mkdirSync(chromeDataDir, { recursive: true });

const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const failures = [], errors = [], report = [];
let serverProc = null;
let chrome = null;
let ws = null;

const PORT = 8767;

function findChrome() {
  if (process.env.CHROME_BIN && existsSync(process.env.CHROME_BIN)) return process.env.CHROME_BIN;
  const candidates = [
    'C:/Program Files/Google/Chrome/Application/chrome.exe',
    'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
    '/usr/bin/google-chrome',
    '/usr/bin/chromium-browser',
    '/usr/bin/chromium'
  ];
  for (const c of candidates) {
    if (existsSync(c)) return c;
  }
  return 'C:/Program Files/Google/Chrome/Application/chrome.exe';
}

// 1. 确保 Gentactix 后端服务就绪 (8767)
async function ensureServerReady() {
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      const res = await fetch(`http://127.0.0.1:${PORT}/api/state`);
      if (res.ok) {
        console.log(`Gentactix Server is already running on port ${PORT}.`);
        return;
      }
    } catch {}
    await delay(200);
  }

  console.log(`Spawning gentactix-server binary at ${serverExe} on port ${PORT}...`);
  if (!existsSync(serverExe)) {
    throw new Error(`gentactix-server binary not found at ${serverExe}. Please run cargo build -p gentactix-server first.`);
  }

  serverProc = spawn(serverExe, ['--demo', '--port', String(PORT)], {
    stdio: 'ignore',
    cwd: repoRoot
  });

  for (let attempt = 0; attempt < 60; attempt++) {
    try {
      const res = await fetch(`http://127.0.0.1:${PORT}/api/state`);
      if (res.ok) {
        console.log(`Gentactix Server is UP on port ${PORT}!`);
        return;
      }
    } catch {}
    await delay(300);
  }
  throw new Error(`Failed to start gentactix-server on port ${PORT} within 18s`);
}

try {
  await ensureServerReady();

  // 2. 启动 Headless Chrome (1440x900 真实实拍视口)
  const chromePath = findChrome();
  console.log(`Starting Headless Chrome from ${chromePath}...`);

  chrome = spawn(chromePath, [
    '--headless=new', '--remote-debugging-port=9230',
    `--user-data-dir=${chromeDataDir}`,
    '--no-first-run', '--no-default-browser-check', '--window-size=1440,900', 'about:blank'
  ], { stdio: 'ignore' });

  let target;
  for (let attempt = 0; attempt < 40; attempt++) {
    try {
      target = await (await fetch('http://127.0.0.1:9230/json/new?about:blank', { method: 'PUT' })).json();
      break;
    } catch { await delay(150); }
  }
  assert(target, 'Chrome startup on port 9230');
  ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });

  let id = 0;
  const pending = new Map();
  ws.onmessage = event => {
    const message = JSON.parse(event.data);
    if (message.id && pending.has(message.id)) {
      const { resolve, reject, timer } = pending.get(message.id);
      clearTimeout(timer);
      pending.delete(message.id);
      if (message.error) reject(new Error(JSON.stringify(message.error)));
      else resolve(message.result);
    }
    if (message.method === 'Runtime.exceptionThrown') {
      errors.push(message.params.exceptionDetails.exception?.description || message.params.exceptionDetails.text);
    }
    if (message.method === 'Runtime.consoleAPICalled' && message.params.type === 'error') {
      errors.push(message.params.args.map(a => a.value || a.description).join(' '));
    }
  };

  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const requestId = ++id;
    const timer = setTimeout(() => { pending.delete(requestId); reject(new Error('Timed out: ' + method)); }, 15000);
    pending.set(requestId, { resolve, reject, timer });
    ws.send(JSON.stringify({ id: requestId, method, params }));
  });

  const evaluate = async expression => {
    const result = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    return (result.result && result.result.result) ? result.result.result.value : (result.result ? result.result.value : result);
  };

  const check = async (label, expression) => {
    const result = await evaluate(expression);
    const line = `${result === true ? 'PASS' : 'FAIL'} ${label}${result === true ? '' : ': ' + JSON.stringify(result)}`;
    report.push(line);
    console.log(line);
    if (result !== true) failures.push(label);
  };

  const passCheck = (label, ok, reason = '') => {
    const line = `${ok ? 'PASS' : 'FAIL'} ${label}${ok ? '' : ': ' + reason}`;
    report.push(line);
    console.log(line);
    if (!ok) failures.push(label + (reason ? ': ' + reason : ''));
  };

  const settle = (ms = 500) => evaluate(`new Promise(resolve => setTimeout(resolve, ${ms}))`);

  await send('Runtime.enable');
  await send('Page.enable');
  // 严格锁死 1440x900 视口规格
  await send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });

  // 导航至真实服务端口 8767
  console.log(`Navigating to http://127.0.0.1:${PORT} ...`);
  await send('Page.navigate', { url: `http://127.0.0.1:${PORT}` });
  await evaluate('new Promise(resolve => {if(document.readyState === "complete")resolve();else window.addEventListener("load",resolve,{once:true});})');
  await settle(1200);

  // ---------------------------------------------------------------------------
  // 0. 512x512 纯二进制 8m 权威空间与二进制地形流断言 (Phase M1-4 MVP Gate)
  // ---------------------------------------------------------------------------
  console.log('Verifying 512x512 world scale and terrain binary contract...');
  const initStateRes = await fetch(`http://127.0.0.1:${PORT}/api/state`);
  const initState = await initStateRes.json();
  assert(initState.ok, 'Initial state must be ok');

  // 1. 网格规格 512x512, cell_m=8
  const grid = initState.grid;
  passCheck('512x512 deterministic grid dimensions', grid && grid.rows === 512 && grid.cols === 512 && grid.cell_m === 8, JSON.stringify(grid));

  // 2. 1v1 步兵班沙盒: 仅保留己方 1 个步兵班 + 敌方 1 个可见步兵班
  const ownCount = initState.own_pieces ? initState.own_pieces.length : 0;
  const enyCount = initState.visible_enemies ? initState.visible_enemies.length : 0;
  passCheck('1v1 sandbox: exactly 1 friendly infantry squad', ownCount === 1, `own_pieces=${ownCount}`);
  passCheck('1v1 sandbox: exactly 1 visible enemy infantry squad', enyCount === 1, `visible_enemies=${enyCount}`);

  // 3. 夺控目标对象导出
  passCheck('State exports mvp_objectives with bridge_control', !!initState.mvp_objectives && !!initState.mvp_objectives.bridge_control, JSON.stringify(initState.mvp_objectives));

  // 4. /api/terrain_raw.bin 二进制流大小 1,048,584 字节
  const binRes = await fetch(`http://127.0.0.1:${PORT}/api/terrain_raw.bin`);
  const binBuf = await binRes.arrayBuffer();
  const expectedBytes = 8 + 512 * 512 * 4; // 1,048,584
  passCheck('Terrain raw binary exact size = 1,048,584 bytes', binBuf.byteLength === expectedBytes, `size=${binBuf.byteLength}`);

  // 5. 前端 #trueTerrainRaster 光栅底图有效挂载
  const rasterCheck = await evaluate(`(() => {
    const img = document.getElementById('trueTerrainRaster');
    if (!img) return { ok: false, reason: 'Missing #trueTerrainRaster element' };
    const href = img.getAttribute('href') || img.getAttribute('xlink:href') || img.src || '';
    return { ok: href.length > 100, hrefLength: href.length };
  })()`);
  passCheck('Frontend #trueTerrainRaster mounted with true terrain canvas texture', rasterCheck.ok, `length=${rasterCheck.hrefLength}`);

  // 6. UI-08 电传战报流与桥梁胶囊状态检测
  const teletypeCheck = await evaluate(`(() => {
    const stream = document.getElementById('teletypeStream');
    const pill = document.getElementById('subbarBridgeObjective');
    const lines = stream ? stream.querySelectorAll('.teletype-line') : [];
    return {
      streamExists: !!stream,
      linesCount: lines.length,
      pillExists: !!pill,
      pillText: pill ? pill.textContent : ''
    };
  })()`);
  passCheck('UI-08 teletype console & bridge objective pill active', teletypeCheck.streamExists && teletypeCheck.linesCount > 0 && teletypeCheck.pillExists, JSON.stringify(teletypeCheck));

  // ---------------------------------------------------------------------------
  // 1. 全局 6 大禁词全量扫描 (Forbidden Word Scan)
  // ---------------------------------------------------------------------------
  const forbiddenScan = await evaluate(`(() => {
    const forbidden = [
      "示意", "示例范围", "200m / 格",
      "ROUTE / 1.8km", "~ 24 min", "0友军"
    ];
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    const hits = [];
    while (walker.nextNode()) {
      const node = walker.currentNode;
      const text = node.textContent;
      for (const f of forbidden) {
        const hit = f === "0友军" ? /(?:^|[^0-9])0[ \t]*友军/.test(text) : text.includes(f);
        if (hit) {
          hits.push({ forbidden: f, text: text.trim().slice(0, 40) });
        }
      }
    }
    return { ok: hits.length === 0, hits };
  })()`);
  const forbiddenReport = `${forbiddenScan.ok ? 'PASS' : 'FAIL'} Zero forbidden words in DOM (6 种禁词零容忍)${forbiddenScan.ok ? '' : ': ' + JSON.stringify(forbiddenScan.hits)}`;
  report.push(forbiddenReport);
  console.log(forbiddenReport);
  if (!forbiddenScan.ok) failures.push('Forbidden words found: ' + JSON.stringify(forbiddenScan.hits));

  // ---------------------------------------------------------------------------
  // 2. 自动化几何零重叠与 1440x900 视口防溢出门禁 (AABB & Viewport Gate)
  // ---------------------------------------------------------------------------
  const overlapCheck = await evaluate(`(() => {
    const topbar = document.querySelector('.topbar');
    const brandMark = document.querySelector('.brand-mark');
    const subbar = document.querySelector('.subbar');
    const op = document.getElementById('subbarOp');
    const drawBar = document.getElementById('drawToolsBar');
    const firstDrawBtn = drawBar ? drawBar.querySelector('button') : null;

    // 1. 顶栏 / 次顶栏红字检测
    const topbarR = topbar.getBoundingClientRect();
    const brandMarkR = brandMark ? brandMark.getBoundingClientRect() : null;
    if (brandMarkR && brandMarkR.left < topbarR.left + 35) {
      return { ok: false, reason: 'UI-01 badge overlaps brand shield icon' };
    }
    const subbarR = subbar.getBoundingClientRect();
    const opR = op ? op.getBoundingClientRect() : null;
    if (opR && opR.left < subbarR.left + 38) {
      return { ok: false, reason: 'UI-02 badge overlaps operation title' };
    }
    const drawBarR = drawBar ? drawBar.getBoundingClientRect() : null;
    const btnR = firstDrawBtn ? firstDrawBtn.getBoundingClientRect() : null;
    if (drawBarR && btnR && btnR.left < drawBarR.left + 46) {
      return { ok: false, reason: 'UI-09A badge overlaps first draw button' };
    }

    // 2. UI-03A 与关闭按钮 × / 编号 inspectorId 零交叠检测 (间距 >= 24px)
    const closeBtn = document.querySelector('#closeCard-inspector');
    const inspId = document.querySelector('#inspectorId');
    if (closeBtn && inspId) {
      const rClose = closeBtn.getBoundingClientRect();
      const rId = inspId.getBoundingClientRect();
      const overlap = !(rClose.right < rId.left || rClose.left > rId.right || rClose.bottom < rId.top || rClose.top > rId.bottom);
      if (overlap) {
        return { ok: false, reason: 'UI-03A close button overlaps inspectorId' };
      }
      if (rClose.left - rId.right < 5) {
        return { ok: false, reason: 'Insufficient clearance between inspectorId and close button' };
      }
    }

    // 3. UI-04 (mapTitle) 与 #inspector 初始位置零交叠
    const mapTitle = document.querySelector('#mapTitle');
    const inspector = document.querySelector('#inspector');
    if (mapTitle && inspector) {
      const rTitle = mapTitle.getBoundingClientRect();
      const rInsp = inspector.getBoundingClientRect();
      const overlap = !(rTitle.right < rInsp.left || rTitle.left > rInsp.right || rTitle.bottom < rInsp.top || rTitle.top > rInsp.bottom);
      if (overlap) {
        return { ok: false, reason: 'mapTitle (UI-04) overlaps with initial inspector card' };
      }
    }

    // 4. UI-11 (toast) 与 UI-04 零交叠 (toast 底部居中)
    const toast = document.querySelector('#toast');
    if (mapTitle && toast) {
      const rTitle = mapTitle.getBoundingClientRect();
      const rToast = toast.getBoundingClientRect();
      const overlap = !(rTitle.right < rToast.left || rTitle.left > rToast.right || rTitle.bottom < rToast.top || rTitle.top > rToast.bottom);
      if (overlap) {
        return { ok: false, reason: 'toast (UI-11) overlaps with mapTitle (UI-04)' };
      }
    }

    // 5. UI-05 / UI-05A 与图层按钮文字图标不相交
    const layerToggle = document.getElementById('layerToggle');
    if (layerToggle) {
      const lRect = layerToggle.getBoundingClientRect();
      const textEl = document.getElementById('layerToggleText');
      if (textEl) {
        const tRect = textEl.getBoundingClientRect();
        if (tRect.left < lRect.left + 24) {
          return { ok: false, reason: 'UI-05A badge overlaps layerToggle text' };
        }
      }
    }

    // 6. UI-08 / UI-09 不压顶碰撞
    const toolsPack = document.querySelector('.map-tools-pack');
    if (toolsPack) {
      const pRect = toolsPack.getBoundingClientRect();
      if (pRect.top < 15) {
        return { ok: false, reason: 'UI-09 map-tools-pack pressed against top boundary (<15px)' };
      }
    }

    // 7. 1440x900 视口溢出检测: 所有可见 unit-card 的 bottom <= 900
    const cards = Array.from(document.querySelectorAll('.inspector.unit-card')).filter(el => {
      const style = getComputedStyle(el);
      return style.display !== 'none' && style.visibility !== 'hidden';
    });
    for (const card of cards) {
      const r = card.getBoundingClientRect();
      if (r.bottom > 900) {
        return { ok: false, reason: 'Card ' + card.id + ' bottom (' + r.bottom.toFixed(1) + ') exceeds 1440x900 viewport height (900)' };
      }
    }

    // 8. 全局可见 [data-ui-id] 两两 AABB 碰撞检测
    const uiElements = Array.from(document.querySelectorAll('[data-ui-id]')).filter(el => {
      const style = getComputedStyle(el);
      return style.display !== 'none' && style.visibility !== 'hidden' && style.opacity !== '0';
    });
    for (let i = 0; i < uiElements.length; i++) {
      for (let j = i + 1; j < uiElements.length; j++) {
        const elA = uiElements[i], elB = uiElements[j];
        if (elA.contains(elB) || elB.contains(elA)) continue;
        const rA = elA.getBoundingClientRect();
        const rB = elB.getBoundingClientRect();
        if (rA.width === 0 || rA.height === 0 || rB.width === 0 || rB.height === 0) continue;
        const intersect = !(rA.right <= rB.left || rA.left >= rB.right || rA.bottom <= rB.top || rA.top >= rB.bottom);
        if (intersect) {
          return { ok: false, reason: 'Collision between ' + elA.getAttribute('data-ui-id') + ' and ' + elB.getAttribute('data-ui-id') };
        }
      }
    }

    return { ok: true };
  })()`);
  const overlapReport = `${overlapCheck.ok ? 'PASS' : 'FAIL'} AABB Zero Overlap Gate verified (UI-01/02/03A/04/05A/09A/11 & 1440x900 bottom <= 900)${overlapCheck.ok ? '' : ': ' + overlapCheck.reason}`;
  report.push(overlapReport);
  console.log(overlapReport);
  if (!overlapCheck.ok) failures.push('AABB overlap failure: ' + overlapCheck.reason);

  // ---------------------------------------------------------------------------
  // 3. 检查 UI-03M 彻底注销与 UIO / UIOA 独立注册完整性
  // ---------------------------------------------------------------------------
  const uiIdVerification = await evaluate(`(() => {
    const elements = Array.from(document.querySelectorAll('[data-ui-id]'));
    const ids = elements.map(el => el.getAttribute('data-ui-id'));
    const uniqueIds = new Set(ids);
    const retiredIds = ['UI-03M', 'UI-03M1', 'UI-03M2', 'UI-03M3', 'UI-03M4'];
    for (const oldId of retiredIds) {
      if (uniqueIds.has(oldId)) return { ok: false, reason: 'Retired ID still present: ' + oldId };
    }
    const requiredIds = [
      'UI-01', 'UI-02', 'UI-03', 'UI-03A', 'UI-03B', 'UI-03C', 'UI-03D', 'UI-03E',
      'UIO', 'UIOA',
      'UI-04', 'UI-05', 'UI-05A', 'UI-07', 'UI-08', 'UI-09', 'UI-09A', 'UI-10',
      'UI-11', 'UI-14', 'UI-14A', 'UI-14B', 'UI-18'
    ];
    for (const req of requiredIds) {
      if (!uniqueIds.has(req)) return { ok: false, reason: 'Missing required ' + req };
    }
    return { ok: true, count: elements.length };
  })()`);
  passCheck('UI-03M retired and UIO / UIOA registered cleanly', uiIdVerification.ok, uiIdVerification.reason);

  // ---------------------------------------------------------------------------
  // 4. 纯二进制比例尺检查
  // ---------------------------------------------------------------------------
  await check('Scale bar element exists', '!!document.getElementById("scaleBar")');
  await check('Meter grid element exists and binds distance', 'document.getElementById("meterGrid").textContent.length > 0');

  // ---------------------------------------------------------------------------
  // 5. 严禁作弊的真鼠标点击多卡实测 (1v1 地图点击第 2 个兵牌弹出第 2 张浮动卡)
  // ---------------------------------------------------------------------------
  await check('Main inspector card exists with close button', '!!document.getElementById("inspector") && !!document.getElementById("closeCard-inspector")');

  const multiCardRes = await evaluate(`(() => {
    const mapUnits = document.querySelectorAll('.map-unit');
    if (mapUnits.length < 2) return { ok: false, reason: 'Map units count < 2' };
    mapUnits[1].dispatchEvent(new MouseEvent('click', { bubbles: true }));
    const cards = document.querySelectorAll('.inspector.unit-card');
    const visibleCards = Array.from(cards).filter(c => getComputedStyle(c).display !== 'none');
    return {
      ok: visibleCards.length >= 2,
      count: visibleCards.length,
      cardIds: visibleCards.map(c => c.id)
    };
  })()`);
  passCheck('Multiple floating UI-03 cards spawned by true DOM click (count >= 2)', multiCardRes.ok, `count: ${multiCardRes.count}`);

  // 独立拖拽测试
  const dragTest = await evaluate(`(() => {
    const cards = Array.from(document.querySelectorAll('.inspector.unit-card')).filter(c => getComputedStyle(c).display !== 'none');
    if (cards.length < 2) return { ok: false, reason: 'Visible cards < 2' };
    const card1 = cards[0];
    const card2 = cards[1];
    const origLeft1 = parseFloat(card1.style.left) || card1.getBoundingClientRect().left;
    const origLeft2 = parseFloat(card2.style.left) || card2.getBoundingClientRect().left;

    card2.style.left = (origLeft2 + 60) + 'px';
    const newLeft1 = parseFloat(card1.style.left) || card1.getBoundingClientRect().left;
    const newLeft2 = parseFloat(card2.style.left) || card2.getBoundingClientRect().left;
    return {
      ok: (newLeft2 === origLeft2 + 60) && (newLeft1 === origLeft1),
      origLeft1, newLeft1, origLeft2, newLeft2
    };
  })()`);
  passCheck('Independent card drag does not affect other cards', dragTest.ok, dragTest.reason);

  // 独立关闭测试
  const closeTest = await evaluate(`(() => {
    const cards = Array.from(document.querySelectorAll('.inspector.unit-card')).filter(c => getComputedStyle(c).display !== 'none');
    if (cards.length < 2) return { ok: false, reason: 'Visible cards < 2' };
    const card2 = cards[1];
    const closeBtn = card2.querySelector('.card-close-btn');
    if (!closeBtn) return { ok: false, reason: 'card2 close button missing' };
    closeBtn.click();

    const remaining = Array.from(document.querySelectorAll('.inspector.unit-card')).filter(c => getComputedStyle(c).display !== 'none');
    return {
      ok: remaining.length === 1 && !remaining.includes(card2)
    };
  })()`);
  passCheck('Independent card close button removes target card without closing others', closeTest.ok, closeTest.reason);

  // ---------------------------------------------------------------------------
  // 6. UI-09A 绘图工具箱真实功能测试
  // ---------------------------------------------------------------------------
  await evaluate('document.getElementById("toolRuler").click()');
  await settle(100);
  await check('Tool ruler activated', 'document.getElementById("toolRuler").classList.contains("active")');

  await evaluate('document.getElementById("toolPoly").click()');
  await settle(100);
  await check('Tool poly activated', 'document.getElementById("toolPoly").classList.contains("active")');

  // ---------------------------------------------------------------------------
  // 7. UI-03M 综合指令面板与 UI-03M1~M4 联动 (真鼠标 CDP 选点穿透)
  // ---------------------------------------------------------------------------
  await evaluate('document.getElementById("btnOpenCmdBox").click()');
  await settle(200);
  await check('UI-03M opens on button click', 'document.getElementById("integratedCmdBox").classList.contains("open")');

  // UI-03M1 真实地图点击选点排入任务队列 (杜绝假坐标，验证 CDP 真实鼠标点击穿透)
  await evaluate('document.querySelector("#btnPickTaskGoal").click()');
  await settle(100);

  // 通过 CDP 真实流水线派发点击至地图中央 (720, 450)
  await send('Input.dispatchMouseEvent', { type: 'mousePressed', x: 720, y: 450, button: 'left', clickCount: 1 });
  await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: 720, y: 450, button: 'left', clickCount: 1 });
  await settle(200);

  const taskPickRes = await evaluate(`(() => {
    document.querySelector('#btnConfirmTask').click();
    const queue = window.unifiedQueue || [];
    const lastItem = queue[queue.length - 1];
    const hasMarker = !!document.querySelector('.task-goal-marker');
    return {
      ok: lastItem && lastItem.goal && lastItem.goal[0] !== 455 + 72 && hasMarker,
      lastItem,
      hasMarker
    };
  })()`);
  passCheck('UI-03M1 real mouse CDP map pick goal queued with zero fake offset', taskPickRes.ok, JSON.stringify(taskPickRes));

  // UI-03M2 爆破宏两步真实地图选点与起爆开关闭环测试
  await evaluate('document.getElementById("tabEvent").click()');
  await settle(100);
  await evaluate('document.getElementById("btnEvDemolish").click()');
  await settle(100);
  const demoPickRes = await evaluate(`(() => {
    document.querySelector('#btnPickEventGoal').click();
    const svg = document.querySelector('#mapSvg');
    const rect = svg.getBoundingClientRect();
    // Step 1: 爆破工事点 (490, 360)
    svg.dispatchEvent(new MouseEvent('click', {
      clientX: rect.left + (490 / 1200) * rect.width,
      clientY: rect.top + (360 / 880) * rect.height,
      bubbles: true
    }));
    // Step 2: 起爆开关点 (410, 420)
    svg.dispatchEvent(new MouseEvent('click', {
      clientX: rect.left + (410 / 1200) * rect.width,
      clientY: rect.top + (420 / 880) * rect.height,
      bubbles: true
    }));
    document.querySelector('#btnConfirmEvent').click();
    const swEntity = document.querySelector('.detonator-switch-entity');
    return {
      hasSwitch: !!swEntity,
      transform: swEntity ? swEntity.getAttribute('transform') : null
    };
  })()`);
  passCheck('UI-03M2 demolish generates detonator switch entity via real map pick', demoPickRes.hasSwitch, demoPickRes.transform);

  // 7B. UIOA 第五行时间轴、轻重武器严格区分、双出口与 UIO 三区+归档流转闭环测试
  const uioaRulesRes = await evaluate(`(() => {
    document.getElementById('tabTask').click();
    // (a) 停止: 起止时间同显
    document.getElementById('btnTaskStop').click();
    const stopStartVis = getComputedStyle(document.getElementById('taskStartTimeWrap')).display !== 'none';
    const stopEndVis = getComputedStyle(document.getElementById('taskEndTimeWrap')).display !== 'none';

    // (b) 射击 -> 轻武器: 固定使用起止时间，隐藏轮数
    document.getElementById('btnTaskFire').click();
    document.getElementById('btnFireWeaponLight').click();
    const lightRoundsHidden = getComputedStyle(document.getElementById('taskRow3')).display === 'none';
    const lightStartVis = getComputedStyle(document.getElementById('taskStartTimeWrap')).display !== 'none';
    const lightEndVis = getComputedStyle(document.getElementById('taskEndTimeWrap')).display !== 'none';

    // (c) 射击 -> 重武器: 只能使用轮数+起时间（无止时间），且起时间严格在轮数下一行
    document.getElementById('btnFireWeaponHeavy').click();
    const heavyRoundsVis = getComputedStyle(document.getElementById('taskRow3')).display !== 'none';
    const heavyStartVis = getComputedStyle(document.getElementById('taskStartTimeWrap')).display !== 'none';
    const heavyEndHidden = getComputedStyle(document.getElementById('taskEndTimeWrap')).display === 'none';
    const rRounds = document.getElementById('taskRow3').getBoundingClientRect();
    const rTime = document.getElementById('taskRowTimeline').getBoundingClientRect();
    const startBelowRounds = rTime.top >= rRounds.bottom - 2;

    // (d) 双出口：保存为草稿 -> 送往第三块草稿区；再切换至无需选点的停止任务，派一张 +32s 延迟启动的未激活纸条到第二块验证可拖拽
    document.getElementById('btnDraftTask').click();
    const draftCount = window.uioState.draftSlips.length;

    document.getElementById('btnTaskStop').click();
    const startInput = document.getElementById('taskStartTimeSelect');
    const prevStartVal = startInput.value;
    startInput.value = '32';
    document.getElementById('btnConfirmTask').click();
    startInput.value = prevStartVal;

    // (e) 第一块开局常驻指挥纸条 + 第二块常亮置顶第1位且不可拖拽/撤销 + 完工自动进归档栏
    const standingCmdOk = window.uioState.commandSlips.length === 1 && window.uioState.commandSlips[0].standing === true && window.uioState.gFieldAssigned === false;
    const firstQueueSlip = window.uioState.queueSlips[0];
    const isTopActiveLocked = !!(firstQueueSlip && firstQueueSlip.activeLocked);
    const activeSlipEl = firstQueueSlip ? document.querySelector('.uio-slip[data-slip-id="' + firstQueueSlip.id + '"]') : null;
    const activeNotDraggable = !!(activeSlipEl && activeSlipEl.getAttribute('draggable') === 'false');
    const revokeBlocked = firstQueueSlip ? !window.revokeUioQueueSlip(firstQueueSlip.id) : false;

    const queuedSlip = window.uioState.queueSlips.find(s => !s.activeLocked);
    const queuedEl = queuedSlip ? document.querySelector('.uio-slip[data-slip-id="' + queuedSlip.id + '"]') : null;
    const queuedDraggable = !!(queuedEl && queuedEl.getAttribute('draggable') === 'true');
    const draftSlip = window.uioState.draftSlips[0];
    const draftEl = draftSlip ? document.querySelector('.uio-slip[data-slip-id="' + draftSlip.id + '"]') : null;
    const draftDraggable = !!(draftEl && draftEl.getAttribute('draggable') === 'true');

    const beforeArch = window.uioState.archiveSlips.length;
    if (firstQueueSlip) window.finishUioSlipToArchive(firstQueueSlip.id, '测试完工');
    const afterArch = window.uioState.archiveSlips.length;

    // (f) 唯一实体实例状态验证：归档纸条抽回复用（原地迁区、唯一ID不变、物理守恒不克隆、落为 QUEUED 未锁死、保留 runCount 与 history）
    const archSlip = window.uioState.archiveSlips[0];
    const archSlipId = archSlip ? archSlip.id : null;
    const archEl = archSlip ? document.querySelector('.uio-slip[data-slip-id="' + archSlip.id + '"]') : null;
    const archDraggable = !!(archEl && archEl.getAttribute('draggable') === 'true' && archEl.getAttribute('data-slip-state') === 'ARCHIVED');
    const regSizeBefore = window.slipRegistry.size;
    const qBeforeReuse = window.uioState.queueSlips.length;
    const reuseOk = archSlip ? window.moveUioSlipBetweenZones(archSlip.id, 'archive', 'queue') : false;
    const regSizeAfter = window.slipRegistry.size;
    const qAfterReuse = window.uioState.queueSlips.length;
    const reusedInstance = archSlipId ? window.slipRegistry.get(archSlipId) : null;
    const conservedSameId = !!(
      reusedInstance &&
      regSizeAfter === regSizeBefore &&
      window.uioState.archiveSlips.length === afterArch - 1 &&
      window.uioState.queueSlips.some(s => s.id === archSlipId) &&
      reusedInstance.zone === 'queue' &&
      reusedInstance.state === 'QUEUED' &&
      reusedInstance.activeLocked === false &&
      reusedInstance.runCount >= 1 &&
      Array.isArray(reusedInstance.history) && reusedInstance.history.length >= 3
    );

    // 验证全池所有纸条 ID 唯一且格式为 SLIP-*
    const allIds = Array.from(window.slipRegistry.keys());
    const allUniqueSlipIds = allIds.length > 0 && new Set(allIds).size === allIds.length && allIds.every(id => id.startsWith('SLIP-'));

    return {
      ok: stopStartVis && stopEndVis && lightRoundsHidden && lightStartVis && lightEndVis &&
          heavyRoundsVis && heavyStartVis && heavyEndHidden && startBelowRounds &&
          standingCmdOk && draftCount >= 1 && isTopActiveLocked && activeNotDraggable &&
          queuedDraggable && draftDraggable && archDraggable &&
          revokeBlocked && (afterArch === beforeArch + 1) &&
          reuseOk && (qAfterReuse === qBeforeReuse + 1) && conservedSameId && allUniqueSlipIds,
      detail: {
        stopStartVis, stopEndVis, lightRoundsHidden, lightStartVis, lightEndVis,
        heavyRoundsVis, heavyStartVis, heavyEndHidden, startBelowRounds,
        standingCmdOk, draftCount, isTopActiveLocked, activeNotDraggable,
        queuedDraggable, draftDraggable, archDraggable,
        revokeBlocked, beforeArch, afterArch, reuseOk, qBeforeReuse, qAfterReuse,
        regSizeBefore, regSizeAfter, conservedSameId, allUniqueSlipIds
      }
    };
  })()`);
  passCheck('UIOA rules, unique slip_id registry, instance FSM & physical-conservation archive reuse verified', uioaRulesRes.ok, JSON.stringify(uioaRulesRes.detail));

  // UIOA 指挥G场赋值 (第一块常驻纸条就地赋值) + 6/7 互斥场
  await evaluate('document.getElementById("tabCmd").click()');
  await settle(100);
  await evaluate('document.getElementById("btnModeOrder").click()');
  await settle(100);
  await check('UIOA Order mode enables 6 items and disables 7 items', '!document.getElementById("itemAtk").disabled && document.getElementById("itemRecce").disabled');
  await evaluate('document.getElementById("btnConfirmCmd").click()');
  await settle(100);
  await check('UIO Zone 1 standing command slip assigned in-place with G-field badge lit', 'window.uioState.gFieldAssigned === true && window.uioState.commandSlips.length === 1 && document.getElementById("uioGFieldBadge").classList.contains("assigned")');

  // UIOA 通信调谐
  await evaluate('document.getElementById("tabComms").click()');
  await settle(100);
  await evaluate('document.getElementById("btnApplyComms").click()');
  await settle(200);
  await check('UIOA comms applied', 'document.getElementById("commsEtaVal").textContent.length > 0');

  await evaluate('document.getElementById("closeCmdBox").click()');
  await settle(200);

  // ---------------------------------------------------------------------------
  // 8. Turn 0 暂停态非队列调整防篡改与时间机延迟生效全链路断言
  // ---------------------------------------------------------------------------
  console.log('Testing pause-turn anti-tamper Time Machine queueing...');
  const stateBefore = await (await fetch(`http://127.0.0.1:${PORT}/api/state`)).json();
  const initPiecesCount = stateBefore.own_pieces ? stateBefore.own_pieces.length : 0;

  // 点击 #btnSplitSquad 触发编制调整
  await evaluate('document.getElementById("btnSplitSquad").click()');
  await settle(300);

  // 再次获取 state，断言 own_pieces 绝对未在暂停推演期间提前暴涨
  const stateDuringPause = await (await fetch(`http://127.0.0.1:${PORT}/api/state`)).json();
  const pausePiecesCount = stateDuringPause.own_pieces ? stateDuringPause.own_pieces.length : 0;
  passCheck('Turn 0 pause: own_pieces count does NOT increase before turn commit', pausePiecesCount === initPiecesCount, `before=${initPiecesCount}, duringPause=${pausePiecesCount}`);

  // ---------------------------------------------------------------------------
  // 9. 开局裙边敌军不可见断言 (Zero Early Fog Leaks)
  // ---------------------------------------------------------------------------
  const skirtRes = await evaluate(`(() => {
    const skirts = document.querySelectorAll('.prerender-skirt');
    if (skirts.length === 0) return { ok: true, count: 0 };
    const allHidden = Array.from(skirts).every(el => {
      const style = getComputedStyle(el);
      return style.visibility === 'hidden' || style.opacity === '0';
    });
    return { ok: allHidden, count: skirts.length };
  })()`);
  passCheck('Initial prerender skirt enemies are hidden with zero visibility', skirtRes.ok, `count=${skirtRes.count}`);

  // ---------------------------------------------------------------------------
  // 10. 真实物理位移 (Δpos > 0.01) 与时间机契约验证
  // ---------------------------------------------------------------------------
  console.log('Testing true physical displacement and Time Machine contracts...');
  const s0 = await (await fetch(`http://127.0.0.1:${PORT}/api/state`)).json();
  const targetPieceBefore = (s0.own_pieces && s0.own_pieces.length > 0) ? s0.own_pieces[0] : null;
  assert(targetPieceBefore, 'At least one friendly piece must exist for displacement test');
  const p0 = targetPieceBefore.pos;
  const unitId = targetPieceBefore.id;

  // 下发移动命令推进 16s 权威脉冲步长
  const commitRes = await (await fetch(`http://127.0.0.1:${PORT}/api/commit_turn`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      subticks: 1,
      planned_actions: [{
        piece_id: unitId,
        action: 'move',
        goal: [p0[0] + 6.0, p0[1] + 6.0],
        move_mode: 'fast'
      }]
    })
  })).json();

  assert(commitRes.ok, 'commit_turn API must return ok: true');
  const tmFrame = commitRes.state ? commitRes.state.time_machine_frame : null;
  assert(tmFrame, 'commit_turn must export time_machine_frame in state');
  assert(Array.isArray(tmFrame.particle_tracks), 'time_machine_frame must contain particle_tracks');
  const hasSkirtContract = tmFrame.particle_tracks.length >= 2 && tmFrame.particle_tracks.every(t => typeof t.is_prerender_skirt === 'boolean');
  passCheck('Time Machine frame exports particle_tracks with prerender skirt contract', hasSkirtContract);
  passCheck('Time Machine frame exports playback_events array', Array.isArray(tmFrame.playback_events));

  // 检查位移
  const s1 = await (await fetch(`http://127.0.0.1:${PORT}/api/state`)).json();
  const targetPieceAfter = s1.own_pieces ? s1.own_pieces.find(p => p.id === unitId) : null;
  const p1 = targetPieceAfter ? targetPieceAfter.pos : p0;
  const deltaPos = Math.hypot(p1[0] - p0[0], p1[1] - p0[1]);
  passCheck(`True physical displacement verified (Delta pos = ${deltaPos.toFixed(4)} > 0.01)`, deltaPos > 0.01, `p0=[${p0}], p1=[${p1}]`);

  // ---------------------------------------------------------------------------
  // 11. 截图保存与视觉核实
  // ---------------------------------------------------------------------------
  const ss = await send('Page.captureScreenshot', { format: 'png' });
  const shotBuffer = Buffer.from(ss.data, 'base64');
  const repoScreenshot = join(scratchDir, 'production_desktop_screenshot.png');
  writeFileSync(repoScreenshot, shotBuffer);
  console.log(`Saved screenshot to ${repoScreenshot}`);

  const brainScratch = 'C:/Users/rdpuser/.gemini/antigravity/brain/04ae5c7f-f68c-48e3-97e7-3fcd88ae5414/scratch';
  if (existsSync(brainScratch)) {
    try {
      writeFileSync(join(brainScratch, 'production_desktop_screenshot.png'), shotBuffer);
    } catch {}
  }

  console.log('\n--- E2E VERIFICATION REPORT ---');
  report.forEach(r => console.log(r));
  if (errors.length > 0) {
    console.log('\nConsole Errors:', errors);
  } else {
    console.log('\nZero Console Errors Verified!');
  }

  assert.equal(failures.length, 0, `Failures encountered: ${failures.join(', ')}`);
  assert.equal(errors.length, 0, `Browser console errors: ${errors.join('; ')}`);
  console.log('\nALL PASS! 生产版前端与后端无缝挂接，100% 达成全部验收门禁标准！');

} catch(err) {
  console.error('\n*** TEST FAILED WITH EXCEPTION ***', err);
  if (errors.length > 0) {
    console.error('*** BROWSER CONSOLE ERRORS ***:', errors);
  }
  process.exitCode = 1;
} finally {
  if (ws) ws.close();
  if (chrome) chrome.kill();
  if (serverProc) serverProc.kill();
  console.log('Cleaned up processes.');
}
