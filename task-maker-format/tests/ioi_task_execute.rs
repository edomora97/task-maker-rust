use std::path::PathBuf;
use std::sync::Arc;
use task_maker_format::ioi::{
    Booklet, BookletConfig, InputGenerator, InputValidator, OutputGenerator, Statement,
    StatementConfig,
};
use task_maker_format::{EvaluationConfig, EvaluationData, SourceFile};

mod utils;

#[test]
fn test_ioi_task_execute_copy() {
    let tmpdir = tempfile::TempDir::new().unwrap();
    let mut task = utils::new_task_with_context(tmpdir.path());
    let (mut eval, _receiver) = EvaluationData::new(tmpdir.path());
    task.build_dag(&mut eval, &EvaluationConfig::default())
        .unwrap();
    assert_eq!(eval.dag.data.provided_files.len(), 6);
}

#[test]
fn test_ioi_task_execute_gen() {
    let tmpdir = tempfile::TempDir::new().unwrap();
    let mut task = utils::new_task_with_context(tmpdir.path());

    std::fs::write(tmpdir.path().join("gen.py"), "x").unwrap();
    let source =
        SourceFile::new(tmpdir.path().join("gen.py"), "", "", None, None::<PathBuf>).unwrap();
    let gen = InputGenerator::Custom(Arc::new(source), vec![]);
    task.testcases.get_mut(&0).unwrap().input_generator = gen;

    let (mut eval, _receiver) = EvaluationData::new(tmpdir.path());
    task.build_dag(&mut eval, &EvaluationConfig::default())
        .unwrap();
    assert_eq!(eval.dag.data.provided_files.len(), 5 + 1); // io + gen.py
    assert_eq!(eval.dag.data.execution_groups.len(), 1);
}

#[test]
fn test_ioi_task_execute_val() {
    let tmpdir = tempfile::TempDir::new().unwrap();
    let mut task = utils::new_task_with_context(tmpdir.path());

    std::fs::write(tmpdir.path().join("val.py"), "x").unwrap();
    let source =
        SourceFile::new(tmpdir.path().join("val.py"), "", "", None, None::<PathBuf>).unwrap();
    let val = InputValidator::Custom(Arc::new(source), vec![]);
    task.testcases.get_mut(&0).unwrap().input_validator = val;

    let (mut eval, _receiver) = EvaluationData::new(tmpdir.path());
    task.build_dag(&mut eval, &EvaluationConfig::default())
        .unwrap();
    assert_eq!(eval.dag.data.provided_files.len(), 6 + 1); // io + val.py
    assert_eq!(eval.dag.data.execution_groups.len(), 1);
}

#[test]
fn test_ioi_task_execute_sol() {
    let tmpdir = tempfile::TempDir::new().unwrap();
    let mut task = utils::new_task_with_context(tmpdir.path());

    std::fs::write(tmpdir.path().join("sol.py"), "x").unwrap();
    let source =
        SourceFile::new(tmpdir.path().join("sol.py"), "", "", None, None::<PathBuf>).unwrap();
    let sol = OutputGenerator::Custom(Arc::new(source), vec![]);
    task.testcases.get_mut(&0).unwrap().output_generator = sol;

    let (mut eval, _receiver) = EvaluationData::new(tmpdir.path());
    task.build_dag(&mut eval, &EvaluationConfig::default())
        .unwrap();
    assert_eq!(eval.dag.data.provided_files.len(), 5 + 1); // io + sol.py
    assert_eq!(eval.dag.data.execution_groups.len(), 1);
}

#[test]
fn test_ioi_task_execute_eval() {
    let tmpdir = tempfile::TempDir::new().unwrap();
    let mut task = utils::new_task_with_context(tmpdir.path());

    std::fs::create_dir(tmpdir.path().join("sol")).unwrap();
    std::fs::write(tmpdir.path().join("sol").join("sol.py"), "foo").unwrap();

    let (mut eval, _receiver) = EvaluationData::new(tmpdir.path());
    task.build_dag(&mut eval, &EvaluationConfig::default())
        .unwrap();
    assert_eq!(eval.dag.data.provided_files.len(), 6 + 1); // io + sol/sol.py
    assert_eq!(eval.dag.data.execution_groups.len(), 3 + 3); // eval + checker
}

#[test]
fn test_ioi_task_execute_booklet() {
    let tmpdir = tempfile::TempDir::new().unwrap();
    let mut task = utils::new_task_with_context(tmpdir.path());

    let config = BookletConfig::default();
    let mut booklet = Booklet::new(config, tmpdir.path().join("booklet.pdf"));

    std::fs::write(tmpdir.path().join("statement.tex"), "foo").unwrap();
    let config = StatementConfig::default();
    let statement = Statement::new(tmpdir.path().join("statement.tex"), config).unwrap();

    booklet.add_statement(statement);
    task.booklets.push(booklet);

    let (mut eval, _receiver) = EvaluationData::new(tmpdir.path());
    task.build_dag(&mut eval, &EvaluationConfig::default())
        .unwrap();
    assert_eq!(eval.dag.data.execution_groups.len(), 1); // latexmk
}
