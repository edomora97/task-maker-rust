use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Error};

use task_maker_format::ioi::{
    InputGenerator, SubtaskInfo, TestcaseId, TestcaseInfo, GENERATION_PRIORITY,
};
use task_maker_format::{EvaluationData, TaskFormat};

/// The information about a testcase to generate.
#[derive(Debug, Clone, Default)]
pub struct TestcaseData {
    /// The arguments to pass to the generator for producing this input file.
    pub generator_args: Vec<String>,
    /// The seed used.
    pub seed: i32,
    /// The path of where to put this input file temporarily.
    pub input_path: PathBuf,
    /// The path of where to put the output file to check.
    pub output_path: PathBuf,
    /// The path of where to put the correct output file.
    pub correct_output_path: PathBuf,
}

/// A set of testcases that will be put in a single DAG.
#[derive(Debug, Clone, Default)]
pub struct Batch {
    pub testcases: HashMap<TestcaseId, TestcaseData>,
}

/// Modify the task changing the subtasks and testcases in order to produce a DAG that runs the test
/// testcases instead of the normal ones.
pub fn patch_task_for_batch(
    task: &mut TaskFormat,
    generator_args: &[String],
    batch_size: usize,
    batch_index: usize,
    working_directory: &Path,
) -> Result<Batch, Error> {
    let mut batch = Batch::default();

    match task {
        TaskFormat::IOI(task) => {
            // A template testcase for selecting the generator and validator.
            let testcase_template = task
                .testcases
                .values()
                .find(|tc| matches!(tc.input_generator, InputGenerator::Custom(_, _)))
                .cloned()
                // FIXME: in theory we can find the generator and the solution even without a testcase
                .ok_or_else(|| anyhow!("Failed to find a base testcase"))?;
            // Remove all the original testcases.
            task.subtasks.clear();
            // Create a single subtask with all the testcases of this batch.
            let mut testcases = HashMap::new();
            for testcase_index in 0..batch_size {
                let testcase_id = (batch_index * batch_size + testcase_index) as TestcaseId;

                // [0, i32::MAX] is a safe range for the seeds, since it is compatible with `stoi` in c++.
                let seed = fastrand::i32(0..i32::MAX);

                let generator_args = generator_args_for_testcase(generator_args, seed);
                let mut input_generator = testcase_template.input_generator.clone();
                match &mut input_generator {
                    InputGenerator::StaticFile(_) => {
                        unreachable!("The generator cannot be StaticFile")
                    }
                    InputGenerator::Custom(_, args) => {
                        *args = generator_args.clone();
                    }
                }

                let testcase = TestcaseInfo::new(
                    testcase_id,
                    input_generator,
                    task.input_validator_generator.generate(Some(0)),
                    testcase_template.output_generator.clone(),
                );

                let data = TestcaseData {
                    generator_args,
                    seed,
                    input_path: working_directory.join(format!("testcase-{}/input.txt", seed)),
                    output_path: working_directory.join(format!("testcase-{}/output.txt", seed)),
                    correct_output_path: working_directory
                        .join(format!("testcase-{}/correct_output.txt", seed)),
                };

                testcases.insert(testcase_id, testcase);
                batch.testcases.insert(testcase_id, data);
            }
            let subtask = SubtaskInfo {
                id: 0,
                name: Some(format!("batch-{}", batch_index)),
                max_score: 100.0,
                testcases: testcases.keys().cloned().collect(),
                testcases_owned: testcases.keys().cloned().collect(),
                is_default: false,
                ..Default::default()
            };
            task.subtasks.insert(0, subtask);
            task.testcases = testcases;
        }
        TaskFormat::Terry(_) => {
            bail!("Terry tasks are not currently supported")
        }
    }
    Ok(batch)
}

/// Produce the set of arguments of the generator replacing '{}' with the seed.
fn generator_args_for_testcase(args: &[String], seed: i32) -> Vec<String> {
    args.iter()
        .map(|arg| match arg.as_str() {
            "{}" => seed.to_string(),
            _ => arg.into(),
        })
        .collect()
}

/// Patch the DAG fixing the file callbacks. We want to redirect where the generated files are
/// stored into a temporary path, and copy them to the task directory only if needed. Furthermore
/// we want to save also the output produced by the solution to test. Additionally, we want to
/// change the priorities of the executions, making generations as important as executions (so that
/// we don't have to wait for all the generations before starting evaluating).
pub fn patch_dag(eval: &mut EvaluationData, batch_size: usize, batch: &Batch) -> Result<(), Error> {
    let mut processed = 0;
    let get_testcase_id = |path: &Path| -> Option<TestcaseId> {
        let file_name = path.file_name().expect("Path without a file name");
        let file_name = file_name.to_string_lossy().to_string();
        let put = file_name.rfind("put")?;
        let dot = file_name.rfind('.')?;
        #[allow(clippy::string_slice)] // The indexes come from char-aligned functions.
        let number = &file_name[put + 3..dot];
        number.parse::<TestcaseId>().ok()
    };

    // Redirect the file write_to to the temporary directory.
    if let Some(callbacks) = eval.dag.callbacks.as_mut() {
        for file_callback in callbacks.file_callbacks.values_mut() {
            if let Some(write_to) = &mut file_callback.write_to {
                let dest = write_to
                    .dest
                    .strip_prefix(&eval.task_root)
                    .with_context(|| {
                        format!(
                            "Found output file outside the task: {}",
                            write_to.dest.display()
                        )
                    })?;
                // This file is neither an input nor an output.
                if !dest.starts_with("input") && !dest.starts_with("output") {
                    continue;
                }
                let testcase_id = get_testcase_id(dest)
                    .ok_or_else(|| anyhow!("Cannot find the testcase id of {}", dest.display()))?;
                let testcase = batch.testcases.get(&testcase_id).ok_or_else(|| {
                    anyhow!(
                        "Testcase {} is not present in the batch (from {})",
                        testcase_id,
                        dest.display()
                    )
                })?;
                if dest.starts_with("input") {
                    write_to.dest = testcase.input_path.clone();
                } else if dest.starts_with("output") {
                    write_to.dest = testcase.correct_output_path.clone();
                }
                // Always write the file.
                write_to.allow_failure = true;
            }
        }
    }

    let get_testcase_id = |description: &str| -> Option<TestcaseId> {
        let start = description.rfind("testcase ")? + "testcase ".len();
        let end = description.rfind(", ")?;
        #[allow(clippy::string_slice)] // The indexes come from char-aligned functions.
        description[start..end].parse::<TestcaseId>().ok()
    };

    let mut new_file_callbacks = vec![];
    for group in eval.dag.data.execution_groups.values_mut() {
        for exec in group.executions.iter_mut() {
            if let Some(tag) = &mut exec.tag {
                if tag.name == "evaluation" {
                    // The priority of generation is GENERATION_PRIORITY - testcase id.
                    exec.priority = GENERATION_PRIORITY + 1;
                    processed += 1;
                    let stdout = exec.stdout.as_ref();
                    if let Some(stdout) = stdout {
                        let testcase_id = get_testcase_id(&exec.description).ok_or_else(|| {
                            anyhow!("Failed to find testcase id from '{}'", exec.description)
                        })?;
                        let testcase = batch.testcases.get(&testcase_id).ok_or_else(|| {
                            anyhow!(
                                "Testcase {} is not present in the batch (from {})",
                                testcase_id,
                                exec.description
                            )
                        })?;
                        new_file_callbacks.push((stdout.uuid, &testcase.output_path));
                    } else {
                        warn!("Execution '{}' doesn't capture stdout", exec.description);
                    }
                }
                if tag.name == "checking" {
                    // The priority of the checker is GENERATION_PRIORITY - testcase id.
                    exec.priority = GENERATION_PRIORITY + 1;
                    processed += 1;
                }
            }
        }
    }
    for (file_id, path) in new_file_callbacks {
        eval.dag.write_file_to_allow_fail(file_id, path, false);
    }

    if processed != batch_size * 2 {
        bail!(
            "Failed to find the {} executions: {} found",
            batch_size * 2,
            processed
        );
    }
    Ok(())
}
