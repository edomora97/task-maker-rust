use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Error;
use clap::Parser;
use itertools::Itertools;
use tempfile::TempDir;

use task_maker_dag::ExecutionStatus;
use task_maker_format::ioi::{
    IOITask, SubtaskId, TestcaseEvaluationStatus, TestcaseGenerationStatus, UIState,
};
use task_maker_format::ui::CompilationStatus;
use task_maker_format::ui::UIStateT;
use task_maker_format::EvaluationConfig;
use task_maker_rust::tools::server::{main_server, ServerOpt};
use task_maker_rust::tools::worker::{main_worker, WorkerOpt};
use task_maker_rust::{run_evaluation, Evaluation, Opt};

use approx::abs_diff_eq;

/// Interface for testing a task.
#[derive(Debug)]
pub struct TestInterface {
    state: Result<UIState, Error>,
    _tempdir: TempDir,
}

/// Interface for testing a task.
#[derive(Debug)]
pub struct TestInterfaceSuccessful {
    state: UIState,
}

impl TestInterface {
    pub fn run_local<P: Into<PathBuf>>(path: P) -> Self {
        let _ = env_logger::Builder::from_default_env()
            .format_timestamp_nanos()
            .is_test(true)
            .try_init();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("tasks")
            .join(path.into());
        let tempdir = TempDir::new().expect("Cannot crete tempdir");
        TestInterface {
            state: TestInterface::run_task_maker(path, false, tempdir.path(), &[]),
            _tempdir: tempdir,
        }
    }

    /// Evaluate the task using a "remote" setup (spawning a local server and local workers).
    pub fn run_remote<P: Into<PathBuf>>(path: P) -> Self {
        let _ = env_logger::Builder::from_default_env()
            .format_timestamp_nanos()
            .is_test(true)
            .try_init();
        let tempdir = TempDir::new().expect("Cannot crete tempdir");
        let client_path = tempdir.path().join("client.sock");
        let worker_path = tempdir.path().join("worker.sock");
        TestInterface::spawn_server(&client_path, &worker_path);
        TestInterface::wait_file(&client_path);

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("tasks")
            .join(path.into());
        TestInterface {
            state: TestInterface::run_task_maker(
                path,
                false,
                tempdir.path(),
                &[
                    "--evaluate-on",
                    &format!("unix://{}", client_path.display()),
                ],
            ),
            _tempdir: tempdir,
        }
    }

    /// Expect task-maker to fail with the specified message.
    pub fn fail<S: AsRef<str>>(self, err: S) {
        let err = err.as_ref();
        if let Err(e) = self.state {
            if !format!("{:?}", e).contains(err) {
                panic!(
                    "Expecting task-maker to fail with '{}' but failed with {:?}",
                    err, e
                );
            }
        } else {
            panic!(
                "Expecting task-maker to fail with '{}' but didn't fail",
                err
            );
        }
    }

    /// Expect task-maker not to fail, unlocking the possibility to test the final state of the
    /// execution.
    pub fn success(self) -> TestInterfaceSuccessful {
        match self.state {
            Ok(state) => TestInterfaceSuccessful { state },
            Err(e) => panic!("Expecting task-maker not to fail, but failed with {:?}", e),
        }
    }

    /// Run task-maker blocking this thread by calling the entry point of the local execution, i.e.
    /// not sending `--server` nor `--worker`. This approach is used to keep a single process
    /// running and keep tracing the coverage.
    fn run_task_maker(
        task_dir: PathBuf,
        cache: bool,
        store_dir: &Path,
        extra_args: &[&str],
    ) -> Result<UIState, Error> {
        let mut args: Vec<&str> = vec!["task-maker"];
        let path = task_dir.to_string_lossy().into_owned();
        let path = format!("--task-dir={}", path);
        args.push(&path);
        args.push("--ui=silent");
        if !cache {
            args.push("--no-cache");
        }
        args.push("-vv");
        let store_dir = format!("--store-dir={}", store_dir.to_string_lossy());
        args.push(&store_dir);
        for arg in extra_args {
            args.push(arg);
        }
        std::env::set_var(
            "TASK_MAKER_TOOLS_PATH",
            env!("CARGO_BIN_EXE_task-maker-tools"),
        );
        let opt = Opt::parse_from(&args);

        let task = IOITask::new(
            &task_dir,
            &EvaluationConfig {
                solution_filter: vec![],
                booklet_solutions: false,
                no_statement: false,
                solution_paths: vec![],
                disabled_sanity_checks: vec![],
                seed: None,
                dry_run: false,
            },
        )
        .unwrap();
        let state = Arc::new(Mutex::new(UIState::new(&task, Default::default())));

        let state2 = state.clone();
        let res = run_evaluation(opt, move |_, mex| state2.lock().unwrap().apply(mex));
        match res? {
            Evaluation::Done => {
                let state = state.lock().unwrap();
                Ok(state.clone())
            }
            Evaluation::Clean => {
                panic!("Unexpected task cleaning");
            }
        }
    }

    /// Block until the specified file exists, meaning that the server is ready to accept
    /// connections.
    fn wait_file(path: &Path) {
        for _ in 0..10 {
            eprintln!("Waiting for the server...");
            std::thread::sleep(Duration::from_millis(500));
            if path.exists() {
                break;
            }
        }
    }

    /// Spawn the server and the worker in different threads by calling their entry points.
    fn spawn_server(client_path: &Path, worker_path: &Path) {
        std::thread::Builder::new()
            .name("Test server".to_string())
            .spawn({
                let client_path = client_path.to_path_buf();
                let worker_path = worker_path.to_path_buf();
                move || {
                    let tmpdir = tempfile::TempDir::new().unwrap();
                    let store = tmpdir.path().to_string_lossy().to_string();
                    let opt = ServerOpt::parse_from([
                        "server",
                        "--store-dir",
                        &store,
                        &format!("unix://{}", client_path.display()),
                        &format!("unix://{}", worker_path.display()),
                    ]);
                    eprintln!("Server opts {:?}", opt);
                    main_server(opt).unwrap();
                }
            })
            .unwrap();
        let worker_path = worker_path.to_path_buf();
        std::thread::Builder::new()
            .name("Test worker".to_string())
            .spawn(move || {
                TestInterface::wait_file(&worker_path);
                let tmpdir = tempfile::TempDir::new().unwrap();
                let store = tmpdir.path().to_string_lossy().to_string();
                let opt = WorkerOpt::parse_from([
                    "worker",
                    "--store-dir",
                    &store,
                    &format!("unix://{}", worker_path.display()),
                ]);
                eprintln!("Worker opts {:?}", opt);
                std::env::set_var(
                    "TASK_MAKER_TOOLS_PATH",
                    env!("CARGO_BIN_EXE_task-maker-tools"),
                );
                main_worker(opt).unwrap();
            })
            .unwrap();
    }
}

impl TestInterfaceSuccessful {
    /// Check that the time limit is the one specified.
    pub fn time_limit(self, time_limit: f64) -> Self {
        let actual = self.state.task.time_limit.expect("No time limit in task");
        assert!(abs_diff_eq!(actual, time_limit), "Wrong time limit");
        self
    }

    /// Check that the memory limit is the one specified.
    pub fn memory_limit(self, memory_limit: u64) -> Self {
        let actual = self.state.task.memory_limit.expect("No memory limit");
        assert_eq!(actual, memory_limit);
        self
    }

    /// Check that the max score of the task is the one specified.
    pub fn max_score(self, max_score: f64) -> Self {
        let task = &self.state.task.subtasks;
        let actual: f64 = task.values().map(|s| s.max_score).sum();
        assert!(abs_diff_eq!(actual, max_score), "Wrong max score");
        assert!(
            abs_diff_eq!(self.state.max_score, max_score),
            "Wrong max score in state"
        );
        self
    }

    /// Check that the specified file is compiled successfully.
    pub fn must_compile<P: Into<PathBuf>>(self, source: P) -> Self {
        let source = source.into();
        let comp = self.get_path_key(&self.state.compilations, &source);
        if let Some(comp) = comp {
            match comp {
                CompilationStatus::Done { .. } => {}
                _ => panic!("Expecting {:?} to compile, but was: {:?}", source, comp),
            }
        } else {
            panic!(
                "Compilation {:?} not present: {:?}",
                source, self.state.compilations
            );
        }
        self
    }

    /// Check that the specified file fails to compile.
    pub fn must_not_compile<P: Into<PathBuf>>(self, source: P) -> Self {
        let source = source.into();
        let comp = self.get_path_key(&self.state.compilations, &source);
        if let Some(comp) = comp {
            match comp {
                CompilationStatus::Failed { .. } => {}
                _ => panic!("Expecting {:?} not to compile, but was: {:?}", source, comp),
            }
        } else {
            panic!("Compilation not present: {:?}", source);
        }
        self
    }

    /// Check that the specified file is not compiled.
    pub fn not_compiled<P: Into<PathBuf>>(self, source: P) -> Self {
        let source = source.into();
        let comp = self.get_path_key(&self.state.compilations, &source);
        if let Some(comp) = comp {
            panic!(
                "Expecting {:?} not to be compiled, but was {:?}",
                source, comp
            );
        }
        self
    }

    /// Check that the subtasks have the following scores.
    pub fn subtask_scores<I: IntoIterator<Item = f64>>(self, scores: I) -> Self {
        let scores: Vec<_> = scores.into_iter().collect();
        let subtasks = &self.state.task.subtasks;
        assert_eq!(subtasks.len(), scores.len());
        for (i, expected) in scores.iter().enumerate() {
            let actual = subtasks
                .get(&(i as SubtaskId))
                .unwrap_or_else(|| panic!("Missing subtask {}", i));
            assert!(
                abs_diff_eq!(actual.max_score, expected),
                "Wrong subtask score of subtask {}",
                i
            );
        }
        self
    }

    /// Check that the solution scores those values for each subtask.
    pub fn solution_score<P: Into<PathBuf>, I: IntoIterator<Item = f64>>(
        self,
        solution: P,
        scores: I,
    ) -> Self {
        let solution = solution.into();
        let scores: Vec<_> = scores.into_iter().collect();
        let state = self
            .get_path_key(&self.state.evaluations, &solution)
            .unwrap_or_else(|| panic!("No evaluation score for solution {:?}", solution));

        let score: f64 = scores.iter().sum();
        let state_score = state
            .score
            .unwrap_or_else(|| panic!("Missing score of {:?}", solution));
        assert!(
            abs_diff_eq!(score, state_score),
            "Solution score mismatch for solution {:?}: {:#?}",
            solution,
            state
        );
        assert_eq!(
            scores.len(),
            state.subtasks.len(),
            "Wrong number of subtask"
        );
        for (st, expected) in scores.iter().enumerate() {
            let actual = state.subtasks[&(st as SubtaskId)].score.unwrap();
            assert!(
                abs_diff_eq!(*expected, actual),
                "Solution subtask score mismatch of solution {:?} at subtask {}: {:#?}",
                solution,
                st,
                state
            );
        }
        self
    }

    /// Check that the statuses of the solution starts with the ones specified.
    pub fn solution_statuses<P: Into<PathBuf>, I: IntoIterator<Item = TestcaseEvaluationStatus>>(
        self,
        solution: P,
        statuses: I,
    ) -> Self {
        let solution = solution.into();
        let statuses: Vec<_> = statuses.into_iter().collect();
        let actuals = self
            .get_path_key(&self.state.evaluations, &solution)
            .unwrap_or_else(|| panic!("Evaluation status missing for solution {:?}", solution));
        let mut id = 0;
        for st in actuals.subtasks.keys().sorted() {
            for tc in &self.state.task.subtasks[st].testcases {
                let expected = &statuses[id];
                let actual = &actuals.testcases[tc].status;
                assert_eq!(
                    actual, expected,
                    "Solution status mismatch of {:?} at subtask {}, testcase {}",
                    solution, st, tc
                );
                if id + 1 < statuses.len() {
                    id += 1;
                }
            }
        }
        self
    }

    /// Check that the statuses of the generation are those.
    pub fn generation_statuses<I: IntoIterator<Item = TestcaseGenerationStatus>>(
        self,
        statuses: I,
    ) -> Self {
        let statuses: Vec<_> = statuses.into_iter().collect();
        let mut id = 0;
        for st in self.state.generations.keys().sorted() {
            let subtask = &self.state.generations[st];
            for tc in subtask.testcases.keys().sorted() {
                let actual = &subtask.testcases[tc].status;
                let expected = statuses.get(id).unwrap_or_else(|| {
                    panic!(
                        "Too few testcases in provided status, needing at least {}",
                        id
                    )
                });

                assert_eq!(actual, expected);
                id += 1;
            }
        }
        assert_eq!(id, statuses.len(), "Too many testcases provided");
        self
    }

    /// Check that the generators fail with the specified messages.
    pub fn generation_fails<I: IntoIterator<Item = Option<String>>>(self, fails: I) -> Self {
        let fails: Vec<_> = fails.into_iter().collect();
        let mut id = 0;
        for st in self.state.generations.keys().sorted() {
            let subtask = &self.state.generations[st];
            for tc in subtask.testcases.keys().sorted() {
                let testcase = &subtask.testcases[tc];
                let status = &testcase
                    .generation
                    .as_ref()
                    .expect("Missing generation execution")
                    .status;
                match fails.get(id) {
                    Some(Some(expected)) => {
                        assert_ne!(
                            &ExecutionStatus::Success,
                            status,
                            "Expecting generation of subtask {}, testcase {} to fail",
                            st,
                            tc
                        );
                        let stderr = testcase
                            .generation
                            .as_ref()
                            .unwrap()
                            .stderr
                            .as_ref()
                            .unwrap();
                        let stderr = String::from_utf8_lossy(stderr);
                        assert!(
                            stderr.contains(expected),
                            "Generation stderr of subtask {}, testcase {} does not contain '{}'. It is '{:?}'",
                            st, tc, expected, stderr
                        );
                    }
                    Some(None) => {
                        assert_eq!(
                            &ExecutionStatus::Success,
                            status,
                            "Expecting generation of subtask {}, testcase {} not to fail, but was: {:?}",
                            st,
                            tc,
                            status
                        );
                    }
                    None => panic!(
                        "Too few testcases in provided status, needing at least {}",
                        id
                    ),
                }
                id += 1;
            }
        }
        assert_eq!(id, fails.len(), "Too many testcases provided");
        self
    }

    /// Check that the validations fail with the specified messages.
    pub fn validation_fails<I: IntoIterator<Item = Option<String>>>(self, fails: I) -> Self {
        let fails: Vec<_> = fails.into_iter().collect();
        let mut id = 0;
        for st in self.state.generations.keys().sorted() {
            let subtask = &self.state.generations[st];
            for tc in subtask.testcases.keys().sorted() {
                let testcase = &subtask.testcases[tc];
                let status = &testcase
                    .validation
                    .as_ref()
                    .expect("Missing validation execution")
                    .status;
                match fails.get(id) {
                    Some(Some(expected)) => {
                        assert_ne!(
                            &ExecutionStatus::Success,
                            status,
                            "Expecting validation of subtask {}, testcase {} to fail",
                            st,
                            tc
                        );
                        let stderr = testcase
                            .validation
                            .as_ref()
                            .unwrap()
                            .stderr
                            .as_ref()
                            .unwrap();
                        let stderr = String::from_utf8_lossy(stderr);
                        assert!(
                            stderr.contains(expected),
                            "Validation stderr of subtask {}, testcase {} does not contain '{}'. It is '{:?}'",
                            st, tc, expected, stderr
                        );
                    }
                    Some(None) => {
                        assert_eq!(
                            &ExecutionStatus::Success,
                            status,
                            "Expecting validation of subtask {}, testcase {} not to fail, but was: {:?}",
                            st,
                            tc,
                            status
                        );
                    }
                    None => panic!(
                        "Too few testcases in provided status, needing at least {}",
                        id
                    ),
                }
                id += 1;
            }
        }
        assert_eq!(id, fails.len(), "Too many testcases provided");
        self
    }

    /// Check that a file is present in the task directory.
    pub fn file_exists<P: AsRef<Path>>(self, path: P) -> Self {
        let full_path = self.state.task.path.join(path.as_ref());
        if !full_path.exists() {
            panic!(
                "File {} (at {}) does not exists",
                path.as_ref().display(),
                full_path.display()
            );
        }
        self
    }

    /// Find the value in a map whose key is a path with the file name equal to the one specified.
    fn get_path_key<'a, V, P>(&self, map: &'a HashMap<PathBuf, V>, path: P) -> Option<&'a V>
    where
        P: AsRef<Path>,
    {
        for (k, v) in map.iter() {
            if k.file_name().unwrap() == path.as_ref() {
                return Some(v);
            }
        }
        None
    }

    /// Check that there is a diagnostic containing the message.
    pub fn has_diagnostic(self, message: impl AsRef<str>) -> Self {
        let message = message.as_ref();
        if !self
            .state
            .diagnostics
            .diagnostics()
            .iter()
            .any(|d| d.message().contains(message))
        {
            panic!(
                "Expecting the diagnostics to contain {}, but they don't",
                message
            );
        }
        self
    }
}
