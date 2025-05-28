use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(trailing_var_arg = true)]
pub struct Cli {
    // #[arg(short = 'S', long = "stow", help("Stow"), default_value_t=true)]
    // pub stow: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Parser, Debug)]
pub struct StowArgs {
    #[arg(
        short = 'd',
        long = "dir",
        help("Directory whose contents are linked to target")
    )]
    pub package_dir: Option<PathBuf>,

    #[arg(
        short = 't',
        long = "target",
        help("Directory to create the links into")
    )]
    pub target_dir: Option<PathBuf>,
    
    #[arg(help("Packages to stow"), required = true, num_args = 1..)]
    pub packages: Vec<String>,

    #[arg(
        short = 'v',
        long = "verbose",
        help("Print some extra info during run"),
        default_value_t = false
    )]
    pub verbose: bool,

    #[arg(
        short = 'n',
        long = "no",
        help(
            "do not perform any operations that modify the filesystem. Just show what would happen"
        ),
        default_value_t = true
    )]
    pub simulate: bool,
}

pub type UnstowArgs = StowArgs;

#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(about = "Stow packages into a target directory")]
    Stow(StowArgs),
    #[command(about = "Unstow packages from a target directory")]
    Unstow(UnstowArgs),
}
