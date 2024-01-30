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
    let mut elements: Vec<String> = Vec::new();
    // first separate by . if not escaped
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
    elements.push(element.clone());
    // now escape special characters for libfyaml in one go
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
fn merge_values(
    base: fyaml::Value,
    overlay: fyaml::Value,
    path: &str,
    policies: &HashMap<String, MergePolicy>,
) -> Result<fyaml::Value, Error> {
    use fyaml::Value;

    // Check if there's a specific policy for this path
    if let Some(policy) = policies.get(path) {
        match policy {
            MergePolicy::Replace => return Ok(overlay),
            MergePolicy::Prepend => {
                // Prepend only makes sense for sequences
                if let (Value::Sequence(base_seq), Value::Sequence(overlay_seq)) = (&base, &overlay)
                {
                    let mut result = overlay_seq.clone();
                    for elt in base_seq {
                        if !result.contains(elt) {
                            result.push(elt.clone());
                        }
                    }
                    return Ok(Value::Sequence(result));
                }
                // For non-sequences, prepend acts like replace
                return Ok(overlay);
            }
            MergePolicy::Merge => {
                // Continue with default merge behavior below
            }
        }
    }

    match (&base, &overlay) {
        // Both null - return null
        (Value::Null, Value::Null) => Ok(Value::Null),

        // Overlay is null - return base unchanged (at merge level)
        // Note: null-deletes-key is handled in mapping merge logic
        (_, Value::Null) => Ok(base),

        // Base is null - return overlay
        (Value::Null, _) => Ok(overlay),

        // Both are mappings - deep merge
        (Value::Mapping(base_map), Value::Mapping(overlay_map)) => {
            let mut result = base_map.clone();
            for (key, overlay_value) in overlay_map {
                // Legacy behavior: explicit null value deletes the key
                if *overlay_value == Value::Null {
                    result.shift_remove(key);
                    continue;
                }

                let key_str = match key {
                    Value::String(s) => s.clone(),
                    _ => format!("{:?}", key),
                };
                let new_path = if path.is_empty() {
                    key_str.clone()
                } else {
                    format!("{}.{}", path, key_str)
                };

                let merged_value = if let Some(base_value) = result.get(key) {
                    merge_values(
                        base_value.clone(),
                        overlay_value.clone(),
                        &new_path,
                        policies,
                    )?
                } else {
                    overlay_value.clone()
                };
                result.insert(key.clone(), merged_value);
            }
            Ok(Value::Mapping(result))
        }

        // Both are sequences - append with deduplication (legacy behavior)
        // Duplicates from base are removed, then overlay elements appended at end
        (Value::Sequence(base_seq), Value::Sequence(overlay_seq)) => {
            let mut result = base_seq.clone();
            for elt in overlay_seq {
                if let Some(pos) = result.iter().position(|x| x == elt) {
                    result.remove(pos);
                }
                result.push(elt.clone());
            }
            Ok(Value::Sequence(result))
        }

        // Both are scalars (or same type) - replace
        (Value::Bool(_), Value::Bool(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::String(_), Value::String(_)) => Ok(overlay),

        // Mixed scalar types - replace (scalar replaces scalar)
        (Value::Bool(_), Value::Number(_))
        | (Value::Bool(_), Value::String(_))
        | (Value::Number(_), Value::Bool(_))
        | (Value::Number(_), Value::String(_))
        | (Value::String(_), Value::Bool(_))
        | (Value::String(_), Value::Number(_)) => Ok(overlay),

        // Tagged values - overlay wins (replace)
        // This includes: tagged+tagged, tagged+scalar, scalar+tagged
        (Value::Tagged(_), _) | (_, Value::Tagged(_)) => Ok(overlay),

        // Type mismatch (mapping vs sequence, scalar vs collection, etc.)
        _ => {
            let base_type = value_type_name(&base);
            let overlay_type = value_type_name(&overlay);
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

/// Apply overlay files to base YAML from stdin
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
