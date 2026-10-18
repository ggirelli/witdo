use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{Status, Task, TaskId};

/// A workspace's tasks, with the links between them indexed both ways.
///
/// Tasks loaded from disk may have problems (missing tasks, cycles); the set holds them anyway
/// and reports them with [`TaskSet::problems`]. Changes made through [`TaskSet::insert`] are
/// checked so that they never add a problem.
#[derive(Clone, Debug, Default)]
pub struct TaskSet {
    tasks: BTreeMap<TaskId, Task>,
    /// Children by parent, leaving out tasks on a parent cycle (they show at the top level).
    children: HashMap<TaskId, Vec<TaskId>>,
    /// Tasks by the prerequisites they wait for.
    dependents: HashMap<TaskId, Vec<TaskId>>,
    parent_cycles: Vec<Vec<TaskId>>,
    dependency_cycles: Vec<Vec<TaskId>>,
    on_parent_cycle: HashSet<TaskId>,
    on_dependency_cycle: HashSet<TaskId>,
}

/// Something wrong with how loaded tasks link to each other. Tasks with problems still load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    MissingParent {
        task: TaskId,
        parent: TaskId,
    },
    MissingPrerequisite {
        task: TaskId,
        prerequisite: TaskId,
    },
    /// Tasks that are each other's ancestors, in order along the loop.
    ParentCycle {
        tasks: Vec<TaskId>,
    },
    /// Tasks that wait for each other, sorted by ID.
    DependencyCycle {
        tasks: Vec<TaskId>,
    },
}

/// Why a change to a task was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("A task can’t be its own parent.")]
    OwnParent,
    #[error("A task can’t depend on itself.")]
    OwnPrerequisite,
    #[error("That parent is already one of this task’s subtasks.")]
    ParentCycle,
    #[error("That task already waits for this one, directly or through others.")]
    DependencyCycle,
    #[error("There is no task with the ID {0}.")]
    UnknownTask(TaskId),
}

/// The state of a task's prerequisite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrerequisiteState {
    Open,
    Done,
    /// Does not block, but is worth a look.
    Cancelled,
    /// Not in the workspace: the task refers to an unknown ID.
    Missing,
}

/// What to do with a task's subtasks when it is deleted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SubtaskChoice {
    #[default]
    MoveToTopLevel,
    Delete,
}

/// Everything deleting a task changes. `delete` starts with the task itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeletionPlan {
    pub delete: Vec<TaskId>,
    /// Subtasks that lose their parent and move to the top level.
    pub move_to_top_level: Vec<TaskId>,
    /// Tasks that depend on a deleted task, and lose it from their prerequisites.
    pub unlink: Vec<TaskId>,
}

/// Unfinished work under or before a task, worth a warning (never a refusal) when completing
/// it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompletionWarnings {
    pub open_prerequisites: Vec<TaskId>,
    pub open_subtasks: Vec<TaskId>,
}

impl CompletionWarnings {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.open_prerequisites.is_empty() && self.open_subtasks.is_empty()
    }
}

impl TaskSet {
    /// A set of tasks as loaded, problems and all. A task whose ID repeats replaces the earlier
    /// one; callers report duplicate IDs before building the set.
    #[must_use]
    pub fn from_tasks(tasks: impl IntoIterator<Item = Task>) -> Self {
        let mut set = Self {
            tasks: tasks.into_iter().map(|task| (task.id, task)).collect(),
            ..Self::default()
        };
        set.reindex();
        set
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    #[must_use]
    pub fn get(&self, id: TaskId) -> Option<&Task> {
        self.tasks.get(&id)
    }

    #[must_use]
    pub fn contains(&self, id: TaskId) -> bool {
        self.tasks.contains_key(&id)
    }

    /// All tasks, oldest ID first.
    pub fn iter(&self) -> impl Iterator<Item = &Task> {
        self.tasks.values()
    }

    /// Checks that saving `task` (new, or a changed version of a task in the set) adds no
    /// problem. Problems the task already has are left alone, so a task loaded with problems
    /// can still be edited.
    ///
    /// # Errors
    ///
    /// When the change would link the task to itself, to an unknown task, or into a cycle.
    pub fn check(&self, task: &Task) -> Result<(), EditError> {
        let old = self.tasks.get(&task.id);
        if let Some(parent) = task.parent_id
            && old.and_then(|old| old.parent_id) != Some(parent)
        {
            if parent == task.id {
                return Err(EditError::OwnParent);
            }
            if !self.contains(parent) {
                return Err(EditError::UnknownTask(parent));
            }
            if self
                .ancestors_of(parent)
                .any(|ancestor| ancestor == task.id)
            {
                return Err(EditError::ParentCycle);
            }
        }
        let old_prerequisites = old.map_or(&[][..], |old| &old.depends_on[..]);
        for &prerequisite in &task.depends_on {
            if old_prerequisites.contains(&prerequisite) {
                continue;
            }
            if prerequisite == task.id {
                return Err(EditError::OwnPrerequisite);
            }
            if !self.contains(prerequisite) {
                return Err(EditError::UnknownTask(prerequisite));
            }
            if self.waits_for(prerequisite, task.id) {
                return Err(EditError::DependencyCycle);
            }
        }
        Ok(())
    }

    /// Adds a task or replaces its earlier version, after [checking](Self::check) it. Repeated
    /// prerequisites are dropped.
    ///
    /// # Errors
    ///
    /// As for [`TaskSet::check`]; the set is unchanged.
    pub fn insert(&mut self, mut task: Task) -> Result<(), EditError> {
        self.check(&task)?;
        let mut seen = HashSet::new();
        task.depends_on.retain(|id| seen.insert(*id));
        self.tasks.insert(task.id, task);
        self.reindex();
        Ok(())
    }

    /// Removes a task, leaving any links to it dangling; see [`TaskSet::deletion_plan`] for
    /// what else to change.
    pub fn remove(&mut self, id: TaskId) -> Option<Task> {
        let task = self.tasks.remove(&id);
        if task.is_some() {
            self.reindex();
        }
        task
    }

    /// The task's parent as shown in the hierarchy: none for top-level tasks, tasks whose
    /// parent is missing, and tasks on a parent cycle.
    #[must_use]
    pub fn parent(&self, id: TaskId) -> Option<TaskId> {
        if self.on_parent_cycle.contains(&id) {
            return None;
        }
        self.tasks
            .get(&id)?
            .parent_id
            .filter(|parent| self.contains(*parent))
    }

    /// The task's subtasks, oldest first.
    #[must_use]
    pub fn children(&self, id: TaskId) -> &[TaskId] {
        self.children.get(&id).map_or(&[], Vec::as_slice)
    }

    /// Top-level tasks (those without a [parent](Self::parent)), oldest first.
    pub fn roots(&self) -> impl Iterator<Item = TaskId> + '_ {
        self.tasks
            .keys()
            .copied()
            .filter(|&id| self.parent(id).is_none())
    }

    /// The task's ancestors as shown in the hierarchy, nearest first.
    pub fn ancestors(&self, id: TaskId) -> impl Iterator<Item = TaskId> + '_ {
        std::iter::successors(self.parent(id), |&id| self.parent(id))
    }

    /// The task's subtasks, their subtasks and so on, depth first.
    #[must_use]
    pub fn descendants(&self, id: TaskId) -> Vec<TaskId> {
        let mut found = Vec::new();
        let mut stack: Vec<TaskId> = self.children(id).iter().rev().copied().collect();
        while let Some(next) = stack.pop() {
            found.push(next);
            stack.extend(self.children(next).iter().rev());
        }
        found
    }

    /// The task's prerequisites and their states, in the order the task lists them.
    #[must_use]
    pub fn prerequisites(&self, id: TaskId) -> Vec<(TaskId, PrerequisiteState)> {
        let Some(task) = self.tasks.get(&id) else {
            return Vec::new();
        };
        task.depends_on
            .iter()
            .map(|&prerequisite| {
                let state = match self.tasks.get(&prerequisite).map(|task| task.status) {
                    None => PrerequisiteState::Missing,
                    Some(Status::Done) => PrerequisiteState::Done,
                    Some(Status::Cancelled) => PrerequisiteState::Cancelled,
                    Some(_) => PrerequisiteState::Open,
                };
                (prerequisite, state)
            })
            .collect()
    }

    /// The tasks that wait for this one ("blocks"), oldest first.
    #[must_use]
    pub fn dependents(&self, id: TaskId) -> &[TaskId] {
        self.dependents.get(&id).map_or(&[], Vec::as_slice)
    }

    /// Whether any of the task's prerequisites is still open. Cancelled and missing
    /// prerequisites do not block.
    #[must_use]
    pub fn is_blocked(&self, id: TaskId) -> bool {
        self.prerequisites(id)
            .iter()
            .any(|(_, state)| *state == PrerequisiteState::Open)
    }

    /// Whether a prerequisite was cancelled, which the UI flags for review.
    #[must_use]
    pub fn has_cancelled_prerequisite(&self, id: TaskId) -> bool {
        self.prerequisites(id)
            .iter()
            .any(|(_, state)| *state == PrerequisiteState::Cancelled)
    }

    #[must_use]
    pub fn is_on_parent_cycle(&self, id: TaskId) -> bool {
        self.on_parent_cycle.contains(&id)
    }

    #[must_use]
    pub fn is_on_dependency_cycle(&self, id: TaskId) -> bool {
        self.on_dependency_cycle.contains(&id)
    }

    /// Every problem in how the tasks link to each other: missing tasks first, by task, then
    /// cycles.
    #[must_use]
    pub fn problems(&self) -> Vec<Problem> {
        let mut problems = Vec::new();
        for task in self.tasks.values() {
            if let Some(parent) = task.parent_id
                && !self.contains(parent)
            {
                problems.push(Problem::MissingParent {
                    task: task.id,
                    parent,
                });
            }
            for &prerequisite in &task.depends_on {
                if !self.contains(prerequisite) {
                    problems.push(Problem::MissingPrerequisite {
                        task: task.id,
                        prerequisite,
                    });
                }
            }
        }
        problems.extend(self.parent_cycles.iter().map(|tasks| Problem::ParentCycle {
            tasks: tasks.clone(),
        }));
        problems.extend(
            self.dependency_cycles
                .iter()
                .map(|tasks| Problem::DependencyCycle {
                    tasks: tasks.clone(),
                }),
        );
        problems
    }

    /// What deleting the task changes, given what to do with its subtasks.
    #[must_use]
    pub fn deletion_plan(&self, id: TaskId, subtasks: SubtaskChoice) -> DeletionPlan {
        if !self.contains(id) {
            return DeletionPlan::default();
        }
        let direct_children = self.tasks_with_parent(id);
        let mut plan = DeletionPlan {
            delete: vec![id],
            ..DeletionPlan::default()
        };
        match subtasks {
            SubtaskChoice::MoveToTopLevel => plan.move_to_top_level = direct_children,
            SubtaskChoice::Delete => {
                // Follows parent_id itself rather than the hierarchy, so that subtasks on a
                // parent cycle go too; `seen` stops at the cycle.
                let mut seen: HashSet<TaskId> = HashSet::from([id]);
                let mut stack = direct_children;
                stack.reverse();
                while let Some(next) = stack.pop() {
                    if seen.insert(next) {
                        plan.delete.push(next);
                        let mut children = self.tasks_with_parent(next);
                        children.reverse();
                        stack.extend(children);
                    }
                }
            }
        }
        let deleted: HashSet<TaskId> = plan.delete.iter().copied().collect();
        plan.unlink = self
            .tasks
            .values()
            .filter(|task| !deleted.contains(&task.id))
            .filter(|task| task.depends_on.iter().any(|p| deleted.contains(p)))
            .map(|task| task.id)
            .collect();
        plan
    }

    /// Open prerequisites and open subtasks (at any depth) of the task.
    #[must_use]
    pub fn completion_warnings(&self, id: TaskId) -> CompletionWarnings {
        CompletionWarnings {
            open_prerequisites: self
                .prerequisites(id)
                .into_iter()
                .filter(|(_, state)| *state == PrerequisiteState::Open)
                .map(|(prerequisite, _)| prerequisite)
                .collect(),
            open_subtasks: self
                .descendants(id)
                .into_iter()
                .filter(|&subtask| self.tasks[&subtask].is_open())
                .collect(),
        }
    }

    fn tasks_with_parent(&self, parent: TaskId) -> Vec<TaskId> {
        self.tasks
            .values()
            .filter(|task| task.parent_id == Some(parent))
            .map(|task| task.id)
            .collect()
    }

    /// Walks up `parent_id` links as stored, stopping at a missing task or a repeat.
    fn ancestors_of(&self, id: TaskId) -> impl Iterator<Item = TaskId> + '_ {
        let mut seen = HashSet::new();
        std::iter::successors(Some(id), |id| self.tasks.get(id)?.parent_id)
            .take_while(move |id| seen.insert(*id))
    }

    /// Whether `from` waits for `target`, directly or through other prerequisites.
    fn waits_for(&self, from: TaskId, target: TaskId) -> bool {
        let mut seen = HashSet::new();
        let mut stack = vec![from];
        while let Some(next) = stack.pop() {
            if next == target {
                return true;
            }
            if seen.insert(next)
                && let Some(task) = self.tasks.get(&next)
            {
                stack.extend(&task.depends_on);
            }
        }
        false
    }

    fn reindex(&mut self) {
        self.parent_cycles = self.find_parent_cycles();
        self.on_parent_cycle = self.parent_cycles.iter().flatten().copied().collect();
        self.dependency_cycles = self.find_dependency_cycles();
        self.on_dependency_cycle = self.dependency_cycles.iter().flatten().copied().collect();

        self.children.clear();
        self.dependents.clear();
        for task in self.tasks.values() {
            if let Some(parent) = self.parent(task.id) {
                self.children.entry(parent).or_default().push(task.id);
            }
            let mut seen = HashSet::new();
            for &prerequisite in &task.depends_on {
                if self.tasks.contains_key(&prerequisite) && seen.insert(prerequisite) {
                    self.dependents
                        .entry(prerequisite)
                        .or_default()
                        .push(task.id);
                }
            }
        }
    }

    /// Each task has at most one parent, so cycles are found by walking up from every task
    /// and noticing when a walk comes back to a task it passed.
    fn find_parent_cycles(&self) -> Vec<Vec<TaskId>> {
        let mut cycles = Vec::new();
        let mut done: HashSet<TaskId> = HashSet::new();
        for &start in self.tasks.keys() {
            let mut path: Vec<TaskId> = Vec::new();
            let mut current = Some(start);
            while let Some(id) = current {
                if done.contains(&id) {
                    break;
                }
                if let Some(position) = path.iter().position(|&seen| seen == id) {
                    cycles.push(path[position..].to_vec());
                    break;
                }
                path.push(id);
                current = self
                    .tasks
                    .get(&id)
                    .and_then(|task| task.parent_id)
                    .filter(|parent| self.tasks.contains_key(parent));
            }
            done.extend(path);
        }
        cycles
    }

    /// Strongly connected components of the prerequisite graph (Tarjan's algorithm), keeping
    /// those that are cycles: more than one task, or a task that waits for itself.
    fn find_dependency_cycles(&self) -> Vec<Vec<TaskId>> {
        struct Tarjan<'a> {
            set: &'a TaskSet,
            next_index: usize,
            index: HashMap<TaskId, usize>,
            low: HashMap<TaskId, usize>,
            stack: Vec<TaskId>,
            on_stack: HashSet<TaskId>,
            cycles: Vec<Vec<TaskId>>,
        }

        impl Tarjan<'_> {
            fn visit(&mut self, id: TaskId) {
                self.index.insert(id, self.next_index);
                self.low.insert(id, self.next_index);
                self.next_index += 1;
                self.stack.push(id);
                self.on_stack.insert(id);
                let prerequisites = &self.set.tasks[&id].depends_on;
                for &next in prerequisites {
                    if !self.set.tasks.contains_key(&next) {
                        continue;
                    }
                    if !self.index.contains_key(&next) {
                        self.visit(next);
                        let low = self.low[&id].min(self.low[&next]);
                        self.low.insert(id, low);
                    } else if self.on_stack.contains(&next) {
                        let low = self.low[&id].min(self.index[&next]);
                        self.low.insert(id, low);
                    }
                }
                if self.low[&id] == self.index[&id] {
                    let mut component = Vec::new();
                    while let Some(member) = self.stack.pop() {
                        self.on_stack.remove(&member);
                        component.push(member);
                        if member == id {
                            break;
                        }
                    }
                    if component.len() > 1 || prerequisites.contains(&id) {
                        component.sort();
                        self.cycles.push(component);
                    }
                }
            }
        }

        let mut tarjan = Tarjan {
            set: self,
            next_index: 0,
            index: HashMap::new(),
            low: HashMap::new(),
            stack: Vec::new(),
            on_stack: HashSet::new(),
            cycles: Vec::new(),
        };
        for &id in self.tasks.keys() {
            if !tarjan.index.contains_key(&id) {
                tarjan.visit(id);
            }
        }
        let mut cycles = tarjan.cycles;
        cycles.sort();
        cycles
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use ulid::Ulid;

    use super::*;
    use crate::Title;

    /// Task `n`: IDs sort by `n`, so "oldest first" means "by number".
    fn id(n: u64) -> TaskId {
        TaskId::from_ulid(Ulid::from_parts(n, 0))
    }

    fn task(n: u64) -> Task {
        Task::capture(
            id(n),
            Title::new(&format!("Task {n}")).unwrap(),
            Timestamp::UNIX_EPOCH,
        )
    }

    fn with_parent(n: u64, parent: u64) -> Task {
        let mut task = task(n);
        task.parent_id = Some(id(parent));
        task
    }

    fn with_prerequisites(n: u64, prerequisites: &[u64]) -> Task {
        waiting_for(task(n), prerequisites)
    }

    fn waiting_for(mut task: Task, prerequisites: &[u64]) -> Task {
        task.depends_on = ids(prerequisites);
        task
    }

    fn with_status(mut task: Task, status: Status) -> Task {
        task.set_status(status, Timestamp::UNIX_EPOCH);
        task
    }

    fn ids(ns: &[u64]) -> Vec<TaskId> {
        ns.iter().map(|&n| id(n)).collect()
    }

    #[test]
    fn indexes_the_hierarchy_both_ways() {
        let set = TaskSet::from_tasks([
            task(1),
            with_parent(2, 1),
            with_parent(3, 1),
            with_parent(4, 2),
        ]);
        assert_eq!(set.children(id(1)), ids(&[2, 3]));
        assert_eq!(set.parent(id(4)), Some(id(2)));
        assert_eq!(set.ancestors(id(4)).collect::<Vec<_>>(), ids(&[2, 1]));
        assert_eq!(set.descendants(id(1)), ids(&[2, 4, 3]));
        assert_eq!(set.roots().collect::<Vec<_>>(), ids(&[1]));
        assert_eq!(set.problems(), []);
    }

    #[test]
    fn indexes_dependencies_both_ways() {
        let set = TaskSet::from_tasks([
            task(1),
            task(2),
            with_prerequisites(3, &[1, 2]),
            with_prerequisites(4, &[1]),
        ]);
        assert_eq!(
            set.prerequisites(id(3)),
            [
                (id(1), PrerequisiteState::Open),
                (id(2), PrerequisiteState::Open)
            ]
        );
        assert_eq!(set.dependents(id(1)), ids(&[3, 4]));
        assert_eq!(set.dependents(id(3)), []);
    }

    #[test]
    fn a_task_is_blocked_while_a_prerequisite_is_open() {
        let mut set = TaskSet::from_tasks([task(1), task(2), with_prerequisites(3, &[1, 2])]);
        assert!(set.is_blocked(id(3)));
        assert!(!set.is_blocked(id(1)));

        set.insert(with_status(task(1), Status::Done)).unwrap();
        assert!(set.is_blocked(id(3)), "task 2 is still open");

        set.insert(with_status(task(2), Status::Done)).unwrap();
        assert!(!set.is_blocked(id(3)));
    }

    #[test]
    fn a_cancelled_prerequisite_does_not_block_but_is_flagged() {
        let set = TaskSet::from_tasks([
            with_status(task(1), Status::Cancelled),
            with_prerequisites(2, &[1]),
        ]);
        assert!(!set.is_blocked(id(2)));
        assert!(set.has_cancelled_prerequisite(id(2)));
    }

    #[test]
    fn missing_tasks_are_problems_but_do_not_block() {
        let set = TaskSet::from_tasks([with_parent(1, 8), with_prerequisites(2, &[9])]);
        assert_eq!(
            set.problems(),
            [
                Problem::MissingParent {
                    task: id(1),
                    parent: id(8)
                },
                Problem::MissingPrerequisite {
                    task: id(2),
                    prerequisite: id(9)
                },
            ]
        );
        assert_eq!(set.parent(id(1)), None, "shown at the top level");
        assert_eq!(
            set.prerequisites(id(2)),
            [(id(9), PrerequisiteState::Missing)]
        );
        assert!(!set.is_blocked(id(2)));
    }

    #[test]
    fn parent_cycles_on_disk_load_and_show_at_the_top_level() {
        let set = TaskSet::from_tasks([
            with_parent(1, 3),
            with_parent(2, 1),
            with_parent(3, 2),
            with_parent(4, 1),
        ]);
        assert_eq!(
            set.problems(),
            [Problem::ParentCycle {
                tasks: ids(&[1, 3, 2])
            }]
        );
        for n in [1, 2, 3] {
            assert!(set.is_on_parent_cycle(id(n)));
            assert_eq!(set.parent(id(n)), None);
        }
        assert_eq!(set.roots().collect::<Vec<_>>(), ids(&[1, 2, 3]));
        assert_eq!(
            set.children(id(1)),
            ids(&[4]),
            "subtasks off the cycle still nest"
        );
    }

    #[test]
    fn a_task_that_is_its_own_parent_is_a_cycle() {
        let set = TaskSet::from_tasks([with_parent(1, 1)]);
        assert_eq!(set.problems(), [Problem::ParentCycle { tasks: ids(&[1]) }]);
        assert_eq!(set.roots().collect::<Vec<_>>(), ids(&[1]));
    }

    #[test]
    fn dependency_cycles_on_disk_are_flagged_on_each_task() {
        let set = TaskSet::from_tasks([
            with_prerequisites(1, &[2]),
            with_prerequisites(2, &[3]),
            with_prerequisites(3, &[1]),
            with_prerequisites(4, &[4]),
            with_prerequisites(5, &[1]),
        ]);
        assert_eq!(
            set.problems(),
            [
                Problem::DependencyCycle {
                    tasks: ids(&[1, 2, 3])
                },
                Problem::DependencyCycle { tasks: ids(&[4]) },
            ]
        );
        assert!(set.is_on_dependency_cycle(id(2)));
        assert!(
            !set.is_on_dependency_cycle(id(5)),
            "waits for the cycle, but is not on it"
        );
    }

    #[test]
    fn refuses_to_make_a_task_its_own_parent_or_prerequisite() {
        let mut set = TaskSet::from_tasks([task(1)]);
        assert_eq!(set.insert(with_parent(1, 1)), Err(EditError::OwnParent));
        assert_eq!(
            set.insert(with_prerequisites(1, &[1])),
            Err(EditError::OwnPrerequisite)
        );
        assert_eq!(set.get(id(1)), Some(&task(1)), "unchanged");
    }

    #[test]
    fn refuses_links_to_unknown_tasks() {
        let mut set = TaskSet::from_tasks([task(1)]);
        assert_eq!(
            set.insert(with_parent(1, 9)),
            Err(EditError::UnknownTask(id(9)))
        );
        assert_eq!(
            set.insert(with_prerequisites(1, &[9])),
            Err(EditError::UnknownTask(id(9)))
        );
    }

    #[test]
    fn refuses_parent_cycles() {
        let mut set = TaskSet::from_tasks([task(1), with_parent(2, 1), with_parent(3, 2)]);
        assert_eq!(set.insert(with_parent(1, 3)), Err(EditError::ParentCycle));
        assert_eq!(set.insert(with_parent(1, 2)), Err(EditError::ParentCycle));
        assert_eq!(
            set.insert(with_parent(3, 1)),
            Ok(()),
            "moving within the tree is fine"
        );
    }

    #[test]
    fn refuses_dependency_cycles() {
        let mut set = TaskSet::from_tasks([
            task(1),
            with_prerequisites(2, &[1]),
            with_prerequisites(3, &[2]),
        ]);
        assert_eq!(
            set.insert(with_prerequisites(1, &[3])),
            Err(EditError::DependencyCycle)
        );
        assert_eq!(
            set.insert(with_prerequisites(1, &[2])),
            Err(EditError::DependencyCycle)
        );
        assert_eq!(
            set.insert(with_prerequisites(3, &[1, 2])),
            Ok(()),
            "a shortcut is not a cycle"
        );
    }

    #[test]
    fn tasks_with_problems_can_still_be_edited() {
        let mut set = TaskSet::from_tasks([
            with_parent(1, 2),
            with_parent(2, 1),
            with_prerequisites(3, &[3, 9]),
        ]);
        let mut renamed = with_parent(1, 2);
        renamed.title = Title::new("Renamed").unwrap();
        assert_eq!(set.insert(renamed), Ok(()));

        let mut edited = with_prerequisites(3, &[3, 9]);
        edited.body = "Notes".into();
        assert_eq!(set.insert(edited), Ok(()));

        assert_eq!(
            set.insert(with_prerequisites(3, &[3, 9, 8])),
            Err(EditError::UnknownTask(id(8))),
            "but not given new problems"
        );
    }

    #[test]
    fn insert_drops_repeated_prerequisites() {
        let mut set = TaskSet::from_tasks([task(1), task(2)]);
        set.insert(with_prerequisites(3, &[1, 2, 1])).unwrap();
        assert_eq!(set.get(id(3)).unwrap().depends_on, ids(&[1, 2]));
    }

    #[test]
    fn deleting_moves_subtasks_to_the_top_level_by_default() {
        let set = TaskSet::from_tasks([
            task(1),
            with_parent(2, 1),
            with_parent(3, 2),
            with_parent(4, 1),
        ]);
        assert_eq!(
            set.deletion_plan(id(1), SubtaskChoice::default()),
            DeletionPlan {
                delete: ids(&[1]),
                move_to_top_level: ids(&[2, 4]),
                unlink: vec![],
            }
        );
    }

    #[test]
    fn deleting_can_take_the_subtasks_too() {
        let set = TaskSet::from_tasks([
            task(1),
            with_parent(2, 1),
            with_parent(3, 2),
            with_parent(4, 1),
        ]);
        assert_eq!(
            set.deletion_plan(id(1), SubtaskChoice::Delete).delete,
            ids(&[1, 2, 3, 4])
        );
    }

    #[test]
    fn deleting_unlinks_dependents_outside_what_is_deleted() {
        let set = TaskSet::from_tasks([
            task(1),
            with_parent(2, 1),
            waiting_for(with_parent(3, 1), &[1, 2]),
            with_prerequisites(4, &[2]),
            with_prerequisites(5, &[1]),
        ]);
        assert_eq!(
            set.deletion_plan(id(1), SubtaskChoice::Delete).unlink,
            ids(&[4, 5])
        );
        assert_eq!(
            set.deletion_plan(id(1), SubtaskChoice::MoveToTopLevel)
                .unlink,
            ids(&[3, 5])
        );
    }

    #[test]
    fn deleting_through_a_parent_cycle_stops() {
        let set = TaskSet::from_tasks([with_parent(1, 2), with_parent(2, 1)]);
        assert_eq!(
            set.deletion_plan(id(1), SubtaskChoice::Delete).delete,
            ids(&[1, 2])
        );
    }

    #[test]
    fn deleting_an_unknown_task_changes_nothing() {
        let set = TaskSet::from_tasks([task(1)]);
        assert_eq!(
            set.deletion_plan(id(9), SubtaskChoice::Delete),
            DeletionPlan::default()
        );
    }

    #[test]
    fn completion_warns_about_open_prerequisites_and_subtasks() {
        let set = TaskSet::from_tasks([
            task(1),
            with_status(task(2), Status::Done),
            with_prerequisites(3, &[1, 2]),
            with_parent(4, 3),
            with_status(with_parent(5, 3), Status::Cancelled),
            with_parent(6, 4),
        ]);
        let warnings = set.completion_warnings(id(3));
        assert_eq!(warnings.open_prerequisites, ids(&[1]));
        assert_eq!(warnings.open_subtasks, ids(&[4, 6]));
        assert!(set.completion_warnings(id(1)).is_empty());
    }

    #[test]
    fn removing_leaves_links_dangling() {
        let mut set =
            TaskSet::from_tasks([task(1), with_parent(2, 1), with_prerequisites(3, &[1])]);
        assert_eq!(set.remove(id(1)).map(|task| task.id), Some(id(1)));
        assert_eq!(set.len(), 2);
        assert_eq!(set.problems().len(), 2);
        assert_eq!(set.remove(id(1)), None);
    }
}
