use structopt::StructOpt;

#[derive(Debug, Clone, StructOpt)]
#[structopt(long_version(option_env!("LONG_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))))]
#[structopt(rename_all = "kebab-case")]
/// Tool for running specified command in parallel against input.
pub struct AppOptions {
    // The number of occurrences of the `v/verbose` flag
    /// Sets the level of verbosity (-v, -vv, -vvv, etc.)
    #[structopt(short, parse(from_occurrences))]
    pub verbose_level: u8,

    /// Specify number of jobs.
    #[structopt(short, long, value_name="JOBS")]
    pub jobs: Option<u32>,

    /// Specify number of lines each job handles in batches.
    #[structopt(short="L", long, value_name="LINE")]
    pub lines: Option<u32>,

    /// Dry run.
    #[structopt(short="D", long)]
    pub dry_run: bool,

    /// Specify input file. If not specified, STDIN will be used.
    #[structopt(short, long, value_name="FILE")]
    pub input_file: Option<String>,

    /// Specify output file. If not specified, write to STDOUT.
    #[structopt(short, long, value_name="FILE")]
    pub output_file: Option<String>,

    /// Specify number of size for buffer.
    #[structopt(short="B", long, value_name="BUFFER_SIZE")]
    pub buffer_size: Option<u32>,

    /// Commands to run in parallel.
    pub commands: Vec<String>,
}
