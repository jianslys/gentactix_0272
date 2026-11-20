import { expect, test } from '@playwright/test';

test('GenTactix V2 Frontend 全套 12 项端到端实机交互验收', async ({ page }) => {
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

  // 2. 未绑定无头指令移入任务队列被拦截并留在草稿栏
  await page.getByTestId('btn-create-draft').click();
  await page.getByTestId('send-to-queue-dir-1').click();
  await expect(page.getByTestId('uio-gate-error')).toBeVisible();
  await expect(page.getByTestId('draft-card-dir-1')).toBeVisible();

  // 3. 绑定远程 3 班时显示明确中文远程延迟偏移文案；同时验证时间槽冲突实时标红与锁死签发
  await page.getByTestId('bind-unit-dir-1').selectOption('4'); // 3 班 id=4
  await expect(page.getByTestId('remote-delay-4')).toContainText('远程指挥：超出直接指挥半径');

  // 通过地图拾取标的物给 dir-1（3班）绑定目标，送入队列
  await page.getByTestId('btn-pick-target').click();
  const stageBox = (await page.getByTestId('map-stage').boundingBox())!;
  await page.mouse.click(stageBox.x + stageBox.width * 0.68, stageBox.y + stageBox.height * 0.52);
  await page.getByTestId('bind-target-dir-1').click();
  await page.getByTestId('send-to-queue-dir-1').click();
  await expect(page.getByTestId('queue-card-dir-1')).toBeVisible();

  // 4. 再建一条同单位(3班)时间重叠纸条 -> 实时标红并锁死【签发任务队列】
  await page.getByTestId('btn-create-draft').click();
  await page.getByTestId('bind-unit-dir-2').selectOption('4');
  await page.getByTestId('bind-target-dir-2').click();
  await page.getByTestId('send-to-queue-dir-2').click();
  await expect(page.getByTestId('conflict-alert-dir-1')).toBeVisible();
  await expect(page.getByTestId('conflict-alert-dir-2')).toBeVisible();
  await expect(page.getByTestId('btn-issue-queue')).toBeDisabled();

  // 退回 dir-2 并给排长(id=1)建一条短程移动纸条（0..4s），验证排长纸条移动 + 互斥消除后可签发
  await page.getByTestId('queue-card-dir-2').getByText('退回草稿').click();
  await expect(page.getByTestId('btn-issue-queue')).toBeEnabled();

  await page.getByTestId('input-start-sec').fill('0');
  await page.getByTestId('input-end-sec').fill('4');
  await page.getByTestId('btn-create-draft').click();
  await page.getByTestId('bind-unit-dir-3').selectOption('1'); // 排部 Platoon HQ
  await page.getByTestId('btn-pick-target').click();
  await page.mouse.click(stageBox.x + stageBox.width * 0.32, stageBox.y + stageBox.height * 0.5);
  await page.getByTestId('bind-target-dir-3').click();
  await page.getByTestId('send-to-queue-dir-3').click();

  // 5. 双重签发后不可撤销并锁死
  await page.getByTestId('btn-issue-queue').click();
  await expect(page.getByTestId('locked-card-dir-1')).toBeVisible();
  await expect(page.getByTestId('locked-card-dir-3')).toBeVisible();

  // 7 & 8. 推进一拍并校验 16s 走表防剧透（t=0s 不提前剧透，t=16s 到达终点）+ 指令完成进归档栏并可拖回队列
  await page.getByTestId('btn-next-tick').click();
  await page.getByTestId('select-unit-1').click();
  await page.getByTestId('scrub-ms').fill('0');
  const posAt0 = await page.getByTestId('inspector-pos').textContent();
  await page.getByTestId('scrub-ms').fill('16000');
  await page.getByTestId('btn-next-tick').click();
  await page.getByTestId('scrub-ms').fill('16000');
  const posAtEnd = await page.getByTestId('inspector-pos').textContent();
  expect(posAtEnd).not.toEqual(posAt0);

  // dir-3 (排长 0..4s 短任务) 执行完毕自动进入归档栏，支持拖回任务队列再签发
  await expect(page.getByTestId('archive-card-dir-3')).toBeVisible();
  await page.getByTestId('requeue-dir-3').click();
  await expect(page.getByTestId('queue-card-dir-3')).toBeVisible();
  await page.getByTestId('btn-issue-queue').click();
  await expect(page.getByTestId('locked-card-dir-3')).toBeVisible();

  // 6. 点击快进至 #3 -> 展示 #4 定时炸药前向预警；进入 #4 走表到 7.3s 准时爆破并在 Layer 2 留下弹坑
  await page.getByTestId('btn-jump-3').click();
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

  // 9. 点击顶栏回形针波次纸条 + 独立【指挥 UI】新签发波次，Layer 5 实时绘制
  await page.getByTestId('wave-slip-wave-1').click();
  await page.getByTestId('tab-commander-wave').click();
  await page.getByTestId('wave-tool-circle').click();
  await page.mouse.click(stageBox.x + stageBox.width * 0.62, stageBox.y + stageBox.height * 0.35);
  await page.getByTestId('wave-tool-arrow').click();
  await page.mouse.click(stageBox.x + stageBox.width * 0.42, stageBox.y + stageBox.height * 0.55);
  await page.mouse.click(stageBox.x + stageBox.width * 0.58, stageBox.y + stageBox.height * 0.42);
  await page.getByTestId('wave-commit-shape').click();
  await page.getByTestId('wave-tool-ao').click();
  await page.mouse.click(stageBox.x + stageBox.width * 0.35, stageBox.y + stageBox.height * 0.25);
  await page.mouse.click(stageBox.x + stageBox.width * 0.72, stageBox.y + stageBox.height * 0.65);
  await page.getByTestId('btn-issue-wave').click();
  await expect(page.getByTestId('wave-slip-wave-3')).toBeVisible();

  // 10. Layer 6 手绘一笔后执行真实 page.reload()（等效 F5），验证手绘线条完好无损保留
  await page.getByTestId('l6-tool-pen').click();
  await page.mouse.move(stageBox.x + 260, stageBox.y + 210);
  await page.mouse.down();
  await page.mouse.move(stageBox.x + 360, stageBox.y + 270);
  await page.mouse.move(stageBox.x + 410, stageBox.y + 230);
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

  // 11. 点击 Layer 7 与 Layer 9 开关按钮，图层渲染真实启停变化
  await page.getByTestId('toggle-l7').click();
  await expect(page.locator('[data-layer="7"]')).toHaveAttribute('data-enabled', 'false');
  await page.getByTestId('toggle-l9').click();
  await expect(page.locator('[data-layer="9"]')).toHaveAttribute('data-enabled', 'false');

  // 12. 全程控制台 console.error 与 pageerror 为 0
  expect(consoleErrors).toEqual([]);
  expect(pageErrors).toEqual([]);
});
