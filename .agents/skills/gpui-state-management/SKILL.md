---
name: gpui-state-management
description: Where state lives and how it reacts in the Zed codebase (gpui entities, globals, element state, settings). Use when adding or refactoring app/view state, choosing where to store something, wiring cx.notify / observe / emit / subscribe, fixing "cannot update X while it is already being updated" panics, entity leaks, or stale-UI bugs — even when the user just says "store this", "add a setting", "react to changes", 状态管理, 状态放哪, 全局状态, 响应式.
---

# State management in Zed (gpui)

All state in a Zed window lives in one of exactly five homes. Picking the wrong
home is the root cause of most state bugs (stale UI, re-entrancy panics, leaks).
Decide the home first, then use that home's access and reaction mechanisms —
never mix them (e.g. no `RefCell`/`Arc<Mutex>` around UI state; entities already
serialize access on the main thread).

## 1. Choose the home

| State | Home | Access | Reaction |
|---|---|---|---|
| Process-level services (DB, FS, HTTP client, registries, version) | gpui `Global` (`cx.set_global`) | `T::global(cx)` / `try_global` / `update_global` | `cx.observe_global::<T>` |
| Business/domain state and view state | `Entity<T>` (`cx.new(\|cx\| ...)`) | `entity.read(cx)` / `entity.update(cx, \|this, cx\| ...)` | `cx.notify()` / `observe` / `emit` / `subscribe` |
| Local UI state of one element (scroll pos, hover, per-element caches) | Element state (`window.with_element_state`, keyed by element path + type) | inside `Element::prepaint`/`paint` | dies automatically when the element stops rendering |
| User-editable settings (external JSON files) | `SettingsStore` global | `T::get(None, cx)` / `T::get_global(cx)` | `cx.observe_global::<SettingsStore>` |
| Persisted workspace/session layout | SQLite domains (`WorkspaceDb`, `KeyValueStore`) | `WorkspaceDb::global(cx)` | reload at startup / on restore |

Rules of thumb:

- **Globals**: process-lifetime services and values only. Never put mutable
  business state in a Global — you only get a single "it changed" signal
  (`observe_global`), no per-field observation, no lifecycle. Reuse existing
  globals before adding new ones (`SettingsStore`, `AppDatabase`, `<dyn Fs>`,
  `Client`, `ThemeRegistry`, `GlobalAppState`).
- **Two views need the same data** → one Entity, shared handles. Never
  duplicate state and sync it manually.
- **`LanguageRegistry` precedent**: widely shared non-UI services that tests
  want to swap should be `Arc` passed explicitly (inside `AppState`), not a
  Global.

## 2. Entity borrow rules (the lease)

`entity.update(cx, ...)` *moves the value out of the entity map* (a "lease")
for the duration of the closure. Consequences:

- Re-entrant `update` **or** `read` of the *same* entity panics with
  `"cannot update {type} while it is already being updated"`. This is a logic
  error, not a condition to handle — restructure instead.
- Updating/reading *other* entities inside the closure is fine.
- `App` itself is `RefCell`-guarded: calling back into `cx` while a `&mut App`
  borrow is live panics the same way.

Three legitimate escapes from re-entrancy, in preference order:

1. **`cx.defer_in(window, |this, window, cx| ...)`** — run after the current
   update and effect flush. Use when you must act on yourself after a change.
2. **Async + `WeakEntity`** — `cx.spawn(async move |this, cx| this.update(cx, ...)?)`.
   The framework hands you a weak handle precisely so long work cannot keep
   the entity alive.
3. **Emit an event** and let another entity react via `subscribe`, instead of
   calling into it directly.

Error-handling conventions for `this.update(...)` in async closures:

```rust
this.update(cx, |this, cx| ...).log_err();  // entity gone; log and stop
this.update(cx, |this, cx| ...).ok()?;      // treat "released" as normal exit
let _ = this.update(cx, ...);               // explicitly don't care
```

## 3. Reactive primitives — pick by semantics

| You want | Use | Notes |
|---|---|---|
| "I changed; redraw me and wake my observers" | `cx.notify()` | call in every `&mut self` mutator that affects UI; Notify effects are deduped per entity within one update |
| "redo X whenever that entity changes" (generic invalidation) | `cx.observe(&other, f)` | handler receives `(this, other, cx)` |
| "announce a typed domain event" | `cx.emit(Event)` | requires `impl EventEmitter<Event> for T` |
| "consume another entity's domain events" | `cx.subscribe(&other, f)` | returns a `Subscription` |

Subscription lifecycle (most common bug in this area):

- `Subscription` is `#[must_use]`; **dropping it unsubscribes immediately**.
  Store in a `_subscriptions: Vec<Subscription>` field for entity-lifetime
  subscriptions; call `.detach()` only for truly permanent ones.
- Observer/subscribe handlers hold weak references — they die with the
  *observing* entity, so you don't need to clean up.
- Effects are queued and flushed at the end of the outermost update
  (`Effect::{Notify, Emit, RefreshWindows, NotifyGlobalObservers, Defer,
  EntityCreated}` in `crates/gpui/src/app.rs`). Observers therefore always see
  the *final* state of a batch of mutations; never assume mid-update
  observation.

## 4. Adding a setting

1. Content struct in `crates/settings_content` with `#[with_fallible_options]`
   (a bad value in user JSON degrades to `None` instead of failing the file).
2. Resolved struct elsewhere, `#[derive(RegisterSetting)]` — inventory
   auto-registers it; no manual list to update.
3. `impl Settings for T` with a hand-written `from_settings` (unwrap Options
   defensively).
4. Add defaults to `assets/settings/default.json`.
5. Read with `T::get(None, cx)` (path-aware: picks up per-directory
   `.zed/settings.json`) or `T::get_global(cx)`.
6. React with `cx.observe_global::<SettingsStore>`.

Cascade precedence (low → high): defaults → extension → managed global → user
(+ release-channel/platform overrides) → active profile → server → project
directory settings.

Do not confuse this with `Refineable`/`Cascade` — that is for gpui style/theme
values, not JSON settings.

## 5. Stateful vs stateless elements

- Component constructed only to be rendered → `RenderOnce` (takes `self`,
  receives `&mut App`), derive `IntoElement`.
- Entity-backed view → `Render` (`&mut self`, `&mut Context<Self>`).
- Element needing private cross-frame state (scroll offsets, caches) →
  `window.with_element_state` keyed by an `ElementId`; storage is
  `element_states: (GlobalElementId, TypeId) -> state`, and state not touched
  during a frame is dropped automatically. Do not build side tables elsewhere
  for this.

## 6. Pre-flight checklist

- [ ] Every mutator that affects UI calls `cx.notify()` (stale-UI bug #1).
- [ ] Returned `Subscription`s are stored or detached, never dropped inline.
- [ ] No strong `Entity<T>` handle cycles — back-pointers are `WeakEntity<T>`.
- [ ] No re-entrant `update`/`read` of the same entity; escapes via defer /
  weak-handle async / events.
- [ ] Mutable business state is not in a Global.
- [ ] Async work holds `WeakEntity`, and handles the released-entity error.
- [ ] Cross-frame element state uses `with_element_state`, not ad-hoc maps.

## Deep dive

- Full analysis: `wiki/state-management-and-architecture.md` (this repo).
- Sources: `crates/gpui/src/app/entity_map.rs` (storage/lease),
  `crates/gpui/src/app.rs` (`Effect`, flush loop, globals),
  `crates/gpui/src/app/context.rs` (context capabilities),
  `crates/gpui/src/subscription.rs`, `crates/settings/src/settings_store.rs`.
