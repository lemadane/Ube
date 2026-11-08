//! Model-free SELECT builder over the shared SQL execution engine.
//! Mutations and snapshots hold the registry lock; database IO never does.
use crate::sql::{self, Dialect, Param};
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr, CString},
    sync::{Mutex, OnceLock},
};

type Handle = *mut c_void;
type Result<T> = std::result::Result<T, String>;
const MAX_PARTS: usize = 128;
const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
struct Filter {
    column: String,
    operator: String,
    value: Option<Param>,
}
#[derive(Clone)]
struct Query {
    target: usize,
    transaction: bool,
    dialect: Dialect,
    table: String,
    columns: Vec<String>,
    filters: Vec<Filter>,
    order: Vec<(String, String)>,
    limit: Option<i32>,
    offset: i32,
    bytes: usize,
}
#[derive(Default)]
struct Registry {
    next: usize,
    queries: HashMap<usize, Query>,
}
impl Registry {
    fn insert(&mut self, query: Query) -> Result<Handle> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or("query handle limit reached")?;
        self.queries.insert(self.next, query);
        Ok(self.next as Handle)
    }
}
fn registry() -> &'static Mutex<Registry> {
    static STATE: OnceLock<Mutex<Registry>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}
fn run<T: Default>(f: impl FnOnce() -> Result<T>) -> T {
    match f() {
        Ok(v) => v,
        Err(e) => {
            crate::exceptions::set_pending(crate::exceptions::named(
                "SQLError",
                format!("SQL builder: {e}"),
            ));
            T::default()
        }
    }
}
fn checked<T>(v: T) -> Result<T> {
    let error = crate::exceptions::take_pending();
    if error == 0 {
        Ok(v)
    } else {
        Err(crate::exceptions::message(error))
    }
}
fn text(ptr: *const c_char) -> Result<String> {
    if ptr.is_null() {
        return Err("null string argument".into());
    }
    let value = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| "invalid UTF-8 argument")?;
    if value.len() > MAX_BYTES {
        return Err("string exceeds 16 MiB".into());
    }
    Ok(value.to_owned())
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
fn mutate(h: Handle, bytes: usize, f: impl FnOnce(&mut Query) -> Result<()>) -> Result<Handle> {
    let mut state = registry().lock().unwrap();
    let q = state
        .queries
        .get_mut(&(h as usize))
        .ok_or("invalid or closed query handle")?;
    if q.bytes + bytes > MAX_BYTES {
        return Err("query exceeds 16 MiB".into());
    }
    f(q)?;
    q.bytes += bytes;
    Ok(h)
}
// Dots separate identifier components. SQL expressions and aliases are not parsed.
fn column(dialect: Dialect, value: &str, wildcard: bool) -> Result<String> {
    let parts = value.split('.').collect::<Vec<_>>();
    parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            if *part == "*" {
                if wildcard && index + 1 == parts.len() {
                    Ok("*".into())
                } else {
                    Err("wildcards are only allowed in projections".into())
                }
            } else {
                dialect.identifier(part)
            }
        })
        .collect::<Result<Vec<_>>>()
        .map(|parts| parts.join("."))
}
fn table(target: Handle, name: *const c_char, transaction: bool) -> Result<Handle> {
    let table = text(name)?;
    let (dialect, _) = sql::orm_target(target as usize, transaction)?;
    // Table names are one identifier component, matching ORM table mappings.
    dialect.identifier(&table)?;
    let bytes = table.len();
    registry().lock().unwrap().insert(Query {
        target: target as usize,
        transaction,
        dialect,
        table,
        columns: vec![],
        filters: vec![],
        order: vec![],
        limit: None,
        offset: 0,
        bytes,
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_table(h: Handle, name: *const c_char) -> Handle {
    run(|| table(h, name, false))
}
#[no_mangle]
pub extern "C" fn ube_sql_transaction_table(h: Handle, name: *const c_char) -> Handle {
    run(|| table(h, name, true))
}
#[no_mangle]
pub extern "C" fn ube_sql_builder_select(h: Handle, name: *const c_char) -> Handle {
    run(|| {
        let name = text(name)?;
        mutate(h, name.len(), |q| {
            if q.columns.len() >= MAX_PARTS {
                return Err("query exceeds 128 projections".into());
            }
            column(q.dialect, &name, true)?;
            q.columns.push(name);
            Ok(())
        })
    })
}
fn filter(h: Handle, name: *const c_char, operator: *const c_char, value: Param) -> Result<Handle> {
    let name = text(name)?;
    let operator = text(operator)?.to_ascii_uppercase();
    if !["=", "!=", "<>", "<", "<=", ">", ">=", "LIKE", "NOT LIKE"].contains(&operator.as_str()) {
        return Err("unsupported comparison operator".into());
    }
    if operator.contains("LIKE") && !matches!(value, Param::Text(_)) {
        return Err("LIKE requires a string value".into());
    }
    if matches!(value, Param::Double(v) if !v.is_finite()) {
        return Err("decimal must be finite".into());
    }
    let bytes = name.len()
        + operator.len()
        + match &value {
            Param::Text(v) => v.len(),
            _ => 8,
        };
    mutate(h, bytes, |q| {
        if q.filters.len() >= MAX_PARTS {
            return Err("query exceeds 128 predicates".into());
        }
        column(q.dialect, &name, false)?;
        q.filters.push(Filter {
            column: name,
            operator,
            value: Some(value),
        });
        Ok(())
    })
}
macro_rules! where_value {
    ($name:ident, $ty:ty, $variant:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(h: Handle, c: *const c_char, op: *const c_char, v: $ty) -> Handle {
            run(|| filter(h, c, op, Param::$variant(v)))
        }
    };
}
where_value!(ube_sql_builder_where_int, i32, Integer);
where_value!(ube_sql_builder_where_long, i64, BigInt);
where_value!(ube_sql_builder_where_decimal, f64, Double);
where_value!(ube_sql_builder_where_bool, bool, Bool);
#[no_mangle]
pub extern "C" fn ube_sql_builder_where_string(
    h: Handle,
    c: *const c_char,
    op: *const c_char,
    v: *const c_char,
) -> Handle {
    run(|| filter(h, c, op, Param::Text(text(v)?)))
}
fn null_filter(h: Handle, name: *const c_char, not: bool) -> Result<Handle> {
    let name = text(name)?;
    mutate(h, name.len() + 16, |q| {
        if q.filters.len() >= MAX_PARTS {
            return Err("query exceeds 128 predicates".into());
        }
        column(q.dialect, &name, false)?;
        q.filters.push(Filter {
            column: name,
            operator: if not { "IS NOT NULL" } else { "IS NULL" }.into(),
            value: None,
        });
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_builder_where_null(h: Handle, name: *const c_char) -> Handle {
    run(|| null_filter(h, name, false))
}
#[no_mangle]
pub extern "C" fn ube_sql_builder_where_not_null(h: Handle, name: *const c_char) -> Handle {
    run(|| null_filter(h, name, true))
}
#[no_mangle]
pub extern "C" fn ube_sql_builder_order_by(
    h: Handle,
    name: *const c_char,
    direction: *const c_char,
) -> Handle {
    run(|| {
        let name = text(name)?;
        let direction = text(direction)?.to_ascii_uppercase();
        if direction != "ASC" && direction != "DESC" {
            return Err("order direction must be asc or desc".into());
        }
        mutate(h, name.len() + direction.len(), |q| {
            if q.order.len() >= MAX_PARTS {
                return Err("query exceeds 128 sort keys".into());
            }
            column(q.dialect, &name, false)?;
            q.order.push((name, direction));
            Ok(())
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_builder_limit(h: Handle, value: i32) -> Handle {
    run(|| {
        if value < 0 {
            return Err("limit must be nonnegative".into());
        }
        mutate(h, 0, |q| {
            q.limit = Some(value);
            Ok(())
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_builder_offset(h: Handle, value: i32) -> Handle {
    run(|| {
        if value < 0 {
            return Err("offset must be nonnegative".into());
        }
        mutate(h, 0, |q| {
            q.offset = value;
            Ok(())
        })
    })
}
fn statement(q: &Query, first: bool, count: bool) -> Result<(String, Vec<Param>)> {
    let projection = if count {
        "COUNT(*)".into()
    } else if q.columns.is_empty() {
        "*".into()
    } else {
        q.columns
            .iter()
            .map(|c| column(q.dialect, c, true))
            .collect::<Result<Vec<_>>>()?
            .join(", ")
    };
    let mut sql = format!(
        "SELECT {projection} FROM {}",
        q.dialect.identifier(&q.table)?
    );
    let mut params = vec![];
    let filters = q
        .filters
        .iter()
        .map(|f| {
            let mut part = format!("{} {}", column(q.dialect, &f.column, false)?, f.operator);
            if let Some(v) = &f.value {
                params.push(v.clone());
                part.push_str(&if q.dialect == Dialect::Postgres {
                    format!(" ${}", params.len())
                } else {
                    " ?".into()
                });
            }
            Ok(part)
        })
        .collect::<Result<Vec<_>>>()?;
    if !filters.is_empty() {
        sql.push_str(&format!(" WHERE {}", filters.join(" AND ")));
    }
    if !count {
        if !q.order.is_empty() {
            sql.push_str(&format!(
                " ORDER BY {}",
                q.order
                    .iter()
                    .map(|(c, d)| Ok(format!("{} {d}", column(q.dialect, c, false)?)))
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
            match q.dialect {
                Dialect::Sqlite => sql.push_str(" LIMIT -1"),
                Dialect::Mysql => sql.push_str(" LIMIT 18446744073709551615"),
                Dialect::Postgres => {}
            }
        }
        if q.offset > 0 {
            sql.push_str(&format!(" OFFSET {}", q.offset));
        }
    }
    Ok((sql, params))
}
fn execute(q: &Query, first: bool, count: bool) -> Result<Handle> {
    let (sql, values) = statement(q, first, count)?;
    let sql = CString::new(sql).map_err(|_| "invalid SQL text")?;
    let params = sql::orm_params(values)?;
    let rows = if q.transaction {
        sql::ube_sql_transaction_query(q.target as Handle, sql.as_ptr(), params)
    } else {
        sql::ube_sql_query(q.target as Handle, sql.as_ptr(), params)
    };
    sql::ube_sql_params_close(params);
    checked(rows)
}
#[no_mangle]
pub extern "C" fn ube_sql_builder_all(h: Handle) -> Handle {
    run(|| execute(&query(h)?, false, false))
}
#[no_mangle]
pub extern "C" fn ube_sql_builder_first(h: Handle) -> Handle {
    run(|| execute(&query(h)?, true, false))
}
#[no_mangle]
pub extern "C" fn ube_sql_builder_count(h: Handle) -> i64 {
    run(|| {
        let rows = execute(&query(h)?, false, true)?;
        let value = (|| {
            if !checked(sql::ube_sql_result_next(rows))? {
                return Err("count returned no row".into());
            }
            checked(sql::ube_sql_result_get_long(rows, 0))
        })();
        sql::ube_sql_result_close(rows);
        value
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_builder_to_sql(h: Handle) -> *mut c_char {
    run(|| sql::string(&statement(&query(h)?, false, false)?.0))
}
#[no_mangle]
pub extern "C" fn ube_sql_builder_clone(h: Handle) -> Handle {
    run(|| {
        let q = query(h)?;
        registry().lock().unwrap().insert(q)
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_builder_close(h: Handle) {
    registry().lock().unwrap().queries.remove(&(h as usize));
}
pub fn cleanup() {
    registry().lock().unwrap().queries.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(dialect: Dialect) -> Query {
        Query {
            target: 0,
            transaction: false,
            dialect,
            table: "users".into(),
            columns: vec!["users.name".into()],
            filters: vec![
                Filter {
                    column: "id".into(),
                    operator: ">".into(),
                    value: Some(Param::Integer(1)),
                },
                Filter {
                    column: "note".into(),
                    operator: "IS NULL".into(),
                    value: None,
                },
                Filter {
                    column: "name".into(),
                    operator: "=".into(),
                    value: Some(Param::Text("' OR 1=1 --".into())),
                },
            ],
            order: vec![("id".into(), "ASC".into())],
            limit: None,
            offset: 2,
            bytes: 0,
        }
    }

    #[test]
    fn dialects_quote_identifiers_and_keep_values_bound() {
        for dialect in [Dialect::Postgres, Dialect::Mysql, Dialect::Sqlite] {
            let q = sample(dialect);
            let (sql, values) = statement(&q, false, false).unwrap();
            assert_eq!(values.len(), 2);
            assert!(!sql.contains("OR 1=1"));
            let quote = if dialect == Dialect::Mysql { '`' } else { '"' };
            assert!(sql.starts_with(&format!(
                "SELECT {quote}users{quote}.{quote}name{quote} FROM {quote}users{quote}"
            )));
            if dialect == Dialect::Postgres {
                assert!(sql.contains("\"id\" > $1 AND \"note\" IS NULL AND \"name\" = $2"));
                assert!(sql.ends_with("ASC OFFSET 2"));
            } else {
                assert_eq!(sql.matches('?').count(), 2);
                assert!(sql.ends_with(if dialect == Dialect::Mysql {
                    "LIMIT 18446744073709551615 OFFSET 2"
                } else {
                    "LIMIT -1 OFFSET 2"
                }));
            }
        }
    }

    #[test]
    fn first_honors_zero_limit_and_count_ignores_projection_and_pagination() {
        let mut q = sample(Dialect::Sqlite);
        assert!(statement(&q, true, false)
            .unwrap()
            .0
            .ends_with("LIMIT 1 OFFSET 2"));
        q.limit = Some(0);
        assert!(statement(&q, true, false)
            .unwrap()
            .0
            .ends_with("LIMIT 0 OFFSET 2"));
        let count = statement(&q, false, true).unwrap().0;
        assert!(count.starts_with("SELECT COUNT(*) FROM"));
        assert!(
            !count.contains("ORDER BY") && !count.contains("LIMIT") && !count.contains("OFFSET")
        );
    }

    #[test]
    fn identifiers_cannot_turn_into_expressions() {
        assert_eq!(
            column(Dialect::Postgres, "a\"; DROP TABLE users; --", false).unwrap(),
            "\"a\"\"; DROP TABLE users; --\""
        );
        assert_eq!(column(Dialect::Mysql, "a`b", false).unwrap(), "`a``b`");
        assert_eq!(
            column(Dialect::Sqlite, "users.*", true).unwrap(),
            "\"users\".*"
        );
        assert!(column(Dialect::Sqlite, "users..name", true).is_err());
        assert!(column(Dialect::Sqlite, "*", false).is_err());
        assert!(column(Dialect::Sqlite, "*.name", true).is_err());
    }
}
