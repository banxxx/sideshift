export { cn } from "cn";

/** 无障碍偏好：系统要求减少动效时为 true（非组件里读它用这条，组件内用 motion 的 `useReducedMotion`） */
export function prefersReducedMotion(): boolean {
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}
