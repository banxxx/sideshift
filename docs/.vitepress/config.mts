import { defineConfig } from "vitepress";

// 文档站三语结构：/ 为简体中文（默认），/en/ 与 /zh-tw/ 为对应语言目录。
// 每个语言一份 nav + sidebar；默认主题的界面文字（「本页目录」等）由 lang 自动带出。

const REPO = "https://github.com/banxxx/sideshift";

interface Pack {
    label: string;
    lang: string;
    title: string;
    description: string;
    nav: { text: string; link: string }[];
    sidebar: { text: string; items: { text: string; link: string }[] }[];
}

const zh: Pack = {
    label: "简体中文",
    lang: "zh-CN",
    title: "SideShift",
    description: "把 CurseForge / Modrinth 整合包转换成开箱即用的服务端",
    nav: [
        { text: "指南", link: "/guide/intro" },
        { text: "维护", link: "/maintain/release" },
        { text: "GitHub", link: REPO },
    ],
    sidebar: [
        {
            text: "指南",
            items: [
                { text: "介绍", link: "/guide/intro" },
                { text: "快速开始", link: "/guide/quick-start" },
                { text: "自动分类", link: "/guide/classification" },
                { text: "设置说明", link: "/guide/settings" },
                { text: "应用更新", link: "/guide/update" },
            ],
        },
        {
            text: "维护",
            items: [
                { text: "发版手册", link: "/maintain/release" },
                { text: "本地开发", link: "/maintain/dev" },
            ],
        },
    ],
};

const en: Pack = {
    label: "English",
    lang: "en-US",
    title: "SideShift",
    description: "Turn CurseForge / Modrinth modpacks into ready-to-run servers",
    nav: [
        { text: "Guide", link: "/en/guide/intro" },
        { text: "Maintenance", link: "/en/maintain/release" },
        { text: "GitHub", link: REPO },
    ],
    sidebar: [
        {
            text: "Guide",
            items: [
                { text: "Introduction", link: "/en/guide/intro" },
                { text: "Quick Start", link: "/en/guide/quick-start" },
                { text: "Auto Classification", link: "/en/guide/classification" },
                { text: "Settings", link: "/en/guide/settings" },
                { text: "App Updates", link: "/en/guide/update" },
            ],
        },
        {
            text: "Maintenance",
            items: [
                { text: "Release Guide", link: "/en/maintain/release" },
                { text: "Local Development", link: "/en/maintain/dev" },
            ],
        },
    ],
};

const tw: Pack = {
    label: "繁體中文",
    lang: "zh-TW",
    title: "SideShift",
    description: "把 CurseForge / Modrinth 整合包轉換成開箱即用的伺服器",
    nav: [
        { text: "指南", link: "/zh-tw/guide/intro" },
        { text: "維護", link: "/zh-tw/maintain/release" },
        { text: "GitHub", link: REPO },
    ],
    sidebar: [
        {
            text: "指南",
            items: [
                { text: "介紹", link: "/zh-tw/guide/intro" },
                { text: "快速開始", link: "/zh-tw/guide/quick-start" },
                { text: "自動分類", link: "/zh-tw/guide/classification" },
                { text: "設定說明", link: "/zh-tw/guide/settings" },
                { text: "應用程式更新", link: "/zh-tw/guide/update" },
            ],
        },
        {
            text: "維護",
            items: [
                { text: "發版手冊", link: "/zh-tw/maintain/release" },
                { text: "本地開發", link: "/zh-tw/maintain/dev" },
            ],
        },
    ],
};

const locale = (p: Pack) => ({
    label: p.label,
    lang: p.lang,
    title: p.title,
    description: p.description,
    themeConfig: {
        nav: p.nav,
        sidebar: p.sidebar,
    },
});

export default defineConfig({
    title: "SideShift",
    description: zh.description,
    base: "/sideshift/",
    locales: {
        root: locale(zh),
        en: { ...locale(en), link: "/en/" },
        "zh-tw": { ...locale(tw), link: "/zh-tw/" },
    },
    themeConfig: {
        // 本地搜索：三语的页面标题与正文都能搜到
        search: { provider: "local" },
        socialLinks: [{ icon: "github", link: REPO }],
    },
});
