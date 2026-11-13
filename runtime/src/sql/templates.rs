//! Immutable GC-traced SQL templates. Nodes own inline text, never Rust heap
//! allocations, so collection needs no finalizer and failed construction leaks no handles.
use super::*;
use std::mem::{align_of, size_of};

#[repr(C)]
struct Node {
    previous: Handle,
    kind: u64, // 0 literal, 1 int, 2 long, 3 decimal, 4 bool, 5 string
    value: u64,
    len: usize,
    bytes: usize,
    params: usize,
}

fn append(previous: Handle, kind: u64, value: u64, text: &[u8]) -> Result<Handle> {
    let (bytes, params) = if previous.is_null() {
        (0, 0)
    } else {
        let node = unsafe { &*previous.cast::<Node>() };
        (node.bytes, node.params)
    };
    let bytes = bytes
        .checked_add(text.len().max(8))
        .ok_or("SQL template size overflow")?;
    let params = params + usize::from(kind != 0);
    if bytes > MAX_BYTES || params > MAX_PARAMETERS {
        return Err("SQL template exceeds 16 MiB or 32766 parameters".into());
    }
    let offset = 0usize;
    let allocation = crate::gc::ube_gc_allocate(
        size_of::<Node>() + text.len(),
        align_of::<Node>(),
        &offset,
        1,
    );
    if allocation.object.is_null() {
        return Err("SQL template allocation failed".into());
    }
    let pointer = allocation.object.cast::<Node>();
    unsafe {
        // The pointer field is published through the GC barrier below.
        (*pointer).kind = kind;
        (*pointer).value = value;
        (*pointer).len = text.len();
        (*pointer).bytes = bytes;
        (*pointer).params = params;
        std::ptr::copy_nonoverlapping(text.as_ptr(), pointer.add(1).cast::<u8>(), text.len());
    }
    crate::gc::ube_gc_store_reference(allocation.object, 0, previous);
    // Handoff root remains until the generated caller pins its result register.
    crate::gc::ube_gc_frame_pin(usize::MAX, allocation.object);
    crate::gc::ube_gc_root_remove(allocation.root);
    Ok(allocation.object)
}

#[no_mangle]
pub extern "C" fn ube_sql_template_empty() -> Handle {
    run(|| append(std::ptr::null_mut(), 0, 0, &[]))
}
#[no_mangle]
pub extern "C" fn ube_sql_template_literal(previous: Handle, value: *const c_char) -> Handle {
    run(|| append(previous, 0, 0, text(value)?.as_bytes()))
}
#[no_mangle]
pub extern "C" fn ube_sql_template_string(previous: Handle, value: *const c_char) -> Handle {
    run(|| append(previous, 5, 0, text(value)?.as_bytes()))
}
#[no_mangle]
pub extern "C" fn ube_sql_template_int(previous: Handle, value: i32) -> Handle {
    run(|| append(previous, 1, value as u64, &[]))
}
#[no_mangle]
pub extern "C" fn ube_sql_template_long(previous: Handle, value: i64) -> Handle {
    run(|| append(previous, 2, value as u64, &[]))
}
#[no_mangle]
pub extern "C" fn ube_sql_template_decimal(previous: Handle, value: f64) -> Handle {
    run(|| {
        if !value.is_finite() {
            return Err("SQL decimals must be finite".into());
        }
        append(previous, 3, value.to_bits(), &[])
    })
}
#[no_mangle]
pub extern "C" fn ube_sql_template_bool(previous: Handle, value: bool) -> Handle {
    run(|| append(previous, 4, u64::from(value), &[]))
}

fn materialize(statement: Handle, dialect: Dialect) -> Result<(String, Vec<Param>)> {
    if statement.is_null() {
        return Err("invalid SQLStatement".into());
    }
    let mut nodes = Vec::new();
    let mut pointer = statement;
    while !pointer.is_null() {
        let node = unsafe { &*pointer.cast::<Node>() };
        nodes.push(node);
        pointer = node.previous;
    }
    let mut sql = String::new();
    let mut params = Vec::new();
    for node in nodes.into_iter().rev() {
        let text = unsafe {
            std::slice::from_raw_parts((node as *const Node).add(1).cast::<u8>(), node.len)
        };
        let text = std::str::from_utf8(text).map_err(|_| "invalid SQL template text")?;
        let param = match node.kind {
            0 => {
                sql.push_str(text);
                continue;
            }
            1 => Param::Integer(node.value as i32),
            2 => Param::BigInt(node.value as i64),
            3 => Param::Double(f64::from_bits(node.value)),
            4 => Param::Bool(node.value != 0),
            5 => Param::Text(text.to_owned()),
            _ => return Err("invalid SQL template binding".into()),
        };
        params.push(param);
        if dialect == Dialect::Postgres {
            sql.push_str(&format!("${}", params.len()));
        } else {
            sql.push('?');
        }
    }
    Ok((sql, params))
}

fn execute_template(target: Handle, statement: Handle, query: bool, tx: bool) -> Result<Outcome> {
    let (dialect, _) = orm_target(target as usize, tx)?;
    let (sql, params) = materialize(statement, dialect)?;
    execute_owned(target, sql, params, query, tx)
}
#[no_mangle]
pub extern "C" fn ube_sql_statement_query(target: Handle, statement: Handle) -> Handle {
    run(|| Ok(store(execute_template(target, statement, true, false)?)? as Handle))
}
#[no_mangle]
pub extern "C" fn ube_sql_statement_execute(target: Handle, statement: Handle) -> i64 {
    run(
        || match execute_template(target, statement, false, false)? {
            Outcome::Count(n) => Ok(n),
            _ => unreachable!(),
        },
    )
}
#[no_mangle]
pub extern "C" fn ube_sql_transaction_statement_query(target: Handle, statement: Handle) -> Handle {
    run(|| Ok(store(execute_template(target, statement, true, true)?)? as Handle))
}
#[no_mangle]
pub extern "C" fn ube_sql_transaction_statement_execute(target: Handle, statement: Handle) -> i64 {
    run(|| match execute_template(target, statement, false, true)? {
        Outcome::Count(n) => Ok(n),
        _ => unreachable!(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Frame;
    impl Frame {
        fn enter() -> Self {
            crate::gc::ube_gc_frame_enter();
            Self
        }
    }
    impl Drop for Frame {
        fn drop(&mut self) {
            crate::gc::ube_gc_frame_leave(std::ptr::null_mut());
        }
    }

    #[test]
    fn dialect_placeholders_and_typed_values_remain_separate() {
        let _frame = Frame::enter();
        let mut q = append(std::ptr::null_mut(), 0, 0, b"SELECT ").unwrap();
        q = append(q, 5, 0, b"'; DROP TABLE users; --").unwrap();
        q = append(q, 0, 0, b", ").unwrap();
        q = append(q, 2, 5_000_000_000, &[]).unwrap();
        q = append(q, 0, 0, b", ").unwrap();
        q = append(q, 3, 1.5f64.to_bits(), &[]).unwrap();
        q = append(q, 0, 0, b", ").unwrap();
        q = append(q, 4, 1, &[]).unwrap();
        crate::gc::ube_gc_collect();
        for (dialect, expected) in [
            (Dialect::Postgres, "SELECT $1, $2, $3, $4"),
            (Dialect::Mysql, "SELECT ?, ?, ?, ?"),
            (Dialect::Sqlite, "SELECT ?, ?, ?, ?"),
        ] {
            let (sql, values) = materialize(q, dialect).unwrap();
            assert_eq!(sql, expected);
            assert!(
                matches!(&values[..], [Param::Text(s), Param::BigInt(5_000_000_000), Param::Double(n), Param::Bool(true)] if s == "'; DROP TABLE users; --" && *n == 1.5)
            );
        }
    }

    #[test]
    fn construction_limits_and_nonfinite_values_raise_sql_errors() {
        let _frame = Frame::enter();
        let q = ube_sql_template_empty();
        assert!(ube_sql_template_decimal(q, f64::INFINITY).is_null());
        let error = crate::exceptions::take_pending();
        assert_ne!(error, 0);
        assert!(crate::exceptions::message(error).contains("finite"));
        assert!(append(q, 5, 0, &vec![b'x'; MAX_BYTES]).is_err());
        assert_eq!(materialize(q, Dialect::Sqlite).unwrap().0, "");
    }
}
