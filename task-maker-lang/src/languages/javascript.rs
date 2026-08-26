use std::path::Path;

use task_maker_dag::*;

use crate::language::Language;

/// The JavaScript language.
#[derive(Debug)]
pub struct LanguageJS;

impl LanguageJS {
    /// Make a new LanguageJS
    pub fn new() -> LanguageJS {
        LanguageJS
    }
}

impl Language for LanguageJS {
    fn name(&self) -> &'static str {
        "JavaScript"
    }

    fn extensions(&self) -> Vec<&'static str> {
        vec!["js", "cjs", "mjs"]
    }

    fn need_compilation(&self) -> bool {
        false
    }

    fn inline_comment_prefix(&self) -> Option<&'static str> {
        Some("//")
    }

    fn runtime_command(&self, _path: &Path, _write_to: Option<&Path>) -> ExecutionCommand {
        ExecutionCommand::system("node")
    }

    fn runtime_args(
        &self,
        path: &Path,
        write_to: Option<&Path>,
        mut args: Vec<String>,
    ) -> Vec<String> {
        // will run for example: node script.js args...
        args.insert(
            0,
            self.executable_name(path, write_to)
                .to_string_lossy()
                .to_string(),
        );
        args
    }

    fn custom_limits(&self, limits: &mut ExecutionLimits) {
        limits.allow_multiprocess();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_args_executable_comes_first() {
        // node requires the script to load to be the first non-option argument;
        // regression test for it being appended after the program's own args instead.
        let lang = LanguageJS::new();
        let args = lang.runtime_args(
            Path::new("solution.js"),
            None,
            vec!["input".into(), "output".into()],
        );
        assert_eq!(args, vec!["solution", "input", "output"]);
    }
}
