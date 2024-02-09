use std::collections::{BTreeSet, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Error};
use itertools::Itertools;
use regex::Regex;

use task_maker_dag::File;
use task_maker_diagnostics::Diagnostic;
use task_maker_lang::GraderMap;

use crate::ioi::sanity_checks::check_missing_graders;
use crate::ioi::{IOITask, InputGenerator, TaskType, TestcaseId};
use crate::sanity_checks::{make_sanity_check, SanityCheck, SanityCheckCategory};
use crate::{list_files, EvaluationData, SolutionCheck, SourceFile, UISender};

use super::io::CheckEndWithNewLine;

/// Check that all the graders are present inside att.
#[derive(Debug, Default)]
pub struct AttGraders;
make_sanity_check!(AttGraders);

impl SanityCheck for AttGraders {
    type Task = IOITask;

    fn name(&self) -> &'static str {
        "AttGraders"
    }

    fn category(&self) -> SanityCheckCategory {
        SanityCheckCategory::Attachments
    }

    fn pre_hook(&self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        check_missing_graders(task, eval, "att")
    }
}

/// Check that all the templates are present inside att.
#[derive(Debug, Default)]
pub struct AttTemplates;
make_sanity_check!(AttTemplates);

impl SanityCheck for AttTemplates {
    type Task = IOITask;

    fn name(&self) -> &'static str {
        "AttTemplates"
    }

    fn category(&self) -> SanityCheckCategory {
        SanityCheckCategory::Attachments
    }

    fn pre_hook(&self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        for grader in task.grader_map.all_paths() {
            let ext = grader
                .extension()
                .ok_or_else(|| anyhow!("Grader has no extension"))?
                .to_string_lossy();
            let att_name = format!("att/{}.{}", task.name, ext);
            let template = task.path.join(&att_name);
            if !template.exists() {
                let grader_name = task.path_of(grader);
                eval.add_diagnostic(
                    Diagnostic::warning(format!("Missing template at {}", att_name))
                        .with_note(format!("Because of {}", grader_name.display())),
                )?;
            }
        }
        Ok(())
    }
}

/// Check that the sample cases inside att are valid symlinks.
#[derive(Debug, Default)]
pub struct AttSampleFiles;
make_sanity_check!(AttSampleFiles);

impl AttSampleFiles {
    /// Extract the list of sample input files from the task.
    ///
    /// These files are the `#COPY` from the first subtask, if the first subtask only contains `#COPY`.
    fn extract_sample_files_from_task(task: &IOITask) -> Vec<PathBuf> {
        let mut testcases = vec![];
        let subtask = if let Some(subtask) = task.subtasks.get(&0) {
            subtask
        } else {
            return testcases;
        };
        for testcase_id in &subtask.testcases {
            let testcase = task.testcases.get(testcase_id).unwrap();
            match &testcase.input_generator {
                InputGenerator::StaticFile(path) => {
                    let path = path.canonicalize().unwrap_or_else(|_| path.clone());
                    testcases.push(path);
                }
                // This subtask is not with the sample cases.
                InputGenerator::Custom(_, _) => return vec![],
            }
        }
        testcases
    }
}

impl SanityCheck for AttSampleFiles {
    type Task = IOITask;

    fn name(&self) -> &'static str {
        "AttSampleFiles"
    }

    fn category(&self) -> SanityCheckCategory {
        SanityCheckCategory::Attachments
    }

    fn post_hook(&self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        let mut no_sample = true;
        let samples_from_task = Self::extract_sample_files_from_task(task);
        let mut samples_from_att = vec![];
        for sample in list_files(&task.path, vec!["att/*input*.txt", "att/*output*.txt"]) {
            no_sample = false;
            // Check if the file is a symlink.
            if let Ok(content) = sample.read_link() {
                // Check if the symlink is broken.
                if sample.canonicalize().is_err() {
                    eval.add_diagnostic(
                        Diagnostic::error(format!(
                            "Sample case {} is a broken link",
                            task.path_of(&sample).display()
                        ))
                        .with_note(format!("It points to {}", content.display())),
                    )?;
                }
            } else {
                eval.add_diagnostic(Diagnostic::warning(format!(
                    "Sample case {} is not a symlink",
                    task.path_of(&sample).display()
                )).with_help("Move this file in the statement folder and symlink it here. This way the sample file can be included in the compiled statement."))?;
            }
            if let Ok(path) = sample.canonicalize() {
                let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                if file_name.contains("input") {
                    samples_from_att.push(path);
                }
            }
        }
        let samples_from_task: BTreeSet<_> = samples_from_task.into_iter().collect();
        let samples_from_att: BTreeSet<_> = samples_from_att.into_iter().collect();
        if !samples_from_task.is_empty() && samples_from_task != samples_from_att {
            let missing_in_att: Vec<_> = samples_from_task.difference(&samples_from_att).collect();
            let missing_in_task: Vec<_> = samples_from_att.difference(&samples_from_task).collect();
            if !missing_in_att.is_empty() {
                let paths = missing_in_att
                    .into_iter()
                    .map(|p| task.path_of(p).to_string_lossy())
                    .join(", ");
                eval.add_diagnostic(
                    Diagnostic::error("Missing samples in att/")
                        .with_note(format!("Samples in the task, but not in att/: {}", paths)),
                )?;
            }
            if !missing_in_task.is_empty() {
                let paths = missing_in_task
                    .into_iter()
                    .map(|p| task.path_of(p).to_string_lossy())
                    .join(", ");
                eval.add_diagnostic(
                    Diagnostic::error("Missing samples in the task")
                        .with_note(format!("Samples in att/, but not in the task: {}", paths)),
                )?;
            }
        }
        if no_sample {
            eval.add_diagnostic(Diagnostic::warning("No sample file in att/"))?;
        }
        Ok(())
    }
}

/// Check that the input files inside the att folder are valid, the solution doesn't crash with them
/// and the sample output files score full score.
#[derive(Debug, Default)]
pub struct AttSampleFilesValid;
make_sanity_check!(AttSampleFilesValid);

impl SanityCheck for AttSampleFilesValid {
    type Task = IOITask;

    fn name(&self) -> &'static str {
        "AttSampleFilesValid"
    }

    fn category(&self) -> SanityCheckCategory {
        SanityCheckCategory::Attachments
    }

    fn pre_hook(&self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        let validator = &task.input_validator_generator;
        let task_type = if let TaskType::Batch(data) = &task.task_type {
            data
        } else {
            return Ok(());
        };
        let official_solution = &task_type.output_generator;
        let samples = get_sample_files(task, eval).context("Failed to get sample files")?;
        for (input, output) in samples {
            let input_name = task.path_of(&input).to_owned();
            let input_handle = File::new(format!("Sample input file at {}", input_name.display()));
            let input_uuid = input_handle.uuid;
            eval.dag
                .provide_file(input_handle, input)
                .context("Failed to provide sample input file")?;

            // validate the input file
            let (val_handle, val) = validator
                .generate(None)
                .validate(
                    eval,
                    format!("Validation of sample case {}", input_name.display()),
                    0,
                    Some("att"),
                    0,
                    input_uuid,
                )
                .context("Failed to validate sample input file")?;
            if let Some(mut val) = val {
                let input_name = input_name.clone();
                let sender = eval.sender.clone();
                val.capture_stderr(1024);
                eval.dag.on_execution_done(&val.uuid, move |res| {
                    if !res.status.is_success() {
                        let mut diagnostic = Diagnostic::error(format!(
                            "Sample input file {} is not valid",
                            input_name.display()
                        ))
                        .with_note(format!("The validator failed with: {:?}", res.status));
                        if let Some(stderr) = res.stderr {
                            diagnostic = diagnostic
                                .with_help("The validator stderr is:")
                                .with_help_attachment(stderr);
                        }
                        sender.add_diagnostic(diagnostic)?;
                    }
                    Ok(())
                });
                eval.dag.add_execution(val);
            }

            if let Some(solution) = &official_solution {
                let output_name = task.path_of(&output).to_owned();
                let output_handle =
                    File::new(format!("Sample output file at {}", output_name.display()));
                let output_uuid = output_handle.uuid;
                eval.dag
                    .provide_file(output_handle, output)
                    .context("Failed to provide sample output file")?;

                // generate the output file
                let (correct_output, sol) = solution
                    .generate(
                        task,
                        eval,
                        format!(
                            "Generation of output file relative to {}",
                            input_name.display()
                        ),
                        0,
                        0,
                        input_uuid,
                        val_handle,
                    )
                    .context("Failed to generate correct sample output file")?;
                let correct_output =
                    correct_output.ok_or_else(|| anyhow!("Missing official solution"))?;
                if let Some(mut sol) = sol {
                    sol.capture_stderr(1024);
                    let sender = eval.sender.clone();
                    eval.dag.on_execution_done(&sol.uuid, move |res| {
                        if !res.status.is_success() {
                            let mut diagnostic = Diagnostic::error(format!(
                                "Solution failed on sample input file {}",
                                input_name.display()
                            ))
                            .with_note(format!("The solution failed with: {:?}", res.status));
                            if let Some(stderr) = res.stderr {
                                diagnostic = diagnostic
                                    .with_help("The solution stderr is:")
                                    .with_help_attachment(stderr);
                            }
                            sender.add_diagnostic(diagnostic)?;
                        }
                        Ok(())
                    });
                    eval.dag.add_execution(sol);
                }

                // validate the output with the correct one
                let sender = eval.sender.clone();
                let chk = task_type
                    .checker
                    .check(
                        eval,
                        None,
                        format!("Checking sample output {}", output_name.display()),
                        input_uuid,
                        correct_output,
                        output_uuid,
                        move |score, message| {
                            if abs_diff_ne!(score, 1.0) {
                                sender.add_diagnostic(Diagnostic::warning(format!(
                                    "Sample output file {} scores {}: {}",
                                    output_name.display(),
                                    score,
                                    message
                                )))?;
                            }
                            Ok(())
                        },
                    )
                    .context("Failed to check sample files")?;
                eval.dag.add_execution(chk);
            }
        }
        Ok(())
    }
}

/// Search the input-output sample pairs inside the att folder. Returns a list of (input,output)
/// pairs with their numbers matching.
fn get_sample_files(
    task: &IOITask,
    eval: &mut EvaluationData,
) -> Result<Vec<(PathBuf, PathBuf)>, Error> {
    let regex = Regex::new(r"(in|out)put(\d+)\.txt$").unwrap();
    let extract_num = |path: &PathBuf| {
        let path_str = path.to_string_lossy();
        let caps = regex.captures(path_str.as_ref());
        if let Some(caps) = caps {
            if let Some(num) = caps.get(2) {
                let num: TestcaseId = num.as_str().parse().ok()?;
                return Some(num);
            }
        }
        None
    };
    let mut inputs: HashMap<_, Vec<_>> = HashMap::new();
    for input in list_files(&task.path, vec!["att/*input*.txt"]) {
        if let Some(num) = extract_num(&input) {
            inputs.entry(num).or_default().push(input);
        }
    }
    for (num, files) in inputs.iter().sorted() {
        if files.len() == 1 {
            continue;
        }
        let paths = files
            .iter()
            .map(|p| task.path_of(p).to_string_lossy())
            .join(", ");
        eval.add_diagnostic(
            Diagnostic::error(format!("Sample input {} is present more than once", num))
                .with_note(format!("Found at: {}", paths)),
        )?;
    }
    let mut outputs: HashMap<_, Vec<_>> = HashMap::new();
    for output in list_files(&task.path, vec!["att/*output*.txt"]) {
        if let Some(num) = extract_num(&output) {
            outputs.entry(num).or_default().push(output);
        }
    }
    for (num, files) in outputs.iter().sorted() {
        if files.len() == 1 {
            continue;
        }
        let paths = files
            .iter()
            .map(|p| task.path_of(p).to_string_lossy())
            .join(", ");
        eval.add_diagnostic(
            Diagnostic::error(format!("Sample output {} is present more than once", num))
                .with_note(format!("Found at: {}", paths)),
        )?;
    }
    let mut samples = Vec::new();
    for (num, inputs) in inputs {
        let output = if let Some(output) = outputs.remove(&num) {
            output[0].clone()
        } else {
            eval.add_diagnostic(Diagnostic::error(format!(
                "Sample input file {} does not have its output file",
                task.path_of(&inputs[0]).display()
            )))?;
            continue;
        };
        samples.push((inputs[0].clone(), output));
    }
    for (_, outputs) in outputs {
        eval.add_diagnostic(Diagnostic::error(format!(
            "Sample output file {} does not have its input file",
            task.path_of(&outputs[0]).display()
        )))?;
    }
    Ok(samples)
}

/// Check that the source files in att don't contain @check rules.
#[derive(Debug, Default)]
pub struct AttWithNoCheck;
make_sanity_check!(AttWithNoCheck);

impl SanityCheck for AttWithNoCheck {
    type Task = IOITask;

    fn name(&self) -> &'static str {
        "AttWithNoCheck"
    }

    fn category(&self) -> SanityCheckCategory {
        SanityCheckCategory::Attachments
    }

    fn pre_hook(&self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        for att in list_files(&task.path, vec!["att/*"]) {
            let path = task.path_of(&att);
            if let Ok(checks) = SolutionCheck::extract_check_list(&att, eval) {
                if let Some(check) = checks.first() {
                    eval.add_diagnostic(
                        Diagnostic::error(format!(
                            "@check rule found in an attachment: {}",
                            path.display()
                        ))
                        .with_code_span(check.code_span.clone()),
                    )?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct AttEndWithNewLine;
make_sanity_check!(AttEndWithNewLine);

impl SanityCheck for AttEndWithNewLine {
    type Task = IOITask;

    fn name(&self) -> &'static str {
        "AttEndWithNewLine"
    }

    fn category(&self) -> SanityCheckCategory {
        SanityCheckCategory::Attachments
    }

    fn pre_hook(&self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        let list = Arc::new(Mutex::new(Vec::new()));
        for att in list_files(&task.path, vec!["att/*"]) {
            let path = task.path_of(&att);

            let mut file = std::fs::File::open(&att)
                .with_context(|| format!("Failed to open attachment {}", path.display()))?;
            // Check the file size to avoid seeking to end-1 if the file is empty (which is not
            // allowed).
            let metadata = file
                .metadata()
                .with_context(|| format!("Failed to read file size at {}", path.display()))?;

            let mut buf = [0u8; 1];
            let chunk = if metadata.len() == 0 {
                &[]
            } else {
                file.seek(SeekFrom::End(-1))
                    .with_context(|| format!("Failed to seek to the end of {}", path.display()))?;
                file.read_exact(&mut buf)
                    .with_context(|| format!("Failed to read last byte of {}", path.display()))?;
                &buf[..]
            };

            let mut checker =
                CheckEndWithNewLine::new(path.to_string_lossy().to_string(), list.clone());
            checker.add_chunk(chunk)?;
            checker.add_chunk(&[])?;
        }
        CheckEndWithNewLine::emit_warning(eval, &list.lock().unwrap(), "attached")?;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct AttNoDirectory;
make_sanity_check!(AttNoDirectory);

impl SanityCheck for AttNoDirectory {
    type Task = IOITask;

    fn name(&self) -> &'static str {
        "AttNoDirectory"
    }

    fn category(&self) -> SanityCheckCategory {
        SanityCheckCategory::Attachments
    }

    fn pre_hook(&self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        let dir =
            std::fs::read_dir(task.path.join("att")).context("Failed to open att/ directory")?;
        for entry in dir {
            let entry = entry.context("Error while reading att/ content")?;
            let path = entry.path();
            let canonical_path = path
                .canonicalize()
                .with_context(|| format!("Failed to find canonical path of {}", path.display()))?;
            if canonical_path.is_dir() {
                eval.add_diagnostic(Diagnostic::error(format!(
                    "Only file attachments are supported: {} is a directory",
                    task.path_of(&path).display()
                )))?;
            }
        }
        Ok(())
    }
}

/// Check that the template and grader in att compile together
#[derive(Debug, Default)]
pub struct AttTemplatesShouldCompile;
make_sanity_check!(AttTemplatesShouldCompile);

impl SanityCheck for AttTemplatesShouldCompile {
    type Task = IOITask;

    fn name(&self) -> &'static str {
        "AttTemplatesShouldCompile"
    }

    fn category(&self) -> SanityCheckCategory {
        SanityCheckCategory::Attachments
    }

    fn pre_hook(&self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        for grader in task.grader_map.all_paths() {
            let ext = grader
                .extension()
                .ok_or_else(|| anyhow!("Grader has no extension"))?
                .to_string_lossy();
            let att_name = format!("att/{}.{}", task.name, ext);
            let template = task.path.join(&att_name);

            let grader_name = grader
                .file_name()
                .ok_or_else(|| anyhow!("Grader has no file name"))?
                .to_string_lossy();
            let att_grader_name = format!("att/{}", grader_name);
            let att_grader = task.path.join(&att_grader_name);

            // Only run the check if the grader is not a symlink, as otherwise we are already
            // testing this when evaluating sol/template.<ext>
            if att_grader.is_symlink() {
                continue;
            }
            let grader_map = GraderMap::new(vec![att_grader]);

            let source_file = SourceFile::new(
                template,
                &task.path,
                format!(
                    "Template {} compiled with attached grader {}",
                    att_name, att_grader_name
                ),
                Some(Arc::new(grader_map)),
                None::<String>,
            );
            if let Some(source_file) = source_file {
                source_file.prepare(eval)?;
            }
        }
        Ok(())
    }
}
