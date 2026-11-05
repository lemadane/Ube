//! Explicit, typed ORM operations over the existing SQL execution engine.
use crate::sql::{self, Dialect, Param};
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr, CString},
    sync::{Arc, Mutex, OnceLock},
};
type Handle = *mut c_void;
type Result<T> = std::result::Result<T, String>;
#[derive(Clone)]
struct Field {
    name: String,
    kind: String,
    nullable: bool,
}
#[derive(Clone)]
struct Model {
    target: usize,
    transaction: bool,
    dialect: Dialect,
    table: String,
    fields: Vec<Field>,
}
impl Model {
    fn field(&self, name: &str) -> Result<&Field> {
        self.fields
            .iter()
            .find(|f| f.name == name)
            .ok_or_else(|| "unknown model column".into())
    }
    fn quoted(&self, name: &str) -> Result<String> {
        self.field(name)?;
        self.dialect.identifier(name)
    }
    fn table(&self) -> Result<String> {
        self.dialect.identifier(&self.table)
    }
    fn placeholder(&self, index: usize) -> String {
        if self.dialect == Dialect::Postgres {
            format!("${index}")
        } else {
            "?".into()
        }
    }
    fn select(&self) -> Result<String> {
        Ok(format!(
            "SELECT {} FROM {}",
            self.fields
                .iter()
                .map(|f| {
                    let column = self.quoted(&f.name)?;
                    Ok(match (self.dialect, f.kind.as_str()) {
                        (Dialect::Mysql, "bool") => format!("CAST({column} AS SIGNED) AS {column}"),
                        _ => column,
                    })
                })
                .collect::<Result<Vec<_>>>()?
                .join(", "),
            self.table()?
        ))
    }
    fn id(&self, id: i64) -> Result<Param> {
        normalize(self.field("id")?, Param::BigInt(id))
    }
}
#[derive(Clone)]
struct Filter {
    column: String,
    operator: String,
    value: Option<Param>,
}
#[derive(Clone)]
struct Join {
    model: Arc<Model>,
    alias: String,
    source: String,
    target: String,
    left: bool,
}
#[derive(Clone)]
struct Query {
    model: Arc<Model>,
    joins: Vec<Join>,
    filters: Vec<Filter>,
    order: Vec<(String, String)>,
    limit: Option<i32>,
    offset: i32,
}
impl Query {
    fn column(&self, column: &str) -> Result<(&Model, &Field, String)> {
        let (alias, name) = column.split_once('.').unwrap_or(("base", column));
        let model = if alias == "base" {
            self.model.as_ref()
        } else {
            self.joins
                .iter()
                .find(|j| j.alias == alias)
                .map(|j| j.model.as_ref())
                .ok_or("unknown join alias")?
        };
        let field = model.field(name)?;
        let quoted = if self.joins.is_empty() {
            model.quoted(name)?
        } else {
            format!(
                "{}.{}",
                self.model.dialect.identifier(alias)?,
                model.quoted(name)?
            )
        };
        Ok((model, field, quoted))
    }
    fn selection(&self, count: bool) -> Result<String> {
        if self.joins.is_empty() {
            return if count {
                Ok(format!("SELECT COUNT(*) FROM {}", self.model.table()?))
            } else {
                self.model.select()
            };
        }
        let m = &self.model;
        let projection = if count {
            "COUNT(*)".into()
        } else {
            m.fields
                .iter()
                .map(|f| {
                    let col = self.column(&f.name)?.2;
                    Ok(if m.dialect == Dialect::Mysql && f.kind == "bool" {
                        format!("CAST({col} AS SIGNED) AS {}", m.quoted(&f.name)?)
                    } else {
                        col
                    })
                })
                .collect::<Result<Vec<_>>>()?
                .join(", ")
        };
        let mut result = format!(
            "SELECT {projection} FROM {} AS {}",
            m.table()?,
            m.dialect.identifier("base")?
        );
        for j in &self.joins {
            let alias = m.dialect.identifier(&j.alias)?;
            result.push_str(&format!(
                " {} JOIN {} AS {} ON {} = {}.{}",
                if j.left { "LEFT" } else { "INNER" },
                j.model.table()?,
                alias,
                self.column(&j.source)?.2,
                alias,
                j.model.quoted(&j.target)?
            ));
        }
        Ok(result)
    }
}
#[derive(Default)]
struct Registry {
    next: usize,
    models: HashMap<usize, Arc<Model>>,
    queries: HashMap<usize, Query>,
}
impl Registry {
    fn id(&mut self) -> Result<usize> {
        self.next = self.next.checked_add(1).ok_or("ORM handle limit reached")?;
        Ok(self.next)
    }
}
fn registry() -> &'static Mutex<Registry> {
    static STATE: OnceLock<Mutex<Registry>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}
fn text(ptr: *const c_char) -> Result<String> {
    if ptr.is_null() {
        return Err("null string argument".into());
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| "invalid UTF-8 argument".into())
}
fn run<T: Default>(f: impl FnOnce() -> Result<T>) -> T {
    match f() {
        Ok(v) => v,
        Err(e) => {
            crate::exceptions::set_pending(crate::exceptions::named(
                "SQLError",
                format!("ORM: {e}"),
            ));
            T::default()
        }
    }
}
fn checked<T>(value: T) -> Result<T> {
    let error = crate::exceptions::take_pending();
    if error == 0 {
        Ok(value)
    } else {
        Err(crate::exceptions::message(error))
    }
}
fn model(h: Handle) -> Result<Arc<Model>> {
    registry()
        .lock()
        .unwrap()
        .models
        .get(&(h as usize))
        .cloned()
        .ok_or_else(|| "invalid or closed model handle".into())
}
fn query(h: Handle) -> Result<Query> {
    registry()
        .lock()
        .unwrap()
        .queries
        .get(&(h as usize))
        .cloned()
        .ok_or_else(|| "invalid or closed query handle".into())
}
fn schema(input: &str) -> Result<Vec<Field>> {
    let mut fields = vec![];
    for line in input.lines() {
        let (name, ty) = line.split_once('\t').ok_or("invalid model metadata")?;
        let nullable = ty.ends_with('?');
        let kind = ty.trim_end_matches('?');
        if name.is_empty()
            || fields.iter().any(|f: &Field| f.name == name)
            || !["int", "long", "decimal", "bool", "string"].contains(&kind)
        {
            return Err("invalid model fields".into());
        }
        fields.push(Field {
            name: name.into(),
            kind: kind.into(),
            nullable,
        });
    }
    if fields.is_empty()
        || fields.len() > 128
        || !fields
            .iter()
            .any(|f| f.name == "id" && !f.nullable && matches!(f.kind.as_str(), "int" | "long"))
    {
        return Err("model requires 1..128 fields and an int/long id".into());
    }
    Ok(fields)
}
fn normalize(field: &Field, value: Param) -> Result<Param> {
    use sqlx::any::AnyTypeInfoKind as Kind;
    match (field.kind.as_str(), value) {
        (_, Param::Null(_)) if field.nullable => Ok(Param::Null(match field.kind.as_str() {
            "int" => Kind::Integer,
            "long" => Kind::BigInt,
            "decimal" => Kind::Double,
            "bool" => Kind::Bool,
            _ => Kind::Text,
        })),
        ("int", Param::Integer(v)) => Ok(Param::Integer(v)),
        ("int", Param::BigInt(v)) => Ok(Param::Integer(
            i32::try_from(v).map_err(|_| "model integer exceeds int range")?,
        )),
        ("long", Param::Integer(v)) => Ok(Param::BigInt(v as i64)),
        ("long", Param::BigInt(v)) => Ok(Param::BigInt(v)),
        ("decimal", Param::Double(v)) if v.is_finite() => Ok(Param::Double(v)),
        ("bool", Param::Bool(v)) => Ok(Param::Bool(v)),
        ("string", Param::Text(v)) => Ok(Param::Text(v)),
        _ => Err("value does not match the model field type or nullability".into()),
    }
}
fn execute(model: &Model, sql: &str, values: Vec<Param>, rows: bool) -> Result<usize> {
    sql::orm_target(model.target, model.transaction)?;
    let sql = CString::new(sql).map_err(|_| "invalid SQL text")?;
    let params = sql::orm_params(values)?;
    let target = model.target as Handle;
    let value = match (model.transaction, rows) {
        (true, true) => sql::ube_sql_transaction_query(target, sql.as_ptr(), params) as usize,
        (false, true) => sql::ube_sql_query(target, sql.as_ptr(), params) as usize,
        (true, false) => sql::ube_sql_transaction_execute(target, sql.as_ptr(), params) as usize,
        (false, false) => sql::ube_sql_execute(target, sql.as_ptr(), params) as usize,
    };
    let result = checked(value);
    sql::ube_sql_params_close(params);
    result
}
#[no_mangle]
pub extern "C" fn ube_orm_model(db: Handle, table: *const c_char, fields: *const c_char) -> Handle {
    run(|| {
        let (dialect, _) = sql::orm_target(db as usize, false)?;
        let table = text(table)?;
        dialect.identifier(&table)?;
        let fields = schema(&text(fields)?)?;
        let model = Model {
            target: db as usize,
            transaction: false,
            dialect,
            table,
            fields,
        };
        let mut s = registry().lock().unwrap();
        let id = s.id()?;
        s.models.insert(id, Arc::new(model));
        Ok(id as Handle)
    })
}
#[no_mangle]
pub extern "C" fn ube_orm_using(h: Handle, tx: Handle) -> Handle {
    run(|| {
        let original = model(h)?;
        let (_, owner) = sql::orm_target(tx as usize, true)?;
        let (_, expected_owner) = sql::orm_target(original.target, original.transaction)?;
        if owner != expected_owner {
            return Err("transaction belongs to another SQL connection".into());
        }
        let mut value = (*original).clone();
        value.target = tx as usize;
        value.transaction = true;
        let mut s = registry().lock().unwrap();
        let id = s.id()?;
        s.models.insert(id, Arc::new(value));
        Ok(id as Handle)
    })
}
#[no_mangle]
pub extern "C" fn ube_orm_migrate(h: Handle) {
    run(|| {
        let model = model(h)?;
        if model.transaction {
            return Err("migrate must run outside an application transaction".into());
        }
        let fields = model
            .fields
            .iter()
            .map(|f| {
                let ty = match (model.dialect, f.kind.as_str()) {
                    (_, "int") => "INTEGER",
                    (_, "long") => "BIGINT",
                    (Dialect::Postgres, "decimal") => "DOUBLE PRECISION",
                    (_, "decimal") => "DOUBLE",
                    (Dialect::Sqlite, "bool") => "INTEGER",
                    (_, "bool") => "BOOLEAN",
                    (Dialect::Mysql, "string") => "TEXT",
                    _ => "TEXT",
                };
                Ok(format!(
                    "{} {ty}{}{}",
                    model.quoted(&f.name)?,
                    if f.name == "id" { " PRIMARY KEY" } else { "" },
                    if !f.nullable { " NOT NULL" } else { "" }
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        execute(
            &model,
            &format!(
                "CREATE TABLE IF NOT EXISTS {} ({})",
                model.table()?,
                fields.join(", ")
            ),
            vec![],
            false,
        )?;
        Ok(())
    })
}
fn write(h: Handle, params: Handle, update: bool) -> Result<i64> {
    // Consume compiler-generated parameters even when model validation fails.
    let values = sql::orm_take_params(params)?;
    let model = model(h)?;
    if values.len() != model.fields.len() {
        return Err("model binding count mismatch".into());
    }
    let values = model
        .fields
        .iter()
        .zip(values)
        .map(|(f, v)| normalize(f, v))
        .collect::<Result<Vec<_>>>()?;
    let (statement, values) = if update {
        let mut updates = vec![];
        let mut args = vec![];
        let mut id = None;
        for (field, value) in model.fields.iter().zip(values) {
            if field.name == "id" {
                id = Some(value);
                continue;
            }
            args.push(value);
            updates.push(format!(
                "{} = {}",
                model.quoted(&field.name)?,
                model.placeholder(args.len())
            ));
        }
        if updates.is_empty() {
            updates.push(format!("{} = {}", model.quoted("id")?, model.quoted("id")?));
        }
        args.push(id.unwrap());
        (
            format!(
                "UPDATE {} SET {} WHERE {} = {}",
                model.table()?,
                updates.join(", "),
                model.quoted("id")?,
                model.placeholder(args.len())
            ),
            args,
        )
    } else {
        let fields = model
            .fields
            .iter()
            .map(|f| model.quoted(&f.name))
            .collect::<Result<Vec<_>>>()?
            .join(", ");
        let placeholders = (1..=values.len())
            .map(|i| model.placeholder(i))
            .collect::<Vec<_>>()
            .join(", ");
        (
            format!(
                "INSERT INTO {} ({fields}) VALUES ({placeholders})",
                model.table()?
            ),
            values,
        )
    };
    Ok(execute(&model, &statement, values, false)? as i64)
}
#[no_mangle]
pub extern "C" fn ube_orm_create(h: Handle, params: Handle) -> i64 {
    run(|| write(h, params, false))
}
#[no_mangle]
pub extern "C" fn ube_orm_save(h: Handle, params: Handle) -> i64 {
    run(|| write(h, params, true))
}
fn validate_rows(model: &Model, rows: Handle) -> Result<()> {
    let fields = model
        .fields
        .iter()
        .map(|f| (f.kind.clone(), f.nullable))
        .collect::<Vec<_>>();
    if let Err(e) = sql::orm_validate_rows(rows, &fields, model.dialect == Dialect::Mysql) {
        sql::ube_sql_result_close(rows);
        Err(e)
    } else {
        Ok(())
    }
}
fn first(model: &Model, statement: &str, params: Vec<Param>, mapped: bool) -> Result<Handle> {
    let rows = execute(model, statement, params, true)? as Handle;
    if mapped {
        validate_rows(model, rows)?;
    }
    match checked(sql::ube_sql_result_next(rows)) {
        Ok(true) => Ok(rows),
        Ok(false) => {
            sql::ube_sql_result_close(rows);
            Ok(std::ptr::null_mut())
        }
        Err(e) => {
            sql::ube_sql_result_close(rows);
            Err(e)
        }
    }
}
#[no_mangle]
pub extern "C" fn ube_orm_find(h: Handle, id: i64) -> Handle {
    run(|| {
        let model = model(h)?;
        first(
            &model,
            &format!(
                "{} WHERE {} = {} LIMIT 1",
                model.select()?,
                model.quoted("id")?,
                model.placeholder(1)
            ),
            vec![model.id(id)?],
            true,
        )
    })
}
#[no_mangle]
pub extern "C" fn ube_orm_get(h: Handle, id: i64) -> Handle {
    run(|| {
        let rows = checked(ube_orm_find(h, id))?;
        if rows.is_null() {
            Err("record not found".into())
        } else {
            Ok(rows)
        }
    })
}
#[no_mangle]
pub extern "C" fn ube_orm_delete(h: Handle, id: i64) -> i64 {
    run(|| {
        let model = model(h)?;
        Ok(execute(
            &model,
            &format!(
                "DELETE FROM {} WHERE {} = {}",
                model.table()?,
                model.quoted("id")?,
                model.placeholder(1)
            ),
            vec![model.id(id)?],
            false,
        )? as i64)
    })
}
#[no_mangle]
pub extern "C" fn ube_orm_model_close(h: Handle) {
    registry().lock().unwrap().models.remove(&(h as usize));
}
#[no_mangle]
pub extern "C" fn ube_orm_query(h: Handle) -> Handle {
    run(|| {
        let model = model(h)?;
        let mut s = registry().lock().unwrap();
        let id = s.id()?;
        s.queries.insert(
            id,
            Query {
                model,
                joins: vec![],
                filters: vec![],
                order: vec![],
                limit: None,
                offset: 0,
            },
        );
        Ok(id as Handle)
    })
}
#[no_mangle]
pub extern "C" fn ube_orm_join(
    h: Handle,
    other: Handle,
    alias: *const c_char,
    source: *const c_char,
    target: *const c_char,
    left: bool,
) -> Handle {
    run(|| {
        let mut q = query(h)?;
        let other = model(other)?;
        sql::orm_target(q.model.target, q.model.transaction)?;
        sql::orm_target(other.target, other.transaction)?;
        if q.model.target != other.target || q.model.transaction != other.transaction {
            return Err("joined models must use the same SQL connection or transaction".into());
        }
        let alias = text(alias)?;
        if alias.is_empty()
            || alias.len() > 63
            || alias.eq_ignore_ascii_case("base")
            || !alias
                .bytes()
                .enumerate()
                .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || i > 0 && b.is_ascii_digit())
            || q.joins.iter().any(|j| j.alias.eq_ignore_ascii_case(&alias))
        {
            return Err("join alias must be a unique ASCII identifier of 1..63 characters and cannot be base".into());
        }
        if q.joins.len() >= 16 {
            return Err("query exceeds 16 joins".into());
        }
        let source = text(source)?;
        let target = text(target)?;
        let (_, a, _) = q.column(&source)?;
        let b = other.field(&target)?;
        if a.kind != b.kind
            && !(matches!(a.kind.as_str(), "int" | "long")
                && matches!(b.kind.as_str(), "int" | "long"))
        {
            return Err("join columns must have compatible types".into());
        }
        q.joins.push(Join {
            model: other,
            alias,
            source,
            target,
            left,
        });
        let mut s = registry().lock().unwrap();
        let id = s.id()?;
        s.queries.insert(id, q);
        Ok(id as Handle)
    })
}
fn mutate(h: Handle, f: impl FnOnce(&mut Query) -> Result<()>) -> Result<Handle> {
    let mut s = registry().lock().unwrap();
    let q = s
        .queries
        .get_mut(&(h as usize))
        .ok_or("invalid or closed query handle")?;
    f(q)?;
    Ok(h)
}
fn filter(
    h: Handle,
    column: *const c_char,
    operator: *const c_char,
    value: Param,
) -> Result<Handle> {
    let column = text(column)?;
    let operator = text(operator)?.to_ascii_uppercase();
    if !["=", "!=", "<>", "<", "<=", ">", ">=", "LIKE", "NOT LIKE"].contains(&operator.as_str()) {
        return Err("unsupported comparison operator".into());
    }
    mutate(h, |q| {
        if q.filters.len() >= 128 {
            return Err("query exceeds 128 filters".into());
        }
        let (_, field, _) = q.column(&column)?;
        if operator.contains("LIKE") && field.kind != "string" {
            return Err("LIKE requires a string column".into());
        }
        let value = normalize(field, value)?;
        let size = |v: &Param| match v {
            Param::Text(v) => v.len(),
            Param::Blob(v) => v.len(),
            _ => 8,
        };
        if q.filters
            .iter()
            .filter_map(|f| f.value.as_ref())
            .map(size)
            .sum::<usize>()
            + size(&value)
            > 16 * 1024 * 1024
        {
            return Err("query parameters exceed 16 MiB".into());
        }
        q.filters.push(Filter {
            column,
            operator,
            value: Some(value),
        });
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn ube_orm_where_int(
    h: Handle,
    c: *const c_char,
    o: *const c_char,
    v: i32,
) -> Handle {
    run(|| filter(h, c, o, Param::Integer(v)))
}
#[no_mangle]
pub extern "C" fn ube_orm_where_long(
    h: Handle,
    c: *const c_char,
    o: *const c_char,
    v: i64,
) -> Handle {
    run(|| filter(h, c, o, Param::BigInt(v)))
}
#[no_mangle]
pub extern "C" fn ube_orm_where_decimal(
    h: Handle,
    c: *const c_char,
    o: *const c_char,
    v: f64,
) -> Handle {
    run(|| filter(h, c, o, Param::Double(v)))
}
#[no_mangle]
pub extern "C" fn ube_orm_where_bool(
    h: Handle,
    c: *const c_char,
    o: *const c_char,
    v: bool,
) -> Handle {
    run(|| filter(h, c, o, Param::Bool(v)))
}
#[no_mangle]
pub extern "C" fn ube_orm_where_string(
    h: Handle,
    c: *const c_char,
    o: *const c_char,
    v: *const c_char,
) -> Handle {
    run(|| filter(h, c, o, Param::Text(text(v)?)))
}
#[no_mangle]
pub extern "C" fn ube_orm_where_null(h: Handle, c: *const c_char, not: bool) -> Handle {
    run(|| {
        let column = text(c)?;
        mutate(h, |q| {
            q.column(&column)?;
            if q.filters.len() >= 128 {
                return Err("query exceeds 128 filters".into());
            }
            q.filters.push(Filter {
                column,
                operator: if not { "IS NOT NULL" } else { "IS NULL" }.into(),
                value: None,
            });
            Ok(())
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_orm_order_by(
    h: Handle,
    c: *const c_char,
    direction: *const c_char,
) -> Handle {
    run(|| {
        let column = text(c)?;
        let direction = text(direction)?.to_ascii_uppercase();
        if direction != "ASC" && direction != "DESC" {
            return Err("sort direction must be asc or desc".into());
        }
        mutate(h, |q| {
            q.column(&column)?;
            if q.order.len() >= 128 {
                return Err("too many sort columns".into());
            }
            q.order.push((column, direction));
            Ok(())
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_orm_limit(h: Handle, value: i32) -> Handle {
    run(|| {
        if value < 0 {
            return Err("limit must be nonnegative".into());
        }
        mutate(h, |q| {
            q.limit = Some(value);
            Ok(())
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_orm_offset(h: Handle, value: i32) -> Handle {
    run(|| {
        if value < 0 {
            return Err("offset must be nonnegative".into());
        }
        mutate(h, |q| {
            q.offset = value;
            Ok(())
        })
    })
}
fn statement(q: &Query, count: bool, first: bool) -> Result<(String, Vec<Param>)> {
    let m = &q.model;
    let mut sql = q.selection(count)?;
    let mut params = vec![];
    let mut filters = vec![];
    for f in &q.filters {
        let mut part = format!("{} {}", q.column(&f.column)?.2, f.operator);
        if let Some(v) = &f.value {
            params.push(v.clone());
            part.push_str(&format!(" {}", m.placeholder(params.len())));
        }
        filters.push(part);
    }
    if !filters.is_empty() {
        sql.push_str(&format!(" WHERE {}", filters.join(" AND ")));
    }
    if !count {
        if !q.order.is_empty() {
            sql.push_str(&format!(
                " ORDER BY {}",
                q.order
                    .iter()
                    .map(|(c, d)| Ok(format!("{} {d}", q.column(c)?.2)))
                    .collect::<Result<Vec<_>>>()?
                    .join(", ")
            ));
        }
        let limit = if first {
            Some(if q.limit == Some(0) { 0 } else { 1 })
        } else {
            q.limit
        };
        if let Some(limit) = limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        } else if q.offset > 0 {
            match m.dialect {
                Dialect::Mysql => sql.push_str(" LIMIT 18446744073709551615"),
                Dialect::Sqlite => sql.push_str(" LIMIT -1"),
                _ => {}
            }
        }
        if q.offset > 0 {
            sql.push_str(&format!(" OFFSET {}", q.offset));
        }
    }
    Ok((sql, params))
}
#[no_mangle]
pub extern "C" fn ube_orm_all(h: Handle) -> Handle {
    run(|| {
        let q = query(h)?;
        let (s, p) = statement(&q, false, false)?;
        let rows = execute(&q.model, &s, p, true)? as Handle;
        validate_rows(&q.model, rows)?;
        Ok(rows)
    })
}
#[no_mangle]
pub extern "C" fn ube_orm_first(h: Handle) -> Handle {
    run(|| {
        let q = query(h)?;
        let (s, p) = statement(&q, false, true)?;
        first(&q.model, &s, p, true)
    })
}
#[no_mangle]
pub extern "C" fn ube_orm_count(h: Handle) -> i64 {
    run(|| {
        let q = query(h)?;
        let (s, p) = statement(&q, true, false)?;
        let rows = first(&q.model, &s, p, false)?;
        if rows.is_null() {
            return Err("count returned no row".into());
        }
        let value = checked(sql::ube_sql_result_get_long(rows, 0));
        sql::ube_sql_result_close(rows);
        value
    })
}
#[no_mangle]
pub extern "C" fn ube_orm_query_close(h: Handle) {
    registry().lock().unwrap().queries.remove(&(h as usize));
}
pub fn cleanup() {
    let mut s = registry().lock().unwrap();
    s.queries.clear();
    s.models.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    fn c(v: &str) -> CString {
        CString::new(v).unwrap()
    }
    fn ok() {
        let e = crate::exceptions::take_pending();
        assert_eq!(e, 0, "{}", crate::exceptions::message(e));
    }
    #[test]
    fn dialects_quote_bind_and_paginate_without_interpolating_values() {
        for dialect in [Dialect::Postgres, Dialect::Mysql, Dialect::Sqlite] {
            let model = Arc::new(Model {
                target: 0,
                transaction: false,
                dialect,
                table: "order".into(),
                fields: schema("id\tlong\nname\tstring\nactive\tbool\n").unwrap(),
            });
            let q = Query {
                model,
                joins: vec![],
                filters: vec![
                    Filter {
                        column: "name".into(),
                        operator: "=".into(),
                        value: Some(Param::Text("'; DROP TABLE users; --".into())),
                    },
                    Filter {
                        column: "active".into(),
                        operator: "=".into(),
                        value: Some(Param::Bool(true)),
                    },
                ],
                order: vec![("id".into(), "DESC".into())],
                limit: None,
                offset: 3,
            };
            let (sql, values) = statement(&q, false, false).unwrap();
            assert!(!sql.contains("DROP"));
            assert_eq!(values.len(), 2);
            let quote = if dialect == Dialect::Mysql { '`' } else { '"' };
            assert!(sql.contains(&format!("FROM {quote}order{quote}")));
            if dialect == Dialect::Postgres {
                assert!(sql.contains("= $1"));
                assert!(sql.contains("= $2"));
                assert!(!sql.contains("LIMIT"));
            } else {
                assert_eq!(sql.matches('?').count(), 2);
                assert!(sql.contains(if dialect == Dialect::Sqlite {
                    "LIMIT -1"
                } else {
                    "LIMIT 18446744073709551615"
                }));
            }
            assert!(sql.ends_with("OFFSET 3"));
            let (count, _) = statement(&q, true, false).unwrap();
            assert!(!count.contains("LIMIT"));
            assert!(!count.contains("OFFSET"));
            assert!(!count.contains("ORDER BY"));
        }
    }
    #[test]
    #[ignore = "requires isolated PostgreSQL database in UBE_TEST_POSTGRES_URL"]
    fn postgres_live() {
        live(&std::env::var("UBE_TEST_POSTGRES_URL").unwrap());
    }
    #[test]
    #[ignore = "requires isolated MySQL/MariaDB database in UBE_TEST_MYSQL_URL"]
    fn mysql_live() {
        live(&std::env::var("UBE_TEST_MYSQL_URL").unwrap());
    }
    fn live(url: &str) {
        let db = sql::ube_sql_open(c(url).as_ptr());
        ok();
        assert!(!db.is_null());
        let empty = sql::ube_sql_params_create();
        sql::ube_sql_execute(
            db,
            c("DROP TABLE IF EXISTS ube_orm_integration").as_ptr(),
            empty,
        );
        ok();
        sql::ube_sql_params_close(empty);
        let model = ube_orm_model(
            db,
            c("ube_orm_integration").as_ptr(),
            c("id\tlong\nname\tstring\nactive\tbool\nage\tint?\n").as_ptr(),
        );
        ok();
        ube_orm_migrate(model);
        ok();
        let params = sql::ube_sql_params_create();
        sql::ube_sql_params_add_long(params, 1);
        sql::ube_sql_params_add_string(params, c("O'Brien 🍠").as_ptr());
        sql::ube_sql_params_add_bool(params, true);
        sql::ube_sql_params_add_null(params, c("int").as_ptr());
        assert_eq!(ube_orm_create(model, params), 1);
        ok();
        let rows = ube_orm_get(model, 1);
        ok();
        assert_eq!(sql::ube_sql_result_get_long(rows, 0), 1);
        assert_eq!(
            unsafe { CStr::from_ptr(sql::ube_sql_result_get_string(rows, 1)) }
                .to_str()
                .unwrap(),
            "O'Brien 🍠"
        );
        assert!(sql::ube_sql_result_get_bool(rows, 2));
        assert!(sql::ube_sql_result_is_null(rows, 3));
        ok();
        sql::ube_sql_result_close(rows);
        let q = ube_orm_query(model);
        ube_orm_where_bool(q, c("active").as_ptr(), c("=").as_ptr(), true);
        ok();
        assert_eq!(ube_orm_count(q), 1);
        ok();
        ube_orm_where_null(q, c("age").as_ptr(), false);
        ok();
        let rows = ube_orm_all(q);
        ok();
        assert!(sql::ube_sql_result_next(rows));
        sql::ube_sql_result_close(rows);
        ube_orm_query_close(q);
        let related = ube_orm_model(
            db,
            c("ube_orm_related").as_ptr(),
            c("id\tlong\nparentId\tlong\nname\tstring\n").as_ptr(),
        );
        ok();
        let empty = sql::ube_sql_params_create();
        sql::ube_sql_execute(
            db,
            c("DROP TABLE IF EXISTS ube_orm_related").as_ptr(),
            empty,
        );
        ok();
        sql::ube_sql_params_close(empty);
        ube_orm_migrate(related);
        ok();
        let values = sql::ube_sql_params_create();
        sql::ube_sql_params_add_long(values, 10);
        sql::ube_sql_params_add_long(values, 1);
        sql::ube_sql_params_add_string(values, c("related").as_ptr());
        assert_eq!(ube_orm_create(related, values), 1);
        ok();
        let base = ube_orm_query(model);
        let joined = ube_orm_join(
            base,
            related,
            c("r").as_ptr(),
            c("id").as_ptr(),
            c("parentId").as_ptr(),
            false,
        );
        ok();
        ube_orm_where_string(
            joined,
            c("r.name").as_ptr(),
            c("=").as_ptr(),
            c("related").as_ptr(),
        );
        ok();
        assert_eq!(ube_orm_count(joined), 1);
        ok();
        let rows = ube_orm_all(joined);
        ok();
        assert!(sql::ube_sql_result_next(rows));
        assert!(sql::ube_sql_result_get_bool(rows, 2));
        ok();
        sql::ube_sql_result_close(rows);
        assert_eq!(ube_orm_delete(related, 10), 1);
        ok();
        let left = ube_orm_join(
            base,
            related,
            c("r").as_ptr(),
            c("id").as_ptr(),
            c("parentId").as_ptr(),
            true,
        );
        ok();
        ube_orm_where_null(left, c("r.id").as_ptr(), false);
        ok();
        assert_eq!(ube_orm_count(left), 1);
        ok();
        assert_eq!(ube_orm_count(base), 1);
        ok();
        ube_orm_query_close(left);
        ube_orm_query_close(joined);
        ube_orm_query_close(base);
        ube_orm_model_close(related);
        let empty = sql::ube_sql_params_create();
        sql::ube_sql_execute(db, c("DROP TABLE ube_orm_related").as_ptr(), empty);
        ok();
        sql::ube_sql_params_close(empty);
        let tx = sql::ube_sql_begin(db);
        ok();
        let transactional = ube_orm_using(model, tx);
        ok();
        assert_eq!(ube_orm_delete(transactional, 1), 1);
        ok();
        sql::ube_sql_transaction_rollback(tx);
        ok();
        ube_orm_model_close(transactional);
        let rows = ube_orm_get(model, 1);
        ok();
        sql::ube_sql_result_close(rows);
        assert_eq!(ube_orm_delete(model, 1), 1);
        ok();
        ube_orm_model_close(model);
        let empty = sql::ube_sql_params_create();
        sql::ube_sql_execute(db, c("DROP TABLE ube_orm_integration").as_ptr(), empty);
        ok();
        sql::ube_sql_params_close(empty);
        sql::ube_sql_close(db);
        ok();
    }
}
