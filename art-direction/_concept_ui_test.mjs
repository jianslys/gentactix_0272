import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import assert from 'node:assert/strict';

const root = 'D:/tools/gentactix/art-direction';
const chrome = spawn('C:/Program Files/Google/Chrome/Application/chrome.exe', [
  '--headless=new', '--remote-debugging-port=9228',
  '--user-data-dir=D:/tools/gentactix/scratch/_concept_chrome',
  '--no-first-run', '--no-default-browser-check', '--window-size=1600,900', 'about:blank'
], { stdio: 'ignore' });
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const failures = [], errors = [], report = [], animationReport = {};
let ws;
try {
  let target;
  for (let attempt = 0; attempt < 40; attempt++) {
    try {
      target = await (await fetch('http://127.0.0.1:9228/json/new?about:blank', { method: 'PUT' })).json();
      break;
    } catch { await delay(150); }
  }
  assert(target, 'Chrome startup');
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
    if (message.method === 'Runtime.exceptionThrown') errors.push(message.params.exceptionDetails.exception?.description || message.params.exceptionDetails.text);
    if (message.method === 'Runtime.consoleAPICalled' && message.params.type === 'error') errors.push(message.params.args.map(a => a.value || a.description).join(' '));
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
    report.push(`${result === true ? 'PASS' : 'FAIL'} ${label}${result === true ? '' : ': ' + JSON.stringify(result)}`);
    if (result !== true) failures.push(label);
  };
  const settle = () => evaluate('new Promise(resolve => setTimeout(resolve, 380))');
  const click = async selector => {
    const point = await evaluate(`(() => {const e=document.querySelector(${JSON.stringify(selector)});const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+Math.min(r.height/2,25)};})()`);
    await send('Input.dispatchMouseEvent', { type: 'mousePressed', ...point, button: 'left', clickCount: 1 });
    await send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...point, button: 'left', clickCount: 1 });
  };
  const key = async key => {
    await send('Input.dispatchKeyEvent', { type: 'keyDown', key, code: key, windowsVirtualKeyCode: key === 'Escape' ? 27 : 13 });
    await send('Input.dispatchKeyEvent', { type: 'keyUp', key, code: key, windowsVirtualKeyCode: key === 'Escape' ? 27 : 13 });
  };
  const screenshot = async name => {
    await evaluate('new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))');
    const result = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
    writeFileSync(`${root}/${name}.png`, Buffer.from(result.data, 'base64'));
  };
  const beginSample = async expression => {
    await evaluate(`window.motionFrames=[];window.motionDone=new Promise(resolve=>{const start=performance.now();function sample(now){motionFrames.push({time:now-start,value:(${expression})});if(now-start<650)requestAnimationFrame(sample);else resolve(motionFrames);}requestAnimationFrame(sample);});void 0`);
  };
  const endSample = async name => {
    animationReport[name] = await evaluate('window.motionDone');
  };
  await send('Runtime.enable');
  await send('Page.enable');
  await send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'no-preference' }] });
  await send('Emulation.setDeviceMetricsOverride', { width: 1600, height: 900, deviceScaleFactor: 1, mobile: false });
  await send('Page.navigate', { url: 'http://127.0.0.1:8771/command-map-concept.html' });
  await evaluate('new Promise(resolve => {if(document.readyState === "complete")resolve();else window.addEventListener("load",resolve,{once:true});})');
  await settle();
  await check('Roster, intelligence-record sidebar and bottom action log are removed', '!document.querySelector(".sidebar,.map-log,#rosterPane,#eventsPane") && !document.body.textContent.includes("行动记录")');
  await check('Only inspector and map remain in workspace, inspector is first at far left', 'document.querySelector(".workspace").children.length===2 && document.querySelector(".workspace").firstElementChild.id==="inspector" && $("inspector").getBoundingClientRect().left===0 && $("viewport").getBoundingClientRect().left===$("inspector").getBoundingClientRect().right');
  await check('Map extends to screen bottom, without log or footer band', '$("viewport").getBoundingClientRect().bottom===innerHeight');
  await check('Scale is centered at map screen bottom', '(()=>{const s=$("meterGrid").getBoundingClientRect(),m=$("viewport").getBoundingClientRect();return Math.abs(s.x+s.width/2-(m.x+m.width/2))<1 && innerHeight-s.bottom<=14;})()');
  await check('Scale uses alternating four printed segments', '$("scaleBar").children.length===4 && getComputedStyle($("scaleBar").children[0]).backgroundColor!==getComputedStyle($("scaleBar").children[1]).backgroundColor');
  await check('Desktop inspector primary actions fit without scrolling', '$("fireButton").getBoundingClientRect().bottom<=$("inspector").getBoundingClientRect().bottom && $("inspector").scrollHeight===$("inspector").clientHeight');
  await check('No horizontal overflow at desktop size', 'document.documentElement.scrollWidth===1600');
  console.log('DESKTOP INSPECTOR', await evaluate('({client:$("inspector").clientHeight,scroll:$("inspector").scrollHeight,buttonBottom:$("fireButton").getBoundingClientRect().bottom})'));
  await check('Upper-right stamp, compass and zoom controls do not overlap', '(()=>{const s=document.querySelector(".map-stamp").getBoundingClientRect(),c=document.querySelector(".compass-wrap").getBoundingClientRect(),t=document.querySelector(".map-tools").getBoundingClientRect();return s.bottom<c.top && c.bottom<t.top;})()');
  await check('Every UI panel has unique red small ID, including subsections', '(()=>{const els=[...document.querySelectorAll("[data-ui-id]")];return els.length===19 && new Set(els.map(e=>e.dataset.uiId)).size===19 && els.every(e=>{const s=getComputedStyle(e,"::after");const rgb=s.color.match(/[0-9]+/g).map(Number);return s.content.includes(e.dataset.uiId) && parseFloat(s.fontSize)<=9 && rgb[0]>rgb[1] && rgb[0]>rgb[2];});})()');
  await check('Self-contained concept does not reference external scripts or APIs', 'document.querySelectorAll("script[src],img[src^=http],link[href^=http]").length===0 && !document.querySelector("script").textContent.includes("fetch(")');
  await check('10 APP-6 inspired symbols and initial friendly selection remain', 'document.querySelectorAll(".map-unit").length===10 && $("unitName").textContent==="第 3 步兵营"');
  await check('Layers start collapsed and cannot receive focus', '$("layerCard").inert && getComputedStyle($("layerCard")).visibility==="hidden" && $("layerToggle").getAttribute("aria-expanded")==="false"');
  await screenshot('concept-desktop');

  await beginSample('Number(getComputedStyle($("layerCard")).opacity)');
  await click('#layerToggle');
  await endSample('layer-open');
  await check('Layer opening has real intermediate opacity frames', 'motionFrames.some(f=>f.value>0&&f.value<1) && motionFrames.at(-1).value===1');
  await check('Layer card is bottom-right and open state is accessible', '(()=>{const d=$("layerDock").getBoundingClientRect();return innerWidth-d.right<=20 && innerHeight-d.bottom<=18 && !$("layerCard").inert && $("layerToggle").getAttribute("aria-expanded")==="true";})()');
  for (const layer of ['terrain', 'labels', 'front', 'routes']) {
    await click(`[data-layer=${layer}]`);
    await check(`${layer} layer hides`, `$(${JSON.stringify(layer + 'Layer')}).classList.contains('view-hidden')`);
    await click(`[data-layer=${layer}]`);
    await check(`${layer} layer restores`, `!$(${JSON.stringify(layer + 'Layer')}).classList.contains('view-hidden')`);
  }
  await click('[data-layer=labels]');
  await beginSample('Number(getComputedStyle($("layerCard")).opacity)');
  await click('#closeLayers');
  await endSample('layer-close');
  await check('Layer closing animates and returns keyboard focus to toggle', 'motionFrames.some(f=>f.value>0&&f.value<1) && motionFrames.at(-1).value===0 && $("layerCard").inert && document.activeElement.id==="layerToggle"');
  await click('#layerToggle');
  await settle();
  await check('Layer selection survives closing and reopening', '!layerState.labels && document.querySelector("[data-layer=labels]").getAttribute("aria-pressed")==="false"');
  await click('[data-layer=labels]');
  await screenshot('concept-layers-open');
  await evaluate('for(let i=0;i<9;i++)$("layerToggle").click()');
  await settle();
  await check('Rapid toggle settles closed with correct pointer state', '$("layerCard").inert && getComputedStyle($("layerCard")).opacity==="0" && $("layerToggle").getAttribute("aria-expanded")==="false"');

  await evaluate(`window.zoomTimingCases=(()=>{
    const originalRAF=window.requestAnimationFrame,originalCancel=window.cancelAnimationFrame;
    const nowDescriptor=Object.getOwnPropertyDescriptor(performance,'now');
    const before={zoom,zoomTarget};
    cancelAnimationFrame(zoomFrame);
    let scheduled;
    window.requestAnimationFrame=callback=>{scheduled=callback;return 1;};
    window.cancelAnimationFrame=()=>{};
    Object.defineProperty(performance,'now',{configurable:true,value:()=>1000});
    try{
      return [[1,1.09],[1,1/1.09],[.7,.7*1.09],[2.5,2.5/1.09]].map(([start,target])=>{
        zoom=zoomTarget=start;
        animateZoom(target);
        const frames=[zoom];
        for(const timestamp of [984,1000,1016,1040,1080,1160,1240]){scheduled(timestamp);frames.push(zoom);}
        return {start,target,frames};
      });
    }finally{
      window.requestAnimationFrame=originalRAF;
      window.cancelAnimationFrame=originalCancel;
      if(nowDescriptor)Object.defineProperty(performance,'now',nowDescriptor);else delete performance.now;
      zoom=before.zoom;zoomTarget=before.zoomTarget;zoomFrame=0;applyView();
    }
  })();void 0`);
  await check('Early animation timestamps cannot reverse zoom or overshoot limits', 'zoomTimingCases.every(({start,target,frames})=>frames[1]===start && frames.every((v,i)=>v>=.7 && v<=2.5 && (!i || (v-frames[i-1])*Math.sign(target-start)>=-1e-10)) && Math.abs(frames.at(-1)-target)<1e-10)');
  animationReport['early-frame-timestamps']=await evaluate('zoomTimingCases');

  await beginSample('({zoom,width:$("scaleBar").getBoundingClientRect().width,alpha:Number(getComputedStyle($("titleDetails")).opacity),meters:Number($("meterGrid").dataset.meters)})');
  await click('#zoomIn');
  await endSample('zoom-ruler-title');
  await check('Camera smoothly interpolates instead of jumping', 'motionFrames.some(f=>f.value.zoom>1&&f.value.zoom<1.2) && Math.abs(zoom-1.2)<.00001');
  await check('Ruler continuously scales with camera animation', '(()=>{const f=motionFrames.map(x=>x.value);return new Set(f.map(x=>Math.round(x.width*10))).size>3 && f.every(x=>Math.abs(x.width-x.meters*Math.max($("mapSvg").clientWidth/1200,$("mapSvg").clientHeight/880)*x.zoom/2)<1);})()');
  await check('Map title details fade in with zoom', 'motionFrames.at(-1).value.alpha>motionFrames[0].value.alpha && new Set(motionFrames.map(f=>f.value.alpha.toFixed(3))).size>3');
  await check('Title starts significantly smaller than original 31px', 'parseFloat(getComputedStyle($("titleHeading")).fontSize)<=21');
  await beginSample('parseFloat(getComputedStyle($("titleHeading")).fontSize)');
  await click('#titleToggle');
  await endSample('title-compact');
  await check('Click animates title shrink to single compact region name', 'motionFrames.some(f=>f.value>13&&f.value<21) && motionFrames.at(-1).value===13 && $("titleHeading").textContent==="白桦河地区" && $("titleDetails").getBoundingClientRect().height===0 && $("titleToggle").getAttribute("aria-expanded")==="false"');
  await evaluate('for(let i=0;i<24;i++)$("zoomOut").click()');
  await settle();
  await check('Repeated zoom out obeys lower bound and preserves compact title', 'zoom===.7 && zoomTarget===.7 && $("mapTitle").classList.contains("compact")');
  await click('#titleToggle');
  await settle();
  await check('Wide view hides title detail annotations', 'getComputedStyle($("titleDetails")).opacity==="0"');
  await evaluate('for(let i=0;i<24;i++)$("zoomIn").click()');
  await settle();
  await check('Repeated zoom in obeys upper bound and fades details fully in', 'zoom===2.5 && getComputedStyle($("titleDetails")).opacity==="1"');
  await check('Ruler stays in 1/2/5 nice-number series', '(()=>{const n=Number($("meterGrid").dataset.meters);return [1,2,5].includes(n/10**Math.floor(Math.log10(n)));})()');
  await click('#resetView');
  await settle();
  await check('Reset restores original zoom and pan', 'zoom===1 && pan.x===0 && pan.y===0');
  await beginSample('zoom');
  await send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: 950, y: 650, deltaX: 0, deltaY: -120 });
  await endSample('wheel-in-no-reversal');
  await check('Actual wheel zoom-in never moves backward on any sampled frame', 'motionFrames.every((f,i)=>f.value>=1 && (!i || f.value>=motionFrames[i-1].value-1e-10)) && Math.abs(zoom-1.09)<.00001');
  await beginSample('zoom');
  await send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: 950, y: 650, deltaX: 0, deltaY: 120 });
  await endSample('wheel-out-no-reversal');
  await check('Actual wheel zoom-out never moves forward on any sampled frame', 'motionFrames.every((f,i)=>f.value<=1.09+1e-10 && (!i || f.value<=motionFrames[i-1].value+1e-10)) && Math.abs(zoom-1)<.00001');
  await click('#resetView');
  await settle();
  await send('Input.dispatchMouseEvent', { type: 'mousePressed', x: 920, y: 660, button: 'left', clickCount: 1 });
  await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: 965, y: 684, button: 'left', buttons: 1 });
  await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: 965, y: 684, button: 'left', clickCount: 1 });
  await check('Pointer pan works and releases capture', 'pan.x>0 && pan.y>0 && drag===null && !$("viewport").classList.contains("dragging")');
  await click('#resetView');
  await settle();

  await click('.map-unit[data-unit=u1]');
  await check('Map selection updates relocated inspector', '$("unitName").textContent==="第 1 步兵营" && $("unitStrength").textContent==="681"');
  await click('.map-unit[data-unit=e1]');
  await check('Enemy selection keeps strength unknown and friendly analysis hidden', '$("unitStrength").textContent==="—" && $("ammoValue").textContent==="未知" && $("fireButton").disabled && $("fireLayer").classList.contains("view-hidden") && $("routesLayer").classList.contains("view-hidden")');
  await evaluate('document.querySelector(".map-unit[data-unit=u3]").focus()');
  await key('Enter');
  await check('Keyboard selection still updates inspector', '$("unitName").textContent==="第 3 步兵营"');
  await click('#fireButton');
  await check('Fire analysis retains ellipse and split line', 'layerState.fire && !$("fireLayer").classList.contains("view-hidden") && $("dispersion").getAttribute("rx")!==$("dispersion").getAttribute("ry") && $("fireLive").getAttribute("stroke")!==$("fireBlocked").getAttribute("stroke")');
  await check('Toast ID survives dynamic text updates', 'getComputedStyle($("toast"),"::after").content.includes("UI-11")');
  await evaluate('$("toast").classList.remove("visible")');
  await settle();
  await screenshot('concept-fire-control');
  await click('#fireButton');
  await click('[data-view=intelligence]');
  await check('Intelligence uncertainty works without removed records panel', '!$("intelLayer").classList.contains("view-hidden")');
  await click('[data-view=situation]');
  await check('Situation mode hides uncertainty overlay', '$("intelLayer").classList.contains("view-hidden")');
  await click('#planButton');
  await check('Planning remains explicitly nonpersistent', '$("toast").textContent.includes("不会下达")');
  await click('#speed');
  await click('#play');
  await evaluate('new Promise(r=>setTimeout(r,1100))');
  await check('Clock preview still runs without altering unit data', 'clockSeconds>23400 && speed===5 && selected.strength===642');
  await click('#play');
  await check('Clock pauses correctly', '!running');
  await click('#designHelp');
  await check('Design modal has visible red ID and correct new layout description', '$("designDialog").open && $("designDialog").textContent.includes("单位详情集中在左侧") && getComputedStyle($("designDialog"),"::after").content.includes("UI-10")');
  await screenshot('concept-design-notes');
  await key('Escape');
  await check('Escape closes design dialog', '!$("designDialog").open');

  await send('Emulation.setDeviceMetricsOverride', { width: 1100, height: 760, deviceScaleFactor: 1, mobile: false });
  await settle();
  await check('Compact desktop keeps left inspector and wide map', '$("inspector").getBoundingClientRect().left===0 && $("viewport").getBoundingClientRect().width>800 && document.documentElement.scrollWidth===1100');
  await evaluate('$("toast").classList.remove("visible")');
  await settle();
  await screenshot('concept-compact');
  await send('Emulation.setDeviceMetricsOverride', { width: 900, height: 740, deviceScaleFactor: 1, mobile: false });
  await settle();
  await check('Tablet hidden inspector is inaccessible and map uses full width', '$("inspector").inert && $("viewport").getBoundingClientRect().width===900');
  await beginSample('$("inspector").getBoundingClientRect().left');
  await click('#openInspector');
  await endSample('inspector-left-drawer');
  await check('Tablet inspector animates in from the left', 'motionFrames.some(f=>f.value<0&&f.value>-286) && motionFrames.at(-1).value===0 && !$("inspector").inert');
  await key('Escape');
  await settle();
  await check('Escape closes drawer and returns focus', '$("inspector").inert && document.activeElement.id==="openInspector"');

  await send('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 1, mobile: true });
  await settle();
  await check('Phone map fills width with no overflow', 'document.documentElement.scrollWidth===390 && $("viewport").getBoundingClientRect().width===390');
  await check('Phone bottom ruler does not overlap layer toggle or status', '(()=>{const s=$("meterGrid").getBoundingClientRect(),l=$("layerToggle").getBoundingClientRect(),f=document.querySelector(".footer").getBoundingClientRect();return s.right<l.left && s.left>f.right && innerHeight-s.bottom<=14;})()');
  await screenshot('concept-mobile');
  await click('#layerToggle');
  await settle();
  await check('Phone layers stay within viewport, above ruler', '(()=>{const c=$("layerCard").getBoundingClientRect();return c.left>=0 && c.right<=innerWidth && c.bottom<$("meterGrid").getBoundingClientRect().top;})()');
  await screenshot('concept-mobile-layers');
  await click('#closeLayers');
  await settle();
  await click('#openInspector');
  await settle();
  await check('Phone inspector opens left and all actions remain reachable', '$("inspector").getBoundingClientRect().left===0 && $("inspector").scrollHeight>=$("inspector").clientHeight && $("fireButton").offsetTop<$("inspector").scrollHeight');
  await screenshot('concept-mobile-inspector');
  await click('#closeInspector');
  await settle();
  await send('Emulation.setDeviceMetricsOverride', { width: 320, height: 640, deviceScaleFactor: 1, mobile: true });
  await settle();
  await check('Small phone has no horizontal overflow and separate bottom controls', 'document.documentElement.scrollWidth===320 && $("meterGrid").getBoundingClientRect().right<$("layerToggle").getBoundingClientRect().left');
  await screenshot('concept-small-phone');

  await send('Emulation.setDeviceMetricsOverride', { width: 1600, height: 900, deviceScaleFactor: 1, mobile: false });
  await send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
  await settle();
  await click('#zoomIn');
  await check('Reduced motion disables camera interpolation', 'zoom===zoomTarget && zoomFrame===0');
  await click('#layerToggle');
  await check('Reduced motion disables layer transition', 'getComputedStyle($("layerCard")).transitionDuration==="0s" && getComputedStyle($("layerCard")).opacity==="1"');
  await check('Resize back to desktop restores interactive inspector', '!$("inspector").inert && $("inspector").getAttribute("aria-hidden")==="false"');
  await key('Escape');
  await check('Escape dismisses layers', '$("layerCard").inert');
  await click('#resetView');
  await evaluate('clockSeconds=23400;$("clock").textContent="06:30:00";speed=1;$("speed").textContent="1×";$("toast").classList.remove("visible")');
  await send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'no-preference' }] });
  await settle();
  await screenshot('concept-desktop');
  console.log(report.join('\n'));
  console.log(`\n${report.length-failures.length}/${report.length} checks passed; browser errors: ${errors.length}`);
  writeFileSync(`${root}/concept-verification.json`, JSON.stringify({ checks: report, failures, errors, animations: animationReport }, null, 2));
  assert.equal(errors.length, 0, errors.join('\n'));
  if (failures.length) process.exitCode = 1;
} catch (error) {
  console.error(error);
  process.exitCode = 1;
} finally {
  ws?.close();
  chrome.kill();
}
