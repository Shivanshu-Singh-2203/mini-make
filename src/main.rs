use std::collections::{BTreeMap, HashMap, HashSet};
use std::iter::Cycle;
use std::process::Command;
use std::sync::mpsc;
use std::{thread, vec};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
        Pending,
        Running, 
        Success,
        Failed(String)
}

#[derive(Debug, Clone)]
pub struct Task {
        pub id : usize,
        pub name : String,
        pub command : String,
        pub dependencies : Vec<usize>,
        pub state : TaskState
}

#[derive(Debug)]
pub struct  BuildGraph {
        pub tasks : Vec<Task>,
        pub name_to_index : HashMap<String, usize>
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
        #[serde(default)]
        tasks: BTreeMap<String, TaskSpec>,
}

#[derive(Debug, Deserialize)]
struct TaskSpec {
    command: String,
    #[serde(default)]
    depends_on: Vec<String>,
}

#[derive(Debug)]
pub enum BuildError {
        Parse(String),
        MissingDependency(String),
        CyclicDependency(String),
        TaskFailed(String),
        NoRunnableTasks(String),
}

impl std::fmt::Display for BuildError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                        BuildError::Parse(message) => write!(f, "parse error: {message}"),
                        BuildError::MissingDependency(name) => write!(f, "missing dependency: {name}"),
                        BuildError::CyclicDependency(message) => write!(f, "cycle detected: {message}"),
                        BuildError::TaskFailed(name) => write!(f, "task failed: {name}"),
                        BuildError::NoRunnableTasks(message) => write!(f, "no runnable tasks: {message}"),
                }
        }
}

impl std::error::Error for BuildError {}

struct Job {
        task_name : String,
        command : String,
}

impl BuildGraph {
        pub fn from_config(contents: &str) -> Result<Self, BuildError> {
            let config: ConfigFile = toml::from_str(contents).map_err(|err| BuildError::Parse(err.to_string()))?;
    
            let mut name_to_index = HashMap::new();
            let mut tasks = Vec::new();
    
            for (name, spec) in &config.tasks {
                let id = tasks.len();
                name_to_index.insert(name.clone(), id);
                tasks.push(Task {
                    id,
                    name: name.clone(),
                    command: spec.command.clone(),
                    dependencies: Vec::new(),
                    state: TaskState::Pending,
                });
            }
    
            let resolved = tasks
                .into_iter()
                .map(|task| {
                    let deps = config
                        .tasks
                        .get(&task.name)
                        .ok_or_else(|| BuildError::Parse(format!("missing task {}", task.name)))?
                        .depends_on
                        .iter()
                        .map(|dep| {
                            name_to_index
                                .get(dep)
                                .copied()
                                .ok_or_else(|| BuildError::MissingDependency(dep.clone()))
                        })
                        .collect::<Result<Vec<_>, _>>()?;
    
                    Ok(Task {
                        id: task.id,
                        name: task.name,
                        command: task.command,
                        dependencies: deps,
                        state: TaskState::Pending,
                    })
                })
                .collect::<Result<Vec<_>, BuildError>>()?;
    
            Ok(Self {
                tasks: resolved,
                name_to_index,
            })
        }

        fn dfs(&self, node : usize, visited : &mut HashSet<usize>, visiting : &mut HashSet<usize>, stack : &mut Vec<usize>) -> Option<String> {
                if visiting.contains(&node) {
                        let start = stack.iter().rposition(|item| *item == node).unwrap_or(0);
                        let cycle = stack[start..].iter().map(|item| self.tasks[*item].name.clone()).collect::<Vec<_>>();
                        return Some(cycle.join("->"));
                }

                if visited.contains(&node) {
                    return None;
                }

                visiting.insert(node);
                stack.push(node);

                for dep in &self.tasks[node].dependencies {
                        if let Some(cycle) = self.dfs(*dep, visited, visiting, stack) {
                                return Some(cycle);
                        }
                }

                stack.pop();
                visited.insert(node);
                visiting.remove(&node);
                None
        }
}
fn main() {
        
}
