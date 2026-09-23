/// <reference types="vite/client" />

/** 构建期注入的包版本号（源：package.json 的 version，见 vite.config.ts 的 define） */
declare const __APP_VERSION__: string;
