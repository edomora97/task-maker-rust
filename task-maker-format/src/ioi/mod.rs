//! The IOI task format.
//!
//! In IOI-like tasks there is the concept of _subtask_ and of _testcase_: a testcase is a single
//! instance of the evaluation of a solution on a given input file, producing a single output file
//! which will be used for the scoring. For each solution every testcase is worth from 0.0 to 1.0
//! points.
//!
//! A subtask is a group of testcases, it has a `max_score` parameter which scales its value from
//! 0.0 to `max_score` points. For computing the score of the subtask a `TestcaseScoreAggregator` is
//! used. The score of the task for a solution is the sum of all the subtask scores.
//!
//! There are many different valid task types, the most common is `Batch` where the solution is
//! simply executed once per testcase, feeding in the input file (either via stdin or normal file)
//! and getting the output file (either via stdout or normal file). The output is then checked using
//! a `Checker`, a program that computes the score of the testcase given the input file, the output
//! file and the _correct_ output file (the one produced by the jury).

use std::collections::HashMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Error};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use typescript_definitions::TypeScriptify;
use unic::normal::StrNormalForm;
use wildmatch::WildMatch;

use curses_ui::CursesUI;
pub use dag::*;
pub use format::italian_yaml;
pub use statement::*;
pub use task_info::*;
use task_maker_dag::{ExecutionDAGConfig, FileUuid};
use task_maker_diagnostics::CodeSpan;
use task_maker_lang::GraderMap;
pub use ui_state::*;

use crate::ioi::format::italian_yaml::TM_ALLOW_DELETE_COOKIE;
use crate::ioi::italian_yaml::is_gen_gen_deletable;
use crate::sanity_checks::SanityChecks;
use crate::solution::SolutionInfo;
use crate::ui::*;
use crate::{EvaluationConfig, EvaluationData, TaskInfo, UISender};

mod curses_ui;
mod dag;
pub(crate) mod finish_ui;
mod format;
pub mod sanity_checks;
mod statement;
pub(crate) mod task_info;
pub(crate) mod ui_state;

/// In IOI tasks the subtask numbers are non-negative 0-based integers.
pub type SubtaskId = u32;
/// In IOI tasks the testcase numbers are non-negative 0-based integers.
pub type TestcaseId = u32;

/// This struct will manage the scores of a solution in a task and will emit the ui messages when
/// a new score is ready.
#[derive(Debug, Clone)]
pub struct ScoreManager {
    /// The path of the solution.
    solution: PathBuf,
    /// The scores of each subtask.
    subtask_scores: HashMap<SubtaskId, Option<f64>>,
    /// The maximum score of each subtask.
    max_subtask_scores: HashMap<SubtaskId, f64>,
    /// The scores of each testcase.
    testcase_scores: HashMap<TestcaseId, Option<f64>>,
    /// The list of testcases of each subtask.
    subtask_testcases: HashMap<SubtaskId, Vec<TestcaseId>>,
    /// The aggregator to use for computing the subtask scores.
    aggregator: TestcaseScoreAggregator,
}

/// A simple struct that generates input validators for a given subtask.
#[derive(Clone)]
pub struct InputValidatorGenerator(Arc<dyn Fn(Option<SubtaskId>) -> InputValidator + Send + Sync>);

impl Debug for InputValidatorGenerator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputValidatorGenerator").finish()
    }
}

impl Default for InputValidatorGenerator {
    fn default() -> Self {
        InputValidatorGenerator(Arc::new(|_| InputValidator::AssumeValid))
    }
}

impl InputValidatorGenerator {
    /// Build a generator based on a generating function.
    pub fn new<F: Fn(Option<SubtaskId>) -> InputValidator + Send + Sync + 'static>(f: F) -> Self {
        InputValidatorGenerator(Arc::new(f))
    }

    /// Obtain a validator for the given subtask.
    pub fn generate(&self, subtask: Option<SubtaskId>) -> InputValidator {
        (self.0)(subtask)
    }
}

/// Information about a generic IOI task.
#[derive(Debug, Clone, Serialize, Deserialize, TypeScriptify)]
pub struct IOITask {
    /// Path of the directory of the task.
    pub path: PathBuf,
    /// The type of the task.
    pub task_type: TaskType,
    /// The name of the task (the short one).
    pub name: String,
    /// The title of the task (the long one).
    pub title: String,
    /// The time limit for the execution of the solutions, if `None` it's unlimited.
    pub time_limit: Option<f64>,
    /// The memory limit in MiB of the execution of the solution, if `None` it's unlimited.
    pub memory_limit: Option<u64>,
    /// The input file for the solutions, usually `Some("input.txt")` or `None` (stdin).
    pub infile: Option<PathBuf>,
    /// The output file for the solutions, usually `Some("output.txt")` or `None` (stdout).
    pub outfile: Option<PathBuf>,
    /// The list of the subtasks.
    pub subtasks: HashMap<SubtaskId, SubtaskInfo>,
    /// The list of the testcases.
    pub testcases: HashMap<TestcaseId, TestcaseInfo>,
    /// The generator of validators for the various subtasks.
    #[serde(skip_serializing, skip_deserializing)]
    pub input_validator_generator: InputValidatorGenerator,
    /// The aggregator to use to compute the score of the subtask based on the score of the
    /// testcases.
    pub testcase_score_aggregator: TestcaseScoreAggregator,
    /// The number of decimal digits when displaying the scores.
    #[serde(default)]
    pub score_precision: usize,
    /// The graders registered for this task.
    pub grader_map: Arc<GraderMap>,
    /// The booklets to compile for this task.
    pub booklets: Vec<Booklet>,
    /// An integer that defines the difficulty of the task. Used only in booklet compilations.
    pub difficulty: Option<u8>,
    /// An integer that defines the level inside a _syllabus_ (for example for the Olympiads in
    /// Teams). Used only in booklet compilations.
    pub syllabus_level: Option<u8>,
    /// The sanity checks attached to this task. Wrapped in Arc since `SanityChecks` is not Clone.
    /// It's also not `Serialize` nor `Deserialize`, all the sanity checks will be lost on
    /// serialization.
    #[serde(skip_serializing, skip_deserializing)]
    pub sanity_checks: Arc<SanityChecks<IOITask>>,
}

/// A subtask of a IOI task.
#[derive(Debug, Clone, Serialize, Deserialize, TypeScriptify, Default)]
pub struct SubtaskInfo {
    /// The id of the subtask.
    pub id: SubtaskId,
    /// The name of the subtask.
    ///
    /// This is what is used for running the solutions' checks.
    pub name: Option<String>,
    /// Textual description of the subtask. (deprecated)
    #[deprecated(note = "Use the `name` field instead")]
    pub description: Option<String>,
    /// The maximum score of the subtask, must be >= 0.
    pub max_score: f64,
    /// The testcases inside this subtask.
    pub testcases: Vec<TestcaseId>,
    /// The original testcases inside this subtask.
    pub testcases_owned: Vec<TestcaseId>,
    /// The span of the definition of this subtask.
    pub span: Option<CodeSpan>,
    /// Whether this subtask was created automatically since no subtask was present in gen/GEN.
    pub is_default: bool,
    /// The list of the dependencies of this subtask.
    pub dependencies: Vec<SubtaskId>,
}

/// A testcase of a IOI task.
///
/// Every testcase has an input and an output that will be put in the input/ and output/ folders.
/// The files are written there only if it's not a dry-run and if the files are not static.
#[derive(Debug, Clone, Serialize, Deserialize, TypeScriptify)]
pub struct TestcaseInfo {
    /// The id of the testcase.
    pub id: TestcaseId,
    /// The generator of the input file for this testcase.
    pub input_generator: InputGenerator,
    /// The validator of the input file for this testcase.
    pub input_validator: InputValidator,
    /// The generator of the output file for this testcase.
    pub output_generator: OutputGenerator,
    /// The generated input file UUID. This is set only after the DAG is built.
    pub input_file: Option<FileUuid>,
    /// The generated official output file UUID. This is set only after the DAG is built.
    pub official_output_file: Option<FileUuid>,
}

impl IOITask {
    /// Try to make a `Task` from the specified path. Will return `Err` if the format of the task
    /// is not IOI or if the task is corrupted and cannot be parsed.
    pub fn new<P: AsRef<Path>>(path: P, eval_config: &EvaluationConfig) -> Result<IOITask, Error> {
        format::italian_yaml::parse_task(path, eval_config)
    }

    /// Create a "fake" `IOITask` that will not contain any data.
    ///
    /// This can be used to setup executions that are not related to tasks (e.g. booklet
    /// compilations).
    pub fn fake() -> IOITask {
        IOITask {
            path: Default::default(),
            task_type: TaskType::None,
            name: "".to_string(),
            title: "".to_string(),
            time_limit: None,
            memory_limit: None,
            infile: None,
            outfile: None,
            subtasks: Default::default(),
            testcases: Default::default(),
            input_validator_generator: Default::default(),
            testcase_score_aggregator: TestcaseScoreAggregator::Min,
            score_precision: 0,
            grader_map: Arc::new(GraderMap::new::<&Path>(vec![])),
            booklets: vec![],
            difficulty: None,
            syllabus_level: None,
            sanity_checks: Arc::new(Default::default()),
        }
    }

    /// Check if in the provided path there could be a IOI-like task.
    pub fn is_valid<P: AsRef<Path>>(path: P) -> bool {
        path.as_ref().join("task.yaml").exists()
    }

    /// Get the root directory of the task.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get the path relative to the task's root.
    pub fn path_of<'a>(&self, path: &'a Path) -> &'a Path {
        path.strip_prefix(&self.path).unwrap_or(path)
    }

    /// Get an appropriate `UI` for this task.
    pub fn ui(&self, ui_type: &UIType, config: ExecutionDAGConfig) -> Result<Box<dyn UI>, Error> {
        match ui_type {
            UIType::Raw => Ok(Box::new(RawUI::new())),
            UIType::Print => Ok(Box::new(PrintUI::new(UIState::new(self, config)))),
            UIType::Curses => Ok(Box::new(
                CursesUI::new(UIState::new(self, config)).context("Cannot build curses UI")?,
            )),
            UIType::Json => Ok(Box::new(JsonUI::new())),
            UIType::Silent => Ok(Box::new(SilentUI::new())),
        }
    }

    /// Add the executions required for evaluating this task to the execution DAG.
    pub fn build_dag(
        &mut self,
        eval: &mut EvaluationData,
        config: &EvaluationConfig,
    ) -> Result<(), Error> {
        eval.sender.send(UIMessage::IOITask {
            task: Box::new(self.clone()),
        })?;
        eval.solutions = config.find_solutions(
            &self.path,
            vec!["sol/*"],
            Some(self.grader_map.clone()),
            eval,
        );

        let solutions: Vec<_> = eval
            .solutions
            .clone()
            .into_iter()
            .map(|source| {
                let path = source.source_file.path.clone();
                Ok((
                    source,
                    Arc::new(Mutex::new(ScoreManager::new(
                        self,
                        eval.sender.clone(),
                        path,
                    )?)),
                ))
            })
            .collect::<Result<_, Error>>()?;

        let solution_info = solutions
            .iter()
            .map(|(solution, _)| SolutionInfo::from(solution))
            .collect_vec();
        eval.sender.send(UIMessage::Solutions {
            solutions: solution_info,
        })?;

        self.task_type
            .prepare_dag(eval)
            .context("Failed to prepare DAG")?;

        let mut generated_io: HashMap<_, _> = HashMap::new();

        for subtask in self.subtasks.values() {
            trace!("Executing the generation of subtask {}", subtask.id);

            for testcase_id in &subtask.testcases_owned {
                let testcase = self.testcases.get(testcase_id).unwrap();

                trace!(
                    "Executing the generation of testcase {} of subtask {}",
                    testcase.id,
                    subtask.id
                );

                let input = testcase
                    .input_generator
                    .generate_and_bind(eval, subtask.id, testcase.id)
                    .context("Failed to bind input generator")?;
                let val_handle = testcase
                    .input_validator
                    .validate_and_bind(
                        eval,
                        subtask.id,
                        subtask.name.as_deref(),
                        testcase.id,
                        input,
                    )
                    .context("Failed to bind validator")?;
                let output = testcase
                    .output_generator
                    .generate_and_bind(self, eval, subtask.id, testcase.id, input, val_handle)
                    .context("Failed to bind output generator")?;
                // Store the generated input and output files for setting them into the task
                // outside the loop.
                generated_io.insert(testcase.id, (input, output));

                for (solution, score_manager) in solutions.iter() {
                    trace!(
                        "Evaluation of the solution {:?} against subtask {} / testcase {}",
                        solution.source_file.name(),
                        subtask.id,
                        testcase.id
                    );

                    self.task_type
                        .evaluate(
                            self,
                            eval,
                            subtask.id,
                            testcase.id,
                            &solution.source_file,
                            input,
                            val_handle,
                            output,
                            score_manager.clone(),
                        )
                        .context("Failed to bind evaluation")?;
                }
            }
        }
        // Store inside the task the FileUuid of the input and official output files. This cannot
        // be done while generating because task cannot be borrowed mutably in the loop.
        for (testcase_id, (input, output)) in generated_io {
            let testcase = self.testcases.get_mut(&testcase_id).unwrap();
            testcase.input_file = Some(input);
            testcase.official_output_file = output;
        }
        for booklet in self.booklets.iter() {
            booklet
                .build(eval)
                .context("Failed to bind booklet compilation")?;
        }
        self.sanity_checks
            .pre_hook(self, eval)
            .context("Sanity check pre-hooks failed")?;
        Ok(())
    }

    /// Hook called after the execution completed, useful for sending messages to the UI about the
    /// results of the sanity checks with data available only after the evaluation.
    pub fn sanity_check_post_hook(&self, eval: &mut EvaluationData) -> Result<(), Error> {
        self.sanity_checks.post_hook(self, eval)
    }

    /// Clean the task folder removing the files that can be generated automatically.
    pub fn clean(&self) -> Result<(), Error> {
        for dir in &["input", "output"] {
            let dir = self.path.join(dir);
            if !dir.exists() {
                continue;
            }
            for file in glob::glob(dir.join("*.txt").to_string_lossy().as_ref())
                .context("Invalid glob pattern")?
            {
                let file = match file {
                    Ok(file) => file,
                    Err(e) => {
                        warn!("Glob error: {:?}", e);
                        continue;
                    }
                };
                // check if the file is used by a static generator
                if self.testcases.values().any(|tc| {
                    match (&tc.input_generator, &tc.output_generator) {
                        (InputGenerator::StaticFile(path), _)
                        | (_, OutputGenerator::StaticFile(path)) => path == &file,
                        _ => false,
                    }
                }) {
                    continue;
                }
                info!("Removing {}", file.display());
                std::fs::remove_file(&file)
                    .with_context(|| format!("Failed to clean file {}", file.display()))?;
            }
            info!("Removing {}", dir.display());
            if let Err(e) = std::fs::remove_dir(&dir) {
                // FIXME: this should be `e.kind() == ErrorKind::DirectoryNotEmpty`, but it is not
                //        stable yet.
                if e.to_string().contains("Directory not empty") {
                    warn!("Directory {} not empty!", dir.display());
                } else {
                    Err(e).with_context(|| format!("Cannot remove {}", dir.display()))?;
                }
            }
        }
        // remove the bin/ folder
        let bin_path = self.path.join("bin");
        if bin_path.exists() {
            info!("Removing {}", bin_path.display());
            std::fs::remove_dir_all(&bin_path).with_context(|| {
                format!("Failed to remove bin/ directory at {}", bin_path.display())
            })?;
        }
        // remove the compiled checkers
        if let TaskType::Batch(data) = &self.task_type {
            if let Checker::Custom(_) = data.checker {
                for checker in &["check/checker", "cor/correttore"] {
                    let path = self.path.join(checker);
                    if path.exists() {
                        info!("Removing {}", path.display());
                        std::fs::remove_file(&path).with_context(|| {
                            format!("Failed to remove compiled checker at {}", path.display())
                        })?;
                    }
                }
            }
        }
        // remove the gen/GEN if there is cases.gen
        let gen_gen_path = self.path.join("gen/GEN");
        let cases_gen_path = self.path.join("gen/cases.gen");
        if cases_gen_path.exists() && gen_gen_path.exists() {
            if is_gen_gen_deletable(&gen_gen_path)? {
                info!("Removing {}", gen_gen_path.display());
                std::fs::remove_file(&gen_gen_path).with_context(|| {
                    format!("Failed to remove gen/GEN at {}", gen_gen_path.display())
                })?;
            } else {
                warn!(
                    "Won't remove gen/GEN since it doesn't contain {}",
                    TM_ALLOW_DELETE_COOKIE
                );
            }
        }
        Ok(())
    }

    /// Get the task information.
    pub fn task_info(&self) -> Result<TaskInfo, Error> {
        Ok(TaskInfo::IOI(
            task_info::IOITaskInfo::new(self).context("Cannot produce IOI task info")?,
        ))
    }

    /// Find the list of all the subtasks that match the given pattern.
    fn find_subtasks_by_pattern_name(&self, pattern: impl AsRef<str>) -> Vec<&SubtaskInfo> {
        // Normalize the pattern; the subtask names are already normalized.
        let pattern = pattern.as_ref().nfkc().collect::<String>();
        let pattern = WildMatch::new(&pattern);
        let mut result = vec![];
        for subtask in self.subtasks.values() {
            if subtask.name_matches(&pattern) {
                result.push(subtask);
            }
        }
        result
    }
}

impl SubtaskInfo {
    /// Check if the pattern matches the subtaks name.
    fn name_matches(&self, pattern: &WildMatch) -> bool {
        if let Some(name) = &self.name {
            pattern.matches(name)
        } else {
            false
        }
    }
}

impl TestcaseInfo {
    /// Make a new instance of [`TestcaseInfo`].
    pub fn new(
        id: TestcaseId,
        input_generator: InputGenerator,
        input_validator: InputValidator,
        output_generator: OutputGenerator,
    ) -> Self {
        Self {
            id,
            input_generator,
            input_validator,
            output_generator,
            input_file: None,
            official_output_file: None,
        }
    }
}

impl FromStr for TestcaseScoreAggregator {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "min" | "GroupMin" => Ok(TestcaseScoreAggregator::Min),
            "sum" | "Sum" => Ok(TestcaseScoreAggregator::Sum),
            "GroupMul" | "GroupThreshold" => bail!("{s} is not supported yet"),
            _ => bail!("Invalid testcase score aggregator: {}", s),
        }
    }
}

impl ScoreManager {
    /// Make a new `ScoreManager` based on the subtasks and testcases of the specified task.
    pub fn new(
        task: &IOITask,
        sender: Arc<Mutex<UIMessageSender>>,
        solution: PathBuf,
    ) -> Result<ScoreManager, Error> {
        let mut this = ScoreManager {
            solution,
            subtask_scores: task.subtasks.keys().map(|st_num| (*st_num, None)).collect(),
            max_subtask_scores: task
                .subtasks
                .values()
                .map(|st| (st.id, st.max_score))
                .collect(),
            testcase_scores: task.testcases.keys().map(|&tc| (tc, None)).collect(),
            subtask_testcases: task
                .subtasks
                .iter()
                .map(|(st_num, st)| (*st_num, st.testcases.clone()))
                .collect(),
            aggregator: task.testcase_score_aggregator,
        };

        this.update_subtasks(sender)?;
        Ok(this)
    }

    /// Store the score of the testcase and eventually compute the score of the subtask and of the
    /// task.
    pub fn score(
        &mut self,
        subtask_id: SubtaskId,
        testcase_id: TestcaseId,
        score: f64,
        message: String,
        sender: Arc<Mutex<UIMessageSender>>,
    ) -> Result<(), Error> {
        // testcase score
        self.testcase_scores.insert(testcase_id, Some(score));
        sender.send(UIMessage::IOITestcaseScore {
            subtask: subtask_id,
            testcase: testcase_id,
            solution: self.solution.clone(),
            score,
            message,
        })?;

        self.update_subtasks(sender)?;
        Ok(())
    }

    fn update_subtasks(&mut self, sender: Arc<Mutex<UIMessageSender>>) -> Result<(), Error> {
        // subtasks' scores
        let mut updated = true;
        while updated {
            updated = false;

            for subtask_id in self.subtask_scores.keys().copied().collect_vec() {
                if self.subtask_scores[&subtask_id].is_none()
                    && self.subtask_testcases[&subtask_id]
                        .iter()
                        .all(|tc| self.testcase_scores[tc].is_some())
                {
                    let normalized_score = self.aggregator.aggregate(
                        self.subtask_testcases[&subtask_id]
                            .iter()
                            .map(|tc| self.testcase_scores[tc].unwrap()),
                    );

                    let subtask_score = self.max_subtask_scores[&subtask_id] * normalized_score;
                    self.subtask_scores.insert(subtask_id, Some(subtask_score));
                    sender.send(UIMessage::IOISubtaskScore {
                        subtask: subtask_id,
                        solution: self.solution.clone(),
                        score: subtask_score,
                        normalized_score,
                    })?;

                    updated = true;
                }
            }
        }

        // task score
        if self.subtask_scores.values().all(Option::is_some) {
            let task_score: f64 = self
                .subtask_scores
                .values()
                .map(|score| score.unwrap())
                .sum();
            sender.send(UIMessage::IOITaskScore {
                solution: self.solution.clone(),
                score: task_score,
            })?;
        }

        Ok(())
    }
}
