use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

use task_maker_dag::*;
use task_maker_diagnostics::DiagnosticContext;
use task_maker_exec::ExecutorStatus;

use crate::solution::{SolutionCheck, SolutionInfo, TestcaseEvaluationResult};
use crate::ui::{CompilationStatus, UIExecutionStatus, UIMessage, UIStateT};
use crate::{ioi::*, ScoreStatus};

/// Status of the generation of a testcase input and output.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TestcaseGenerationStatus {
    /// The generation has not started yet.
    Pending,
    /// The input file is generating.
    Generating,
    /// The input file has been generated.
    Generated,
    /// The input file is being validated.
    Validating,
    /// The input file has been validated.
    Validated,
    /// The output file is generating.
    Solving,
    /// The output file has been generated.
    Solved,
    /// The generation of the testcase has failed.
    Failed,
    /// The generation has been skipped.
    Skipped,
}

/// Status of the evaluation of a solution on a testcase.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TestcaseEvaluationStatus {
    /// The solution has not started yet.
    Pending,
    /// The solution is running.
    Solving,
    /// The solution exited successfully, waiting for the checker.
    Solved,
    /// Checker is running.
    Checking,
    /// The solution scored 100% of the testcase.
    Accepted(String),
    /// The output is wrong.
    WrongAnswer(String),
    /// The solution is partially correct.
    Partial(String),
    /// The solution timed out.
    TimeLimitExceeded,
    /// The solution exceeded the wall time limit.
    WallTimeLimitExceeded,
    /// The solution exceeded the memory limit.
    MemoryLimitExceeded,
    /// The solution crashed.
    RuntimeError,
    /// Something went wrong.
    Failed,
    /// The evaluation has been skipped.
    Skipped,
}

impl From<&TestcaseEvaluationStatus> for Option<TestcaseEvaluationResult> {
    fn from(status: &TestcaseEvaluationStatus) -> Self {
        use TestcaseEvaluationResult as TER;
        use TestcaseEvaluationStatus as TES;
        match status {
            TES::Accepted(_) => Some(TER::Accepted),
            TES::Partial(_) => Some(TER::Partial),
            TES::WrongAnswer(_) => Some(TER::WrongAnswer),
            TES::TimeLimitExceeded => Some(TER::TimeLimitExceeded),
            TES::WallTimeLimitExceeded => Some(TER::WallTimeLimitExceeded),
            TES::MemoryLimitExceeded => Some(TER::MemoryLimitExceeded),
            TES::RuntimeError => Some(TER::RuntimeError),
            _ => None,
        }
    }
}

/// State of the generation of a testcases.
#[derive(Debug, Clone)]
pub struct TestcaseGenerationState {
    /// Status of the generation.
    pub status: TestcaseGenerationStatus,
    /// Result of the generation.
    pub generation: Option<ExecutionResult>,
    /// Result of the validation.
    pub validation: Option<ExecutionResult>,
    /// Result of the solution.
    pub solution: Option<ExecutionResult>,
}

/// State of the generation of a subtask.
#[derive(Debug, Clone)]
pub struct SubtaskGenerationState {
    /// State of the testcases of this subtask.
    pub testcases: HashMap<TestcaseId, TestcaseGenerationState>,
}

/// State of the evaluation of a testcase.
#[derive(Debug, Clone)]
pub struct SolutionTestcaseEvaluationState {
    /// The score on that testcase
    pub score: Option<f64>,
    /// The status of the execution.
    pub status: TestcaseEvaluationStatus,
    /// The result of the solution.
    pub results: Vec<Option<ExecutionResult>>,
    /// The result of the checker.
    pub checker: Option<ExecutionResult>,
}

impl SolutionTestcaseEvaluationState {
    /// Checks whether the resources used by a solution on a testcase are close to the limits of
    /// time or memory.
    ///
    /// The time 't' is close to the limit TL if:
    ///   TL * threshold <= t <= TL / threshold  &&  t <= ceil(TL + extra time) - 0.1s
    ///
    /// The second condition guards against a value of extra time too small, which would mark every
    /// TLE as "close to the limits".
    ///
    /// Memory limit is in MiB.
    pub fn is_close_to_limits(
        &self,
        time_limit: Option<f64>,
        extra_time: f64,
        memory_limit: Option<u64>,
        threshold: f64,
    ) -> bool {
        for result in self.results.iter().flatten() {
            let resources = &result.resources;
            if let Some(time_limit) = time_limit {
                let lower_bound = time_limit * threshold;
                let upper_bound = time_limit / threshold;
                if lower_bound <= resources.cpu_time
                    && resources.cpu_time <= upper_bound
                    && resources.cpu_time <= (time_limit + extra_time).ceil() - 0.1
                {
                    return true;
                }
            }
            if let Some(memory_limit) = memory_limit {
                if resources.memory as f64 >= memory_limit as f64 * 1024.0 * threshold {
                    return true;
                }
            }
        }

        false
    }
}

/// State of the evaluation of a subtask.
#[derive(Debug, Clone)]
pub struct SolutionSubtaskEvaluationState {
    /// Score of the subtask.
    pub score: Option<f64>,
    /// Score of the subtask, normalized from 0.0 to 1.0.
    pub normalized_score: Option<f64>,
}

/// State of the evaluation of a solution.
#[derive(Debug, Clone)]
pub struct SolutionEvaluationState {
    /// Score of the solution.
    pub score: Option<f64>,
    /// The state of the evaluation of the subtasks.
    pub subtasks: HashMap<SubtaskId, SolutionSubtaskEvaluationState>,
    /// The state of the evaluation of the testcases.
    pub testcases: HashMap<TestcaseId, SolutionTestcaseEvaluationState>,
}

impl SolutionEvaluationState {
    /// Make a new, empty, `SolutionEvaluationState`.
    pub fn new(task: &IOITask) -> SolutionEvaluationState {
        SolutionEvaluationState {
            score: None,
            subtasks: task
                .subtasks
                .values()
                .map(|subtask| {
                    (
                        subtask.id,
                        SolutionSubtaskEvaluationState {
                            score: None,
                            normalized_score: None,
                        },
                    )
                })
                .collect(),
            testcases: task
                .testcases
                .values()
                .map(|testcase| {
                    (
                        testcase.id,
                        SolutionTestcaseEvaluationState {
                            score: None,
                            status: TestcaseEvaluationStatus::Pending,
                            results: Vec::new(),
                            checker: None,
                        },
                    )
                })
                .collect(),
        }
    }
}

/// The status of the compilation of a dependency of a booklet.
#[derive(Debug, Clone)]
pub struct BookletDependencyState {
    /// The status of the execution.
    pub status: UIExecutionStatus,
}

/// The status of the compilation of a booklet.
#[derive(Debug, Clone)]
pub struct BookletState {
    /// The status of the execution.
    pub status: UIExecutionStatus,
    /// The state of all the dependencies
    pub dependencies: HashMap<String, Vec<BookletDependencyState>>,
}

/// The state of a IOI task, all the information for the UI are stored here.
#[derive(Debug, Clone)]
pub struct UIState {
    /// The task.
    pub task: IOITask,
    /// The configuration of this evaluation.
    pub config: ExecutionDAGConfig,
    /// The maximum score of this task.
    pub max_score: f64,
    /// The set of solutions that will be evaluated.
    pub solutions: HashMap<PathBuf, SolutionInfo>,
    /// The status of the compilations.
    pub compilations: HashMap<PathBuf, CompilationStatus>,
    /// The state of the generation of the testcases.
    pub generations: HashMap<SubtaskId, SubtaskGenerationState>,
    /// The status of the evaluations of the solutions.
    pub evaluations: HashMap<PathBuf, SolutionEvaluationState>,
    /// The status of the executor.
    pub executor_status: Option<ExecutorStatus<SystemTime>>,
    /// The status of the booklets
    pub booklets: HashMap<String, BookletState>,
    /// Diagnostic context.
    pub diagnostics: DiagnosticContext,
}

impl TestcaseEvaluationStatus {
    /// Whether the testcase evaluation has completed, either successfully or not.
    pub fn has_completed(&self) -> bool {
        !matches!(
            self,
            TestcaseEvaluationStatus::Pending
                | TestcaseEvaluationStatus::Solving
                | TestcaseEvaluationStatus::Solved
                | TestcaseEvaluationStatus::Checking
        )
    }

    /// Whether the testcase evaluation has completed successfully.
    pub fn is_success(&self) -> bool {
        matches!(self, TestcaseEvaluationStatus::Accepted(_))
    }

    /// Whether the testcase evaluation has completed with a partial score.
    pub fn is_partial(&self) -> bool {
        matches!(self, TestcaseEvaluationStatus::Partial(_))
    }

    /// A message representing this status.
    pub fn message(&self) -> String {
        use TestcaseEvaluationStatus::*;
        match self {
            Pending => "Not done".into(),
            Solving => "Solution running".into(),
            Solved => "Solution completed".into(),
            Checking => "Checker running".into(),
            Accepted(s) => {
                if s.is_empty() {
                    "Output is correct".into()
                } else {
                    s.clone()
                }
            }
            WrongAnswer(s) => {
                if s.is_empty() {
                    "Output is not correct".into()
                } else {
                    s.clone()
                }
            }
            Partial(s) => {
                if s.is_empty() {
                    "Partially correct".into()
                } else {
                    s.clone()
                }
            }
            TimeLimitExceeded => "Time limit exceeded".into(),
            WallTimeLimitExceeded => "Execution took too long".into(),
            MemoryLimitExceeded => "Memory limit exceeded".into(),
            RuntimeError => "Runtime error".into(),
            Failed => "Execution failed".into(),
            Skipped => "Execution skipped".into(),
        }
    }
}

/// The outcome of the execution of a check on a subtask.
#[derive(Clone, Debug)]
pub struct SolutionCheckOutcome {
    /// The path of the solution.
    pub solution: PathBuf,
    /// The check that originated this outcome.
    pub check: SolutionCheck,
    /// The id of the subtask this outcome refers to.
    pub subtask_id: SubtaskId,
    /// Whether the check was successful or not.
    pub success: bool,
}

impl UIState {
    /// Make a new `UIState`.
    pub fn new(task: &IOITask, config: ExecutionDAGConfig) -> UIState {
        let generations = task
            .subtasks
            .iter()
            .map(|(st_num, subtask)| {
                (
                    *st_num,
                    SubtaskGenerationState {
                        testcases: subtask
                            .testcases_owned
                            .iter()
                            .map(|tc_num| {
                                (
                                    *tc_num,
                                    TestcaseGenerationState {
                                        status: TestcaseGenerationStatus::Pending,
                                        generation: None,
                                        validation: None,
                                        solution: None,
                                    },
                                )
                            })
                            .collect(),
                    },
                )
            })
            .collect();
        UIState {
            config,
            max_score: task.subtasks.values().map(|s| s.max_score).sum(),
            task: task.clone(),
            solutions: HashMap::new(),
            compilations: HashMap::new(),
            generations,
            evaluations: HashMap::new(),
            executor_status: None,
            booklets: HashMap::new(),
            diagnostics: Default::default(),
        }
    }

    /// Evaluate the checks of all the solutions.
    ///
    /// This function should be called only after all the executions have completed.
    pub fn run_solution_checks(&self) -> Vec<SolutionCheckOutcome> {
        let mut result = vec![];
        for (path, solution) in self.solutions.iter() {
            for check in solution.checks.iter() {
                let subtasks = self
                    .task
                    .find_subtasks_by_pattern_name(&check.subtask_name_pattern);
                for subtask in subtasks {
                    let solution_result = self.evaluations.get(path);
                    // The solution was not run on this subtask.
                    let Some(solution_result) = solution_result else {
                        continue;
                    };
                    let testcase_results: Vec<Option<TestcaseEvaluationResult>> = self
                        .task
                        .subtasks[&subtask.id]
                        .testcases
                        .iter()
                        .map(|testcase_id| (&solution_result.testcases[testcase_id].status).into())
                        .collect();
                    if testcase_results.iter().any(Option::is_none) {
                        continue;
                    }
                    let testcase_results = testcase_results
                        .into_iter()
                        .map(Option::unwrap)
                        .collect_vec();
                    let success = check.result.check(&testcase_results);
                    result.push(SolutionCheckOutcome {
                        solution: path.clone(),
                        check: check.clone(),
                        subtask_id: subtask.id,
                        success,
                    })
                }
            }
        }
        result
    }
}

impl UIStateT for UIState {
    /// Apply a `UIMessage` to this state.
    fn apply(&mut self, message: UIMessage) {
        match message {
            UIMessage::StopUI => {}
            UIMessage::ServerStatus { status } => self.executor_status = Some(status),
            UIMessage::Solutions { solutions } => {
                self.solutions = solutions
                    .into_iter()
                    .map(|info| (info.path.clone(), info))
                    .collect()
            }
            UIMessage::Compilation { file, status } => self
                .compilations
                .entry(file)
                .or_insert(CompilationStatus::Pending)
                .apply_status(status),
            UIMessage::IOITask { .. } => {}
            UIMessage::IOIGeneration {
                subtask,
                testcase,
                status,
            } => {
                let gen = self
                    .generations
                    .get_mut(&subtask)
                    .expect("Subtask is gone")
                    .testcases
                    .get_mut(&testcase)
                    .expect("Testcase is gone");
                match status {
                    UIExecutionStatus::Pending => gen.status = TestcaseGenerationStatus::Pending,
                    UIExecutionStatus::Started { .. } => {
                        gen.status = TestcaseGenerationStatus::Generating
                    }
                    UIExecutionStatus::Done { result } => {
                        if let ExecutionStatus::Success = result.status {
                            gen.status = TestcaseGenerationStatus::Generated;
                        } else {
                            gen.status = TestcaseGenerationStatus::Failed;
                        }
                        gen.generation = Some(result);
                    }
                    UIExecutionStatus::Skipped => gen.status = TestcaseGenerationStatus::Skipped,
                }
            }
            UIMessage::IOIValidation {
                subtask,
                testcase,
                status,
            } => {
                let gen = self
                    .generations
                    .get_mut(&subtask)
                    .expect("Subtask is gone")
                    .testcases
                    .get_mut(&testcase)
                    .expect("Testcase is gone");
                match status {
                    UIExecutionStatus::Pending => gen.status = TestcaseGenerationStatus::Pending,
                    UIExecutionStatus::Started { .. } => {
                        gen.status = TestcaseGenerationStatus::Validating
                    }
                    UIExecutionStatus::Done { result } => {
                        if let ExecutionStatus::Success = result.status {
                            gen.status = TestcaseGenerationStatus::Validated;
                        } else {
                            gen.status = TestcaseGenerationStatus::Failed;
                        }
                        gen.validation = Some(result);
                    }
                    UIExecutionStatus::Skipped => {
                        if let TestcaseGenerationStatus::Failed = gen.status {
                        } else {
                            gen.status = TestcaseGenerationStatus::Skipped;
                        }
                    }
                }
            }
            UIMessage::IOISolution {
                subtask,
                testcase,
                status,
            } => {
                let gen = self
                    .generations
                    .get_mut(&subtask)
                    .expect("Subtask is gone")
                    .testcases
                    .get_mut(&testcase)
                    .expect("Testcase is gone");
                match status {
                    UIExecutionStatus::Pending => gen.status = TestcaseGenerationStatus::Pending,
                    UIExecutionStatus::Started { .. } => {
                        gen.status = TestcaseGenerationStatus::Solving
                    }
                    UIExecutionStatus::Done { result } => {
                        if let ExecutionStatus::Success = result.status {
                            gen.status = TestcaseGenerationStatus::Solved;
                        } else {
                            gen.status = TestcaseGenerationStatus::Failed;
                        }
                        gen.solution = Some(result);
                    }
                    UIExecutionStatus::Skipped => {
                        if let TestcaseGenerationStatus::Failed = gen.status {
                        } else {
                            gen.status = TestcaseGenerationStatus::Skipped;
                        }
                    }
                }
            }
            UIMessage::IOIEvaluation {
                testcase,
                solution,
                status,
                part,
                num_parts,
                ..
            } => {
                let task = &self.task;
                let eval = self
                    .evaluations
                    .entry(solution)
                    .or_insert_with(|| SolutionEvaluationState::new(task));
                let testcase = eval.testcases.get_mut(&testcase).expect("Missing testcase");
                if testcase.results.len() != num_parts {
                    testcase.results = vec![None; num_parts];
                }
                match status {
                    UIExecutionStatus::Pending => {}
                    UIExecutionStatus::Started { .. } => {
                        testcase.status = TestcaseEvaluationStatus::Solving
                    }
                    UIExecutionStatus::Done { result } => {
                        match result.status {
                            ExecutionStatus::Success => {
                                testcase.status = TestcaseEvaluationStatus::Solved
                            }
                            ExecutionStatus::ReturnCode(_) => {
                                testcase.status = TestcaseEvaluationStatus::RuntimeError
                            }
                            ExecutionStatus::Signal(_, _) => {
                                testcase.status = TestcaseEvaluationStatus::RuntimeError
                            }
                            ExecutionStatus::TimeLimitExceeded => {
                                testcase.status = TestcaseEvaluationStatus::TimeLimitExceeded
                            }
                            ExecutionStatus::SysTimeLimitExceeded => {
                                testcase.status = TestcaseEvaluationStatus::TimeLimitExceeded
                            }
                            ExecutionStatus::WallTimeLimitExceeded => {
                                testcase.status = TestcaseEvaluationStatus::WallTimeLimitExceeded
                            }
                            ExecutionStatus::MemoryLimitExceeded => {
                                testcase.status = TestcaseEvaluationStatus::MemoryLimitExceeded
                            }
                            ExecutionStatus::InternalError(_) => {
                                testcase.status = TestcaseEvaluationStatus::Failed
                            }
                        }
                        testcase.results[part] = Some(result);
                    }
                    UIExecutionStatus::Skipped => {
                        testcase.status = TestcaseEvaluationStatus::Skipped
                    }
                }
            }
            UIMessage::IOIChecker {
                testcase,
                solution,
                status,
                ..
            } => {
                let task = &self.task;
                let eval = self
                    .evaluations
                    .entry(solution)
                    .or_insert_with(|| SolutionEvaluationState::new(task));
                let testcase = eval.testcases.get_mut(&testcase).expect("Missing testcase");
                match status {
                    UIExecutionStatus::Started { .. } => {
                        testcase.status = TestcaseEvaluationStatus::Checking;
                    }
                    UIExecutionStatus::Done { result } => {
                        testcase.checker = Some(result);
                    }
                    _ => {}
                }
            }
            UIMessage::IOITestcaseScore {
                testcase,
                solution,
                score,
                message,
                ..
            } => {
                let task = &self.task;
                let eval = self
                    .evaluations
                    .entry(solution)
                    .or_insert_with(|| SolutionEvaluationState::new(task));
                let testcase = eval.testcases.get_mut(&testcase).expect("Missing testcase");
                testcase.score = Some(score);
                if !testcase.status.has_completed() {
                    testcase.status = match ScoreStatus::from_score(score, 1.0) {
                        ScoreStatus::WrongAnswer => TestcaseEvaluationStatus::WrongAnswer(message),
                        ScoreStatus::Accepted => TestcaseEvaluationStatus::Accepted(message),
                        ScoreStatus::PartialScore => TestcaseEvaluationStatus::Partial(message),
                    };
                }
            }
            UIMessage::IOISubtaskScore {
                subtask,
                solution,
                score,
                normalized_score,
            } => {
                let task = &self.task;
                let eval = self
                    .evaluations
                    .entry(solution)
                    .or_insert_with(|| SolutionEvaluationState::new(task));
                let subtask = eval.subtasks.get_mut(&subtask).expect("Missing subtask");
                subtask.score = Some(score);
                subtask.normalized_score = Some(normalized_score);
            }
            UIMessage::IOITaskScore { solution, score } => {
                let task = &self.task;
                let eval = self
                    .evaluations
                    .entry(solution)
                    .or_insert_with(|| SolutionEvaluationState::new(task));
                eval.score = Some(score);
            }
            UIMessage::IOIBooklet { name, status } => {
                self.booklets
                    .entry(name)
                    .or_insert_with(|| BookletState {
                        status: UIExecutionStatus::Pending,
                        dependencies: HashMap::new(),
                    })
                    .status = status;
            }
            UIMessage::IOIBookletDependency {
                booklet,
                name,
                step,
                num_steps,
                status,
            } => {
                self.booklets
                    .entry(booklet)
                    .or_insert_with(|| BookletState {
                        status: UIExecutionStatus::Pending,
                        dependencies: HashMap::new(),
                    })
                    .dependencies
                    .entry(name)
                    .or_insert_with(|| {
                        (0..num_steps)
                            .map(|_| BookletDependencyState {
                                status: UIExecutionStatus::Pending,
                            })
                            .collect()
                    })
                    .get_mut(step)
                    .expect("Statement dependency step is gone")
                    .status = status;
            }
            UIMessage::Diagnostic { diagnostic } => {
                self.diagnostics.add_diagnostic(diagnostic);
            }
            UIMessage::TerryTask { .. }
            | UIMessage::TerryGeneration { .. }
            | UIMessage::TerryValidation { .. }
            | UIMessage::TerrySolution { .. }
            | UIMessage::TerryChecker { .. }
            | UIMessage::TerrySolutionOutcome { .. } => unreachable!("Terry message on IOI UI"),
        }
    }

    fn finish(&mut self) {
        finish_ui::FinishUI::print(self);
    }
}
