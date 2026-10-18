//! The task model: tasks, their hierarchy and dependencies, and the views over them. It knows
//! nothing about files or the UI.

mod graph;
mod id;
mod task;

pub use graph::{
    CompletionWarnings, DeletionPlan, EditError, PrerequisiteState, Problem, SubtaskChoice, TaskSet,
};
pub use id::{InvalidId, TaskId};
pub use task::{Category, EmptyText, Priority, Status, Tag, Task, Title, UnknownValue};
