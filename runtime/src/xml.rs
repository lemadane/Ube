//! XML documents, explicit serialization options, and shared typed object traversal.
use crate::json::Data;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::{c_char, c_void, CStr},
    sync::{Arc, Mutex, OnceLock},
};
type Result<T> = std::result::Result<T, String>;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_NODES: usize = 100_000;
const MAX_DEPTH: usize = 64;
const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
const XML: &str = "http://www.w3.org/XML/1998/namespace";
#[derive(Clone)]
enum Content {
    Text(String),
    Element(usize),
}
#[derive(Clone)]
struct Node {
    name: String,
    attributes: BTreeMap<String, String>,
    content: Vec<Content>,
    parent: Option<usize>,
}
#[derive(Default)]
struct Tree {
    nodes: Vec<Node>,
    bytes: usize,
}
#[derive(Clone)]
struct Handle {
    tree: Arc<Mutex<Tree>>,
    node: usize,
}
#[derive(Clone, Default)]
struct Options {
    root: Option<String>,
    attributes: HashSet<String>,
    renames: BTreeMap<String, String>,
    namespaces: BTreeMap<String, String>,
    item: Option<String>,
}
#[derive(Default)]
struct State {
    handles: HashMap<usize, Box<Handle>>,
    options: HashMap<usize, Box<Options>>,
}
fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}
fn text<'a>(ptr: *const c_char) -> Result<&'a str> {
    if ptr.is_null() {
        return Err("XML string argument cannot be null".into());
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| "XML strings must be UTF-8".into())
}
fn run<T: Default>(kind: &str, work: impl FnOnce() -> Result<T>) -> T {
    match work() {
        Ok(value) => value,
        Err(error) => {
            crate::exceptions::set_pending(crate::exceptions::named(kind, error));
            T::default()
        }
    }
}
fn string(value: &str) -> Result<*mut c_char> {
    crate::json::string(value).map_err(|error| error.replace("JSON", "XML"))
}
fn handle(ptr: *mut c_void) -> Result<Handle> {
    state()
        .lock()
        .unwrap()
        .handles
        .get(&(ptr as usize))
        .map(|value| (**value).clone())
        .ok_or("Invalid XML document/element handle".into())
}
fn own(value: Handle) -> *mut c_void {
    let mut value = Box::new(value);
    let ptr = (&mut *value as *mut Handle).cast();
    state().lock().unwrap().handles.insert(ptr as usize, value);
    ptr
}
fn publish(tree: Tree) -> *mut c_void {
    own(Handle {
        tree: Arc::new(Mutex::new(tree)),
        node: 0,
    })
}
fn options(ptr: *mut c_void) -> Result<Options> {
    if ptr.is_null() {
        Ok(Options::default())
    } else {
        state()
            .lock()
            .unwrap()
            .options
            .get(&(ptr as usize))
            .map(|value| (**value).clone())
            .ok_or("Invalid XmlOptions handle".into())
    }
}
fn charge(tree: &mut Tree, bytes: usize) -> Result<()> {
    let next = tree.bytes.checked_add(bytes).ok_or("XML size overflow")?;
    if next > MAX_BYTES {
        return Err("XML document exceeds 16 MiB storage budget".into());
    }
    tree.bytes = next;
    Ok(())
}
fn add_node(tree: &mut Tree, name: &str, parent: Option<usize>) -> Result<usize> {
    valid_name(name)?;
    if tree.nodes.len() >= MAX_NODES {
        return Err("XML node limit exceeded".into());
    }
    let mut depth = 0;
    let mut ancestor = parent;
    while let Some(index) = ancestor {
        depth += 1;
        ancestor = tree.nodes[index].parent;
    }
    if depth > MAX_DEPTH {
        return Err("XML nesting exceeds 64 levels".into());
    }
    charge(tree, name.len() + 8)?;
    let index = tree.nodes.len();
    tree.nodes.push(Node {
        name: name.into(),
        attributes: BTreeMap::new(),
        content: vec![],
        parent,
    });
    Ok(index)
}
fn put_attribute(tree: &mut Tree, node: usize, name: &str, value: &str) -> Result<()> {
    valid_chars(value)?;
    let previous = tree.nodes[node]
        .attributes
        .get(name)
        .map_or(0, |value| name.len() + value.len());
    let added = name.len() + value.len();
    if added > previous {
        charge(tree, added - previous)?;
    } else {
        tree.bytes -= previous - added;
    }
    tree.nodes[node]
        .attributes
        .insert(name.into(), value.into());
    Ok(())
}
fn push_text(tree: &mut Tree, node: usize, value: &str) -> Result<()> {
    valid_chars(value)?;
    charge(tree, value.len())?;
    tree.nodes[node].content.push(Content::Text(value.into()));
    Ok(())
}
fn valid_chars(value: &str) -> Result<()> {
    if value
        .chars()
        .any(|ch| !matches!(ch as u32,9|10|13|0x20..=0xD7FF|0xE000..=0xFFFD|0x10000..=0x10FFFF))
    {
        return Err("Text contains a character forbidden by XML 1.0".into());
    }
    Ok(())
}
fn escaped(value: &str, attribute: bool) -> Result<String> {
    valid_chars(value)?;
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            '\r' => out.push_str("&#xD;"),
            '\n' if attribute => out.push_str("&#xA;"),
            '\t' if attribute => out.push_str("&#x9;"),
            _ => out.push(ch),
        }
    }
    Ok(out)
}
fn valid_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 1024 {
        return Err("XML names require 1–1024 UTF-8 bytes".into());
    }
    // Parse a synthetic element to use the parser's complete Unicode name rules.
    let parts: Vec<_> = name.split(':').collect();
    if parts.len() > 2
        || parts.iter().any(|part| part.is_empty())
        || (parts.len() == 2 && parts[0] == "xmlns")
    {
        return Err("Invalid or reserved XML name".into());
    }
    // Reject markup before interpolation into the synthetic document.
    if name
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '<' | '>' | '/' | '=' | '\'' | '"' | '&'))
    {
        return Err("Invalid XML name".into());
    }
    let declaration = if parts.len() == 2 && parts[0] != "xml" {
        format!(" xmlns:{}=\"urn:ube:name-check\"", parts[0])
    } else {
        String::new()
    };
    roxmltree::Document::parse(&format!("<{name}{declaration}/>"))
        .map_err(|_| "Invalid XML name".to_string())?;
    Ok(())
}
fn namespace_declaration(prefix: &str, uri: &str) -> Result<String> {
    if prefix.contains(':') || prefix == "xmlns" {
        return Err("Invalid XML namespace prefix".into());
    }
    if !prefix.is_empty() {
        valid_name(prefix)?;
    }
    if prefix == "xml" && uri != XML {
        return Err("The xml prefix has a fixed namespace URI".into());
    }
    if prefix == "xsi" && uri != XSI {
        return Err("The xsi prefix is reserved for XML Schema instance metadata".into());
    }
    if !prefix.is_empty() && uri.is_empty() {
        return Err("A namespace prefix requires a non-empty URI".into());
    }
    let name = if prefix.is_empty() {
        "xmlns".into()
    } else {
        format!("xmlns:{prefix}")
    };
    roxmltree::Document::parse(&format!("<check {name}=\"{}\"/>", escaped(uri, true)?))
        .map_err(|error| format!("Invalid namespace: {error}"))?;
    Ok(name)
}
fn scope(tree: &Tree, node: usize) -> BTreeMap<String, String> {
    let mut ancestors = vec![node];
    let mut parent = tree.nodes[node].parent;
    while let Some(index) = parent {
        ancestors.push(index);
        parent = tree.nodes[index].parent;
    }
    let mut values = BTreeMap::from([("xml".into(), XML.into())]);
    for index in ancestors.into_iter().rev() {
        for (name, value) in &tree.nodes[index].attributes {
            if name == "xmlns" {
                values.insert(String::new(), value.clone());
            } else if let Some(prefix) = name.strip_prefix("xmlns:") {
                values.insert(prefix.into(), value.clone());
            }
        }
    }
    values
}
fn namespace_uri(tree: &Tree, node: usize) -> String {
    let name = &tree.nodes[node].name;
    let prefix = name.split_once(':').map_or("", |(prefix, _)| prefix);
    scope(tree, node).get(prefix).cloned().unwrap_or_default()
}
// roxmltree replaces out-of-range numeric references with U+FFFD. Validate
// original references so invalid XML cannot silently become different data.
// References inside CDATA, comments, and processing instructions are literal.
fn validate_character_references(input: &str) -> Result<()> {
    let mut rest = input;
    while let Some(index) = rest.find(['<', '&']) {
        rest = &rest[index..];
        let literal_end = if rest.starts_with("<![CDATA[") {
            Some("]]>")
        } else if rest.starts_with("<!--") {
            Some("-->")
        } else if rest.starts_with("<?") {
            Some("?>")
        } else {
            None
        };
        if let Some(end) = literal_end {
            let index = rest.find(end).ok_or("Unterminated XML markup")?;
            rest = &rest[index + end.len()..];
        } else if let Some(reference) = rest.strip_prefix("&#") {
            let end = reference
                .find(';')
                .ok_or("Invalid XML character reference")?;
            let digits = &reference[..end];
            let (digits, radix) = digits
                .strip_prefix('x')
                .map_or((digits, 10), |hex| (hex, 16));
            let value = u32::from_str_radix(digits, radix)
                .ok()
                .and_then(char::from_u32)
                .ok_or("Invalid XML character reference")?;
            valid_chars(value.encode_utf8(&mut [0; 4]))?;
            rest = &reference[end + 1..];
        } else {
            rest = &rest[1..];
        }
    }
    Ok(())
}
fn parse_tree(input: &str) -> Result<Tree> {
    if input.len() > MAX_BYTES {
        return Err("XML input exceeds 16 MiB".into());
    }
    let doc = roxmltree::Document::parse_with_options(
        input,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_NODES as u32,
            ..Default::default()
        },
    )
    .map_err(|error| format!("Invalid XML: {error}"))?;
    validate_character_references(input)?;
    fn copy(
        source: roxmltree::Node<'_, '_>,
        input: &str,
        parent: Option<usize>,
        tree: &mut Tree,
    ) -> Result<usize> {
        let raw = &input[source.range().start + 1..];
        let name = raw
            .split(|ch: char| ch.is_whitespace() || ch == '/' || ch == '>')
            .next()
            .ok_or("XML element name missing")?;
        let index = add_node(tree, name, parent)?;
        for ns in source.namespaces() {
            if ns.name() == Some("xml") {
                continue;
            }
            let name = ns
                .name()
                .map_or("xmlns".into(), |prefix| format!("xmlns:{prefix}"));
            put_attribute(tree, index, &name, ns.uri())?;
        }
        for attr in source.attributes() {
            let name = input[attr.range().start..]
                .split(|ch: char| ch.is_whitespace() || ch == '=')
                .next()
                .ok_or("XML attribute name missing")?;
            valid_name(name)?;
            put_attribute(tree, index, name, attr.value())?;
        }
        for child in source.children() {
            if child.is_element() {
                let child = copy(child, input, Some(index), tree)?;
                tree.nodes[index].content.push(Content::Element(child));
            } else if child.is_text() {
                push_text(tree, index, child.text().unwrap_or_default())?;
            }
        }
        Ok(index)
    }
    let mut tree = Tree::default();
    copy(doc.root_element(), input, None, &mut tree)?;
    Ok(tree)
}
fn render(tree: &Tree, node: usize) -> Result<String> {
    fn write(tree: &Tree, index: usize, root: bool, output: &mut String) -> Result<()> {
        let node = &tree.nodes[index];
        output.push('<');
        output.push_str(&node.name);
        let mut attributes = node.attributes.clone();
        if root {
            for (prefix, uri) in scope(tree, index) {
                if prefix != "xml" {
                    attributes
                        .entry(if prefix.is_empty() {
                            "xmlns".into()
                        } else {
                            format!("xmlns:{prefix}")
                        })
                        .or_insert(uri);
                }
            }
        }
        for (name, value) in attributes {
            output.push(' ');
            output.push_str(&name);
            output.push_str("=\"");
            output.push_str(&escaped(&value, true)?);
            output.push('"');
        }
        if node.content.is_empty() {
            output.push_str("/>");
        } else {
            output.push('>');
            for content in &node.content {
                match content {
                    Content::Text(value) => output.push_str(&escaped(value, false)?),
                    Content::Element(child) => write(tree, *child, false, output)?,
                }
                if output.len() > MAX_BYTES {
                    return Err("Encoded XML exceeds 16 MiB".into());
                }
            }
            output.push_str("</");
            output.push_str(&node.name);
            output.push('>');
        }
        if output.len() > MAX_BYTES {
            return Err("Encoded XML exceeds 16 MiB".into());
        }
        Ok(())
    }
    let mut output = String::new();
    write(tree, node, true, &mut output)?;
    // Namespace resolution and duplicate expanded attributes must remain valid
    // after explicit builder edits as well as automatic serialization.
    roxmltree::Document::parse_with_options(
        &output,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_NODES as u32,
            ..Default::default()
        },
    )
    .map_err(|error| format!("Invalid XML document: {error}"))?;
    Ok(output)
}
fn encode(value: Data, options: &Options) -> Result<Tree> {
    let default = match &value {
        Data::Object {
            class: Some(name), ..
        } => name.as_str(),
        Data::Object { class: None, .. } => "map",
        Data::List(_) => "list",
        _ => "value",
    };
    let root = options.root.as_deref().unwrap_or(default);
    let mut tree = Tree::default();
    let root = add_node(&mut tree, root, None)?;
    for (prefix, uri) in &options.namespaces {
        let name = namespace_declaration(prefix, uri)?;
        put_attribute(&mut tree, root, &name, uri)?;
    }
    fn scalar(value: &Data) -> Result<String> {
        match value {
            Data::Scalar(Value::String(value)) => Ok(value.clone()),
            Data::Scalar(Value::Bool(value)) => Ok(value.to_string()),
            Data::Scalar(Value::Number(value)) => Ok(value.to_string()),
            _ => Err("XML attributes require non-null scalar fields".into()),
        }
    }
    fn write(
        tree: &mut Tree,
        index: usize,
        value: Data,
        options: &Options,
        root: bool,
    ) -> Result<()> {
        match value {
            Data::Scalar(Value::Null) => {
                put_attribute(tree, 0, "xmlns:xsi", XSI)?;
                put_attribute(tree, index, "xsi:nil", "true")?;
            }
            Data::Scalar(value) => {
                let value = scalar(&Data::Scalar(value))?;
                push_text(tree, index, &value)?;
            }
            Data::List(values) => {
                for value in values {
                    let child =
                        add_node(tree, options.item.as_deref().unwrap_or("item"), Some(index))?;
                    tree.nodes[index].content.push(Content::Element(child));
                    write(tree, child, value, options, false)?;
                }
            }
            Data::Object {
                class: None,
                fields,
            } => {
                for (key, value) in fields {
                    let child = add_node(tree, "entry", Some(index))?;
                    put_attribute(tree, child, "key", &key)?;
                    tree.nodes[index].content.push(Content::Element(child));
                    write(tree, child, value, options, false)?;
                }
            }
            Data::Object {
                class: Some(_),
                fields,
            } => {
                let mut used = HashSet::new();
                if root {
                    for field in options.attributes.iter().chain(options.renames.keys()) {
                        if !fields.iter().any(|(name, _)| name == field) {
                            return Err(format!("Unknown XML option field '{field}'"));
                        }
                    }
                }
                for (field, value) in fields {
                    let name = if root {
                        options.renames.get(&field).unwrap_or(&field)
                    } else {
                        &field
                    };
                    let attribute = root && options.attributes.contains(&field);
                    let bindings = scope(tree, index);
                    let (prefix, local) = name
                        .split_once(':')
                        .map_or(("", name.as_str()), |(prefix, local)| (prefix, local));
                    let uri = if prefix.is_empty() && attribute {
                        String::new()
                    } else if prefix.is_empty() {
                        bindings.get("").cloned().unwrap_or_default()
                    } else {
                        bindings
                            .get(prefix)
                            .cloned()
                            .ok_or_else(|| format!("Unbound XML prefix: {prefix}"))?
                    };
                    if !used.insert((uri.clone(), local.to_string())) {
                        return Err(format!("XML field name collision: {name}"));
                    }
                    if attribute && uri == XSI && local == "nil" {
                        return Err("XML Schema instance nil is reserved for null values".into());
                    }
                    if attribute {
                        if name == "xmlns" || name.starts_with("xmlns:") {
                            return Err("Namespace declarations cannot be serialized from data fields; use XmlOptions.namespace".into());
                        }
                        put_attribute(tree, index, name, &scalar(&value)?)?;
                    } else {
                        let child = add_node(tree, name, Some(index))?;
                        tree.nodes[index].content.push(Content::Element(child));
                        write(tree, child, value, options, false)?;
                    }
                }
            }
        }
        Ok(())
    }
    if (!options.attributes.is_empty() || !options.renames.is_empty())
        && !matches!(&value, Data::Object { class: Some(_), .. })
    {
        return Err("Field XML options require a data-class root".into());
    }
    write(&mut tree, root, value, options, true)?;
    Ok(tree)
}
#[no_mangle]
pub extern "C" fn ube_to_xml(
    payload: i64,
    meta: u64,
    kind: i64,
    name: *const c_char,
    settings: *mut c_void,
) -> *mut c_char {
    run("XmlError", || {
        let options = options(settings)?;
        let value = crate::json::encode_data(payload, meta, kind, text(name)?)
            .map_err(|error| error.replace("JSON", "XML"))?;
        let tree = encode(value, &options)?;
        string(&render(&tree, 0)?)
    })
}
#[no_mangle]
pub extern "C" fn ube_xml_parse(input: *const c_char) -> *mut c_void {
    run("XmlParseError", || Ok(publish(parse_tree(text(input)?)?)))
}
#[no_mangle]
pub extern "C" fn ube_xml_create(name: *const c_char) -> *mut c_void {
    run("XmlError", || {
        let mut tree = Tree::default();
        add_node(&mut tree, text(name)?, None)?;
        Ok(publish(tree))
    })
}
#[no_mangle]
pub extern "C" fn ube_xmldocument_root(ptr: *mut c_void) -> *mut c_void {
    run("XmlError", || Ok(own(handle(ptr)?)))
}
#[no_mangle]
pub extern "C" fn ube_xmldocument_to_string(ptr: *mut c_void) -> *mut c_char {
    run("XmlError", || {
        let handle = handle(ptr)?;
        let tree = handle.tree.lock().unwrap();
        string(&render(&tree, handle.node)?)
    })
}
fn children(tree: &Tree, index: usize, name: &str) -> Vec<usize> {
    tree.nodes[index]
        .content
        .iter()
        .filter_map(|content| match content {
            Content::Element(child) if name == "*" || tree.nodes[*child].name == name => {
                Some(*child)
            }
            _ => None,
        })
        .collect()
}
#[no_mangle]
pub extern "C" fn ube_xmlement_child(ptr: *mut c_void, name: *const c_char) -> *mut c_void {
    ube_xml_child_at(ptr, name, 0)
}
#[no_mangle]
pub extern "C" fn ube_xml_child_at(
    ptr: *mut c_void,
    name: *const c_char,
    index: i32,
) -> *mut c_void {
    run("XmlError", || {
        let handle = handle(ptr)?;
        let name = text(name)?;
        let node = {
            let tree = handle.tree.lock().unwrap();
            let index = usize::try_from(index).map_err(|_| "XML child index cannot be negative")?;
            children(&tree, handle.node, name)
                .get(index)
                .copied()
                .ok_or("XML child not found or index out of bounds")?
        };
        Ok(own(Handle {
            tree: handle.tree,
            node,
        }))
    })
}

#[no_mangle]
pub extern "C" fn ube_xml_child_count(ptr: *mut c_void, name: *const c_char) -> i32 {
    run("XmlError", || {
        let handle = handle(ptr)?;
        let tree = handle.tree.lock().unwrap();
        Ok(children(&tree, handle.node, text(name)?).len() as i32)
    })
}
#[no_mangle]
pub extern "C" fn ube_xml_child_ns(
    ptr: *mut c_void,
    uri: *const c_char,
    local: *const c_char,
) -> *mut c_void {
    run("XmlError", || {
        let handle = handle(ptr)?;
        let uri = text(uri)?;
        let local = text(local)?;
        let node = {
            let tree = handle.tree.lock().unwrap();
            children(&tree, handle.node, "*")
                .into_iter()
                .find(|child| {
                    tree.nodes[*child].name.rsplit(':').next() == Some(local)
                        && namespace_uri(&tree, *child) == uri
                })
                .ok_or("XML child not found in namespace")?
        };
        Ok(own(Handle {
            tree: handle.tree,
            node,
        }))
    })
}
#[no_mangle]
pub extern "C" fn ube_xmlement_element(ptr: *mut c_void, name: *const c_char) -> *mut c_void {
    run("XmlError", || {
        let handle = handle(ptr)?;
        let node = {
            let mut tree = handle.tree.lock().unwrap();
            let node = add_node(&mut tree, text(name)?, Some(handle.node))?;
            tree.nodes[handle.node].content.push(Content::Element(node));
            node
        };
        Ok(own(Handle {
            tree: handle.tree,
            node,
        }))
    })
}
#[no_mangle]
pub extern "C" fn ube_xmlement_text(ptr: *mut c_void, value: *const c_char) -> *mut c_void {
    run("XmlError", || {
        let handle = handle(ptr)?;
        let value = text(value)?;
        valid_chars(value)?;
        let mut tree = handle.tree.lock().unwrap();
        let previous: usize = tree.nodes[handle.node]
            .content
            .iter()
            .map(|content| match content {
                Content::Text(value) => value.len(),
                _ => 0,
            })
            .sum();
        if value.len() > previous {
            charge(&mut tree, value.len() - previous)?;
        } else {
            tree.bytes -= previous - value.len();
        }
        tree.nodes[handle.node].content = vec![Content::Text(value.into())];
        Ok(ptr)
    })
}
#[no_mangle]
pub extern "C" fn ube_xmlement_get_text(ptr: *mut c_void) -> *mut c_char {
    run("XmlError", || {
        fn collect(tree: &Tree, index: usize, text: &mut String) {
            for content in &tree.nodes[index].content {
                match content {
                    Content::Text(value) => text.push_str(value),
                    Content::Element(child) => collect(tree, *child, text),
                }
            }
        }
        let handle = handle(ptr)?;
        let tree = handle.tree.lock().unwrap();
        let mut value = String::new();
        collect(&tree, handle.node, &mut value);
        string(&value)
    })
}
#[no_mangle]
pub extern "C" fn ube_xml_attribute(
    ptr: *mut c_void,
    name: *const c_char,
    value: *const c_char,
) -> *mut c_void {
    run("XmlError", || {
        let handle = handle(ptr)?;
        let name = text(name)?;
        valid_name(name)?;
        if name == "xmlns" || name.starts_with("xmlns:") {
            return Err("Use namespace(prefix, uri) for namespace declarations".into());
        }
        put_attribute(
            &mut handle.tree.lock().unwrap(),
            handle.node,
            name,
            text(value)?,
        )?;
        Ok(ptr)
    })
}
#[no_mangle]
pub extern "C" fn ube_xml_get_attribute(ptr: *mut c_void, name: *const c_char) -> *mut c_char {
    run("XmlError", || {
        let handle = handle(ptr)?;
        let tree = handle.tree.lock().unwrap();
        string(
            tree.nodes[handle.node]
                .attributes
                .get(text(name)?)
                .ok_or("XML attribute is missing")?,
        )
    })
}
#[no_mangle]
pub extern "C" fn ube_xml_namespace(
    ptr: *mut c_void,
    prefix: *const c_char,
    uri: *const c_char,
) -> *mut c_void {
    run("XmlError", || {
        let handle = handle(ptr)?;
        let uri = text(uri)?;
        let name = namespace_declaration(text(prefix)?, uri)?;
        put_attribute(&mut handle.tree.lock().unwrap(), handle.node, &name, uri)?;
        Ok(ptr)
    })
}
#[no_mangle]
pub extern "C" fn ube_xml_name(ptr: *mut c_void) -> *mut c_char {
    run("XmlError", || {
        let handle = handle(ptr)?;
        let tree = handle.tree.lock().unwrap();
        string(&tree.nodes[handle.node].name)
    })
}
#[no_mangle]
pub extern "C" fn ube_xml_namespace_uri(ptr: *mut c_void) -> *mut c_char {
    run("XmlError", || {
        let handle = handle(ptr)?;
        let tree = handle.tree.lock().unwrap();
        string(&namespace_uri(&tree, handle.node))
    })
}
#[no_mangle]
pub extern "C" fn ube_xml_is_null(ptr: *mut c_void) -> bool {
    run("XmlError", || {
        let handle = handle(ptr)?;
        let tree = handle.tree.lock().unwrap();
        let scope = scope(&tree, handle.node);
        Ok(tree.nodes[handle.node]
            .attributes
            .iter()
            .any(|(name, value)| {
                name.split_once(':').is_some_and(|(prefix, local)| {
                    local == "nil" && scope.get(prefix).is_some_and(|uri| uri == XSI)
                }) && (value == "true" || value == "1")
            }))
    })
}
#[no_mangle]
pub extern "C" fn ube_xml_options_create() -> *mut c_void {
    let mut value = Box::<Options>::default();
    let ptr = (&mut *value as *mut Options).cast();
    state().lock().unwrap().options.insert(ptr as usize, value);
    ptr
}
fn edit_options(ptr: *mut c_void, edit: impl FnOnce(&mut Options) -> Result<()>) -> *mut c_void {
    run("XmlError", || {
        let mut state = state().lock().unwrap();
        let options = state
            .options
            .get_mut(&(ptr as usize))
            .ok_or("Invalid XmlOptions handle")?;
        edit(options)?;
        Ok(ptr)
    })
}
#[no_mangle]
pub extern "C" fn ube_xml_options_root(ptr: *mut c_void, name: *const c_char) -> *mut c_void {
    edit_options(ptr, |options| {
        let name = text(name)?;
        valid_name(name)?;
        options.root = Some(name.into());
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn ube_xml_options_attribute(ptr: *mut c_void, field: *const c_char) -> *mut c_void {
    edit_options(ptr, |options| {
        let field = text(field)?;
        valid_name(field)?;
        options.attributes.insert(field.into());
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn ube_xml_options_rename(
    ptr: *mut c_void,
    field: *const c_char,
    name: *const c_char,
) -> *mut c_void {
    edit_options(ptr, |options| {
        let field = text(field)?;
        let name = text(name)?;
        valid_name(field)?;
        valid_name(name)?;
        options.renames.insert(field.into(), name.into());
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn ube_xml_options_namespace(
    ptr: *mut c_void,
    prefix: *const c_char,
    uri: *const c_char,
) -> *mut c_void {
    edit_options(ptr, |options| {
        let prefix = text(prefix)?;
        let uri = text(uri)?;
        namespace_declaration(prefix, uri)?;
        options.namespaces.insert(prefix.into(), uri.into());
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn ube_xml_options_item(ptr: *mut c_void, name: *const c_char) -> *mut c_void {
    edit_options(ptr, |options| {
        let name = text(name)?;
        valid_name(name)?;
        options.item = Some(name.into());
        Ok(())
    })
}
pub fn cleanup() {
    let mut state = state().lock().unwrap();
    state.handles.clear();
    state.options.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_preserves_unicode_quotes_and_attribute_whitespace() {
        let mut tree = Tree::default();
        let root = add_node(&mut tree, "root", None).unwrap();
        let text = "東京 <&> \"quoted\"\r\n\t";
        put_attribute(&mut tree, root, "value", text).unwrap();
        push_text(&mut tree, root, text).unwrap();
        let xml = render(&tree, root).unwrap();
        let parsed = roxmltree::Document::parse(&xml).unwrap();
        assert_eq!(parsed.root_element().attribute("value"), Some(text));
        assert_eq!(parsed.root_element().text(), Some(text));
        assert!(xml.contains("&amp;") && xml.contains("&lt;") && xml.contains("&quot;"));
        assert!(xml.contains("&#xD;") && xml.contains("&#xA;") && xml.contains("&#x9;"));
        for invalid in ["\0", "\u{1}", "\u{b}", "\u{fffe}", "\u{ffff}"] {
            assert!(escaped(invalid, false).is_err());
            assert!(parse_tree(&format!("<r>{invalid}</r>")).is_err());
        }
    }

    #[test]
    fn parser_rejects_invalid_input_dtd_and_resource_limits() {
        for input in [
            "",
            "text",
            "<a/><b/>",
            "<a></b>",
            "<a>&missing;</a>",
            "<p:a/>",
            "<!DOCTYPE a><a/>",
            "<!DOCTYPE a SYSTEM 'file:///etc/passwd'><a/>",
            "<a x='1' x='2'/>",
            "<a xmlns:p='urn:x' xmlns:q='urn:x' p:x='1' q:x='2'/>",
        ] {
            assert!(parse_tree(input).is_err(), "accepted {input}");
        }
        assert!(parse_tree(&" ".repeat(MAX_BYTES + 1)).is_err());
        let deep = format!(
            "{}{}",
            "<n>".repeat(MAX_DEPTH + 2),
            "</n>".repeat(MAX_DEPTH + 2)
        );
        assert!(parse_tree(&deep).err().unwrap().contains("nesting"));
        let large = format!("<r>{}</r>", "<n/>".repeat(MAX_NODES));
        assert!(parse_tree(&large).is_err());
        let mut tree = Tree::default();
        add_node(&mut tree, "r", None).unwrap();
        assert!(charge(&mut tree, MAX_BYTES).is_err());
        assert!(parse_tree("<after>valid</after>").is_ok());
    }

    #[test]
    fn names_and_namespace_bindings_are_checked_without_markup_injection() {
        for name in [
            "",
            "bad name",
            "a:b:c",
            "a\" injected=\"yes",
            "a/><evil",
            "1root",
            "xmlns:fake",
        ] {
            assert!(valid_name(name).is_err(), "accepted {name}");
        }
        assert!(valid_name("資料").is_ok());
        assert!(namespace_declaration("xml", "urn:wrong").is_err());
        assert!(namespace_declaration("p", "").is_err());
        let mut tree = Tree::default();
        add_node(&mut tree, "p:root", None).unwrap();
        assert!(render(&tree, 0).is_err());
        put_attribute(&mut tree, 0, "xmlns:p", "urn:p").unwrap();
        assert!(render(&tree, 0).is_ok());
    }

    #[test]
    fn parsed_namespaces_mixed_content_and_aliases_survive_serialization() {
        let input = "<p:r xmlns:p='urn:p' xmlns:q='urn:p' q:id='7'>before<p:c/>after</p:r>";
        let tree = parse_tree(input).unwrap();
        assert_eq!(tree.nodes[0].name, "p:r");
        assert_eq!(tree.nodes[0].attributes["q:id"], "7");
        let xml = render(&tree, 0).unwrap();
        let doc = roxmltree::Document::parse(&xml).unwrap();
        assert_eq!(doc.root_element().tag_name().namespace(), Some("urn:p"));
        assert_eq!(doc.root_element().attribute(("urn:p", "id")), Some("7"));
        assert!(xml.contains("before<p:c") && xml.contains("after</p:r>"));
        let child = children(&tree, 0, "p:c")[0];
        let standalone = render(&tree, child).unwrap();
        assert_eq!(
            roxmltree::Document::parse(&standalone)
                .unwrap()
                .root_element()
                .tag_name()
                .namespace(),
            Some("urn:p")
        );
    }

    #[test]
    fn nulls_and_attribute_rules_are_explicit() {
        let data = || Data::Object {
            class: Some("Record".into()),
            fields: vec![
                ("missing".into(), Data::Scalar(Value::Null)),
                ("empty".into(), Data::Scalar(Value::String(String::new()))),
            ],
        };
        let xml = render(&encode(data(), &Options::default()).unwrap(), 0).unwrap();
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let root = doc.root_element();
        let missing = root
            .children()
            .find(|node| node.has_tag_name("missing"))
            .unwrap();
        let empty = root
            .children()
            .find(|node| node.has_tag_name("empty"))
            .unwrap();
        assert_eq!(missing.attribute((XSI, "nil")), Some("true"));
        assert_eq!(empty.attribute((XSI, "nil")), None);
        let mut options = Options::default();
        options.attributes.insert("missing".into());
        assert!(encode(data(), &options).is_err());
        options.attributes.clear();
        options.attributes.insert("unknown".into());
        assert!(encode(data(), &options).is_err());
        assert!(encode(Data::List(vec![]), &options).is_err());
    }
    #[test]
    fn customization_rejects_namespace_alias_collisions_and_nil_spoofing() {
        let data = || Data::Object {
            class: Some("Pair".into()),
            fields: vec![
                ("a".into(), Data::Scalar(Value::Bool(true))),
                ("b".into(), Data::Scalar(Value::Bool(false))),
            ],
        };
        let mut options = Options::default();
        options.namespaces.insert("p".into(), "urn:same".into());
        options.namespaces.insert("q".into(), "urn:same".into());
        options.renames.insert("a".into(), "p:value".into());
        options.renames.insert("b".into(), "q:value".into());
        assert!(encode(data(), &options).is_err());
        let mut options = Options::default();
        options.namespaces.insert("xsi".into(), XSI.into());
        options.attributes.insert("a".into());
        options.renames.insert("a".into(), "xsi:nil".into());
        assert!(encode(data(), &options).is_err());
    }
}
