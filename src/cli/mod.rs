mod def;
include!(concat!(env!("OUT_DIR"), "/rustc_version.rs"));
use clap::Parser;

pub mod log;

impl From<crate::yaml::Error> for String {
    fn from(e: crate::yaml::Error) -> Self {
        e.to_string()
    }
}

fn split_compound_args(args: Vec<String>) -> Vec<Vec<String>> {
    let mut groups = Vec::new();
    let mut current_group = Vec::new();
    let program_name = args
        .first()
        .cloned()
        .unwrap_or_else(|| "shyaml".to_string());

    for arg in args.into_iter().skip(1) {
        if arg == ";" {
            if !current_group.is_empty() {
                let mut group = vec![program_name.clone()];
                group.append(&mut current_group);
                groups.push(group);
                current_group = Vec::new();
            }
        } else {
            current_group.push(arg);
        }
    }

    if !current_group.is_empty() {
        let mut group = vec![program_name];
        group.append(&mut current_group);
        groups.push(group);
    }

    groups
}

fn setup_cli_context(args: &[String]) -> Result<def::Args, String> {
    let cli = def::Args::try_parse_from(args).map_err(|e| e.to_string())?;

    let logs = cli.log.clone().unwrap_or_default();
    let logs = logs
        .iter()
        .flat_map(|log| log.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>();

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

    Ok(cli)
}

fn run_command_chain(
    command_groups: &[Vec<String>],
    initial_value: crate::yaml::Value,
) -> Result<crate::yaml::Value, String> {
    let mut current_value = initial_value;

    for (i, cmd_args) in command_groups.iter().enumerate() {
        let is_last_cmd = i == command_groups.len() - 1;
        current_value = run_single(cmd_args.clone(), current_value, is_last_cmd, false)?;
    }
    Ok(current_value)
}

fn is_line_buffered(cli: &def::Args) -> bool {
    matches!(
        &cli.action,
        Some(def::Actions::GetValue {
            line_buffer: true,
            ..
        })
    )
}

fn is_yaml_output(cli: &def::Args) -> bool {
    if cli.yaml {
        return true;
    }
    matches!(&cli.action, Some(def::Actions::GetValue { yaml: true, .. }))
}

pub fn run() -> Result<bool, String> {
    let args: Vec<String> = std::env::args().collect();
    let command_groups = split_compound_args(args);

    if command_groups.is_empty() {
        return Err("No command provided".to_string());
    }

    let cli = setup_cli_context(&command_groups[0])?;

    if cli.version {
        println!("version: {}", env!("CARGO_PKG_VERSION"));
        println!(
            "libfyaml used: True\nlibfyaml available: {}",
            crate::yaml::get_version()?
        );
        println!("Rust: {}", RUSTC_VERSION);
        return Ok(true);
    }

    let line_buffered = is_line_buffered(&cli);
    let yaml_output = is_yaml_output(&cli);
    let separator = if yaml_output { "---\n" } else { "\0" };

    use std::io::Write;

    let doc_iter = crate::yaml::streaming_documents_from_stdin(line_buffered)?;
    let mut first = true;

    for doc_result in doc_iter {
        let doc = doc_result?;
        if !first {
            print!("{}", separator);
        }
        first = false;
        run_command_chain(&command_groups, doc)?;
        if line_buffered {
            std::io::stdout().flush().ok();
        }
    }

    if first {
        run_command_chain(&command_groups, crate::yaml::Value::Null)?;
    }

    Ok(true)
}

fn output_value(value: &crate::yaml::Value, yaml_mode: bool) -> Result<String, String> {
    if yaml_mode {
        crate::yaml::serialize(value).map_err(|e| e.to_string())
    } else {
        Ok(crate::yaml::serialize_raw(value))
    }
}

fn output_sequence(seq: &[crate::yaml::Value], separator: &str, yaml_mode: bool, _is_last: bool) {
    for (i, item) in seq.iter().enumerate() {
        if i > 0 {
            print!("{}", separator);
        }
        let s = if yaml_mode {
            crate::yaml::serialize(item).unwrap_or_default()
        } else {
            crate::yaml::serialize_raw(item)
        };
        print!("{}", s);
    }
    if !seq.is_empty() && separator == "\n" {
        println!();
    }
}

fn run_single(
    args: Vec<String>,
    value: crate::yaml::Value,
    is_last: bool,
    setup_logging: bool,
) -> Result<crate::yaml::Value, String> {
    let cli = def::Args::try_parse_from(args).map_err(|e| e.to_string())?;

    if setup_logging {
        let logs = cli.log.clone().unwrap_or_default();
        let logs = logs
            .iter()
            .flat_map(|log| log.split(','))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<&str>>();

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
    }

    if cli.version {
        println!("version: {}", env!("CARGO_PKG_VERSION"));
        println!(
            "libfyaml used: True\nlibfyaml available: {}",
            crate::yaml::get_version()?
        );
        println!("Rust: {}", RUSTC_VERSION);
        return Ok(crate::yaml::Value::Null);
    }

    let yaml_mode = cli.yaml;

    match &cli.action {
        Some(def::Actions::GetValue {
            path,
            default,
            yaml,
            line_buffer: _,
        }) => {
            let yaml_mode = yaml_mode || *yaml;
            let path = path.as_ref().map(|s| s.as_str());

            match crate::yaml::get_value(path, &value) {
                Ok(result) => {
                    if is_last {
                        let output = output_value(&result, yaml_mode)?;
                        print!("{}", output);
                    }
                    Ok(result)
                }
                Err(crate::yaml::Error::PathError(e)) => {
                    if let Some(default_val) = default {
                        if is_last {
                            print!("{}", default_val);
                        }
                        return Ok(crate::yaml::Value::String(default_val.clone()));
                    }
                    if cli.quiet {
                        std::process::exit(1);
                    }
                    Err(e)
                }
                Err(e) => Err(e.to_string()),
            }
        }

        Some(def::Actions::GetType { path }) => {
            let path = path.as_ref().map(|s| s.as_str());
            let result = crate::yaml::get_type(path, &value)?;
            if is_last {
                println!("{}", crate::yaml::serialize_raw(&result));
            }
            Ok(result)
        }

        Some(def::Actions::GetLength { path }) => {
            let path = path.as_ref().map(|s| s.as_str());
            let result = crate::yaml::get_length(path, &value)?;
            if is_last {
                println!("{}", crate::yaml::serialize_raw(&result));
            }
            Ok(result)
        }

        Some(def::Actions::Keys { path, yaml }) => {
            let yaml_mode = yaml_mode || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let result = crate::yaml::keys(path, &value)?;
            if is_last {
                if let crate::yaml::Value::Sequence(seq) = &result {
                    output_sequence(seq, "\n", yaml_mode, is_last);
                }
            }
            Ok(result)
        }

        Some(def::Actions::Keys0 { path, yaml }) => {
            let yaml_mode = yaml_mode || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let result = crate::yaml::keys(path, &value)?;
            if is_last {
                if let crate::yaml::Value::Sequence(seq) = &result {
                    for item in seq {
                        let s = if yaml_mode {
                            crate::yaml::serialize(item).unwrap_or_default()
                        } else {
                            crate::yaml::serialize_raw(item)
                        };
                        print!("{}\0", s);
                    }
                }
            }
            Ok(result)
        }

        Some(def::Actions::Values { path, yaml }) => {
            let yaml_mode = yaml_mode || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let result = crate::yaml::values(path, &value)?;
            if is_last {
                if let crate::yaml::Value::Sequence(seq) = &result {
                    output_sequence(seq, "\n", yaml_mode, is_last);
                }
            }
            Ok(result)
        }

        Some(def::Actions::Values0 { path, yaml }) => {
            let yaml_mode = yaml_mode || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let result = crate::yaml::values(path, &value)?;
            if is_last {
                if let crate::yaml::Value::Sequence(seq) = &result {
                    for item in seq {
                        let s = if yaml_mode {
                            crate::yaml::serialize(item).unwrap_or_default()
                        } else {
                            crate::yaml::serialize_raw(item)
                        };
                        print!("{}\0", s);
                    }
                }
            }
            Ok(result)
        }

        Some(def::Actions::KeyValues { path, yaml }) => {
            let yaml_mode = yaml_mode || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let result = crate::yaml::key_values(path, &value)?;
            if is_last {
                if let crate::yaml::Value::Sequence(seq) = &result {
                    output_sequence(seq, "\n", yaml_mode, is_last);
                }
            }
            Ok(result)
        }

        Some(def::Actions::KeyValues0 { path, yaml }) => {
            let yaml_mode = yaml_mode || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let result = crate::yaml::key_values(path, &value)?;
            if is_last {
                if let crate::yaml::Value::Sequence(seq) = &result {
                    for item in seq {
                        let s = if yaml_mode {
                            crate::yaml::serialize(item).unwrap_or_default()
                        } else {
                            crate::yaml::serialize_raw(item)
                        };
                        print!("{}\0", s);
                    }
                }
            }
            Ok(result)
        }

        Some(def::Actions::GetValues { path, yaml }) => {
            let yaml_mode = yaml_mode || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let result = crate::yaml::get_values(path, &value)?;
            if is_last {
                if let crate::yaml::Value::Sequence(seq) = &result {
                    output_sequence(seq, "\n", yaml_mode, is_last);
                }
            }
            Ok(result)
        }

        Some(def::Actions::GetValues0 { path, yaml }) => {
            let yaml_mode = yaml_mode || *yaml;
            let path = path.as_ref().map(|s| s.as_str());
            let result = crate::yaml::get_values(path, &value)?;
            if is_last {
                if let crate::yaml::Value::Sequence(seq) = &result {
                    for item in seq {
                        let s = if yaml_mode {
                            crate::yaml::serialize(item).unwrap_or_default()
                        } else {
                            crate::yaml::serialize_raw(item)
                        };
                        print!("{}\0", s);
                    }
                }
            }
            Ok(result)
        }

        Some(def::Actions::Apply {
            overlays,
            merge_policy,
        }) => {
            let policies = crate::yaml::parse_merge_policies(merge_policy.as_ref())?;
            let result = crate::yaml::apply(overlays, &policies, value)?;
            if is_last {
                print!("{}", crate::yaml::serialize(&result)?);
            }
            Ok(result)
        }

        Some(def::Actions::SetValue {
            key,
            value: val_str,
            yaml,
        }) => {
            let new_value = crate::yaml::parse_value(val_str, *yaml)?;
            let result = crate::yaml::set_value(key, new_value, value)?;
            if is_last {
                print!("{}", crate::yaml::serialize(&result)?);
            }
            Ok(result)
        }

        Some(def::Actions::Del { key }) => {
            let result = crate::yaml::del(key, value)?;
            if is_last {
                print!("{}", crate::yaml::serialize(&result)?);
            }
            Ok(result)
        }

        None => Err("Missing action".to_string()),
    }
}
