use std::collections::{BTreeMap, HashMap, HashSet};
use std::process::Command;
use std::sync::mpsc;
use std::{thread};
use serde::Deserialize;
mod test;

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


        pub fn validate(&self) -> Result<(), BuildError> {
                let mut visited = HashSet::new();
                let mut visiting = HashSet::new();
                let mut stack = Vec::new();

                for index in 0..self.tasks.len() {
                        if visited.contains(&index) {
                                continue;
                        }
                        if let Some(cycle) = self.dfs(index, &mut visited, &mut visiting, &mut stack) {
                                return Err(BuildError::CyclicDependency(cycle));
                        } 
                }

                Ok(())
        }
        pub fn run(&self) -> Result<(), BuildError> {
                let mut graph = self.tasks.clone();
                let worker_count = std::thread::available_parallelism().map_or(4, |n| n.get()).max(1);

                let (result_tx, result_rx) = mpsc::channel();

                let mut job_senders : Vec<mpsc::Sender<Job>> = Vec::with_capacity(worker_count);

                for _ in 0..worker_count {
                        let (job_tx, job_rx) = mpsc::channel();
                        job_senders.push(job_tx);
                        let result_tx = result_tx.clone();

                        thread::spawn(move || loop {
                                match job_rx.recv() {
                                        Ok(job) => {
                                                let status = Command::new("sh").arg("-c").arg(&job.command).status();
                                                let outcome = match status {
                                                        Ok(status) if status.success() => Ok(()),
                                                        Ok(status) => Err(format!("exit code : {status}")),
                                                        Err(err) => Err(err.to_string()),
                                                };

                                                let _ = result_tx.send((job.task_name, outcome));
                                        },
                                        Err(_) => break 
                                }
                        });
                }

                let mut completed = 0usize;
                let mut next_worker = 0usize;

                while completed < graph.len() {
                    let ready: Vec<usize> = graph
                        .iter()
                        .enumerate()
                        .filter(|(_, task)| matches!(task.state, TaskState::Pending))
                        .filter(|(idx, _)| {
                            graph[*idx]
                                .dependencies
                                .iter()
                                .all(|dep| graph[*dep].state == TaskState::Success)
                        })
                        .map(|(idx, _)| idx)
                        .collect();
        
                    let running_count = graph.iter().filter(|t| matches!(t.state, TaskState::Running)).count();
                    
                    if ready.is_empty() && running_count == 0 && completed < graph.len() {
                        return Err(BuildError::NoRunnableTasks(format!("{} tasks remain", graph.len() - completed)));
                    }
                    
                    for idx in ready {
                        graph[idx].state = TaskState::Running;
                        let task_name = graph[idx].name.clone();
                        let command = graph[idx].command.clone();
                        let sender = &job_senders[next_worker % job_senders.len()];
                        sender
                            .send(Job { task_name, command })
                            .map_err(|_| BuildError::Parse("worker channel closed".to_string()))?;
                        next_worker += 1;
                    }
        
                    let (task_name, outcome) = result_rx
                        .recv()
                        .map_err(|_| BuildError::Parse("worker channel closed".to_string()))?;
                    completed += 1;
                    let target = graph.iter().position(|task| task.name == task_name).unwrap();
                    match outcome {
                        Ok(()) => graph[target].state = TaskState::Success,
                        Err(message) => {
                            graph[target].state = TaskState::Failed(message.clone());
                            return Err(BuildError::TaskFailed(task_name));
                        }
                    }
                }

                Ok(())
        }
}

pub fn run_from_file(path : &str) -> Result<(), BuildError> {
        let contents = std::fs::read_to_string(path).map_err(|err| BuildError::Parse(err.to_string()))?;
        let graph = BuildGraph::from_config(&contents)?;
        graph.validate()?;
        graph.run()
}

fn main() {
    if let Some(path) = std::env::args().nth(1) {
        
        if let Some(parent_dir) = std::path::Path::new(&path).parent() {
                if parent_dir.exists() && parent_dir.is_dir() {
                        let _ = std::env::set_current_dir(parent_dir);
                }
        } 

        let file_name = std::path::Path::new(&path).file_name().unwrap().to_str().unwrap();

        match run_from_file(file_name) {
                Ok(()) => {}
                Err(err) => {
                        eprintln!("{err}");
                        std::process::exit(1);
            }
        }
    } else {
        println!("usage: cargo run -- <build-config>");
    }
}