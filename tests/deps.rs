mod common;
use common::TestInterface;

fn classic(test: TestInterface) {
    test.success()
        .time_limit(1.0)
        .memory_limit(64)
        .max_score(100.0)
        .subtask_scores(vec![30.0, 30.0, 40.0])
        .must_compile("sol-.cpp")
        .must_compile("sol-0.cpp")
        .must_compile("sol-1.cpp")
        .must_compile("sol-2.cpp")
        .must_compile("sol-01.cpp")
        .must_compile("sol-02.cpp")
        .must_compile("sol-12.cpp")
        .must_compile("sol-012.cpp")
        .solution_score("sol-.cpp", vec![0.0, 0.0, 0.0])
        .solution_score("sol-0.cpp", vec![0.0, 0.0, 0.0])
        .solution_score("sol-1.cpp", vec![0.0, 30.0, 0.0])
        .solution_score("sol-2.cpp", vec![0.0, 0.0, 0.0])
        .solution_score("sol-01.cpp", vec![30.0, 30.0, 0.0])
        .solution_score("sol-02.cpp", vec![0.0, 0.0, 0.0])
        .solution_score("sol-12.cpp", vec![0.0, 30.0, 0.0])
        .solution_score("sol-012.cpp", vec![30.0, 30.0, 40.0]);
}

#[test]
fn deps_local() {
    better_panic::install();
    classic(TestInterface::run_local("deps"));
}

#[test]
fn deps_remote() {
    better_panic::install();
    classic(TestInterface::run_remote("deps"));
}
