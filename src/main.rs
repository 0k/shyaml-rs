use colored::*;
use std::process;

mod cli;
mod tag;
mod yaml;

fn main() {
    match cli::run() {
        Ok(true) => process::exit(0),
        Ok(false) => process::exit(1),
        Err(e) => {
            eprintln!("{}: {}", "Error".bright_red(), e);
            std::process::exit(127);
        }
    }
}
