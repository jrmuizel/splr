use std::path::PathBuf;
use structopt::StructOpt;

/// Configuration built from command line options
#[derive(Debug, StructOpt)]
#[structopt(
    name = "splr",
    about = "SAT solver for Propositional Logic in Rust, version 0.1.0"
)]
pub struct Config {
    /// solf limit of clause DB (default is about 4GB)
    #[structopt(long = "cl", default_value = "18000000")]
    pub clause_limit: usize,
    /// grow limit of #clauses by var elimination
    #[structopt(long = "eg", default_value = "0")]
    pub elim_grow_limit: usize,
    /// #literals in a merged clause by var elimination
    #[structopt(long = "el", default_value = "100")]
    pub elim_lit_limit: usize,
    /// #samples for average assignment rate
    #[structopt(long = "ra", default_value = "3500")]
    pub restart_asg_samples: usize,
    /// #samples for average LBD of learnt clauses
    #[structopt(long = "rl", default_value = "50")]
    pub restart_lbd_samples: usize,
    /// threshold for forcing restart (K in Glucose)
    #[structopt(long = "rt", default_value = "0.60")]
    pub restart_threshold: f64,
    /// threshold for blocking restart (R in Glucose)
    #[structopt(long = "rb", default_value = "1.40")]
    pub restart_blocking: f64,
    /// #conflicts between restarts
    #[structopt(long = "rs", default_value = "50")]
    pub restart_step: usize,
    /// output filiname; use default rule if it's empty.
    #[structopt(long = "--output", short = "o", default_value = "")]
    pub output_filname: String,
    /// Uses Glucose format for progress report
    #[structopt(long = "--log", short = "l")]
    pub use_log: bool,
    /// Disables exhaustive simplification
    #[structopt(long = "no-elim", short = "e")]
    pub no_elim: bool,
    /// Disables dynamic strategy adaptation
    #[structopt(long = "no-adaptation", short = "a")]
    pub no_adapt: bool,
    /// a CNF file to solve
    #[structopt(parse(from_os_str))]
    pub cnf: std::path::PathBuf,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            clause_limit: 18_000_000,
            elim_grow_limit: 0,
            elim_lit_limit: 100,
            restart_asg_samples: 3500,
            restart_lbd_samples: 50,
            restart_threshold: 0.60,
            restart_blocking: 1.40,
            restart_step: 50,
            output_filname: "".to_string(),
            use_log: false,
            no_elim: false,
            no_adapt: false,
            cnf: PathBuf::new(),
        }
    }
}
