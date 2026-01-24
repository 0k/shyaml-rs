mod def;
mod output;
include!(concat!(env!("OUT_DIR"), "/rustc_version.rs"));
use clap::Parser;
use fyaml::Document;

pub mod log;

impl From<crate::yaml::Error> for String {
    fn from(e: crate::yaml::Error) -> Self {
        e.to_string()
    }
}

// =============================================================================
// Normalized Action Types
// =============================================================================

/// Kind of iteration action.
#[derive(Clone, Copy, Debug)]
enum IterKind {
    Keys,
    Values,
    KeyValues,
    GetValues,
}

/// Normalized iteration action with common parameters extracted.
struct IterAction<'a> {
    kind: IterKind,
    path: Option<&'a str>,
    policy: output::OutputPolicy,
}

/// Extract iteration action parameters from Actions enum.
/// Returns None if the action is not an iteration action.
fn normalize_iter_action<'a>(
    action: &'a def::Actions,
    base_yaml_mode: bool,
) -> Option<IterAction<'a>> {
    match action {
        def::Actions::Keys { path, yaml } => Some(IterAction {
            kind: IterKind::Keys,
            path: path.as_ref().map(|s| s.as_str()),
            policy: output::OutputPolicy::newline(base_yaml_mode || *yaml),
        }),
        def::Actions::Keys0 { path, yaml } => Some(IterAction {
            kind: IterKind::Keys,
            path: path.as_ref().map(|s| s.as_str()),
            policy: output::OutputPolicy::nul(base_yaml_mode || *yaml),
        }),
        def::Actions::Values { path, yaml } => Some(IterAction {
            kind: IterKind::Values,
            path: path.as_ref().map(|s| s.as_str()),
            policy: output::OutputPolicy::newline(base_yaml_mode || *yaml),
        }),
        def::Actions::Values0 { path, yaml } => Some(IterAction {
            kind: IterKind::Values,
            path: path.as_ref().map(|s| s.as_str()),
            policy: output::OutputPolicy::nul(base_yaml_mode || *yaml),
        }),
        def::Actions::KeyValues { path, yaml } => Some(IterAction {
            kind: IterKind::KeyValues,
            path: path.as_ref().map(|s| s.as_str()),
            policy: output::OutputPolicy::newline(base_yaml_mode || *yaml),
        }),
        def::Actions::KeyValues0 { path, yaml } => Some(IterAction {
            kind: IterKind::KeyValues,
            path: path.as_ref().map(|s| s.as_str()),
            policy: output::OutputPolicy::nul(base_yaml_mode || *yaml),
        }),
        def::Actions::GetValues { path, yaml } => Some(IterAction {
            kind: IterKind::GetValues,
            path: path.as_ref().map(|s| s.as_str()),
            policy: output::OutputPolicy::newline(base_yaml_mode || *yaml),
        }),
        def::Actions::GetValues0 { path, yaml } => Some(IterAction {
            kind: IterKind::GetValues,
            path: path.as_ref().map(|s| s.as_str()),
            policy: output::OutputPolicy::nul(base_yaml_mode || *yaml),
        }),
        _ => None,
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

// =============================================================================
// Zero-Copy Detection
// =============================================================================

/// Check if the action is read-only (can use zero-copy path).
fn is_readonly_action(action: &Option<def::Actions>) -> bool {
    matches!(
        action,
        Some(def::Actions::GetValue { .. })
            | Some(def::Actions::GetType { .. })
            | Some(def::Actions::GetLength { .. })
            | Some(def::Actions::Keys { .. })
            | Some(def::Actions::Keys0 { .. })
            | Some(def::Actions::Values { .. })
            | Some(def::Actions::Values0 { .. })
            | Some(def::Actions::KeyValues { .. })
            | Some(def::Actions::KeyValues0 { .. })
            | Some(def::Actions::GetValues { .. })
            | Some(def::Actions::GetValues0 { .. })
    )
}

fn run_command_chain(
    command_groups: &[Vec<String>],
    initial_value: crate::yaml::Value,
    multi_doc_yaml: bool,
) -> Result<crate::yaml::Value, String> {
    let mut current_value = initial_value;

    for (i, cmd_args) in command_groups.iter().enumerate() {
        let is_last_cmd = i == command_groups.len() - 1;
        // Only apply multi_doc_yaml newline handling on the last command
        let apply_multi_doc = is_last_cmd && multi_doc_yaml;
        current_value = run_single(
            cmd_args.clone(),
            current_value,
            is_last_cmd,
            false,
            apply_multi_doc,
        )?;
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

// =============================================================================
// Main Entry Point
// =============================================================================

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

    // Determine if we can use zero-copy path:
    // - Single command (no chaining)
    // - Read-only action
    let use_zero_copy = command_groups.len() == 1 && is_readonly_action(&cli.action);

    use std::io::Write;

    let doc_iter = crate::yaml::streaming_documents_from_stdin(line_buffered)?;
    let mut first = true;

    for doc_result in doc_iter {
        let doc = doc_result.map_err(|e| e.to_string())?;

        if !first {
            print!("{}", separator);
        }
        first = false;

        if use_zero_copy {
            // Zero-copy path: work directly with Document
            run_single_readonly(&cli, &doc, yaml_output)?;
        } else {
            // Value path: convert to owned Value for mutations/chains
            let value = crate::yaml::document_to_value(&doc).map_err(|e| e.to_string())?;
            run_command_chain(&command_groups, value, yaml_output)?;
        }

        if line_buffered {
            std::io::stdout().flush().ok();
        }
    }

    if first {
        // Empty input - no multi-doc separation needed
        if use_zero_copy {
            // Create empty doc for readonly - handle gracefully
            run_single_readonly_empty(&cli)?;
        } else {
            run_command_chain(&command_groups, crate::yaml::Value::Null, false)?;
        }
    }

    Ok(true)
}

// =============================================================================
// Zero-Copy Command Handler
// =============================================================================

/// Handle read-only commands using zero-copy path.
/// When `multi_doc_yaml` is true, ensures output ends with newline for proper YAML doc separation.
fn run_single_readonly(
    cli: &def::Args,
    doc: &Document,
    multi_doc_yaml: bool,
) -> Result<(), String> {
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

            match crate::yaml::get_value_ref(path, doc) {
                Ok(value_ref) => {
                    let output = if yaml_mode {
                        crate::yaml::serialize_ref(value_ref).map_err(|e| e.to_string())?
                    } else {
                        crate::yaml::serialize_raw_ref(value_ref)
                    };
                    print!("{}", output);
                    // Ensure output ends with newline for proper multi-doc YAML separation
                    if multi_doc_yaml && !output.ends_with('\n') {
                        println!();
                    }
                    Ok(())
                }
                Err(crate::yaml::Error::Path(e)) => {
                    if let Some(default_val) = default {
                        print!("{}", default_val);
                        if multi_doc_yaml && !default_val.ends_with('\n') {
                            println!();
                        }
                        return Ok(());
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
            let type_name = crate::yaml::get_type_ref(path, doc).map_err(|e| e.to_string())?;
            println!("{}", type_name);
            Ok(())
        }

        Some(def::Actions::GetLength { path }) => {
            let path = path.as_ref().map(|s| s.as_str());
            let len = crate::yaml::get_length_ref(path, doc).map_err(|e| e.to_string())?;
            println!("{}", len);
            Ok(())
        }

        // Handle all iteration actions (Keys/Keys0, Values/Values0, etc.) uniformly
        Some(action) if normalize_iter_action(action, yaml_mode).is_some() => {
            let iter_action = normalize_iter_action(action, yaml_mode).unwrap();
            match iter_action.kind {
                IterKind::Keys => {
                    let keys =
                        crate::yaml::keys_ref(iter_action.path, doc).map_err(|e| e.to_string())?;
                    output::print_items(keys, &iter_action.policy);
                }
                IterKind::Values => {
                    let values = crate::yaml::values_ref(iter_action.path, doc)
                        .map_err(|e| e.to_string())?;
                    output::print_items(values, &iter_action.policy);
                }
                IterKind::KeyValues => {
                    let kv = crate::yaml::key_values_ref(iter_action.path, doc)
                        .map_err(|e| e.to_string())?;
                    output::print_kv_items(kv, &iter_action.policy);
                }
                IterKind::GetValues => {
                    let iter = crate::yaml::get_values_ref(iter_action.path, doc)
                        .map_err(|e| e.to_string())?;
                    output::print_get_values(iter, &iter_action.policy);
                }
            }
            Ok(())
        }

        _ => unreachable!("Non-readonly action in readonly path"),
    }
}

/// Handle read-only commands on empty input.
fn run_single_readonly_empty(cli: &def::Args) -> Result<(), String> {
    let yaml_mode = cli.yaml;

    match &cli.action {
        Some(def::Actions::GetValue {
            path: _,
            default,
            yaml: _,
            line_buffer: _,
        }) => {
            // Empty document with path access should use default or error
            if let Some(default_val) = default {
                print!("{}", default_val);
                return Ok(());
            }
            if cli.quiet {
                std::process::exit(1);
            }
            Err("empty document".to_string())
        }

        Some(def::Actions::GetType { path: _ }) => {
            // Empty document type
            println!("NoneType");
            Ok(())
        }

        Some(def::Actions::GetLength { path: _ }) => {
            Err("get-length does not support 'NoneType' type. Please provide or select a sequence or struct.".to_string())
        }

        Some(def::Actions::Keys { path: _, yaml: _ })
        | Some(def::Actions::Keys0 { path: _, yaml: _ })
        | Some(def::Actions::Values { path: _, yaml: _ })
        | Some(def::Actions::Values0 { path: _, yaml: _ })
        | Some(def::Actions::KeyValues { path: _, yaml: _ })
        | Some(def::Actions::KeyValues0 { path: _, yaml: _ }) => {
            Err("keys/values does not support 'NoneType' type. Please provide or select a struct.".to_string())
        }

        Some(def::Actions::GetValues { path: _, yaml: _ })
        | Some(def::Actions::GetValues0 { path: _, yaml: _ }) => {
            Err("get-values does not support 'NoneType' type. Please provide or select a sequence or struct.".to_string())
        }

        _ => {
            // For other cases, output nothing for null
            if yaml_mode {
                print!("");
            }
            Ok(())
        }
    }
}
// =============================================================================
// Value-Based Command Handler (for mutations/chains)
// =============================================================================

fn output_value(value: &crate::yaml::Value, yaml_mode: bool) -> Result<String, String> {
    if yaml_mode {
        crate::yaml::serialize(value).map_err(|e| e.to_string())
    } else {
        Ok(crate::yaml::serialize_raw(value))
    }
}

fn run_single(
    args: Vec<String>,
    value: crate::yaml::Value,
    is_last: bool,
    setup_logging: bool,
    multi_doc_yaml: bool,
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

    // Handle iteration actions (Keys/Keys0, Values/Values0, etc.) uniformly
    if let Some(action) = &cli.action {
        if let Some(iter_action) = normalize_iter_action(action, yaml_mode) {
            let result = match iter_action.kind {
                IterKind::Keys => crate::yaml::keys(iter_action.path, &value)?,
                IterKind::Values => crate::yaml::values(iter_action.path, &value)?,
                IterKind::KeyValues => crate::yaml::key_values(iter_action.path, &value)?,
                IterKind::GetValues => crate::yaml::get_values(iter_action.path, &value)?,
            };
            if is_last {
                if let crate::yaml::Value::Sequence(seq) = &result {
                    output::print_items(seq.iter(), &iter_action.policy);
                }
            }
            return Ok(result);
        }
    }

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
                        // Ensure output ends with newline for proper multi-doc YAML separation
                        if multi_doc_yaml && !output.ends_with('\n') {
                            println!();
                        }
                    }
                    Ok(result)
                }
                Err(crate::yaml::Error::Path(e)) => {
                    if let Some(default_val) = default {
                        if is_last {
                            print!("{}", default_val);
                            if multi_doc_yaml && !default_val.ends_with('\n') {
                                println!();
                            }
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

        // Iteration actions are handled before this match
        Some(def::Actions::Keys { .. })
        | Some(def::Actions::Keys0 { .. })
        | Some(def::Actions::Values { .. })
        | Some(def::Actions::Values0 { .. })
        | Some(def::Actions::KeyValues { .. })
        | Some(def::Actions::KeyValues0 { .. })
        | Some(def::Actions::GetValues { .. })
        | Some(def::Actions::GetValues0 { .. }) => {
            unreachable!("Iteration actions handled above")
        }

        None => Err("Missing action".to_string()),
    }
}
