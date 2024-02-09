use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Error};
use clap::Parser;
use itertools::Itertools;

use task_maker_format::ioi::UIState;
use task_maker_format::ui::{StdoutPrinter, UIStateT, BLUE, BOLD, YELLOW};
use task_maker_format::{
    cwrite, cwriteln, EvaluationConfig, SolutionCheck, SolutionCheckResult, TaskFormat,
    TestcaseEvaluationResult,
};
use task_maker_lang::LanguageManager;

use crate::context::RuntimeContext;
use crate::{ExecutionOpt, FilterOpt, FindTaskOpt, LoggerOpt, StorageOpt, UIOpt};

#[derive(Parser, Debug, Clone)]
pub struct AddSolutionChecksOpt {
    #[clap(flatten, next_help_heading = Some("TASK SEARCH"))]
    pub find_task: FindTaskOpt,

    #[clap(flatten, next_help_heading = Some("UI"))]
    pub ui: UIOpt,

    #[clap(flatten, next_help_heading = Some("STORAGE"))]
    pub storage: StorageOpt,

    #[clap(flatten, next_help_heading = Some("FILTER"))]
    pub filter: FilterOpt,

    #[clap(flatten, next_help_heading = Some("EXECUTION"))]
    pub execution: ExecutionOpt,

    /// Write the @check directly to the solution files.
    ///
    /// Warning: while this is generally safe, make sure to have a way of reverting the changes.
    #[clap(long, short)]
    pub in_place: bool,

    /// Overwrite existing @check rules.
    ///
    /// Warning: this will remove the existing lines containing the @check rules.
    #[clap(long, short)]
    pub overwrite: bool,
}

pub fn main_add_solution_checks(
    mut opt: AddSolutionChecksOpt,
    logger_opt: LoggerOpt,
) -> Result<(), Error> {
    opt.ui.disable_if_needed(&logger_opt);
    let eval_config = EvaluationConfig {
        solution_filter: opt.filter.filter,
        booklet_solutions: false,
        no_statement: true,
        solution_paths: opt.filter.solution,
        disabled_sanity_checks: Default::default(),
        seed: Default::default(),
        dry_run: true,
    };
    let task = opt
        .find_task
        .find_task(&eval_config)
        .context("Failed to locate the task")?;

    // This is a mutex because this state is updated in the UI thread, but it will later be used by
    // this main thread. In theory after executor.execute() the UI thread should have exited, so we
    // are the only owner of this state, but at the moment it's hard to express.
    let ui_state = Arc::new(Mutex::new(None::<UIState>));

    // setup the configuration and the evaluation metadata
    let context = RuntimeContext::new(task, &opt.execution, |task, eval| {
        // build the DAG for the task
        task.build_dag(eval, &eval_config)
            .context("Cannot build the task DAG")?;
        let ioi_task = match &task {
            TaskFormat::IOI(task) => {
                if task.subtasks.values().any(|st| st.name.is_none()) {
                    bail!("Not all the subtasks have a name, cannot proceed");
                }
                task
            }
            _ => bail!("The add-solution-checks tool only supports IOI-tasks for now"),
        };
        *ui_state.lock().unwrap() = Some(UIState::new(ioi_task, eval.dag.data.config.clone()));
        Ok(())
    })?;

    // start the execution
    let executor = context.connect_executor(&opt.execution, &opt.storage)?;
    let executor = executor.start_ui(&opt.ui.ui, {
        let ui_state = ui_state.clone();
        move |ui, message| {
            ui.on_message(message.clone());
            ui_state.lock().unwrap().as_mut().unwrap().apply(message);
        }
    })?;
    executor.execute()?;

    let mut printer = StdoutPrinter::default();
    cwriteln!(printer, BLUE, "Solution checks");
    let ui_state = ui_state.lock().unwrap().take().unwrap();
    let mut skipped = vec![];
    let mut changes_to_write = false;
    for solution_name in ui_state.solutions.keys() {
        let solution = &ui_state.solutions[solution_name];
        if solution.path.is_symlink() {
            continue;
        }
        if !opt.overwrite && !solution.checks.is_empty() {
            skipped.push(&solution.name);
            continue;
        }
        let checks_to_remove = if opt.overwrite {
            &solution.checks[..]
        } else {
            &[]
        };
        let has_changes = process_solution(
            &ui_state,
            solution_name,
            &mut printer,
            opt.in_place,
            checks_to_remove,
        );
        if has_changes && !opt.in_place {
            changes_to_write = true;
        }
        println!();
    }

    if !skipped.is_empty() {
        cwrite!(printer, YELLOW, "Warning");
        println!(
            ": These solutions already have at least one check, so they have been skipped: {}",
            skipped.iter().join(", ")
        );
    }
    if changes_to_write {
        cwrite!(printer, BLUE, "Note");
        println!(": The comments above have not been written to the solution files. To do this automatically pass -i.");
    }

    Ok(())
}

/// Generate (and add with in_place) the @check comments to this solution.
fn process_solution(
    state: &UIState,
    solution_name: &Path,
    printer: &mut StdoutPrinter,
    in_place: bool,
    checks_to_remove: &[SolutionCheck],
) -> bool {
    let solution = &state.solutions[solution_name];
    let language = LanguageManager::detect_language(solution_name);

    let solution_results = if let Some(solution_results) = state.evaluations.get(solution_name) {
        solution_results
    } else {
        println!("Solution '{}' not evaluated, skipping", solution.name);
        return false;
    };

    let mut checks: HashMap<_, Vec<_>> = HashMap::new();
    for st_num in solution_results.subtasks.keys().sorted() {
        let subtask = &state.task.subtasks[st_num];

        let testcase_results: Option<Vec<TestcaseEvaluationResult>> = state.task.subtasks[st_num]
            .testcases
            .iter()
            .map(|tc_num| (&solution_results.testcases[tc_num].status).into())
            .collect();
        let Some(testcase_results) = testcase_results else {
            println!(
                "Solution '{}' not evaluated on all the testcases, skipping.",
                solution.name
            );
            return false;
        };

        let check = choose_check_for_subtask(testcase_results);
        checks
            .entry(check)
            .or_default()
            .push(subtask.name.as_deref().unwrap());
    }

    let comments = checks
        .into_iter()
        .sorted_by_key(|(result, _)| *result)
        .map(|(result, subtasks)| {
            let prefix = language
                .as_ref()
                .and_then(|lang| lang.inline_comment_prefix())
                .unwrap_or_default();
            let subtasks = subtasks.iter().join(" ");
            format!("{} @check-{}: {}", prefix, result.as_str(), subtasks)
        })
        .collect_vec();
    let mut written = "";
    if in_place && !comments.is_empty() {
        if let Err(e) = write_comments_to_file(&solution.path, &comments, checks_to_remove)
            .with_context(|| {
                format!(
                    "Failed to write @check comments to '{}'",
                    solution.path.display()
                )
            })
        {
            eprintln!("Error: {:?}", e);
        } else {
            written = " (written!)";
        }
    }
    cwrite!(printer, BOLD, "{}", solution.name);
    println!("{}\n{}", written, comments.iter().join("\n"));

    !comments.is_empty()
}

/// Find the check that is most appropriate for the results of a given subtask.
fn choose_check_for_subtask(
    mut testcase_results: Vec<TestcaseEvaluationResult>,
) -> SolutionCheckResult {
    // Deduplicate the testcase results.
    testcase_results.sort_by_key(|&res| res as u32);
    testcase_results.dedup();

    // Find the testcases for which there aren't "greater" testcases.
    // E.g. [MLE AC TLE] --> [MLE TLE] (note: AC > MLE, AC > TLE).
    let minimal_results = testcase_results
        .iter()
        .copied()
        .filter(|res| {
            testcase_results
                .iter()
                .all(|res2| res2.partial_cmp(res) != Some(Ordering::Less))
        })
        .collect_vec();

    // Find the smallest check that includes every minimal testcase.
    for check in SolutionCheckResult::sorted_all() {
        if minimal_results
            .iter()
            .all(|&res| check.minimals().contains(&res))
        {
            return *check;
        }
    }

    unreachable!("failed to find check for subtask")
}

fn write_comments_to_file(
    path: &Path,
    comments: &[String],
    checks_to_remove: &[SolutionCheck],
) -> Result<(), Error> {
    let mut file =
        File::open(path).with_context(|| format!("Failed to open '{}'", path.display()))?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .context("Failed to read solution content")?;
    drop(file);

    // Remove existing @check rules.
    let mut lines: Vec<_> = content.lines().collect();
    let lines_to_remove: HashSet<_> = checks_to_remove
        .iter()
        .map(|c| c.code_span.line_number() - 1)
        .collect();
    let lines_to_remove = lines_to_remove.iter().sorted().rev();
    for line in lines_to_remove {
        lines.remove(*line);
    }

    // If the source file starts with the shebang, we cannot simply add the comments at the
    // beginning.
    #[allow(clippy::bool_to_int_with_if)]
    let insert_position = if content.starts_with("#!") { 1 } else { 0 };
    let comments = comments.iter().join("\n");
    lines.insert(insert_position, &comments);

    let new_content = lines.join("\n") + "\n";

    std::fs::write(path, new_content.as_bytes()).with_context(|| {
        format!(
            "Failed to write the source file content to {}",
            path.display()
        )
    })?;
    Ok(())
}
