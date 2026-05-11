#![allow(clippy::items_after_test_module)]
#![allow(clippy::single_match)]
#![allow(clippy::single_char_pattern)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::ptr_arg)]
//! # roxmltree_to_serde
//! Fast and flexible conversion from XML to JSON using [quick-xml](https://github.com/tafia/quick-xml)
//! and [serde](https://github.com/serde-rs/json). Inspired by [node2object](https://github.com/vorot93/node2object).
//!
//! This crate converts XML elements, attributes and text nodes directly into corresponding JSON structures.
//! Some common usage scenarios would be converting XML into JSON for loading into No-SQL databases
//! or sending it to the front end application.
//!
//! Because of the richness and flexibility of XML some conversion behavior is configurable:
//! - attribute name prefixes
//! - naming of text nodes
//! - number format conversion
//!
//! ## Usage example
//! ```
//! extern crate roxmltree_to_serde;
//! extern crate serde_json;
//! use roxmltree_to_serde::{xml_string_to_json, Config, NullValue};
//!
//! fn main() {
//!    let xml = r#"<a attr1="1"><b><c attr2="001">some text</c></b></a>"#;
//!    let conf = Config::new_with_defaults();
//!    let json = xml_string_to_json(xml.to_owned(), &conf).expect("Malformed XML");
//!    println!("{json}");
//!
//!    let conf = Config::new_with_custom_values(true, "", "txt", NullValue::Null, false);
//!    let json = xml_string_to_json(xml.to_owned(), &conf).expect("Malformed XML");
//!    println!("{json}");
//! }
//! ```
//! * **Output with the default config:** `{"a":{"@attr1":1,"b":{"c":{"#text":"some text","@attr2":1}}}}`
//! * **Output with a custom config:** `{"a":{"attr1":1,"b":{"c":{"attr2":"001","txt":"some text"}}}}`
//!
//! ## Additional features
//! Use `roxmltree_to_serde = { version = "0.4", features = ["json_types"] }` to enable support for enforcing JSON types
//! for some XML nodes using xPath-like notations. Example for enforcing attribute `attr2` from the snippet above
//! as JSON String regardless of its contents:
//! ```
//! use roxmltree_to_serde::{Config, JsonArray, JsonType};
//!
//! #[cfg(feature = "json_types")]
//! let conf = Config::new_with_defaults()
//!            .add_json_type_override("/a/b/c/@attr2", JsonArray::Infer(JsonType::AlwaysString));
//! ```
//!
//! ## Detailed documentation
//! See [README](https://github.com/marcomq/roxmltree_to_serde) in the source repo for more examples, limitations and detailed behavior description.
//!
//! ## Testing your XML files
//!
//! If you want to see how your XML files are converted into JSON, place them into `./test_xml_files` directory
//! and run `cargo test`. They will be converted into JSON and saved in the saved directory.

extern crate roxmltree;
extern crate serde_json;

#[cfg(feature = "regex_path")]
extern crate regex;

use serde_json::{Map as GenericMap, Number, Value};
type Map = GenericMap<String, Value>;
use std::collections::HashMap;

#[cfg(feature = "regex_path")]
use regex::Regex;

#[cfg(test)]
mod tests;

/// Defines how empty elements like `<x />` should be handled.
/// `Ignore` -> exclude from JSON, `Null` -> `"x":null`, EmptyObject -> `"x":{}`.
/// `EmptyObject` is the default option and is how it was handled prior to v.0.4
/// Using `Ignore` on an XML document with an empty root element falls back to `Null` option.
/// E.g. both `<a><x/></a>` and `<a/>` are converted into `{"a":null}`.
#[derive(Debug)]
pub enum NullValue {
    Ignore,
    Null,
    EmptyObject,
}

/// Defines how the values of this Node should be converted into a JSON array with the underlying types.
/// * `Infer` - the nodes are converted into a JSON array only if there are multiple identical elements.
///   E.g. `<a><b>1</b></a>` becomes a map `{"a": {"b": 1 }}` and `<a><b>1</b><b>2</b><b>3</b></a>` becomes
///   an array `{"a": {"b": [1, 2, 3] }}`
/// * `Always` - the nodes are converted into a JSON array regardless of how many there are.
///   E.g. `<a><b>1</b></a>` becomes an array with a single value `{"a": {"b": [1] }}` and
///   `<a><b>1</b><b>2</b><b>3</b></a>` also becomes an array `{"a": {"b": [1, 2, 3] }}`
/// * `PlaceSingletonIntoArray` - the nodes are converted into a JSON array with the specified name
///   regardless of how many there are, and the value of each node is placed inside an object with the name
///   of the original node as the key.
///   E.g. when `array_name` is set to "consonants" and both `/a/b` and `/a/d` match,
///   `<a><b>1</b></a>` becomes `{"a": {"consonants": [{"b": 1 }] }}` and
///   `<a><b>1</b><b>2</b><d>3</d></a>` becomes `{"a": {"consonants": [{"b": 1 }, {"b": 2}, {"d": 3}] }}`
#[derive(Debug)]
pub enum JsonArray {
    /// Convert the nodes into a JSON array even if there is only one element.
    /// e.g. when matching `/a/b` in `<a><b>1</b></a>` the output will be `{"a": {"b": [1] }}`,
    /// or when matching `/a/b` in `<a><b>1</b><b>2</b><b>3</b></a>` the output will be `{"a": {"b": [1, 2, 3] }}`.
    Always(JsonType),
    /// Convert the nodes into a JSON array only if there are multiple identical elements
    /// e.g. when matching `/a/b` in `<a><b>1</b></a>` the output will be `{"a": {"b": 1 }}`,
    /// or when matching `/a/b` in `<a><b>1</b><b>2</b><b>3</b></a>` the output will be `{"a": {"b": [1, 2, 3] }}`.
    Infer(JsonType),
    /// e.g. when matching `/a/b` in `<a><b>1</b></a>` with `array_name: "consonants"` the output will be `{"a": {"consonants": [{"b": 1 }] }}`,
    /// or when matching both `/a/b` and `/a/d` in `<a><b>1</b><b>2</b><d>3</d></a>` with `array_name: "consonants"` the output will be `{"a": {"consonants": [{"b": 1 }, {"b": 2}, {"d": 3}] }}`.
    PlaceSingletonIntoArray { ty: JsonType, array_name: String, },
}

impl JsonArray {
    fn scalar_type(&self) -> &JsonType {
        match self {
            | JsonArray::Always(ty)
            | JsonArray::Infer(ty)
            | JsonArray::PlaceSingletonIntoArray { ty, .. }
            => ty,
        }
    }
}

/// Used as a parameter for `Config.add_json_type_override`. Defines how the XML path should be matched
/// in order to apply the JSON type overriding rules. This enumerator exists to allow the same function
/// to be used for multiple different types of path matching rules.
#[derive(Debug)]
pub enum PathMatcher {
    /// An absolute path starting with a leading slash (`/`). E.g. `/a/b/c/@d`.
    /// It's implicitly converted from `&str` and automatically includes the leading slash.
    Absolute(String),
    /// A regex that will be checked against the XML path. E.g. `(\w/)*c$`.
    /// It's implicitly converted from `regex::Regex`.
    #[cfg(feature = "regex_path")]
    Regex(Regex),
}

// For retro-compatibility and for syntax's sake, a string may be coerced into an absolute path.
impl From<&str> for PathMatcher {
    fn from(value: &str) -> Self {
        let path_with_leading_slash = if value.starts_with("/") {
            value.into()
        } else {
            ["/", value].concat()
        };

        PathMatcher::Absolute(path_with_leading_slash)
    }
}

// ... While a Regex may be coerced into a regex path.
#[cfg(feature = "regex_path")]
impl From<Regex> for PathMatcher {
    fn from(value: Regex) -> Self {
        PathMatcher::Regex(value)
    }
}

/// Defines which data type to apply in JSON format for consistency of output.
/// E.g., the range of XML values for the same node type may be `1234`, `001234`, `AB1234`.
/// It is impossible to guess with 100% consistency which data type to apply without seeing
/// the entire range of values. Use this enum to tell the converter which data type should
/// be applied.
#[derive(Debug, PartialEq, Clone)]
pub enum JsonType {
    /// Do not try to infer the type and convert the value to JSON string.
    /// E.g. convert `<a>1234</a>` into `{"a":"1234"}` or `<a>true</a>` into `{"a":"true"}`
    AlwaysString,
    /// Convert values included in this member into JSON bool `true` and any other value into `false`.
    /// E.g. `Bool(vec!["True", "true", "TRUE"])` will result in any of these values to become JSON bool `true`.
    Bool(Vec<&'static str>),
    /// Attempt to infer the type by looking at the single value of the node being converted.
    /// Not guaranteed to be consistent across multiple nodes.
    /// E.g. convert `<a>1234</a>` and `<a>001234</a>` into `{"a":1234}`, or `<a>true</a>` into `{"a":true}`
    /// Check if your values comply with JSON data types (case, range, format) to produce the expected result.
    Infer,
}

/// Tells the converter how to perform certain conversions.
/// See docs for individual fields for more info.
#[derive(Debug)]
pub struct Config {
    /// Numeric values starting with 0 will be treated as strings.
    /// E.g. convert `<agent>007</agent>` into `"agent":"007"` or `"agent":7`
    /// Defaults to `false`.
    pub leading_zero_as_string: bool,
    /// Prefix XML attribute names with this value to distinguish them from XML elements.
    /// E.g. set it to `@` for `<x a="Hello!" />` to become `{"x": {"@a":"Hello!"}}`
    /// or set it to a blank string for `{"x": {"a":"Hello!"}}`
    /// Defaults to `@`.
    pub xml_attr_prefix: String,
    /// A property name for XML text nodes.
    /// E.g. set it to `text` for `<x a="Hello!">Goodbye!</x>` to become `{"x": {"@a":"Hello!", "text":"Goodbye!"}}`
    /// XML nodes with text only and no attributes or no child elements are converted into JSON properties with the
    /// name of the element. E.g. `<x>Goodbye!</x>` becomes `{"x":"Goodbye!"}`
    /// Defaults to `#text`
    pub xml_text_node_prop_name: String,
    /// Defines how empty elements like `<x />` should be handled.
    pub empty_element_handling: NullValue,
    /// Allow DTD parsing.
    ///
    /// When set to `false`, XML with DTD will cause an error.
    /// Empty DTD block is not an error.
    ///
    /// Currently, there is no option to simply skip DTD.
    /// Mainly because you will get `UnknownEntityReference` error later anyway.
    ///
    /// This flag is set to `false` by default for security reasons,
    /// but `roxmltree` still has checks for billion laughs attack,
    /// so this is just an extra security measure.
    ///
    /// Default: false
    pub allow_dtd: bool,
    pub process_xpointer_xincludes: bool,
    /// A map of XML paths with their JsonArray overrides. They take precedence over the document-wide `json_type`
    /// property. The path syntax is based on xPath: literal element names and attribute names prefixed with `@`.
    /// The path must start with a leading `/`. It is a bit of an inconvenience to remember about it, but it saves
    /// an extra `if`-check in the code to improve the performance.
    /// # Example
    /// - **XML**: `<a><b c="123">007</b></a>`
    /// - path for `c`: `/a/b/@c`
    /// - path for `b` text node (007): `/a/b`
    #[cfg(feature = "json_types")]
    pub json_type_overrides: HashMap<String, JsonArray>,
    /// A list of pairs of regex and JsonArray overrides. They take precedence over both the document-wide `json_type`
    /// property and the `json_type_overrides` property. The path syntax is based on xPath just like `json_type_overrides`.
    #[cfg(feature = "regex_path")]
    pub json_regex_type_overrides: Vec<(Regex, JsonArray)>,
}

impl Config {
    /// Numbers with leading zero will be treated as numbers.
    /// Prefix XML Attribute names with `@`
    /// Name XML text nodes `#text` for XML Elements with other children
    #[must_use]
    pub fn new_with_defaults() -> Self {
        Config {
            leading_zero_as_string: false,
            xml_attr_prefix: "@".to_owned(),
            xml_text_node_prop_name: "#text".to_owned(),
            empty_element_handling: NullValue::EmptyObject,
            allow_dtd: false,
            process_xpointer_xincludes: false,
            #[cfg(feature = "json_types")]
            json_type_overrides: HashMap::new(),
            #[cfg(feature = "regex_path")]
            json_regex_type_overrides: Vec::new(),
        }
    }

    /// Create a Config object with non-default values. See the `Config` struct docs for more info.
    #[must_use]
    pub fn new_with_custom_values(
        leading_zero_as_string: bool,
        xml_attr_prefix: &str,
        xml_text_node_prop_name: &str,
        empty_element_handling: NullValue,
        allow_dtd: bool,
        process_xpointer_xincludes: bool,
    ) -> Self {
        Config {
            leading_zero_as_string,
            xml_attr_prefix: xml_attr_prefix.to_owned(),
            xml_text_node_prop_name: xml_text_node_prop_name.to_owned(),
            empty_element_handling,
            allow_dtd,
            process_xpointer_xincludes,
            #[cfg(feature = "json_types")]
            json_type_overrides: HashMap::new(),
            #[cfg(feature = "regex_path")]
            json_regex_type_overrides: Vec::new(),
        }
    }

    /// Adds a single JSON Type override rule to the current config.
    /// # Example
    /// - **XML**: `<a><b c="123">007</b></a>`
    /// - path for `c`: `/a/b/@c`
    /// - path for `b` text node (007): `/a/b`
    /// - regex path for any `element` node: `(\w/)*element$` [requires `regex_path` feature]
    #[cfg(feature = "json_types")]
    #[must_use]
    pub fn add_json_type_override<P>(self, path: P, json_type: JsonArray) -> Self
    where
        P: Into<PathMatcher>,
    {
        let mut conf = self;

        match path.into() {
            PathMatcher::Absolute(path) => {
                conf.json_type_overrides.insert(path, json_type);
            }
            #[cfg(feature = "regex_path")]
            PathMatcher::Regex(regex) => {
                conf.json_regex_type_overrides.push((regex, json_type));
            }
        }

        conf
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new_with_defaults()
    }
}

/// Returns the text as one of `serde::Value` types: int, float, bool or string.
fn parse_text(text: &str, leading_zero_as_string: bool, json_type: &JsonType) -> Value {
    let text = text.trim();

    // enforce JSON String data type regardless of the underlying type
    if json_type == &JsonType::AlwaysString {
        return Value::from(text);
    }

    // enforce JSON Bool data type
    #[cfg(feature = "json_types")]
    if let JsonType::Bool(true_values) = json_type {
        // any values matching the `true` list are bool/true; anything else is false
        return Value::from(true_values.contains(&text));
    }

    // ints
    if let Ok(v) = text.parse::<u64>() {
        // don't parse octal numbers and those with leading 0
        // `text` value "0" will always be converted into number 0, "0000" may be converted
        // into 0 or "0000" depending on `leading_zero_as_string`
        if leading_zero_as_string && text.starts_with("0") && (v != 0 || text.len() > 1) {
            return Value::from(text);
        }
        return Value::from(Number::from(v));
    }

    // floats
    if let Ok(v) = text.parse::<f64>() {
        if text.starts_with("0") && !text.starts_with("0.") {
            return Value::from(text);
        }
        if let Some(val) = Number::from_f64(v) {
            return Value::from(val);
        }
    }

    // booleans
    if let Ok(v) = text.parse::<bool>() {
        return Value::from(v);
    }

    Value::from(text)
}

fn convert_text(
    el: &roxmltree::Node,
    config: &Config,
    text: &str,
    #[cfg(feature = "json_types")]
    path: &str,
    json_type_value: &JsonType,
) -> Value {
    // process node's attributes, if present
    if el.attributes().count() > 0 {
        Value::from(
            el.attributes()
                .filter(|attr| {
                    !(config.process_xpointer_xincludes && attr.namespace() == Some("http://www.w3.org/XML/1998/namespace") && attr.name() == "id")
                })
                .map(|attr| {
                    // add the current node to the path
                    #[cfg(feature = "json_types")]
                    let path = [path, "/@", attr.name()].concat();
                    // get the json_type for this node
                    #[cfg(feature = "json_types")]
                    let json_type_value = get_json_type(config, &path).scalar_type();
                    (
                        [&config.xml_attr_prefix, attr.name()].concat(),
                        parse_text(
                            attr.value(),
                            config.leading_zero_as_string,
                            json_type_value,
                        ),
                    )
                })
                .chain(vec![(
                    config.xml_text_node_prop_name.clone(),
                    parse_text(text, config.leading_zero_as_string, &json_type_value),
                )])
                .collect::<Map>(),
        )
    } else {
        parse_text(
            text,
            config.leading_zero_as_string,
            &json_type_value,
        )
    }
}

fn convert_no_text(
    el: &roxmltree::Node,
    config: &Config,
    path: &str,
    #[cfg(not(feature = "json_types"))]
    json_type_value: &JsonType,
    xpointer_pointees: &mut HashMap<String, (String, Value)>,
) -> Option<Value> {
    // this element has no text, but may have other child nodes
    let mut data = Map::new();

    for attr in el.attributes() {
        // add the current node to the path
        #[cfg(feature = "json_types")]
        let path = [path, "/@", attr.name()].concat();
        // get the json_type for this node
        #[cfg(feature = "json_types")]
        let json_type_value = get_json_type(config, &path).scalar_type();
        if config.process_xpointer_xincludes && attr.namespace() == Some("http://www.w3.org/XML/1998/namespace") && attr.name() == "id" {
            continue;
        }
        data.insert(
            [&config.xml_attr_prefix, attr.name()].concat(),
            parse_text(
                attr.value(),
                config.leading_zero_as_string,
                json_type_value,
            ),
        );
    }

    // process child element recursively
    for child in el.children() {
        if let Some((val, name)) = convert_node(&child, config, &path, xpointer_pointees) {
            let name = name.as_deref().unwrap_or_else(|| child.tag_name().name());
            if !name.is_empty() {
                #[cfg(feature = "json_types")]
                let path = [path, "/", name].concat();

                match (get_json_type(config, &path), data.contains_key(name)) {
                    (JsonArray::Always(_), _) | (JsonArray::Infer(_), true) => {
                        // was this property converted to an array earlier?
                        match data.entry(name) {
                            serde_json::map::Entry::Vacant(vacant_entry) => {
                                // absent, so add the new value to a new array
                                vacant_entry.insert(Value::from(vec![val]));
                            }
                            serde_json::map::Entry::Occupied(mut occupied_entry) => {
                                match occupied_entry.get_mut() {
                                    Value::Array(values) => {
                                        // add the new value to an existing array
                                        values.push(val);
                                    }
                                    lval => {
                                        // convert the property to an array with the existing and the new values
                                        *lval = Value::from(vec![std::mem::take(lval), val]);
                                    }
                                }
                            }
                        }
                    }
                    (JsonArray::Infer(_), false) => {
                        // this is the first time this property is encountered and it doesn't
                        // have to be an array, so add it as-is
                        let name = name.to_owned();
                        data.insert(name, val);
                    }
                    (JsonArray::PlaceSingletonIntoArray { ty: _, array_name }, _) => {
                        let val = [(name.to_owned(), val)].into_iter().collect();
                        match data.entry(array_name) {
                            serde_json::map::Entry::Vacant(vacant_entry) => {
                                vacant_entry.insert(Value::from(vec![val]));
                            }
                            serde_json::map::Entry::Occupied(mut occupied_entry) => {
                                match occupied_entry.get_mut() {
                                    Value::Array(values) => { values.push(val); }
                                    lval => { *lval = Value::from(vec![std::mem::take(lval), val]); }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // return the JSON object if it's not empty
    if !data.is_empty() {
        return Some(data.into());
    }

    // empty objects are treated according to config rules set by the caller
    match config.empty_element_handling {
        NullValue::Null => Some(Value::Null),
        NullValue::EmptyObject => Some(data.into()),
        NullValue::Ignore => None,
    }
}

/// Converts an XML Element into a JSON property
fn convert_node(el: &roxmltree::Node, config: &Config, path: &str, xpointer_pointees: &mut HashMap<String, (String, Value)>) -> Option<(Value, Option<String>)> {

    if config.process_xpointer_xincludes
        && (el.tag_name() == roxmltree::ExpandedName::from_static("http://www.w3.org/2001/XInclude", "include")) && el.text().is_none() && !el.has_children() {
            if let &[xpointer] = &el.attributes().filter(|a| a.name() == "xpointer").map(|a| a.value()).collect::<Vec<_>>()[..] {
                if let Some((rename, pointee)) = xpointer_pointees.get(xpointer).cloned() {
                    return Some((pointee, Some(rename)));
                }
            }
        }

    // add the current node to the path
    #[cfg(feature = "json_types")]
    let path = [path, "/", el.tag_name().name()].concat();

    // get the json_type for this node
    let json_type_value = get_json_type(config, &path).scalar_type().clone();

    // is it an element with text?
    let out = match el.text() {
        Some(mut text) => {
            text = text.trim();

            if text.is_empty() {
                convert_no_text(el, config, &path, #[cfg(not(feature = "json_types"))] &json_type_value, xpointer_pointees)
            } else {
                Some(convert_text(el, config, text, #[cfg(feature = "json_types")] &path, &json_type_value))
            }
        }
        None => convert_no_text(el, config, &path, #[cfg(not(feature = "json_types"))] &json_type_value, xpointer_pointees),
    };

    if config.process_xpointer_xincludes {
        if let Some(id) = el.attribute(("http://www.w3.org/XML/1998/namespace", "id")) {
            if let Some(out) = &out {
                xpointer_pointees.insert(id.to_owned(), (el.tag_name().name().to_owned(), out.clone()));
            }
        }
    }

    out.map(|out| (out, None))
}

fn xml_to_map(e: &roxmltree::Node, config: &Config) -> Value {
    let mut data = Map::new();
    let name = e.tag_name().name().to_owned();
    let mut xpointer_pointees = HashMap::default();
    let (val, new_name) = convert_node(&e, &config, "", &mut xpointer_pointees).unwrap_or((Value::Null, None));
    let name = new_name.unwrap_or(name);
    data.insert(name, val);
    data.into()
}

/// Converts the given XML string into `serde::Value` using settings from `Config` struct.
pub fn xml_str_to_json(xml: &str, config: &Config) -> Result<Value, roxmltree::Error> {
    let Config { allow_dtd, .. } = *config;
    let doc = roxmltree::Document::parse_with_options(xml, roxmltree::ParsingOptions { allow_dtd, .. Default::default() })?;
    let root = doc.root_element();
    Ok(xml_to_map(&root, config))
}

/// Converts the given XML string into `serde::Value` using settings from `Config` struct.
pub fn xml_string_to_json(xml: String, config: &Config) -> Result<Value, roxmltree::Error> {
    xml_str_to_json(xml.as_str(), config)
}

/// Returns a tuple for Array and Value enforcements for the current node or
/// `(false, JsonArray::Infer(JsonType::Infer)` if the current path is not found
/// in the list of paths with custom config.
#[cfg(feature = "json_types")]
#[inline]
fn get_json_type_with_absolute_path<'conf>(
    config: &'conf Config,
    path: &str,
) -> &'conf JsonArray {
    config
        .json_type_overrides
        .get(path)
        .unwrap_or(&JsonArray::Infer(JsonType::Infer))
}

/// Simply returns `get_json_type_with_absolute_path` if `regex_path` feature is disabled.
#[cfg(feature = "json_types")]
#[cfg(not(feature = "regex_path"))]
#[inline]
fn get_json_type<'conf>(config: &'conf Config, path: &str) -> &'conf JsonArray {
    get_json_type_with_absolute_path(config, path)
}

/// Returns a tuple for Array and Value enforcements for the current node. Searches both absolute paths
/// and regex paths, giving precedence to regex paths. Returns `(false, JsonArray::Infer(JsonType::Infer)`
/// if the current path is not found in the list of paths with custom config.
#[cfg(feature = "json_types")]
#[cfg(feature = "regex_path")]
#[inline]
fn get_json_type<'conf>(config: &'conf Config, path: &str) -> &'conf JsonArray {
    for (regex, json_array) in &config.json_regex_type_overrides {
        if regex.is_match(path) {
            return json_array;
        }
    }

    get_json_type_with_absolute_path(config, path)
}

/// Always returns `JsonArray::Infer(JsonType::Infer)` if `json_types` feature is not enabled.
#[cfg(not(feature = "json_types"))]
#[inline]
fn get_json_type<'conf>(_config: &'conf Config, _path: &str) -> &'conf JsonArray {
    &JsonArray::Infer(JsonType::Infer)
}
