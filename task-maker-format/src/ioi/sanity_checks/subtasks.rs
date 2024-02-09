use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Error;
use blake3::{Hash, Hasher};
use itertools::Itertools;
use task_maker_dag::FileUuid;
use task_maker_diagnostics::Diagnostic;

use crate::ioi::SubtaskId;
use crate::sanity_checks::{make_sanity_check, SanityCheck, SanityCheckCategory};
use crate::{EvaluationData, IOITask};

/// Check that all the subtasks have a name.
#[derive(Debug, Default)]
pub struct MissingSubtaskNames;
make_sanity_check!(MissingSubtaskNames);

impl SanityCheck for MissingSubtaskNames {
    type Task = IOITask;

    fn name(&self) -> &'static str {
        "MissingSubtaskNames"
    }

    fn category(&self) -> SanityCheckCategory {
        SanityCheckCategory::Io
    }

    fn pre_hook(&self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        // Ignore the default subtask if it's missing the name.
        if task.subtasks.values().all(|st| st.is_default) {
            return Ok(());
        }

        let mut missing_name = vec![];
        for subtask_id in task.subtasks.keys().sorted() {
            let subtask = &task.subtasks[subtask_id];
            if subtask.name.is_none() {
                missing_name.push((
                    format!("Subtask {} ({} points)", subtask.id, subtask.max_score),
                    subtask.span.clone(),
                ));
            }
        }
        if !missing_name.is_empty() {
            let message = format!(
                "These subtasks are missing a name: {}",
                missing_name.iter().map(|(name, _)| name).join(", ")
            );
            let mut diagnostic = Diagnostic::warning(message);
            if missing_name.iter().any(|(_, span)| span.is_some()) {
                diagnostic = diagnostic
                    .with_help("Add '#STNAME: name' in gen/GEN after each subtask definition:");
            }
            for (_, span) in missing_name {
                if let Some(span) = span {
                    diagnostic = diagnostic.with_code_span(span);
                }
            }
            eval.add_diagnostic(diagnostic)?;
        }
        Ok(())
    }
}

/// Check that all the checks target at least one subtask.
#[derive(Debug, Default)]
pub struct InvalidSubtaskName;
make_sanity_check!(InvalidSubtaskName);

impl SanityCheck for InvalidSubtaskName {
    type Task = IOITask;

    fn name(&self) -> &'static str {
        "InvalidSubtaskName"
    }

    fn category(&self) -> SanityCheckCategory {
        SanityCheckCategory::Io
    }

    fn pre_hook(&self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        let subtask_names = task
            .subtasks
            .keys()
            .sorted()
            .filter_map(|st| task.subtasks[st].name.as_ref())
            .join(", ");
        for solution in &eval.solutions {
            for check in &solution.checks {
                let subtasks = task.find_subtasks_by_pattern_name(&check.subtask_name_pattern);
                if subtasks.is_empty() {
                    eval.add_diagnostic(
                        Diagnostic::error(format!(
                            "Invalid subtask name '{}' in solution '{}'",
                            check.subtask_name_pattern,
                            solution.source_file.relative_path().display()
                        ))
                        .with_note(format!("The valid names are: {}", subtask_names)),
                    )?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct TestcaseOutput {
    pub first_chunk: Option<Vec<u8>>,
    pub hash: Hash,
}

#[derive(Debug, Default)]
struct OutputHasher {
    subtask: SubtaskId,
    first_chunk: Option<Vec<u8>>,
    hasher: Hasher,
    output: Arc<Mutex<HashMap<SubtaskId, Vec<TestcaseOutput>>>>,
}

impl OutputHasher {
    pub fn new(
        subtask: SubtaskId,
        output: Arc<Mutex<HashMap<SubtaskId, Vec<TestcaseOutput>>>>,
    ) -> Self {
        Self {
            subtask,
            first_chunk: None,
            hasher: Hasher::new(),
            output,
        }
    }

    pub fn bind(
        eval: &mut EvaluationData,
        file: FileUuid,
        subtask: SubtaskId,
        output: Arc<Mutex<HashMap<SubtaskId, Vec<TestcaseOutput>>>>,
    ) {
        let mut hasher = Self::new(subtask, output);
        eval.dag
            .get_file_content_chunked(file, move |chunk| hasher.add_chunk(chunk));
    }

    pub fn add_chunk(&mut self, chunk: &[u8]) -> Result<(), Error> {
        if chunk.is_empty() {
            let hash = self.hasher.finalize();
            let mut output = self.output.lock().unwrap();
            if let Some(out) = output.get_mut(&self.subtask) {
                out.push(TestcaseOutput {
                    first_chunk: self.first_chunk.clone(),
                    hash,
                });
            };
        } else {
            if self.first_chunk.is_none() {
                self.first_chunk = Some(chunk.to_owned());
            }
            self.hasher.update(chunk);
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct AllOutputsEqual {
    outputs: Arc<Mutex<HashMap<SubtaskId, Vec<TestcaseOutput>>>>,
}
make_sanity_check!(AllOutputsEqual);

impl SanityCheck for AllOutputsEqual {
    type Task = IOITask;

    fn name(&self) -> &'static str {
        "AllOutputsEqual"
    }

    fn category(&self) -> SanityCheckCategory {
        SanityCheckCategory::Io
    }

    fn pre_hook(&self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        let mut outputs = self.outputs.lock().unwrap();
        for subtask in task.subtasks.values() {
            if subtask.testcases.len() >= 2 {
                outputs.insert(subtask.id, Vec::new());

                for testcase_id in &subtask.testcases {
                    let testcase = &task.testcases[testcase_id];
                    if let Some(output_file) = testcase.official_output_file {
                        OutputHasher::bind(eval, output_file, subtask.id, self.outputs.clone());
                    }
                }
            }
        }
        Ok(())
    }

    fn post_hook(&self, task: &Self::Task, eval: &mut EvaluationData) -> Result<(), Error> {
        let outputs = self.outputs.lock().unwrap();

        for (id, out) in outputs.iter() {
            let Some(subtask) = task.subtasks.get(id) else {
                continue;
            };
            if out.len() != subtask.testcases.len() {
                continue;
            }

            let first = &out[0];
            let all_equal = out.iter().all(|x| x.hash == first.hash);

            if all_equal {
                let name = subtask
                    .name
                    .as_ref()
                    .map(|name| format!(" ({})", name))
                    .unwrap_or_default();
                let message = format!("All outputs for subtask {id}{name} are identical");

                let mut diag = Diagnostic::warning(message);
                if let Ok(contents) = std::str::from_utf8(first.first_chunk.as_ref().unwrap()) {
                    let contents = contents.chars().take(20).join("");
                    diag = diag.with_note(format!("They all start with: {contents}"));
                }
                eval.add_diagnostic(diag)?;
            }
        }

        Ok(())
    }
}

/// Check if any subtask uses the deprecated `description` field.
#[derive(Debug, Default)]
pub struct DeprecatedDescriptionInSubtask;
make_sanity_check!(DeprecatedDescriptionInSubtask);

impl SanityCheck for DeprecatedDescriptionInSubtask {
    type Task = IOITask;

    fn name(&self) -> &'static str {
        "DeprecatedDescriptionInSubtask"
    }

    fn category(&self) -> SanityCheckCategory {
        SanityCheckCategory::Task
    }

    fn pre_hook(&self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        for subtask in task.subtasks.values() {
            #[allow(deprecated)]
            let description = subtask.description.as_deref();
            match (subtask.name.as_deref(), description) {
                (Some(a), Some(b)) if a != b => {
                    let mut diagnostic = Diagnostic::warning(format!(
                        "Subtask {} uses spaces and/or tabs in its name, which are now deprecated",
                        subtask.id
                    ));
                    if let Some(span) = subtask.span.clone() {
                        diagnostic = diagnostic.with_code_span(span);
                    }
                    eval.add_diagnostic(diagnostic)?;
                }
                _ => {}
            }
        }

        Ok(())
    }
}
