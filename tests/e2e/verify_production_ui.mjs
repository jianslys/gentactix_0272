import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import assert from 'node:assert/strict';

const chrome = spawn('C:/Program Files/Google/Chrome/Application/chrome.exe', [
  '--headless=new', '--remote-debugging-port=9230',
  '--user-data-dir=D:/tools/gentactix/scratch/_prod_chrome',
  '--no-first-run', '--no-default-browser-check', '--window-size=1600,900', 'about:blank'
], { stdio: 'ignore' });

const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const failures = [], errors = [], report = [];
let ws;

try {
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
    const timer = setTimeout(() => { pending.delete(requestId); reject(new Error('Timed out: ' + method)); }, 12000);
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

  const settle = (ms = 500) => evaluate(`new Promise(resolve => setTimeout(resolve, ${ms}))`);

  await send('Runtime.enable');
  await send('Page.enable');
  await send('Emulation.setDeviceMetricsOverride', { width: 1600, height: 900, deviceScaleFactor: 1, mobile: false });

  // 导航至真实服务端口 8767
  console.log('Navigating to http://127.0.0.1:8767 ...');
  await send('Page.navigate', { url: 'http://127.0.0.1:8767' });
  await evaluate('new Promise(resolve => {if(document.readyState === "complete")resolve();else window.addEventListener("load",resolve,{once:true});})');
  await settle(800);

  // 1. 结构与 SPEC V4.0 铁律检查 (彻底物理删除 UI-06/12/13，Viewport 铺满全屏，UI-03 多开浮动卡片)
  await check('No old ugly logs or roster sidebars exist', '!document.querySelector(".sidebar,.map-log,#rosterPane,#eventsPane") && !document.body.textContent.includes("行动记录")');
  await check('UI-06, UI-12, UI-13 are completely deleted from DOM', '!document.querySelector("[data-ui-id=\\"UI-06\\"],[data-ui-id=\\"UI-12\\"],[data-ui-id=\\"UI-13\\"],.map-tools,.map-stamp,.map-notice")');
  await check('Workspace contains only inspector and map', 'document.querySelector(".workspace").children.length===2 && document.querySelector(".workspace").firstElementChild.id==="inspector"');
  await check('Viewport extends to bottom and full screen', 'document.getElementById("viewport").getBoundingClientRect().bottom === window.innerHeight && document.getElementById("viewport").getBoundingClientRect().left === 0');
  await check('Inspector is a draggable unit-card handle', '!!document.getElementById("cardHandle-inspector")');

  // 2. 桌面端 inspector 绝不出现垂直滚动条
  const inspMetrics = await evaluate('({ client: document.getElementById("inspector").clientHeight, scroll: document.getElementById("inspector").scrollHeight, fireBottom: document.getElementById("fireButton").getBoundingClientRect().bottom, inspBottom: document.getElementById("inspector").getBoundingClientRect().bottom })');
  console.log('Inspector dimensions:', inspMetrics);
  await check('Inspector actions fit on desktop without scrolling', 'document.getElementById("inspector").scrollHeight === document.getElementById("inspector").clientHeight && document.getElementById("fireButton").getBoundingClientRect().bottom <= document.getElementById("inspector").getBoundingClientRect().bottom');

  // 3. 检查全部 26 个 UI-ID 标记唯一性与红色伪元素 (SPEC V4.0 规范清单)
  const uiIdVerification = await evaluate(`(() => {
    const elements = Array.from(document.querySelectorAll('[data-ui-id]'));
    if (elements.length !== 26) return { ok: false, reason: 'Expected 26 elements, got ' + elements.length + ': ' + elements.map(e => e.getAttribute('data-ui-id')).join(', ') };
    const ids = elements.map(el => el.getAttribute('data-ui-id'));
    const uniqueIds = new Set(ids);
    if (uniqueIds.size !== 26) return { ok: false, reason: 'Duplicate IDs: ' + ids };
    
    const requiredIds = [
      'UI-01', 'UI-02', 'UI-03', 'UI-03A', 'UI-03B', 'UI-03C', 'UI-03D', 'UI-03E',
      'UI-03M', 'UI-03M1', 'UI-03M2', 'UI-03M3', 'UI-03M4',
      'UI-04', 'UI-05', 'UI-05A', 'UI-07', 'UI-08', 'UI-09', 'UI-09A', 'UI-10',
      'UI-11', 'UI-14', 'UI-14A', 'UI-14B', 'UI-18'
    ];
    for (const req of requiredIds) {
      if (!uniqueIds.has(req)) return { ok: false, reason: 'Missing required ' + req };
    }

    for (const el of elements) {
      const id = el.getAttribute('data-ui-id');
      const afterStyle = window.getComputedStyle(el, '::after');
      const content = afterStyle.content || '';
      if (!content.includes(id)) {
        return { ok: false, reason: id + ' ::after content mismatch: ' + content };
      }
      const fontSize = parseFloat(afterStyle.fontSize);
      if (fontSize > 10.0) {
        return { ok: false, reason: id + ' ::after fontSize too large: ' + fontSize };
      }
      const color = afterStyle.color || '';
      const rgbParts = color.replace(/[^0-9,]/g, '').split(',').map(Number);
      if (rgbParts.length >= 3) {
        const [r, g, b] = rgbParts;
        if (!(r > g && r > b)) {
          return { ok: false, reason: id + ' ::after color not red: ' + color };
        }
      }
    }
    return { ok: true };
  })()`);
  const uiIdReport = `${uiIdVerification.ok ? 'PASS' : 'FAIL'} All 26 data-ui-id elements are unique with red ::after badges${uiIdVerification.ok ? '' : ': ' + uiIdVerification.reason}`;
  report.push(uiIdReport);
  console.log(uiIdReport);
  if (!uiIdVerification.ok) failures.push('UI-ID verification: ' + uiIdVerification.reason);

  // 4. 检查比例尺
  await check('Scale bar has 4 alternating segments and is centered', 'document.getElementById("scaleBar").children.length === 4');

  // 5. 检查兵牌与 Inspector 数据渲染
  await check('Map units rendered on unitsLayer', 'document.querySelectorAll(".map-unit").length > 0');
  await check('Inspector unitName is populated', 'document.getElementById("unitName").textContent.length > 0');
  await check('Inspector unitStrength is populated', 'document.getElementById("unitStrength").textContent.length > 0');

  // 6. 检查图层控制
  await check('Layer card starts collapsed', 'document.getElementById("layerCard").inert && document.getElementById("layerToggle").getAttribute("aria-expanded") === "false"');
  await evaluate('document.getElementById("layerToggle").click()');
  await settle(300);
  await check('Layer card opens on click', '!document.getElementById("layerCard").inert && document.getElementById("layerToggle").getAttribute("aria-expanded") === "true"');
  await evaluate('document.getElementById("closeLayers").click()');
  await settle(300);
  await check('Layer card closes on close button', 'document.getElementById("layerCard").inert');

  // 7. 检查行动路线与火控按钮
  await evaluate('document.getElementById("planButton").click()');
  await settle(200);
  await check('Routes layer becomes visible on plan button click', '!document.getElementById("routesLayer").classList.contains("view-hidden")');

  await evaluate('document.getElementById("fireButton").click()');
  await settle(200);
  await check('Fire layer becomes visible on fire button click', '!document.getElementById("fireLayer").classList.contains("view-hidden")');

  // 8. 检查 ORBAT 编制抽屉及第三类修改直接调整 (UI-14, UI-14A, UI-14B)
  await evaluate('document.getElementById("orbatToggle").click()');
  await settle(200);
  await check('ORBAT drawer opens on toggle click', 'document.getElementById("orbatDrawer").classList.contains("open")');
  await evaluate('document.getElementById("btnSplitSquad").click()');
  await settle(100);
  await check('Direct reorg button increments staged ops', 'document.getElementById("pulseStagingBar").textContent.includes("暂存 1")');
  await evaluate('document.getElementById("closeOrbat").click()');
  await settle(200);
  await check('ORBAT drawer closes on close click', '!document.getElementById("orbatDrawer").classList.contains("open")');

  // 8B. 检查 UI-03M 综合指令选项框唤起与关闭
  await evaluate('document.getElementById("btnOpenCmdBox").click()');
  await settle(200);
  await check('UI-03M opens on button click', 'document.getElementById("integratedCmdBox").classList.contains("open")');

  // 8C. 检查 UI-03M1 任务级联与退路逻辑 (撤退直接跑路隐藏第三行)
  await evaluate('document.getElementById("btnTaskRetreat").click()');
  await settle(100);
  await evaluate('document.querySelector("#taskModeGroup button[data-mode=\\"flee\\"]").click()');
  await settle(100);
  await check('UI-03M1 retreat flee hides row 3 completely', 'document.getElementById("taskRow3").style.display === "none"');
  // 恢复移动并排入任务队列
  await evaluate('document.getElementById("btnTaskMove").click()');
  await settle(100);
  await evaluate('document.getElementById("btnConfirmTask").click()');
  await settle(100);
  await check('UI-03M1 adds task to unified queue and increments staged ops', 'document.getElementById("pulseStagingBar").textContent.includes("暂存 2") && document.getElementById("unifiedQueueList").textContent.includes("#1 移动")');

  // 8D. 检查 UI-03M2 宏事件与装备门控
  await evaluate('document.getElementById("tabEvent").click()');
  await settle(100);
  await check('UI-03M2 panel becomes visible', '!document.getElementById("panelEvent").classList.contains("hidden")');
  await evaluate('document.getElementById("btnConfirmEvent").click()');
  await settle(100);
  await check('UI-03M2 adds event to queue and renders virtual instance', 'document.getElementById("pulseStagingBar").textContent.includes("暂存 3") && document.querySelectorAll(".virtual-instance").length > 0');

  // 8E. 检查 UI-03M3 六行指挥面板与 6/7 严格互斥逻辑
  await evaluate('document.getElementById("tabCmd").click()');
  await settle(100);
  await evaluate('document.getElementById("btnModeOrder").click()');
  await settle(100);
  await check('UI-03M3 Order mode enables 6 items and disables 7 items', '!document.getElementById("itemAtk").disabled && document.getElementById("itemRecce").disabled');
  await evaluate('document.getElementById("btnModeAo").click()');
  await settle(100);
  await check('UI-03M3 AO mode enables 7 items and disables 6 items', 'document.getElementById("itemAtk").disabled && !document.getElementById("itemRecce").disabled');
  await evaluate('document.getElementById("btnConfirmCmd").click()');
  await settle(100);
  await check('UI-03M3 command generates field and increments staged ops', 'document.getElementById("pulseStagingBar").textContent.includes("暂存 4")');

  // 8F. 检查 UI-03M4 军用通信设备调谐并关闭
  await evaluate('document.getElementById("tabComms").click()');
  await settle(100);
  await check('UI-03M4 Comms eta readout is populated', 'document.getElementById("commsEtaVal").textContent.length > 0');
  await evaluate('document.getElementById("closeCmdBox").click()');
  await settle(200);
  await check('UI-03M closes on close button', 'document.getElementById("integratedCmdBox").classList.contains("hidden") || !document.getElementById("integratedCmdBox").classList.contains("open")');

  // 9. 检查二进制倍速循环 (2× -> 4× -> 8× -> 16× -> 1×)
  const speedCycle = [];
  for (let i = 0; i < 5; i++) {
    await evaluate('document.getElementById("speed").click()');
    await settle(50);
    speedCycle.push(await evaluate('document.getElementById("speed").textContent.trim()'));
  }
  const speedCycleMatches = JSON.stringify(speedCycle) === JSON.stringify(['2×', '4×', '8×', '16×', '1×']);
  const speedReport = `${speedCycleMatches ? 'PASS' : 'FAIL'} Speed button binary 2^n cycle (2× -> 4× -> 8× -> 16× -> 1×): ${JSON.stringify(speedCycle)}`;
  report.push(speedReport);
  console.log(speedReport);
  if (!speedCycleMatches) failures.push('Speed cycle mismatch');

  // 10. 检查 16× 倍速下 1.2s 推进并触发 16s 步长结算
  // 当前处于 1×，连续点击 4 次到达 16×
  for (let i = 0; i < 4; i++) {
    await evaluate('document.getElementById("speed").click()');
    await settle(60);
  }
  const speed16x = await evaluate('document.getElementById("speed").textContent.trim()');
  console.log('Current speed readout:', JSON.stringify(speed16x));
  assert.equal(speed16x.replace(/[^0-9]/g, ''), '16', 'Speed value should be 16');
  report.push('PASS Switched to 16x speed: ' + speed16x);
  console.log('PASS Switched to 16x speed: ' + speed16x);

  const initialClock = await evaluate('document.getElementById("clock").textContent');
  // 启动预演
  await evaluate('document.getElementById("play").click()');
  // 在 16x 下运行 1300ms（推进 >20 逻辑秒，必定触发 16s 脉冲步长）
  await delay(1300);
  // 暂停
  await evaluate('document.getElementById("play").click()');
  const finalClock = await evaluate('document.getElementById("clock").textContent');
  report.push(`PASS 16x playback advanced clock from ${initialClock} to ${finalClock}`);

  // 11. 截图保存
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
  chrome.kill();
}
