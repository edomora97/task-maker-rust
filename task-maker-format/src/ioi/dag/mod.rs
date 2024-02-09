use serde::{Deserialize, Serialize};
use typescript_definitions::TypeScriptify;

pub use checker::Checker;
pub use input_generator::InputGenerator;
pub use input_validator::{InputValidator, TM_VALIDATION_FILE_NAME};
pub use output_generator::OutputGenerator;
use task_maker_dag::Priority;
pub use task_type::{BatchTypeData, CommunicationTypeData, TaskType, UserIo};

mod checker;
mod input_generator;
mod input_validator;
mod output_generator;
mod task_type;

/// Base priority for the generation executions.
pub const GENERATION_PRIORITY: Priority = 1_000_000;
/// Base priority for the evaluation executions.
pub const EVALUATION_PRIORITY: Priority = 1_000;
/// Base priority for booklets compilation.
pub const BOOKLET_PRIORITY: Priority = 10_000;

/// Maximum number of bytes of the captured standard error.
pub const STDERR_CONTENT_LENGTH: usize = 10 * 1024;

/// The aggregator of testcase scores for computing the subtask score.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, TypeScriptify)]
pub enum TestcaseScoreAggregator {
    /// Take the minimum of all the testcases, formally:
    ///
    /// `st_score = st_max_score * min(*testcase_scores)`
    Min,
    /// Sum the score of all the testcases, formally:
    ///
    /// `st_score = st_max_score * sum(*testcase_scores) / len(*testcase_scores)`
    Sum,
}

/// Bind the input/output of an execution to the input and output file of a testcase. It correctly
/// chooses if using stdin/stdout or using normal files by looking at the value set in the `Task`.
///
/// # Parameters
/// - `exec: Execution`
/// - `task: Task`
/// - `input: File`
/// - `validation_handle: Option<File>`
#[macro_export]
macro_rules! bind_exec_io {
    ($exec:expr, $task:expr, $input:expr, $validation_handle:expr) => {{
        match &$task.infile {
            None => $exec.stdin($input),
            Some(infile) => $exec.input($input, infile, false),
        };
        match $validation_handle {
            None => {}
            Some(file) => {
                $exec.input(file, "wait_for_validation", false);
            }
        };
        match &$task.outfile {
            None => $exec.stdout(),
            Some(outfile) => $exec.output(outfile),
        }
    }};
}

impl TestcaseScoreAggregator {
    /// Aggregate the scores of a subtask from an iterator with the scores of the testcases.
    pub(crate) fn aggregate<I: IntoIterator<Item = f64>>(&self, iter: I) -> f64 {
        match self {
            TestcaseScoreAggregator::Min => iter
                .into_iter()
                .min_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(1.0),
            TestcaseScoreAggregator::Sum => {
                let sum_count = iter
                    .into_iter()
                    .fold((0.0, 0), |prev, cur| (prev.0 + cur, prev.1 + 1));
                if sum_count.1 == 0 {
                    return 1.0;
                }
                sum_count.0 / (f64::from(sum_count.1))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use task_maker_dag::{ExecutionResourcesUsage, ExecutionResult, ExecutionStatus, File};
    use task_maker_lang::GraderMap;

    use crate::ioi::IOITask;
    use crate::ui::UIMessage;
    use crate::{EvaluationData, SourceFile, Tag};

    use super::*;

    fn make_task<P: Into<PathBuf>>(path: P) -> IOITask {
        IOITask {
            path: path.into(),
            task_type: TaskType::Batch(BatchTypeData {
                output_generator: None,
                checker: Checker::WhiteDiff,
            }),
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
            grader_map: Arc::new(GraderMap::new(Vec::<PathBuf>::new())),
            booklets: vec![],
            difficulty: None,
            syllabus_level: None,
            sanity_checks: Default::default(),
        }
    }

    #[test]
    fn test_aggregate_min() {
        let aggregator = TestcaseScoreAggregator::Min;
        let min = aggregator.aggregate(vec![1.0, 0.1, 0.5]);
        assert_abs_diff_eq!(0.1, min);
    }

    #[test]
    fn test_aggregate_min_empty() {
        let aggregator = TestcaseScoreAggregator::Min;
        let min = aggregator.aggregate(vec![]);
        assert_abs_diff_eq!(1.0, min);
    }

    #[test]
    fn test_aggregate_sum() {
        let aggregator = TestcaseScoreAggregator::Sum;
        let sum = aggregator.aggregate(vec![1.0, 0.1, 0.7]);
        assert_abs_diff_eq!(0.6, sum);
    }

    #[test]
    fn test_aggregate_sum_empty() {
        let aggregator = TestcaseScoreAggregator::Sum;
        let sum = aggregator.aggregate(vec![]);
        assert_abs_diff_eq!(1.0, sum);
    }

    #[test]
    fn test_input_generator_static() {
        let tmpdir = tempfile::TempDir::new().unwrap();
        let path = tmpdir.path().join("input.txt");
        std::fs::write(&path, "x").unwrap();
        let generator = InputGenerator::StaticFile(path);
        let (mut eval, _) = EvaluationData::new(tmpdir.path());
        let out = generator.generate_and_bind(&mut eval, 0, 0).unwrap();
        assert!(eval.dag.data.provided_files.contains_key(&out));
        assert!(eval
            .dag
            .file_callbacks()
            .get(&out)
            .unwrap()
            .write_to
            .is_some());
    }

    #[test]
    fn test_input_generator_static_not_found() {
        let tmpdir = tempfile::TempDir::new().unwrap();
        let path = tmpdir.path().join("input.txt");
        let generator = InputGenerator::StaticFile(path.clone());
        let (mut eval, _) = EvaluationData::new(tmpdir.path());
        let gen = generator.generate_and_bind(&mut eval, 0, 0);
        assert!(gen.is_err());
        let err = gen.unwrap_err().to_string();
        assert!(err.contains("COPY"));
        assert!(err.contains(path.to_string_lossy().as_ref()));
    }

    #[test]
    fn test_input_generator_custom() {
        let tmpdir = tempfile::TempDir::new().unwrap();
        let path = tmpdir.path().join("gen.py");
        std::fs::write(&path, "x").unwrap();
        let source = SourceFile::new(&path, "", "", None, None::<PathBuf>).unwrap();
        let generator = InputGenerator::Custom(Arc::new(source), vec![]);
        let (mut eval, _recv) = EvaluationData::new(tmpdir.path());
        let out = generator.generate_and_bind(&mut eval, 0, 0).unwrap();
        assert_eq!(eval.dag.data.provided_files.len(), 1);
        assert_eq!(eval.dag.data.execution_groups.len(), 1);
        let group = eval.dag.data.execution_groups.values().next().unwrap();
        assert_eq!(group.tag().as_ref().unwrap(), &Tag::Generation.into());
        assert_eq!(group.executions[0].stdout.as_ref().unwrap().uuid, out);
        assert!(eval
            .dag
            .file_callbacks()
            .get(&out)
            .unwrap()
            .write_to
            .is_some());
    }

    #[test]
    fn test_input_validator_assume_valid() {
        let validator = InputValidator::AssumeValid;
        let file = File::new("input");
        let (mut eval, _recv) = EvaluationData::new("");
        let out = validator
            .validate_and_bind(&mut eval, 0, None, 0, file.uuid)
            .unwrap();
        assert_eq!(eval.dag.data.provided_files.len(), 0);
        assert_eq!(eval.dag.data.execution_groups.len(), 0);
        assert!(out.is_none());
    }

    #[test]
    fn test_input_validator_custom() {
        let tmpdir = tempfile::TempDir::new().unwrap();
        let path = tmpdir.path().join("val.py");
        std::fs::write(&path, "x").unwrap();
        let source = SourceFile::new(&path, "", "", None, None::<PathBuf>).unwrap();
        let validator = InputValidator::Custom(Arc::new(source), vec![]);
        let file = File::new("input");
        let (mut eval, _recv) = EvaluationData::new(tmpdir.path());
        let out = validator
            .validate_and_bind(&mut eval, 0, None, 0, file.uuid)
            .unwrap();
        assert_eq!(eval.dag.data.provided_files.len(), 1);
        assert_eq!(eval.dag.data.execution_groups.len(), 1);
        let group = eval.dag.data.execution_groups.values().next().unwrap();
        assert_eq!(group.tag().as_ref().unwrap(), &Tag::Generation.into());
        assert_eq!(
            group.executions[0].stdout.as_ref().unwrap().uuid,
            out.unwrap()
        );
        assert_eq!(group.executions[0].env["TM_SUBTASK"], "0");
        assert_eq!(group.executions[0].env["TM_TESTCASE"], "0");
    }

    #[test]
    fn test_input_validator_custom_with_name() {
        let tmpdir = tempfile::TempDir::new().unwrap();
        let path = tmpdir.path().join("val.py");
        std::fs::write(&path, "x").unwrap();
        let source = SourceFile::new(&path, "", "", None, None::<PathBuf>).unwrap();
        let validator = InputValidator::Custom(Arc::new(source), vec![]);
        let file = File::new("input");
        let (mut eval, _recv) = EvaluationData::new(tmpdir.path());
        let out = validator
            .validate_and_bind(&mut eval, 0, Some("name"), 0, file.uuid)
            .unwrap();
        assert_eq!(eval.dag.data.provided_files.len(), 1);
        assert_eq!(eval.dag.data.execution_groups.len(), 1);
        let group = eval.dag.data.execution_groups.values().next().unwrap();
        assert_eq!(group.tag().as_ref().unwrap(), &Tag::Generation.into());
        assert_eq!(
            group.executions[0].stdout.as_ref().unwrap().uuid,
            out.unwrap()
        );
        assert_eq!(group.executions[0].env["TM_SUBTASK"], "0");
        assert_eq!(group.executions[0].env["TM_SUBTASK_NAME"], "name");
        assert_eq!(group.executions[0].env["TM_TESTCASE"], "0");
    }

    #[test]
    fn test_output_generator_static() {
        let tmpdir = tempfile::TempDir::new().unwrap();
        let path = tmpdir.path().join("output.txt");
        std::fs::write(&path, "x").unwrap();
        let generator = OutputGenerator::StaticFile(path);
        let file = File::new("input");
        let task = make_task(tmpdir.path());
        let (mut eval, _) = EvaluationData::new(tmpdir.path());
        let out = generator
            .generate_and_bind(&task, &mut eval, 0, 0, file.uuid, None)
            .unwrap()
            .unwrap();
        assert!(eval.dag.data.provided_files.contains_key(&out));
        assert!(eval
            .dag
            .file_callbacks()
            .get(&out)
            .unwrap()
            .write_to
            .is_some());
    }

    #[test]
    fn test_output_generator_static_not_found() {
        let tmpdir = tempfile::TempDir::new().unwrap();
        let path = tmpdir.path().join("output.txt");
        let generator = OutputGenerator::StaticFile(path.clone());
        let file = File::new("input");
        let task = make_task(tmpdir.path());
        let (mut eval, _) = EvaluationData::new(tmpdir.path());
        let gen = generator.generate_and_bind(&task, &mut eval, 0, 0, file.uuid, None);
        assert!(gen.is_err());
        let err = gen.unwrap_err().to_string();
        assert!(err.contains("Static output file not found"));
        assert!(err.contains(path.to_string_lossy().as_ref()));
    }

    #[test]
    fn test_output_generator_custom() {
        let tmpdir = tempfile::TempDir::new().unwrap();
        let path = tmpdir.path().join("sol.py");
        std::fs::write(&path, "x").unwrap();
        let source = SourceFile::new(&path, "", "", None, None::<PathBuf>).unwrap();
        let generator = OutputGenerator::Custom(Arc::new(source), vec![]);
        let file = File::new("input");
        let val = File::new("validation");
        let task = make_task(tmpdir.path());
        let (mut eval, _recv) = EvaluationData::new(tmpdir.path());
        let out = generator
            .generate_and_bind(&task, &mut eval, 0, 0, file.uuid, Some(val.uuid))
            .unwrap()
            .unwrap();
        assert_eq!(eval.dag.data.provided_files.len(), 1);
        assert_eq!(eval.dag.data.execution_groups.len(), 1);
        let group = eval.dag.data.execution_groups.values().next().unwrap();
        assert_eq!(group.tag().as_ref().unwrap(), &Tag::Generation.into());
        assert_eq!(group.executions[0].stdout.as_ref().unwrap().uuid, out);
        assert!(group.executions[0].dependencies().contains(&file.uuid));
        assert!(group.executions[0].dependencies().contains(&val.uuid));
        assert!(eval
            .dag
            .file_callbacks()
            .get(&out)
            .unwrap()
            .write_to
            .is_some());
    }

    #[test]
    fn test_checker_whitediff() {
        let checker = Checker::WhiteDiff;
        let (mut eval, _recv) = EvaluationData::new("");
        let input = File::new("input").uuid;
        let output = File::new("output").uuid;
        let test = File::new("test").uuid;
        checker
            .check_and_bind(&mut eval, 0, 0, "sol", input, output, test, |_, _| {
                panic!("the callback should not be called here")
            })
            .unwrap();
        assert_eq!(eval.dag.data.provided_files.len(), 0);
        assert_eq!(eval.dag.data.execution_groups.len(), 1);
        let group = eval.dag.data.execution_groups.values().next().unwrap();
        assert_eq!(group.tag().as_ref().unwrap(), &Tag::Checking.into());
        assert!(group.executions[0]
            .args
            .contains(&"--ignore-blank-lines".into()));
        assert!(group.executions[0]
            .args
            .contains(&"--ignore-space-change".into()));
        assert!(group.executions[0].dependencies().contains(&output));
        assert!(group.executions[0].dependencies().contains(&test));
    }

    #[test]
    fn test_checker_whitediff_correct() {
        let checker = Checker::WhiteDiff;
        let (mut eval, _recv) = EvaluationData::new("");
        let input = File::new("input").uuid;
        let output = File::new("output").uuid;
        let test = File::new("test").uuid;
        let cb_called = Arc::new(AtomicBool::new(false));
        let cb_called2 = cb_called.clone();
        let cb = move |score, mex| {
            assert_abs_diff_eq!(score, 1.0);
            assert_eq!(mex, "Output is correct");
            cb_called2.store(true, Ordering::Relaxed);
            Ok(())
        };
        checker
            .check_and_bind(&mut eval, 0, 0, "sol", input, output, test, cb)
            .unwrap();
        let callbacks = eval.dag.execution_callbacks().drain().next().unwrap().1;
        callbacks.on_done.into_iter().for_each(|cb| {
            cb(ExecutionResult {
                status: ExecutionStatus::Success,
                was_killed: false,
                was_cached: false,
                resources: ExecutionResourcesUsage {
                    cpu_time: 0.0,
                    sys_time: 0.0,
                    wall_time: 0.0,
                    memory: 0,
                },
                stdout: None,
                stderr: None,
            })
            .unwrap();
        });
        assert!(cb_called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_checker_whitediff_incorrect() {
        let checker = Checker::WhiteDiff;
        let (mut eval, _recv) = EvaluationData::new("");
        let input = File::new("input").uuid;
        let output = File::new("output").uuid;
        let test = File::new("test").uuid;
        let cb_called = Arc::new(AtomicBool::new(false));
        let cb_called2 = cb_called.clone();
        let cb = move |score, mex| {
            assert_abs_diff_eq!(score, 0.0);
            assert_eq!(mex, "Output is incorrect");
            cb_called2.store(true, Ordering::Relaxed);
            Ok(())
        };
        checker
            .check_and_bind(&mut eval, 0, 0, "sol", input, output, test, cb)
            .unwrap();
        let callbacks = eval.dag.execution_callbacks().drain().next().unwrap().1;
        callbacks.on_done.into_iter().for_each(|cb| {
            cb(ExecutionResult {
                status: ExecutionStatus::ReturnCode(1),
                was_killed: false,
                was_cached: false,
                resources: ExecutionResourcesUsage {
                    cpu_time: 0.0,
                    sys_time: 0.0,
                    wall_time: 0.0,
                    memory: 0,
                },
                stdout: None,
                stderr: None,
            })
            .unwrap();
        });
        assert!(cb_called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_checker_custom() {
        let tmpdir = tempfile::TempDir::new().unwrap();
        let path = tmpdir.path().join("check.py");
        std::fs::write(&path, "x").unwrap();
        let source = SourceFile::new(&path, "", "", None, None::<PathBuf>).unwrap();
        let checker = Checker::Custom(Arc::new(source));
        let (mut eval, _recv) = EvaluationData::new(tmpdir.path());
        let input = File::new("input").uuid;
        let output = File::new("output").uuid;
        let test = File::new("test").uuid;
        checker
            .check_and_bind(&mut eval, 0, 0, "sol", input, output, test, |_, _| {
                panic!("the callback should not be called here")
            })
            .unwrap();
        assert_eq!(eval.dag.data.provided_files.len(), 1);
        assert_eq!(eval.dag.data.execution_groups.len(), 1);
        let group = eval.dag.data.execution_groups.values().next().unwrap();
        assert_eq!(group.tag().as_ref().unwrap(), &Tag::Checking.into());
        assert!(group.executions[0].dependencies().contains(&input));
        assert!(group.executions[0].dependencies().contains(&output));
        assert!(group.executions[0].dependencies().contains(&test));
    }

    #[test]
    fn test_checker_custom_correct() {
        let tmpdir = tempfile::TempDir::new().unwrap();
        let path = tmpdir.path().join("check.py");
        std::fs::write(&path, "x").unwrap();
        let source = SourceFile::new(&path, "", "", None, None::<PathBuf>).unwrap();
        let checker = Checker::Custom(Arc::new(source));
        let (mut eval, _recv) = EvaluationData::new(tmpdir.path());
        let input = File::new("input").uuid;
        let output = File::new("output").uuid;
        let test = File::new("test").uuid;
        let cb_called = Arc::new(AtomicBool::new(false));
        let cb_called2 = cb_called.clone();
        let cb = move |score, mex| {
            assert_abs_diff_eq!(score, 1.0);
            assert_eq!(mex, "Ok!");
            cb_called2.store(true, Ordering::Relaxed);
            Ok(())
        };
        checker
            .check_and_bind(&mut eval, 0, 0, "sol", input, output, test, cb)
            .unwrap();
        let group = eval.dag.data.execution_groups.values().next().unwrap();
        let exec = group.executions[0].uuid;
        let on_done = eval.dag.execution_callbacks().get_mut(&exec).unwrap();
        on_done.on_done.remove(0)(ExecutionResult {
            status: ExecutionStatus::Success,
            was_killed: false,
            was_cached: false,
            resources: Default::default(),
            stdout: Some("1.0\n\n".into()),
            stderr: Some("Ok!\n\n".into()),
        })
        .unwrap();

        assert!(cb_called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_checker_custom_incorrect() {
        let tmpdir = tempfile::TempDir::new().unwrap();
        let path = tmpdir.path().join("check.py");
        std::fs::write(&path, "x").unwrap();
        let source = SourceFile::new(&path, "", "", None, None::<PathBuf>).unwrap();
        let checker = Checker::Custom(Arc::new(source));
        let (mut eval, _recv) = EvaluationData::new(tmpdir.path());
        let input = File::new("input").uuid;
        let output = File::new("output").uuid;
        let test = File::new("test").uuid;
        let cb_called = Arc::new(AtomicBool::new(false));
        let cb_called2 = cb_called.clone();
        let cb = move |score, mex| {
            assert_abs_diff_eq!(score, 0.0);
            assert_eq!(mex, "Ko!");
            cb_called2.store(true, Ordering::Relaxed);
            Ok(())
        };
        checker
            .check_and_bind(&mut eval, 0, 0, "sol", input, output, test, cb)
            .unwrap();
        let group = eval.dag.data.execution_groups.values().next().unwrap();
        let exec = group.executions[0].uuid;
        let on_done = eval.dag.execution_callbacks().get_mut(&exec).unwrap();
        on_done.on_done.remove(0)(ExecutionResult {
            status: ExecutionStatus::Success,
            was_killed: false,
            was_cached: false,
            resources: Default::default(),
            stdout: Some("0.0\n\n".into()),
            stderr: Some("Ko!\n\n".into()),
        })
        .unwrap();

        assert!(cb_called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_checker_custom_invalid_score() {
        let tmpdir = tempfile::TempDir::new().unwrap();
        let path = tmpdir.path().join("check.py");
        std::fs::write(&path, "x").unwrap();
        let source = SourceFile::new(&path, "", "", None, None::<PathBuf>).unwrap();
        let checker = Checker::Custom(Arc::new(source));
        let (mut eval, recv) = EvaluationData::new(tmpdir.path());
        let input = File::new("input").uuid;
        let output = File::new("output").uuid;
        let test = File::new("test").uuid;
        let cb = move |_, _| panic!("the callback should not be called here");
        checker
            .check_and_bind(&mut eval, 0, 0, "sol", input, output, test, cb)
            .unwrap();
        let group = eval.dag.data.execution_groups.values().next().unwrap();
        let exec = group.executions[0].uuid;
        let on_done = eval.dag.execution_callbacks().get_mut(&exec).unwrap();
        on_done.on_done.remove(0)(ExecutionResult {
            status: ExecutionStatus::Success,
            was_killed: false,
            was_cached: false,
            resources: Default::default(),
            stdout: Some(":<\n\n".into()),
            stderr: Some("Ko!\n\n".into()),
        })
        .unwrap();
        drop(eval);

        let diagnostics = recv
            .into_iter()
            .flat_map(|m| match m {
                UIMessage::Diagnostic { diagnostic } => Some(diagnostic),
                _ => None,
            })
            .collect_vec();
        let diagnostics = diagnostics
            .iter()
            .map(|d| d.message())
            .any(|m| m.contains("Checker returned an invalid score"));
        assert!(diagnostics);
    }
}
