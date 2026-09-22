mod cli;
mod commands;
mod error;
mod fs;

use std::process::ExitCode;

use clap::Parser;
use cli::{Cli, Commands};

use commands::{stow, unstow};
use error::Error;

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Commands::Stow(args) => {
            println!("stow::run");
            report(
                stow::run(&args),
                "Stow operation completed successfully.",
                "Error during stow operation",
            )
        }
        Commands::Unstow(args) => {
            println!("unstow::run");
            report(
                unstow::run(&args),
                "Unstow operation completed successfully.",
                "Error during unstow operation",
            )
        }
        Commands::Restow(args) => {
            let result = unstow::run(&args).and_then(|_| stow::run(&args));
            report(
                result,
                "Restow operation completed successfully.",
                "Error during restow operation",
            )
        }
    }
}

fn report(result: Result<(), Error>, success: &str, failure: &str) -> ExitCode {
    match result {
        Ok(()) => {
            println!("{success}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{failure}: {error:?}");
            ExitCode::FAILURE
        }
    }
}
