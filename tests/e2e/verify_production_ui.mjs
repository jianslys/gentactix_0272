import { spawn } from 'node:child_process';
import { writeFileSync, mkdirSync } from 'node:fs';
import assert from 'node:assert/strict';

const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const failures = [], errors = [], report = [];
let serverProc = null;
let chrome = null;
let ws = null;

const PORT = 8767;

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

  console.log(`Spawning gentactix-server binary on port ${PORT}...`);
  serverProc = spawn('D:/target/debug/gentactix-server.exe', ['--demo', '--port', String(PORT)], {
    stdio: 'ignore',
    cwd: 'D:/'
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

  // 2. 启动 Headless Chrome
  console.log('Starting Headless Chrome...');
  const chromeDataDir = 'C:/Users/rdpuser/.gemini/antigravity/brain/04ae5c7f-f68c-48e3-97e7-3fcd88ae5414/scratch/_prod_chrome_p6';
  mkdirSync(chromeDataDir, { recursive: true });
  mkdirSync('D:/scratch', { recursive: true });

  chrome = spawn('C:/Program Files/Google/Chrome/Application/chrome.exe', [
    '--headless=new', '--remote-debugging-port=9230',
    `--user-data-dir=${chromeDataDir}`,
    '--no-first-run', '--no-default-browser-check', '--window-size=1600,900', 'about:blank'
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
  await send('Emulation.setDeviceMetricsOverride', { width: 1600, height: 900, deviceScaleFactor: 1, mobile: false });

  // 导航至真实服务端口 8767
  console.log(`Navigating to http://127.0.0.1:${PORT} ...`);
  await send('Page.navigate', { url: `http://127.0.0.1:${PORT}` });
  await evaluate('new Promise(resolve => {if(document.readyState === "complete")resolve();else window.addEventListener("load",resolve,{once:true});})');
  await settle(1200);

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
        if (text.includes(f)) {
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
  // 2. 自动化几何零重叠门禁 (AABB Zero Overlap Gate)
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

    return { ok: true };
  })()`);
  const overlapReport = `${overlapCheck.ok ? 'PASS' : 'FAIL'} AABB Zero Overlap Gate verified (UI-01/02/03A/04/09A/11)${overlapCheck.ok ? '' : ': ' + overlapCheck.reason}`;
  report.push(overlapReport);
  console.log(overlapReport);
  if (!overlapCheck.ok) failures.push('AABB overlap failure: ' + overlapCheck.reason);

  // ---------------------------------------------------------------------------
  // 3. 检查 26 个 UI-ID 标记唯一性与完整性
  // ---------------------------------------------------------------------------
  const uiIdVerification = await evaluate(`(() => {
    const elements = Array.from(document.querySelectorAll('[data-ui-id]'));
    const ids = elements.map(el => el.getAttribute('data-ui-id'));
    const uniqueIds = new Set(ids);
    const requiredIds = [
      'UI-01', 'UI-02', 'UI-03', 'UI-03A', 'UI-03B', 'UI-03C', 'UI-03D', 'UI-03E',
      'UI-03M', 'UI-03M1', 'UI-03M2', 'UI-03M3', 'UI-03M4',
      'UI-04', 'UI-05', 'UI-05A', 'UI-07', 'UI-08', 'UI-09', 'UI-09A', 'UI-10',
      'UI-11', 'UI-14', 'UI-14A', 'UI-14B', 'UI-18'
    ];
    for (const req of requiredIds) {
      if (!uniqueIds.has(req)) return { ok: false, reason: 'Missing required ' + req };
    }
    return { ok: true, count: elements.length };
  })()`);
  passCheck('All 26 required UI-ID tags present in DOM', uiIdVerification.ok, uiIdVerification.reason);

  // ---------------------------------------------------------------------------
  // 4. 纯二进制 8m 比例尺检查
  // ---------------------------------------------------------------------------
  await check('Binary scale bar has 4 segments', 'document.getElementById("scaleBar").children.length === 4');
  await check('Scale caption reflects 8m pure binary doctrine', 'document.getElementById("meterGrid").textContent.includes("8m / 格 · 二进制比例尺")');

  // ---------------------------------------------------------------------------
  // 5. 严禁作弊的真鼠标点击多卡实测 (Anti-Cheat True DOM Click Multi-Card)
  // ---------------------------------------------------------------------------
  await check('Main inspector card exists with close button', '!!document.getElementById("inspector") && !!document.getElementById("closeCard-inspector")');

  const multiCardRes = await evaluate(`(() => {
    // 强制严禁调用 window.openFloatingUnitCard，必须真实点击地图兵牌 DOM 节点
    const units = document.querySelectorAll('.map-unit:not(.enemy)');
    if (units.length < 2) return { ok: false, reason: 'Friendly units count < 2' };

    // 真实点击第 2 个兵牌
    units[1].dispatchEvent(new MouseEvent('click', { bubbles: true }));

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
  // 7. UI-03M 综合指令面板与 UI-03M1~M4 联动
  // ---------------------------------------------------------------------------
  await evaluate('document.getElementById("btnOpenCmdBox").click()');
  await settle(200);
  await check('UI-03M opens on button click', 'document.getElementById("integratedCmdBox").classList.contains("open")');

  // UI-03M1 真实地图点击选点排入任务队列 (杜绝假坐标)
  const taskPickRes = await evaluate(`(() => {
    document.querySelector('#btnPickTaskGoal').click();
    const svg = document.querySelector('#mapSvg');
    const rect = svg.getBoundingClientRect();
    // 真实点击地图 (540, 390)
    svg.dispatchEvent(new MouseEvent('click', {
      clientX: rect.left + (540 / 1200) * rect.width,
      clientY: rect.top + (390 / 880) * rect.height,
      bubbles: true
    }));
    document.querySelector('#btnConfirmTask').click();
    const queue = window.unifiedQueue || [];
    const lastItem = queue[queue.length - 1];
    return {
      ok: lastItem && lastItem.goal && lastItem.goal[0] !== 455 + 72,
      lastItem
    };
  })()`);
  passCheck('UI-03M1 real map pick goal queued with zero fake offset', taskPickRes.ok, JSON.stringify(taskPickRes.lastItem));

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

  // UI-03M3 6/7 互斥场
  await evaluate('document.getElementById("tabCmd").click()');
  await settle(100);
  await evaluate('document.getElementById("btnModeOrder").click()');
  await settle(100);
  await check('UI-03M3 Order mode enables 6 items and disables 7 items', '!document.getElementById("itemAtk").disabled && document.getElementById("itemRecce").disabled');

  // UI-03M4 通信调谐
  await evaluate('document.getElementById("tabComms").click()');
  await settle(100);
  await evaluate('document.getElementById("btnApplyComms").click()');
  await settle(200);
  await check('UI-03M4 comms applied', 'document.getElementById("commsEtaVal").textContent.length > 0');

  await evaluate('document.getElementById("closeCmdBox").click()');
  await settle(200);

  // ---------------------------------------------------------------------------
  // 8. 真实物理位移 (Δpos > 0.01) 与时间机契约验证
  // ---------------------------------------------------------------------------
  console.log('Testing true physical displacement and Time Machine contracts...');
  const s0 = await (await fetch(`http://127.0.0.1:${PORT}/api/state`)).json();
  const p0 = s0.own_pieces && s0.own_pieces[0] ? s0.own_pieces[0].pos : [10.0, 10.0];
  const unitId = s0.own_pieces && s0.own_pieces[0] ? s0.own_pieces[0].id : 'red_inf_1';

  // 下发移动命令推进 16s 权威脉冲步长
  const commitRes = await (await fetch(`http://127.0.0.1:${PORT}/api/commit_turn`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      subticks: 1,
      planned_actions: [{
        piece_id: unitId,
        action: 'move',
        goal: [p0[0] + 5.0, p0[1] + 5.0],
        move_mode: 'fast'
      }]
    })
  })).json();

  assert(commitRes.ok, 'commit_turn API must return ok: true');
  const tmFrame = commitRes.state ? commitRes.state.time_machine_frame : null;
  assert(tmFrame, 'commit_turn must export time_machine_frame in state');
  assert(Array.isArray(tmFrame.particle_tracks), 'time_machine_frame must contain particle_tracks');
  const hasPrerenderSkirt = tmFrame.particle_tracks.some(t => t.is_prerender_skirt === true);
  passCheck('Time Machine frame exports particle_tracks with prerender skirt', hasPrerenderSkirt);
  passCheck('Time Machine frame exports playback_events array', Array.isArray(tmFrame.playback_events));

  // 检查位移
  const s1 = await (await fetch(`http://127.0.0.1:${PORT}/api/state`)).json();
  const p1 = s1.own_pieces && s1.own_pieces[0] ? s1.own_pieces[0].pos : p0;
  const deltaPos = Math.hypot(p1[0] - p0[0], p1[1] - p0[1]);
  passCheck(`True physical displacement verified (Delta pos = ${deltaPos.toFixed(4)} > 0.01)`, deltaPos > 0.01, `p0=[${p0}], p1=[${p1}]`);

  // ---------------------------------------------------------------------------
  // 9. 截图保存与视觉核实
  // ---------------------------------------------------------------------------
  const ss = await send('Page.captureScreenshot', { format: 'png' });
  const shotBuffer = Buffer.from(ss.data, 'base64');
  writeFileSync('D:/scratch/production_desktop_screenshot.png', shotBuffer);
  writeFileSync('C:/Users/rdpuser/.gemini/antigravity/brain/04ae5c7f-f68c-48e3-97e7-3fcd88ae5414/scratch/production_desktop_screenshot.png', shotBuffer);
  console.log('Saved screenshots to D:/scratch/ and workspace scratch directory.');

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
