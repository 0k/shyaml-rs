//! Unified output handling for YAML values.
//!
//! This module provides a trait-based abstraction for outputting YAML values,
//! whether they come from zero-copy (`ValueRef`) or owned (`Value`) sources.

use crate::yaml;

// =============================================================================
// YamlOutput Trait
// =============================================================================

/// Trait for types that can be output as YAML or raw strings.
///
/// Implemented for both zero-copy `ValueRef` and owned `Value` types.
pub trait YamlOutput {
    /// Serialize to strict YAML format.
    fn to_yaml_string(&self) -> Result<String, yaml::Error>;

    /// Serialize to raw format (unquoted scalars, YAML for complex types).
    fn to_raw_string(&self) -> String;

    /// Output with current policy (yaml_mode determines format).
    fn format(&self, yaml_mode: bool) -> String {
        if yaml_mode {
            self.to_yaml_string().unwrap_or_default()
        } else {
            self.to_raw_string()
        }
    }
}

impl YamlOutput for fyaml::ValueRef<'_> {
    fn to_yaml_string(&self) -> Result<String, yaml::Error> {
        yaml::serialize_ref(*self)
    }

    fn to_raw_string(&self) -> String {
        yaml::serialize_raw_ref(*self)
    }
}

impl YamlOutput for yaml::Value {
    fn to_yaml_string(&self) -> Result<String, yaml::Error> {
        yaml::serialize(self)
    }

    fn to_raw_string(&self) -> String {
        yaml::serialize_raw(self)
    }
}

impl YamlOutput for &yaml::Value {
    fn to_yaml_string(&self) -> Result<String, yaml::Error> {
        yaml::serialize(self)
    }

    fn to_raw_string(&self) -> String {
        yaml::serialize_raw(self)
    }
}

// =============================================================================
// OutputPolicy
// =============================================================================

/// Policy for formatting output.
#[derive(Clone, Debug)]
pub struct OutputPolicy {
    /// Separator between items ("\n" for lines, "\0" for null-terminated).
    pub separator: Separator,
    /// If true, output strict YAML; if false, use raw format for scalars.
    pub yaml_mode: bool,
}

/// Type of separator between output items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Separator {
    /// Newline-separated output (standard mode).
    Newline,
    /// Null-terminated output (for -0 variants).
    Nul,
}

impl Separator {
    /// Get the separator string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Separator::Newline => "\n",
            Separator::Nul => "\0",
        }
    }
}

impl OutputPolicy {
    /// Create a new output policy with newline separator.
    pub fn newline(yaml_mode: bool) -> Self {
        Self {
            separator: Separator::Newline,
            yaml_mode,
        }
    }

    /// Create a new output policy with null separator.
    pub fn nul(yaml_mode: bool) -> Self {
        Self {
            separator: Separator::Nul,
            yaml_mode,
        }
    }
}

// =============================================================================
// Output Functions
// =============================================================================

/// Print a sequence of values with the given policy.
pub fn print_items<T: YamlOutput>(iter: impl Iterator<Item = T>, policy: &OutputPolicy) {
    let sep = policy.separator.as_str();
    let yaml_mode = policy.yaml_mode;

    match policy.separator {
        Separator::Newline => {
            let mut first = true;
            for item in iter {
                if !first {
                    print!("{}", sep);
                }
                first = false;
                print!("{}", item.format(yaml_mode));
            }
            // Add trailing newline if we printed anything
            if !first {
                println!();
            }
        }
        Separator::Nul => {
            for item in iter {
                print!("{}\0", item.format(yaml_mode));
            }
        }
    }
}

/// Print key-value pairs with the given policy.
pub fn print_kv_items<K: YamlOutput, V: YamlOutput>(
    iter: impl Iterator<Item = (K, V)>,
    policy: &OutputPolicy,
) {
    let sep = policy.separator.as_str();
    let yaml_mode = policy.yaml_mode;

    match policy.separator {
        Separator::Newline => {
            let mut first = true;
            for (k, v) in iter {
                if !first {
                    print!("{}", sep);
                }
                first = false;
                print!("{}", k.format(yaml_mode));
                print!("{}", sep);
                print!("{}", v.format(yaml_mode));
            }
            // Add trailing newline if we printed anything
            if !first {
                println!();
            }
        }
        Separator::Nul => {
            for (k, v) in iter {
                print!("{}\0", k.format(yaml_mode));
                print!("{}\0", v.format(yaml_mode));
            }
        }
    }
}

/// Print get-values iterator (handles both sequence and mapping cases).
pub fn print_get_values(iter: yaml::GetValuesIter<'_>, policy: &OutputPolicy) {
    match iter {
        yaml::GetValuesIter::Seq(seq_iter) => {
            print_items(seq_iter, policy);
        }
        yaml::GetValuesIter::Map(map_iter) => {
            print_kv_items(map_iter, policy);
        }
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_separator_as_str() {
        assert_eq!(Separator::Newline.as_str(), "\n");
        assert_eq!(Separator::Nul.as_str(), "\0");
    }

    #[test]
    fn test_output_policy_constructors() {
        let policy = OutputPolicy::newline(true);
        assert_eq!(policy.separator, Separator::Newline);
        assert!(policy.yaml_mode);

        let policy = OutputPolicy::nul(false);
        assert_eq!(policy.separator, Separator::Nul);
        assert!(!policy.yaml_mode);
    }

    #[test]
    fn test_value_yaml_output() {
        let value = yaml::Value::String("hello".to_string());
        assert_eq!(value.to_raw_string(), "hello");
        // YAML mode quotes strings
        let yaml_str = value.to_yaml_string().unwrap();
        assert!(yaml_str.contains("hello"));
    }

    #[test]
    fn test_value_yaml_output_integer() {
        let value = yaml::Value::Number(yaml::Number::Int(42));
        assert_eq!(value.to_raw_string(), "42");
        assert_eq!(value.to_yaml_string().unwrap().trim(), "42");
    }

    #[test]
    fn test_format_respects_mode() {
        let value = yaml::Value::String("test".to_string());
        // Raw mode returns unquoted
        assert_eq!(value.format(false), "test");
        // YAML mode may quote/format
        let yaml_output = value.format(true);
        assert!(yaml_output.contains("test"));
    }
}
