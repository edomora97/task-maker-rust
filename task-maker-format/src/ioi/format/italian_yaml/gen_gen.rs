use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, bail, ensure, Context, Error};
use pest::Parser;

use task_maker_diagnostics::CodeSpan;

use crate::ioi::format::italian_yaml::TaskInputEntry;
use crate::ioi::italian_yaml::cleanup_subtask_name;
use crate::ioi::{
    InputGenerator, InputValidator, OutputGenerator, SubtaskId, SubtaskInfo, TestcaseId,
    TestcaseInfo,
};
use crate::{find_source_file, WriteBinTo};

/// This module exists because of a `pest`'s bug: <https://github.com/pest-parser/pest/issues/326>
#[allow(missing_docs)]
mod parser {
    /// The gen/GEN file parser.
    #[derive(Parser)]
    #[grammar = "ioi/format/italian_yaml/GEN.pest"]
    pub struct GENParser;
}

/// Parse the `gen/GEN` file extracting the subtasks and the testcases.
pub(crate) fn parse_gen_gen<P: AsRef<Path>, V, O>(
    path: P,
    get_validator: V,
    get_output_gen: O,
) -> Result<Vec<TaskInputEntry>, Error>
where
    V: Fn(Option<SubtaskId>) -> InputValidator,
    O: Fn(TestcaseId) -> OutputGenerator,
{
    let path = path.as_ref();
    let task_dir = path
        .parent()
        .context("Invalid gen/GEN path")?
        .parent()
        .context("Invalid gen/GEN path")?;
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read gen/GEN from {}", path.display()))?;
    let mut file =
        parser::GENParser::parse(parser::Rule::file, &content).context("Cannot parse gen/GEN")?;
    let file = file.next().ok_or_else(|| anyhow!("Corrupted parser"))?; // extract the real file
    let mut testcase_count = 0;
    let mut subtask_id: SubtaskId = 0;
    let mut entries = vec![];
    let mut st_deps = Vec::new();
    let mut stname_table = HashMap::new();

    let mut default_subtask = Some(SubtaskInfo {
        id: 0,
        max_score: 100.0,
        is_default: true,
        ..Default::default()
    });

    let mut generators = find_source_file(
        task_dir,
        vec![
            "gen/generator.*",
            "gen/generatore.*",
            "gen/generator",
            "gen/generatore",
        ],
        task_dir,
        "Input file generator at",
        None,
        WriteBinTo::path("bin/generator"),
    );
    if generators.len() > 1 {
        let paths = generators.iter().map(|s| s.name()).collect::<Vec<_>>();
        bail!("Multiple generators found: {:?}", paths);
    } else if generators.is_empty() {
        bail!("No generator found");
    }
    let generator = generators.pop().map(Arc::new).unwrap();
    debug!("Detected input generator: {:?}", generator);

    for line in file.into_inner() {
        match line.as_rule() {
            parser::Rule::line => {
                let span = line.as_span();
                let line = line
                    .into_inner()
                    .next()
                    .ok_or_else(|| anyhow!("Corrupted parser"))?;
                match line.as_rule() {
                    parser::Rule::subtask => {
                        default_subtask.take(); // ignore the default subtask ever
                        let score = line
                            .into_inner()
                            .next()
                            .ok_or_else(|| anyhow!("Corrupted parser"))?
                            .as_str();
                        let path = path.strip_prefix(task_dir).unwrap_or(path);
                        entries.push(TaskInputEntry::Subtask(SubtaskInfo {
                            id: subtask_id,
                            max_score: score.parse::<f64>().context("Invalid subtask score")?,
                            span: CodeSpan::from_str(
                                path,
                                &content,
                                span.start(),
                                span.end() - span.start(),
                            )
                            .ok(),
                            is_default: false,
                            ..Default::default()
                        }));
                        subtask_id += 1;
                        st_deps.push(Vec::new());
                    }
                    parser::Rule::subtask_name => {
                        if let Some(TaskInputEntry::Subtask(subtask)) = entries.last_mut() {
                            let name = line
                                .into_inner()
                                .next()
                                .ok_or_else(|| anyhow!("Corrupted parser"))?
                                .as_str();
                            let name = cleanup_subtask_name(name)
                                .with_context(|| format!("Invalid subtask name: {:?}", name))?;
                            let old_id = stname_table.insert(name.clone(), subtask.id);
                            ensure!(old_id.is_none(), "Subtask name {:?} already used", name);
                            ensure!(
                                subtask.name.is_none(),
                                "Cannot assign the name of a subtask twice"
                            );
                            subtask.name = Some(name);
                        } else {
                            bail!("#STNAME: rule must immediately follow a #ST: in gen/GEN");
                        }
                    }
                    parser::Rule::subtask_dep => {
                        if let Some(TaskInputEntry::Subtask(_)) = entries.last_mut() {
                            for dependency in line.into_inner() {
                                let dependency_str = dependency.as_str();
                                st_deps.last_mut().unwrap().push(
                                    cleanup_subtask_name(dependency_str).with_context(|| {
                                        format!("Invalid subtask name: {}", dependency_str)
                                    })?,
                                );
                            }
                        } else {
                            bail!("#STDEP: rule must immediately follow a #ST: in gen/GEN");
                        }
                    }
                    parser::Rule::copy => {
                        if let Some(default) = default_subtask.take() {
                            entries.push(TaskInputEntry::Subtask(default));
                            st_deps.push(Vec::new());
                            subtask_id += 1;
                        }
                        let what = line
                            .into_inner()
                            .next()
                            .ok_or_else(|| anyhow!("Corrupted parser"))?
                            .as_str();
                        entries.push(TaskInputEntry::Testcase(TestcaseInfo::new(
                            testcase_count,
                            InputGenerator::StaticFile(task_dir.join(what)),
                            get_validator(Some(subtask_id - 1)),
                            get_output_gen(testcase_count),
                        )));
                        testcase_count += 1;
                    }
                    parser::Rule::command => {
                        if let Some(default) = default_subtask.take() {
                            entries.push(TaskInputEntry::Subtask(default));
                            st_deps.push(Vec::new());
                            subtask_id += 1;
                        }
                        let cmd: Vec<String> =
                            line.into_inner().map(|x| x.as_str().to_owned()).collect();
                        let output_generator = get_output_gen(testcase_count);
                        if let OutputGenerator::StaticFile(_) = output_generator {
                            bail!("Generator detected but no solution found. Cannot generate output files.");
                        }
                        entries.push(TaskInputEntry::Testcase(TestcaseInfo::new(
                            testcase_count,
                            InputGenerator::Custom(generator.clone(), cmd),
                            get_validator(Some(subtask_id - 1)),
                            output_generator,
                        )));
                        testcase_count += 1;
                    }
                    parser::Rule::comment => {}
                    parser::Rule::empty => {}
                    _ => unreachable!(),
                }
            }
            parser::Rule::EOI => {}
            _ => unreachable!(),
        }
    }

    for entry in &mut entries {
        let TaskInputEntry::Subtask(SubtaskInfo {
            id, dependencies, ..
        }) = entry
        else {
            continue;
        };
        *dependencies = st_deps[*id as usize]
            .iter()
            .map(|dep| {
                stname_table
                    .get(dep)
                    .copied()
                    .ok_or_else(|| anyhow!("Unknown subtask name: {dep}"))
            })
            .collect::<Result<_, _>>()?;
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;

    use anyhow::Error;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    use TaskInputEntry::*;

    use crate::ioi::format::italian_yaml::gen_gen::parse_gen_gen;
    use crate::ioi::format::italian_yaml::TaskInputEntry;
    use crate::ioi::{InputGenerator, InputValidator, OutputGenerator, SubtaskId, TestcaseId};
    use crate::SourceFile;

    fn make_task<S: AsRef<str>>(gen_gen: S) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("task.yaml"), "name: foo\ntitle: foo bar\n").unwrap();
        fs::create_dir(dir.path().join("gen")).unwrap();
        fs::create_dir(dir.path().join("sol")).unwrap();
        fs::write(dir.path().join("gen").join("GEN"), gen_gen.as_ref()).unwrap();
        fs::write(
            dir.path().join("gen").join("generator.py"),
            "#!/usr/bin/env python",
        )
        .unwrap();
        fs::write(
            dir.path().join("sol").join("solution.py"),
            "#!/usr/bin/env python",
        )
        .unwrap();
        dir
    }

    fn get_validator(_subtask: Option<SubtaskId>) -> InputValidator {
        InputValidator::AssumeValid
    }

    fn get_output_generator(_testcase: TestcaseId) -> OutputGenerator {
        let source = SourceFile::new("a.py", "", "", None, None::<&str>).unwrap();
        OutputGenerator::Custom(Arc::new(source), vec![])
    }

    fn get_entries(dir: &Path) -> Result<Vec<TaskInputEntry>, Error> {
        parse_gen_gen(
            dir.join("gen").join("GEN"),
            get_validator,
            get_output_generator,
        )
    }

    /// Build a task and return the parsed gen/GEN.
    fn get_parsed_gen_gen(gen_gen: &str) -> Result<Vec<TaskInputEntry>, Error> {
        let task = make_task(gen_gen);
        get_entries(task.path())
    }

    #[test]
    fn test_parser_single_line() {
        let entries = get_parsed_gen_gen("1234\n").unwrap();
        if let [Subtask(subtask), Testcase(testcase)] = entries.as_slice() {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.max_score as u32, 100);
            assert_eq!(testcase.id, 0);
            match &testcase.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["1234".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
        } else {
            panic!("Wrong entries returned: {:?}", entries);
        }
    }

    #[test]
    fn test_parser_single_line_without_ending_lf() {
        let entries = get_parsed_gen_gen("1234").unwrap();
        if let [Subtask(subtask), Testcase(testcase)] = entries.as_slice() {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.max_score as u32, 100);
            assert_eq!(testcase.id, 0);
            match &testcase.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["1234".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
        } else {
            panic!("Wrong entries returned: {:?}", entries);
        }
    }

    #[test]
    fn test_parser_single_line_with_comments() {
        let entries =
            get_parsed_gen_gen("# this is a comment\n1234\n# this is a comment too\n").unwrap();
        if let [Subtask(subtask), Testcase(testcase)] = entries.as_slice() {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.max_score as u32, 100);
            assert_eq!(testcase.id, 0);
            match &testcase.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["1234".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
        } else {
            panic!("Wrong entries returned: {:?}", entries);
        }
    }

    #[test]
    fn test_parser_comment_empty() {
        let entries = get_parsed_gen_gen("#\n1234\n#\n").unwrap();
        if let [Subtask(subtask), Testcase(testcase)] = entries.as_slice() {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.max_score as u32, 100);
            assert_eq!(testcase.id, 0);
            match &testcase.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["1234".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
        } else {
            panic!("Wrong entries returned: {:?}", entries);
        }
    }

    #[test]
    fn test_parser_comment_empty_no_ending_lf() {
        let entries = get_parsed_gen_gen("#\n1234\n#").unwrap();
        if let [Subtask(subtask), Testcase(testcase)] = entries.as_slice() {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.max_score as u32, 100);
            assert_eq!(testcase.id, 0);
            match &testcase.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["1234".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
        } else {
            panic!("Wrong entries returned: {:?}", entries);
        }
    }

    #[test]
    fn test_parser_line_with_comment() {
        let entries = get_parsed_gen_gen("1234 # normal comment\n5678 #risky comment").unwrap();
        if let [Subtask(subtask), Testcase(testcase1), Testcase(testcase2)] = entries.as_slice() {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.max_score as u32, 100);
            assert_eq!(testcase1.id, 0);
            assert_eq!(testcase2.id, 1);
            match &testcase1.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["1234".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
            match &testcase2.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["5678".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
        } else {
            panic!("Wrong entries returned: {:?}", entries);
        }
    }

    #[test]
    fn test_parser_multiple_lines() {
        let entries = get_parsed_gen_gen("1234\n5678\n").unwrap();
        if let [Subtask(subtask), Testcase(testcase1), Testcase(testcase2)] = entries.as_slice() {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.max_score as u32, 100);
            assert_eq!(testcase1.id, 0);
            assert_eq!(testcase2.id, 1);
            match &testcase1.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["1234".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
            match &testcase2.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["5678".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
        } else {
            panic!("Wrong entries returned: {:?}", entries);
        }
    }

    #[test]
    fn test_parser_copy() {
        let task = make_task("#COPY: random/file\n5678\n");
        let entries = get_entries(task.path()).unwrap();
        if let [Subtask(subtask), Testcase(testcase1), Testcase(testcase2)] = entries.as_slice() {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.max_score as u32, 100);
            assert_eq!(testcase1.id, 0);
            assert_eq!(testcase2.id, 1);
            match &testcase1.input_generator {
                InputGenerator::StaticFile(path) => {
                    assert_eq!(path, &task.path().join("random/file"))
                }
                InputGenerator::Custom(_, _) => panic!("Invalid generator"),
            }
            match &testcase2.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["5678".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
        } else {
            panic!("Wrong entries returned: {:?}", entries);
        }
    }

    #[test]
    fn test_parser_subtasks() {
        let task = make_task("#ST: 123\n#COPY: random/file\n5678\n#ST: 321\n1234\n");
        let entries = get_entries(task.path()).unwrap();
        if let [Subtask(subtask1), Testcase(testcase1), Testcase(testcase2), Subtask(subtask2), Testcase(testcase3)] =
            entries.as_slice()
        {
            assert_eq!(subtask1.id, 0);
            assert_eq!(subtask1.max_score as u32, 123);
            assert_eq!(testcase1.id, 0);
            assert_eq!(testcase2.id, 1);
            assert_eq!(subtask2.id, 1);
            assert_eq!(subtask2.max_score as u32, 321);
            match &testcase1.input_generator {
                InputGenerator::StaticFile(path) => {
                    assert_eq!(path, &task.path().join("random/file"))
                }
                InputGenerator::Custom(_, _) => panic!("Invalid generator"),
            }
            match &testcase2.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["5678".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
            match &testcase3.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["1234".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
        } else {
            panic!("Wrong entries returned: {:?}", entries);
        }
    }

    #[test]
    fn test_parser_subtasks_with_names() {
        let entries =
            get_parsed_gen_gen("#ST: 123\n#STNAME: name\n5678\n#ST: 321\n#STNAME:wow\n1234\n")
                .unwrap();
        if let [Subtask(subtask1), Testcase(_), Subtask(subtask2), Testcase(_)] = entries.as_slice()
        {
            assert_eq!(subtask1.id, 0);
            assert_eq!(subtask1.max_score as u32, 123);
            assert_eq!(subtask1.name.as_deref(), Some("name"));
            assert_eq!(subtask2.id, 1);
            assert_eq!(subtask2.max_score as u32, 321);
            assert_eq!(subtask2.name.as_deref(), Some("wow"));
        } else {
            panic!("Wrong entries returned: {:?}", entries);
        }
    }

    #[test]
    fn test_parser_subtasks_with_names_with_comments() {
        let entries = get_parsed_gen_gen(
            "#ST: 123\n#STNAME: name # comment\n5678\n#ST: 321\n#STNAME: wow#comment\n1234\n",
        )
        .unwrap();
        if let [Subtask(subtask1), Testcase(_), Subtask(subtask2), Testcase(_)] = entries.as_slice()
        {
            assert_eq!(subtask1.id, 0);
            assert_eq!(subtask1.max_score as u32, 123);
            assert_eq!(subtask1.name.as_deref(), Some("name"));
            assert_eq!(subtask2.id, 1);
            assert_eq!(subtask2.max_score as u32, 321);
            assert_eq!(subtask2.name.as_deref(), Some("wow"));
        } else {
            panic!("Wrong entries returned: {:?}", entries);
        }
    }

    #[test]
    fn test_parser_empty_lines() {
        let entries = get_parsed_gen_gen("\n\n1234\n\n\n5678\n\n").unwrap();
        if let [Subtask(subtask), Testcase(testcase1), Testcase(testcase2)] = entries.as_slice() {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.max_score as u32, 100);
            assert_eq!(testcase1.id, 0);
            assert_eq!(testcase2.id, 1);
            match &testcase1.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["1234".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
            match &testcase2.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["5678".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
        } else {
            panic!("Wrong entries returned: {:?}", entries);
        }
    }

    #[test]
    fn test_parser_spaces_before_and_after() {
        let entries = get_parsed_gen_gen("  \t 1234\t  \t\n").unwrap();
        if let [Subtask(subtask), Testcase(testcase)] = entries.as_slice() {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.max_score as u32, 100);
            assert_eq!(testcase.id, 0);
            match &testcase.input_generator {
                InputGenerator::Custom(_, args) => assert_eq!(args, &vec!["1234".to_string()]),
                InputGenerator::StaticFile(_) => panic!("Invalid generator"),
            }
        } else {
            panic!("Wrong entries returned: {:?}", entries);
        }
    }

    #[test]
    fn test_parser_subtask_name_empty() {
        let res = get_parsed_gen_gen("#ST:1\n#STNAME:");
        assert!(res.is_err());
    }

    #[test]
    fn test_parser_subtask_name_with_spaces() {
        let res = get_parsed_gen_gen("#ST:1\n#STNAME: two names");
        assert!(res.is_err());
        let res = get_parsed_gen_gen("#ST:1\n#STNAME: two\tnames");
        assert!(res.is_err());
        let res = get_parsed_gen_gen("#ST:1\n#STNAME: two\u{200B}names");
        assert!(res.is_err());
        let res = get_parsed_gen_gen("#ST:1\n#STNAME: two\u{FEFF}names");
        assert!(res.is_err());
        let res = get_parsed_gen_gen("#ST:1\n#STNAME: two\u{180E}names");
        assert!(res.is_err());
        let res = get_parsed_gen_gen("#ST:1\n#STNAME: a b");
        assert!(res.is_err());
    }

    #[test]
    fn test_parser_subtask_name_illegal_characters() {
        let res = get_parsed_gen_gen("#ST:1\n#STNAME: n*pe");
        assert!(res.is_err());
    }

    #[test]
    fn test_parser_subtask_name_start_with_dash() {
        let res = get_parsed_gen_gen("#ST:1\n#STNAME: -nope");
        assert!(res.is_err());
    }

    #[test]
    fn test_parser_subtask_name_double() {
        let res = get_parsed_gen_gen("#ST:1\n#STNAME: one\n#STNAME: two");
        assert!(res.is_err());
    }

    #[test]
    fn test_parser_subtask_name_utf_and_symbols() {
        let res = get_parsed_gen_gen("#ST:1\n#STNAME: n²logn");
        assert!(res.is_ok());
        let res = get_parsed_gen_gen("#ST:1\n#STNAME: N<=2");
        assert!(res.is_ok());
        let res = get_parsed_gen_gen("#ST:1\n#STNAME: N=5_&&_M=2");
        assert!(res.is_ok());
        let res = get_parsed_gen_gen("#ST:1\n#STNAME: tree-line");
        assert!(res.is_ok());
    }
}
