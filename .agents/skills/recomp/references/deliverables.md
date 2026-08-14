# Deliverables

## Two phases

1. **Before replicating (confirmation gate — mandatory)** — validation
   result + behavior contract summary presented as an option list. The user
   picks 1/2/3/4; nothing is generated until they do.
2. **After confirmation (delivery)** — present in user-priority order, lean:

   1. Opening line + **source code** (top priority)
   2. **Usage** — minimal working example + API summary
   3. **Interface standard** — props/events/slots/types + style interface contract
   4. (optional, last) acceptance script

Open every delivery with:

> ✅ 组件已复刻成功，复制以下组件代码到你的项目目录即可使用。

Do not repeat the behavior contract summary in the delivery.

## Confirmation gate (phase 1)

Behavior contract summary template:

- **Component**: name, source URL, fetch date
- **Interface**: props / events / slots; controlled/uncontrolled pattern in the
  target framework (see idiom mapping below)
- **Behavior**: states, keyboard interaction, focus management, ARIA, edge
  cases — only what the docs state explicitly
- **Dependencies**: name + version + known conflicts in the user's project

Anything the docs do NOT state explicitly is collected as **open questions**,
each with a proposed approach (APG standard / ecosystem reference). Present
them **one by one** and confirm each before moving on — confirming the last
question ends the gate. **Never generate code before that.** Template:

> ✅ 验证通过：<library> 的 <component> 文档页
> ---
> 接口：<props/events/slots 及目标框架惯用法>
> 行为：<文档明确的行为清单>
> 依赖：<依赖及版本>
> ---
> 复刻目标：<framework>（web 技术栈附带 CSS 方案；非 web 用平台样式系统；小程序用 WXSS/主题）
> 文档未明确的行为有 N 处，我们逐个确认：
>
> 第 1/N 处：<问题>
> 建议：<方案（web：按 APG 规范；非 web：平台无障碍惯例 / 参考生态实现）>
> 回复 yes / 确认 采纳建议，或直接输入你的自定义方案

Single option → reply yes / 确认. Numbers are only used when multiple
choices exist.

Confirm each question in turn. After the last one is confirmed, go straight
to delivery (phase 2) — no extra confirmation round.

The contract summary and the open questions live ONLY in this gate — never in
the final delivery.

Never guess ARIA roles, focus management, or keyboard semantics — they become
open questions with proposed approaches, never invented answers. An ecosystem
headless library (Kobalte / Reka UI / Bits UI) is a *reference source* for
proposals, never a replacement: even when one exists, still deliver the code.

## Framework idiom mapping (interface layer)

Translating from React docs is a refactor of the *controlled pattern*, not a rename:

| React | Vue | Svelte 5 | Solid |
|---|---|---|---|
| `open` + `onOpenChange` | `v-model:open` | `bind:open` (+ `onOpenChange` event) | signal prop + `onOpenChange` callback |

Show this mapping explicitly in the confirmation gate summary so the user
reviews semantics, not prop names.

The mapping above covers web frameworks. For non-web stacks, map the
interface to the platform's native patterns instead: Flutter
StatefulWidget + callbacks, SwiftUI `@State` / `Binding`, Jetpack Compose
`remember` / `mutableStateOf`, Qt QML properties / signals. Mini-program
native: React controlled props → `properties` + `triggerEvent` (e.g.
`onOpenChange` becomes an `open` property + a `bind:openchange` event),
`externalClasses` for style injection.

## Source code (delivery, #1)

- One code block per file with a filename header (`src/Dialog.tsx`, `src/index.ts`, …)
- Always output **complete files**, never patches or diffs (unless the user
  explicitly asks for a diff)
- List npm dependencies with versions; check against the user's existing deps
  for conflicts (e.g. floating-ui, date-fns, TanStack Virtual)

## Usage (delivery, #2)

- A minimal working example that compiles and demonstrates the main use case
- API summary: the props/events/slots a user will actually touch

## Interface standard (delivery, #3)

- Full props / events / slots / types
- Style interface contract — translated per platform:
  - Web: DOM structure / class slot inventory; state hooks (data-attribute
    set + animation phases); CSS custom properties marked with which global
    tokens they reference
  - Flutter: theme-agnostic widget; exposed parameters / ThemeExtension slots
  - SwiftUI: bare View; Environment values / ViewModifier hooks
  - Jetpack Compose: unstyled composable; Modifier parameters /
    CompositionLocal slots
  - Qt: unstyled QML item; property hooks / theme bindings
  - Mini-program (native): Component() constructor — properties / data /
    observers as the API; externalClasses as the class-slot mechanism;
    WXSS variables / theme.json as token hooks. Cross-end (Taro / uni-app):
    deliver like their web base (React / Vue) instead
- Reference styles PRE-WIRED to the probed project tokens (real token names,
  not generic placeholders — defaults only, user owns final values)
- Written for the probed styling system (web: className passthrough vs style
  props; non-web: the platform's theming hooks above)

## Acceptance script (optional, last)

Playwright spec covering: keyboard sequence, focus order / trap, ARIA snapshot
(getByRole / axe checks). The user copies it into their project and runs it;
passing = the assertion list passes. State clearly which behaviors it covers
and which require a manual screen-reader pass (complex components).
