// 启动页收场：index.html 里内联的「体素」动画由这里摘掉。
// 口径：不等数据，只等 React 首屏真的画上去（main.tsx 里两帧之后调用）。

/** 四条 chunk 落定的时刻：首条延迟 240ms + 入场 900ms + 错峰 80ms×3。
 *  快机器上包 600ms 就就绪，不压这段的话会看见动画被腰斩；设成 0 即「就绪立刻收」。 */
const MIN_SHOW_MS = 1380;

/** 与 index.html 里 .sp-out 的两段退场（33ms + 27ms）对齐，留 40ms 余量 */
const EXIT_MS = 100;

let dismissing = false;

/** 启动页真的不挡屏了的那一刻（收场动画走完）。
 *  冷启动要挂全局提示的调用方等这个，别自己抄一份 `MIN_SHOW_MS`：提示卡的停留时间是挂载那一拍
 *  就开始走的，遮罩底下走完大半的话，用户看到的是一个正在消失、甚至已经消失的卡 */
let settle: () => void = () => {};
export const splashGone = new Promise<void>((resolve) => {
  settle = resolve;
});

export function dismissSplash() {
  if (dismissing) return;
  dismissing = true;

  const el = document.getElementById("splash");
  if (!el) {
    settle();
    return;
  }

  const hold = Math.max(0, MIN_SHOW_MS - performance.now());
  window.setTimeout(() => {
    el.classList.add("sp-out");
    window.setTimeout(() => {
      el.remove();
      settle();
    }, EXIT_MS);
  }, hold);
}
