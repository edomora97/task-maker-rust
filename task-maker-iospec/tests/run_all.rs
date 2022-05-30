use std::env;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use assert_cmd::Command;
use goldenfile::Mint;
use task_maker_iospec::tools::iospec_gen::LangOpt;
use task_maker_iospec::tools::iospec_gen::TargetOpt;
use task_maker_iospec::tools::*;
use tempdir::TempDir;
use walkdir::WalkDir;

const TEST_PREFIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/specs");
const GOLDEN_PREFIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/goldenfiles");

#[test]
fn check_all_specs() -> Result<(), anyhow::Error> {
    for e in WalkDir::new(TEST_PREFIX)
        .into_iter()
        .map(|e| e.unwrap())
        .filter(|e| !e.file_type().is_dir())
        .filter(|e| {
            e.path()
                .parent()
                .unwrap()
                .extension()
                .map_or(true, |e| e != "skip")
        })
        .filter(|e| {
            e.file_name() == "IOSPEC" || e.path().extension().map_or(false, |e| e == "iospec")
        })
    {
        let spec_path = &e.path();
        let dir_path = spec_path.parent().unwrap();
        let spec_name = spec_path.file_name().unwrap().to_str().unwrap();

        let mint = &mut create_mint(dir_path);

        let _temp_dir = within_temp_dir();

        test_spec(dir_path, spec_name, mint);

        if spec_name == "IOSPEC" {
            test_valid_spec(dir_path, mint);
        }
    }

    Ok(())
}

fn create_mint(dir_path: &Path) -> Mint {
    let mint_path = &PathBuf::from_iter(vec![
        PathBuf::from(GOLDEN_PREFIX).as_path(),
        dir_path.strip_prefix(TEST_PREFIX).unwrap(),
    ]);
    // Only useful when minting for the first time
    fs::create_dir_all(mint_path).ok();
    Mint::new(mint_path)
}

fn test_spec(dir_path: &Path, name: &str, mint: &mut Mint) {
    copy_file(dir_path, name, mint);

    let _ = iospec_check::do_main(
        iospec_check::Opt {
            spec: SpecOpt {
                spec: name.into(),
                cfg: vec![],
                color: ColorOpt::Never,
            },
            input: None,
            output: None,
        },
        &mut File::create(format!("{}.check.stderr", name)).unwrap(),
    );
    mint_file(mint, format!("{}.check.stderr", name));
}

fn copy_file(dir_path: &Path, name: &str, mint: &mut Mint) {
    let path = &PathBuf::from_iter(vec![dir_path, &PathBuf::from(name)]);
    let data = fs::read(path).unwrap();
    fs::write(name, &data).unwrap();
    mint_file(mint, name);
}

fn test_valid_spec(dir_path: &Path, mint: &mut Mint) {
    let all_langs = &vec![
        (LangOpt::Cpp, TargetOpt::Grader),
        (LangOpt::Cpp, TargetOpt::Template),
        (LangOpt::Cpp, TargetOpt::Support),
        (LangOpt::C, TargetOpt::Grader),
        (LangOpt::C, TargetOpt::Template),
        (LangOpt::Inspect, TargetOpt::Grader),
    ];

    for (lang_opt, target_opt) in all_langs {
        generate(lang_opt, target_opt, mint);
        if matches!(target_opt, TargetOpt::Grader) {
            compile_generated(dir_path, lang_opt, mint);
        }
    }

    for e in fs::read_dir(dir_path)
        .unwrap()
        .into_iter()
        .map(|e| e.unwrap())
        .filter(|e| {
            e.path()
                .extension()
                .map_or(false, |e| e.to_str() == Some("input"))
        })
    {
        let input_path = &e.path();
        let stem = &PathBuf::from(input_path.file_stem().unwrap());

        copy_input(input_path, stem);
        check_input_and_mint_stderr(stem, mint);

        for ref lang_opt in vec![LangOpt::Cpp, LangOpt::C] {
            run_generated_and_mint_output(lang_opt, stem, mint);
            check_output_and_mint_stderr(lang_opt, stem, mint);
        }
    }
}

fn generate(lang_opt: &LangOpt, target_opt: &TargetOpt, mint: &mut Mint) {
    let extension = lang_extension(lang_opt);

    let dest = &match target_opt {
        TargetOpt::Grader => format!("grader.{}", extension),
        TargetOpt::Template => format!("template.{}", extension),
        TargetOpt::Support => format!("support.{}", extension),
    };
    let stderr = &format!("{}.gen.stderr", dest);

    File::create(dest).unwrap();

    let _ = iospec_gen::do_main(
        iospec_gen::Opt {
            spec: SpecOpt {
                spec: "IOSPEC".into(),
                cfg: vec![],
                color: ColorOpt::Never,
            },
            target: *target_opt,
            dest: Some(dest.into()),
            lang: lang_opt.clone(),
        },
        &mut File::create(stderr).unwrap(),
    );

    mint_file(mint, stderr);
    mint_file(mint, dest);
}

fn compile_generated(dir_path: &Path, lang_opt: &LangOpt, mint: &mut Mint) {
    match lang_opt {
        LangOpt::Cpp => {
            copy_file(dir_path, "solution.cpp", mint);
            Command::new("g++")
                .arg("grader.cpp")
                .arg("solution.cpp")
                .arg("-o")
                .arg("main.cpp.bin")
                .arg("-fsanitize=address")
                .assert()
                .success();
        }
        LangOpt::C => {
            copy_file(dir_path, "solution.c", mint);
            Command::new("gcc")
                .arg("grader.c")
                .arg("solution.c")
                .arg("-o")
                .arg("main.c.bin")
                // FIXME: missing `free` in generated C
                // .arg("-fsanitize=address")
                .assert()
                .success();
        }
        _ => (),
    };
}

fn copy_input(input_path: &std::path::Path, stem: &std::path::Path) {
    let input_data = fs::read(input_path).unwrap();
    fs::write(stem.with_extension("input"), input_data).unwrap();
}

fn check_input_and_mint_stderr(stem: &PathBuf, mint: &mut Mint) {
    let stderr_path = &stem.with_extension("input.check.stderr");
    let _ = iospec_check::do_main(
        iospec_check::Opt {
            spec: SpecOpt {
                spec: "IOSPEC".into(),
                cfg: vec![],
                color: ColorOpt::Never,
            },
            input: Some(stem.with_extension("input")),
            output: None,
        },
        &mut File::create(stderr_path).unwrap(),
    );
    mint_file(mint, stderr_path);
}

fn run_generated_and_mint_output(lang_opt: &LangOpt, stem: &std::path::Path, mint: &mut Mint) {
    match lang_opt {
        LangOpt::Cpp => {
            let output_path = &stem.with_extension("cpp.output");
            fs::write(
                output_path,
                &Command::new("./main.cpp.bin")
                    .write_stdin(fs::read(stem.with_extension("input")).unwrap())
                    .assert()
                    .success()
                    .get_output()
                    .stdout,
            )
            .unwrap();
            mint_file(mint, output_path)
        }
        LangOpt::C => {
            let output_path = &stem.with_extension("c.output");
            fs::write(
                output_path,
                &Command::new("./main.c.bin")
                    .write_stdin(fs::read(stem.with_extension("input")).unwrap())
                    .assert()
                    .success()
                    .get_output()
                    .stdout,
            )
            .unwrap();
            mint_file(mint, output_path)
        }
        _ => (),
    }
}

fn check_output_and_mint_stderr(lang_opt: &LangOpt, stem: &std::path::Path, mint: &mut Mint) {
    let extension = lang_extension(lang_opt);
    let stderr_path = &stem.with_extension(format!("{}.output.check.stderr", extension));
    let _ = iospec_check::do_main(
        iospec_check::Opt {
            spec: SpecOpt {
                spec: "IOSPEC".into(),
                cfg: vec![],
                color: ColorOpt::Never,
            },
            input: Some(stem.with_extension("input")),
            output: Some(stem.with_extension(format!("{}.output", extension))),
        },
        &mut File::create(stderr_path).unwrap(),
    );
    mint_file(mint, stderr_path);
}

fn lang_extension(lang_opt: &LangOpt) -> &'static str {
    let extension = match lang_opt {
        LangOpt::Cpp => "cpp",
        LangOpt::C => "c",
        LangOpt::Inspect => "inspect",
        // LangOpt::Tex => "tex",
    };
    extension
}

fn mint_file<P: AsRef<Path>>(mint: &mut Mint, path: P) {
    mint.new_goldenfile(&path)
        .unwrap()
        .write(&fs::read(path).unwrap())
        .unwrap();
}

fn within_temp_dir() -> TempDir {
    // Use tmpfs if available
    let dir = option_env!("XDG_RUNTIME_DIR")
        .map_or_else(
            || TempDir::new("task-maker-iospec-test"),
            |path| TempDir::new_in(path, "task-maker-iospec-test"),
        )
        .unwrap();
    env::set_current_dir(dir.path()).unwrap();
    dir
}
