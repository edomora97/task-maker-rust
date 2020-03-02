use std::collections::HashMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use failure::_core::fmt::Formatter;
use failure::{bail, format_err, Error};
use pest::Parser;

use crate::ioi::format::italian_yaml::TaskInputEntry;
use crate::ioi::{
    InputGenerator, InputValidator, OutputGenerator, SubtaskId, SubtaskInfo, TestcaseId,
    TestcaseInfo,
};
use crate::SourceFile;

/// This module exists because of a `pest`'s bug: https://github.com/pest-parser/pest/issues/326
#[allow(missing_docs)]
mod parser {
    /// The gen/cases.gen file parser.
    #[derive(Parser)]
    #[grammar = "ioi/format/italian_yaml/cases.gen.pest"]
    pub struct CasesGenParser;
}

/// Helper type for lightening the types.
type Pair<'a> = pest::iterators::Pair<'a, parser::Rule>;

/// A manager is either a generator or a validator, since they have the same internal structure they
/// are abstracted as a `Manager`.
#[derive(Debug)]
struct Manager {
    /// Source file of the manager.
    source: Arc<SourceFile>,
    /// Symbolic arguments to pass to the manager. All the variables will be replaced with their
    /// value.
    args: Vec<String>,
}

/// Operand of a constraint. It is either a constant integer value or a symbolic variable to
/// substitute.
#[derive(Debug, PartialEq, Eq)]
enum ConstraintOperand {
    /// This operand is a constant integer value.
    Constant(i64),
    /// This operand is a symbolic variable. It is stored the variable name, without the dollar
    /// sign.
    Variable(String),
}

/// The operator of a constraint.
#[derive(Debug, PartialEq, Eq)]
enum ConstraintOperator {
    /// Operator `<`.
    Less,
    /// Operator `<=`.
    LessEqual,
    /// Operator `>`.
    Greater,
    /// Operator `>=`.
    GreaterEqual,
    /// Operator `=`.
    Equal,
}

/// A constraint between the variables. It is in the following format:
///     operand (operator operand)+
/// Note that the number of operands is one more than the operators.
/// All the operators must be _in the same direction_: in the same constraint there cannot be both
/// a _less_ operator and a _greater_ one.
#[derive(Default)]
struct Constraint {
    /// List of the operands of the constraint.
    operands: Vec<ConstraintOperand>,
    /// List of the operators of the contraint.
    operators: Vec<ConstraintOperator>,
}

/// Temporary structure with the metadata of the parsing of the `cases.gen` file. The internal data
/// is filled and updated during the parsing of the file.
#[derive(Derivative)]
#[derivative(Debug)]
struct CasesGen<O>
where
    O: Fn(TestcaseId) -> OutputGenerator,
{
    /// The base directory of the task.
    task_dir: PathBuf,
    /// The function to call for getting the `OutputGenerator` for a given testcase.
    #[derivative(Debug = "ignore")]
    get_output_gen: O,
    /// The resulting `TaskInputEntry` that will be produced after the parsing of the `cases.gen`
    /// file.
    result: Vec<TaskInputEntry>,
    /// The list of constraints found in the file.
    constraints: Vec<Constraint>,
    /// The list of additional constraints for the current subtask.
    subtask_constraints: Vec<Constraint>,
    /// The list of all the generators found, indexed by generator name.
    generators: HashMap<String, Manager>,
    /// The list of all the validators found, indexed by validator name.
    validators: HashMap<String, Manager>,
    /// The name of the default generator of the task. It's the generator with name `default`, if
    /// present. Each subtask will use this generator, unless specified.
    default_generator: Option<String>,
    /// The name of the default validator of the task. It's the validator with name `default`, if
    /// present. Each subtask will use this validator, unless specified.
    default_validator: Option<String>,
    /// The current generator for this subtask, it's the task's default, unless specified.
    current_generator: Option<String>,
    /// The current validator for this subtask, it's the task's default, unless specified.
    current_validator: Option<String>,
    /// The identifier of the next subtask to process.
    subtask_id: SubtaskId,
    /// The identifier of the next testcase to process.
    testcase_id: TestcaseId,
}

/// Parse the `gen/cases.gen` file extracting the subtasks and the testcases.
pub(crate) fn parse_cases_gen<P: AsRef<Path>, O>(
    path: P,
    output_gen: O,
) -> Result<Box<dyn Iterator<Item = TaskInputEntry>>, Error>
where
    O: Fn(TestcaseId) -> OutputGenerator,
{
    Ok(Box::new(
        CasesGen::new(path, output_gen)?.result.into_iter(),
    ))
}

impl<O> CasesGen<O>
where
    O: Fn(TestcaseId) -> OutputGenerator,
{
    /// Parse the `cases.gen` file pointed at the specified path.
    fn new<P: AsRef<Path>>(path: P, output_gen: O) -> Result<CasesGen<O>, Error> {
        let task_dir = path
            .as_ref()
            .parent()
            .expect("Invalid gen/cases.gen path")
            .parent()
            .expect("Invalid gen/cases.gen path");
        let content = std::fs::read_to_string(&path)?;
        let mut file = parser::CasesGenParser::parse(parser::Rule::file, &content)?;
        let file = file.next().ok_or_else(|| format_err!("Corrupted parser"))?; // extract the real file

        let mut cases = CasesGen {
            task_dir: task_dir.into(),
            get_output_gen: output_gen,
            result: vec![],
            constraints: vec![],
            subtask_constraints: vec![],
            generators: Default::default(),
            validators: Default::default(),
            default_generator: None,
            default_validator: None,
            current_generator: None,
            current_validator: None,
            subtask_id: 0,
            testcase_id: 0,
        };

        for line in file.into_inner() {
            match line.as_rule() {
                parser::Rule::line => {
                    let line = line
                        .into_inner()
                        .next()
                        .ok_or_else(|| format_err!("Corrupted parser"))?;
                    match line.as_rule() {
                        parser::Rule::command => {
                            let command = line
                                .into_inner()
                                .next()
                                .ok_or_else(|| format_err!("Corrupted parser"))?;
                            cases.parse_command(command)?;
                        }
                        parser::Rule::testcase => {
                            cases.parse_testcase(line.as_str(), cases.current_generator.clone())?;
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
        Ok(cases)
    }

    /// Parse a line with a command: one of the `:` prefixed actions.
    fn parse_command(&mut self, line: Pair) -> Result<(), Error> {
        match line.as_rule() {
            parser::Rule::GEN => {
                self.parse_gen(line)?;
            }
            parser::Rule::VAL => {
                self.parse_val(line)?;
            }
            parser::Rule::CONSTRAINT => {
                self.parse_constraint(line)?;
            }
            parser::Rule::SUBTASK => {
                self.parse_subtask(line)?;
            }
            parser::Rule::COPY => {
                self.parse_copy(line)?;
            }
            parser::Rule::RUN => {
                self.parse_run(line)?;
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    /// Parse a raw testcase, a line not starting with `:`.
    fn parse_testcase(
        &mut self,
        line: &str,
        current_generator: Option<String>,
    ) -> Result<(), Error> {
        if self.subtask_id == 0 {
            bail!("Cannot add a testcase outside a subtask");
        }
        let current_generator = if current_generator.is_none()
            || !self
                .generators
                .contains_key(current_generator.as_ref().unwrap())
        {
            bail!("Cannot generate testcase: no default generator set");
        } else {
            current_generator.unwrap()
        };
        let args = shell_words::split(line)
            .map_err(|e| format_err!("Invalid command arguments for testcase '{}': {}", line, e))?;
        let generator =
            InputGenerator::Custom(self.generators[&current_generator].source.clone(), args);
        // TODO check arguments
        self.result.push(TaskInputEntry::Testcase(TestcaseInfo {
            id: self.testcase_id,
            input_generator: generator,
            input_validator: self.get_validator(),
            output_generator: (self.get_output_gen)(self.testcase_id),
        }));
        self.testcase_id += 1;
        Ok(())
    }

    /// Parse a `GEN` or a `VAL`, since they have the same internal format their parsing function is
    /// abstracted in this.
    fn process_gen_val(
        line: Vec<Pair>,
        task_dir: &Path,
        subtask_id: SubtaskId,
        default: &mut Option<String>,
        current: &mut Option<String>,
        managers: &mut HashMap<String, Manager>,
        kind: &str,
    ) -> Result<(), Error> {
        let name = line[0].as_str();
        // Case 1: GEN|VAL name
        // Set the current generator/validator to the specified one
        if line.len() == 1 {
            if subtask_id == 0 {
                bail!(
                    "Cannot set the current {} to {} outside a subtask",
                    kind,
                    name
                );
            }
            if !managers.contains_key(name) {
                bail!(
                    "Cannot set the current {} to '{}': unknown {}",
                    kind,
                    name,
                    kind
                );
            }
            *current = Some(name.to_string());
        // Case 2: GEN|VAL name path [args...]
        // Add a new generator/validator to the list
        } else {
            let path = line[1].as_str();
            let path = task_dir.join(path);
            if !path.exists() {
                bail!(
                    "Cannot add {} '{}': '{}' does not exists",
                    kind,
                    name,
                    path.display()
                );
            }
            let source = SourceFile::new(
                &path,
                task_dir,
                None,
                Some(
                    task_dir
                        .join("bin")
                        .join(path.file_name().expect("invalid file name")),
                ),
            )
            .map(Arc::new)
            .ok_or_else(|| {
                format_err!("Cannot use {} '{}': unknown language", kind, path.display())
            })?;
            let args = shell_words::split(line[2].as_str())
                .map_err(|e| format_err!("Invalid arguments of '{}': {}", name, e))?;
            if managers.contains_key(name) {
                bail!("Duplicate {} with name {}", kind, name);
            }
            managers.insert(name.to_string(), Manager { source, args });
            if default.is_none() || name == "default" {
                *default = Some(name.to_string());
            }
        }
        Ok(())
    }

    /// Parse a `:GEN` command.
    fn parse_gen(&mut self, line: Pair) -> Result<(), Error> {
        let line: Vec<_> = line.into_inner().collect();
        CasesGen::<O>::process_gen_val(
            line,
            &self.task_dir,
            self.subtask_id,
            &mut self.default_generator,
            &mut self.current_generator,
            &mut self.generators,
            "generator",
        )?;
        Ok(())
    }

    /// Parse a `:VAL` command.
    fn parse_val(&mut self, line: Pair) -> Result<(), Error> {
        let line: Vec<_> = line.into_inner().collect();
        CasesGen::<O>::process_gen_val(
            line,
            &self.task_dir,
            self.subtask_id,
            &mut self.default_validator,
            &mut self.current_validator,
            &mut self.validators,
            "validator",
        )?;
        Ok(())
    }

    /// Parse a `:CONSTRAINT` command.
    fn parse_constraint(&mut self, line: Pair) -> Result<(), Error> {
        let line_str = line.as_str().to_string();
        let line: Vec<_> = line.into_inner().collect();
        let mut constraint = Constraint::default();
        let mut direction = None;
        for item in line {
            match item.as_rule() {
                parser::Rule::number => {
                    constraint.operands.push(ConstraintOperand::Constant(
                        i64::from_str(item.as_str()).map_err(|e| {
                            format_err!(
                                "Invalid integer constant '{}' in constraint '{}': {}",
                                item.as_str(),
                                line_str,
                                e
                            )
                        })?,
                    ));
                }
                parser::Rule::variable => {
                    constraint
                        .operands
                        .push(ConstraintOperand::Variable(item.as_str()[1..].to_string()));
                }
                parser::Rule::comp_operator => {
                    let operator = ConstraintOperator::from_str(item.as_str())?;
                    let dir = match operator {
                        ConstraintOperator::Less | ConstraintOperator::LessEqual => Some(true),
                        ConstraintOperator::Greater | ConstraintOperator::GreaterEqual => {
                            Some(false)
                        }
                        ConstraintOperator::Equal => None,
                    };
                    if direction.is_none() {
                        direction = dir;
                    }
                    if dir.is_some() && direction != dir {
                        bail!("Malformed constraint: inequality direction must be the same")
                    }
                    constraint.operators.push(operator)
                }
                _ => unreachable!(),
            }
        }
        if constraint.operators.len() + 1 != constraint.operands.len() {
            bail!("Malformed constraint: invalid number of operators");
        }
        if constraint.operands.len() < 2 {
            bail!("Malformed constraint: too few operands");
        }
        // subtask_id = 0 means no subtask has been defined yet, so this constraint is global
        if self.subtask_id == 0 {
            self.constraints.push(constraint);
        } else {
            self.subtask_constraints.push(constraint);
        }
        Ok(())
    }

    /// Parse a `:SUBTASK` command.
    fn parse_subtask(&mut self, line: Pair) -> Result<(), Error> {
        let line: Vec<_> = line.into_inner().collect();
        self.current_generator = self.default_generator.clone();
        self.current_validator = self.default_validator.clone();
        let score = f64::from_str(line[0].as_str()).map_err(|e| {
            format_err!(
                "Invalid subtask score for subtask {}: {}",
                self.subtask_id,
                e
            )
        })?;
        let description = if line.len() >= 2 {
            Some(line[1].as_str().to_string())
        } else {
            None
        };
        self.result.push(TaskInputEntry::Subtask(SubtaskInfo {
            id: self.subtask_id,
            description,
            max_score: score,
            testcases: HashMap::new(),
        }));
        self.subtask_id += 1;
        Ok(())
    }

    /// Parse a `:COPY` command.
    fn parse_copy(&mut self, line: Pair) -> Result<(), Error> {
        if self.subtask_id == 0 {
            bail!("Cannot add a COPY testcase outside a subtask");
        }
        let path = line.into_inner().next().expect("corrupted parser").as_str();
        let path = self.task_dir.join(path);
        if !path.exists() {
            bail!(
                "Cannot copy testcase from '{}': file not found",
                path.display()
            );
        }
        self.result.push(TaskInputEntry::Testcase(TestcaseInfo {
            id: self.testcase_id,
            input_generator: InputGenerator::StaticFile(path),
            input_validator: self.get_validator(),
            output_generator: (self.get_output_gen)(self.testcase_id),
        }));
        self.testcase_id += 1;
        Ok(())
    }

    /// Get the current validator for the next testcase.
    fn get_validator(&self) -> InputValidator {
        match &self.current_validator {
            // TODO: build the command line argument
            Some(val) => InputValidator::Custom(
                self.validators[val].source.clone(),
                vec![
                    "tm_validation_file".to_string(),
                    (self.subtask_id - 1).to_string(),
                ],
            ),
            None => InputValidator::AssumeValid,
        }
    }

    /// Parse a `:RUN` command.
    fn parse_run(&mut self, line: Pair) -> Result<(), Error> {
        if self.subtask_id == 0 {
            bail!("Cannot add a testcase outside a subtask");
        }
        let line: Vec<_> = line.into_inner().collect();
        let name = line[0].as_str();
        let args = line[1].as_str();
        if !self.generators.contains_key(name) {
            bail!("Generator '{}' not declared", name);
        }
        self.parse_testcase(args, Some(name.into()))?;
        Ok(())
    }
}

impl FromStr for ConstraintOperator {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "<" => Ok(ConstraintOperator::Less),
            "<=" => Ok(ConstraintOperator::LessEqual),
            ">" => Ok(ConstraintOperator::Greater),
            ">=" => Ok(ConstraintOperator::GreaterEqual),
            "=" => Ok(ConstraintOperator::Equal),
            _ => bail!("Invalid operator: {}", s),
        }
    }
}

impl ToString for ConstraintOperator {
    fn to_string(&self) -> String {
        match self {
            ConstraintOperator::Less => "<",
            ConstraintOperator::LessEqual => "<=",
            ConstraintOperator::Greater => ">",
            ConstraintOperator::GreaterEqual => ">=",
            ConstraintOperator::Equal => "=",
        }
        .to_string()
    }
}

impl ToString for ConstraintOperand {
    fn to_string(&self) -> String {
        match self {
            ConstraintOperand::Constant(k) => k.to_string(),
            ConstraintOperand::Variable(v) => format!("${}", v),
        }
    }
}

impl Debug for Constraint {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        let mut constraint = self.operands[0].to_string();
        for (op, var) in self.operators.iter().zip(self.operands[1..].iter()) {
            constraint += &format!(" {} {}", op.to_string(), var.to_string());
        }
        write!(f, "{}", constraint)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use failure::Error;
    use spectral::assert_that;
    use spectral::prelude::*;
    use tempdir::TempDir;

    use crate::ioi::format::italian_yaml::cases_gen::{
        CasesGen, ConstraintOperand, ConstraintOperator,
    };
    use crate::ioi::format::italian_yaml::TaskInputEntry;
    use crate::ioi::{InputGenerator, InputValidator, OutputGenerator, TestcaseId};

    struct TestHelper(TempDir);

    impl TestHelper {
        fn new() -> TestHelper {
            TestHelper(TempDir::new("tm-test").unwrap())
        }

        fn add_file<P: AsRef<Path>>(&self, path: P) -> &Self {
            if let Some(parent) = path.as_ref().parent() {
                std::fs::create_dir_all(self.0.path().join(parent)).unwrap();
            }
            std::fs::write(self.0.path().join(path), "").unwrap();
            self
        }

        fn cases_gen<S: AsRef<str>>(
            &self,
            content: S,
        ) -> Result<CasesGen<impl Fn(TestcaseId) -> OutputGenerator>, Error> {
            std::fs::create_dir_all(self.0.path().join("gen")).unwrap();
            let dest = self.0.path().join("gen/cases.gen");
            std::fs::write(&dest, content.as_ref()).unwrap();
            CasesGen::new(dest, |_| OutputGenerator::StaticFile("nope".into()))
        }
    }

    #[test]
    fn test_comment() {
        let gen = TestHelper::new().cases_gen("# this is a comment").unwrap();
        assert_eq!(gen.result.len(), 0);
    }

    /**********************
     * : GEN
     *********************/

    #[test]
    fn test_add_generator() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py")
            .unwrap();
        assert!(gen.generators.contains_key("gen"));
    }

    #[test]
    fn test_add_generator_with_args() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py N M seed")
            .unwrap();
        assert!(gen.generators.contains_key("gen"));
        assert_eq!(gen.generators["gen"].args, vec!["N", "M", "seed"]);
    }

    #[test]
    fn test_add_generator_auto_default() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py")
            .unwrap();
        assert_eq!(gen.default_generator, Some("gen".into()));
    }

    #[test]
    fn test_add_generator_default() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .add_file("gen/default.py")
            .cases_gen(":GEN gen gen/generator.py\n:GEN default gen/default.py")
            .unwrap();
        assert_eq!(gen.default_generator, Some("default".into()));
    }

    #[test]
    fn test_set_current_generator() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(
                ":GEN gen gen/generator.py\n:GEN gen2 gen/generator.py\n:SUBTASK 42\n:GEN gen2",
            )
            .unwrap();
        assert_eq!(gen.default_generator, Some("gen".into()));
        assert_eq!(gen.current_generator, Some("gen2".into()));
    }

    #[test]
    fn test_set_current_generator_outside_subtask() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py\n:GEN gen");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("outside a subtask");
    }

    #[test]
    fn test_set_current_generator_unknown() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py\n:SUBTASK 42\n:GEN lolnope");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("unknown generator");
    }

    #[test]
    fn test_add_generator_duplicate() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py\n:GEN gen gen/generator.py");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("Duplicate generator");
    }

    #[test]
    fn test_add_generator_missing_file() {
        let gen = TestHelper::new().cases_gen(":GEN gen gen/generator.py");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("does not exists");
    }

    #[test]
    fn test_add_generator_invalid_lang() {
        let gen = TestHelper::new()
            .add_file("gen/gen.lolnope")
            .cases_gen(":GEN gen gen/gen.lolnope");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("unknown language");
    }

    /**********************
     * : VAL
     *********************/

    #[test]
    fn test_add_validator() {
        let gen = TestHelper::new()
            .add_file("gen/validator.py")
            .cases_gen(":VAL val gen/validator.py")
            .unwrap();
        assert!(gen.validators.contains_key("val"));
    }

    #[test]
    fn test_add_validator_with_args() {
        let gen = TestHelper::new()
            .add_file("gen/validator.py")
            .cases_gen(":VAL val gen/validator.py $INPUT $ST_NUM")
            .unwrap();
        assert!(gen.validators.contains_key("val"));
        assert_eq!(gen.validators["val"].args, vec!["$INPUT", "$ST_NUM"]);
    }

    #[test]
    fn test_add_validator_auto_default() {
        let gen = TestHelper::new()
            .add_file("gen/validator.py")
            .cases_gen(":VAL val gen/validator.py")
            .unwrap();
        assert_eq!(gen.default_validator, Some("val".into()));
    }

    #[test]
    fn test_add_validator_default() {
        let gen = TestHelper::new()
            .add_file("gen/validator.py")
            .add_file("gen/default.py")
            .cases_gen(":VAL val gen/validator.py\n:VAL default gen/default.py")
            .unwrap();
        assert_eq!(gen.default_validator, Some("default".into()));
    }

    #[test]
    fn test_set_current_validator() {
        let gen = TestHelper::new()
            .add_file("gen/validator.py")
            .cases_gen(
                ":VAL val gen/validator.py\n:VAL val2 gen/validator.py\n:SUBTASK 42\n:VAL val2",
            )
            .unwrap();
        assert_eq!(gen.default_validator, Some("val".into()));
        assert_eq!(gen.current_validator, Some("val2".into()));
    }

    #[test]
    fn test_set_current_validator_outside_subtask() {
        let gen = TestHelper::new()
            .add_file("gen/validator.py")
            .cases_gen(":VAL val gen/validator.py\n:VAL val");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("outside a subtask");
    }

    #[test]
    fn test_set_current_validator_unknown() {
        let gen = TestHelper::new()
            .add_file("gen/validator.py")
            .cases_gen(":VAL val gen/validator.py\n:SUBTASK 42\n:VAL lolnope");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("unknown validator");
    }

    #[test]
    fn test_add_validator_duplicate() {
        let gen = TestHelper::new()
            .add_file("gen/validator.py")
            .cases_gen(":VAL val gen/validator.py\n:VAL val gen/validator.py");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("Duplicate validator");
    }

    #[test]
    fn test_add_validator_missing_file() {
        let gen = TestHelper::new().cases_gen(":VAL val gen/validator.py");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("does not exists");
    }

    #[test]
    fn test_add_validator_invalid_lang() {
        let gen = TestHelper::new()
            .add_file("gen/gen.lolnope")
            .cases_gen(":VAL val gen/gen.lolnope");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("unknown language");
    }

    /**********************
     * : CONSTRAINT
     *********************/

    #[test]
    fn test_add_constraint_less() {
        let gen = TestHelper::new()
            .cases_gen(":CONSTRAINT 1 < $N = $K <= $M")
            .unwrap();
        assert_eq!(gen.constraints.len(), 1);
        let constr = &gen.constraints[0];
        assert_eq!(
            constr.operands,
            vec![
                ConstraintOperand::Constant(1),
                ConstraintOperand::Variable("N".into()),
                ConstraintOperand::Variable("K".into()),
                ConstraintOperand::Variable("M".into())
            ]
        );
        assert_eq!(
            constr.operators,
            vec![
                ConstraintOperator::Less,
                ConstraintOperator::Equal,
                ConstraintOperator::LessEqual
            ]
        );
    }

    #[test]
    fn test_add_constraint_greater() {
        let gen = TestHelper::new()
            .cases_gen(":CONSTRAINT $K = 1 > $N >= $M")
            .unwrap();
        assert_eq!(gen.constraints.len(), 1);
        let constr = &gen.constraints[0];
        assert_eq!(
            constr.operands,
            vec![
                ConstraintOperand::Variable("K".into()),
                ConstraintOperand::Constant(1),
                ConstraintOperand::Variable("N".into()),
                ConstraintOperand::Variable("M".into())
            ]
        );
        assert_eq!(
            constr.operators,
            vec![
                ConstraintOperator::Equal,
                ConstraintOperator::Greater,
                ConstraintOperator::GreaterEqual
            ]
        );
    }

    #[test]
    fn test_add_constraint_equal() {
        let gen = TestHelper::new()
            .cases_gen(":CONSTRAINT $K = $N = $M")
            .unwrap();
        assert_eq!(gen.constraints.len(), 1);
        let constr = &gen.constraints[0];
        assert_eq!(
            constr.operands,
            vec![
                ConstraintOperand::Variable("K".into()),
                ConstraintOperand::Variable("N".into()),
                ConstraintOperand::Variable("M".into())
            ]
        );
        assert_eq!(
            constr.operators,
            vec![ConstraintOperator::Equal, ConstraintOperator::Equal,]
        );
    }

    #[test]
    fn test_add_constraint_mixed_directions() {
        let gen = TestHelper::new().cases_gen(":CONSTRAINT $K < $N > $M");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string())
            .contains("inequality direction must be the same");
    }

    #[test]
    fn test_add_constraint_floats() {
        let gen = TestHelper::new().cases_gen(":CONSTRAINT $N < 10.2");
        assert!(gen.is_err());
    }

    #[test]
    fn test_add_constraint_invalid_integer() {
        let gen = TestHelper::new().cases_gen(":CONSTRAINT $N < 100000000000000000000000000000000");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("Invalid integer constant");
    }

    /**********************
     * : SUBTASK
     *********************/

    #[test]
    fn test_add_subtask() {
        let gen = TestHelper::new().cases_gen(":SUBTASK 42").unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.result.len(), 1);
        let subtask = &gen.result[0];
        if let TaskInputEntry::Subtask(subtask) = subtask {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.description, None);
            assert_abs_diff_eq!(subtask.max_score, 42.0);
        } else {
            panic!("Expecting a subtask, got: {:?}", subtask);
        }
    }

    #[test]
    fn test_add_subtask_description() {
        let gen = TestHelper::new()
            .cases_gen(":SUBTASK 42 the description")
            .unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.result.len(), 1);
        let subtask = &gen.result[0];
        if let TaskInputEntry::Subtask(subtask) = subtask {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.description, Some("the description".into()));
            assert_abs_diff_eq!(subtask.max_score, 42.0);
        } else {
            panic!("Expecting a subtask, got: {:?}", subtask);
        }
    }

    #[test]
    fn test_add_subtask_float_score() {
        let gen = TestHelper::new()
            .cases_gen(":SUBTASK 42.42 the description")
            .unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.result.len(), 1);
        let subtask = &gen.result[0];
        if let TaskInputEntry::Subtask(subtask) = subtask {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.description, Some("the description".into()));
            assert_abs_diff_eq!(subtask.max_score, 42.42);
        } else {
            panic!("Expecting a subtask, got: {:?}", subtask);
        }
    }

    /**********************
     * : COPY
     *********************/

    #[test]
    fn test_add_copy() {
        let gen = TestHelper::new()
            .add_file("example.in")
            .cases_gen(":SUBTASK 42\n:COPY example.in")
            .unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.testcase_id, 1);
        assert_eq!(gen.result.len(), 2);
        let testcase = &gen.result[1];
        if let TaskInputEntry::Testcase(testcase) = testcase {
            assert_eq!(testcase.id, 0);
            if let InputGenerator::StaticFile(_) = testcase.input_generator {
            } else {
                panic!(
                    "Expecting a static file, got: {:?}",
                    testcase.input_generator
                );
            }
            if let InputValidator::AssumeValid = testcase.input_validator {
            } else {
                panic!(
                    "Expecting an AssumeValid but got: {:?}",
                    testcase.input_validator
                );
            }
        } else {
            panic!("Expecting a testcase, got: {:?}", testcase);
        }
    }

    #[test]
    fn test_add_copy_missing_file() {
        let gen = TestHelper::new().cases_gen(":SUBTASK 42\n:COPY example.in");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("file not found");
    }

    #[test]
    fn test_add_copy_no_subtask() {
        let gen = TestHelper::new().cases_gen(":COPY example.in");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("outside a subtask");
    }

    /**********************
     * : RUN
     *********************/

    #[test]
    fn test_add_run() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py\n:SUBTASK 42\n:RUN gen 1 2 3")
            .unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.testcase_id, 1);
        assert_eq!(gen.result.len(), 2);
        let testcase = &gen.result[1];
        if let TaskInputEntry::Testcase(testcase) = testcase {
            assert_eq!(testcase.id, 0);
            if let InputGenerator::Custom(_, args) = &testcase.input_generator {
                assert_eq!(args, &vec!["1", "2", "3"]);
            } else {
                panic!(
                    "Expecting a custom generator, got: {:?}",
                    testcase.input_generator
                );
            }
            if let InputValidator::AssumeValid = testcase.input_validator {
            } else {
                panic!(
                    "Expecting an AssumeValid but got: {:?}",
                    testcase.input_validator
                );
            }
        } else {
            panic!("Expecting a testcase, got: {:?}", testcase);
        }
    }

    #[test]
    fn test_add_run_with_val() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .add_file("gen/val.py")
            .cases_gen(
                ":GEN gen gen/generator.py\n:VAL default gen/val.py\n:SUBTASK 42\n:RUN gen 4 5 6",
            )
            .unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.testcase_id, 1);
        assert_eq!(gen.result.len(), 2);
        let testcase = &gen.result[1];
        if let TaskInputEntry::Testcase(testcase) = testcase {
            assert_eq!(testcase.id, 0);
            if let InputGenerator::Custom(_, args) = &testcase.input_generator {
                assert_eq!(args, &vec!["4", "5", "6"]);
            } else {
                panic!(
                    "Expecting a custom generator, got: {:?}",
                    testcase.input_generator
                );
            }
            if let InputValidator::Custom(_, args) = &testcase.input_validator {
                assert_eq!(args.len(), 2);
                assert_eq!(args[1], "0");
            } else {
                panic!(
                    "Expecting an AssumeValid but got: {:?}",
                    testcase.input_validator
                );
            }
        } else {
            panic!("Expecting a testcase, got: {:?}", testcase);
        }
    }

    #[test]
    fn test_add_run_with_spaces() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py\n:SUBTASK 42\n:RUN gen '1 2' \"3 4\" '\"5 6'")
            .unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.testcase_id, 1);
        assert_eq!(gen.result.len(), 2);
        let testcase = &gen.result[1];
        if let TaskInputEntry::Testcase(testcase) = testcase {
            assert_eq!(testcase.id, 0);
            if let InputGenerator::Custom(_, args) = &testcase.input_generator {
                assert_eq!(args, &vec!["1 2", "3 4", "\"5 6"]);
            } else {
                panic!(
                    "Expecting a custom generator, got: {:?}",
                    testcase.input_generator
                );
            }
        } else {
            panic!("Expecting a testcase, got: {:?}", testcase);
        }
    }

    #[test]
    fn test_add_run_corrupted_command() {
        let gen = TestHelper::new()
            .add_file("gen/gen.py")
            .cases_gen(":GEN foo gen/gen.py\n:SUBTASK 42\n:RUN foo good ol' quotes");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("Invalid command arguments");
    }

    #[test]
    fn test_add_run_missing_gen() {
        let gen = TestHelper::new().cases_gen(":SUBTASK 42\n:RUN foo 42 42");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("Generator 'foo' not declared");
    }

    #[test]
    fn test_add_run_no_subtask() {
        let gen = TestHelper::new()
            .add_file("gen/gen.py")
            .cases_gen(":GEN default gen/gen.py\n:RUN default 42 42");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("outside a subtask");
    }

    /**********************
     * testcase
     *********************/
    #[test]
    fn test_testcase_default_gen() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN default gen/generator.py\n:SUBTASK 42\n1 2 3")
            .unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.testcase_id, 1);
        assert_eq!(gen.result.len(), 2);
        let testcase = &gen.result[1];
        if let TaskInputEntry::Testcase(testcase) = testcase {
            assert_eq!(testcase.id, 0);
            if let InputGenerator::Custom(source, args) = &testcase.input_generator {
                assert_eq!(source.name(), "generator.py");
                assert_eq!(args, &vec!["1", "2", "3"]);
            } else {
                panic!(
                    "Expecting a custom generator, got: {:?}",
                    testcase.input_generator
                );
            }
        } else {
            panic!("Expecting a testcase, got: {:?}", testcase);
        }
    }

    #[test]
    fn test_testcase_subtask_gen() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .add_file("gen/other.py")
            .cases_gen(":GEN default gen/generator.py\n:GEN other gen/other.py\n:SUBTASK 42\n:GEN other\n1 2 3")
            .unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.testcase_id, 1);
        assert_eq!(gen.result.len(), 2);
        let testcase = &gen.result[1];
        if let TaskInputEntry::Testcase(testcase) = testcase {
            assert_eq!(testcase.id, 0);
            if let InputGenerator::Custom(source, args) = &testcase.input_generator {
                assert_eq!(source.name(), "other.py");
                assert_eq!(args, &vec!["1", "2", "3"]);
            } else {
                panic!(
                    "Expecting a custom generator, got: {:?}",
                    testcase.input_generator
                );
            }
        } else {
            panic!("Expecting a testcase, got: {:?}", testcase);
        }
    }

    #[test]
    fn test_testcase_subtask_gen_outside() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .add_file("gen/other.py")
            .cases_gen(":GEN default gen/generator.py\n:GEN other gen/other.py\n:SUBTASK 42\n:GEN other\n:SUBTASK 20\n1 2 3")
            .unwrap();
        assert_eq!(gen.subtask_id, 2);
        assert_eq!(gen.testcase_id, 1);
        assert_eq!(gen.result.len(), 3);
        let testcase = &gen.result[2];
        if let TaskInputEntry::Testcase(testcase) = testcase {
            assert_eq!(testcase.id, 0);
            if let InputGenerator::Custom(source, args) = &testcase.input_generator {
                assert_eq!(source.name(), "generator.py");
                assert_eq!(args, &vec!["1", "2", "3"]);
            } else {
                panic!(
                    "Expecting a custom generator, got: {:?}",
                    testcase.input_generator
                );
            }
        } else {
            panic!("Expecting a testcase, got: {:?}", testcase);
        }
    }

    #[test]
    fn test_testcase_spaces_in_command() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN default gen/generator.py\n:SUBTASK 42\n'1 2' \"3 4\" '\"5 6'")
            .unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.testcase_id, 1);
        assert_eq!(gen.result.len(), 2);
        let testcase = &gen.result[1];
        if let TaskInputEntry::Testcase(testcase) = testcase {
            assert_eq!(testcase.id, 0);
            if let InputGenerator::Custom(source, args) = &testcase.input_generator {
                assert_eq!(source.name(), "generator.py");
                assert_eq!(args, &vec!["1 2", "3 4", "\"5 6"]);
            } else {
                panic!(
                    "Expecting a custom generator, got: {:?}",
                    testcase.input_generator
                );
            }
        } else {
            panic!("Expecting a testcase, got: {:?}", testcase);
        }
    }

    #[test]
    fn test_testcase_corrupted_command() {
        let gen = TestHelper::new()
            .add_file("gen/gen.py")
            .cases_gen(":GEN gen gen/gen.py\n:SUBTASK 42\ngood ol' quotes");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("Invalid command arguments");
    }

    #[test]
    fn test_testcase_missing_gen() {
        let gen = TestHelper::new().cases_gen(":SUBTASK 42\n42 42");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("no default generator set");
    }

    #[test]
    fn test_testcase_no_subtask() {
        let gen = TestHelper::new()
            .add_file("gen/gen.py")
            .cases_gen(":GEN default gen/gen.py\n42 42");
        assert!(gen.is_err());
        assert_that!(gen.unwrap_err().to_string()).contains("outside a subtask");
    }
}