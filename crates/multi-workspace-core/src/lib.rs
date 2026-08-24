//! A framework-agnostic core for "one window hosts many workspaces" containers.
//!
//! Distilled from zed's `crates/workspace/src/multi_workspace.rs`: the design
//! ideas are kept, the GPUI specifics are factored out into an [`Adapter`]
//! trait plus an event queue that a host maps onto its own notification
//! system.
//!
//! | Original mechanism                           | Core counterpart                           |
//! |----------------------------------------------|--------------------------------------------|
//! | `held: Vec<HeldWorkspace>` (pinned + LRU)    | [`Core::activate`] / [`Core::add`]         |
//! | Active workspace derived from `activated_at` | [`Core::displayed`] (never stored)         |
//! | Groups store only key/expanded/order         | [`GroupState`]; members always derived     |
//! | `Rc<Cell<EntityId>>` chrome arbitration      | [`Core::active_cell`] / [`Core::owns_chrome`] |
//! | hold -> pin -> activate -> detach lifecycle  | same four primitives                       |
//! | Three-phase async removal                    | [`Core::begin_removal`] + [`RemovalSession::commit`] |
//! | Feature-flag collapse                        | [`Core::set_retain_enabled`]               |
//! | Integrity assertions                         | [`Core::assert_integrity`]                 |
//!
//! Group keys are queried live through [`Adapter`] rather than cached per
//! row, mirroring how the original reads `project_group_key(cx)` straight off
//! the entity so renames are immediately visible.

use std::cell::Cell;
use std::rc::Rc;

/// Host-supplied view over the workspace objects managed by the container.
pub trait Adapter {
    /// A cheap handle identifying a workspace (entity id, pointer, key...).
    type Workspace: Copy + PartialEq + Clone + std::fmt::Debug;
    /// Identity of one project group: e.g. (main worktree paths, host).
    type Key: Clone + PartialEq + std::fmt::Debug;

    fn group_key(&self, workspace: &Self::Workspace) -> Self::Key;

    /// Whether the workspace can serve as a replacement target. In zed this
    /// filters out disconnected remote projects.
    fn is_available(&self, workspace: &Self::Workspace) -> bool;

    /// Whether removing the last member of this group should suggest
    /// reopening it afterwards. In zed: local host and non-empty paths.
    fn allows_reopen(&self, _key: &Self::Key) -> bool {
        true
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemovalIntent {
    /// After removal, keep the project reachable by reopening its roots.
    KeepProject,
    /// Remove without reopening anything (e.g. the user deleted the row).
    CloseProject,
}

#[derive(Clone, Debug)]
pub enum Event<W: Clone> {
    ActiveWorkspaceChanged {
        previous: Option<W>,
        source: Option<W>,
    },
    WorkspaceAdded(W),
    WorkspaceRemoved(W),
    GroupsChanged,
}

#[derive(Clone, Debug)]
struct Held<W> {
    workspace: W,
    pinned: bool,
    activated_at: Option<u64>,
}

/// Display metadata of one project group. Members are *not* stored here.
#[derive(Clone, Debug)]
pub struct GroupState<K> {
    pub key: K,
    pub expanded: bool,
}

#[derive(Clone, Debug)]
pub struct RemovalOutcome<W, K> {
    /// Consent was denied; no state changed.
    pub aborted: bool,
    pub removed_any: bool,
    /// Project the host may want to reopen after removal completed.
    pub reopen_key: Option<K>,
    pub replacement: Option<W>,
}

/// Everything captured before any mutation happens. The host runs its consent
/// UI between [`Core::begin_removal`] and [`RemovalSession::commit`]; because
/// the world may change while prompts are open, all positional information is
/// frozen in this snapshot.
pub struct RemovalSession<A: Adapter> {
    targets: Vec<A::Workspace>,
    intent: RemovalIntent,
    original_active: A::Workspace,
    original_active_key: A::Key,
    neighbor_keys: Vec<A::Key>,
    adjacent_key: Option<A::Key>,
}

pub struct Core<A: Adapter> {
    held: Vec<Held<A::Workspace>>,
    groups: Vec<GroupState<A::Key>>,
    active_cell: Rc<Cell<Option<A::Workspace>>>,
    retain_enabled: bool,
    stamp_counter: u64,
    events: Vec<Event<A::Workspace>>,
}

impl<A: Adapter> Core<A> {
    pub fn new(initial: A::Workspace, retain_enabled: bool) -> Self {
        Self {
            held: vec![Held {
                workspace: initial,
                pinned: false,
                activated_at: Some(0),
            }],
            groups: Vec::new(),
            active_cell: Rc::new(Cell::new(Some(initial))),
            retain_enabled,
            stamp_counter: 0,
            events: Vec::new(),
        }
    }

    // ------------------------------------------------------------------
    // Shared ownership cell (zed: Rc<Cell<EntityId>> chrome arbitration).
    //
    // Child components compare themselves against this cell instead of
    // reading back into the container, which would be impossible under
    // GPUI's single-writer borrow rules. Contract: update the cell BEFORE
    // emitting ActiveWorkspaceChanged so observers see consistent state.
    // ------------------------------------------------------------------

    pub fn active_cell(&self) -> Rc<Cell<Option<A::Workspace>>> {
        self.active_cell.clone()
    }

    pub fn owns_chrome(&self, workspace: &A::Workspace) -> bool {
        self.active_cell.get() == Some(*workspace)
    }

    // ------------------------------------------------------------------
    // Derived active state: never stored, always computed
    // ------------------------------------------------------------------

    /// The displayed workspace: most recently activated row.
    ///
    /// Panics when empty; [`Core::detach`] guards that invariant by refusing
    /// to detach the displayed row directly.
    pub fn displayed(&self) -> A::Workspace {
        self.held
            .iter()
            .max_by_key(|held| held.activated_at)
            .expect("a container always holds at least one workspace")
            .workspace
    }

    pub fn workspaces(&self) -> impl Iterator<Item = &A::Workspace> {
        self.held.iter().map(|held| &held.workspace)
    }

    pub fn held_index(&self, workspace: &A::Workspace) -> Option<usize> {
        self.held
            .iter()
            .position(|held| held.workspace == *workspace)
    }

    pub fn is_retained(&self, workspace: &A::Workspace) -> bool {
        self.held
            .iter()
            .any(|held| held.pinned && held.workspace == *workspace)
    }

    pub fn displayed_is_retained(&self) -> bool {
        let displayed = self.displayed();
        self.is_retained(&displayed)
    }

    pub fn events(&self) -> &[Event<A::Workspace>] {
        &self.events
    }

    pub fn take_events(&mut self) -> Vec<Event<A::Workspace>> {
        std::mem::take(&mut self.events)
    }

    // ------------------------------------------------------------------
    // Lifecycle primitives: hold -> pin -> activate -> detach
    // ------------------------------------------------------------------

    /// Ensures the workspace has a row; returns its index.
    fn hold(&mut self, workspace: A::Workspace) -> usize {
        if let Some(index) = self.held_index(&workspace) {
            return index;
        }
        self.held.push(Held {
            workspace,
            pinned: false,
            activated_at: None,
        });
        self.held.len() - 1
    }

    /// Pins the row so it survives navigating away, creating its group row.
    fn pin(&mut self, index: usize, key: &A::Key) {
        if self.held[index].pinned {
            return;
        }
        self.held[index].pinned = true;
        self.ensure_group(key.clone());
        let workspace = self.held[index].workspace;
        self.events.push(Event::WorkspaceAdded(workspace));
    }

    /// Removes a row and reports the removal. The displayed workspace must be
    /// switched away from first.
    fn detach(&mut self, workspace: &A::Workspace) {
        let displayed = self.displayed();
        if let Some(index) = self.held_index(workspace) {
            assert_ne!(
                self.held[index].workspace, displayed,
                "the displayed workspace must be re-pointed before it is detached"
            );
            self.held.remove(index);
        }
        self.events.push(Event::WorkspaceRemoved(*workspace));
    }

    /// Makes `workspace` held and active. Ordering mirrors the original
    /// `activate`: promote the outgoing transient first, publish the shared
    /// cell before any observer runs, stamp the LRU clock, and degrade to
    /// replace-semantics when retention is disabled.
    pub fn activate(&mut self, workspace: A::Workspace, source: Option<A::Workspace>, adapter: &A) {
        if self.displayed() == workspace {
            return;
        }

        let old_active = self.displayed();
        let old_active_was_retained = self.displayed_is_retained();
        let retain = self.retain_enabled;

        if retain && !old_active_was_retained {
            let key = adapter.group_key(&old_active);
            let index = self.hold(old_active);
            self.pin(index, &key);
        }

        let displayed = self.hold(workspace);
        if retain {
            let key = adapter.group_key(&workspace);
            self.pin(displayed, &key);
        }

        self.active_cell.set(Some(workspace));

        self.stamp_counter += 1;
        let stamp = self.stamp_counter;
        self.held[displayed].activated_at = Some(stamp);

        if !retain && !old_active_was_retained {
            self.detach(&old_active);
        }

        self.events.push(Event::ActiveWorkspaceChanged {
            previous: Some(old_active),
            source,
        });
    }

    /// Adds a workspace as persistent without changing which one is active.
    pub fn add(&mut self, workspace: A::Workspace, adapter: &A) {
        if self.is_retained(&workspace) {
            return;
        }
        let key = adapter.group_key(&workspace);
        let index = self.hold(workspace);
        self.pin(index, &key);
    }

    /// Promotes the displayed workspace to persistent if transient.
    pub fn retain_active(&mut self, adapter: &A) {
        let index = self
            .held_index(&self.displayed())
            .expect("displayed row exists");
        if self.held[index].pinned {
            return;
        }
        let key = adapter.group_key(&self.held[index].workspace);
        self.pin(index, &key);
    }

    /// Discards everything but the displayed workspace; used when multi-
    /// workspace support is switched off at runtime.
    pub fn collapse_to_single(&mut self) {
        let displayed = self.displayed();
        let others: Vec<_> = self.workspaces().copied().collect();
        for workspace in others {
            if workspace != displayed {
                self.detach(&workspace);
            }
        }
        for held in &mut self.held {
            held.pinned = false;
        }
        self.groups.clear();
    }

    pub fn set_retain_enabled(&mut self, enabled: bool, adapter: &A) {
        if enabled == self.retain_enabled {
            return;
        }
        self.retain_enabled = enabled;
        if !enabled {
            self.collapse_to_single();
        } else {
            self.retain_active(adapter);
        }
    }

    // ------------------------------------------------------------------
    // Project groups: display metadata only (order + expanded flag)
    // ------------------------------------------------------------------

    pub fn ensure_group(&mut self, key: A::Key) {
        if self.groups.iter().any(|group| group.key == key) {
            return;
        }
        self.groups.insert(0, GroupState { key, expanded: true });
    }

    pub fn groups(&self) -> &[GroupState<A::Key>] {
        &self.groups
    }

    pub fn set_expanded(&mut self, key: &A::Key, expanded: bool) -> bool {
        match self.groups.iter_mut().find(|group| &group.key == key) {
            Some(group) => {
                group.expanded = expanded;
                true
            }
            None => false,
        }
    }

    pub fn set_all_groups_expanded(&mut self, expanded: bool) {
        for group in &mut self.groups {
            group.expanded = expanded;
        }
    }

    pub fn move_group_up(&mut self, key: &A::Key) -> bool {
        let Some(index) = self.groups.iter().position(|group| &group.key == key) else {
            return false;
        };
        if index == 0 {
            return false;
        }
        self.groups.swap(index - 1, index);
        self.events.push(Event::GroupsChanged);
        true
    }

    pub fn move_group_down(&mut self, key: &A::Key) -> bool {
        let Some(index) = self.groups.iter().position(|group| &group.key == key) else {
            return false;
        };
        if index + 1 >= self.groups.len() {
            return false;
        }
        self.groups.swap(index, index + 1);
        self.events.push(Event::GroupsChanged);
        true
    }

    /// Deletes the group row itself. Members survive unless separately
    /// removed.
    pub fn remove_group_row(&mut self, key: &A::Key) {
        self.groups.retain(|group| &group.key != key);
        self.events.push(Event::GroupsChanged);
    }

    /// Transitions a group from `old_key` to `new_key`, resolving collisions:
    /// the active workspace's group wins; the winner keeps its sidebar slot by
    /// being re-keyed in place; the loser's row is dropped. If another
    /// retained workspace still carries the old key its group row is
    /// recreated so it remains reachable.
    pub fn rekey_group(&mut self, old_key: &A::Key, new_key: &A::Key, adapter: &A) {
        if old_key == new_key {
            return;
        }

        let old_exists = self.groups.iter().any(|g| &g.key == old_key);
        let new_exists = self.groups.iter().any(|g| &g.key == new_key);

        if !old_exists {
            self.ensure_group(new_key.clone());
            return;
        }

        if new_exists {
            let active_key = adapter.group_key(&self.displayed());
            if active_key == *new_key {
                self.groups.retain(|g| &g.key != old_key);
            } else {
                self.groups.retain(|g| &g.key != new_key);
                if let Some(group) = self.groups.iter_mut().find(|g| &g.key == old_key) {
                    group.key = new_key.clone();
                }
            }
        } else if let Some(group) = self.groups.iter_mut().find(|g| &g.key == old_key) {
            group.key = new_key.clone();
        }

        let other_needs_old_key = self
            .held
            .iter()
            .filter(|held| held.pinned)
            .any(|held| adapter.group_key(&held.workspace) == *old_key);
        if other_needs_old_key {
            self.ensure_group(old_key.clone());
        }

        self.events.push(Event::GroupsChanged);
    }

    /// Merges restored rows ahead of existing ones, dropping duplicates.
    pub fn restore_groups(&mut self, restored: Vec<GroupState<A::Key>>) {
        let mut merged: Vec<GroupState<A::Key>> = Vec::new();
        for group in restored {
            if merged.iter().any(|existing| existing.key == group.key) {
                continue;
            }
            merged.push(group);
        }
        let existing = std::mem::take(&mut self.groups);
        for group in existing {
            if !merged.iter().any(|candidate| candidate.key == group.key) {
                merged.push(group);
            }
        }
        self.groups = merged;
    }

    /// Pinned members whose live group key matches.
    pub fn members_of_group(&self, key: &A::Key, adapter: &A) -> Vec<A::Workspace> {
        self.held
            .iter()
            .filter(|held| held.pinned)
            .map(|held| held.workspace)
            .filter(|workspace| adapter.group_key(workspace) == *key)
            .collect()
    }

    /// Most recently displayed member of a group.
    pub fn last_active_for_group(&self, key: &A::Key, adapter: &A) -> Option<A::Workspace> {
        self.held
            .iter()
            .filter(|held| adapter.group_key(&held.workspace) == *key)
            .filter_map(|held| Some((held.activated_at?, held.workspace)))
            .max_by_key(|(stamp, _)| *stamp)
            .map(|(_, workspace)| workspace)
    }

    /// Other group keys nearest-first from `index`, preferring the following
    /// group over the preceding one at equal distance.
    pub fn neighbor_keys_from(&self, index: usize) -> Vec<A::Key> {
        (1..self.groups.len())
            .flat_map(|distance| [index.checked_add(distance), index.checked_sub(distance)])
            .flatten()
            .filter_map(|index| self.groups.get(index))
            .map(|group| group.key.clone())
            .collect()
    }

    // ------------------------------------------------------------------
    // Three-phase removal
    //
    // Phase 1 (begin): freeze the neighborhood; mutate nothing.
    // Phase 2 (host): run consent UI against the frozen snapshot.
    // Phase 3 (commit): if every consent holds, one synchronous edit:
    //     delete rows, pick a replacement (same group -> nearest neighbor ->
    //     factory), restore the display if prompts moved it, and report
    //     whether a project should be reopened.
    // ------------------------------------------------------------------

    pub fn begin_removal(
        &mut self,
        targets: Vec<A::Workspace>,
        intent: RemovalIntent,
        adapter: &A,
    ) -> Option<RemovalSession<A>> {
        if targets.is_empty() {
            return None;
        }
        let original_active = self.displayed();
        let original_active_key = adapter.group_key(&original_active);
        let group_index = self
            .groups
            .iter()
            .position(|group| group.key == original_active_key);
        let neighbor_keys = group_index
            .map(|index| self.neighbor_keys_from(index))
            .unwrap_or_default();
        let adjacent_key = group_index.and_then(|index| {
            self.groups
                .get(index + 1)
                .or_else(|| index.checked_sub(1).and_then(|prev| self.groups.get(prev)))
                .map(|group| group.key.clone())
        });

        Some(RemovalSession {
            targets,
            intent,
            original_active,
            original_active_key,
            neighbor_keys,
            adjacent_key,
        })
    }

    /// Removes a whole group row plus its members. Pins the displayed
    /// workspace first when needed so the later switch cannot re-pin it and
    /// resurrect the row this call just deleted.
    pub fn begin_group_removal(&mut self, key: &A::Key, adapter: &A) -> Option<RemovalSession<A>> {
        let displayed = self.displayed();
        if adapter.group_key(&displayed) == *key && !self.is_retained(&displayed) {
            let index = self.hold(displayed);
            self.pin(index, key);
        }
        let members = self.members_of_group(key, adapter);
        self.remove_group_row(key);
        self.begin_removal(members, RemovalIntent::CloseProject, adapter)
    }

    /// Checks container invariants: every retained workspace appears once and
    /// has a matching group row for its live key.
    pub fn assert_integrity(&self, adapter: &A) -> Result<(), String> {
        let mut seen = Vec::new();
        for held in self.held.iter().filter(|held| held.pinned) {
            if seen.contains(&held.workspace) {
                return Err(format!("workspace {:?} is retained more than once", held.workspace));
            }
            seen.push(held.workspace);

            let key = adapter.group_key(&held.workspace);
            if !self.groups.iter().any(|group| group.key == key) {
                return Err(format!(
                    "workspace {:?} has live key {:?} but no group row",
                    held.workspace, key
                ));
            }
        }
        Ok(())
    }
}

impl<A: Adapter> RemovalSession<A> {
    fn live_member(
        &self,
        core: &Core<A>,
        key: &A::Key,
        doomed: &[A::Workspace],
        adapter: &A,
    ) -> Option<A::Workspace> {
        let available = |workspace: &A::Workspace| {
            !doomed.contains(workspace) && adapter.is_available(workspace)
        };
        core.last_active_for_group(key, adapter)
            .filter(available)
            .or_else(|| {
                core.held
                    .iter()
                    .filter(|held| held.pinned)
                    .map(|held| held.workspace)
                    .filter(|workspace| adapter.group_key(workspace) == *key)
                    .find(available)
            })
    }

    /// Executes phase three. Consumes the session; returns what happened so
    /// the host can reopen projects or create UI for the replacement.
    pub fn commit(
        self,
        core: &mut Core<A>,
        consents: &[bool],
        adapter: &A,
        make_empty_workspace: impl FnOnce() -> A::Workspace,
    ) -> RemovalOutcome<A::Workspace, A::Key> {
        if consents.len() != self.targets.len() || !consents.iter().all(|&consent| consent) {
            return RemovalOutcome {
                aborted: true,
                removed_any: false,
                reopen_key: None,
                replacement: None,
            };
        }

        let displayed = core.displayed();
        let mut removed_any = false;
        let mut reopen_key = None;

        for workspace in &self.targets {
            if *workspace == displayed {
                continue;
            }
            if core.held_index(workspace).is_some() {
                core.detach(workspace);
                removed_any = true;
            }
        }

        let mut replacement = None;
        if self.targets.contains(&displayed) {
            let doomed = [displayed];

            let same_group = core
                .held
                .iter()
                .filter(|held| held.pinned && held.workspace != displayed)
                .map(|held| held.workspace)
                .find(|workspace| adapter.group_key(workspace) == self.original_active_key);

            if self.intent == RemovalIntent::KeepProject
                && same_group.is_none()
                && adapter.allows_reopen(&self.original_active_key)
            {
                reopen_key = Some(self.original_active_key.clone());
            }

            let chosen = same_group
                .or_else(|| {
                    self.neighbor_keys
                        .iter()
                        .find_map(|key| self.live_member(core, key, &doomed, adapter))
                })
                .unwrap_or_else(|| {
                    if reopen_key.is_none() {
                        reopen_key = self
                            .adjacent_key
                            .clone()
                            .filter(|key| adapter.allows_reopen(key));
                    }
                    make_empty_workspace()
                });

            core.activate(chosen, None, adapter);
            core.detach(&displayed);
            replacement = Some(chosen);
            removed_any = true;
        } else if core.displayed() != self.original_active
            && !self.targets.contains(&self.original_active)
        {
            // Consent UI moved the display elsewhere; go back.
            core.activate(self.original_active, None, adapter);
        }

        RemovalOutcome {
            aborted: false,
            removed_any,
            reopen_key,
            replacement,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[derive(Default)]
    struct TestAdapter {
        keys: HashMap<u32, String>,
        unavailable: HashSet<u32>,
        reopenable: HashSet<String>,
    }

    impl Adapter for TestAdapter {
        type Workspace = u32;
        type Key = String;

        fn group_key(&self, workspace: &u32) -> String {
            self.keys
                .get(workspace)
                .cloned()
                .unwrap_or_else(|| format!("g{}", workspace))
        }
        fn is_available(&self, workspace: &u32) -> bool {
            !self.unavailable.contains(workspace)
        }
        fn allows_reopen(&self, key: &String) -> bool {
            self.reopenable.contains(key)
        }
    }

    fn setup(members: &[(&str, &[u32])]) -> (Core<TestAdapter>, TestAdapter) {
        let mut adapter = TestAdapter::default();
        for (name, ids) in members {
            for id in *ids {
                adapter.keys.insert(*id, name.to_string());
            }
            adapter.reopenable.insert(name.to_string());
        }
        let initial = members[0].1[0];
        (Core::new(initial, true), adapter)
    }

    #[test]
    fn navigating_away_promotes_transient_workspace() {
        let (mut core, adapter) = setup(&[("g1", &[1]), ("g2", &[2])]);
        core.activate(2, None, &adapter);
        assert!(core.is_retained(&1), "previous transient must survive");
        assert_eq!(core.displayed(), 2);
    }

    #[test]
    fn displayed_is_derived_from_lru_stamps_and_shared_cell_tracks_it() {
        let (mut core, adapter) = setup(&[("g1", &[1]), ("g2", &[2]), ("g3", &[3])]);
        core.activate(2, None, &adapter);
        core.activate(3, None, &adapter);
        core.activate(1, None, &adapter);
        assert_eq!(core.displayed(), 1);
        assert!(core.owns_chrome(&1));
        assert!(!core.owns_chrome(&3));
    }

    #[test]
    fn disabling_retention_degrades_to_replace_semantics() {
        let (mut core, adapter) = setup(&[("g1", &[1]), ("g2", &[2])]);
        core.activate(2, None, &adapter); // both pinned now
        core.set_retain_enabled(false, &adapter); // collapse keeps displayed (2)
        assert_eq!(core.workspaces().count(), 1);

        core.activate(1, None, &adapter); // replace semantics
        assert_eq!(core.workspaces().count(), 1);
        assert_eq!(core.displayed(), 1);
        assert!(!core.is_retained(&1));
        assert!(core.groups().is_empty());
    }

    #[test]
    fn rekey_collision_lets_the_active_group_win_in_place() {
        let (mut core, mut adapter) = setup(&[("g1", &[1]), ("g2", &[2])]);
        core.add(2, &adapter);
        core.activate(2, None, &adapter); // active in g2

        // The workspace itself moved onto g2's identity first (in zed: the
        // Project emitted WorktreePathsChanged before the rekey).
        adapter.keys.insert(1, "g2".to_string());

        // Rename g1's project onto g2's identity.
        core.rekey_group(&"g1".to_string(), &"g2".to_string(), &adapter);

        let keys: Vec<&String> = core.groups().iter().map(|g| &g.key).collect();
        assert_eq!(keys, [&"g2".to_string()]);
        core.assert_integrity(&adapter).unwrap();
    }

    #[test]
    fn rekey_without_collision_preserves_sidebar_order() {
        let (mut core, mut adapter) = setup(&[("g1", &[1]), ("g2", &[2])]);
        core.add(2, &adapter);
        core.activate(2, None, &adapter); // pins ws1 too; rows: [g2, g1]

        // The workspace's live key changes along with the rename.
        adapter.keys.insert(2, "renamed".to_string());
        core.rekey_group(&"g2".to_string(), &"renamed".to_string(), &adapter);

        let keys: Vec<&str> = core.groups().iter().map(|g| g.key.as_str()).collect();
        assert_eq!(keys, ["g1", "renamed"], "winner keeps its sidebar slot");
        core.assert_integrity(&adapter).unwrap();
    }

    #[test]
    fn removal_prefers_same_group_member_over_neighbors() {
        let (mut core, adapter) = setup(&[("g1", &[1, 2]), ("g2", &[3])]);
        core.add(2, &adapter);
        core.add(3, &adapter);

        let session = core
            .begin_removal(vec![1], RemovalIntent::KeepProject, &adapter)
            .unwrap();
        let outcome = session.commit(&mut core, &[true], &adapter, || 99);

        assert!(!outcome.aborted);
        assert_eq!(outcome.replacement, Some(2));
        assert_eq!(core.displayed(), 2);
        assert!(outcome.reopen_key.is_none(), "same-group member found");
    }

    #[test]
    fn removal_without_candidates_creates_empty_and_reports_reopen() {
        let (mut core, adapter) = setup(&[("g1", &[1]), ("empty", &[])]);
        // "empty" group row exists but has no members.

        let session = core
            .begin_removal(vec![1], RemovalIntent::KeepProject, &adapter)
            .unwrap();
        let outcome = session.commit(&mut core, &[true], &adapter, || 42);

        assert_eq!(outcome.replacement, Some(42));
        assert_eq!(outcome.reopen_key, Some("g1".to_string()));
        assert_eq!(core.displayed(), 42);
    }

    #[test]
    fn denied_consent_leaves_state_untouched() {
        let (mut core, adapter) = setup(&[("g1", &[1])]);
        core.take_events();

        let session = core
            .begin_removal(vec![1], RemovalIntent::KeepProject, &adapter)
            .unwrap();
        let outcome = session.commit(&mut core, &[false], &adapter, || 99);

        assert!(outcome.aborted);
        assert_eq!(core.displayed(), 1);
        assert!(core.events().is_empty(), "abort must not emit events");
    }

    #[test]
    fn group_removal_pins_transient_displayed_first() {
        let (mut core, adapter) = setup(&[("g1", &[1, 2])]);
        core.add(2, &adapter);
        // Displayed workspace 1 is still transient (never navigated away).

        let session = core.begin_group_removal(&"g1".to_string(), &adapter).unwrap();
        let outcome = session.commit(&mut core, &[true, true], &adapter, || 77);

        assert!(outcome.removed_any);
        // The factory replacement gets its own group row (key "g77" derived
        // from its id), mirroring how activating a workspace pins it.
        assert_eq!(core.groups().len(), 1);
        assert_eq!(core.displayed(), 77);
        assert_eq!(core.members_of_group(&"g1".to_string(), &adapter).len(), 0);
    }

    #[test]
    fn collapse_keeps_only_the_displayed_workspace() {
        let (mut core, adapter) = setup(&[("g1", &[1]), ("g2", &[2])]);
        core.activate(2, None, &adapter);
        core.collapse_to_single();

        assert_eq!(core.workspaces().count(), 1);
        assert_eq!(core.displayed(), 2);
        assert!(core.groups().is_empty());
        core.assert_integrity(&adapter).unwrap();
    }

    #[test]
    fn integrity_detects_missing_group_row() {
        let (core, adapter) = setup(&[("g1", &[1]), ("g2", &[2])]);
        let mut core = core;
        core.add(2, &adapter); // pins ws2 under group g2
        assert!(core.assert_integrity(&adapter).is_ok());

        let mut broken_adapter = TestAdapter::default();
        broken_adapter.keys.insert(2, "orphan".to_string());
        assert!(core.assert_integrity(&broken_adapter).is_err());
    }
}
