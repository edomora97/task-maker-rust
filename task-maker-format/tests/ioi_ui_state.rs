use std::path::PathBuf;

use task_maker_dag::ExecutionStatus;
use task_maker_exec::ExecutorStatus;
use task_maker_format::ioi::{TestcaseEvaluationStatus, TestcaseGenerationStatus, UIState};
use task_maker_format::ui::UIStateT;
use task_maker_format::ui::{CompilationStatus, UIExecutionStatus, UIMessage};

mod utils;

#[test]
fn test_ui_state_server_status() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let status = ExecutorStatus {
        connected_workers: vec![],
        ready_execs: 1,
        waiting_execs: 123,
    };
    assert_eq!(ui.executor_status, None);
    ui.apply(UIMessage::ServerStatus {
        status: status.clone(),
    });
    assert_eq!(ui.executor_status, Some(status));
}

#[test]
fn test_ui_state_compilation_skipped() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    ui.apply(UIMessage::Compilation {
        file: file.clone(),
        status: UIExecutionStatus::Skipped,
    });
    assert_eq!(ui.compilations[&file], CompilationStatus::Skipped);
}

#[test]
fn test_ui_state_compilation_success() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    let result = utils::good_result();
    ui.apply(UIMessage::Compilation {
        file: file.clone(),
        status: UIExecutionStatus::Done {
            result: result.clone(),
        },
    });
    assert_eq!(
        ui.compilations[&file],
        CompilationStatus::Done {
            result,
            stdout: None,
            stderr: None
        }
    );
}

#[test]
fn test_ui_state_compilation_failure() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    let result = utils::bad_result();
    ui.apply(UIMessage::Compilation {
        file: file.clone(),
        status: UIExecutionStatus::Done {
            result: result.clone(),
        },
    });
    assert_eq!(
        ui.compilations[&file],
        CompilationStatus::Failed {
            result,
            stdout: None,
            stderr: None
        }
    );
}

#[test]
fn test_ui_state_compilation_stdout() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    let content = "ciao".to_string();
    let mut result = utils::bad_result();
    result.stdout = Some(content.clone().into_bytes());
    ui.apply(UIMessage::Compilation {
        file: file.clone(),
        status: UIExecutionStatus::Done {
            result: result.clone(),
        },
    });
    assert_eq!(
        ui.compilations[&file],
        CompilationStatus::Failed {
            result,
            stderr: None,
            stdout: Some(content)
        }
    );
}

#[test]
fn test_ui_state_compilation_stderr() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    let content = "ciao".to_string();
    let mut result = utils::good_result();
    result.stderr = Some(content.clone().into_bytes());
    ui.apply(UIMessage::Compilation {
        file: file.clone(),
        status: UIExecutionStatus::Done {
            result: result.clone(),
        },
    });
    assert_eq!(
        ui.compilations[&file],
        CompilationStatus::Done {
            result,
            stderr: Some(content),
            stdout: None
        }
    );
}

#[test]
fn test_ui_state_generation_skipped() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    ui.apply(UIMessage::IOIGeneration {
        subtask: 0,
        testcase: 0,
        status: UIExecutionStatus::Skipped,
    });
    assert_eq!(
        ui.generations[&0].testcases[&0].status,
        TestcaseGenerationStatus::Skipped
    );
}

#[test]
fn test_ui_state_generation_started() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    ui.apply(UIMessage::IOIGeneration {
        subtask: 0,
        testcase: 0,
        status: UIExecutionStatus::Started {
            worker: Default::default(),
        },
    });
    assert_eq!(
        ui.generations[&0].testcases[&0].status,
        TestcaseGenerationStatus::Generating
    );
}

#[test]
fn test_ui_state_generation_generated() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    ui.apply(UIMessage::IOIGeneration {
        subtask: 0,
        testcase: 0,
        status: UIExecutionStatus::Done {
            result: utils::good_result(),
        },
    });
    assert_eq!(
        ui.generations[&0].testcases[&0].status,
        TestcaseGenerationStatus::Generated
    );
}

#[test]
fn test_ui_state_generation_failed() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    ui.apply(UIMessage::IOIGeneration {
        subtask: 0,
        testcase: 0,
        status: UIExecutionStatus::Done {
            result: utils::bad_result(),
        },
    });
    assert_eq!(
        ui.generations[&0].testcases[&0].status,
        TestcaseGenerationStatus::Failed
    );
}

#[test]
fn test_ui_state_validation_skipped() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    ui.apply(UIMessage::IOIValidation {
        subtask: 0,
        testcase: 0,
        status: UIExecutionStatus::Skipped,
    });
    assert_eq!(
        ui.generations[&0].testcases[&0].status,
        TestcaseGenerationStatus::Skipped
    );
}

#[test]
fn test_ui_state_validation_started() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    ui.apply(UIMessage::IOIValidation {
        subtask: 0,
        testcase: 0,
        status: UIExecutionStatus::Started {
            worker: Default::default(),
        },
    });
    assert_eq!(
        ui.generations[&0].testcases[&0].status,
        TestcaseGenerationStatus::Validating
    );
}

#[test]
fn test_ui_state_validation_validated() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    ui.apply(UIMessage::IOIValidation {
        subtask: 0,
        testcase: 0,
        status: UIExecutionStatus::Done {
            result: utils::good_result(),
        },
    });
    assert_eq!(
        ui.generations[&0].testcases[&0].status,
        TestcaseGenerationStatus::Validated
    );
}

#[test]
fn test_ui_state_validation_failed() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    ui.apply(UIMessage::IOIValidation {
        subtask: 0,
        testcase: 0,
        status: UIExecutionStatus::Done {
            result: utils::bad_result(),
        },
    });
    assert_eq!(
        ui.generations[&0].testcases[&0].status,
        TestcaseGenerationStatus::Failed
    );
}

#[test]
fn test_ui_state_solution_skipped() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    ui.apply(UIMessage::IOISolution {
        subtask: 0,
        testcase: 0,
        status: UIExecutionStatus::Skipped,
    });
    assert_eq!(
        ui.generations[&0].testcases[&0].status,
        TestcaseGenerationStatus::Skipped
    );
}

#[test]
fn test_ui_state_solution_started() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    ui.apply(UIMessage::IOISolution {
        subtask: 0,
        testcase: 0,
        status: UIExecutionStatus::Started {
            worker: Default::default(),
        },
    });
    assert_eq!(
        ui.generations[&0].testcases[&0].status,
        TestcaseGenerationStatus::Solving
    );
}

#[test]
fn test_ui_state_solution_validated() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    ui.apply(UIMessage::IOISolution {
        subtask: 0,
        testcase: 0,
        status: UIExecutionStatus::Done {
            result: utils::good_result(),
        },
    });
    assert_eq!(
        ui.generations[&0].testcases[&0].status,
        TestcaseGenerationStatus::Solved
    );
}

#[test]
fn test_ui_state_solution_failed() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    ui.apply(UIMessage::IOISolution {
        subtask: 0,
        testcase: 0,
        status: UIExecutionStatus::Done {
            result: utils::bad_result(),
        },
    });
    assert_eq!(
        ui.generations[&0].testcases[&0].status,
        TestcaseGenerationStatus::Failed
    );
}

#[test]
fn test_ui_state_evaluation_skipped() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    ui.apply(UIMessage::IOIEvaluation {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        status: UIExecutionStatus::Skipped,
        part: 0,
        num_parts: 1,
    });
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::Skipped
    );
}

#[test]
fn test_ui_state_evaluation_started() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    ui.apply(UIMessage::IOIEvaluation {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        status: UIExecutionStatus::Started {
            worker: Default::default(),
        },
        part: 0,
        num_parts: 1,
    });
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::Solving
    );
}

#[test]
fn test_ui_state_evaluation_solved() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    ui.apply(UIMessage::IOIEvaluation {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        status: UIExecutionStatus::Done {
            result: utils::good_result(),
        },
        part: 0,
        num_parts: 1,
    });
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::Solved
    );
}

#[test]
fn test_ui_state_evaluation_return_code() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    let mut result = utils::bad_result();
    result.status = ExecutionStatus::ReturnCode(123);
    ui.apply(UIMessage::IOIEvaluation {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        status: UIExecutionStatus::Done { result },
        part: 0,
        num_parts: 1,
    });
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::RuntimeError
    );
}

#[test]
fn test_ui_state_evaluation_signal() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    let mut result = utils::bad_result();
    result.status = ExecutionStatus::Signal(1, "BOH".to_string());
    ui.apply(UIMessage::IOIEvaluation {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        status: UIExecutionStatus::Done { result },
        part: 0,
        num_parts: 1,
    });
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::RuntimeError
    );
}

#[test]
fn test_ui_state_evaluation_time_limit() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    let mut result = utils::bad_result();
    result.status = ExecutionStatus::TimeLimitExceeded;
    ui.apply(UIMessage::IOIEvaluation {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        status: UIExecutionStatus::Done { result },
        part: 0,
        num_parts: 1,
    });
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::TimeLimitExceeded
    );
}

#[test]
fn test_ui_state_evaluation_sys_limit() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    let mut result = utils::bad_result();
    result.status = ExecutionStatus::SysTimeLimitExceeded;
    ui.apply(UIMessage::IOIEvaluation {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        status: UIExecutionStatus::Done { result },
        part: 0,
        num_parts: 1,
    });
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::TimeLimitExceeded
    );
}

#[test]
fn test_ui_state_evaluation_wall_limit() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    let mut result = utils::bad_result();
    result.status = ExecutionStatus::WallTimeLimitExceeded;
    ui.apply(UIMessage::IOIEvaluation {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        status: UIExecutionStatus::Done { result },
        part: 0,
        num_parts: 1,
    });
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::WallTimeLimitExceeded
    );
}

#[test]
fn test_ui_state_evaluation_memory_limit() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    let mut result = utils::bad_result();
    result.status = ExecutionStatus::MemoryLimitExceeded;
    ui.apply(UIMessage::IOIEvaluation {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        status: UIExecutionStatus::Done { result },
        part: 0,
        num_parts: 1,
    });
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::MemoryLimitExceeded
    );
}

#[test]
fn test_ui_state_evaluation_internal_error() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    let mut result = utils::bad_result();
    result.status = ExecutionStatus::InternalError("foo".into());
    ui.apply(UIMessage::IOIEvaluation {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        status: UIExecutionStatus::Done { result },
        part: 0,
        num_parts: 1,
    });
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::Failed
    );
}

#[test]
fn test_ui_state_checker_skipped() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    ui.apply(UIMessage::IOIEvaluation {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        status: UIExecutionStatus::Started {
            worker: Default::default(),
        },
        part: 0,
        num_parts: 1,
    });
    ui.apply(UIMessage::IOIChecker {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        status: UIExecutionStatus::Skipped,
    });
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::Solving // !! not skipped !!
    );
}

#[test]
fn test_ui_state_checker_started() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    ui.apply(UIMessage::IOIChecker {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        status: UIExecutionStatus::Started {
            worker: Default::default(),
        },
    });
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::Checking
    );
}

#[test]
fn test_ui_state_checker_done() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    let result = utils::good_result();
    ui.apply(UIMessage::IOIChecker {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        status: UIExecutionStatus::Done {
            result: result.clone(),
        },
    });
    assert_eq!(ui.evaluations[&file].testcases[&0].checker, Some(result));
}

#[test]
fn test_ui_state_testcase_score_wrong_answer() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    ui.apply(UIMessage::IOITestcaseScore {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        score: 0.0,
        message: "nope".to_string(),
    });
    assert_eq!(ui.evaluations[&file].testcases[&0].score, Some(0.0));
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::WrongAnswer("nope".into())
    );
}

#[test]
fn test_ui_state_testcase_score_partial() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    ui.apply(UIMessage::IOITestcaseScore {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        score: 0.5,
        message: "almost".to_string(),
    });
    assert_eq!(ui.evaluations[&file].testcases[&0].score, Some(0.5));
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::Partial("almost".into())
    );
}

#[test]
fn test_ui_state_testcase_score_accepted() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    ui.apply(UIMessage::IOITestcaseScore {
        subtask: 0,
        testcase: 0,
        solution: file.clone(),
        score: 1.0,
        message: "yep".to_string(),
    });
    assert_eq!(ui.evaluations[&file].testcases[&0].score, Some(1.0));
    assert_eq!(
        ui.evaluations[&file].testcases[&0].status,
        TestcaseEvaluationStatus::Accepted("yep".into())
    );
}

#[test]
fn test_ui_state_subtask_score() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    ui.apply(UIMessage::IOISubtaskScore {
        subtask: 0,
        solution: file.clone(),
        score: 10.0,
        normalized_score: 1.0,
    });
    assert_eq!(ui.evaluations[&file].subtasks[&0].score, Some(10.0));
}

#[test]
fn test_ui_state_task_score() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = PathBuf::from("file");
    ui.apply(UIMessage::IOITaskScore {
        solution: file.clone(),
        score: 10.0,
    });
    assert_eq!(ui.evaluations[&file].score, Some(10.0));
}

#[test]
fn test_ui_state_booklet() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let file = "file".to_string();
    ui.apply(UIMessage::IOIBooklet {
        name: file.clone(),
        status: UIExecutionStatus::Skipped,
    });
    assert_eq!(ui.booklets[&file].status, UIExecutionStatus::Skipped);
    assert_eq!(ui.booklets[&file].dependencies.len(), 0);
}

#[test]
fn test_ui_state_booklet_dep() {
    let task = utils::new_task();
    let mut ui = UIState::new(&task, Default::default());
    let booklet = "booklet".to_string();
    let file = "file".to_string();
    ui.apply(UIMessage::IOIBookletDependency {
        booklet: booklet.clone(),
        name: file.clone(),
        step: 0,
        num_steps: 2,
        status: UIExecutionStatus::Skipped,
    });
    assert_eq!(ui.booklets[&booklet].dependencies[&file].len(), 2);
    assert_eq!(
        ui.booklets[&booklet].dependencies[&file][0].status,
        UIExecutionStatus::Skipped
    );
    assert_eq!(
        ui.booklets[&booklet].dependencies[&file][1].status,
        UIExecutionStatus::Pending
    );
}
