use anyhow::{Context, Error};
use task_maker_diagnostics::Diagnostic;

use crate::ioi::IOITask;
use crate::sanity_checks::{make_sanity_check, SanityCheck, SanityCheckCategory};
use crate::UISender;
use task_maker_dag::File;

/// Run the custom checker with some nasty output files and expect the checker not to crash and to
/// score 0 points.
#[derive(Debug, Default)]
pub struct FuzzCheckerWithJunkOutput;
make_sanity_check!(FuzzCheckerWithJunkOutput);

lazy_static! {
    static ref JUNK_OUTPUTS: [(&'static str, Vec<u8>); 4] = [
        ("empty file", Vec::new()),
        ("bytes from 0 to 255", (0..=255u8).collect()),
        ("ASCII from 32 to 127", (32..=127u8).collect()),
        ("wibble monster", b"wibble monster".to_vec()),
    ];
}

impl SanityCheck for FuzzCheckerWithJunkOutput {
    type Task = IOITask;

    fn name(&self) -> &'static str {
        "FuzzCheckerWithJunkOutput"
    }

    fn category(&self) -> SanityCheckCategory {
        SanityCheckCategory::Checker
    }

    fn pre_hook(&self, task: &IOITask, eval: &mut crate::EvaluationData) -> Result<(), Error> {
        // Only tasks with a custom checker are supported.
        let checker = match &task.task_type {
            crate::ioi::TaskType::Batch(batch) => match &batch.checker {
                crate::ioi::Checker::Custom(_) => &batch.checker,
                _ => return Ok(()),
            },
            _ => return Ok(()),
        };

        // Find a testcase to use for fuzzing.
        let testcase = task
            .testcases
            .values()
            .find(|tc| tc.input_file.is_some() && tc.official_output_file.is_some());
        if testcase.is_none() {
            return Ok(());
        }
        let testcase = testcase.unwrap();
        let input = testcase.input_file.unwrap();
        let official_output = testcase.official_output_file.unwrap();

        for (description, content) in &*JUNK_OUTPUTS {
            let test_output = File::new(format!(
                "Junk input for fuzzing checker with '{}'",
                description
            ));
            let test_output_uuid = test_output.uuid;
            eval.dag.provide_content(test_output, content.to_vec());

            let sender = eval.sender.clone();
            let exec = checker
                .check(
                    eval,
                    None,
                    format!(
                        "Fuzzing checker with junk input '{}' (\"{}\")",
                        description,
                        String::from_utf8_lossy(
                            &content
                                .iter()
                                .flat_map(|&c| std::ascii::escape_default(c))
                                .collect::<Vec<_>>()
                        )
                    ),
                    input,
                    official_output,
                    test_output_uuid,
                    move |score, outcome| {
                        if score != 0.0 {
                            sender.add_diagnostic(Diagnostic::error(format!(
                                "Junk file '{}' scored {} (with message '{}')",
                                description, score, outcome
                            )))?;
                        }
                        Ok(())
                    },
                )
                .with_context(|| {
                    format!(
                        "Failed to build DAG for fuzzing the checker with '{}'",
                        description
                    )
                })?;

            eval.dag.add_execution(exec);
        }

        Ok(())
    }
}
