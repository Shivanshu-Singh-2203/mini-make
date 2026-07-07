# mini-make

A tiny parallel build/task runner. Define tasks and their dependencies in a TOML file, and `mini-make` will execute them concurrently across a worker pool, respecting dependency order, detecting cycles, and stopping on failure.

## Features

- **Declarative task graph** — define tasks and dependencies in a simple TOML file
- **Parallel execution** — tasks with no unmet dependencies run concurrently across a pool of worker threads
- **Cycle detection** — validates the dependency graph before running anything, reporting the exact cycle found
- **Fail-fast** — a failing task stops the build and reports which task failed
- **Zero external build config** — just a `[tasks.<name>]` table per task

## Installation

Requires a Rust toolchain (stable) and Cargo.

```bash
git clone git@github.com:Shivanshu-Singh-2203/mini-make.git
cd mini-make
cargo build 
```

## Usage

```bash
cargo run -- <path-to-build-config>.toml
```

or, using the release binary:

```bash
./target/release/mini-make build.toml
```

`mini-make` changes its working directory to the folder containing the config file before running any commands, so relative paths inside `command` strings are resolved relative to the config file's location, not the caller's current directory.

## Config format

```toml
[tasks.fetch]
command = "curl -O https://example.com/data.zip"

[tasks.extract]
command = "unzip data.zip"
depends_on = ["fetch"]

[tasks.build]
command = "make"
depends_on = ["extract"]

[tasks.test]
command = "make test"
depends_on = ["build"]
```

Each task has:

| Field         | Type             | Required | Description                                   |
|---------------|------------------|----------|------------------------------------------------|
| `command`     | string           | yes      | Shell command to run (via `sh -c`)             |
| `depends_on`  | array of strings | no       | Names of tasks that must succeed before this one starts |

Tasks with no `depends_on` run as soon as a worker is free. Tasks with dependencies run once **all** of their dependencies have completed successfully.

## How it works

1. **Parse** — the TOML file is parsed into a set of named tasks, each with a command and a list of dependency names.
2. **Resolve** — dependency names are resolved to indices into the task list; an unknown dependency name is reported as a `MissingDependency` error.
3. **Validate** — a depth-first search over the dependency graph detects cycles before any command runs, reporting the cycle as `name1->name2->...`.
4. **Run** — a pool of worker threads (sized to available CPU parallelism) pulls ready tasks and executes each as `sh -c "<command>"`. The scheduler repeatedly computes the set of tasks whose dependencies have all succeeded, dispatches them, and waits for results, until every task has completed or one fails.

## Errors

| Error                | Meaning                                                        |
|-----------------------|-----------------------------------------------------------------|
| `Parse`               | The TOML file couldn't be read or parsed                       |
| `MissingDependency`   | A task depends on a name that doesn't exist                    |
| `CyclicDependency`    | The dependency graph contains a cycle                           |
| `TaskFailed`          | A task's command exited non-zero or failed to spawn             |
| `NoRunnableTasks`     | The scheduler has tasks left but none are currently runnable    |

On any error, `mini-make` prints the error to stderr and exits with status `1`.

## Limitations / known caveats

- Worker dispatch is round-robin over a fixed-size thread pool; if more tasks become ready at once than there are workers, extra tasks queue up on individual workers rather than being load-balanced dynamically.
- Worker threads are not explicitly joined; the process relies on the main scheduling loop's channel communication and exits once the build completes or fails.
- No support yet for task timeouts, output capturing/logging per task, or partial/continue-on-failure builds.