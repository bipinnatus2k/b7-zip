---
name: recomp
description: >
  Replicate a UI component from a component library documentation page into a
  headless (behavior-only, unstyled) component for the user's framework
  (SolidJS / Vue / Svelte — also Flutter / SwiftUI / Jetpack Compose / Qt /
  mini-programs (WeChat native, Taro, uni-app)). Use when the user provides a component library docs
  URL (shadcn/ui, Radix, MUI, Ant Design, ...) and wants that component ported
  to their stack as paste-ready source code — e.g. "按照文档复刻组件",
  "照着文档生成 headless 组件", "把这个组件移植到我的框架",
  "replicate/port/convert this component from <url>". Everything is delivered
  in the conversation (source files, usage notes, style contract); nothing is
  written to the user's project.
---

# recomp — component docs → headless replicator

Replicates a component from a library documentation URL into a **headless**
(behavior-only, zero styles) component in the user's framework, delivered in
conversation for the user to copy into their own component directory. The user
owns styling, design tokens, and integration.

## Input

| Input | Required | Notes |
|---|---|---|
| Docs URL | yes | single-component docs page of a component library |
| Project root | no | enables read-only probing of framework/CSS/tokens; falls back to cwd; if nothing detectable, ask one line |

## Workflow

1. **Fetch & validate the URL** — must be a single-component docs page
   (component name + props/API table + usage examples). Not a component library
   → refuse with a reason. Unreachable page → propose npm / GitHub source.
   → `references/verification.md`
2. **Probe project context (read-only)** — framework+version; styling system
   (web: CSS approach, design tokens, atomic CSS definitions; non-web:
   Flutter ThemeData, Compose MaterialTheme, SwiftUI Asset Catalog, Qt
   themes; mini-program: WXSS variables / theme.json); existing conventions;
   ecosystem headless libraries (used as reference material). Probe results
   are APPLIED to the generated source, not just described. Ask the user only
   on ambiguity or blind spots, with detected results pre-filled.
   → `references/probing.md`
3. **Build the behavior contract** — extract interface (props/events/slots,
   controlled/uncontrolled) and behavior from docs. Anything the docs do NOT
   state explicitly becomes an **open question** with a proposed approach
   (APG standard / ecosystem reference implementation) — never implemented
   silently.
4. **Confirm the contract (gate — mandatory)** — present the behavior
   contract summary, then walk through the open questions **one by one**.
   Each question offers a single option — the user replies yes / 确认 to
   accept, or types their own solution for custom input. Numbers are only
   used when multiple choices exist. Confirming the last question ends the
   gate — generate the deliverables directly afterwards, with no extra
   confirmation round. The contract summary and open questions live ONLY in
   this gate — never embed them in the final delivery.
   → `references/deliverables.md`
5. **Generate deliverables** — in conversation only; complete files, never
   patches; framework-native idioms (Vue `v-model`, Svelte 5 `bind:`/runes,
   Solid signals). Pre-wire the probed design tokens / atomic styles into the
   generated code (reference styles bind to real token names, class strings
   use the project's utilities) — the user can override afterwards. Open with
   "✅ 组件已复刻成功…". → `references/deliverables.md`
6. **Iterate** — user copies (accept) or requests changes; always re-emit full files.

## Deliverables (after the user confirms the contract)

User-priority order — source first, keep the delivery lean:

1. **Opening line**: "✅ 组件已复刻成功，复制以下组件代码到你的项目目录即可使用。"
2. **Source code** (top priority) — complete files, one code block per file
   with filenames, dependency list with versions
3. **Usage** — minimal working example + API summary
4. **Interface standard** — props/events/slots/types, plus the style
   interface contract translated per platform (web: data-attributes / class
   slots / CSS variables; non-web: ThemeExtension / Environment / Modifier /
   QML property hooks). → `references/deliverables.md`
5. Optional, at the very end — acceptance script (keyboard, focus, ARIA snapshot)

The behavior contract summary (validation + open questions) is presented
BEFORE replication as the confirmation gate; do not repeat it in the delivery.

## Hard rules

- **Read-only**: never create directories or modify files in the user's
  project; probe results are session-only judgment input, not persisted
- **Never guess accessibility semantics**: web uses ARIA / WAI-ARIA APG;
  non-web uses the platform's own API (Flutter Semantics, SwiftUI
  accessibility modifiers, Compose semantics, Qt QAccessible); mini-programs
  use aria-* attributes / platform a11y docs. Uncertain behavior becomes open
  questions with a proposed approach for the user to confirm — never
  invented, never implemented silently
- **Refuse** non-component-library pages (with a reason). If the user's
  ecosystem already has an equivalent headless implementation
  (Kobalte / Reka UI / Bits UI), **learn from it as a reference** (behavior +
  API), but still deliver the replicated code — never redirect the user to
  install a third-party library
- **License**: replicate interface + behavior only, never copy proprietary
  source or style values; record source URL + fetch date in the decision log
- Complex components (DataGrid, virtualized tables, date pickers): state
  quality expectations up front or recommend a mature library

## References

- `references/verification.md` — URL validation checklist & refusal cases
- `references/probing.md` — project probing signals, confidence, fallback questions
- `references/deliverables.md` — deliverable templates, decision classification, framework idiom mapping
