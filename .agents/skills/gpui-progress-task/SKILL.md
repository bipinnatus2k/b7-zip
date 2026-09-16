---
name: gpui-progress-task
description: Implementing long-running operations in Zed UI that show progress and can be controlled by the user (cancel/dismiss) — the HeavyJob entity pattern. Use when adding progress bars, spinners, download/install/search/index/import flows, cancel buttons, or task status indicators, and when fixing progress that never updates, UI that can't be cancelled, or work that keeps running after the dialog closes — also when the user says 进度条, 可取消任务, 长时间任务.
---

# Long-running tasks with progress and control

Zed has no framework-level "progress task" API. The repeated, verified pattern
across auto-update, project search, extension install, and LSP work is:

**Make the job an entity. Progress is a status field (mutate + `cx.notify()`).
Control is the task handle stored in a field (overwrite/take/drop to cancel).**

```rust
struct HeavyJob {
    status: JobStatus,               // the ONLY thing render() reads
    _task: Option<Task<Result<()>>>, // handle: overwrite/take = cancel; drop of entity = cancel
}

enum JobStatus {
    Idle,
    Running { progress: Option<f32> }, // None = total size unknown
    Done,
    Errored(Arc<str>),                 // errors are state too — render them
}
```

## 1. Pick the progress granularity first

| Output shape | Progress form | Real case |
|---|---|---|
| Single value, known total | percent `Option<f32>` | auto-update download (`crates/auto_update/src/auto_update.rs`) |
| A stream of results | grow a collection; the count IS the progress | project search (`crates/search/src/project_search.rs`) |
| Few phases / unmeasurable | coarse status (Idle/Installing/Done/Errored) | extension install (`crates/extension_host`) |
| Job runs in another process | peer pushes progress via protocol; local state is a cache | LSP `$/progress` (`crates/project/src/lsp_store.rs`) |

## 2. Start (action handler)

```rust
fn start(&mut self, cx: &mut Context<Self>) {
    if self._task.is_some() { return; }              // concurrency guard; duplicates are no-ops
    self.status = JobStatus::Running { progress: None };
    cx.notify();                                     // surface the progress UI immediately

    self._task = Some(cx.spawn(async move |this, cx| {
        // Pack the reporting channel BEFORE going background:
        // background code can't touch entities (not Send), so hand it a
        // WeakEntity + AsyncApp pair it may call from its progress callback
        // (this is exactly what auto_update.rs does).
        let (progress_entity, mut progress_cx) = (this.clone(), cx.clone());

        let result = cx.background_spawn(heavy_work(move |fraction: Option<f32>| {
            progress_entity
                .update(&mut progress_cx, |this, cx| {
                    if let JobStatus::Running { progress } = &mut this.status {
                        *progress = fraction;
                    }
                    cx.notify();
                })
                .ok();
        }))
        .await;

        this.update(cx, |this, cx| {                 // hop back to the main thread to land
            this._task = None;
            this.status = match result {
                Ok(value) => JobStatus::Done,
                Err(err) => JobStatus::Errored(err.to_string().into()),
            };
            cx.notify();
        }).ok();
    }));
}
```

UI-side convention ("state first, result later" — see
`crates/extensions_ui/src/extensions_ui.rs` fetch flow): set a
loading/failed flag and `cx.notify()` **before** awaiting, then overwrite with
the final outcome. Errors are rendered state, not just log lines.

## 3. Cancel (action handler)

```rust
fn cancel(&mut self, cx: &mut Context<Self>) {
    if let Some(task) = self._task.take() {
        drop(task);                                  // drop = cancel the future
    }
    self.status = JobStatus::Idle;
    cx.notify();
}
```

- **Detached tasks cannot be cancelled.** Any flow with a cancel button must
  keep the handle in `Option<Task>` — this is the single rule that decides
  detach vs. store.
- **The peer doesn't see your drop.** If the work lives in another process,
  register a `gpui_util::defer` guard when spawning that sends the protocol
  cancel on drop: LSP sends `$/cancelRequest` (`crates/lsp/src/lsp.rs`),
  remote search sends `FindSearchCandidatesCancelled`
  (`crates/project/src/project_search.rs`).
- **Work that can't be aborted mid-flight** (e.g. tree-sitter parse): don't
  try. Let it finish and discard via version/generation check
  (`version.changed_since(...)`, `search_id >= latest_search_id`).

## 4. Progress that renders smoothly

- Batch before you notify: consume streams with `.ready_chunks(N)` (project
  search uses 1024) or throttle by delta. Per-byte `update+notify` floods the
  main thread; notify dedup and frame merging are a safety net, not a design.
- Coarse flows can skip counts entirely: an in-flight map
  (`outstanding_operations` in extension_host) is the status; an `on_drop` hook
  removes the entry and notifies — drop doubles as the completion signal.
- A total size you don't have yet is `None`, and the UI should show an
  indeterminate spinner for it (auto-update's `progress: Option<f32>`).

## 5. Test it deterministically

Debounced starts and progress reporting are unit-testable with the virtual
clock (see `test_download_release_reports_progress` in auto_update.rs and the
document_colors tests):

```rust
cx.executor().advance_clock(DEBOUNCE);      // jump the fake clock
// await the expected side effect (request handle, status read)
cx.run_until_parked();
// assert progress/status transitions
```

Infinite supervision loops must be excluded from tests:
`cfg!(not(any(test, feature = "test-support")))`.

## 6. Pre-flight checklist

- [ ] Status enum covers Idle/Running/Done/Errored and render() reads only it.
- [ ] `cx.notify()` after every status transition, including the initial one.
- [ ] Task handle stored in a field; duplicate starts are guarded no-ops.
- [ ] Cancel takes + drops the handle; peer-side cancel via defer guard if remote.
- [ ] Progress callback holds WeakEntity + AsyncApp, never strong refs.
- [ ] Streamed progress is batched before notify.
- [ ] Failure lands in `Errored` state and is rendered, not just logged.
- [ ] Debounce/progress covered by `advance_clock` tests.

## Deep dive

- Full analysis with the four real cases compared:
  `wiki/long-running-tasks-with-progress.md`.
- Related skills: `gpui-async-tasks` (task mechanics), `gpui-state-management`
  (entity/reactive rules).
- Sources: `crates/auto_update/src/auto_update.rs:123-152, 749-775`,
  `crates/search/src/project_search.rs:515-527`,
  `crates/extension_host/src/extension_host.rs:764-880`,
  `crates/lsp/src/lsp.rs:1408-1519`.
