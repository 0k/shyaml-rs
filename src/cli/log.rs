use log::LevelFilter::*;

use std::io;
const DATE_FORMAT_STR: &str =
    "[year]-[month]-[day] [hour]:[minute]:[second] [offset_hour sign:mandatory][offset_minute]";
const LOG_DIRECTIVE_REGEX: &str =
    r"^[a-zA-Z0-9_-]+(\.[a-zA-Z0-9_-]+)*:(TRACE|DEBUG|INFO|WARN|ERROR|OFF)$";

// Converts a level string to the corresponding log::LevelFilter
fn level_from_str(level: &str) -> Result<log::LevelFilter, String> {
    match level.to_lowercase().as_str() {
        "trace" => Ok(Trace),
        "debug" => Ok(Debug),
        "info" => Ok(Info),
        "warn" => Ok(Warn),
        "error" => Ok(Error),
        "off" => Ok(Off),
        _ => unreachable!("Regex should have ensured that level is valid"),
    }
}

pub fn setup(verbosity: u8, logs: Vec<&str>, log_time: bool) -> Result<(), String> {
    let mut error_log: Vec<String> = Vec::new();
    let mut base_config = fern::Dispatch::new();
    use colored::*;

    let log_directive_re = regex::Regex::new(LOG_DIRECTIVE_REGEX).map_err(|e| e.to_string())?;

    // log statements are in the format TARGET:LEVEL
    for log in logs {
        if !log_directive_re.is_match(log) {
            error_log.push(format!("Invalid log directive: {:?}", log));
            continue;
        }

        if let Some((target, level)) = log.rsplit_once(':') {
            let level_filter = level_from_str(level);
            base_config =
                base_config.level_for(target.replace(".", "::").to_string(), level_filter?);
        } else {
            unreachable!("Regex should have ensured that we can split on ':'");
        }
    }
    if !error_log.is_empty() {
        return Err(format!(
            "Unexpected logging directives:\n - {}\n  Please use TARGET:LEVEL, for instance: \"rsipe:DEBUG\"",
            error_log.join("\n - ")
        ));
    }

    base_config = match verbosity {
        0 => base_config.level(log::LevelFilter::Warn),
        1 => base_config.level(log::LevelFilter::Info),
        2 => base_config.level(log::LevelFilter::Debug),
        _3_or_more => base_config.level(log::LevelFilter::Trace),
    };

    // Common formatting shared by both closures, without the time component.
    fn common_format(message: &std::fmt::Arguments, record: &log::Record) -> String {
        let level = match record.level() {
            log::Level::Error => "E".bright_red(),
            log::Level::Warn => "W".bright_yellow(),
            log::Level::Info => "I".bright_green(),
            log::Level::Trace => "T".black(),
            log::Level::Debug => "D".blue(),
        };
        let target = record.target().replace("::", ".").to_string().yellow();

        format!("{} {}: {}", level, target, message)
    }

    let mut stderr_config = fern::Dispatch::new();
    // Choose the appropriate formatting function based on log_time.
    stderr_config = if log_time {
        // Define the closure for formatting with time.
        let dt_fmt = match time::format_description::parse(DATE_FORMAT_STR) {
            Ok(e) => e,
            Err(e) => return Err(e.to_string()),
        };

        stderr_config.format(
            move |out: fern::FormatCallback,
                  message: &std::fmt::Arguments,
                  record: &log::Record| {
                let now = time::OffsetDateTime::now_local()
                    .unwrap_or_else(|_| time::OffsetDateTime::now_utc());
                let time_str = now.format(&dt_fmt).unwrap().to_string().cyan();
                let formatted_message = common_format(message, record);
                out.finish(format_args!("{} {}", time_str, formatted_message));
            },
        )
    } else {
        stderr_config.format(
            move |out: fern::FormatCallback,
                  message: &std::fmt::Arguments,
                  record: &log::Record| {
                let formatted_message = common_format(message, record);
                out.finish(format_args!("{}", formatted_message));
            },
        )
    }
    .chain(io::stderr());
    base_config
        .chain(stderr_config)
        .apply()
        .map_err(|e| e.to_string())
}
