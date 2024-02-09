#[macro_use]
extern crate approx;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use task_maker_format::ioi::*;
use task_maker_format::ui::{UIMessage, UIMessageSender};

mod utils;

#[test]
#[allow(clippy::cognitive_complexity)]
fn test_score_manager() {
    let task = utils::new_task();
    let (sender, receiver) = UIMessageSender::new();
    let sender = Arc::new(Mutex::new(sender));
    let mut manager = ScoreManager::new(&task, sender.clone(), "sol".into()).unwrap();

    assert!(receiver.try_recv().is_err());

    manager
        .score(0, 0, 1.0, "foo".into(), sender.clone())
        .unwrap();
    if let Ok(mex) = receiver.try_recv() {
        match mex {
            UIMessage::IOITestcaseScore {
                subtask,
                testcase,
                solution,
                score,
                message,
            } => {
                assert_eq!(subtask, 0);
                assert_eq!(testcase, 0);
                assert_eq!(solution, PathBuf::from("sol"));
                assert_abs_diff_eq!(score, 1.0);
                assert_eq!(message, "foo");
            }
            _ => panic!("Expecting UIMessage::IOITestcaseScore but was {:?}", mex),
        }
    } else {
        panic!("Expecting UIMessage::IOITestcaseScore but was nothing");
    }
    if let Ok(mex) = receiver.try_recv() {
        match mex {
            UIMessage::IOISubtaskScore {
                subtask,
                solution,
                score,
                normalized_score,
            } => {
                assert_eq!(subtask, 0);
                assert_eq!(solution, PathBuf::from("sol"));
                assert_abs_diff_eq!(score, 10.0);
                assert_abs_diff_eq!(normalized_score, 1.0);
            }
            _ => panic!("Expecting UIMessage::IOISubtaskScore but was {:?}", mex),
        }
    } else {
        panic!("Expecting UIMessage::IOISubtaskScore but was nothing");
    }
    assert!(receiver.try_recv().is_err());

    manager
        .score(1, 1, 1.0, "foo".into(), sender.clone())
        .unwrap();
    if let Ok(mex) = receiver.try_recv() {
        match mex {
            UIMessage::IOITestcaseScore {
                subtask,
                testcase,
                solution,
                score,
                message,
            } => {
                assert_eq!(subtask, 1);
                assert_eq!(testcase, 1);
                assert_eq!(solution, PathBuf::from("sol"));
                assert_abs_diff_eq!(score, 1.0);
                assert_eq!(message, "foo");
            }
            _ => panic!("Expecting UIMessage::IOITestcaseScore but was {:?}", mex),
        }
    } else {
        panic!("Expecting UIMessage::IOITestcaseScore but was nothing");
    }
    if let Ok(mex) = receiver.try_recv() {
        panic!("Expecting nothing but was {:?}", mex);
    }

    manager.score(1, 2, 0.0, "foo".into(), sender).unwrap();
    if let Ok(mex) = receiver.try_recv() {
        match mex {
            UIMessage::IOITestcaseScore {
                subtask,
                testcase,
                solution,
                score,
                message,
            } => {
                assert_eq!(subtask, 1);
                assert_eq!(testcase, 2);
                assert_eq!(solution, PathBuf::from("sol"));
                assert_abs_diff_eq!(score, 0.0);
                assert_eq!(message, "foo");
            }
            _ => panic!("Expecting UIMessage::IOITestcaseScore but was {:?}", mex),
        }
    } else {
        panic!("Expecting UIMessage::IOITestcaseScore but was nothing");
    }
    if let Ok(mex) = receiver.try_recv() {
        match mex {
            UIMessage::IOISubtaskScore {
                subtask,
                solution,
                score,
                normalized_score,
            } => {
                assert_eq!(subtask, 1);
                assert_eq!(solution, PathBuf::from("sol"));
                assert_abs_diff_eq!(score, 0.0);
                assert_abs_diff_eq!(normalized_score, 0.0);
            }
            _ => panic!("Expecting UIMessage::IOISubtaskScore but was {:?}", mex),
        }
    } else {
        panic!("Expecting UIMessage::IOISubtaskScore but was nothing");
    }
    if let Ok(mex) = receiver.try_recv() {
        match mex {
            UIMessage::IOITaskScore { solution, score } => {
                assert_eq!(solution, PathBuf::from("sol"));
                assert_abs_diff_eq!(score, 10.0);
            }
            _ => panic!("Expecting UIMessage::IOITaskScore but was {:?}", mex),
        }
    } else {
        panic!("Expecting UIMessage::IOITaskScore but was nothing");
    }
    assert!(receiver.try_recv().is_err());
}

#[test]
fn test_score_manager_empty_subtask() {
    let mut task = utils::new_task();
    assert_eq!(task.subtasks.len(), 2);
    assert_eq!(task.subtasks.get(&0).unwrap().testcases.len(), 1);

    // Make the second subtask empty.
    task.subtasks.get_mut(&1).unwrap().testcases.clear();
    task.subtasks.get_mut(&1).unwrap().testcases_owned.clear();

    let (sender, receiver) = UIMessageSender::new();
    let sender = Arc::new(Mutex::new(sender));
    let mut manager = ScoreManager::new(&task, sender.clone(), "sol".into()).unwrap();

    if let Ok(mex) = receiver.try_recv() {
        match mex {
            UIMessage::IOISubtaskScore {
                subtask,
                solution,
                score,
                normalized_score,
            } => {
                assert_eq!(subtask, 1);
                assert_eq!(solution, PathBuf::from("sol"));
                assert_abs_diff_eq!(score, 90.0);
                assert_abs_diff_eq!(normalized_score, 1.0);
            }
            _ => panic!("Expecting UIMessage::IOISubtaskScore but was {:?}", mex),
        }
    } else {
        panic!("Expecting UIMessage::IOISubtaskScore but was nothing");
    }
    assert!(receiver.try_recv().is_err());

    manager.score(0, 0, 1.0, "foo".into(), sender).unwrap();
    if let Ok(mex) = receiver.try_recv() {
        match mex {
            UIMessage::IOITestcaseScore {
                subtask,
                testcase,
                solution,
                score,
                message,
            } => {
                assert_eq!(subtask, 0);
                assert_eq!(testcase, 0);
                assert_eq!(solution, PathBuf::from("sol"));
                assert_abs_diff_eq!(score, 1.0);
                assert_eq!(message, "foo");
            }
            _ => panic!("Expecting UIMessage::IOITestcaseScore but was {:?}", mex),
        }
    } else {
        panic!("Expecting UIMessage::IOITestcaseScore but was nothing");
    }
    if let Ok(mex) = receiver.try_recv() {
        match mex {
            UIMessage::IOISubtaskScore {
                subtask,
                solution,
                score,
                normalized_score,
            } => {
                assert_eq!(subtask, 0);
                assert_eq!(solution, PathBuf::from("sol"));
                assert_abs_diff_eq!(score, 10.0);
                assert_abs_diff_eq!(normalized_score, 1.0);
            }
            _ => panic!("Expecting UIMessage::IOISubtaskScore but was {:?}", mex),
        }
    } else {
        panic!("Expecting UIMessage::IOISubtaskScore but was nothing");
    }
    if let Ok(mex) = receiver.try_recv() {
        match mex {
            UIMessage::IOITaskScore { solution, score } => {
                assert_eq!(solution, PathBuf::from("sol"));
                assert_abs_diff_eq!(score, 100.0);
            }
            _ => panic!("Expecting UIMessage::IOITaskScore but was {:?}", mex),
        }
    } else {
        panic!("Expecting UIMessage::IOITaskScore but was nothing");
    }
    assert!(receiver.try_recv().is_err());
}
