use std::path::Path;

use itertools::Itertools;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{Block, Borders, Paragraph};

use task_maker_dag::ExecutionStatus;

use crate::ioi::finish_ui::{FinishUI, YELLOW_RESOURCE_THRESHOLD};
use crate::ioi::{
    SolutionTestcaseEvaluationState, SubtaskId, TestcaseEvaluationStatus, TestcaseGenerationStatus,
    UIState,
};
use crate::ui::curses::{
    compilation_status_text, draw_compilations, inner_block, render_block, render_server_status,
    CursesDrawer, CursesUI as GenericCursesUI, FrameType, GREEN, ORANGE, RED, YELLOW,
};
use crate::ui::UIExecutionStatus;
use crate::ScoreStatus;

/// An animated UI for IOI tasks, dynamically refreshing using curses as a backend.
pub(crate) type CursesUI = GenericCursesUI<UIState, Drawer, FinishUI>;

/// The drawer of the IOI CursesUI.
pub(crate) struct Drawer;

impl CursesDrawer<UIState> for Drawer {
    fn draw(state: &UIState, frame: &mut FrameType, loading: char, frame_index: usize) {
        draw_frame(state, frame, loading, frame_index);
    }
}

/// Draw a frame of interface to the provided `Frame`.
fn draw_frame(state: &UIState, f: &mut FrameType, loading: char, frame_index: usize) {
    let size = f.size();
    if size.width < 16 || size.height < 16 {
        let error = Span::styled("Too small", Style::default().add_modifier(Modifier::BOLD));
        let paragraph = Paragraph::new(error);
        f.render_widget(paragraph, size);
        return;
    }
    let header: Spans = vec![
        Span::styled(
            state.task.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ("),
        Span::raw(state.task.name.clone()),
        Span::raw(")"),
    ]
    .into();
    let header_len = 2;
    let num_compilations = state
        .compilations
        .iter()
        .filter(|(k, _)| !state.evaluations.contains_key(*k))
        .count();
    let compilations_len = if num_compilations > 0 {
        num_compilations as u16 + 2
    } else {
        0
    };
    let booklet_len = if state.booklets.is_empty() {
        0
    } else {
        state
            .booklets
            .values()
            .map(|s| s.dependencies.len() as u16 + 1)
            .sum::<u16>()
            + 2
    };
    let generations_len = if state.generations.is_empty() { 0 } else { 3 };
    let evaluations_len = state.evaluations.len() as u16 + 2;
    let mut workers_len = state
        .executor_status
        .as_ref()
        .map(|s| s.connected_workers.len())
        .unwrap_or(0) as u16
        + 2;
    let total_height = f.size().height;
    // fixed size section heights
    let top_height = header_len + compilations_len + booklet_len + generations_len;
    // if the sections don't just fit, reduce the size of the workers until they fit but
    // without shortening it more than 3 lines (aka box + 1 worker).
    if top_height + evaluations_len + workers_len > total_height {
        workers_len = std::cmp::max(
            3,
            total_height as i16 - top_height as i16 - evaluations_len as i16,
        ) as u16;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(header_len),
                Constraint::Length(compilations_len),
                Constraint::Length(booklet_len),
                Constraint::Length(generations_len),
                Constraint::Min(0),
                Constraint::Length(workers_len),
            ]
            .as_ref(),
        )
        .split(f.size());
    let paragraph = Paragraph::new(header).block(Block::default().borders(Borders::NONE));
    f.render_widget(paragraph, chunks[0]);
    if compilations_len > 0 {
        render_block(f, chunks[1], " Compilations ");
        draw_compilations(
            f,
            inner_block(chunks[1]),
            state
                .compilations
                .iter()
                .filter(|(p, _)| !state.evaluations.contains_key(*p)),
            loading,
        );
    }
    if !state.booklets.is_empty() {
        render_block(f, chunks[2], " Statements ");
        draw_booklets(f, inner_block(chunks[2]), state, loading);
    }
    if !state.generations.is_empty() {
        render_block(f, chunks[3], " Generation ");
        draw_generations(f, inner_block(chunks[3]), state, loading);
    }
    if !state.evaluations.is_empty() {
        render_block(f, chunks[4], " Evaluations ");
        draw_evaluations(f, inner_block(chunks[4]), state, loading);
    }
    render_server_status(
        f,
        chunks[5],
        state.executor_status.as_ref(),
        loading,
        frame_index,
    );
}

/// Draw the content of the booklet box.
fn draw_booklets(frame: &mut FrameType, rect: Rect, state: &UIState, loading: char) {
    let text: Vec<Spans> = state
        .booklets
        .keys()
        .sorted()
        .flat_map(|name| {
            let booklet = &state.booklets[name];
            let mut text: Vec<Spans> = vec![vec![
                Span::raw(format!("{:<20} ", name)),
                ui_execution_status_text(&booklet.status, loading),
            ]
            .into()];
            for name in booklet.dependencies.keys().sorted() {
                let mut line = Vec::new();
                let dep = &booklet.dependencies[name];
                line.push(Span::raw(format!("  {:<18} ", name)));
                line.push(Span::raw("["));
                for step in dep {
                    line.push(ui_execution_status_text(&step.status, loading));
                }
                line.push(Span::raw("]"));
                text.push(line.into());
            }
            text
        })
        .collect();
    let paragraph = Paragraph::new(text);
    frame.render_widget(paragraph, rect);
}

fn ui_execution_status_text(status: &UIExecutionStatus, loading: char) -> Span {
    match status {
        UIExecutionStatus::Pending => Span::raw("."),
        UIExecutionStatus::Started { .. } => Span::raw(format!("{}", loading)),
        UIExecutionStatus::Skipped => Span::raw("S"),
        UIExecutionStatus::Done { result } => match &result.status {
            ExecutionStatus::Success => Span::styled("S", *GREEN),
            ExecutionStatus::InternalError(_) => Span::raw("I"),
            _ => Span::styled("F", *RED),
        },
    }
}

/// Draw the content of the generation box.
fn draw_generations(frame: &mut FrameType, rect: Rect, state: &UIState, loading: char) {
    let text: Vec<Span> = state
        .generations
        .iter()
        .sorted_by_key(|(k, _)| *k)
        .flat_map(|(_, subtask)| {
            let mut testcases: Vec<Span> = subtask
                .testcases
                .iter()
                .sorted_by_key(|(k, _)| *k)
                .map(|(_, tc)| generation_status_text(&tc.status, loading))
                .collect();
            let mut res = vec![Span::raw("[")];
            res.append(&mut testcases);
            res.push(Span::raw("]"));
            res
        })
        .collect();
    let paragraph = Paragraph::new(Spans(text));
    frame.render_widget(paragraph, rect);
}

/// Get the colored character corresponding to the status of the generation of a testcase.
fn generation_status_text(status: &TestcaseGenerationStatus, loading: char) -> Span {
    match status {
        TestcaseGenerationStatus::Pending => Span::raw("."),
        TestcaseGenerationStatus::Generating => Span::raw(format!("{}", loading)),
        TestcaseGenerationStatus::Generated => Span::styled("G", *GREEN),
        TestcaseGenerationStatus::Validating => Span::raw(format!("{}", loading)),
        TestcaseGenerationStatus::Validated => Span::styled("V", *GREEN),
        TestcaseGenerationStatus::Solving => Span::raw(format!("{}", loading)),
        TestcaseGenerationStatus::Solved => Span::styled("S", *GREEN),
        TestcaseGenerationStatus::Failed => Span::styled("F", *RED),
        TestcaseGenerationStatus::Skipped => Span::styled("s", *YELLOW),
    }
}

/// Draw the content of the evaluation box.
fn draw_evaluations(frame: &mut FrameType, rect: Rect, state: &UIState, loading: char) {
    let max_len = state
        .evaluations
        .keys()
        .map(|k| k.file_name().expect("Invalid file name").len())
        .max()
        .unwrap_or(0)
        + 4;
    let text: Vec<Spans> = state
        .evaluations
        .keys()
        .sorted()
        .map(|solution| {
            let mut spans = vec![Span::raw(format!(
                "{:<max_len$} ",
                solution
                    .file_name()
                    .expect("Invalid file name")
                    .to_string_lossy(),
                max_len = max_len
            ))];
            if let Some(comp_status) = state.compilations.get(solution) {
                spans.push(compilation_status_text(comp_status, loading));
            } else {
                spans.push(Span::raw("    "));
            }
            spans.push(Span::raw(" "));
            spans.push(evaluation_score(state, solution, loading));
            spans.append(&mut evaluation_line(state, solution, loading));
            spans.into()
        })
        .collect();
    let paragraph = Paragraph::new(text);
    frame.render_widget(paragraph, rect);
}

/// Get the colored score of a solution.
fn evaluation_score<'a>(state: &'a UIState, solution: &Path, loading: char) -> Span<'a> {
    let sol_state = if let Some(state) = state.evaluations.get(solution) {
        state
    } else {
        return Span::raw("  ?  ");
    };
    if let Some(score) = sol_state.score {
        match ScoreStatus::from_score(score, state.max_score) {
            ScoreStatus::WrongAnswer => Span::styled(format!(" {:>3.0} ", score), *RED),
            ScoreStatus::Accepted => Span::styled(format!(" {:>3.0} ", score), *GREEN),
            ScoreStatus::PartialScore => Span::styled(format!(" {:>3.0} ", score), *YELLOW),
        }
    } else {
        let has_skipped = sol_state
            .testcases
            .values()
            .any(|tc| tc.status == TestcaseEvaluationStatus::Skipped);
        if has_skipped {
            Span::raw("  X  ")
        } else {
            Span::raw(format!("  {}  ", loading))
        }
    }
}

/// Get the line at the right of the score of a solution.
fn evaluation_line<'a>(state: &'a UIState, solution: &Path, loading: char) -> Vec<Span<'a>> {
    state
        .task
        .subtasks
        .keys()
        .sorted()
        .flat_map(|st| subtask_evaluation_status_text(state, solution, *st, loading))
        .collect()
}

/// Get the status of a subtask, like `[AATTR]` where each letter corresponds to
/// the status of a single testcase.
fn subtask_evaluation_status_text<'a>(
    state: &'a UIState,
    solution: &Path,
    subtask_id: SubtaskId,
    loading: char,
) -> Vec<Span<'a>> {
    let mut texts = vec![];
    let solution = &state.evaluations[solution];
    if !solution.subtasks.contains_key(&subtask_id) {
        return vec![Span::raw("[---]")];
    }
    let subtask = &solution.subtasks[&subtask_id];
    let par_style = if let Some(normalized_score) = subtask.normalized_score {
        match ScoreStatus::from_score(normalized_score, 1.0) {
            ScoreStatus::Accepted => *GREEN,
            ScoreStatus::WrongAnswer => *RED,
            ScoreStatus::PartialScore => *YELLOW,
        }
    } else {
        Style::default()
    };
    texts.push(Span::styled("[", par_style));
    for testcase_id in &state.task.subtasks[&subtask_id].testcases_owned {
        let testcase = &solution.testcases[testcase_id];
        texts.push(testcase_evaluation_status_text(testcase, loading, state));
    }
    texts.push(Span::styled("]", par_style));
    texts
}

/// Get the colored character corresponding to the status of the evaluation of a testcase.
fn testcase_evaluation_status_text<'a>(
    testcase: &'a SolutionTestcaseEvaluationState,
    loading: char,
    state: &'a UIState,
) -> Span<'a> {
    let time_limit = state.task.time_limit;
    let memory_limit = state.task.memory_limit;
    let extra_time = state.config.extra_time;
    let close_color = if testcase.is_close_to_limits(
        time_limit,
        extra_time,
        memory_limit,
        YELLOW_RESOURCE_THRESHOLD,
    ) {
        Some(*ORANGE)
    } else {
        None
    };
    match &testcase.status {
        TestcaseEvaluationStatus::Pending => Span::raw("."),
        TestcaseEvaluationStatus::Solving => Span::raw(format!("{}", loading)),
        TestcaseEvaluationStatus::Solved => Span::raw("s"),
        TestcaseEvaluationStatus::Checking => Span::raw(format!("{}", loading)),
        TestcaseEvaluationStatus::Accepted(_) => Span::styled("A", close_color.unwrap_or(*GREEN)),
        TestcaseEvaluationStatus::WrongAnswer(_) => Span::styled("W", *RED),
        TestcaseEvaluationStatus::Partial(_) => Span::styled("P", *YELLOW),
        TestcaseEvaluationStatus::TimeLimitExceeded => {
            Span::styled("T", close_color.unwrap_or(*RED))
        }
        TestcaseEvaluationStatus::WallTimeLimitExceeded => Span::styled("T", *RED),
        TestcaseEvaluationStatus::MemoryLimitExceeded => {
            Span::styled("M", close_color.unwrap_or(*RED))
        }
        TestcaseEvaluationStatus::RuntimeError => Span::styled("R", *RED),
        TestcaseEvaluationStatus::Failed => Span::styled(
            "F",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ),
        TestcaseEvaluationStatus::Skipped => Span::raw("X"),
    }
}
