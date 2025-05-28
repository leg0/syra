mod cli;
mod fs;
mod error;
mod commands;

use cli::{Cli, Commands};
use clap::Parser;

use commands::stow;

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
    }
}
