import { expect, test } from '@playwright/test';

test('GenTactix V2 Frontend 全套 12 项端到端实机交互验收 + UI ID 标牌与 P0/P1 手感', async ({ page }) => {
  const consoleErrors: string[] = [];
  const pageErrors: string[] = [];
  page.on('console', (msg) => {
    if (msg.type() === 'error') consoleErrors.push(msg.text());
  });
  page.on('pageerror', (err) => {
    pageErrors.push(String(err));
  });

  await page.goto('/');
  await expect(page.getByTestId('wave-slip-wave-1')).toBeVisible();

  // 0. 校验全量窗口/控件 UI ID 标牌可见（含 W10/W12），零 display:none 假节点，且 T12 开关可切换显隐
  for (const id of ['W1', 'W2', 'W3', 'W5', 'W5A', 'W6', 'W10', 'W12', 'T2', 'U8', 'U9']) {
    await expect(page.locator(`[data-ui-tag="${id}"]`).first()).toBeVisible();
  }
  const hiddenInlineCount = await page.evaluate(
    () => Array.from(document.querySelectorAll<HTMLElement>('*')).filter((el) => el.style.display === 'none').length,
  );
  expect(hiddenInlineCount).toBe(0);

  await page.getByTestId('toggle-ui-ids').click();
  await expect(page.locator('[data-ui-tag]')).toHaveCount(0);
  await page.getByTestId('toggle-ui-ids').click();
  await expect(page.locator('[data-ui-tag="W1"]').first()).toBeVisible();

  // 0b. 单壳双地图切换验收：默认全屏主地图 field -> 底部 W6/MAP.SWITCH 切副图 tactical -> W10 图例内切回 field
  const viewportEl = page.getByTestId('nine-layer-viewport');
  await expect(viewportEl).toHaveAttribute('data-map-view', 'field');
  await page.getByTestId('toggle-map-view').click();
  await expect(viewportEl).toHaveAttribute('data-map-view', 'tactical');
  await page.getByTestId('legend-toggle').click();
  await expect(page.getByTestId('legend-switch-map')).toBeVisible();
  await page.getByTestId('legend-switch-map').click();
  await expect(viewportEl).toHaveAttribute('data-map-view', 'field');
  await page.getByTestId('legend-toggle').click();

  // 0c. 验证 W5 纯排纸条与 W12 独立编写器分离，并校验视野缩放 (M1) 与右侧手写板折叠/展开 (R3)
  await expect(page.getByTestId('directive-composer')).toBeVisible();
  await expect(page.getByTestId('uio-panel').getByTestId('btn-create-draft')).toHaveCount(0);
  await page.getByTestId('zoom-in').click();
  await expect(page.getByTestId('zoom-level')).toContainText('120%');
  await page.getByTestId('zoom-reset').click();
  await expect(page.getByTestId('zoom-level')).toContainText('100%');
  await page.getByTestId('btn-collapse-right').click();
  await expect(page.getByTestId('uio-panel')).toBeHidden();
  await page.getByTestId('btn-collapse-right').click();
  await expect(page.getByTestId('uio-panel')).toBeVisible();

  // 1. 9 个物理图层容器 data-layer="1..9" 挂载且 z-index 严格递增
  const zOrder = await page.evaluate(() => {
    const out: number[] = [];
    for (let i = 1; i <= 9; i++) {
      const el = document.querySelector<HTMLElement>(`[data-layer="${i}"]`);
      if (!el) throw new Error(`missing data-layer=${i}`);
      out.push(Number(getComputedStyle(el).zIndex));
    }
    return out;
  });
  expect(zOrder).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);

  // 1b. 点击主地图上的己方排部兵牌 -> 弹出 W3 单位编制卡片并可点开士兵详情与个人物品栏
  const stageBox = (await page.getByTestId('map-stage').boundingBox())!;
  const baseScale = Math.min(stageBox.width / 640, stageBox.height / 480);
  const originX = (stageBox.width - 640 * baseScale) / 2;
  const originY = (stageBox.height - 480 * baseScale) / 2;
  await page.mouse.click(stageBox.x + originX + 160 * baseScale, stageBox.y + originY + 240 * baseScale);
  await expect(page.getByTestId('unit-card-modal')).toBeVisible();
  await page.getByTestId('unit-card-modal').locator('[data-testid^="soldier-row-"]').first().click();
  await expect(page.getByTestId('unit-card-modal').locator('[data-testid^="soldier-detail-"]')).toBeVisible();
  await expect(page.getByTestId('unit-card-modal').locator('[data-testid^="soldier-inventory-"]')).toBeVisible();
  await page.getByTestId('unit-card-close').click();
  await expect(page.getByTestId('unit-card-modal')).toHaveCount(0);

  // 2. W12 生成无头指令写入 W5 草稿栏；未绑定移入任务队列被拦截并留在草稿栏
  await page.getByTestId('btn-create-draft').click();
  await page.getByTestId('send-to-queue-dir-1').click();
  await expect(page.getByTestId('uio-gate-error')).toBeVisible();
  await expect(page.getByTestId('draft-card-dir-1')).toBeVisible();

  // 3. 绑定远程 3 班时显示明确中文远程延迟偏移文案；同时使用逐卡就地拾取绑定目标（自动切副图标绘）
  await page.getByTestId('bind-unit-dir-1').selectOption('4'); // 3 班 id=4
  await expect(page.getByTestId('remote-delay-4')).toContainText('远程指挥：超出直接指挥半径');

  await page.getByTestId('pick-for-dir-1').click();
  await page.mouse.click(stageBox.x + stageBox.width * 0.56, stageBox.y + stageBox.height * 0.52);
  await page.getByTestId('finish-poly-dir-1').click();
  await page.getByTestId('send-to-queue-dir-1').click();
  await expect(page.getByTestId('queue-card-dir-1')).toBeVisible();

  // 4. 再建一条同单位(3班)时间重叠纸条 -> 时间轴条 U10 标红 + 锁死签发，随后内联改时间消除冲突
  await page.getByTestId('btn-create-draft').click();
  await page.getByTestId('bind-unit-dir-2').selectOption('4');
  await page.getByTestId('bind-target-dir-2').click();
  await page.getByTestId('send-to-queue-dir-2').click();
  await expect(page.getByTestId('queue-timeline')).toBeVisible();
  await expect(page.getByTestId('timeline-bar-dir-2')).toHaveClass(/conflict/);
  await expect(page.getByTestId('conflict-alert-dir-1')).toBeVisible();
  await expect(page.getByTestId('btn-issue-queue')).toBeDisabled();

  // 内联修改 dir-2 起止时间到 [12..18] 实时解除冲突后再退回草稿
  await page.getByTestId('queue-start-dir-2').fill('12');
  await page.getByTestId('queue-end-dir-2').fill('18');
  await expect(page.getByTestId('btn-issue-queue')).toBeEnabled();
  await page.getByTestId('queue-card-dir-2').getByText('退回草稿').click();

  // 给排长(id=1)建一条短程移动纸条（0..4s），验证排长纸条移动
  await page.getByTestId('input-start-sec').fill('0');
  await page.getByTestId('input-end-sec').fill('4');
  await page.getByTestId('btn-create-draft').click();
  await page.getByTestId('bind-unit-dir-3').selectOption('1'); // 排部 Platoon HQ
  await page.getByTestId('btn-pick-target').click();
  await page.mouse.click(stageBox.x + stageBox.width * 0.34, stageBox.y + stageBox.height * 0.5);
  await page.getByTestId('bind-target-dir-3').click();
  await page.getByTestId('send-to-queue-dir-3').click();

  // 5. 双重签发后不可撤销并锁死
  await page.getByTestId('btn-issue-queue').click();
  await expect(page.getByTestId('locked-card-dir-1')).toBeVisible();
  await expect(page.getByTestId('locked-card-dir-3')).toBeVisible();

  // 7 & 8. 切换倍速 T4 + 推进一拍并校验 16s 走表防剧透 + 指令完成进归档栏并可回炉重签
  await page.getByTestId('select-playback-speed').selectOption('4');
  await page.getByTestId('btn-next-tick').click();
  await page.getByTestId('select-unit-1').click();
  await page.getByTestId('scrub-ms').fill('0');
  const posAt0 = await page.getByTestId('inspector-pos').textContent();
  await page.getByTestId('scrub-ms').fill('16000');
  await page.getByTestId('btn-next-tick').click();
  await page.getByTestId('scrub-ms').fill('16000');
  const posAtEnd = await page.getByTestId('inspector-pos').textContent();
  expect(posAtEnd).not.toEqual(posAt0);

  await expect(page.getByTestId('archive-card-dir-3')).toBeVisible();
  await page.getByTestId('requeue-dir-3').click();
  await expect(page.getByTestId('queue-card-dir-3')).toBeVisible();
  await page.getByTestId('btn-issue-queue').click();
  await expect(page.getByTestId('locked-card-dir-3')).toBeVisible();

  // 6. 点击未来书签 #4 (T8) -> 自动跳至 #3 并展示 #4 定时炸药前向预警；进 #4 到达 8s 准时爆破留弹坑
  await page.getByTestId('bookmark-tick-4').click();
  await expect(page.getByTestId('current-tick')).toContainText('#3');
  await expect(page.getByTestId('lookahead-banner')).toContainText('#4 定时炸药即将爆炸');

  await page.getByTestId('btn-next-tick').click();
  await expect(page.getByTestId('current-tick')).toContainText('#4');
  await page.getByTestId('scrub-ms').fill('0');
  const l2BytesBefore = await page.evaluate(() => {
    const c = document.querySelector<HTMLCanvasElement>('[data-layer="2"] canvas')!;
    const data = c.getContext('2d')!.getImageData(0, 0, c.width, c.height).data;
    let nonZero = 0;
    for (let i = 3; i < data.length; i += 4) if (data[i]! > 0) nonZero++;
    return nonZero;
  });
  await page.getByTestId('scrub-ms').fill('8000');
  await expect(page.getByTestId('playback-log')).toContainText('定时炸药起爆');
  const l2BytesAfter = await page.evaluate(() => {
    const c = document.querySelector<HTMLCanvasElement>('[data-layer="2"] canvas')!;
    const data = c.getContext('2d')!.getImageData(0, 0, c.width, c.height).data;
    let nonZero = 0;
    for (let i = 3; i < data.length; i += 4) if (data[i]! > 0) nonZero++;
    return nonZero;
  });
  expect(l2BytesAfter).toBeGreaterThan(l2BytesBefore);

  // 9. 点击顶栏回形针波次纸条 + 独立【指挥 UI】拖圆+橡皮筋+撤销+签发波次
  await page.getByTestId('wave-slip-wave-1').click();
  await page.getByTestId('tab-commander-wave').click();
  await page.getByTestId('wave-tool-circle').click();
  await page.mouse.move(stageBox.x + stageBox.width * 0.52, stageBox.y + stageBox.height * 0.38);
  await page.mouse.down();
  await page.mouse.move(stageBox.x + stageBox.width * 0.56, stageBox.y + stageBox.height * 0.38);
  await page.mouse.up();
  await page.getByTestId('wave-tool-arrow').click();
  await page.mouse.click(stageBox.x + stageBox.width * 0.4, stageBox.y + stageBox.height * 0.55);
  await page.mouse.click(stageBox.x + stageBox.width * 0.48, stageBox.y + stageBox.height * 0.48);
  await page.mouse.click(stageBox.x + stageBox.width * 0.55, stageBox.y + stageBox.height * 0.42);
  await page.getByTestId('wave-undo-point').click();
  await page.getByTestId('wave-commit-shape').click();
  await page.getByTestId('wave-tool-ao').click();
  await page.mouse.click(stageBox.x + stageBox.width * 0.34, stageBox.y + stageBox.height * 0.28);
  await page.mouse.click(stageBox.x + stageBox.width * 0.62, stageBox.y + stageBox.height * 0.66);
  await page.getByTestId('btn-issue-wave').click();
  await expect(page.getByTestId('wave-slip-wave-3')).toBeVisible();

  // 10. Layer 6 手绘一笔后执行真实 page.reload()（等效 F5），验证手绘线条完好无损保留
  await page.getByTestId('l6-tool-pen').click();
  await page.mouse.move(stageBox.x + 420, stageBox.y + 260);
  await page.mouse.down();
  await page.mouse.move(stageBox.x + 520, stageBox.y + 320);
  await page.mouse.move(stageBox.x + 580, stageBox.y + 280);
  await page.mouse.up();
  await expect(page.getByTestId('l6-count')).toContainText('笔画 1');

  await page.reload();
  await expect(page.getByTestId('l6-count')).toContainText('笔画 1');
  const l6NonZeroAfterReload = await page.evaluate(() => {
    const c = document.querySelector<HTMLCanvasElement>('[data-layer="6"] canvas')!;
    const data = c.getContext('2d')!.getImageData(0, 0, c.width, c.height).data;
    let nonZero = 0;
    for (let i = 3; i < data.length; i += 4) if (data[i]! > 0) nonZero++;
    return nonZero;
  });
  expect(l6NonZeroAfterReload).toBeGreaterThan(0);
  // 主地图默认清爽隐藏 L6 手绘，切到副地图时透明还原展示
  await expect(page.locator('[data-layer="6"]')).toHaveCSS('opacity', '0');
  await page.getByTestId('toggle-map-view').click();
  await expect(viewportEl).toHaveAttribute('data-map-view', 'tactical');
  await expect(page.locator('[data-layer="6"]')).toHaveCSS('opacity', '1');

  // 11. 点击 Layer 7 与 Layer 9 开关按钮，图层渲染真实启停变化
  await page.getByTestId('toggle-l7').click();
  await expect(page.locator('[data-layer="7"]')).toHaveAttribute('data-enabled', 'false');
  await page.getByTestId('toggle-l9').click();
  await expect(page.locator('[data-layer="9"]')).toHaveAttribute('data-enabled', 'false');

  // 12. 全程控制台 console.error 与 pageerror 为 0
  expect(consoleErrors).toEqual([]);
  expect(pageErrors).toEqual([]);
});
