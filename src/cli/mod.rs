mod def;
include!(concat!(env!("OUT_DIR"), "/rustc_version.rs"));
use clap::Parser;
use std::io::Write;

pub mod log;

impl From<crate::yaml::Error> for String {
    fn from(e: crate::yaml::Error) -> Self {
        e.to_string()
    }
}

pub fn run() -> Result<bool, String> {
    let cli = def::Args::parse();

    // Split log strings upon comma, trim them and flatten all in
    // `logs`, remove empty values
    let logs = cli.log.unwrap_or_else(Vec::new); // Provide an empty Vec if cli.log is None
    let logs = logs
        .iter()
        .flat_map(|log| log.split(',')) // Split each log entry on commas
        .map(str::trim) // Trim whitespace from each resulting entry
        .filter(|s| !s.is_empty()) // Remove empty strings
        .collect::<Vec<&str>>(); // Collect into a Vec<&str>

    // Upon failure, display error message and usage string
    log::setup(cli.verbose, logs, cli.log_time)?;

    if cli.color && cli.no_color {
        return Err("Cannot use both --color and --no-color".to_string());
    }
    if cli.color {
        colored::control::set_override(true);
    }
    if cli.no_color {
        colored::control::set_override(false);
    }

    if cli.version {
        // use crate version
        println!("version: {}", env!("CARGO_PKG_VERSION"));
        println!(
            "libfyaml used: True\nlibfyaml available: {}",
            crate::yaml::get_version()?
        );
        println!("Rust: {}", RUSTC_VERSION);
        return Ok(true);
    }

    match &cli.action {
        Some(def::Actions::GetValue {
            path,
            default,
            yaml,
            line_buffer,
        }) => {
            let yaml = cli.yaml || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let print_sep = if yaml { "\n---\n" } else { "\0" };
            let mut first = true;
            let values = crate::yaml::get_value(path, yaml);
            if let Err(crate::yaml::Error::PathError(e)) = values {
                if cli.quiet {
                    return Ok(false);
                }
                return Err(e.to_string());
            }
            for doc_value in values? {
                ::log::trace!("got string");

                if first {
                    first = false;
                } else {
                    print!("{}", print_sep);
                }
                match doc_value {
                    Ok(s) => {
                        print!("{}", s);
                        if *line_buffer {
                            // flush stdout
                            ::log::trace!("flushing stdout");
                            let _ = std::io::stdout().flush().map_err(|e| e.to_string());
                        }
                    }
                    Err(e) => {
                        if let Some(default) = default {
                            print!("{}", default.to_string());
                            return Ok(true);
                        }
                        if let crate::yaml::Error::PathError(_) = e {
                            if cli.quiet {
                                return Ok(false);
                            }
                        }
                        return Err(e.to_string());
                    }
                }
            }
            if first {
                if !path.is_none() {
                    if cli.quiet {
                        return Ok(false);
                    }
                    return Err(format!("Invalid path: {:?}", path));
                }
            }
        }
        Some(def::Actions::GetType { path }) => {
            let path = path.as_ref().map(|s| s.as_str());
            match crate::yaml::get_type(path) {
                Ok(t) => println!("{}", t),
                Err(e) => {
                    if cli.quiet {
                        return Ok(false);
                    }
                    return Err(e.to_string());
                }
            }
        }
        Some(def::Actions::GetLength { path }) => {
            let path = path.as_ref().map(|s| s.as_str());
            match crate::yaml::get_length(path) {
                Ok(t) => println!("{}", t),
                Err(e) => {
                    if cli.quiet {
                        return Ok(false);
                    }
                    return Err(e.to_string());
                }
            }
        }
        Some(def::Actions::Keys { path, yaml }) => {
            let yaml = cli.yaml || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let _ = crate::yaml::keys(path, yaml, |k| {
                println!("{}", k.unwrap());
                Ok(())
            })?;
        }
        Some(def::Actions::Keys0 { path, yaml }) => {
            let yaml = cli.yaml || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let _ = crate::yaml::keys(path, yaml, |k| {
                print!("{}\0", k.unwrap());
                Ok(())
            })?;
        }
        Some(def::Actions::Values { path, yaml }) => {
            let yaml = cli.yaml || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let _ = crate::yaml::values(path, yaml, |k| {
                println!("{}", k.unwrap());
                Ok(())
            })?;
        }
        Some(def::Actions::Values0 { path, yaml }) => {
            let yaml = cli.yaml || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let _ = crate::yaml::values(path, yaml, |k| {
                print!("{}\0", k.unwrap());
                Ok(())
            })?;
        }
        Some(def::Actions::KeyValues { path, yaml }) => {
            let yaml = cli.yaml || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let _ = crate::yaml::key_values(path, yaml, |k| {
                let (key, value) = k.unwrap();
                println!("{}", key);
                println!("{}", value);
                Ok(())
            })?;
        }
        Some(def::Actions::KeyValues0 { path, yaml }) => {
            let yaml = cli.yaml || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let _ = crate::yaml::key_values(path, yaml, |k| {
                let (key, value) = k.unwrap();
                print!("{}\0", key);
                print!("{}\0", value);
                Ok(())
            })?;
        }
        Some(def::Actions::GetValues { path, yaml }) => {
            let yaml = cli.yaml || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let _ = crate::yaml::get_values(path, yaml, |k| {
                println!("{}", k.unwrap());
                Ok(())
            })?;
        }
        Some(def::Actions::GetValues0 { path, yaml }) => {
            let yaml = cli.yaml || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let _ = crate::yaml::get_values(path, yaml, |k| {
                print!("{}\0", k.unwrap());
                Ok(())
            })?;
        }
        Some(def::Actions::Apply {
            overlays,
            merge_policy,
        }) => {
            let policies = crate::yaml::parse_merge_policies(merge_policy.as_ref())?;
            let result = crate::yaml::apply(overlays, &policies)?;
            print!("{}", result);
        }
        Some(def::Actions::SetValue { key, value, yaml }) => {
            let result = crate::yaml::set_value(key, value, *yaml)?;
            print!("{}", result);
        }
        None => {
            return Err("Missing action".to_string());
        }
    }
    Ok(true)
}
