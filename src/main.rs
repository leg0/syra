mod cli;
mod fs;
mod error;
mod commands;

use cli::{Cli, Commands};
use clap::Parser;

use commands::{stow, unstow};

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Stow(args) => {
            println!("stow::run");
            match stow::run(args) {
                Ok(_) => println!("Stow operation completed successfully."),

                // TODO: handle error properly
                Err(e) => eprintln!("Error during stow operation: {:?}", e),
            }
        },
        Commands::Unstow(args) => {
            println!("unstow::run");
            match unstow::run(args) {
                Ok(_) => println!("Unstow operation completed successfully."),
                Err(e) => eprintln!("Error during unstow operation: {:?}", e),
            }
        }
    }
}
