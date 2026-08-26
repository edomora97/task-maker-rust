use std::path::Path;

use task_maker_dag::*;

use crate::language::{
    CompilationSettings, CompiledLanguageBuilder, Language, SimpleCompiledLanguageBuilder,
};

/// The C# language.
#[derive(Debug)]
pub struct LanguageCSharp;

impl LanguageCSharp {
    /// Make a new LanguageCSharp
    pub fn new() -> LanguageCSharp {
        LanguageCSharp
    }
}

impl Language for LanguageCSharp {
    fn name(&self) -> &'static str {
        "C#"
    }

    fn extensions(&self) -> Vec<&'static str> {
        vec!["cs"]
    }

    fn need_compilation(&self) -> bool {
        true
    }

    fn inline_comment_prefix(&self) -> Option<&'static str> {
        Some("//")
    }

    fn compilation_builder(
        &self,
        source: &Path,
        settings: CompilationSettings,
    ) -> Option<Box<dyn CompiledLanguageBuilder + '_>> {
        let mut metadata = SimpleCompiledLanguageBuilder::new(
            self,
            source,
            settings,
            ExecutionCommand::system("csc"),
        );
        let binary_name = metadata.binary_name.clone();
        metadata
            .add_arg("-define:EVAL")
            .add_arg("-optimize+")
            .add_arg(format!("-out:{binary_name}"));

        metadata.callback(move |comp| {
            comp.limits_mut().add_extra_readable_dir("/etc/mono");
        });

        Some(Box::new(metadata))
    }

    fn runtime_command(&self, _path: &Path, _write_to: Option<&Path>) -> ExecutionCommand {
        ExecutionCommand::system("mono")
    }

    fn runtime_args(
        &self,
        path: &Path,
        write_to: Option<&Path>,
        mut args: Vec<String>,
    ) -> Vec<String> {
        // will run for example: mono program.exe args...
        args.insert(
            0,
            self.executable_name(path, write_to)
                .to_string_lossy()
                .to_string(),
        );
        args
    }

    fn custom_limits(&self, limits: &mut ExecutionLimits) {
        limits
            .add_extra_readable_dir("/etc/mono")
            .mount_proc(true)
            .allow_multiprocess();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_args_executable_comes_first() {
        // mono requires the assembly to load to be the first non-option argument;
        // regression test for it being appended after the program's own args instead.
        let lang = LanguageCSharp::new();
        let args = lang.runtime_args(
            Path::new("solution.cs"),
            None,
            vec!["input".into(), "output".into()],
        );
        assert_eq!(args, vec!["solution", "input", "output"]);
    }
}
