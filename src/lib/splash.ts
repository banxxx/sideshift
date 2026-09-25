// 启动页收场：index.html 里内联的「体素」动画由这里摘掉。
// 口径：不等数据，只等 React 首屏真的画上去（main.tsx 里两帧之后调用）。

/** 四条 chunk 落定的时刻：首条延迟 240ms + 入场 900ms + 错峰 80ms×3。
 *  快机器上包 600ms 就就绪，不压这段的话会看见动画被腰斩；设成 0 即「就绪立刻收」。 */
const MIN_SHOW_MS = 1380;

/** 与 index.html 里 .sp-out 的两段退场（33ms + 27ms）对齐，留 40ms 余量 */
const EXIT_MS = 100;

let dismissing = false;

export function dismissSplash() {
  if (dismissing) return;
  dismissing = true;

  const el = document.getElementById("splash");
  if (!el) return;

  const hold = Math.max(0, MIN_SHOW_MS - performance.now());
  window.setTimeout(() => {
    el.classList.add("sp-out");
    window.setTimeout(() => el.remove(), EXIT_MS);
  }, hold);
}
