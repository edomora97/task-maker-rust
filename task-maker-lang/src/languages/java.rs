use std::path::Path;

use task_maker_dag::*;

use crate::language::{
    CompilationSettings, CompiledLanguageBuilder, Language, SimpleCompiledLanguageBuilder,
};

/// The Java language.
#[derive(Debug)]
pub struct LanguageJava;

impl LanguageJava {
    /// Make a new LanguageJava
    pub fn new() -> LanguageJava {
        LanguageJava
    }
}

impl Language for LanguageJava {
    fn name(&self) -> &'static str {
        "Java"
    }

    fn extensions(&self) -> Vec<&'static str> {
        vec!["java"]
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
            ExecutionCommand::system("sh"),
        );
        let binary_name = metadata.binary_name.clone();
        let main_class = source
            .file_stem()
            .expect("Invalid file name")
            .to_string_lossy()
            .to_string();

        // binary_name and main_class are interpolated into a shell command (needed for the
        // `*.java`/`*.class` globs); quote them so a source filename with spaces or shell
        // metacharacters can't break or inject into the compilation command.
        let binary_name = shell_quote(&binary_name);
        let main_class = shell_quote(&main_class);
        metadata.add_arg("-c").add_arg(format!(
            "javac -encoding UTF-8 -d . *.java && jar cfe {binary_name} {main_class} *.class"
        ));

        metadata.grader_only();

        Some(Box::new(metadata))
    }

    fn runtime_command(&self, _path: &Path, _write_to: Option<&Path>) -> ExecutionCommand {
        ExecutionCommand::system("java")
    }

    fn runtime_args(
        &self,
        path: &Path,
        write_to: Option<&Path>,
        mut args: Vec<String>,
    ) -> Vec<String> {
        let mut new_args = vec![
            "-Xmx512M".to_string(),
            "-Xss64M".to_string(),
            "-XX:+UseSerialGC".to_string(),
            "-Dfile.encoding=UTF-8".to_string(),
            "-jar".to_string(),
            self.executable_name(path, write_to)
                .to_string_lossy()
                .to_string(),
        ];
        new_args.append(&mut args);
        new_args
    }

    fn custom_limits(&self, limits: &mut ExecutionLimits) {
        limits
            .mount_proc(true)
            .allow_multiprocess()
            .add_extra_readable_dir("/etc");
    }
}

/// Quotes a string for safe, literal inclusion inside single quotes in a POSIX shell command.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_quote_neutralizes_metacharacters() {
        assert_eq!(shell_quote("solution"), "'solution'");
        assert_eq!(
            shell_quote("a$(touch /tmp/pwned).java"),
            r"'a$(touch /tmp/pwned).java'"
        );
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}
