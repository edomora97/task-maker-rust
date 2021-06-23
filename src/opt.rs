use std::path::PathBuf;

use structopt::StructOpt;

use itertools::Itertools;
use task_maker_format::get_sanity_check_names;
use task_maker_format::terry::Seed;
use task_maker_format::{EvaluationConfig, VALID_TAGS};

#[derive(StructOpt, Debug)]
#[structopt(
    name = "task-maker",
    setting = structopt::clap::AppSettings::ColoredHelp,
)]
pub struct Opt {
    /// Directory of the task to evaluate
    #[structopt(short = "t", long = "task-dir", default_value = "")]
    pub task_dir: PathBuf,

    /// Which UI to use, available UIS are: print, raw, curses, json.
    ///
    /// Note that the JSON api is not stable yet.
    #[structopt(long = "ui", default_value = "curses")]
    pub ui: task_maker_format::ui::UIType,

    /// Keep all the sandbox directories
    #[structopt(long = "keep-sandboxes")]
    pub keep_sandboxes: bool,

    /// Do not write any file inside the task directory
    #[structopt(long = "dry-run")]
    pub dry_run: bool,

    /// Print the evaluation DAG in DOT format
    #[structopt(long = "print-dag")]
    pub print_dag: bool,

    /// Disable the cache for this comma separated list of tags
    #[structopt(long = "no-cache", long_help = no_cache_long_help())]
    #[allow(clippy::option_option)]
    pub no_cache: Option<Option<String>>,

    /// Do not run in parallel time critical executions on the same machine
    #[structopt(long = "exclusive")]
    pub exclusive: bool,

    /// Give to the solution some extra time before being killed
    #[structopt(long = "extra-time")]
    pub extra_time: Option<f64>,

    /// Copy the executables to the bin/ folder
    #[structopt(long = "copy-exe")]
    pub copy_exe: bool,

    /// Copy the logs of some executions to the bin/logs/ folder
    #[structopt(long = "copy-logs")]
    pub copy_logs: bool,

    /// Execute only the solutions whose names start with the filter
    ///
    /// Note that just the file name is checked (e.g. sol.cpp is the same as sol/sol.cpp). Without
    /// specifying anything all the solutions are executed.
    pub filter: Vec<String>,

    /// Evaluate only the solution with the specified path
    ///
    /// The solution can reside anywhere in the filesystem.
    #[structopt(long = "solution", short = "-s")]
    pub solution: Vec<PathBuf>,

    /// Force this seed instead of a random one in Terry.
    #[structopt(long)]
    pub seed: Option<Seed>,

    /// Look at most for this number of parents for searching the task
    #[structopt(long = "max-depth", default_value = "3")]
    pub max_depth: u32,

    /// Clear the task directory and exit
    #[structopt(long = "clean")]
    pub clean: bool,

    /// Where to store the storage files, including the cache
    #[structopt(long = "store-dir")]
    pub store_dir: Option<PathBuf>,

    /// Maximum size of the storage directory, in MiB
    #[structopt(long = "max-cache", default_value = "3072")]
    pub max_cache: u64,

    /// When the storage is flushed, this is the new maximum size, in MiB.
    #[structopt(long = "min-cache", default_value = "2048")]
    pub min_cache: u64,

    /// The number of CPU cores to use. Ignored for workers.
    #[structopt(long = "num-cores")]
    pub num_cores: Option<usize>,

    /// Include the solutions in the booklet
    #[structopt(long = "booklet-solutions")]
    pub booklet_solutions: bool,

    /// Do not build the statement files and the booklets
    #[structopt(long = "no-statement")]
    pub no_statement: bool,

    /// Verbose mode (-v, -vv, -vvv, etc.). Note that it does not play well with curses ui.
    #[structopt(short, long, parse(from_occurrences))]
    pub verbose: u8,

    /// Run the evaluation on a remote server instead of locally
    #[structopt(long = "evaluate-on")]
    pub evaluate_on: Option<String>,

    /// List of sanity checks to skip.
    #[structopt(short = "-W", long_help = skip_sanity_checks_long_help())]
    pub skip_sanity_checks: Vec<String>,

    /// The name to use for the workers or for the client in remote executions
    ///
    /// This option only has no effect with `--server`.
    #[structopt(long)]
    pub name: Option<String>,

    /// Options for spawning a client or a server.
    #[structopt(subcommand)]
    pub remote: Option<Remote>,

    /// Show task information
    #[structopt(long)]
    pub task_info: bool,

    /// Wipe the internal storage of task-maker
    ///
    /// Warning: no other instances of task-maker should be running when this flag is provided.
    #[structopt(long)]
    pub dont_panic: bool,

    /// Run the sandbox instead of the normal task-maker.
    ///
    /// This option is left as undocumented as it's not part of the public API.
    #[structopt(long, hidden(true))]
    pub sandbox: bool,
}

/// Options for spawning a client or a server.
#[derive(StructOpt, Debug)]
pub enum Remote {
    /// Spawn a server instead of the normal task-maker
    #[structopt(name = "--server")]
    Server(ServerOptions),
    /// Spawn a worker instead of the normal task-maker
    #[structopt(name = "--worker")]
    Worker(WorkerOptions),
}

#[derive(StructOpt, Debug, Clone)]
pub struct ServerOptions {
    /// Address to bind the server on for listening for the clients
    #[structopt(default_value = "0.0.0.0:27182")]
    pub client_addr: String,
    /// Address to bind the server on for listening for the workers
    #[structopt(default_value = "0.0.0.0:27183")]
    pub worker_addr: String,
    /// Password for the connection of the clients
    #[structopt(long = "client-password")]
    pub client_password: Option<String>,
    /// Password for the connection of the workers
    #[structopt(long = "worker-password")]
    pub worker_password: Option<String>,
}

#[derive(StructOpt, Debug, Clone)]
pub struct WorkerOptions {
    /// Address to use to connect to a remote server
    pub server_addr: String,
    /// ID of the worker (to differentiate between multiple workers on the same machine).
    pub worker_id: Option<u32>,
}

/// Returns the long-help for the "skip sanity checks" option.
fn skip_sanity_checks_long_help() -> &'static str {
    lazy_static! {
        pub static ref DOC: String = format!(
            "List of sanity checks to skip.\n\nThe available checks are: {}.",
            get_sanity_check_names()
        );
    }
    &DOC
}

/// Returns the long-help for the --no-cache option.
fn no_cache_long_help() -> &'static str {
    lazy_static! {
        pub static ref DOC: String = format!(
            "Disable the cache for this comma separated list of tags\n\nProviding an empty list will disable all the caches. The supported tags are: {}.",
            VALID_TAGS.iter().join(", ")
        );
    }
    &DOC
}

impl Opt {
    /// Make an `EvaluationConfig` from this command line options.
    pub fn to_config(&self) -> EvaluationConfig {
        EvaluationConfig {
            solution_filter: self.filter.clone(),
            booklet_solutions: self.booklet_solutions,
            no_statement: self.no_statement,
            solution_paths: self.solution.clone(),
            disabled_sanity_checks: self.skip_sanity_checks.clone(),
            seed: self.seed,
            dry_run: self.dry_run,
        }
    }

    /// Get the store directory of this configuration. If nothing is specified a cache directory is
    /// used if available, otherwise a temporary directory.
    pub fn store_dir(&self) -> PathBuf {
        match &self.store_dir {
            Some(dir) => dir.clone(),
            None => {
                let project = directories::ProjectDirs::from("", "", "task-maker");
                if let Some(project) = project {
                    project.cache_dir().to_owned()
                } else {
                    std::env::temp_dir().join("task-maker")
                }
            }
        }
    }

    pub fn enable_log(&mut self) {
        // configure the logger based on the verbosity level
        let mut show_warning = false;
        if self.verbose > 0 {
            if let task_maker_format::ui::UIType::Curses = self.ui {
                if self.remote.is_none() {
                    // warning deferred to after the logger has been initialized
                    show_warning = true;
                }
                self.ui = task_maker_format::ui::UIType::Print;
            }
            std::env::set_var("RUST_BACKTRACE", "1");
            match self.verbose {
                0 => unreachable!(),
                1 => std::env::set_var("RUST_LOG", "info,tabox=info"),
                2 => std::env::set_var("RUST_LOG", "debug,tabox=debug"),
                _ => std::env::set_var("RUST_LOG", "trace,tabox=trace"),
            }
        }

        env_logger::Builder::from_default_env()
            .default_format_timestamp_nanos(true)
            .init();
        better_panic::install();
        if show_warning {
            warn!(
                "Do not combine -v with curses ui, bad things will happen! Fallbacking to print ui"
            );
        }
    }
}
