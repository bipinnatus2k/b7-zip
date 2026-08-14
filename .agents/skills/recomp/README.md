# recomp

recomp is a component-replication skill for frontend developers (works with
Claude, Codex and other mainstream AI agents). It bridges the gap between a
component library's framework (e.g. React) and your project's framework
(e.g. SolidJS): the AI reads the library docs and replicates the component's
interaction and behavior patterns into headless component source and usage
examples adapted to your framework — all delivered in the conversation, copy
and use.

> 中文版见 [README_cn.md](./README_cn.md)

## What it does

Copy a component library docs link (shadcn/ui, Radix, MUI, Ant Design, …),
send it to recomp, and it will:

1. **Validate** the page is actually a component library docs page (refuses
   blogs, utility libs, design files — with a reason)
2. **Probe your project read-only** — framework + version, CSS approach,
   design tokens (DESIGN.md, `:root` CSS vars, `@theme`, stylex tokens) — no
   files created, nothing modified
3. **Produce**:
   - a **behavior contract summary** — what was taken from the docs, what was
     filled from WAI-ARIA APG patterns, and what is left open for you to decide
   - **headless component source** in your framework
     (SolidJS / Vue / Svelte), complete files, with a dependency list
   - **usage instructions** (examples + API summary)
   - a **style interface contract** — data-attributes, class slots, CSS
     variables — matched to your CSS setup (className passthrough for
     Tailwind, style props for StyleX, …)
   - **no installation** — the output is plain source files, not an npm
     package: nothing lands in node_modules, no build config changes, no
     dependency tree to maintain. The component becomes your own code, not a
     dependency you track upstream

You keep styling and design tokens entirely in your hands: paste the source
into your component directory, wire up the styles, done. Don't like a
decision? Say so — the component is re-emitted in full.

## Supported stacks

recomp replicates *into* any of these stacks — the docs URL you send decides
what gets replicated:

| Category | Stacks | Notes |
|---|---|---|
| Web | SolidJS / Vue / Svelte — any CSS approach (Tailwind, StyleX, CSS-in-JS, vanilla) | the sweet spot: behavior + interface adapted to framework idioms |
| Native | Flutter / SwiftUI / Jetpack Compose / Qt | style interface contract translated per platform (ThemeExtension / Environment / Modifier / QML hooks) |
| Mini-programs | native (WeChat etc., Component() + WXML) · Taro · uni-app | native: properties / externalClasses / triggerEvent; Taro & uni-app follow their web base |
| Desktop shells | Electron / Tauri | the UI layer is web tech — treated as web stacks, components live in the renderer |

Accessibility sources are platform-appropriate: ARIA / WAI-ARIA APG for web,
platform APIs for native (Flutter Semantics, SwiftUI accessibility
modifiers, Compose semantics, Qt QAccessible), aria-* attributes for
mini-programs.

## Install

**Option 1 — one-liner (recommended)** — either source works:

```sh
npx skills add brickhu/recomp          # standalone repo
npx skills add brickhu/skills/recomp   # skills repo (path form)
```

**Option 2 — manual copy** (works with any agent that loads SKILL.md)

```sh
git clone https://github.com/brickhu/recomp.git
cp -r recomp ~/.agents/skills/recomp
```

No dependencies, no build step. **Restart your agent after installing** so
the skill gets loaded.

## Usage

1. Copy a component library docs link and send it to recomp: `recomp <url>`
2. (Optional) Point it at your project so it can probe your stack:
   `recomp <url> --project ~/my-app` — otherwise it falls back to the current
   working directory
3. Confirm the open questions one by one (reply `yes` / 确认, or type your
   own solution) — the component is generated right after the last one
4. Copy the source files, wire your styles
5. Iterate in conversation: *"the escape handling is wrong"*, *"use `bind:`
   instead of `v-model`"* — every revision is delivered as complete files

### Full example

Replicating shadcn/ui's Button for a SolidJS + StyleX project, end to end:

Open your AI coding assistant (Claude Code, Codex, Cursor, Zed), start a new
conversation and send:

```text
recomp https://ui.shadcn.com/docs/components/button, project at ~/my-app (SolidJS + StyleX)

# If your conversation is inside the project directory, just send:
recomp https://ui.shadcn.com/docs/components/button
```

It replies with the behavior contract summary:

```text
✅ Validated: shadcn/ui Button docs page
---
API: variant / size / disabled / loading → SolidJS props
Documented behavior: disabled blocks clicks · loading disables the button
Dependencies: none, plain native implementation
---
1 behavior is not specified in the docs — your call:

Item 1/1: keep the button's size while loading?
Suggestion: keep the size and expose data-state="loading" (no layout shift,
the style layer gets a state hook)
Reply yes / 确认 to accept, or type your own solution
```

Reply `yes` — the gate closes and it delivers the files directly:

```text
✅ 组件已复刻成功，复制以下组件代码到你的项目目录即可使用。
```

Then: complete source files (one code block per file), a usage example, and
the interface standard — with reference styles pre-wired to your probed
design tokens. Copy the code, wire the rest of your styles, done. Not happy
with a behavior? Say so in the conversation — the full file is re-emitted,
no patches.

## What it refuses

- **Non-library pages** (blogs, utility libraries, design files) — refused
  with a reason
- **Guessing accessibility semantics** (ARIA / focus / keyboard) — filled from
  WAI-ARIA APG patterns or explicitly marked "not implemented"
- **Writing into your project** — probing is read-only; everything is
  delivered in chat
- **Pushing third-party libraries on you** — if your ecosystem already has an
  equivalent headless library (Kobalte for Solid, Reka UI for Vue, Bits UI for
  Svelte), it learns from it as a reference but still delivers your own
  copy-paste code — the component library stays in your hands

## Files

```
recomp/
├── SKILL.md                     # skill definition (entry point)
├── README.md                    # this guide
├── README_cn.md                 # 中文版
└── references/
    ├── verification.md          # URL validation checklist & refusal cases
    ├── probing.md               # read-only project probing signals
    └── deliverables.md          # deliverable templates & decision classification
```

## License

MIT
