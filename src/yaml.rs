use crate::tag::{parse_tag, MergeOp, TagError};
use fyaml::document::{Document, FyParser, Parse};
use fyaml::node::{MappingIterator, Node, NodeType};

use std::io::{self, Read};
use std::rc::Rc;

#[derive(Debug)]
pub enum Error {
    FyError(String),
    IoError(String),
    PathError(String),
    TypeError(String),
    BaseError(String),
}

impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::IoError(e.to_string())
    }
}

impl From<String> for Error {
    fn from(e: String) -> Self {
        Error::BaseError(e)
    }
}

impl From<&str> for Error {
    fn from(e: &str) -> Self {
        Error::BaseError(e.to_string())
    }
}

impl From<TagError> for Error {
    fn from(e: TagError) -> Self {
        Error::BaseError(e.to_string())
    }
}

// Implementation of std::fmt::Display
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let e = match self {
            Error::FyError(e)
            | Error::IoError(e)
            | Error::PathError(e)
            | Error::TypeError(e)
            | Error::BaseError(e) => e,
        };
        write!(f, "{}", e)
    }
}

use crate::yaml::Error::*;
use std::collections::HashMap;

/// Merge policy for specific paths
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MergePolicy {
    /// Deep recursive merge (default for mappings)
    Merge,
    /// Replace entirely with overlay value
    Replace,
    /// Prepend overlay sequence to base sequence
    Prepend,
}

impl std::str::FromStr for MergePolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "merge" => Ok(MergePolicy::Merge),
            "replace" => Ok(MergePolicy::Replace),
            "prepend" => Ok(MergePolicy::Prepend),
            _ => Err(format!(
                "Invalid merge policy '{}': expected merge, replace, or prepend",
                s
            )),
        }
    }
}

/// Parse merge policy specifications from CLI arguments
/// Format: "path=policy" where policy is merge|replace|prepend
pub fn parse_merge_policies(
    args: Option<&Vec<String>>,
) -> Result<HashMap<String, MergePolicy>, String> {
    let mut policies = HashMap::new();

    if let Some(specs) = args {
        for spec in specs {
            let parts: Vec<&str> = spec.splitn(2, '=').collect();
            if parts.len() != 2 {
                return Err(format!(
                    "Invalid merge policy '{}': expected format PATH=POLICY",
                    spec
                ));
            }
            let path = parts[0].trim();
            let policy: MergePolicy = parts[1].trim().parse()?;
            policies.insert(path.to_string(), policy);
        }
    }

    Ok(policies)
}

/// Split a dot-notation path into its components.
///
/// Handles escape sequences: `\.` for literal dots, `\\` for literal backslashes.
/// For example, `a.b\.c.d` becomes `["a", "b.c", "d"]`.
fn split_path(path: &str) -> Vec<String> {
    let mut elements = Vec::new();
    let mut escaped = false;
    let mut element = String::new();

    for c in path.chars() {
        if escaped {
            escaped = false;
            element.push(c);
            continue;
        }
        match c {
            '\\' => escaped = true,
            '.' => {
                elements.push(element.clone());
                element.clear();
            }
            _ => element.push(c),
        }
    }
    elements.push(element);
    elements
}

/// Convert a path from the form "foo.bar.0.baz" to "foo/bar/0/baz"
///
/// This is necessary because libfyaml uses '/' as a path separator, but
/// shyaml historically uses '.' as a path separator.
///
/// In shyaml, `\.` is used to escape a literal '.' in a key name, but this
/// does not need to be escaped in libfyaml.
///
/// To the contrary, libfyaml uses `/{}[]` as special characters in a path, so
/// these must be escaped using double quoted strings.
fn convert_path(path: &str) -> String {
    let elements = split_path(path);
    // escape special characters for libfyaml
    let path = elements
        .into_iter()
        .map(|e| {
            let e = e.replace("\\", "\\\\");
            // if e contains any special characters, double quote the string
            if e.contains('/')
                || e.contains('{')
                || e.contains('}')
                || e.contains('[')
                || e.contains(']')
                || e.contains('\\')
                || e.is_empty()
            {
                format!("\"{:}\"", e.replace('"', "\\\""))
            } else {
                e
            }
        })
        .collect::<Vec<String>>()
        .join("/");
    log::trace!("Converted path: {}", path);
    path
}

pub fn get_value(
    path: Option<&str>,
    to_yaml: bool,
) -> Result<impl Iterator<Item = Result<String, Error>> + '_, Error> {
    let parser = Parser::from_stdin()?;
    log::trace!("got parser");

    Ok(parser.traverse(path)?.map(move |node| {
        log::trace!("got node");
        let node = node?;
        if !to_yaml && node.is_scalar() {
            return Ok(node.to_raw_string()?);
        }
        Ok(node.to_string())
    }))
}

fn nt2shyaml(node: &Node) -> Result<String, String> {
    let tag = node.get_tag()?;
    match tag {
        Some(t) => Ok(t.to_string()),
        None => Ok(match node.get_type() {
            NodeType::Scalar => {
                // check if float using regex
                if regex::Regex::new(r"^-?\d+\.\d+$")
                    .unwrap()
                    .is_match(node.to_raw_string()?.as_str())
                {
                    "float"
                } else if regex::Regex::new(r"^-?\d+$")
                    .unwrap()
                    .is_match(node.to_raw_string()?.as_str())
                {
                    "int"
                } else {
                    "str"
                }
            }
            NodeType::Sequence => "sequence",
            NodeType::Mapping => "struct",
        }
        .to_string()),
    }
}

struct YamlDoc {
    _input: String, // keep ref to avoid dropping
    root_node: Rc<Node>,
    doc: Document,
}

impl YamlDoc {
    fn traverse(&self, path: Option<&str>) -> Result<Rc<Node>, Error> {
        // Parse and get root node
        log::trace!("Root node: {:p}", self.root_node);
        log::trace!("Path: {:?}", path);
        match path {
            Some(p) => self
                .root_node
                .node_by_path(&convert_path(p))
                .ok_or(PathError(format!("Path not found: {:?}", p))),
            None => Ok(Rc::clone(&self.root_node)),
        }
    }

    fn load(_input: String) -> Result<Self, Error> {
        let doc = _input.parse::<Document>()?;
        let root_node = Rc::new(doc.root_node().ok_or("Empty YAML document")?);
        let d = YamlDoc {
            _input,
            doc,
            root_node,
        };
        log::trace!("Doc: {:?}", d.doc.to_string());
        //log::trace!("Input {:?}", input);
        Ok(d)
    }
}

struct Parser {
    fy_parser: Rc<FyParser>,
}

impl Parser {
    fn from_stdin() -> Result<Self, String> {
        let fy_parser = FyParser::from_stdin()?;
        Ok(Parser { fy_parser })
    }
    /// traverse applies to each document
    fn traverse<'a>(
        self,
        path: Option<&'a str>,
    ) -> Result<impl Iterator<Item = Result<Rc<Node>, Error>> + 'a, Error> {
        // Parse and get root node
        let doc_iter = self.fy_parser.doc_iter();
        log::trace!("got doc_iter");

        Ok(doc_iter.map(move |doc| {
            log::trace!("got doc");

            let root_node = Rc::new(doc.root_node().ok_or("Empty YAML document")?);
            match path {
                Some(p) => root_node
                    .node_by_path(&convert_path(p))
                    .ok_or(PathError(format!("Path not found: {}", p))),
                None => Ok(root_node),
            }
        }))
    }
}

fn read_stdin() -> Result<String, Error> {
    // Read input from stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    // .expect("Failed to read from stdin");
    log::trace!("Stdin: {:?}", input);
    Ok(input)
}

pub fn get_type(path: Option<&str>) -> Result<String, Error> {
    let doc = YamlDoc::load(read_stdin()?.to_string())?;
    let node = doc.traverse(path)?;

    Ok(nt2shyaml(&node)?)
}

pub fn get_length(path: Option<&str>) -> Result<i32, Error> {
    let doc = YamlDoc::load(read_stdin()?.to_string())?;
    let node = doc.traverse(path)?;

    match node.get_type() {
        NodeType::Scalar => {
            return Err(TypeError(format!(
            "get-length does not support '{}' type. Please provide or select a sequence or struct.",
            nt2shyaml(&node)?
        )))
        }
        NodeType::Sequence => Ok(node.seq_len()?),
        NodeType::Mapping => Ok(node.map_len()?),
    }
}

///
/// use fy_library_version() to get the version of the libfyaml library
pub fn get_version() -> Result<String, String> {
    fyaml::get_c_version()
}

pub fn keys<F>(path: Option<&str>, yaml: bool, cb: F) -> Result<(), Error>
where
    F: FnMut(Result<String, String>) -> Result<(), String>,
{
    let doc = YamlDoc::load(read_stdin()?.to_string())?;
    let node = doc.traverse(path)?;
    if !node.is_mapping() {
        return Err(TypeError(format!(
            "keys does not support '{}' type. Please provide or select a struct.",
            nt2shyaml(&node)?
        )));
    }
    let keys = MapKeyIterator::new(node.map_iter());

    fn node_to_raw_string(node: Result<Node, String>) -> Result<String, String> {
        log::trace!("node_to_raw_string");
        let node = node?;
        if node.is_scalar() {
            return node.to_raw_string();
        }
        Ok(node.to_string())
    }
    fn node_to_string(node: Result<Node, String>) -> Result<String, String> {
        log::trace!("node_to_string");
        Ok(node?.to_string())
    }
    let _ = keys
        .map(if !yaml {
            node_to_raw_string
        } else {
            node_to_string
        })
        .map(cb)
        .collect::<Vec<_>>();
    Ok(())
}

pub fn values<F>(path: Option<&str>, yaml: bool, cb: F) -> Result<(), Error>
where
    F: FnMut(Result<String, String>) -> Result<(), String>,
{
    let doc = YamlDoc::load(read_stdin()?.to_string())?;
    let node = doc.traverse(path)?;
    if !node.is_mapping() {
        return Err(TypeError(format!(
            "values does not support '{}' type. Please provide or select a struct.",
            nt2shyaml(&node)?
        )));
    }

    let keys = MapValueIterator::new(node.map_iter());

    fn node_to_raw_string(node: Result<Node, String>) -> Result<String, String> {
        log::trace!("node_to_raw_string");
        let node = node?;
        if node.is_scalar() {
            return node.to_raw_string();
        }
        Ok(node.to_string())
    }
    fn node_to_string(node: Result<Node, String>) -> Result<String, String> {
        log::trace!("node_to_string");
        Ok(node?.to_string())
    }
    let _ = keys
        .map(if !yaml {
            node_to_raw_string
        } else {
            node_to_string
        })
        .map(cb)
        .collect::<Vec<_>>();
    Ok(())
}

pub fn get_values<F>(path: Option<&str>, yaml: bool, mut cb: F) -> Result<(), Error>
where
    F: FnMut(Result<String, String>) -> Result<(), String>,
{
    let doc = YamlDoc::load(read_stdin()?.to_string())?;
    let node = doc.traverse(path)?;
    if !node.is_sequence() && !node.is_mapping() {
        return Err(TypeError(format!(
            "get-values does not support '{}' type. Please provide or select a sequence or struct.",
            nt2shyaml(&node)?
        )));
    }
    if node.is_mapping() {
        let pairs = node.map_iter();

        fn pair_to_raw_string(
            pair: Result<(Node, Node), String>,
        ) -> Result<(String, String), String> {
            log::trace!("pair_to_raw_string");
            let (key, value) = pair?;
            Ok((node_to_raw_string(Ok(key))?, node_to_raw_string(Ok(value))?))
        }
        fn pair_to_string(pair: Result<(Node, Node), String>) -> Result<(String, String), String> {
            log::trace!("pair_to_raw_string");
            let (key, value) = pair?;
            Ok((key.to_string(), value.to_string()))
        }
        let _ = pairs
            .map(if !yaml {
                pair_to_raw_string
            } else {
                pair_to_string
            })
            .map(|k| {
                let (k, v) = k?;
                let _ = cb(Ok(k));
                let _ = cb(Ok(v));
                Ok::<(), String>(())
            })
            .collect::<Vec<_>>();
        return Ok(());
    };

    let values = node.seq_iter();

    fn node_to_raw_string(node: Result<Node, String>) -> Result<String, String> {
        log::trace!("node_to_raw_string");
        let node = node?;
        if node.is_scalar() {
            return node.to_raw_string();
        }
        Ok(node.to_string())
    }
    fn node_to_string(node: Result<Node, String>) -> Result<String, String> {
        log::trace!("node_to_string");
        Ok(node?.to_string())
    }
    let _ = values
        .map(if !yaml {
            node_to_raw_string
        } else {
            node_to_string
        })
        .map(cb)
        .collect::<Vec<_>>();
    Ok(())
}

fn node_to_raw_string(node: Node) -> Result<String, String> {
    log::trace!("node_to_raw_string");
    if node.is_scalar() {
        return node.to_raw_string();
    }
    Ok(node.to_string())
}

pub fn key_values<F>(path: Option<&str>, yaml: bool, cb: F) -> Result<(), Error>
where
    F: FnMut(Result<(String, String), String>) -> Result<(), String>,
{
    let doc = YamlDoc::load(read_stdin()?.to_string())?;
    let node = doc.traverse(path)?;
    if !node.is_mapping() {
        return Err(TypeError(format!(
            "key-values does not support '{}' type. Please provide or select a struct.",
            nt2shyaml(&node)?
        )));
    }

    let pairs = node.map_iter();

    fn pair_to_raw_string(pair: Result<(Node, Node), String>) -> Result<(String, String), String> {
        log::trace!("pair_to_raw_string");
        let (key, value) = pair?;
        Ok((node_to_raw_string(key)?, node_to_raw_string(value)?))
    }
    fn pair_to_string(pair: Result<(Node, Node), String>) -> Result<(String, String), String> {
        log::trace!("pair_to_raw_string");
        let (key, value) = pair?;
        Ok((key.to_string(), value.to_string()))
    }
    let _ = pairs
        .map(if !yaml {
            pair_to_raw_string
        } else {
            pair_to_string
        })
        .map(cb)
        .collect::<Vec<_>>();
    Ok(())
}

struct MapKeyIterator<'a> {
    map_iter: MappingIterator<'a>,
}

impl<'a> MapKeyIterator<'a> {
    fn new(map_iter: MappingIterator<'a>) -> MapKeyIterator<'a> {
        MapKeyIterator { map_iter }
    }
}

impl<'a> Iterator for MapKeyIterator<'a> {
    type Item = Result<Node, String>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.map_iter.next() {
            Some(Ok((key, _))) => Some(Ok(key)),
            Some(Err(e)) => Some(Err(e.to_string())),
            None => None,
        }
    }
}

struct MapValueIterator<'a> {
    map_iter: MappingIterator<'a>,
}

impl<'a> MapValueIterator<'a> {
    fn new(map_iter: MappingIterator<'a>) -> MapValueIterator<'a> {
        MapValueIterator { map_iter }
    }
}

impl<'a> Iterator for MapValueIterator<'a> {
    type Item = Result<Node, String>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.map_iter.next() {
            Some(Ok((_, value))) => Some(Ok(value)),
            Some(Err(e)) => Some(Err(e.to_string())),
            None => None,
        }
    }
}

/// Merge two YAML values according to merge rules:
/// - Scalars: child replaces parent
/// - Mappings: deep recursive merge
/// - Sequences: append child to parent with deduplication
///
/// Merge policies can override default behavior for specific paths:
/// - Replace: overlay completely replaces base
/// - Prepend: overlay sequence is prepended to base sequence
/// - Merge: default deep merge behavior

fn extract_merge_directive(value: fyaml::Value) -> Result<(Option<MergeOp>, fyaml::Value), Error> {
    use fyaml::value::TaggedValue;
    use fyaml::Value;

    match value {
        Value::Tagged(tagged) => {
            let parsed = parse_tag(&tagged.tag)?;
            let stripped_value = match parsed.remaining {
                Some(remaining_tag) => Value::Tagged(Box::new(TaggedValue {
                    tag: remaining_tag,
                    value: tagged.value,
                })),
                None => tagged.value,
            };
            Ok((parsed.merge_op, stripped_value))
        }
        other => Ok((None, other)),
    }
}

fn validate_merge_op_for_type(op: &MergeOp, value: &fyaml::Value, path: &str) -> Result<(), Error> {
    if !matches!(op, MergeOp::Append | MergeOp::Prepend) {
        return Ok(());
    }

    let inner = match value {
        fyaml::Value::Tagged(t) => &t.value,
        other => other,
    };

    if matches!(inner, fyaml::Value::Sequence(_)) {
        return Ok(());
    }

    let location = if path.is_empty() {
        "at root".to_string()
    } else {
        format!("at '{}'", path)
    };

    Err(TypeError(format!(
        "Invalid merge directive {}: !merge:{} can only be used on sequences, got {}",
        location,
        op,
        value_type_name(inner)
    )))
}

/// Merge two YAML values according to merge rules:
/// - Scalars: child replaces parent
/// - Mappings: deep recursive merge
/// - Sequences: append child to parent with deduplication
///
/// Merge policies can be specified via:
/// 1. CLI arguments (--merge-policy PATH=POLICY) - highest priority
/// 2. Inline tags in overlay (!merge:replace, !merge:append, !merge:prepend)
/// 3. Default behavior (merge for mappings, append for sequences, replace for scalars)
///
/// Inline merge tags are always stripped from the output.
fn merge_values(
    base: fyaml::Value,
    overlay: fyaml::Value,
    path: &str,
    policies: &HashMap<String, MergePolicy>,
) -> Result<fyaml::Value, Error> {
    // Extract merge directive from overlay (if any) and strip the merge tag
    let (inline_op, stripped_overlay) = extract_merge_directive(overlay)?;

    // Determine effective policy: CLI > inline tag > default
    let cli_policy = policies.get(path);

    if let Some(policy) = cli_policy {
        return apply_policy(*policy, base, stripped_overlay, path, policies);
    }

    if let Some(op) = inline_op {
        validate_merge_op_for_type(&op, &stripped_overlay, path)?;

        let policy = match op {
            MergeOp::Replace => MergePolicy::Replace,
            MergeOp::Append => MergePolicy::Merge,
            MergeOp::Prepend => MergePolicy::Prepend,
        };
        return apply_policy(policy, base, stripped_overlay, path, policies);
    }

    // Default behavior with stripped overlay
    apply_default_merge(base, stripped_overlay, path, policies)
}

/// Apply a specific merge policy
fn apply_policy(
    policy: MergePolicy,
    base: fyaml::Value,
    overlay: fyaml::Value,
    path: &str,
    policies: &HashMap<String, MergePolicy>,
) -> Result<fyaml::Value, Error> {
    use fyaml::value::TaggedValue;
    use fyaml::Value;

    match policy {
        MergePolicy::Replace => Ok(overlay),
        MergePolicy::Prepend => {
            let (overlay_tag, overlay_inner) = match &overlay {
                Value::Tagged(t) => (Some(&t.tag), &t.value),
                other => (None, other),
            };
            let base_inner = match &base {
                Value::Tagged(t) => &t.value,
                other => other,
            };

            if let (Value::Sequence(base_seq), Value::Sequence(overlay_seq)) =
                (base_inner, overlay_inner)
            {
                let mut result = overlay_seq.clone();
                for elt in base_seq {
                    if !result.contains(elt) {
                        result.push(elt.clone());
                    }
                }
                let result_value = Value::Sequence(result);
                return Ok(match overlay_tag {
                    Some(tag) => Value::Tagged(Box::new(TaggedValue {
                        tag: tag.clone(),
                        value: result_value,
                    })),
                    None => result_value,
                });
            }
            Ok(overlay)
        }
        MergePolicy::Merge => apply_default_merge(base, overlay, path, policies),
    }
}

/// Apply default merge behavior based on types
fn apply_default_merge(
    base: fyaml::Value,
    overlay: fyaml::Value,
    path: &str,
    policies: &HashMap<String, MergePolicy>,
) -> Result<fyaml::Value, Error> {
    use fyaml::Value;

    // Get the inner value if overlay is tagged (for type comparison)
    // Note: merge tags already stripped, but other tags may remain
    let overlay_inner = match &overlay {
        Value::Tagged(t) => &t.value,
        other => other,
    };

    // Get the inner value if base is tagged (for type comparison)
    let base_inner = match &base {
        Value::Tagged(t) => &t.value,
        other => other,
    };

    match (base_inner, overlay_inner) {
        // Both null - return overlay (which might be tagged null)
        (Value::Null, Value::Null) => Ok(overlay),

        // Overlay is null - return base unchanged
        // Note: null-deletes-key is handled in mapping merge logic
        (_, Value::Null) => Ok(base),

        // Base is null - return overlay
        (Value::Null, _) => Ok(overlay),

        // Both are mappings - deep merge
        (Value::Mapping(_), Value::Mapping(_)) => {
            // Extract the actual mappings (handling tags)
            let base_map = match base {
                Value::Mapping(m) => m,
                Value::Tagged(t) => match t.value {
                    Value::Mapping(m) => m,
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            };
            let overlay_map = match overlay {
                Value::Mapping(m) => m,
                Value::Tagged(t) => match t.value {
                    Value::Mapping(m) => m,
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            };

            let mut result = base_map;
            for (key, overlay_value) in overlay_map {
                // Legacy behavior: explicit null value deletes the key
                let is_null = match &overlay_value {
                    Value::Null => true,
                    Value::Tagged(t) => matches!(t.value, Value::Null),
                    _ => false,
                };
                if is_null {
                    result.shift_remove(&key);
                    continue;
                }

                let key_str = match &key {
                    Value::String(s) => s.clone(),
                    _ => format!("{:?}", key),
                };
                let new_path = if path.is_empty() {
                    key_str
                } else {
                    format!("{}.{}", path, key_str)
                };

                let merged_value = if let Some(base_value) = result.get(&key) {
                    merge_values(base_value.clone(), overlay_value, &new_path, policies)?
                } else {
                    // New key - still need to strip any merge tags
                    let (_, stripped) = extract_merge_directive(overlay_value)?;
                    stripped
                };
                result.insert(key, merged_value);
            }
            Ok(Value::Mapping(result))
        }

        // Both are sequences - append with deduplication (legacy behavior)
        (Value::Sequence(_), Value::Sequence(_)) => {
            // Extract the actual sequences (handling tags)
            let base_seq = match base {
                Value::Sequence(s) => s,
                Value::Tagged(t) => match t.value {
                    Value::Sequence(s) => s,
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            };
            let overlay_seq = match overlay {
                Value::Sequence(s) => s,
                Value::Tagged(t) => match t.value {
                    Value::Sequence(s) => s,
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            };

            let mut result = base_seq;
            for elt in overlay_seq {
                if let Some(pos) = result.iter().position(|x| x == &elt) {
                    result.remove(pos);
                }
                result.push(elt);
            }
            Ok(Value::Sequence(result))
        }

        // Scalars (same or different types) - replace
        (Value::Bool(_), Value::Bool(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::String(_), Value::String(_))
        | (Value::Bool(_), Value::Number(_))
        | (Value::Bool(_), Value::String(_))
        | (Value::Number(_), Value::Bool(_))
        | (Value::Number(_), Value::String(_))
        | (Value::String(_), Value::Bool(_))
        | (Value::String(_), Value::Number(_)) => Ok(overlay),

        // Type mismatch (mapping vs sequence, scalar vs collection, etc.)
        _ => {
            let base_type = value_type_name(base_inner);
            let overlay_type = value_type_name(overlay_inner);
            let location = if path.is_empty() {
                "at root".to_string()
            } else {
                format!("at '{}'", path)
            };
            Err(TypeError(format!(
                "Type mismatch {}: cannot merge {} with {}",
                location, base_type, overlay_type
            )))
        }
    }
}

fn value_type_name(value: &fyaml::Value) -> &'static str {
    use fyaml::Value;
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Sequence(_) => "sequence",
        Value::Mapping(_) => "mapping",
        Value::Tagged(_) => "tagged",
    }
}

pub fn set_value(key: &str, value: &str, parse_as_yaml: bool) -> Result<String, Error> {
    let base_str = read_stdin()?;
    let mut base: fyaml::Value = if base_str.trim().is_empty() {
        fyaml::Value::Mapping(Default::default())
    } else {
        base_str
            .parse()
            .map_err(|e| BaseError(format!("Failed to parse base YAML: {}", e)))?
    };

    let new_value: fyaml::Value = if parse_as_yaml {
        value
            .parse()
            .map_err(|e| BaseError(format!("Failed to parse value as YAML: {}", e)))?
    } else {
        fyaml::Value::String(value.to_string())
    };

    set_value_at_path(&mut base, key, new_value)?;

    let output = base
        .to_yaml_string()
        .map_err(|e| BaseError(format!("Failed to serialize result: {}", e)))?;

    Ok(output)
}

fn resolve_sequence_index(path: &str, part: &str, seq_len: usize) -> Result<usize, Error> {
    let idx: i64 = part.parse().map_err(|_| {
        PathError(format!(
            "invalid path '{}', non-integer index '{}' provided on a sequence.",
            path, part
        ))
    })?;

    let resolved = if idx < 0 {
        let abs_idx = (-idx) as usize;
        if abs_idx > seq_len {
            return Err(PathError(format!(
                "invalid path '{}', index {} is out of range ({} elements in sequence).",
                path, idx, seq_len
            )));
        }
        seq_len - abs_idx
    } else {
        idx as usize
    };

    if resolved >= seq_len {
        return Err(PathError(format!(
            "invalid path '{}', index {} is out of range ({} elements in sequence).",
            path, idx, seq_len
        )));
    }

    Ok(resolved)
}

fn set_value_at_path(
    root: &mut fyaml::Value,
    path: &str,
    value: fyaml::Value,
) -> Result<(), Error> {
    use fyaml::Value;

    let path_parts = split_path(path);

    if path_parts.is_empty() {
        return Err(PathError("Empty path".to_string()));
    }

    let mut current = root;

    for (i, part) in path_parts.iter().enumerate() {
        let is_last = i == path_parts.len() - 1;

        if is_last {
            match current {
                Value::Mapping(map) => {
                    map.insert(Value::String(part.clone()), value);
                    return Ok(());
                }
                Value::Sequence(seq) => {
                    let idx = resolve_sequence_index(path, part, seq.len())?;
                    seq[idx] = value;
                    return Ok(());
                }
                _ => {
                    return Err(PathError(format!(
                        "invalid path '{}', cannot set value on scalar at '{}'.",
                        path, part
                    )));
                }
            }
        }

        match current {
            Value::Mapping(map) => {
                let key = Value::String(part.clone());
                if !map.contains_key(&key) {
                    map.insert(key.clone(), Value::Mapping(Default::default()));
                }
                current = map.get_mut(&key).unwrap();
            }
            Value::Sequence(seq) => {
                let idx = resolve_sequence_index(path, part, seq.len())?;
                current = &mut seq[idx];
            }
            _ => {
                return Err(PathError(format!(
                    "invalid path '{}', cannot traverse scalar at '{}'.",
                    path, part
                )));
            }
        }
    }

    Ok(())
}

pub fn del(key: &str) -> Result<String, Error> {
    let base_str = read_stdin()?;
    let mut base: fyaml::Value = if base_str.trim().is_empty() {
        return Err(PathError("Cannot delete from empty document".to_string()));
    } else {
        base_str
            .parse()
            .map_err(|e| BaseError(format!("Failed to parse base YAML: {}", e)))?
    };

    del_at_path(&mut base, key)?;

    let output = base
        .to_yaml_string()
        .map_err(|e| BaseError(format!("Failed to serialize result: {}", e)))?;

    Ok(output)
}

fn del_at_path(root: &mut fyaml::Value, path: &str) -> Result<(), Error> {
    use fyaml::Value;

    let path_parts = split_path(path);

    if path_parts.is_empty() || (path_parts.len() == 1 && path_parts[0].is_empty()) {
        return Err(PathError("Empty path".to_string()));
    }

    let mut current = root;

    for (i, part) in path_parts.iter().enumerate() {
        let is_last = i == path_parts.len() - 1;

        if is_last {
            return match current {
                Value::Mapping(map) => {
                    let key = Value::String(part.clone());
                    if map.shift_remove(&key).is_none() {
                        Err(PathError(format!(
                            "invalid path '{}', missing key '{}' in struct.",
                            path, part
                        )))
                    } else {
                        Ok(())
                    }
                }
                Value::Sequence(seq) => {
                    let idx = parse_seq_index(part, seq.len(), path)?;
                    seq.remove(idx);
                    Ok(())
                }
                _ => Err(PathError(format!(
                    "invalid path '{}', cannot delete from scalar.",
                    path
                ))),
            };
        }

        current = match current {
            Value::Mapping(map) => {
                let key = Value::String(part.clone());
                map.get_mut(&key).ok_or_else(|| {
                    PathError(format!(
                        "invalid path '{}', missing key '{}' in struct.",
                        path, part
                    ))
                })?
            }
            Value::Sequence(seq) => {
                let idx = parse_seq_index(part, seq.len(), path)?;
                &mut seq[idx]
            }
            _ => {
                return Err(PathError(format!(
                    "invalid path '{}', cannot traverse scalar at '{}'.",
                    path, part
                )));
            }
        };
    }

    Ok(())
}

fn parse_seq_index(part: &str, len: usize, path: &str) -> Result<usize, Error> {
    let idx: i64 = part.parse().map_err(|_| {
        PathError(format!(
            "invalid path '{}', non-integer index '{}' provided on a sequence.",
            path, part
        ))
    })?;
    let resolved = if idx < 0 { len as i64 + idx } else { idx };
    if resolved < 0 || resolved >= len as i64 {
        return Err(PathError(format!(
            "invalid path '{}', index {} is out of range ({} elements in sequence).",
            path, idx, len
        )));
    }
    Ok(resolved as usize)
}
pub fn apply(
    overlay_paths: &[String],
    policies: &HashMap<String, MergePolicy>,
) -> Result<String, Error> {
    // Read base from stdin
    let base_str = read_stdin()?;
    let mut result: fyaml::Value = if base_str.trim().is_empty() {
        fyaml::Value::Null
    } else {
        base_str
            .parse()
            .map_err(|e| BaseError(format!("Failed to parse base YAML: {}", e)))?
    };

    // Apply each overlay in order
    for overlay_path in overlay_paths {
        let overlay_str = std::fs::read_to_string(overlay_path)
            .map_err(|e| IoError(format!("Failed to read '{}': {}", overlay_path, e)))?;

        let overlay: fyaml::Value = if overlay_str.trim().is_empty() {
            fyaml::Value::Null
        } else {
            overlay_str
                .parse()
                .map_err(|e| BaseError(format!("Failed to parse '{}': {}", overlay_path, e)))?
        };

        result = merge_values(result, overlay, "", policies)?;
    }

    // Serialize result to YAML
    let output = result
        .to_yaml_string()
        .map_err(|e| BaseError(format!("Failed to serialize result: {}", e)))?;

    Ok(output)
}
