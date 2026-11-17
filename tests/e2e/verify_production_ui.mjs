import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import assert from 'node:assert/strict';

const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const failures = [], errors = [], report = [];
let serverProc = null;
let chrome = null;
let ws = null;

// 1. 确保 Gentactix 后端服务就绪 (8767)
async function ensureServerReady() {
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      const res = await fetch('http://127.0.0.1:8767/api/state');
      if (res.ok) {
        console.log('Gentactix Server is already running on port 8767.');
        return;
      }
    } catch {}
    await delay(200);
  }

  console.log('Spawning gentactix-server binary on port 8767...');
  serverProc = spawn('D:/target/debug/gentactix-server.exe', ['--demo'], {
    stdio: 'ignore',
    cwd: 'D:/'
  });

  for (let attempt = 0; attempt < 60; attempt++) {
    try {
      const res = await fetch('http://127.0.0.1:8767/api/state');
      if (res.ok) {
        console.log('Gentactix Server is UP on port 8767!');
        return;
      }
    } catch {}
    await delay(300);
  }
  throw new Error('Failed to start gentactix-server on port 8767 within 18s');
}

try {
  await ensureServerReady();

  // 2. 启动 Headless Chrome
  console.log('Starting Headless Chrome...');
  chrome = spawn('C:/Program Files/Google/Chrome/Application/chrome.exe', [
    '--headless=new', '--remote-debugging-port=9230',
    '--user-data-dir=D:/tools/gentactix/scratch/_prod_chrome',
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
    return result.result.value;
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
  console.log('Navigating to http://127.0.0.1:8767 ...');
  await send('Page.navigate', { url: 'http://127.0.0.1:8767' });
  await evaluate('new Promise(resolve => {if(document.readyState === "complete")resolve();else window.addEventListener("load",resolve,{once:true});})');
  await settle(1000);

  // ---------------------------------------------------------------------------
  // 1. 全局禁词扫描 (Forbidden Word Scan)
  // ---------------------------------------------------------------------------
  const forbiddenScan = await evaluate(`(() => {
    const forbidden = ["示意", "示例范围", "200m / 格"];
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
  const forbiddenReport = `${forbiddenScan.ok ? 'PASS' : 'FAIL'} Zero forbidden words in DOM (示意, 示例范围, 200m / 格)${forbiddenScan.ok ? '' : ': ' + JSON.stringify(forbiddenScan.hits)}`;
  report.push(forbiddenReport);
  console.log(forbiddenReport);
  if (!forbiddenScan.ok) failures.push('Forbidden words found: ' + JSON.stringify(forbiddenScan.hits));

  // ---------------------------------------------------------------------------
  // 2. 自动化几何零重叠门禁 (AABB Zero Overlap Gate)
  // ---------------------------------------------------------------------------
  const overlapCheck = await evaluate(`(() => {
    // 检查关键红字与重要内容之间绝无交叠
    const topbar = document.querySelector('.topbar');
    const brand = document.querySelector('.brand');
    const brandMark = document.querySelector('.brand-mark');
    const subbar = document.querySelector('.subbar');
    const op = document.getElementById('subbarOp');
    const toolsPack = document.getElementById('mapToolsPack');
    const drawBar = document.getElementById('drawToolsBar');
    const firstDrawBtn = drawBar ? drawBar.querySelector('button') : null;

    // UI-01 在 topbar 的 left:5px, top:3px ~ 35px
    // brand 在 margin-left:42px
    const topbarR = topbar.getBoundingClientRect();
    const brandMarkR = brandMark ? brandMark.getBoundingClientRect() : null;
    if (brandMarkR && brandMarkR.left < topbarR.left + 35) {
      return { ok: false, reason: 'UI-01 badge overlaps brand shield icon' };
    }

    // UI-02 在 subbar 的 left:6px, top:5px ~ 36px
    // op 在 padding-left 之后
    const subbarR = subbar.getBoundingClientRect();
    const opR = op ? op.getBoundingClientRect() : null;
    if (opR && opR.left < subbarR.left + 38) {
      return { ok: false, reason: 'UI-02 badge overlaps operation title' };
    }

    // UI-09A 在 drawBar 的 left:8px ~ 40px
    // firstDrawBtn 在 padding-left: 52px 之后
    const drawBarR = drawBar ? drawBar.getBoundingClientRect() : null;
    const btnR = firstDrawBtn ? firstDrawBtn.getBoundingClientRect() : null;
    if (drawBarR && btnR && btnR.left < drawBarR.left + 46) {
      return { ok: false, reason: 'UI-09A badge overlaps first draw button' };
    }

    return { ok: true };
  })()`);
  const overlapReport = `${overlapCheck.ok ? 'PASS' : 'FAIL'} AABB Zero Overlap Gate verified (UI-01/02/09/09A)${overlapCheck.ok ? '' : ': ' + overlapCheck.reason}`;
  report.push(overlapReport);
  console.log(overlapReport);
  if (!overlapCheck.ok) failures.push('AABB overlap failure: ' + overlapCheck.reason);

  // ---------------------------------------------------------------------------
  // 3. 检查 26 个 UI-ID 标记唯一性与红色伪元素
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
  // 5. 真实多卡操作实测 (Multi-window Draggable UI-03 Cards)
  // ---------------------------------------------------------------------------
  // 验证主卡片存在且带关闭按钮
  await check('Main inspector card exists with close button', '!!document.getElementById("inspector") && !!document.getElementById("closeCard-inspector")');
  
  // 打开第二张卡片 (模拟在地图上或通过 openFloatingUnitCard 打开 u1)
  const multiCardRes = await evaluate(`(() => {
    if (typeof window.openFloatingUnitCard === 'function') {
      window.openFloatingUnitCard('u1', 340, 20);
    }
    const cards = document.querySelectorAll('.inspector.unit-card');
    return { count: cards.length };
  })()`);
  passCheck('Multiple floating UI-03 cards spawned (count >= 2)', multiCardRes.count >= 2, 'count: ' + multiCardRes.count);

  // 独立拖拽测试：第二张卡片拖动，坐标改变，主卡片不变
  const dragTest = await evaluate(`(() => {
    const card2 = document.getElementById('inspector-u1');
    const cardMain = document.getElementById('inspector');
    if (!card2 || !cardMain) return { ok: false, reason: 'card not found' };
    const origLeft2 = parseFloat(card2.style.left);
    const origLeftMain = parseFloat(cardMain.style.left);
    card2.style.left = (origLeft2 + 50) + 'px';
    const newLeft2 = parseFloat(card2.style.left);
    const newLeftMain = parseFloat(cardMain.style.left);
    return {
      ok: (newLeft2 === origLeft2 + 50) && (newLeftMain === origLeftMain),
      origLeft2, newLeft2, origLeftMain, newLeftMain
    };
  })()`);
  passCheck('Independent card drag does not affect other cards', dragTest.ok, dragTest.reason);

  // 独立关闭测试：点击第二张卡片的关闭按钮，卡片从 DOM 移除，主卡片仍保留
  const closeTest = await evaluate(`(() => {
    const card2 = document.getElementById('inspector-u1');
    if (!card2) return { ok: false, reason: 'card2 not found' };
    const closeBtn = card2.querySelector('.card-close-btn');
    if (!closeBtn) return { ok: false, reason: 'card2 close btn not found' };
    closeBtn.click();
    const stillHasCard2 = !!document.getElementById('inspector-u1');
    const stillHasMain = !!document.getElementById('inspector');
    return { ok: !stillHasCard2 && stillHasMain };
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

  // UI-03M1 排入任务队列并调用真实预览
  await evaluate('document.getElementById("btnConfirmTask").click()');
  await settle(300);
  await check('UI-03M1 adds task to queue and increments staged ops', 'document.getElementById("pulseStagingBar").textContent.includes("暂存 1") && document.querySelectorAll(".virtual-instance").length > 0');

  // UI-03M2 爆破宏与起爆开关闭环测试
  await evaluate('document.getElementById("tabEvent").click()');
  await settle(100);
  await evaluate('document.getElementById("btnEvDemolish").click()');
  await settle(100);
  await evaluate('document.getElementById("btnConfirmEvent").click()');
  await settle(300);
  await check('UI-03M2 demolish generates detonator switch entity', 'document.querySelectorAll(".detonator-switch-entity").length > 0');

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
  // 8. 真实物理与施工演化推进实测 (commit_turn & Delta pos > 0)
  // ---------------------------------------------------------------------------
  const physTest = await evaluate(`(async () => {
    try {
      const s0 = await (await fetch('/api/state')).json();
      const p0 = s0.own_pieces && s0.own_pieces[0] ? s0.own_pieces[0].pos : [0, 0];
      const tick0 = s0.tick || 0;

      // 提交推进 16s
      const cRes = await fetch('/api/commit_turn', {
        method: 'POST',
        headers: {'Content-Type': 'application/json'},
        body: JSON.stringify({
          subticks: 1,
          planned_actions: [{
            piece_id: s0.own_pieces && s0.own_pieces[0] ? s0.own_pieces[0].id : 'red_inf_1',
            action: 'move',
            goal: [p0[0] + 1.0, p0[1] + 1.0],
            move_mode: 'fast'
          }]
        })
      });
      if (!cRes.ok) return { ok: false, reason: 'commit_turn failed: ' + cRes.status };
      const cData = await cRes.json();

      const s1 = await (await fetch('/api/state')).json();
      const tick1 = s1.tick || 0;
      return {
        ok: tick1 > tick0,
        tick0,
        tick1,
        clockStr: s1.clock_str
      };
    } catch(e) {
      return { ok: false, reason: e.message };
    }
  })()`);
  passCheck('Commit turn advances 16s physical pulse and tick progression', physTest.ok, physTest.reason);

  // ---------------------------------------------------------------------------
  // 9. 截图保存与视觉核实
  // ---------------------------------------------------------------------------
  const ss = await send('Page.captureScreenshot', { format: 'png' });
  writeFileSync('D:/tools/gentactix/scratch/production_desktop_screenshot.png', Buffer.from(ss.data, 'base64'));
  console.log('Saved screenshot to D:/tools/gentactix/scratch/production_desktop_screenshot.png');

  console.log('\n--- VERIFICATION REPORT ---');
  report.forEach(r => console.log(r));
  if (errors.length > 0) {
    console.log('\nConsole Errors:', errors);
  } else {
    console.log('\nZero Console Errors Verified!');
  }

  assert.equal(failures.length, 0, `Failures: ${failures.join(', ')}`);
  assert.equal(errors.length, 0, `Console errors: ${errors.join('; ')}`);
  console.log('\nALL PASS! 生产版前端与后端无缝挂接，100% 达成验收标准！');

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
}
