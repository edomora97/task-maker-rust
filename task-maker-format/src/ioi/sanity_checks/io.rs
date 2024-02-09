use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::ioi::IOITask;
use crate::sanity_checks::{make_sanity_check, SanityCheck, SanityCheckCategory};
use crate::EvaluationData;
use anyhow::Error;
use itertools::Itertools;
use task_maker_dag::FileUuid;
use task_maker_diagnostics::Diagnostic;

/// Check that the input and output files end with `\n`.
#[derive(Debug, Default)]
pub struct IOEndWithNewLine {
    /// The list of input files that triggered the warning.
    inputs: Arc<Mutex<Vec<String>>>,
    /// The list of output files that triggered the warning.
    outputs: Arc<Mutex<Vec<String>>>,
}
make_sanity_check!(IOEndWithNewLine);

/// Check that a file ends with `\n` and emit a warning if it doesn't. An empty file is considered
/// valid.
#[derive(Debug)]
pub struct CheckEndWithNewLine {
    /// Whether the last chunk ends with a new line.
    last_chunk_ends_with_new_line: bool,
    /// Whether the file is binary, if so, do not emit the warning.
    is_binary: bool,
    /// The path of the file that is being checked.
    path: String,
    /// Where to insert the warning.
    list: Arc<Mutex<Vec<String>>>,
}

impl CheckEndWithNewLine {
    pub fn new(path: String, list: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            last_chunk_ends_with_new_line: true,
            is_binary: false,
            path,
            list,
        }
    }

    pub fn bind(
        eval: &mut EvaluationData,
        file: FileUuid,
        path: String,
        list: Arc<Mutex<Vec<String>>>,
    ) {
        let mut checker = Self::new(path, list);
        eval.dag
            .get_file_content_chunked(file, move |chunk| checker.add_chunk(chunk));
    }

    pub fn add_chunk(&mut self, chunk: &[u8]) -> Result<(), Error> {
        self.is_binary |= chunk.contains(&0); // UTF-8 never contains NULL bytes.
        if chunk.is_empty() {
            if !self.last_chunk_ends_with_new_line && !self.is_binary {
                self.list.lock().unwrap().push(self.path.clone());
            }
        } else {
            self.last_chunk_ends_with_new_line = chunk.last().map(|&c| c == b'\n').unwrap_or(false);
        }
        Ok(())
    }

    pub fn emit_warning(
        eval: &mut EvaluationData,
        files: &[String],
        kind: &str,
    ) -> Result<(), Error> {
        if !files.is_empty() {
            let files: HashSet<_> = files.iter().collect();
            let message = format!(
                "These {} files don't end with a new line: {}",
                kind,
                files.iter().sorted().join(", ")
            );
            eval.add_diagnostic(
                Diagnostic::warning(message)
                    .with_note("It's bad practice to have files that do not end with new-line"),
            )?;
        }
        Ok(())
    }
}

impl SanityCheck for IOEndWithNewLine {
    type Task = IOITask;

    fn name(&self) -> &'static str {
        "IOEndWithNewLine"
    }

    fn category(&self) -> SanityCheckCategory {
        SanityCheckCategory::Io
    }

    fn pre_hook(&self, task: &IOITask, eval: &mut EvaluationData) -> Result<(), Error> {
        for (testcase_id, testcase) in task.testcases.iter() {
            if let Some(input_file) = testcase.input_file {
                CheckEndWithNewLine::bind(
                    eval,
                    input_file,
                    format!("input/input{}.txt", testcase_id),
                    self.inputs.clone(),
                );
            }
            if let Some(output_file) = testcase.official_output_file {
                CheckEndWithNewLine::bind(
                    eval,
                    output_file,
                    format!("output/output{}.txt", testcase_id),
                    self.outputs.clone(),
                );
            }
        }
        Ok(())
    }

    fn post_hook(&self, _task: &Self::Task, eval: &mut EvaluationData) -> Result<(), Error> {
        let inputs = self.inputs.lock().unwrap();
        CheckEndWithNewLine::emit_warning(eval, &inputs, "input")?;
        let outputs = self.outputs.lock().unwrap();
        CheckEndWithNewLine::emit_warning(eval, &outputs, "official output")?;

        Ok(())
    }
}
