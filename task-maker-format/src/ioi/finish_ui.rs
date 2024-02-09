use std::collections::HashMap;
use std::path::{Path, PathBuf};

use itertools::Itertools;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream};

use task_maker_dag::ExecutionStatus;

use crate::ioi::ui_state::{SolutionEvaluationState, TestcaseEvaluationStatus, UIState};
use crate::ioi::{
    IOITask, SolutionCheckOutcome, SolutionTestcaseEvaluationState, SubtaskId, TestcaseId,
};
use crate::ui::{
    FinishUI as FinishUITrait, FinishUIUtils, UIExecutionStatus, BLUE, BOLD, GREEN, ORANGE, RED,
    YELLOW,
};
use crate::{cwrite, cwriteln, ScoreStatus};

/// Percentage threshold for showing a resource usage in bold for a solution. If the maximum
/// cpu_time used by the solution among the testcases is X, all the cpu_time of that solution that
/// are >= X*BOLD_RESOURCE_THRESHOLD will be shown in bold. Same for the memory usage.
pub const BOLD_RESOURCE_THRESHOLD: f64 = 0.9;
/// Percentage threshold for showing a resource usage in yellow for a solution. If the cpu_time of
/// a solution is >= time limit of the task * YELLOW_RESOURCE_THRESHOLD, it is shown in yellow. Same
/// for the memory usage.
pub const YELLOW_RESOURCE_THRESHOLD: f64 = 0.6;

/// UI that prints to `stdout` the ending result of the evaluation of a IOI task.
pub struct FinishUI {
    /// Stream where to print to.
    stream: StandardStream,
}

impl FinishUITrait<UIState> for FinishUI {
    fn print(state: &UIState) {
        let mut ui = FinishUI {
            stream: StandardStream::stdout(ColorChoice::Auto),
        };
        ui.print_task_info(state);
        if !state.compilations.is_empty() {
            println!();
            FinishUIUtils::new(&mut ui.stream).print_compilations(&state.compilations);
        }
        if !state.booklets.is_empty() {
            println!();
            ui.print_booklets(state);
        }
        if !state.generations.is_empty() {
            println!();
            ui.print_generations(state);
        }
        if !state.evaluations.is_empty() {
            println!();
            ui.print_evaluations(state);
            if state.task.subtasks.values().all(|st| st.name.is_some()) {
                ui.print_subtask_checks_table(state);
            }
            ui.print_summary(state);
        }
        FinishUIUtils::new(&mut ui.stream).print_diagnostic_messages(&state.diagnostics);
    }
}

impl FinishUI {
    /// Print the basic task info.
    fn print_task_info(&mut self, state: &UIState) {
        cwrite!(self, BOLD, "Task:         ");
        println!("{} ({})", state.task.title, state.task.name);
        cwrite!(self, BOLD, "Path:         ");
        println!("{}", state.task.path.display());
        cwrite!(self, BOLD, "Max score:    ");
        println!("{}", state.max_score);
        cwrite!(self, BOLD, "Time limit:   ");
        println!(
            "{}",
            state
                .task
                .time_limit
                .map(|t| format!("{}s", t))
                .unwrap_or_else(|| "unlimited".to_string())
        );
        cwrite!(self, BOLD, "Memory limit: ");
        println!(
            "{}",
            state
                .task
                .memory_limit
                .map(|t| format!("{}MiB", t))
                .unwrap_or_else(|| "unlimited".to_string())
        );
    }

    /// Print all the booklet states.
    fn print_booklets(&mut self, state: &UIState) {
        cwriteln!(self, BLUE, "Statements");
        for name in state.booklets.keys().sorted() {
            let booklet = &state.booklets[name];
            cwrite!(self, BOLD, "{:<20}  ", name);
            self.print_execution_status(&booklet.status);
            println!();
            for name in booklet.dependencies.keys().sorted() {
                let dep = &booklet.dependencies[name];
                print!("  {:<18}  ", name);
                let mut first = true;
                for step in dep.iter() {
                    if first {
                        first = false;
                    } else {
                        print!(" | ");
                    }
                    self.print_execution_status(&step.status);
                }
                println!();
            }
        }
    }

    /// Print all the generation states.
    fn print_generations(&mut self, state: &UIState) {
        cwriteln!(self, BLUE, "Generations");
        for (st_num, subtask) in state.generations.iter().sorted_by_key(|(n, _)| *n) {
            cwrite!(self, BOLD, "Subtask {}", st_num);
            if let Some(name) = &state.task.subtasks[st_num].name {
                print!(" [{}]", name);
            }
            println!(": {} points", state.task.subtasks[st_num].max_score);
            for (tc_num, testcase) in subtask.testcases.iter().sorted_by_key(|(n, _)| *n) {
                print!("#{:<3} ", tc_num);

                let mut first = true;
                let mut gen_failed = false;
                let mut val_failed = false;
                if let Some(gen) = &testcase.generation {
                    if let ExecutionStatus::Success = gen.status {
                        cwrite!(self, GREEN, "Generated");
                    } else {
                        cwrite!(self, YELLOW, "Generation failed: {:?}", gen.status);
                        gen_failed = true;
                    }
                    first = false;
                }
                if let Some(val) = &testcase.validation {
                    if !first {
                        print!(" | ");
                    }
                    if let ExecutionStatus::Success = val.status {
                        cwrite!(self, GREEN, "Validated");
                    } else {
                        cwrite!(self, YELLOW, "Validation failed: {:?}", val.status);
                        val_failed = true;
                    }
                    first = false;
                }
                if let Some(sol) = &testcase.solution {
                    if !first {
                        print!(" | ");
                    }
                    if let ExecutionStatus::Success = sol.status {
                        cwrite!(self, GREEN, "Solved");
                    } else {
                        cwrite!(self, YELLOW, "Solution failed: {:?}", sol.status);
                    }
                }
                println!();
                if gen_failed {
                    let stderr = testcase.generation.as_ref().and_then(|g| g.stderr.as_ref());
                    if let Some(stderr) = stderr {
                        let stderr = String::from_utf8_lossy(stderr);
                        if !stderr.trim().is_empty() {
                            cwriteln!(self, BOLD, "Generation stderr:");
                            println!("{}", stderr.trim());
                        }
                    }
                }
                if val_failed {
                    let stderr = testcase.validation.as_ref().and_then(|g| g.stderr.as_ref());
                    if let Some(stderr) = stderr {
                        let stderr = String::from_utf8_lossy(stderr);
                        if !stderr.trim().is_empty() {
                            cwriteln!(self, BOLD, "Validation stderr:");
                            println!("{}", stderr.trim());
                        }
                    }
                }
            }
        }
    }

    /// Print all the evaluation states.
    fn print_evaluations(&mut self, state: &UIState) {
        cwriteln!(self, BLUE, "Evaluations");
        for path in state.evaluations.keys().sorted() {
            let eval = &state.evaluations[path];
            self.print_evaluation(
                path,
                eval.score.unwrap_or(0.0),
                state.max_score,
                eval,
                state,
            );
            println!();
        }
    }

    /// Print the state of the evaluation of a single solution.
    fn print_evaluation(
        &mut self,
        path: &Path,
        score: f64,
        max_score: f64,
        eval: &SolutionEvaluationState,
        state: &UIState,
    ) {
        let name = path
            .file_name()
            .expect("Invalid file name")
            .to_string_lossy();
        cwrite!(self, BOLD, "{}", name);
        print!(": ");
        self.print_score_frac(score, max_score, &state.task);
        println!();

        let results = eval
            .testcases
            .values()
            .flat_map(|tc| tc.results.iter())
            .filter_map(|e| e.as_ref())
            .map(|r| &r.resources);
        let (max_time, max_memory) = results.fold((0.0, 0), |(time, mem), r| {
            (f64::max(time, r.cpu_time), u64::max(mem, r.memory))
        });

        for (st_num, subtask) in eval.subtasks.iter().sorted_by_key(|(n, _)| *n) {
            cwrite!(self, BOLD, "Subtask #{}", st_num);
            if let Some(name) = &state.task.subtasks[st_num].name {
                print!(" [{}]", name);
            }
            print!(": ");
            let max_score = state.task.subtasks[st_num].max_score;
            let score = subtask.score.unwrap_or(0.0);
            self.print_score_frac(score, max_score, &state.task);
            println!();
            for tc_num in &state.task.subtasks[st_num].testcases_owned {
                let testcase = &eval.testcases[tc_num];
                self.print_testcase_outcome(&name, *tc_num, testcase, max_time, max_memory, state);
            }
        }
    }

    /// Print the testcase info line for a single solution.
    fn print_testcase_outcome(
        &mut self,
        name: &str,
        tc_num: TestcaseId,
        testcase: &SolutionTestcaseEvaluationState,
        max_time: f64,
        max_memory: u64,
        state: &UIState,
    ) {
        print!("{:3}) ", tc_num);
        let score = testcase.score.unwrap_or(0.0);
        let score_precision = Self::score_precision(&state.task);
        if abs_diff_eq!(score, 1.0) {
            cwrite!(self, GREEN, "[{:.prec$}]", score, prec = score_precision);
        } else if abs_diff_eq!(score, 0.0) {
            cwrite!(self, RED, "[{:.prec$}]", score, prec = score_precision);
        } else {
            cwrite!(self, YELLOW, "[{:.prec$}]", score, prec = score_precision);
        }
        // print the time and memory info
        for result in &testcase.results {
            if let Some(result) = result {
                print!(" [");
                let time_color = FinishUI::resource_color(
                    result.resources.cpu_time,
                    max_time * BOLD_RESOURCE_THRESHOLD,
                    state.task.time_limit.unwrap_or(f64::INFINITY) * YELLOW_RESOURCE_THRESHOLD,
                );
                let memory_color = FinishUI::resource_color(
                    result.resources.memory as f64,
                    max_memory as f64 * BOLD_RESOURCE_THRESHOLD,
                    state.task.memory_limit.unwrap_or(u64::MAX) as f64
                        * 1024.0
                        * YELLOW_RESOURCE_THRESHOLD,
                );
                cwrite!(self, time_color, "{:2.3}s", result.resources.cpu_time);
                print!(" | ");
                cwrite!(
                    self,
                    memory_color,
                    "{:3.1}MiB",
                    (result.resources.memory as f64) / 1024.0
                );
                print!("]");
            } else {
                print!(" [???]")
            }
        }
        print!(" {}", testcase.status.message());
        let mut was_killed = false;
        let mut was_cached = true;
        for res in testcase.results.iter().flatten() {
            was_killed |= res.was_killed;
            was_cached &= res.was_cached;
        }
        for result in testcase.results.iter().flatten() {
            match &result.status {
                ExecutionStatus::ReturnCode(code) => print!(": Exited with {}", code),
                ExecutionStatus::Signal(sig, name) => print!(": Signal {} ({})", sig, name),
                ExecutionStatus::InternalError(err) => print!(": Internal error: {}", err),
                _ => {}
            }
        }
        if was_killed {
            print!(" (killed)");
        }
        if was_cached {
            print!(" (from cache)");
        }
        if FinishUI::is_ansi() {
            self.print_right(format!("[{}]", name));
        }
        println!();
    }

    /// The number of significant digits to use for printing a score.
    fn score_precision(task: &IOITask) -> usize {
        let task_max_score_digits = task
            .subtasks
            .values()
            .map(|st| st.max_score)
            .sum::<f64>()
            .log10() as usize;
        task.score_precision + task_max_score_digits
    }

    fn print_summary(&mut self, state: &UIState) {
        let score_precision = state.task.score_precision;
        let column_width = score_precision + 4;
        cwriteln!(self, BLUE, "Summary");
        let max_len = FinishUIUtils::get_max_len(&state.evaluations);
        print!("{:width$} ", "", width = max_len);
        cwrite!(
            self,
            BOLD,
            "{:>width$.prec$} | ",
            state.max_score,
            width = column_width,
            prec = score_precision
        );
        for st_num in state.task.subtasks.keys().sorted() {
            let subtask = &state.task.subtasks[st_num];
            cwrite!(self, BOLD, " {:^3.0} ", subtask.max_score);
        }
        println!();
        for path in state.evaluations.keys().sorted() {
            let eval = &state.evaluations[path];
            print!(
                "{:>width$} ",
                path.file_name()
                    .expect("Invalid file name")
                    .to_string_lossy(),
                width = max_len
            );
            print!(
                "{:>width$.prec$} | ",
                eval.score.unwrap_or(0.0),
                width = column_width,
                prec = score_precision
            );
            for st_num in eval.subtasks.keys().sorted() {
                let subtask = &eval.subtasks[st_num];
                let score = subtask.score.unwrap_or(0.0);
                let normalized_score = subtask.normalized_score.unwrap_or(0.0);
                let color = self.score_color(normalized_score);
                cwrite!(self, color, " {:^3.0} ", score);
            }
            print!("  ");
            for st_num in eval.subtasks.keys().sorted() {
                let subtask = &eval.subtasks[st_num];
                let normalized_score = subtask.normalized_score.unwrap_or(0.0);
                let color = self.score_color(normalized_score);
                cwrite!(self, color, "[");
                let time_limit = state.task.time_limit;
                let memory_limit = state.task.memory_limit;
                let extra_time = state.config.extra_time;
                for tc_num in &state.task.subtasks[st_num].testcases_owned {
                    let testcase = &eval.testcases[tc_num];
                    let close_color = if testcase.is_close_to_limits(
                        time_limit,
                        extra_time,
                        memory_limit,
                        YELLOW_RESOURCE_THRESHOLD,
                    ) {
                        Some(&*ORANGE)
                    } else {
                        None
                    };
                    use TestcaseEvaluationStatus::*;
                    match testcase.status {
                        Accepted(_) => cwrite!(self, close_color.unwrap_or(&*GREEN), "A"),
                        WrongAnswer(_) => cwrite!(self, RED, "W"),
                        Partial(_) => cwrite!(self, close_color.unwrap_or(&*YELLOW), "P"),
                        TimeLimitExceeded => cwrite!(self, close_color.unwrap_or(&*RED), "T"),
                        WallTimeLimitExceeded => cwrite!(self, RED, "T"),
                        MemoryLimitExceeded => cwrite!(self, close_color.unwrap_or(&*RED), "M"),
                        RuntimeError => cwrite!(self, RED, "R"),
                        Failed => cwrite!(self, BOLD, "F"),
                        Skipped => cwrite!(self, BOLD, "S"),
                        _ => cwrite!(self, BOLD, "X"),
                    }
                }
                cwrite!(self, color, "]");
            }
            println!();
        }
        println!();
    }

    /// Print the score fraction of a solution using colors.
    fn print_score_frac(&mut self, score: f64, max_score: f64, task: &IOITask) {
        if max_score == 0.0 {
            print!(
                "{:.prec$} / {:.prec$}",
                score,
                max_score,
                prec = task.score_precision
            );
        } else {
            let color = self.score_color(score / max_score);
            cwrite!(
                self,
                color,
                "{:.prec$} / {:.prec$}",
                score,
                max_score,
                prec = task.score_precision
            );
        }
    }

    /// Color to use for displaying a score.
    fn score_color(&mut self, normalized_score: f64) -> &'static ColorSpec {
        match ScoreStatus::from_score(normalized_score, 1.0) {
            ScoreStatus::Accepted => &GREEN,
            ScoreStatus::WrongAnswer => &RED,
            ScoreStatus::PartialScore => &YELLOW,
        }
    }

    /// Color to use for displaying a resource usage.
    fn resource_color(value: f64, bold_threshold: f64, yellow_threshold: f64) -> ColorSpec {
        let mut color = ColorSpec::new();
        if value >= bold_threshold {
            color.set_bold(true);
        }
        if value >= yellow_threshold {
            color.set_fg(Some(Color::Yellow));
        }
        color
    }

    /// Print some text to the right of the screen. Note that this will print some ANSI escape
    /// sequences.
    fn print_right(&mut self, what: String) {
        // \x1b[1000C  move the cursor to the right margin
        // \x1b[{}D    move the cursor left by {} characters
        print!("\x1b[1000C\x1b[{}D{}", what.len() - 1, what);
    }

    /// Check if ANSI is supported: if not in windows and not in a "dumb" terminal.
    fn is_ansi() -> bool {
        !cfg!(windows) && std::env::var("TERM").map(|v| v != "dumb").unwrap_or(false)
    }

    /// Print the status of an `UIExecutionStatus` using colors.
    fn print_execution_status(&mut self, status: &UIExecutionStatus) {
        match status {
            UIExecutionStatus::Pending => print!("..."),
            UIExecutionStatus::Skipped => print!("skipped"),
            UIExecutionStatus::Started { .. } => cwrite!(self, YELLOW, "started"),
            UIExecutionStatus::Done { result } => match &result.status {
                ExecutionStatus::Success => cwrite!(self, GREEN, "Success"),
                _ => cwrite!(self, RED, "{:?}", result.status),
            },
        }
    }

    /// Print the table with the summary of the subtask checks.
    fn print_subtask_checks_table(&mut self, state: &UIState) {
        let check_results = state.run_solution_checks();
        let mut results: HashMap<PathBuf, HashMap<SubtaskId, Vec<SolutionCheckOutcome>>> =
            Default::default();
        for result in check_results {
            let sol = results.entry(result.solution.clone()).or_default();
            sol.entry(result.subtask_id).or_default().push(result);
        }
        let solutions = state
            .solutions
            .keys()
            .filter(|&p| !p.is_symlink())
            .sorted()
            .collect_vec();

        // The widths of the longest cell in each column. The first column contains the solution
        // names.
        let mut column_widths = vec![1; state.task.subtasks.len() + 1];

        // Compute the widths of all the columns, based on the cell content.
        for subtask in state.task.subtasks.values() {
            let column_index = subtask.id as usize + 1;
            column_widths[column_index] =
                column_widths[column_index].max(subtask.name.as_ref().unwrap().len());
        }
        for &solution_name in solutions.iter() {
            let solution = &state.solutions[solution_name];
            column_widths[0] = column_widths[0].max(solution.name.len());
            let solution_results = results.entry(solution_name.clone()).or_default();
            for st_num in state.task.subtasks.keys() {
                let subtask_results = solution_results.entry(*st_num).or_default();
                let cell = subtask_results
                    .iter()
                    .map(|outcome| outcome.check.result.as_compact_str())
                    .join(" ");
                let column_index = *st_num as usize + 1;
                column_widths[column_index] = column_widths[column_index].max(cell.len());
            }
        }

        // Print the header.
        cwriteln!(self, BLUE, "Subtask results");
        print!("{:width$}", "", width = column_widths[0]);
        for st_num in state.task.subtasks.keys().sorted() {
            let subtask = &state.task.subtasks[st_num];
            let width = column_widths[*st_num as usize + 1];
            print!(" | ");
            cwrite!(
                self,
                BOLD,
                "{:width$}",
                subtask.name.as_ref().unwrap(),
                width = width
            );
        }
        println!();

        // Print the solution lines
        for solution_name in solutions {
            let solution = &state.solutions[solution_name];
            print!("{:>width$}", solution.name, width = column_widths[0]);
            let solution_results = &results[solution_name];
            for st_num in state.task.subtasks.keys().sorted() {
                print!(" | ");
                let width = column_widths[*st_num as usize + 1];
                let subtask_results = &solution_results[st_num];
                if subtask_results.is_empty() {
                    print!("{:width$}", "?", width = width);
                } else {
                    let mut printed = 0;
                    for result in subtask_results {
                        if printed > 0 {
                            print!(" ");
                            printed += 1;
                        }
                        let as_str = result.check.result.as_compact_str();
                        let color = if result.success { &*GREEN } else { &*RED };
                        cwrite!(self, color, "{}", as_str);
                        printed += as_str.len();
                    }
                    let column_index = *st_num as usize + 1;
                    let remaining = column_widths[column_index] - printed;
                    print!("{:remaining$}", "", remaining = remaining);
                }
            }
            println!();
        }
        println!();
    }
}
