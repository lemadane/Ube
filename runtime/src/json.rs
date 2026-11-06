//! Real JSON documents and compiler-described object serialization.
use serde_json::{Number, Value};
use std::{
    collections::{HashMap, HashSet},
    ffi::{c_char, c_void, CStr},
    sync::{Mutex, OnceLock},
};
type Result<T> = std::result::Result<T, String>;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_NODES: usize = 100_000;
const MAX_DEPTH: usize = 64;
#[derive(Clone)]
struct Field {
    name: String,
    child: String,
    kind: i64,
    offset: usize,
    payload: usize,
    presence: i64,
    width: usize,
}
fn schemas() -> &'static Mutex<HashMap<String, Vec<Field>>> {
    static SCHEMAS: OnceLock<Mutex<HashMap<String, Vec<Field>>>> = OnceLock::new();
    SCHEMAS.get_or_init(Default::default)
}
#[repr(C)]
struct Document {
    id: i64,
    value: Value,
}
fn documents() -> &'static Mutex<HashMap<usize, Box<Document>>> {
    static DOCUMENTS: OnceLock<Mutex<HashMap<usize, Box<Document>>>> = OnceLock::new();
    DOCUMENTS.get_or_init(Default::default)
}
fn text<'a>(pointer: *const c_char) -> Result<&'a str> {
    if pointer.is_null() {
        return Err("JSON string argument cannot be null".into());
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .map_err(|_| "JSON strings must be UTF-8".into())
}
fn run<T: Default>(operation: impl FnOnce() -> Result<T>) -> T {
    match operation() {
        Ok(value) => value,
        Err(error) => {
            crate::exceptions::set_pending(crate::exceptions::named("JsonError", error));
            T::default()
        }
    }
}
pub(crate) fn string(value: &str) -> Result<*mut c_char> {
    if value.contains('\0') {
        return Err("Ube strings cannot contain embedded NUL characters".into());
    }
    let pointer = crate::memory::ube_alloc(value.len() + 1, 1) as *mut c_char;
    if pointer.is_null() {
        return Err("Cannot allocate JSON string".into());
    }
    unsafe {
        std::ptr::copy_nonoverlapping(value.as_ptr(), pointer.cast(), value.len());
        *pointer.add(value.len()) = 0;
    }
    Ok(pointer)
}
fn document(value: Value) -> *mut c_void {
    let mut doc = Box::new(Document { id: 0, value });
    let pointer = (&mut *doc as *mut Document).cast();
    documents().lock().unwrap().insert(pointer as usize, doc);
    pointer
}
fn doc(pointer: *mut c_void) -> Result<Value> {
    documents()
        .lock()
        .unwrap()
        .get(&(pointer as usize))
        .map(|doc| doc.value.clone())
        .ok_or("Invalid JSON document handle".into())
}
#[no_mangle]
pub extern "C" fn ube_json_class(name: *const c_char) {
    if let Ok(name) = text(name) {
        schemas().lock().unwrap().entry(name.into()).or_default();
    }
}
#[no_mangle]
pub extern "C" fn ube_json_field(
    owner: *const c_char,
    name: *const c_char,
    child: *const c_char,
    kind: i64,
    offset: u64,
    payload: u64,
    presence: i64,
    width: u64,
) {
    if let (Ok(owner), Ok(name), Ok(child)) = (text(owner), text(name), text(child)) {
        schemas()
            .lock()
            .unwrap()
            .entry(owner.into())
            .or_default()
            .push(Field {
                name: name.into(),
                child: child.into(),
                kind,
                offset: offset as usize,
                payload: payload as usize,
                presence,
                width: width as usize,
            });
    }
}
/// Shared typed data traversal. Keep classes distinct from maps for XML names.
pub(crate) enum Data {
    Scalar(Value),
    List(Vec<Data>),
    Object { class: Option<String>, fields: Vec<(String, Data)> },
}
impl Data {
    fn from_json(value: Value) -> Self {
        match value {
            Value::Array(values) => Self::List(values.into_iter().map(Self::from_json).collect()),
            Value::Object(values) => Self::Object { class: None, fields: values.into_iter().map(|(key,value)| (key,Self::from_json(value))).collect() },
            value => Self::Scalar(value),
        }
    }
    fn into_json(self) -> Value {
        match self {
            Self::Scalar(value) => value,
            Self::List(values) => Value::Array(values.into_iter().map(Self::into_json).collect()),
            Self::Object { fields, .. } => Value::Object(fields.into_iter().map(|(key,value)| (key,value.into_json())).collect()),
        }
    }
}

struct Encoder {
    ancestry: HashSet<usize>,
    nodes: usize,
    bytes: usize,
    pin: usize,
}
impl Encoder {
    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.bytes = self.bytes.checked_add(bytes).ok_or("JSON size overflow")?;
        if self.bytes > MAX_BYTES {
            Err("JSON exceeds 16 MiB serialization budget".into())
        } else {
            Ok(())
        }
    }
    fn value(
        &mut self,
        payload: i64,
        meta: u64,
        kind: i64,
        name: &str,
        depth: usize,
    ) -> Result<Data> {
        self.nodes += 1;
        if depth > MAX_DEPTH || self.nodes > MAX_NODES {
            return Err("JSON nesting or node limit exceeded".into());
        }
        self.charge(8)?;
        if kind & 256 != 0 && meta == 0 {
            return Ok(Data::Scalar(Value::Null));
        }
        match kind&255 {
            1=>Ok(Data::Scalar(Value::from(payload as i32))),2=>Ok(Data::Scalar(Value::from(payload))),3=>Ok(Data::Scalar(Value::from(payload as u8))),4=>Ok(Data::Scalar(Value::Bool(payload!=0))),
            5=>Number::from_f64(f64::from_bits(payload as u64)).map(|value|Data::Scalar(Value::Number(value))).ok_or("Non-finite decimal cannot be represented as JSON".into()),
            6=>{if payload==0{return Ok(Data::Scalar(Value::Null));}let value=text(payload as *const c_char)?;self.charge(value.len())?;Ok(Data::Scalar(Value::String(value.into())))},
            7=>{
                if payload==0{return Ok(Data::Scalar(Value::Null));}
                if !self.ancestry.insert(payload as usize){return Err("Circular object reference cannot be represented as JSON".into());}
                let fields=schemas().lock().unwrap().get(name).cloned().ok_or_else(||format!("No JSON schema for {name}"))?;
                let mut object=Vec::new();
                for field in fields {
                    self.charge(field.name.len())?;
                    let base=payload as *mut u8;
                    let value=if (1..=5).contains(&(field.kind&255)) {
                        let presence=if field.presence<0{std::ptr::null()}else{unsafe{base.add(field.offset+field.presence as usize)}};
                        let scalar=crate::gc::ube_shared_load(unsafe{base.add(field.offset+field.payload)},presence,field.width);
                        self.value(scalar.payload,scalar.meta,field.kind,&field.child,depth+1)?
                    } else if field.kind&255==0 {
                        return Err(format!("Field '{}' has unsupported JSON type '{}'",field.name,field.child));
                    } else {
                        self.pin+=1;
                        let pointer=crate::gc::ube_gc_load_reference(payload as *mut c_void,field.offset,self.pin);
                        self.value(pointer as i64,1,field.kind,&field.child,depth+1)?
                    };
                    object.push((field.name,value));
                }
                self.ancestry.remove(&(payload as usize));Ok(Data::Object { class: Some(name.into()), fields: object })
            },
            8=>{if payload==0{Ok(Data::Scalar(Value::Null))}else{let value=doc(payload as *mut c_void)?;self.check(&value,depth)?;Ok(Data::from_json(value))}},
            9=>Ok(Data::Scalar(Value::Null)),
            10=>{
                if payload==0{return Ok(Data::Scalar(Value::Null));}
                if !self.ancestry.insert(payload as usize){return Err("Circular list reference cannot be represented as JSON".into());}
                let items=crate::collections::json_snapshot(payload as *mut crate::collections::UbeList, MAX_NODES.saturating_sub(self.nodes))?;
                let mut values=Vec::with_capacity(items.len().min(MAX_NODES));
                for (value,meta,kind,name) in items {
                    values.push(self.value(value,meta,kind,&name,depth+1)?);
                }
                self.ancestry.remove(&(payload as usize));
                Ok(Data::List(values))
            },
            11=>{
                if payload==0{return Ok(Data::Scalar(Value::Null));}
                if !self.ancestry.insert(payload as usize){return Err("Circular map reference cannot be represented as JSON".into());}
                let entries=crate::collections::json_map_snapshot(payload as *mut crate::collections::UbeMap, MAX_NODES.saturating_sub(self.nodes))?;
                let mut object=Vec::new();
                let mut keys=HashSet::new();
                for ((key,meta,kind,name),(value,value_meta,value_kind,value_name)) in entries {
                    if !(1..=6).contains(&(kind&255)) {
                        return Err("JSON map keys must be strings, finite numbers, or booleans".into());
                    }
                    let key=match self.value(key,meta,kind,&name,depth+1)? {
                        Data::Scalar(Value::String(key))=>key,
                        Data::Scalar(Value::Number(key))=>key.to_string(),
                        Data::Scalar(Value::Bool(key))=>key.to_string(),
                        _=>return Err("JSON map keys cannot be null".into()),
                    };
                    self.charge(key.len())?;
                    if !keys.insert(key.clone()){return Err(format!("Duplicate JSON map key after conversion: {key}"));}
                    let value=self.value(value,value_meta,value_kind,&value_name,depth+1)?;
                    object.push((key,value));
                }
                self.ancestry.remove(&(payload as usize));
                Ok(Data::Object { class: None, fields: object })
            },
            _=>Err(format!("Type '{name}' cannot be serialized to JSON; runtime resources and values without type metadata are unsupported")),
        }
    }
    fn check(&mut self, value: &Value, depth: usize) -> Result<()> {
        self.nodes += 1;
        if depth > MAX_DEPTH || self.nodes > MAX_NODES {
            return Err("JSON nesting or node limit exceeded".into());
        }
        self.charge(8)?;
        match value {
            Value::String(value) => self.charge(value.len())?,
            Value::Array(values) => {
                for value in values {
                    self.check(value, depth + 1)?;
                }
            }
            Value::Object(values) => {
                for (key, value) in values {
                    self.charge(key.len())?;
                    self.check(value, depth + 1)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
}
pub(crate) fn encode_data(payload: i64, meta: u64, kind: i64, name: &str) -> Result<Data> {
    crate::gc::ube_gc_frame_enter();
    let root = crate::gc::ube_gc_root_add(payload as *mut c_void);
    let mut encoder = Encoder { ancestry: HashSet::new(), nodes: 0, bytes: 0, pin: 0 };
    let result = encoder.value(payload, meta, kind, name, 0);
    crate::gc::ube_gc_root_remove(root);
    crate::gc::ube_gc_frame_leave(std::ptr::null_mut());
    result
}
#[no_mangle]
pub extern "C" fn ube_to_json(payload: i64, meta: u64, kind: i64, name: *const c_char) -> *mut c_char {
    run(|| {
        let value = encode_data(payload, meta, kind, text(name)?)?.into_json();
        let encoded = serde_json::to_string(&value).map_err(|e|e.to_string())?;
        if encoded.len() > MAX_BYTES { return Err("Encoded JSON exceeds 16 MiB".into()); }
        string(&encoded)
    })
}
pub(crate) fn parse(pointer: *const c_char) -> *mut c_void {
    let result = (|| {
        let text = text(pointer)?;
        if text.len() > MAX_BYTES {
            return Err("JSON input exceeds 16 MiB".to_string());
        }
        let value: Value = serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {e}"))?;
        Ok(document(value))
    })();
    match result {
        Ok(value) => value,
        Err(error) => {
            crate::exceptions::set_pending(crate::exceptions::named("JsonParseError", error));
            std::ptr::null_mut()
        }
    }
}
pub(crate) fn stringify(pointer: *mut c_void) -> *mut c_char {
    run(|| string(&serde_json::to_string(&doc(pointer)?).map_err(|e| e.to_string())?))
}
fn field(pointer: *mut c_void, key: *const c_char) -> Result<Value> {
    doc(pointer)?
        .as_object()
        .ok_or("JSON value is not an object")?
        .get(text(key)?)
        .cloned()
        .ok_or("JSON field is missing".into())
}
pub(crate) fn get_string(pointer: *mut c_void, key: *const c_char) -> *mut c_char {
    run(|| {
        string(
            field(pointer, key)?
                .as_str()
                .ok_or("JSON field is not a string")?,
        )
    })
}
pub(crate) fn get_long(pointer: *mut c_void, key: *const c_char) -> i64 {
    run(|| {
        field(pointer, key)?
            .as_i64()
            .ok_or("JSON field is not a signed integer".into())
    })
}
pub(crate) fn get_int(pointer: *mut c_void, key: *const c_char) -> i32 {
    run(|| {
        let value = field(pointer, key)?
            .as_i64()
            .ok_or("JSON field is not an integer")?;
        value
            .try_into()
            .map_err(|_| "JSON integer exceeds int range".into())
    })
}
pub(crate) fn get_decimal(pointer: *mut c_void, key: *const c_char) -> f64 {
    run(|| {
        field(pointer, key)?
            .as_f64()
            .ok_or("JSON field is not a number".into())
    })
}
pub(crate) fn get_bool(pointer: *mut c_void, key: *const c_char) -> bool {
    run(|| {
        field(pointer, key)?
            .as_bool()
            .ok_or("JSON field is not a bool".into())
    })
}
#[no_mangle]
pub extern "C" fn ube_jsonobject_get_object(
    pointer: *mut c_void,
    key: *const c_char,
) -> *mut c_void {
    run(|| {
        let value = field(pointer, key)?;
        if !value.is_object() {
            return Err("JSON field is not an object".into());
        }
        Ok(document(value))
    })
}
#[no_mangle]
pub extern "C" fn ube_jsonobject_get_array(
    pointer: *mut c_void,
    key: *const c_char,
) -> *mut c_void {
    run(|| {
        let value = field(pointer, key)?;
        if !value.is_array() {
            return Err("JSON field is not an array".into());
        }
        Ok(document(value))
    })
}
#[no_mangle]
pub extern "C" fn ube_jsonarray_len(pointer: *mut c_void) -> i32 {
    run(|| {
        doc(pointer)?
            .as_array()
            .ok_or("JSON value is not an array")?
            .len()
            .try_into()
            .map_err(|_| "JSON array too long".into())
    })
}
fn element(pointer: *mut c_void, index: i32) -> Result<Value> {
    let value = doc(pointer)?;
    let values = value.as_array().ok_or("JSON value is not an array")?;
    usize::try_from(index)
        .ok()
        .and_then(|index| values.get(index))
        .cloned()
        .ok_or("JSON array index out of bounds".into())
}
#[no_mangle]
pub extern "C" fn ube_jsonarray_get(pointer: *mut c_void, index: i32) -> *mut c_void {
    run(|| {
        let value = element(pointer, index)?;
        if !value.is_object() {
            return Err("JSON array element is not an object".into());
        }
        Ok(document(value))
    })
}
#[no_mangle]
pub extern "C" fn ube_jsonarray_get_string(pointer: *mut c_void, index: i32) -> *mut c_char {
    run(|| {
        string(
            element(pointer, index)?
                .as_str()
                .ok_or("JSON array element is not a string")?,
        )
    })
}
pub fn cleanup() {
    documents().lock().unwrap().clear();
}
