---
name: gpui-async-tasks
description: Spawning and managing foreground/background async work in Zed (gpui Task system). Use when writing cx.spawn / spawn_in / background_spawn / detach, handling Task cancellation, debounce or throttle, periodic timer loops, blocking work off the main thread, UI freezes, tasks that never run or never stop, priority questions — also when the user says 后台任务, 异步, 防抖, 取消, 卡顿.
---

# Async tasks in Zed (gpui)

One invariant drives everything: **dropping a `Task<T>` cancels it.** Every
task is awaited, detached, or stored in a field — there is no fourth option,
and "orphaned" tasks (dropped without any of the three) are bugs caught by
`forbid_parking` in tests.

## 1. Pick the entry point

| Need | API |
|---|---|
| Update an entity later, on the main thread | `cx.spawn(async move \|this, cx\| ...)` — `this: WeakEntity<T>` |
| Same but also need the window | `cx.spawn_in(window, async move \|this, cx\| ...)` → `this.update_in(cx, \|this, window, cx\| ...)` |
| Already have `&mut Window` | `window.spawn(cx, ...)` |
| Heavy `Send` work off the main thread | `cx.background_spawn(future)` |
| Idle-time background polish | `foreground_executor().spawn_when_idle(...)` |
| Deep recursion / huge stacks / blocking syscalls | `background_executor().spawn_dedicated(...)` (fresh OS thread, ~2 MiB stack) |
| Already-computed value where a `Task` is required | `Task::ready(value)` — zero cost |

Rules baked into the framework:

- Foreground futures are `!Send`; background futures must be `Send`. So a
  background closure can only hold **snapshots** (rope/syntax snapshots, plain
  data), never entities or `cx`.
- `cx.spawn` hands you `WeakEntity<T>` on purpose: long work must not keep the
  entity alive. Handle the released-entity error (`.ok()?`, `.log_err()`, `?`).

## 2. The standard hop: background → foreground

```rust
let snapshot = this.take_snapshot();                 // immutable data only
let parse_task = cx.background_spawn(async move { compute(snapshot) });  // heavy work
cx.spawn(async move |this, cx| {
    let result = parse_task.await;                   // foreground task hops back
    this.update(cx, |this, cx| {
        this.apply(result);
        cx.notify();
    }).ok();
});
```

Prefer the **bounded foreground wait** for interactive work so small inputs
never pay a thread hop (`crates/editor/src/display_map/wrap_map.rs`,
`crates/language/src/buffer.rs`):

```rust
let task = cx.background_spawn(future);
match cx.foreground_executor().block_with_timeout(Duration::from_millis(5), task) {
    Ok(result) => apply_sync(result),                // fast path: still on main thread
    Err(task) => { /* store task, resume in cx.spawn */ }
}
```

## 3. Task lifetime — five patterns

1. **Overwrite-cancels (debounce/restart).** Store `Option<Task<...>>`; a new
   request overwrites the old task, dropping = cancelling it.

   ```rust
   struct Foo { pending_search: Option<Task<Results>>, ... }
   fn search(&mut self, cx: &mut Context<Self>) {
       self.pending_search = Some(cx.spawn(async move |this, cx| { ... }));
   }
   ```

2. **Two-field throttle** (for high-frequency triggers, e.g. workspace state
   serialization in `crates/workspace/src/workspace.rs::serialize_workspace`):
   a `_schedule_*` field gates a `timer(200ms).await` coalescer, the inner
   `_task` field holds the actual work so it can be `.take()`n for a flush on
   quit.
3. **Held until the owner drops** — `_field: Task<...>` (underscore = never
   read, only keeps alive). Long-lived workers: scanner loops, leader-update
   queues (`crates/worktree/src/worktree.rs`, workspace.rs).
4. **Fire-and-forget** — `detach()` (infallible, ~1300 uses) /
   `task.detach_and_log_err(cx)` (fallible, ~430 uses) /
   `detach_and_notify_err(entity, window, cx)` when the user must see the
   error as a toast.
5. **Periodic loop** — the canonical shape; the `?` on the weak update is the
   entity's natural exit:

   ```rust
   self._task = Some(cx.spawn(async move |this, cx| {
       loop {
           this.update(cx, |this, cx| this.poll(cx))?;
           cx.background_executor().timer(POLL_INTERVAL).await;
       }
   }));
   ```

## 4. Cancellation — five mechanisms, pick by workload

| Mechanism | When | Example |
|---|---|---|
| Drop the task | work is safely abortable | overwrite `pending_search`; `self.background_task.take()` in wrap_map |
| Generation/version check | work cannot be aborted; let it finish, discard the result | fuzzy finder `search_id`; parse `version.changed_since(...)` in `crates/language/src/buffer.rs` |
| Cooperative `AtomicBool` | long loops that can't be dropped mid-iteration | `cancel_flag` in `crates/fuzzy_nucleo/src/paths.rs` |
| Protocol cancel on drop | the peer is another process | `gpui_util::defer` sending `$/cancelRequest` (`crates/lsp/src/lsp.rs`) |
| Channel close | producer/consumer pipelines | search workers stop when `tx.is_closed()` |

Combine them when a flow has both an abortable and an unabortable stage
(fuzzy finder uses AtomicBool in the background + generation check at apply
time).

## 5. Discipline

- **Timers**: `cx.background_executor().timer(dur)` only. Never
  `smol::Timer`/`tokio::time` in app logic or tests — the test scheduler
  cannot drive them (`run_until_parked` would hang or race).
- **Long background loops**: `yield_now().await` periodically (search yields
  every 20k matches, wrap every 100 rows).
- **Priority**: default `Medium` is right almost always. `High` only for
  latency-critical batching (project search result flushes), `Low` for bulk
  scans (`scoped_priority` in worktree scanning), `RealtimeAudio` only for
  audio (livekit).
- **tokio**: only via `gpui_tokio` and only because a dependency needs tokio
  (TCP connect, WebSocket, livekit SDK). Dropping the gpui task aborts the
  tokio task — don't break that by leaking the join handle.
- **Error paths**: fallible detached tasks get `detach_and_log_err`; user-visible
  failures update entity state (and get rendered), not just logs.

## 6. Testing async behavior

`TestAppContext` runs a deterministic virtual clock:

- `cx.run_until_parked()` drains all runnable tasks; if only timers remain it
  advances the fake clock to the next deadline.
- `cx.executor().advance_clock(dur)` makes timers ready without running tasks —
  use it to skip debounce windows deterministically
  (`crates/editor/src/document_colors.rs` tests).
- Teardown runs `forbid_parking`: leftover tasks or non-parking infinite loops
  fail the test. Exclude genuinely infinite loops from tests with
  `cfg!(not(any(test, feature = "test-support")))` (see
  `crates/session/src/session.rs`).

## 7. Pre-flight checklist

- [ ] Every spawned task is awaited, stored, or detached (never dropped orphan).
- [ ] Background closures hold snapshots only — no entities, no `cx`.
- [ ] Debounce = stored `Option<Task>` overwritten; not a bool flag + sleep.
- [ ] Unabortable work has a version/generation check before results apply.
- [ ] Cross-process work has a `defer` guard sending the cancel message.
- [ ] Detached fallible tasks log or surface errors.
- [ ] Timers come from the gpui executor, in tests too.

## Deep dive

- Full analysis with 11 worked examples: `wiki/foreground-background-tasks.md`.
- Sources: `crates/gpui/src/executor.rs`, `crates/scheduler/src/executor.rs`,
  `crates/gpui/src/app/context.rs` (spawn entry points),
  `crates/gpui_tokio/src/gpui_tokio.rs`.
