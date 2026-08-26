use std::io;
use std::path::Path;

use anstream::AutoStream;
use itertools::Itertools;
use task_maker_dag::{ExecutionResult, ExecutionStatus};

use crate::terry::ui_state::{SolutionState, SolutionStatus, UIState};
use crate::terry::CaseStatus;
use crate::ui::{FinishUI as FinishUITrait, FinishUIUtils, BLUE, BOLD, GREEN, RED, YELLOW};
use crate::{cwrite, cwriteln};

/// UI that prints to `stdout` the ending result of the evaluation of a IOI task.
pub struct FinishUI {
    /// Stream where to print to.
    stream: AutoStream<io::Stdout>,
}

impl FinishUITrait<UIState> for FinishUI {
    fn print(state: &UIState) {
        let mut ui = FinishUI {
            stream: anstream::stdout(),
        };

        ui.print_task_info(state);
        println!();
        FinishUIUtils::new(&mut ui.stream).print_compilations(&state.compilations);
        println!();
        ui.print_evaluations(state);
        ui.print_summary(state);
        println!();
        FinishUIUtils::new(&mut ui.stream).print_diagnostic_messages(&state.diagnostics);
    }
}

impl FinishUI {
    fn print_task_info(&mut self, state: &UIState) {
        cwrite!(self, BOLD, "Task:      ");
        println!("{} ({})", state.task.description, state.task.name);
        cwrite!(self, BOLD, "Path:      ");
        println!("{}", state.task.path.display());
        cwrite!(self, BOLD, "Max score: ");
        println!("{}", state.task.max_score);
    }

    fn print_evaluations(&mut self, state: &UIState) {
        cwriteln!(self, BLUE, "Evaluations");
        for (path, solution) in state.solutions.iter().sorted_by_key(|(n, _)| *n) {
            let name = path.file_name().expect("Invalid file name");
            cwrite!(self, BOLD, "{} ", Path::new(name).display());
            match &solution.outcome {
                Some(Ok(outcome)) => {
                    let score = outcome.score * state.task.max_score;
                    if abs_diff_eq!(outcome.score, 0.0) {
                        cwriteln!(self, RED, "{:.2} / {:.2}", score, state.task.max_score);
                    } else if abs_diff_eq!(outcome.score, 1.0) {
                        cwriteln!(self, GREEN, "{:.2} / {:.2}", score, state.task.max_score);
                    } else {
                        cwriteln!(self, YELLOW, "{:.2} / {:.2}", score, state.task.max_score);
                    }
                }
                Some(Err(e)) => {
                    println!();
                    cwrite!(self, RED, "Fail: ");
                    println!("{e}");
                }
                None => {
                    println!();
                }
            }
            if let Some(seed) = solution.seed {
                println!("      Seed: {seed}");
            }

            let print_result = |result: &Option<ExecutionResult>| {
                if let Some(result) = &result {
                    FinishUIUtils::print_time_memory(&result.resources);
                    if let ExecutionStatus::Success = result.status {
                    } else {
                        print!(" | ");
                        print!(
                            "{}",
                            FinishUIUtils::display_fail_execution_status(&result.status)
                        );
                    }
                    if result.was_cached {
                        print!(" (cached)");
                    }
                    if result.was_killed {
                        print!(" (killed)");
                    }
                } else {
                    print!("unknown");
                }
            };

            print!("Generation: ");
            print_result(&solution.generator_result);
            println!();
            self.print_stderr(&solution.generator_result);

            print!("Validation: ");
            print_result(&solution.validator_result);
            println!();
            self.print_stderr(&solution.validator_result);

            print!("Evaluation: ");
            print_result(&solution.solution_result);
            println!();
            self.print_stderr(&solution.solution_result);

            print!("   Checker: ");
            print_result(&solution.checker_result);
            println!();
            self.print_stderr(&solution.checker_result);

            self.print_feedback(solution);
            self.print_subtasks(solution);

            println!();
        }
    }

    /// Print the standard error in the provided, if present and not empty.
    fn print_stderr(&mut self, result: &Option<ExecutionResult>) {
        if let Some(res) = result {
            if let Some(content) = &res.stderr {
                let content = String::from_utf8_lossy(content);
                let content = content.trim();
                if !content.is_empty() {
                    cwriteln!(self, YELLOW, "Stderr:");
                    println!("{content}");
                }
            }
        }
    }

    fn print_feedback(&mut self, solution: &SolutionState) {
        let Some(Ok(outcome)) = &solution.outcome else {
            return;
        };

        let validation_results = &outcome.validation.cases;
        let evaluation_results = &outcome.feedback.cases;

        for (index, (val, feedback)) in validation_results
            .iter()
            .zip(evaluation_results)
            .enumerate()
        {
            print!("#{index:<3}  ");
            match val.status {
                CaseStatus::Missing => cwrite!(self, YELLOW, "Missing"),
                CaseStatus::Parsed => cwrite!(self, GREEN, " Valid "),
                CaseStatus::Invalid => cwrite!(self, RED, "Invalid"),
            }
            print!(" | ");
            if feedback.correct {
                cwrite!(self, GREEN, "Correct");
            } else {
                cwrite!(self, RED, "Wrong  ");
            }
            if let Some(message) = &val.message {
                print!(" | {message}");
            }
            if let Some(message) = &feedback.message {
                print!(" | {message}");
            }
            println!();
        }
    }

    fn print_subtasks(&mut self, solution: &SolutionState) {
        let Some(Ok(outcome)) = &solution.outcome else {
            return;
        };

        let Some(subtasks) = &outcome.subtasks else {
            return;
        };

        let evaluation_results = &outcome.feedback.cases;

        for (index, subtask) in subtasks.iter().enumerate() {
            print!("Subtask #{index:<2}  ");

            if abs_diff_eq!(subtask.score, 0.0) {
                cwrite!(
                    self,
                    RED,
                    "{:>5.2} / {:>5.2}",
                    subtask.score,
                    subtask.max_score
                );
            } else if abs_diff_eq!(subtask.score, subtask.max_score) {
                cwrite!(
                    self,
                    GREEN,
                    "{:>5.2} / {:>5.2}",
                    subtask.score,
                    subtask.max_score
                );
            } else {
                cwrite!(
                    self,
                    YELLOW,
                    "{:>5.2} / {:>5.2}",
                    subtask.score,
                    subtask.max_score
                );
            }

            print!(" | ");

            for &testcase in &subtask.testcases {
                // `testcase` is a checker-reported index (from the solution's own JSON stdout),
                // not something task-maker validated; a buggy checker can report an index past
                // the end of `evaluation_results`, so don't let that panic the whole evaluation.
                match evaluation_results.get(testcase) {
                    Some(result) if result.correct => cwrite!(self, GREEN, "{} ", testcase),
                    Some(_) => cwrite!(self, RED, "{} ", testcase),
                    None => cwrite!(self, YELLOW, "{}? ", testcase),
                }
            }

            println!();
        }
    }

    /// Print the summary of the solution results.
    fn print_summary(&mut self, state: &UIState) {
        cwriteln!(self, BLUE, "Summary");
        let max_len = FinishUIUtils::get_max_len(&state.solutions);
        for (path, solution) in state.solutions.iter().sorted_by_key(|(n, _)| *n) {
            print!(
                "{:>width$} ",
                Path::new(path.file_name().expect("Invalid file name")).display(),
                width = max_len
            );
            match &solution.outcome {
                Some(Ok(outcome)) => {
                    let score = outcome.score * state.task.max_score;
                    if abs_diff_eq!(outcome.score, 1.0) {
                        cwrite!(self, GREEN, "{:>3}", score.floor());
                    } else if abs_diff_eq!(outcome.score, 0.0) {
                        cwrite!(self, RED, "{:>3}", score.floor());
                    } else {
                        cwrite!(self, YELLOW, "{:>3}", score.floor());
                    }
                    for (val, feed) in outcome
                        .validation
                        .cases
                        .iter()
                        .zip(outcome.feedback.cases.iter())
                    {
                        match val.status {
                            CaseStatus::Missing => print!(" m"),
                            CaseStatus::Parsed => {
                                if feed.correct {
                                    cwrite!(self, GREEN, " c");
                                } else {
                                    cwrite!(self, RED, " w");
                                }
                            }
                            CaseStatus::Invalid => cwrite!(self, RED, " i"),
                        }
                    }
                }
                Some(Err(e)) => {
                    print!("    {e}");
                }
                None => {
                    if let SolutionStatus::Failed(e) = &solution.status {
                        print!("    {e}");
                    } else {
                        print!("    Failed");
                    }
                }
            }
            println!();
        }
    }
}
