use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeSet, HashSet};

use jiff::ToSpan;
use jiff::civil::Date;

use crate::{Category, Priority, Status, Tag, Task, TaskId, TaskSet};

/// A built-in list of tasks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    /// Tasks waiting to be triaged: status `inbox`.
    Inbox,
    /// What to do now: open, triaged (not `inbox`), and not blocked.
    Next,
    /// Open tasks due today or earlier.
    Due,
    /// All open tasks.
    Open,
    /// Closed tasks, most recently closed first.
    Completed,
    /// Tasks in a category, or with none.
    Category(Option<Category>),
    /// Every task.
    All,
}

impl View {
    fn includes(&self, tasks: &TaskSet, task: &Task, today: Date) -> bool {
        match self {
            Self::Inbox => task.status == Status::Inbox,
            Self::Next => {
                task.is_open() && task.status != Status::Inbox && !tasks.is_blocked(task.id)
            }
            Self::Due => task.is_open() && task.due_date.is_some_and(|due| due <= today),
            Self::Open => task.is_open(),
            Self::Completed => !task.is_open(),
            Self::Category(category) => task.category == *category,
            Self::All => true,
        }
    }

    /// The order tasks are listed in: closed tasks most recent first in `Completed`; anywhere
    /// else, soonest due first (undated last), then most urgent, then oldest.
    fn compare(&self, a: &Task, b: &Task) -> Ordering {
        match self {
            Self::Completed => Reverse(a.completed_at)
                .cmp(&Reverse(b.completed_at))
                .then(a.id.cmp(&b.id)),
            _ => (a.due_date.is_none(), a.due_date, Reverse(a.priority), a.id).cmp(&(
                b.due_date.is_none(),
                b.due_date,
                Reverse(b.priority),
                b.id,
            )),
        }
    }
}

/// Narrows any view. Every part that is set must match (AND); within a part, any listed value
/// matches (OR), except tags, which must all be present.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    /// Matched, ignoring case, anywhere in the title or notes.
    pub text: String,
    pub statuses: Vec<Status>,
    pub priorities: Vec<Priority>,
    pub due: Option<DueFilter>,
    pub category: Option<CategoryFilter>,
    pub tags: Vec<Tag>,
    pub parent: Option<ParentFilter>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DueFilter {
    /// Open and due before today.
    Overdue,
    Today,
    /// Due this week, Monday to Sunday.
    ThisWeek,
    NoDate,
    /// Due between two dates, both included; a missing end is open.
    Between {
        from: Option<Date>,
        to: Option<Date>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CategoryFilter {
    Is(Category),
    Uncategorized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParentFilter {
    TopLevel,
    /// A task and everything under it.
    SubtreeOf(TaskId),
}

impl Filter {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn matches(&self, tasks: &TaskSet, task: &Task, today: Date) -> bool {
        self.matches_text(task)
            && (self.statuses.is_empty() || self.statuses.contains(&task.status))
            && (self.priorities.is_empty() || self.priorities.contains(&task.priority))
            && self.due.is_none_or(|due| due.matches(task, today))
            && self
                .category
                .as_ref()
                .is_none_or(|category| match category {
                    CategoryFilter::Is(category) => task.category.as_ref() == Some(category),
                    CategoryFilter::Uncategorized => task.category.is_none(),
                })
            && self.tags.iter().all(|tag| task.has_tag(tag))
            && self.parent.is_none_or(|parent| match parent {
                ParentFilter::TopLevel => tasks.parent(task.id).is_none(),
                ParentFilter::SubtreeOf(root) => {
                    task.id == root || tasks.ancestors(task.id).any(|ancestor| ancestor == root)
                }
            })
    }

    fn matches_text(&self, task: &Task) -> bool {
        let text = self.text.trim();
        if text.is_empty() {
            return true;
        }
        let text = text.to_lowercase();
        task.title.as_str().to_lowercase().contains(&text)
            || task.body.to_lowercase().contains(&text)
    }
}

impl DueFilter {
    fn matches(self, task: &Task, today: Date) -> bool {
        let Some(due) = task.due_date else {
            return self == Self::NoDate;
        };
        match self {
            Self::Overdue => task.is_open() && due < today,
            Self::Today => due == today,
            Self::ThisWeek => {
                let monday =
                    today.saturating_sub(i64::from(today.weekday().to_monday_zero_offset()).days());
                (monday..=monday.saturating_add(6.days())).contains(&due)
            }
            Self::NoDate => false,
            Self::Between { from, to } => {
                from.is_none_or(|from| from <= due) && to.is_none_or(|to| due <= to)
            }
        }
    }
}

/// A row of the hierarchy: a task, how deep it is, and whether it is in the view and filter or
/// only shown for context (its ancestors, and in a category view its subtasks elsewhere).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub id: TaskId,
    pub depth: usize,
    pub matches: bool,
}

impl TaskSet {
    /// The tasks in a view that match a filter, in the view's order.
    #[must_use]
    pub fn list(&self, view: &View, filter: &Filter, today: Date) -> Vec<TaskId> {
        let mut tasks: Vec<&Task> = self
            .iter()
            .filter(|task| view.includes(self, task, today) && filter.matches(self, task, today))
            .collect();
        tasks.sort_by(|a, b| view.compare(a, b));
        tasks.into_iter().map(|task| task.id).collect()
    }

    /// The tasks in a view that match a filter, nested under their parents, depth first. Their
    /// ancestors are included for context so the tree stays readable; a category view also
    /// shows their subtasks from other categories. Siblings are in the view's order.
    #[must_use]
    pub fn tree(&self, view: &View, filter: &Filter, today: Date) -> Vec<TreeRow> {
        let matching: HashSet<TaskId> = self.list(view, filter, today).into_iter().collect();
        let mut shown = matching.clone();
        for &id in &matching {
            shown.extend(self.ancestors(id));
            if matches!(view, View::Category(_)) {
                shown.extend(self.descendants(id));
            }
        }

        let sorted = |ids: &mut Vec<TaskId>| {
            ids.retain(|id| shown.contains(id));
            ids.sort_by(|a, b| view.compare(self.tasks_get(*a), self.tasks_get(*b)));
        };
        let mut roots: Vec<TaskId> = self.roots().collect();
        sorted(&mut roots);

        let mut rows = Vec::with_capacity(shown.len());
        let mut stack: Vec<(TaskId, usize)> = roots.into_iter().rev().map(|id| (id, 0)).collect();
        while let Some((id, depth)) = stack.pop() {
            rows.push(TreeRow {
                id,
                depth,
                matches: matching.contains(&id),
            });
            let mut children = self.children(id).to_vec();
            sorted(&mut children);
            stack.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
        }
        rows
    }

    /// The categories in use, sorted.
    #[must_use]
    pub fn categories(&self) -> Vec<Category> {
        let categories: BTreeSet<&Category> = self
            .iter()
            .filter_map(|task| task.category.as_ref())
            .collect();
        categories.into_iter().cloned().collect()
    }

    /// The tags in use, sorted ignoring case, each in the spelling it first appears with.
    #[must_use]
    pub fn tags(&self) -> Vec<Tag> {
        let mut seen = HashSet::new();
        let mut tags: Vec<Tag> = self
            .iter()
            .flat_map(Task::tags)
            .filter(|tag| seen.insert(tag.key()))
            .cloned()
            .collect();
        tags.sort_by_key(Tag::key);
        tags
    }

    fn tasks_get(&self, id: TaskId) -> &Task {
        self.get(id).expect("listed tasks are in the set")
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use jiff::civil::date;
    use ulid::Ulid;

    use super::*;
    use crate::Title;

    const TODAY: Date = date(2026, 10, 21); // a Wednesday

    fn id(n: u64) -> TaskId {
        TaskId::from_ulid(Ulid::from_parts(n, 0))
    }

    fn ids(ns: &[u64]) -> Vec<TaskId> {
        ns.iter().map(|&n| id(n)).collect()
    }

    /// A triaged task (`todo`), so that it shows in Next unless something says otherwise.
    fn task(n: u64) -> Task {
        let mut task = Task::capture(
            id(n),
            Title::new(&format!("Task {n}")).unwrap(),
            Timestamp::UNIX_EPOCH,
        );
        task.status = Status::Todo;
        task
    }

    fn edit(n: u64, change: impl FnOnce(&mut Task)) -> Task {
        let mut task = task(n);
        change(&mut task);
        task
    }

    fn closed(n: u64, status: Status, at: i64) -> Task {
        edit(n, |task| {
            task.set_status(status, Timestamp::from_second(at).unwrap());
        })
    }

    fn list(set: &TaskSet, view: &View, filter: &Filter) -> Vec<TaskId> {
        set.list(view, filter, TODAY)
    }

    fn all(set: &TaskSet, filter: &Filter) -> Vec<TaskId> {
        list(set, &View::All, filter)
    }

    fn category(name: &str) -> Category {
        Category::new(name).unwrap()
    }

    fn tag(name: &str) -> Tag {
        Tag::new(name).unwrap()
    }

    #[test]
    fn inbox_holds_untriaged_tasks() {
        let set = TaskSet::from_tasks([edit(1, |t| t.status = Status::Inbox), task(2)]);
        assert_eq!(list(&set, &View::Inbox, &Filter::default()), ids(&[1]));
    }

    #[test]
    fn next_holds_open_triaged_unblocked_tasks() {
        let set = TaskSet::from_tasks([
            edit(1, |t| t.status = Status::Inbox),
            task(2),
            edit(3, |t| t.status = Status::InProgress),
            closed(4, Status::Done, 1),
            edit(5, |t| t.depends_on = ids(&[2])),
            edit(6, |t| t.depends_on = ids(&[4])),
        ]);
        assert_eq!(list(&set, &View::Next, &Filter::default()), ids(&[2, 3, 6]));
    }

    #[test]
    fn due_holds_open_tasks_due_today_or_earlier() {
        let set = TaskSet::from_tasks([
            edit(1, |t| t.due_date = Some(TODAY)),
            edit(2, |t| t.due_date = Some(date(2026, 10, 1))),
            edit(3, |t| t.due_date = Some(date(2026, 10, 22))),
            task(4),
            edit(5, |t| {
                t.due_date = Some(TODAY);
                t.complete(Timestamp::UNIX_EPOCH);
            }),
        ]);
        assert_eq!(list(&set, &View::Due, &Filter::default()), ids(&[2, 1]));
    }

    #[test]
    fn open_and_completed_split_by_status() {
        let set = TaskSet::from_tasks([
            task(1),
            closed(2, Status::Done, 10),
            closed(3, Status::Cancelled, 30),
            closed(4, Status::Done, 20),
            edit(5, |t| t.status = Status::Inbox),
        ]);
        assert_eq!(list(&set, &View::Open, &Filter::default()), ids(&[1, 5]));
        assert_eq!(
            list(&set, &View::Completed, &Filter::default()),
            ids(&[3, 4, 2]),
            "most recently closed first"
        );
    }

    #[test]
    fn category_views_include_uncategorized() {
        let set = TaskSet::from_tasks([
            edit(1, |t| t.category = Some(category("Work"))),
            edit(2, |t| t.category = Some(category("work"))),
            task(3),
        ]);
        let work = View::Category(Some(category("Work")));
        assert_eq!(list(&set, &work, &Filter::default()), ids(&[1]));
        assert_eq!(
            list(&set, &View::Category(None), &Filter::default()),
            ids(&[3])
        );
        assert_eq!(set.categories(), [category("Work"), category("work")]);
    }

    #[test]
    fn lists_soonest_due_then_most_urgent_then_oldest() {
        let set = TaskSet::from_tasks([
            task(1),
            edit(2, |t| t.priority = Priority::Urgent),
            edit(3, |t| t.due_date = Some(date(2026, 11, 1))),
            edit(4, |t| t.due_date = Some(date(2026, 10, 25))),
            edit(5, |t| {
                t.due_date = Some(date(2026, 10, 25));
                t.priority = Priority::High;
            }),
            edit(6, |t| t.priority = Priority::Low),
        ]);
        assert_eq!(all(&set, &Filter::default()), ids(&[5, 4, 3, 2, 1, 6]));
    }

    #[test]
    fn text_search_ignores_case_in_titles_and_notes() {
        let set = TaskSet::from_tasks([
            edit(1, |t| t.title = Title::new("Call the BANK").unwrap()),
            edit(2, |t| t.body = "Ask the bank about the loan.\n".into()),
            task(3),
        ]);
        let filter = Filter {
            text: "  bank ".into(),
            ..Filter::default()
        };
        assert_eq!(all(&set, &filter), ids(&[1, 2]));
    }

    #[test]
    fn filters_by_status_and_priority() {
        let set = TaskSet::from_tasks([
            task(1),
            edit(2, |t| t.status = Status::InProgress),
            edit(3, |t| {
                t.status = Status::InProgress;
                t.priority = Priority::High;
            }),
        ]);
        let in_progress = Filter {
            statuses: vec![Status::InProgress],
            ..Filter::default()
        };
        assert_eq!(all(&set, &in_progress), ids(&[3, 2]));
        let high_in_progress = Filter {
            priorities: vec![Priority::High, Priority::Urgent],
            ..in_progress
        };
        assert_eq!(all(&set, &high_in_progress), ids(&[3]));
    }

    #[test]
    fn filters_by_due_date() {
        let due = |n, date| edit(n, |t| t.due_date = Some(date));
        let set = TaskSet::from_tasks([
            due(1, date(2026, 10, 19)), // Monday: overdue, this week
            due(2, TODAY),
            due(3, date(2026, 10, 25)), // Sunday: this week
            due(4, date(2026, 10, 26)), // next Monday
            due(5, date(2026, 10, 18)), // last Sunday: overdue
            task(6),
        ]);
        let by = |due| {
            all(
                &set,
                &Filter {
                    due: Some(due),
                    ..Filter::default()
                },
            )
        };
        assert_eq!(by(DueFilter::Overdue), ids(&[5, 1]));
        assert_eq!(by(DueFilter::Today), ids(&[2]));
        assert_eq!(by(DueFilter::ThisWeek), ids(&[1, 2, 3]));
        assert_eq!(by(DueFilter::NoDate), ids(&[6]));
        assert_eq!(
            by(DueFilter::Between {
                from: Some(TODAY),
                to: None
            }),
            ids(&[2, 3, 4])
        );
        assert_eq!(
            by(DueFilter::Between {
                from: None,
                to: Some(date(2026, 10, 19))
            }),
            ids(&[5, 1])
        );
    }

    #[test]
    fn closed_tasks_are_never_overdue() {
        let set = TaskSet::from_tasks([edit(1, |t| {
            t.due_date = Some(date(2026, 10, 1));
            t.complete(Timestamp::UNIX_EPOCH);
        })]);
        let overdue = Filter {
            due: Some(DueFilter::Overdue),
            ..Filter::default()
        };
        assert_eq!(all(&set, &overdue), []);
    }

    #[test]
    fn filters_by_category_and_tags() {
        let set = TaskSet::from_tasks([
            edit(1, |t| {
                t.category = Some(category("Work"));
                t.set_tags([tag("writing"), tag("q4")]);
            }),
            edit(2, |t| t.set_tags([tag("Writing")])),
            edit(3, |t| t.category = Some(category("Home"))),
        ]);
        let work = Filter {
            category: Some(CategoryFilter::Is(category("Work"))),
            ..Filter::default()
        };
        assert_eq!(all(&set, &work), ids(&[1]));
        let uncategorized = Filter {
            category: Some(CategoryFilter::Uncategorized),
            ..Filter::default()
        };
        assert_eq!(all(&set, &uncategorized), ids(&[2]));
        let writing = Filter {
            tags: vec![tag("#WRITING")],
            ..Filter::default()
        };
        assert_eq!(all(&set, &writing), ids(&[1, 2]));
        let both = Filter {
            tags: vec![tag("writing"), tag("q4")],
            ..Filter::default()
        };
        assert_eq!(all(&set, &both), ids(&[1]));
        assert_eq!(set.tags(), [tag("q4"), tag("writing")]);
    }

    #[test]
    fn filters_by_parent() {
        let set = TaskSet::from_tasks([
            task(1),
            edit(2, |t| t.parent_id = Some(id(1))),
            edit(3, |t| t.parent_id = Some(id(2))),
            task(4),
        ]);
        let top = Filter {
            parent: Some(ParentFilter::TopLevel),
            ..Filter::default()
        };
        assert_eq!(all(&set, &top), ids(&[1, 4]));
        let subtree = Filter {
            parent: Some(ParentFilter::SubtreeOf(id(2))),
            ..Filter::default()
        };
        assert_eq!(all(&set, &subtree), ids(&[2, 3]));
    }

    #[test]
    fn filters_combine_with_views() {
        let set = TaskSet::from_tasks([
            edit(1, |t| t.priority = Priority::High),
            edit(2, |t| {
                t.priority = Priority::High;
                t.status = Status::Inbox;
            }),
        ]);
        let high = Filter {
            priorities: vec![Priority::High],
            ..Filter::default()
        };
        assert_eq!(list(&set, &View::Inbox, &high), ids(&[2]));
        assert!(!high.is_empty());
        assert!(Filter::default().is_empty());
    }

    #[test]
    fn tree_nests_subtasks_depth_first() {
        let set = TaskSet::from_tasks([
            task(1),
            edit(2, |t| t.parent_id = Some(id(1))),
            edit(3, |t| t.parent_id = Some(id(2))),
            edit(4, |t| t.parent_id = Some(id(1))),
            task(5),
        ]);
        let rows = set.tree(&View::All, &Filter::default(), TODAY);
        let shape: Vec<(TaskId, usize)> = rows.iter().map(|row| (row.id, row.depth)).collect();
        assert_eq!(
            shape,
            [(id(1), 0), (id(2), 1), (id(3), 2), (id(4), 1), (id(5), 0)]
        );
        assert!(rows.iter().all(|row| row.matches));
    }

    #[test]
    fn tree_shows_ancestors_of_matches_for_context() {
        let set = TaskSet::from_tasks([
            task(1),
            edit(2, |t| t.parent_id = Some(id(1))),
            edit(3, |t| {
                t.parent_id = Some(id(2));
                t.title = Title::new("Find the receipts").unwrap();
            }),
            edit(4, |t| t.parent_id = Some(id(1))),
        ]);
        let filter = Filter {
            text: "receipts".into(),
            ..Filter::default()
        };
        assert_eq!(
            set.tree(&View::All, &filter, TODAY),
            [
                TreeRow {
                    id: id(1),
                    depth: 0,
                    matches: false
                },
                TreeRow {
                    id: id(2),
                    depth: 1,
                    matches: false
                },
                TreeRow {
                    id: id(3),
                    depth: 2,
                    matches: true
                },
            ]
        );
    }

    #[test]
    fn category_trees_show_subtasks_from_other_categories_for_context() {
        let set = TaskSet::from_tasks([
            edit(1, |t| t.category = Some(category("Home"))),
            edit(2, |t| {
                t.parent_id = Some(id(1));
                t.category = Some(category("Work"));
            }),
            edit(3, |t| t.parent_id = Some(id(2))),
        ]);
        let work = View::Category(Some(category("Work")));
        assert_eq!(
            set.tree(&work, &Filter::default(), TODAY),
            [
                TreeRow {
                    id: id(1),
                    depth: 0,
                    matches: false
                },
                TreeRow {
                    id: id(2),
                    depth: 1,
                    matches: true
                },
                TreeRow {
                    id: id(3),
                    depth: 2,
                    matches: false
                },
            ]
        );
    }

    #[test]
    fn tree_siblings_follow_the_view_order() {
        let set = TaskSet::from_tasks([
            task(1),
            edit(2, |t| t.parent_id = Some(id(1))),
            edit(3, |t| {
                t.parent_id = Some(id(1));
                t.priority = Priority::Urgent;
            }),
        ]);
        let order: Vec<TaskId> = set
            .tree(&View::All, &Filter::default(), TODAY)
            .iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(order, ids(&[1, 3, 2]));
    }
}
