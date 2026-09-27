/**
 * 鸣谢名单的唯一数据源（关于页）。
 *
 * **手工维护、不打网络**：关于页自己那句「只有两种情形会主动联网」必须是真话，
 * 所以这里既不查 GitHub 的贡献者接口，也不引 `avatars.githubusercontent.com` 的图。
 * 要放真人头像，就把切好的图放进 `public/about/` 再在条目上引（图形单源那条规矩同样适用：
 * 一处文件、一处引用，不许在组件里自绘）。
 *
 * 数组顺序即屏上顺序（密排卡从左到右、从上到下）。
 */
export interface Contributor {
    /** 稳定标识：只当 React key 用，绝不显示、绝不翻译 */
    id: string;
    /** 显示名：专有名词，三档语言下都按原样显示 */
    name: string;
    /** 本地头像（相对 public 的路径，如 "/about/teal.png"）；没有就落首字母块 */
    avatar?: string;
}

export const CONTRIBUTORS: Contributor[] = [
    { id: "banxxx", name: "Banxxx" },
];

/**
 * 首字母兜底：没有头像时块上放的字符。
 *
 * 汉字取首字（「张三」→「张」），拉丁取词首字母最多两位（"Banxxx Ng"→"BN"）。
 * 拆词只用 ASCII 的空白与连字符类分隔符，别把全角空格当分隔符——那不是空白。
 */
export function initialsOf(name: string): string {
    const s = name.trim();
    if (!s) return "?";
    const first = Array.from(s)[0];
    if (/[㐀-鿿]/.test(first)) return first;
    const words = s.split(/[\s._\-']+/, 3).filter(Boolean);
    if (words.length >= 2) return (words[0][0] + words[1][0]).toUpperCase();
    return s.slice(0, 2).toUpperCase();
}
