# Project Context Probing (read-only)

Goal: determine framework + version, CSS approach, and design tokens so the
deliverables (especially the style interface contract) are tailored to the
user's stack. Probing never writes to the user's project.

## Sources

- Project root: user-provided path, else the current working directory.
- Read files only. No directories are created, no files are modified.

## Signals & confidence

| Target | Signals | Confidence |
|---|---|---|
| Framework + version | package.json deps; vite/nuxt/svelte.config; file extensions (.vue / .svelte / .tsx) | High |
| CSS approach | deps (stylex / tailwind / unocss / vanilla-extract / …); build plugins; source scan (`@theme`, `@apply`, `stylex.create`) | Medium-high |
| Design tokens | DESIGN.md; `:root {}` CSS vars; Tailwind v4 `@theme`; stylex tokens; tokens.js / tokens.json | Medium-high |
| Atomic CSS utilities | Tailwind theme / custom utilities; unocss shortcuts; class conventions in existing components | Medium-high — applied to generated reference styles |
| Ecosystem headless libs | deps: kobalte / reka-ui / bits-ui / melt-ui | High — used as *behavior reference*, never as an install recommendation |
| Non-web framework + version | Flutter: pubspec.yaml / .dart · SwiftUI: Package.swift / .swift · Jetpack Compose: build.gradle.kts / .kt · Qt: CMakeLists.txt / .qml / .cpp | High |
| Non-web styling system | Flutter ThemeData / ThemeExtension · Compose MaterialTheme / CompositionLocal · SwiftUI Asset Catalog / Color assets · Qt QSS / theme properties | Medium-high — replaces "CSS approach" for non-web stacks |
| Mini-program framework | native: app.json / project.config.json / .wxml / .wxss · cross-end: Taro (taro deps + .jsx/.tsx), uni-app (manifest.json + .vue pages) | High — Taro/uni-app follow their web base (React/Vue), native follows Component()/WXML |
| Mini-program styling | WXSS variables / theme.json / app.json window config · Taro follows CSS approach · uni-app: uni.scss variables | Medium-high |
| Desktop shells (Electron / Tauri) | Electron: electron deps + a web frontend inside · Tauri: tauri.conf.json frontendDist, src-tauri/Cargo.toml | High — treat as web stack: components live in the renderer, ignore main process / Rust backend |
| Existing conventions | read 1–2 representative existing components: className vs style prop passthrough, naming | Reference |

## Ask the user only in two cases

1. **Ambiguity** — multiple candidates (e.g. Tailwind + CSS Modules coexisting;
   monorepo with several frameworks). Ask a *confirm-style* question with the
   detected results pre-filled: "Detected X and Y — new components use which?"
2. **Blind spot** — wrong path, empty project, project not local, nothing
   detectable. Ask one line: framework + CSS approach + token location.

## Usage rules

- Probe results are judgment input for this session only — not stored, not remembered.
- Probe results are APPLIED, not just reported: generated reference styles
  bind to the real token names and atomic utilities found in the project
  (e.g. `var(--color-surface)`, Tailwind theme colors). The user can override
  them freely — this step just saves them the wiring.
- Framework **version precision matters**: Svelte 4 vs 5 (runes), Solid 1 vs 2,
  Vue 3.x — the wrong version can produce uncompilable code.
- The style interface contract must match the styling system:
  web → className passthrough vs style props; non-web → the platform's
  theming hooks (ThemeExtension / Environment / Modifier / QML properties).
- Non-web projects: "CSS approach" maps to the platform's styling system;
  tokens are probed from ThemeData / MaterialTheme / Asset Catalog / QSS
  locations instead of `:root` / `@theme`.
- Mini-programs: distinguish **native** (Component() + WXML, styling via
  WXSS / externalClasses / theme.json) from **cross-end frameworks**
  (Taro → React-like, uni-app → Vue-like; probe and deliver like their web
  base).
- Electron / Tauri: the UI layer is web tech — probe the frontend framework
  inside (package.json) and deliver like any web stack. For Tauri, ignore
  the src-tauri / Rust signals so the frontend is not mistaken for a native
  stack.
