//! Compile-time ORM model validation and field metadata.
use crate::{
    parser::{ClassDecl, TypeSpec},
    types::Type,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Model,
    Query,
    Rows,
}
impl Kind {
    pub fn class(self) -> &'static str {
        match self {
            Self::Model => "OrmModel",
            Self::Query => "OrmQuery",
            Self::Rows => "OrmRows",
        }
    }
}
pub fn handle(model: &str, kind: Kind) -> Type {
    Type::Orm {
        model: model.into(),
        kind,
        joins: vec![],
    }
}
pub fn scalar(spec: &TypeSpec) -> Option<Type> {
    Some(match spec {
        TypeSpec::Int => Type::Int,
        TypeSpec::Long => Type::Long,
        TypeSpec::Decimal => Type::Decimal,
        TypeSpec::Bool => Type::Bool,
        TypeSpec::String => Type::String,
        TypeSpec::Nullable(inner) if !matches!(inner.as_ref(), TypeSpec::Nullable(_)) => {
            Type::Nullable(Box::new(scalar(inner)?))
        }
        _ => return None,
    })
}
pub fn fields(class: &ClassDecl) -> Result<Vec<(String, Type)>, String> {
    if !class.cases.is_empty()
        || class.primary_params.is_empty()
        || class.primary_params.len() > 128
    {
        return Err(
            "ORM models must be ordinary classes with 1..128 scalar constructor fields".into(),
        );
    }
    let fields = class.primary_params.iter().map(|p| {
        if p.name.starts_with('_') { return Err("ORM model fields must be public constructor fields".into()); }
        Ok((p.name.clone(), scalar(&p.type_spec).ok_or_else(|| format!("ORM field '{}' must be int, long, decimal, bool, string, or a nullable scalar", p.name))?))
    }).collect::<Result<Vec<_>, String>>()?;
    if !fields
        .iter()
        .any(|(name, ty)| name == "id" && matches!(ty, Type::Int | Type::Long))
    {
        return Err("ORM models require a non-null int or long field named id".into());
    }
    Ok(fields)
}
pub fn schema(fields: &[(String, Type)]) -> String {
    fields
        .iter()
        .map(|(name, ty)| format!("{name}\t{ty}\n"))
        .collect()
}
pub fn generic(name: &str, spec: &TypeSpec) -> Option<Type> {
    let kind = match name {
        "OrmModel" => Kind::Model,
        "OrmQuery" => Kind::Query,
        "OrmRows" => Kind::Rows,
        _ => return None,
    };
    if let TypeSpec::Class(model) = spec {
        Some(handle(model, kind))
    } else {
        None
    }
}

pub fn alias_valid(alias: &str) -> bool {
    !alias.is_empty()
        && alias.len() <= 63
        && !alias.eq_ignore_ascii_case("base")
        && alias
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || i > 0 && b.is_ascii_digit())
}
pub fn joined(query: &Type, model: &str, alias: &str) -> Type {
    let Type::Orm {
        model: base, joins, ..
    } = query
    else {
        unreachable!()
    };
    let mut joins = joins.clone();
    joins.push((alias.into(), model.into()));
    Type::Orm {
        model: base.clone(),
        kind: Kind::Query,
        joins,
    }
}
