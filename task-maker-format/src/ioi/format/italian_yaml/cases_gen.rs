use std::collections::HashMap;
use std::fmt::Debug;
use std::fmt::Formatter;
use std::fmt::Write;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use anyhow::ensure;
use anyhow::{anyhow, bail, Context, Error};
use pest::Parser;

use task_maker_diagnostics::CodeSpan;

use crate::ioi::format::italian_yaml::TaskInputEntry;
use crate::ioi::italian_yaml::cleanup_subtask_name;
use crate::ioi::{
    InputGenerator, InputValidator, OutputGenerator, SubtaskId, SubtaskInfo, TestcaseId,
    TestcaseInfo, TM_VALIDATION_FILE_NAME,
};
use crate::SourceFile;

/// String placed in the auto-generated gen/GEN marking it as safely deletable.
pub(crate) const TM_ALLOW_DELETE_COOKIE: &str = "tm-allow-delete";

/// This module exists because of a `pest`'s bug: <https://github.com/pest-parser/pest/issues/326>
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
pub(crate) struct CasesGen<O>
where
    O: Fn(TestcaseId) -> OutputGenerator,
{
    /// The base directory of the task.
    task_dir: PathBuf,
    /// The function to call for getting the `OutputGenerator` for a given testcase.
    #[derivative(Debug = "ignore")]
    get_output_gen: O,
    /// The path to the cases.gen file.
    file_path: PathBuf,
    /// The content of the cases.gen file.
    file_content: String,
    /// The resulting `TaskInputEntry` that will be produced after the parsing of the `cases.gen`
    /// file.
    result: Vec<TaskInputEntry>,
    /// The list of constraints found in the file.
    constraints: Vec<Constraint>,
    /// The list of additional constraints for the current subtask.
    subtask_constraints: Vec<Vec<Constraint>>,
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
    /// The name of the last subtask added, if any.
    subtask_name: Option<String>,
    /// The identifier of the next testcase to process.
    testcase_id: TestcaseId,
    /// The list of dependencies of each subtask.
    st_deps: Vec<Vec<String>>,
    /// The table of subtask names to subtask ids.
    stname_table: HashMap<String, SubtaskId>,
}

impl<OutGen> CasesGen<OutGen>
where
    OutGen: Fn(TestcaseId) -> OutputGenerator,
{
    /// Parse the `cases.gen` file pointed at the specified path.
    pub(crate) fn new<P: AsRef<Path>>(
        path: P,
        output_gen: OutGen,
    ) -> Result<CasesGen<OutGen>, Error> {
        let path = path.as_ref();
        let task_dir = path
            .parent()
            .context("Invalid gen/cases.gen path")?
            .parent()
            .context("Invalid gen/cases.gen path")?;
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let mut file = parser::CasesGenParser::parse(parser::Rule::file, &content)
            .context("Cannot parse cases.gen")?;
        let file = file.next().ok_or_else(|| anyhow!("Corrupted parser"))?; // extract the real file

        let mut cases = CasesGen {
            task_dir: task_dir.into(),
            get_output_gen: output_gen,
            file_path: path.into(),
            file_content: content.clone(),
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
            subtask_name: None,
            testcase_id: 0,
            st_deps: Vec::new(),
            stname_table: HashMap::new(),
        };

        for line in file.into_inner() {
            match line.as_rule() {
                parser::Rule::line => {
                    let line = line
                        .into_inner()
                        .next()
                        .ok_or_else(|| anyhow!("Corrupted parser"))?;
                    match line.as_rule() {
                        parser::Rule::command => {
                            let command = line
                                .into_inner()
                                .next()
                                .ok_or_else(|| anyhow!("Corrupted parser"))?;
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

        cases
            .resolve_dependencies()
            .context("Failed to resolve subtask dependencies")?;

        Ok(cases)
    }

    /// Return the list of `TaskInputEntry` from the `cases.gen` file.
    pub(crate) fn get_task_entries(&self) -> Vec<TaskInputEntry> {
        self.result.clone()
    }

    /// Write an auto-generated version of the gen/GEN file inside the task directory.
    pub(crate) fn write_gen_gen(&self) -> Result<(), Error> {
        let dest = self.task_dir.join("gen/GEN");
        if dest.exists() && !is_gen_gen_deletable(&dest)? {
            warn!(
                "The gen/GEN file does not contain {}. Won't overwrite",
                TM_ALLOW_DELETE_COOKIE
            );
            return Ok(());
        }
        let mut gen = "# Generated by task-maker. Do not edit!\n".to_string();
        let _ = writeln!(gen, "# {}", TM_ALLOW_DELETE_COOKIE);
        gen += "# Removing or changing the line above will prevent task-maker from touching this file again.\n\n";
        if !self.constraints.is_empty() {
            gen += "# Constraints:\n";
            for constr in &self.constraints {
                let _ = writeln!(gen, "# {:?}", constr);
            }
        }
        for entry in &self.result {
            match entry {
                TaskInputEntry::Subtask(subtask) => {
                    let _ = writeln!(gen, "\n# Subtask {}", subtask.id);
                    let _ = writeln!(gen, "#ST: {}", subtask.max_score);
                    if let Some(name) = &subtask.name {
                        let _ = writeln!(gen, "#STNAME: {}", name);
                    }
                    if let Some(constraints) = self.subtask_constraints.get(subtask.id as usize) {
                        for constr in constraints {
                            let _ = writeln!(gen, "# {:?}", constr);
                        }
                    }
                }
                TaskInputEntry::Testcase(testcase) => {
                    let _ = writeln!(gen, "# Testcase {}", testcase.id);
                    match &testcase.input_generator {
                        InputGenerator::StaticFile(path) => {
                            if let Ok(path) = path.strip_prefix(&self.task_dir) {
                                let _ = writeln!(gen, "#COPY: {}", path.display());
                            } else {
                                let _ = writeln!(gen, "#COPY: {}", path.display());
                            }
                        }
                        InputGenerator::Custom(source, args) => {
                            let _ = writeln!(gen, "{} {}", source.name(), args.join(" "));
                        }
                    }
                }
            }
        }
        std::fs::write(self.task_dir.join("gen/GEN"), gen).context("Failed to write gen/GEN")?;
        Ok(())
    }

    /// Resolve the dependencies names to the subtask ids.
    fn resolve_dependencies(&mut self) -> Result<(), Error> {
        for entry in &mut self.result {
            let TaskInputEntry::Subtask(SubtaskInfo {
                id, dependencies, ..
            }) = entry
            else {
                continue;
            };
            *dependencies = self.st_deps[*id as usize]
                .iter()
                .map(|dep| {
                    self.stname_table
                        .get(dep)
                        .copied()
                        .ok_or_else(|| anyhow!("Unknown subtask name: {dep}"))
                })
                .collect::<Result<_, _>>()?;
        }
        Ok(())
    }

    /// Parse a line with a command: one of the `:` prefixed actions.
    fn parse_command(&mut self, line: Pair) -> Result<(), Error> {
        match line.as_rule() {
            parser::Rule::GEN => {
                self.parse_gen(line)
                    .context("Failed to parse GEN command")?;
            }
            parser::Rule::VAL => {
                self.parse_val(line)
                    .context("Failed to parse VAL command")?;
            }
            parser::Rule::CONSTRAINT => {
                self.parse_constraint(line)
                    .context("Failed to parse CONSTRAINT command")?;
            }
            parser::Rule::SUBTASK => {
                self.parse_subtask(line)
                    .context("Failed to parse SUBTASK command")?;
            }
            parser::Rule::DEP => {
                self.parse_dep(line)
                    .context("Failed to parse DEP command")?;
            }
            parser::Rule::COPY => {
                self.parse_copy(line)
                    .context("Failed to parse COPY command")?;
            }
            parser::Rule::RUN => {
                self.parse_run(line)
                    .context("Failed to parse RUN command")?;
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
        let current_generator = if let Some(gen) = current_generator {
            gen
        } else {
            bail!("Cannot generate testcase: no default generator set");
        };
        let args = shell_words::split(line)
            .with_context(|| format!("Invalid command arguments for testcase '{}'", line))?;
        let generator = &self
            .generators
            .get(&current_generator)
            .context("invalid current generator")?;
        let variables = self.get_variables(&generator.args, &args);
        for constr in self
            .constraints
            .iter()
            .chain(self.subtask_constraints.last().unwrap())
        {
            if let Some(false) = constr.is_valid(&variables) {
                let mut error = format!(
                    "Testcase '{}' violates constraint {:?}\nWith:",
                    line, constr
                );
                for (var, val) in &variables {
                    let _ = write!(error, "\n  ${} = {}", var, val);
                }
                bail!("{}", error)
            }
        }
        let generator = InputGenerator::Custom(generator.source.clone(), args);
        self.result.push(TaskInputEntry::Testcase(TestcaseInfo::new(
            self.testcase_id,
            generator,
            self.get_validator(&variables)
                .context("Cannot get testcase validator")?,
            (self.get_output_gen)(self.testcase_id),
        )));
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
                format!("The {} named {}", kind, name),
                None,
                Some(
                    task_dir
                        .join("bin")
                        .join(path.file_name().context("invalid file name")?),
                ),
            )
            .map(Arc::new)
            .ok_or_else(|| anyhow!("Cannot use {} '{}': unknown language", kind, path.display()))?;
            let args = shell_words::split(line[2].as_str())
                .with_context(|| format!("Invalid arguments of '{}'", name))?;
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
        CasesGen::<OutGen>::process_gen_val(
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
        CasesGen::<OutGen>::process_gen_val(
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
                        i64::from_str(item.as_str()).with_context(|| {
                            format!(
                                "Invalid integer constant '{}' in constraint '{}'",
                                item.as_str(),
                                line_str,
                            )
                        })?,
                    ));
                }
                parser::Rule::variable => {
                    constraint.operands.push(ConstraintOperand::Variable(
                        item.as_str()
                            .strip_prefix('$')
                            .expect("Invalid parser")
                            .into(),
                    ));
                }
                parser::Rule::comp_operator => {
                    let operator = ConstraintOperator::from_str(item.as_str())
                        .context("Failed to parse contraint operator")?;
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
            self.subtask_constraints
                .last_mut()
                .unwrap()
                .push(constraint);
        }
        Ok(())
    }

    /// Parse a `:SUBTASK` command.
    fn parse_subtask(&mut self, line: Pair) -> Result<(), Error> {
        let span = line.as_span();
        let line: Vec<_> = line.into_inner().collect();
        self.current_generator = self.default_generator.clone();
        self.current_validator = self.default_validator.clone();
        self.subtask_constraints.push(vec![]);
        let score = line[0].as_str();
        let score = f64::from_str(score).with_context(|| {
            format!(
                "Invalid subtask score for subtask {}: {}",
                self.subtask_id, score
            )
        })?;
        let (description, name) = if line.len() >= 2 {
            let desc = line[1].as_str().to_string();
            let name = desc
                // Remove whitespaces for retrocompatibility with descriptions
                .chars()
                .filter(|&c| c != ' ' && c != '\t')
                .collect::<String>();
            let name = cleanup_subtask_name(&name)?;
            let old_id = self.stname_table.insert(name.clone(), self.subtask_id);
            ensure!(old_id.is_none(), "Subtask name {:?} already used", name);

            (Some(desc), Some(name))
        } else {
            (None, None)
        };
        self.subtask_name = name.clone();
        self.result.push(TaskInputEntry::Subtask(
            #[allow(deprecated)]
            SubtaskInfo {
                id: self.subtask_id,
                name,
                description,
                max_score: score,
                span: CodeSpan::from_str(
                    &self.file_path,
                    &self.file_content,
                    span.start(),
                    span.end() - span.start(),
                )
                .ok(),
                is_default: false,
                ..Default::default()
            },
        ));
        self.st_deps.push(Vec::new());
        self.subtask_id += 1;
        Ok(())
    }

    /// Parse a `:DEP` command.
    fn parse_dep(&mut self, line: Pair) -> Result<(), Error> {
        if let Some(TaskInputEntry::Subtask(_)) = self.result.last_mut() {
            for dependency in line.into_inner() {
                let dependency_str = dependency.as_str();
                self.st_deps.last_mut().unwrap().push(
                    cleanup_subtask_name(dependency_str)
                        .with_context(|| format!("Invalid subtask name: {}", dependency_str))?,
                );
            }
        } else {
            bail!("#STDEP: rule must immediately follow a #ST: in gen/GEN");
        }
        Ok(())
    }

    /// Parse a `:COPY` command.
    fn parse_copy(&mut self, line: Pair) -> Result<(), Error> {
        if self.subtask_id == 0 {
            bail!("Cannot add a COPY testcase outside a subtask");
        }
        let path = line
            .into_inner()
            .next()
            .context("corrupted parser")?
            .as_str();
        let path = self.task_dir.join(path);
        if !path.exists() {
            bail!(
                "Cannot copy testcase from '{}': file not found",
                path.display()
            );
        }
        self.result.push(TaskInputEntry::Testcase(TestcaseInfo::new(
            self.testcase_id,
            InputGenerator::StaticFile(path),
            self.get_validator(&self.get_auto_variables())
                .context("Cannot get testcase validator")?,
            (self.get_output_gen)(self.testcase_id),
        )));
        self.testcase_id += 1;
        Ok(())
    }

    /// Get the current validator for the next testcase.
    fn get_validator(&self, variables: &HashMap<String, String>) -> Result<InputValidator, Error> {
        match &self.current_validator {
            Some(val) => {
                let validator = &self.validators[val];
                let args = if validator.args.is_empty() {
                    vec![variables["INPUT"].clone(), variables["ST_NUM"].clone()]
                } else {
                    let mut args = Vec::new();
                    for arg in &validator.args {
                        // variables may (and should!) start with `$`, remove it before accessing
                        // the `variables` map.
                        let arg = if let Some(rest) = arg.strip_prefix('$') {
                            rest
                        } else {
                            arg.as_str()
                        };
                        if let Some(value) = variables.get(arg) {
                            args.push(value.clone());
                        } else {
                            bail!("Unknown variable in validator arguments: ${}", arg);
                        }
                    }
                    args
                };
                Ok(InputValidator::Custom(validator.source.clone(), args))
            }
            None => Ok(InputValidator::AssumeValid),
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

    /// Compute the list of all the variables accessible for the current testcase, including the
    /// automatic ones and the ones extracted from the command line arguments of the generator.
    fn get_variables(&self, definition: &[String], args: &[String]) -> HashMap<String, String> {
        let mut vars = self.get_auto_variables();
        for (var, val) in definition.iter().zip(args.iter()) {
            vars.insert(var.clone(), val.clone());
        }
        vars
    }

    /// Obtain the automatic variables for the current testcase.
    fn get_auto_variables(&self) -> HashMap<String, String> {
        let mut vars = HashMap::new();
        vars.insert("INPUT".to_string(), TM_VALIDATION_FILE_NAME.to_string());
        vars.insert("ST_NUM".to_string(), (self.subtask_id - 1).to_string());
        vars.insert("TC_NUM".to_string(), self.testcase_id.to_string());
        if let Some(name) = &self.subtask_name {
            vars.insert("ST_NAME".to_string(), name.clone());
        }
        vars
    }
}

/// Check if the gen/GEN file is deletable, i.e. it exists and it is autogenerated.
pub(crate) fn is_gen_gen_deletable(path: &Path) -> Result<bool, Error> {
    if !path.exists() {
        return Ok(false);
    }
    let mut buffer = vec![];
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Cannot open file {}", path.display()))?;
    file.read_to_end(&mut buffer)
        .context("Cannot read gen/GEN file")?;
    let content = String::from_utf8_lossy(&buffer);
    // if the gen/GEN is present and it does not contain the cookie, skip all the generation
    if !content.contains(TM_ALLOW_DELETE_COOKIE) {
        return Ok(false);
    }
    Ok(true)
}

impl ConstraintOperator {
    /// Apply the operator to the provided values and return the result of the comparison.
    fn is_valid(&self, lhs: i64, rhs: i64) -> bool {
        match self {
            ConstraintOperator::Less => lhs < rhs,
            ConstraintOperator::LessEqual => lhs <= rhs,
            ConstraintOperator::Equal => lhs == rhs,
            ConstraintOperator::Greater => lhs > rhs,
            ConstraintOperator::GreaterEqual => lhs >= rhs,
        }
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

impl ConstraintOperand {
    /// Return the value of this operand which is either the constant or the integer value of the
    /// variable contained in it. If the variable is not present, or it's not a valid integer `None`
    /// is returned.
    fn get_val(&self, vars: &HashMap<String, String>) -> Option<i64> {
        match self {
            ConstraintOperand::Constant(k) => Some(*k),
            ConstraintOperand::Variable(var) => {
                if let Some(val) = vars.get(var) {
                    if let Ok(val) = i64::from_str(val) {
                        Some(val)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
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

impl Constraint {
    /// Check if the variables verify this constraint, returning `Some(res)` if the check was
    /// successful (i.e. all the variables were present and valid). `None` is returned if this
    /// constraint cannot be fully verified.
    fn is_valid(&self, vars: &HashMap<String, String>) -> Option<bool> {
        let mut last = self.operands[0].get_val(vars)?;
        for (operator, operand) in self.operators.iter().zip(&self.operands[1..]) {
            let this = operand.get_val(vars)?;
            if !operator.is_valid(last, this) {
                return Some(false);
            }
            last = this;
        }
        Some(true)
    }
}

impl Debug for Constraint {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        let mut constraint = self.operands[0].to_string();
        for (op, var) in self.operators.iter().zip(self.operands[1..].iter()) {
            let _ = write!(constraint, " {} {}", op.to_string(), var.to_string());
        }
        write!(f, "{}", constraint)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use anyhow::Error;
    use speculoos::{assert_that, AssertionFailure, Spec};
    use tempfile::TempDir;

    use crate::ioi::format::italian_yaml::cases_gen::{
        CasesGen, ConstraintOperand, ConstraintOperator,
    };
    use crate::ioi::format::italian_yaml::TaskInputEntry;
    use crate::ioi::{
        InputGenerator, InputValidator, OutputGenerator, TestcaseId, TM_VALIDATION_FILE_NAME,
    };

    struct TestHelper(TempDir);

    trait ErrorAssertion {
        fn has_error(&mut self, message: &str);
    }

    impl<'s> ErrorAssertion for Spec<'s, Error> {
        fn has_error(&mut self, message: &str) {
            let error = format!("{:?}", self.subject);
            if !error.contains(message) {
                AssertionFailure::from_spec(self)
                    .with_actual(error)
                    .with_expected(format!("error containing <\"{}\">", message))
                    .fail();
            }
        }
    }

    impl TestHelper {
        fn new() -> TestHelper {
            TestHelper(TempDir::new().unwrap())
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
    fn test_gen_gen() {
        let helper = TestHelper::new();
        let gen = helper
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py\n:SUBTASK 42 lol\n12 34\n:SUBTASK 24\n21 21")
            .unwrap();
        gen.write_gen_gen().unwrap();
        let path = gen.task_dir.join("gen/GEN");
        let data = std::fs::read_to_string(path).unwrap();
        let res: Vec<_> = data
            .split('\n')
            .filter(|s| !s.is_empty() && !s.starts_with("# ") && !s.starts_with("#COPY"))
            .collect();
        assert_eq!(res.len(), 5);
        assert_eq!(res[0], "#ST: 42");
        assert_eq!(res[1], "#STNAME: lol");
        assert!(res[2].contains("12 34"));
        assert_eq!(res[3], "#ST: 24");
        assert!(res[4].contains("21 21"));
    }

    #[test]
    fn test_auto_variables() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py\n:SUBTASK 42 lol\n12 34")
            .unwrap();
        let vars = gen.get_auto_variables();
        assert_eq!(vars["INPUT"], TM_VALIDATION_FILE_NAME);
        assert_eq!(vars["ST_NUM"], "0");
        assert_eq!(vars["TC_NUM"], "1");
        assert_eq!(vars["ST_NAME"], "lol");
    }

    #[test]
    fn test_auto_variables_no_name() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py\n:SUBTASK 42 lol\n12 34\n: SUBTASK 43")
            .unwrap();
        let vars = gen.get_auto_variables();
        assert_eq!(vars["INPUT"], TM_VALIDATION_FILE_NAME);
        assert_eq!(vars["ST_NUM"], "1");
        assert_eq!(vars["TC_NUM"], "1");
        assert!(!vars.contains_key("ST_NAME"));
    }

    #[test]
    fn test_variables() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py N M seed\n:SUBTASK 42\n12 34")
            .unwrap();
        let args = vec!["1".into(), "2".into(), "1337".into()];
        let vars = gen.get_variables(&gen.generators["gen"].args, &args);
        assert_eq!(vars["INPUT"], TM_VALIDATION_FILE_NAME);
        assert_eq!(vars["ST_NUM"], "0");
        assert_eq!(vars["TC_NUM"], "1");
        assert_eq!(vars["N"], "1");
        assert_eq!(vars["M"], "2");
        assert_eq!(vars["seed"], "1337");
    }

    #[test]
    fn test_variables_extra_vars() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py N M seed\n:SUBTASK 42\n12 34")
            .unwrap();
        let args = vec!["1".into(), "2".into(), "1337".into(), "boh!".into()];
        let vars = gen.get_variables(&gen.generators["gen"].args, &args);
        assert_eq!(vars["INPUT"], TM_VALIDATION_FILE_NAME);
        assert_eq!(vars["ST_NUM"], "0");
        assert_eq!(vars["TC_NUM"], "1");
        assert_eq!(vars["N"], "1");
        assert_eq!(vars["M"], "2");
        assert_eq!(vars["seed"], "1337");
    }

    #[test]
    fn test_variables_fewer_vars() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py N M seed\n:SUBTASK 42\n12 34")
            .unwrap();
        let args = vec!["1".into()];
        let vars = gen.get_variables(&gen.generators["gen"].args, &args);
        assert_eq!(vars["INPUT"], TM_VALIDATION_FILE_NAME);
        assert_eq!(vars["ST_NUM"], "0");
        assert_eq!(vars["TC_NUM"], "1");
        assert_eq!(vars["N"], "1");
        assert!(!vars.contains_key("M"));
        assert!(!vars.contains_key("seed"));
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
        assert_that(&gen.unwrap_err()).has_error("outside a subtask");
    }

    #[test]
    fn test_set_current_generator_unknown() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py\n:SUBTASK 42\n:GEN lolnope");
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("unknown generator");
    }

    #[test]
    fn test_add_generator_duplicate() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .cases_gen(":GEN gen gen/generator.py\n:GEN gen gen/generator.py");
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("Duplicate generator");
    }

    #[test]
    fn test_add_generator_missing_file() {
        let gen = TestHelper::new().cases_gen(":GEN gen gen/generator.py");
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("does not exists");
    }

    #[test]
    fn test_add_generator_invalid_lang() {
        let gen = TestHelper::new()
            .add_file("gen/gen.lolnope")
            .cases_gen(":GEN gen gen/gen.lolnope");
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("unknown language");
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
        assert_that(&gen.unwrap_err()).has_error("outside a subtask");
    }

    #[test]
    fn test_set_current_validator_unknown() {
        let gen = TestHelper::new()
            .add_file("gen/validator.py")
            .cases_gen(":VAL val gen/validator.py\n:SUBTASK 42\n:VAL lolnope");
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("unknown validator");
    }

    #[test]
    fn test_add_validator_duplicate() {
        let gen = TestHelper::new()
            .add_file("gen/validator.py")
            .cases_gen(":VAL val gen/validator.py\n:VAL val gen/validator.py");
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("Duplicate validator");
    }

    #[test]
    fn test_add_validator_missing_file() {
        let gen = TestHelper::new().cases_gen(":VAL val gen/validator.py");
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("does not exists");
    }

    #[test]
    fn test_add_validator_invalid_lang() {
        let gen = TestHelper::new()
            .add_file("gen/gen.lolnope")
            .cases_gen(":VAL val gen/gen.lolnope");
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("unknown language");
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
        assert_that(&gen.unwrap_err()).has_error("inequality direction must be the same");
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
        assert_that(&gen.unwrap_err()).has_error("Invalid integer constant");
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
            assert_eq!(subtask.name, None);
            assert_abs_diff_eq!(subtask.max_score, 42.0);
        } else {
            panic!("Expecting a subtask, got: {:?}", subtask);
        }
    }

    #[test]
    fn test_add_subtask_name() {
        let gen = TestHelper::new().cases_gen(":SUBTASK 42 the-name").unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.result.len(), 1);
        let subtask = &gen.result[0];
        if let TaskInputEntry::Subtask(subtask) = subtask {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.name, Some("the-name".into()));
            assert_abs_diff_eq!(subtask.max_score, 42.0);
        } else {
            panic!("Expecting a subtask, got: {:?}", subtask);
        }
    }

    #[test]
    fn test_add_subtask_space_in_name() {
        let gen = TestHelper::new()
            .cases_gen(":SUBTASK 42.42 the name")
            .unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.result.len(), 1);
        let subtask = &gen.result[0];
        if let TaskInputEntry::Subtask(subtask) = subtask {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.name, Some("thename".into()));
            assert_abs_diff_eq!(subtask.max_score, 42.42);
        } else {
            panic!("Expecting a subtask, got: {:?}", subtask);
        }
    }

    #[test]
    fn test_add_subtask_float_score() {
        let gen = TestHelper::new()
            .cases_gen(":SUBTASK 42.42 the-name")
            .unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.result.len(), 1);
        let subtask = &gen.result[0];
        if let TaskInputEntry::Subtask(subtask) = subtask {
            assert_eq!(subtask.id, 0);
            assert_eq!(subtask.name, Some("the-name".into()));
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
        assert_that(&gen.unwrap_err()).has_error("file not found");
    }

    #[test]
    fn test_add_copy_no_subtask() {
        let gen = TestHelper::new().cases_gen(":COPY example.in");
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("outside a subtask");
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
        assert_that(&gen.unwrap_err()).has_error("Invalid command arguments");
    }

    #[test]
    fn test_add_run_missing_gen() {
        let gen = TestHelper::new().cases_gen(":SUBTASK 42\n:RUN foo 42 42");
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("Generator 'foo' not declared");
    }

    #[test]
    fn test_add_run_no_subtask() {
        let gen = TestHelper::new()
            .add_file("gen/gen.py")
            .cases_gen(":GEN default gen/gen.py\n:RUN default 42 42");
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("outside a subtask");
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
    fn test_testcase_validator_args_default() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .add_file("gen/val.py")
            .cases_gen(":GEN default gen/generator.py\n:VAL default gen/val.py\n:SUBTASK 42\n1 2 3")
            .unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.testcase_id, 1);
        assert_eq!(gen.result.len(), 2);
        let testcase = &gen.result[1];
        if let TaskInputEntry::Testcase(testcase) = testcase {
            assert_eq!(testcase.id, 0);
            if let InputValidator::Custom(source, args) = &testcase.input_validator {
                assert_eq!(source.name(), "val.py");
                assert_eq!(args, &vec![TM_VALIDATION_FILE_NAME, "0"]);
            } else {
                panic!(
                    "Expecting a custom validator, got: {:?}",
                    testcase.input_validator
                );
            }
        } else {
            panic!("Expecting a testcase, got: {:?}", testcase);
        }
    }

    #[test]
    fn test_testcase_validator_args_custom() {
        let gen = TestHelper::new()
            .add_file("gen/generator.py")
            .add_file("gen/val.py")
            .cases_gen(":GEN default gen/generator.py N M seed\n:VAL default gen/val.py $N $M $seed $INPUT $TC_NUM $ST_NUM\n:SUBTASK 42\n1 2 3")
            .unwrap();
        assert_eq!(gen.subtask_id, 1);
        assert_eq!(gen.testcase_id, 1);
        assert_eq!(gen.result.len(), 2);
        let testcase = &gen.result[1];
        if let TaskInputEntry::Testcase(testcase) = testcase {
            assert_eq!(testcase.id, 0);
            if let InputValidator::Custom(source, args) = &testcase.input_validator {
                assert_eq!(source.name(), "val.py");
                assert_eq!(
                    args,
                    &vec!["1", "2", "3", TM_VALIDATION_FILE_NAME, "0", "0"]
                );
            } else {
                panic!(
                    "Expecting a custom validator, got: {:?}",
                    testcase.input_validator
                );
            }
        } else {
            panic!("Expecting a testcase, got: {:?}", testcase);
        }
    }

    #[test]
    fn test_testcase_valid_constraints() {
        let gen = TestHelper::new().add_file("gen/generator.py").cases_gen(
            ":GEN default gen/generator.py N M\n:CONSTRAINT 1 <= $N < $M < 1000\n:SUBTASK 42\n1 2",
        );
        assert!(gen.is_ok());
    }

    #[test]
    fn test_testcase_invalid_constraints() {
        let gen = TestHelper::new().add_file("gen/generator.py").cases_gen(
            ":GEN default gen/generator.py N M\n:CONSTRAINT 1 <= $N < $M < 1000\n:SUBTASK 42\n1 1000",
        );
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("violates constraint");
    }

    #[test]
    fn test_testcase_uncheckable_constraints() {
        let gen = TestHelper::new().add_file("gen/generator.py").cases_gen(
            ":GEN default gen/generator.py N M\n:CONSTRAINT 1 <= $N < $M < 1000\n:SUBTASK 42\n10000",
        );
        assert!(gen.is_ok());
    }

    #[test]
    fn test_testcase_corrupted_command() {
        let gen = TestHelper::new()
            .add_file("gen/gen.py")
            .cases_gen(":GEN gen gen/gen.py\n:SUBTASK 42\ngood ol' quotes");
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("Invalid command arguments");
    }

    #[test]
    fn test_testcase_missing_gen() {
        let gen = TestHelper::new().cases_gen(":SUBTASK 42\n42 42");
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("no default generator set");
    }

    #[test]
    fn test_testcase_no_subtask() {
        let gen = TestHelper::new()
            .add_file("gen/gen.py")
            .cases_gen(":GEN default gen/gen.py\n42 42");
        assert!(gen.is_err());
        assert_that(&gen.unwrap_err()).has_error("outside a subtask");
    }
}
