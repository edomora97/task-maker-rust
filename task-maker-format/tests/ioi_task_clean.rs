use std::path::PathBuf;
use std::sync::Arc;
use task_maker_format::ioi::{Checker, InputGenerator, TaskType};
use task_maker_format::SourceFile;

mod utils;

#[test]
fn test_ioi_task_clean() {
    let tmpdir = tempfile::TempDir::new().unwrap();
    let task = utils::new_task_with_context(tmpdir.path());
    let input = tmpdir.path().join("input");
    let output = tmpdir.path().join("output");
    std::fs::create_dir(&input).unwrap();
    std::fs::create_dir(&output).unwrap();
    for i in 0..3 {
        std::fs::write(input.join(format!("input{}.txt", i)), "x").unwrap();
        std::fs::write(output.join(format!("output{}.txt", i)), "x").unwrap();
    }
    task.clean().unwrap();
    assert!(!input.exists());
    assert!(!output.exists());
}

#[test]
fn test_ioi_task_clean_skip_static() {
    let tmpdir = tempfile::TempDir::new().unwrap();
    let mut task = utils::new_task_with_context(tmpdir.path());
    let input = tmpdir.path().join("input");
    let output = tmpdir.path().join("output");
    std::fs::create_dir(&input).unwrap();
    std::fs::create_dir(&output).unwrap();
    for i in 0..3 {
        std::fs::write(input.join(format!("input{}.txt", i)), "x").unwrap();
        std::fs::write(output.join(format!("output{}.txt", i)), "x").unwrap();
    }
    task.testcases.get_mut(&0).unwrap().input_generator =
        InputGenerator::StaticFile(input.join("input0.txt"));

    task.clean().unwrap();
    assert!(input.exists());
    assert!(input.join("input0.txt").exists());
    assert!(!input.join("input1.txt").exists());
    assert!(!input.join("input2.txt").exists());
    assert!(!output.exists());
}

#[test]
fn test_ioi_task_clean_bin() {
    let tmpdir = tempfile::TempDir::new().unwrap();
    let task = utils::new_task_with_context(tmpdir.path());
    let bin = tmpdir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::write(bin.join("foo"), "x").unwrap();

    task.clean().unwrap();

    assert!(!bin.exists());
}

#[test]
fn test_ioi_task_clean_checker() {
    let tmpdir = tempfile::TempDir::new().unwrap();
    let mut task = utils::new_task_with_context(tmpdir.path());
    let check = tmpdir.path().join("check");
    let cor = tmpdir.path().join("cor");
    std::fs::create_dir(&check).unwrap();
    std::fs::create_dir(&cor).unwrap();
    std::fs::write(check.join("checker"), "x").unwrap();
    std::fs::write(cor.join("correttore"), "x").unwrap();
    std::fs::write(tmpdir.path().join("check.py"), "x").unwrap();
    let source = SourceFile::new(
        tmpdir.path().join("check.py"),
        "",
        "",
        None,
        None::<PathBuf>,
    )
    .unwrap();
    if let TaskType::Batch(data) = &mut task.task_type {
        data.checker = Checker::Custom(Arc::new(source));
    }
    task.clean().unwrap();

    assert!(!check.join("checker").exists());
    assert!(!cor.join("correttore").exists());
}
