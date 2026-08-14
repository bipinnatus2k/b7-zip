# recomp

recomp 是一个面向前端开发的组件复刻技能（支持 Claude、Codex 等主流 AI agents），它解决了组件库框架（例如 React）和你项目开发框架（例如 SolidJS）不一致的问题：指导 AI 阅读组件库文档，把组件的交互和行为模式复刻成适配你框架的 headless 组件源码和使用示范——所有产出直接展示在对话里，复制即用。

> English version: [README.md](./README.md)

## 它能做什么

把组件库文档页的链接复制下来（shadcn/ui、Radix、MUI、Ant Design 等），发给 recomp，它会：

1. **验证页面**确实是组件库文档页——博客、工具库、设计稿一律拒绝，并给出原因
2. **只读探测你的工程**——框架+版本、CSS 方案、设计 token（DESIGN.md、`:root` CSS 变量、`@theme`、stylex tokens）——不建目录、不改文件
3. **产出**：
   - **行为契约摘要**——哪些来自文档、哪些按 WAI-ARIA APG 补全、哪些留给你拍板
   - **headless 组件源码**（SolidJS / Vue / Svelte），完整文件 + 依赖清单
   - **使用说明**（示例 + API 摘要）
   - **样式接口契约**——data-attribute、class 插槽、CSS 变量，按你的 CSS 方案定制（Tailwind 走 className 透传、StyleX 走 style props）
   - **免安装**——交付的是纯源码文件，不是 npm 包：不进 node_modules、不改构建配置、没有依赖树要维护。组件从此是你自己的代码，而不是一个需要跟进升级的依赖

样式和设计 token 完全由你掌控：把源码复制进你的组件目录，接上样式即可。
不满意某处行为？直接说，我重出整个文件。

## 支持的技术栈

recomp 可以把组件库文档复刻成以下任意技术栈的实现——发什么文档 URL，就复刻什么：

| 类别 | 技术栈 | 说明 |
|---|---|---|
| Web | SolidJS / Vue / Svelte——任意 CSS 方案（Tailwind、StyleX、CSS-in-JS、原生） | 主场：行为 + 接口按框架惯用法适配 |
| 原生 | Flutter / SwiftUI / Jetpack Compose / Qt | 样式接口契约按平台翻译（ThemeExtension / Environment / Modifier / QML 钩子） |
| 小程序 | 原生（微信等，Component() + WXML）· Taro · uni-app | 原生走 properties / externalClasses / triggerEvent；Taro、uni-app 按其 Web 底座（React/Vue）处理 |
| 桌面壳层 | Electron / Tauri | UI 层就是 Web 技术——按 Web 栈处理，组件在渲染层 |

无障碍依据按平台取源：Web 用 ARIA / WAI-ARIA APG，原生用平台 API（Flutter Semantics、SwiftUI accessibility 修饰符、Compose semantics、Qt QAccessible），小程序用 aria-* 属性。

## 安装

**方式一：一行命令（推荐）**——两个仓库源都可以：

```sh
npx skills add brickhu/recomp          # 独立仓库
npx skills add brickhu/skills/recomp   # skills 仓库（路径形式）
```

**方式二：手动复制**（适用于任何能加载 SKILL.md 的 AI 助手）

```sh
git clone https://github.com/brickhu/recomp.git
cp -r recomp ~/.agents/skills/recomp
```

纯提示词驱动：无依赖、无构建步骤。**安装后请重启你的 agent**，技能才会被加载。

## 使用

1. 复制组件库文档链接，发给 recomp：`recomp <url>`
2. （可选）指定项目根目录以便探测：`recomp <url> --project ~/my-app`——不指定则回退到当前工作目录
3. 逐个确认开放问题（回复 `yes` / 确认，或直接输入你的方案）——最后一个确认完即开始生成
4. 复制源码，接上你的样式
5. 对话迭代：*"escape 处理不对"*、*"这里用 `bind:` 不要用 `v-model`"*——每次修订都输出完整文件

### 完整例子

以"给 SolidJS + StyleX 项目复刻 shadcn/ui 的 Button"为例，完整流程是这样：

打开你的 AI 编程助手（Claude Code、Codex、Cursor、Zed 都行），新建对话，输入：

```text
recomp https://ui.shadcn.com/docs/components/button，项目在 ~/my-app（SolidJS + StyleX）

# 如果你的对话在项目工程目录下可以直接输入：
recomp https://ui.shadcn.com/docs/components/button
```

它先回你一份**行为契约摘要**：

```text
✅ 验证通过：是 shadcn/ui 的 Button 组件文档
---
接口：variant / size / disabled / loading → SolidJS props
文档明确的行为：disabled 拦截点击 · loading 时禁用
依赖：无，纯原生实现
---
文档没写清楚的 1 处行为，需要你拍板：

第 1/1 处：loading 时按钮尺寸是否保持不变？
建议：保持尺寸并暴露 data-state="loading"（避免布局跳动，样式层有状态可接）
回复 yes / 确认 同意，或直接输入你的方案
```

你回复 yes——开放问题确认完毕，它直接按文件输出**完整源码**（节选）：

```tsx
// src/Button.tsx（节选）
import { splitProps, type JSX } from "solid-js";

type Props = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  loading?: boolean;
};

export function Button(props: Props) {
  const [local, rest] = splitProps(props, ["disabled", "loading", "onClick", "children"]);

  return (
    <button
      {...rest}
      disabled={local.disabled || local.loading}
      data-state={local.loading ? "loading" : "idle"}
      aria-busy={local.loading}
      onClick={(e) => {
        if (local.disabled || local.loading) return;
        local.onClick?.(e);
      }}
    >
      {local.loading && <span aria-hidden="true">…</span>}
      {local.children}
    </button>
  );
}
```

以及**使用示范**：

```tsx
import { createSignal } from "solid-js";
import { Button } from "./src/Button";

function App() {
  const [loading, setLoading] = createSignal(false);

  return (
    <Button
      loading={loading()}
      class="submit" // class 就是样式入口，接你的设计 token
      onClick={() => {
        setLoading(true);
        submit();
      }}
    >
      提交
    </Button>
  );
}
```

复制源码、接上你的样式，完事。不满意某处行为？直接在对话里说——它当场重出完整文件，不用打补丁。

## 它会拒绝什么

- **非组件库页面**（博客、工具库、设计稿）——拒绝并说明原因
- **猜测无障碍语义**（ARIA / 焦点 / 键盘行为）——按 WAI-ARIA APG 补全，或明确标注"未实现"
- **写入你的工程**——探测只读，产出全在对话里
- **让你去装第三方库**——你生态里已有等价实现（Solid 的 Kobalte、Vue 的 Reka UI、Svelte 的 Bits UI）时，它只把它们当参考来学习，**依然交付你自己可复制的代码和使用指南**——组件库始终握在你手里

## 文件结构

```
recomp/
├── SKILL.md                     # 技能定义（入口）
├── README.md                    # English guide
├── README_cn.md                 # 本指南
└── references/
    ├── verification.md          # URL 验证清单与拒绝情形
    ├── probing.md               # 只读工程探测的信号与置信度
    └── deliverables.md          # 交付物模板与决策分类
```

## License

MIT
