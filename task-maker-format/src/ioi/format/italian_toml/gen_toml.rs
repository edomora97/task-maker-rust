use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use serde::Deserialize;
use task_maker_lang::GraderMap;
use toml::Value;

use crate::ioi::italian_yaml::{cleanup_subtask_name, TaskYAML};
use crate::ioi::{
    InputGenerator, InputValidator, OutputGenerator, SubtaskInfo, TaskType, TestcaseInfo,
    TM_VALIDATION_FILE_NAME,
};
use crate::{find_source_file, SourceFile, WriteBinTo};

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum SamplesConfig {
    Simple(usize),
    Detailed {
        group_name: Option<String>,
        num: usize,
        pattern: Option<String>,
    },
}

impl SamplesConfig {
    fn num(&self) -> usize {
        match self {
            Self::Simple(n) => *n,
            Self::Detailed { num, .. } => *num,
        }
    }

    fn group_name(&self) -> &str {
        match self {
            Self::Simple(_) => "samples",
            Self::Detailed { group_name, .. } => group_name.as_deref().unwrap_or("samples"),
        }
    }

    fn path(&self, task_name: &str, index: usize) -> PathBuf {
        let pattern = match self {
            Self::Simple(_) => None,
            Self::Detailed { pattern, .. } => pattern.as_ref(),
        };
        if let Some(pattern) = pattern {
            PathBuf::from_str(&pattern.replace("$#", &index.to_string())).unwrap()
        } else {
            PathBuf::new()
                .join("statement")
                .join(format!("{task_name}.input{index}.txt"))
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
enum CommandLineArguments {
    SpaceSeparated(String),
    List(Vec<String>),
}

impl CommandLineArguments {
    fn instantiate(&self, variables: &HashMap<String, Value>) -> Result<Vec<String>> {
        let map_arg = |arg: &str| -> Result<String> {
            if let Some(var) = arg.strip_prefix("$") {
                let Some(v) = variables.get(var) else {
                    bail!("Unknown variable {var}")
                };
                match v {
                    Value::String(s) => Ok(s.clone()),
                    Value::Integer(v) => Ok(v.to_string()),
                    Value::Float(v) => Ok(v.to_string()),
                    Value::Boolean(v) => Ok(v.to_string()),
                    _ => bail!("Invalid value {v} for variable {var}"),
                }
            } else {
                Ok(arg.to_string())
            }
        };
        match self {
            CommandLineArguments::SpaceSeparated(x) => x.split_whitespace().map(map_arg).collect(),
            CommandLineArguments::List(x) => x.iter().map(|x| map_arg(x)).collect(),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
enum TestCase {
    Args(CommandLineArguments),
    Detailed {
        generator: Option<String>,
        args: CommandLineArguments,
        group_name: Option<String>,
        repeat: Option<usize>,
    },
}

impl TestCase {
    fn args(&self) -> &CommandLineArguments {
        match self {
            Self::Args(a) => a,
            Self::Detailed { args, .. } => args,
        }
    }

    fn generator(&self) -> Option<&str> {
        match self {
            Self::Args(_) => None,
            Self::Detailed { generator, .. } => generator.as_deref(),
        }
    }

    fn group_name(&self) -> Option<&str> {
        match self {
            Self::Args(_) => None,
            Self::Detailed { group_name, .. } => group_name.as_deref(),
        }
    }

    fn consume_repeat(&mut self) -> Option<usize> {
        match self {
            Self::Args(_) => None,
            Self::Detailed { repeat, .. } => std::mem::take(repeat),
        }
    }
}

#[derive(Deserialize, Debug)]
struct GroupConfig {
    #[serde(default = "default_repeat")]
    repeat: usize,

    generator: Option<String>,

    #[serde(default)]
    include: Vec<String>,

    #[serde(default)]
    copy: Vec<PathBuf>,

    #[serde(default)]
    testcases: Vec<TestCase>,

    #[serde(flatten)]
    local_constants: HashMap<String, Value>,
}

impl GroupConfig {
    fn resolve_repeats(&mut self) {
        let repeats = self.repeat;
        self.repeat = 1;
        self.testcases = self
            .testcases
            .iter()
            .cloned()
            .flat_map(|mut x| {
                let n = x.consume_repeat().unwrap_or(repeats);
                std::iter::repeat_n(x, n)
            })
            .collect();
        self.repeat = 1;
    }
}

#[derive(Deserialize, Debug)]
struct SubtaskConfig {
    score: f64,

    #[serde(default)]
    always_show_testcases: bool,

    #[serde(flatten)]
    group: GroupConfig,

    validator: Option<String>,
    validator_args: Option<CommandLineArguments>,
}

#[derive(Deserialize, Debug)]
struct GenConfig {
    #[serde(default = "default_generator")]
    generator: String,

    #[serde(default = "default_validator")]
    validator: String,

    #[serde(default = "default_validator_args")]
    validator_args: CommandLineArguments,

    samples: SamplesConfig,

    #[serde(default)]
    group: IndexMap<String, GroupConfig>,

    subtask: IndexMap<String, SubtaskConfig>,

    #[serde(flatten)]
    constants: HashMap<String, Value>,
}

fn default_generator() -> String {
    "generator".to_string()
}

fn default_validator() -> String {
    "validator".to_string()
}

fn default_repeat() -> usize {
    1
}

fn default_validator_args() -> CommandLineArguments {
    CommandLineArguments::List(vec!["$FILENAME#".to_string(), "$SUBTASK_NAME#".to_string()])
}

fn detect_validator(task_dir: &Path, validator: &str) -> Result<Arc<SourceFile>> {
    let mut validators = find_source_file(
        task_dir,
        vec![&format!("gen/{validator}.*")],
        task_dir,
        "Input file validator at",
        None,
        WriteBinTo::None,
    );
    if validators.len() > 1 {
        let paths = validators.iter().map(|s| s.name()).collect::<Vec<_>>();
        bail!("Multiple validators found: {:?}", paths);
    } else if validators.is_empty() {
        bail!("No validator `{validator}` found");
    }
    Ok(validators.pop().map(Arc::new).unwrap())
}

/// Finds a generator with the specified name.
pub fn get_generator(generator: &str, task_dir: &Path) -> Result<Arc<SourceFile>> {
    let mut generators = find_source_file(
        task_dir,
        vec![&format!("gen/{generator}.*")],
        task_dir,
        "Input file generator at",
        None,
        WriteBinTo::None,
    );
    if generators.len() > 1 {
        let paths = generators.iter().map(|s| s.name()).collect::<Vec<_>>();
        bail!("Multiple generators found: {:?}", paths);
    } else if generators.is_empty() {
        bail!("No generator `{generator}` found");
    }
    Ok(generators.pop().map(Arc::new).unwrap())
}

pub(super) fn parse(
    task_dir: &Path,
    config: &TaskYAML,
    task_type: &TaskType,
    grader_map: Arc<GraderMap>,
) -> Result<(HashMap<u32, SubtaskInfo>, HashMap<u32, TestcaseInfo>)> {
    let gen_toml_path = task_dir.join("gen").join("gen.toml");
    let gen_toml = std::fs::read_to_string(gen_toml_path).context("Failed to open gen/gen.toml")?;
    let gen_toml: GenConfig =
        toml::from_str(&gen_toml).context("Failed to deserialize task.toml")?;

    debug!("parsed {gen_toml:?}");

    let GenConfig {
        generator,
        validator,
        validator_args,
        samples,
        group,
        subtask,
        constants,
    } = gen_toml;

    // Ensure subtask (and group) names are valid.
    let group: Result<IndexMap<_, _>> = group
        .into_iter()
        .map(|x| Ok((cleanup_subtask_name(&x.0)?, x.1)))
        .collect();
    let mut group = group?;

    let subtask: Result<IndexMap<_, _>> = subtask
        .into_iter()
        .map(|x| Ok((cleanup_subtask_name(&x.0)?, x.1)))
        .collect();
    let mut subtask = subtask?;

    let mut group_to_testcases: HashMap<String, Vec<u32>> = HashMap::new();
    let mut group_inclusions: HashMap<String, Vec<String>> = HashMap::new();
    let mut testcases = HashMap::new();

    // First, generate all the testcases, and all the groups.

    let output_generator = if let TaskType::Batch(_) = task_type {
        super::detect_output_generator(task_dir, grader_map)?
    } else {
        OutputGenerator::NotAvailable
    };

    {
        let name = samples.group_name();
        group_inclusions.insert(name.to_string(), vec![]);
        let group_tcs = group_to_testcases.entry(name.to_string()).or_default();

        for i in 0..samples.num() {
            let id = testcases.len() as u32;
            group_tcs.push(id);
            testcases.insert(
                id,
                TestcaseInfo {
                    id,
                    input_generator: InputGenerator::StaticFile(
                        task_dir.join(samples.path(&config.name, i)),
                    ),
                    output_generator: output_generator.clone(),
                    input_file: None,
                    official_output_file: None,
                },
            );
        }
    }

    let mut process_group = |name: &str, group: &mut GroupConfig| -> Result<()> {
        group.resolve_repeats();
        let mut constants = constants.clone();
        for (k, v) in group.local_constants.iter() {
            constants.insert(k.clone(), v.clone());
        }
        let generator = group.generator.as_ref().unwrap_or(&generator).as_str();

        // Ensure an entry for the group is created.
        let group_incl = group_inclusions.entry(name.to_string()).or_default();
        let group_tcs = group_to_testcases.entry(name.to_string()).or_default();

        for incl in group.include.iter() {
            group_incl.push(incl.clone());
        }

        for c in group.copy.iter() {
            let id = testcases.len() as u32;
            group_tcs.push(id);
            testcases.insert(
                id,
                TestcaseInfo {
                    id,
                    input_generator: InputGenerator::StaticFile(task_dir.join(c)),
                    output_generator: output_generator.clone(),
                    input_file: None,
                    official_output_file: None,
                },
            );
        }

        let mut process_testcase = |testcase: &TestCase| -> Result<()> {
            let id = testcases.len() as u32;
            constants.insert("#".to_owned(), Value::Integer(id as i64));
            group_to_testcases
                .entry(name.to_string())
                .or_default()
                .push(id);
            if let Some(g) = testcase.group_name() {
                group_to_testcases
                    .entry(g.to_string())
                    .or_default()
                    .push(id);
            }
            let generator = testcase.generator().unwrap_or(generator);
            let generator = get_generator(generator, task_dir)?;
            let input_generator =
                InputGenerator::Custom(generator, testcase.args().instantiate(&constants)?);
            testcases.insert(
                id,
                TestcaseInfo {
                    id,
                    input_generator,
                    output_generator: output_generator.clone(),
                    input_file: None,
                    official_output_file: None,
                },
            );
            Ok(())
        };

        for (i, testcase) in group.testcases.iter().enumerate() {
            process_testcase(testcase)
                .with_context(|| format!("while processing testcase {i} of group {name}"))?;
        }

        Ok(())
    };

    for group in group.iter_mut() {
        process_group(group.0, group.1)?;
    }

    for subtask in subtask.iter_mut() {
        process_group(subtask.0, &mut subtask.1.group)?;
    }

    // Finally, prepare subtasks.
    let mut subtasks = HashMap::new();

    // Due to a limitation in the IOI format, all testcases should belong to *some* subtask.
    // We assign them to the first subtask that uses them.
    let mut testcases_with_owner = HashSet::new();

    for (id, (name, subtask)) in subtask.iter().enumerate() {
        let id = id as u32;

        let validator_args = subtask.validator_args.as_ref().unwrap_or(&validator_args);
        let mut constants = constants.clone();
        for (k, v) in subtask.group.local_constants.iter() {
            constants.insert(k.clone(), v.clone());
        }
        constants.insert("SUBTASK_NAME#".to_string(), Value::String(name.clone()));
        constants.insert(
            "FILENAME#".to_string(),
            Value::String(TM_VALIDATION_FILE_NAME.to_string()),
        );
        let validator_args = validator_args.instantiate(&constants).with_context(|| {
            format!("when instantiating command line for validator of subtask {name}")
        })?;

        let validator = subtask.validator.as_ref().unwrap_or(&validator);
        let validator = detect_validator(task_dir, validator)
            .with_context(|| format!("when finding validator of subtask {name}"))?;

        let input_validator = InputValidator::Custom(validator, validator_args);

        let mut visited = HashSet::new();
        let mut testcases_owned = vec![];
        let mut testcases = vec![];

        let mut stack = vec![name.clone()];

        while let Some(cur) = stack.pop() {
            if !visited.insert(cur.clone()) {
                continue;
            }
            // Handle the special case of wildcard include.
            if &cur == "*" {
                for s in group_to_testcases.iter() {
                    stack.push(s.0.clone());
                }
                continue;
            }
            let cur_testcases = group_to_testcases.get(&cur).with_context(|| {
                format!("unknown group {cur} when computing testcases for subtask {name}")
            })?;
            testcases.extend_from_slice(cur_testcases);
            for tc in cur_testcases {
                if testcases_with_owner.insert(*tc) {
                    testcases_owned.push(*tc);
                }
            }
            for dep in group_inclusions.get(&cur).unwrap() {
                stack.push(dep.clone());
            }
        }

        testcases.sort();
        // A testcase can be reachable through more than one included group (e.g. its own
        // group and a `group_name` override both ending up in the same subtask); without
        // deduplicating it would be counted twice when averaging the subtask's score.
        testcases.dedup();
        testcases_owned.sort();

        subtasks.insert(
            id,
            SubtaskInfo {
                id,
                name: Some(name.clone()),
                max_score: subtask.score,
                testcases,
                testcases_owned,
                input_validator,
                always_show_testcases: subtask.always_show_testcases,
                ..Default::default()
            },
        );
    }

    // Return an error if some testcase ends up being unused.
    for testcase in testcases.keys() {
        if !testcases_with_owner.contains(testcase) {
            for (g, tc) in group_to_testcases.iter() {
                if tc.contains(testcase) {
                    bail!("Testcase {testcase}, from group {g}, is never used.");
                }
            }
        }
    }

    Ok((subtasks, testcases))
}
