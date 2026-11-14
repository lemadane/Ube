//! Shared signatures for sequential collection streams (separate from IO streams).
use crate::types::Type;

pub fn element_supported(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int
            | Type::Long
            | Type::Byte
            | Type::Decimal
            | Type::Bool
            | Type::String
            | Type::Class { .. }
    )
}

pub fn call(receiver: &Type, method: &str, args: &[Type]) -> Option<Result<Type, String>> {
    let error = || {
        Err(format!(
            "Invalid stream call '{method}': argument types or count do not match"
        ))
    };
    if matches!(receiver, Type::Class { name } if name == "Stream") {
        return Some(match method {
            "of" if args.iter().all(element_supported) && args.windows(2).all(|a| a[0] == a[1]) => {
                Ok(Type::Stream(Box::new(
                    args.first().cloned().unwrap_or(Type::Int),
                )))
            }
            "range" if args == [Type::Int, Type::Int] => Ok(Type::Stream(Box::new(Type::Int))),
            _ => error(),
        });
    }
    if matches!(receiver, Type::Class { name } if name == "List") && method == "stream" {
        return Some(if args.is_empty() {
            Ok(Type::Stream(Box::new(Type::Int)))
        } else {
            error()
        });
    }
    if let Type::Sequence(element) = receiver {
        return Some(match method {
            "stream" if args.is_empty() => Ok(Type::Stream(element.clone())),
            "length" if args.is_empty() => Ok(Type::Int),
            "get" if args == [Type::Int] => Ok(*element.clone()),
            "add" if args == [*element.clone()] => Ok(Type::Void),
            _ => error(),
        });
    }
    let Type::Stream(element) = receiver else {
        return None;
    };
    let element = element.as_ref();
    let func = |params: Vec<Type>, result: Type| Type::Function {
        params,
        return_type: Box::new(result),
    };
    let predicate = func(vec![element.clone()], Type::Bool);
    let consumer = func(vec![element.clone()], Type::Void);
    Some(match method {
        "filter" if args == [predicate.clone()] => Ok(receiver.clone()),
        "peek" if args == [consumer.clone()] => Ok(receiver.clone()),
        "map" => match args {
            [Type::Function {
                params,
                return_type,
            }] if params == &[element.clone()] && element_supported(return_type) => {
                Ok(Type::Stream(return_type.clone()))
            }
            _ => error(),
        },
        "limit" | "skip" if args == [Type::Int] || args == [Type::Long] => Ok(receiver.clone()),
        "unique" | "distinct" if args.is_empty() => Ok(receiver.clone()),
        "sort" | "sorted"
            if args.is_empty()
                && matches!(
                    element,
                    Type::Int | Type::Long | Type::Byte | Type::Decimal | Type::String
                ) =>
        {
            Ok(receiver.clone())
        }
        "count" if args.is_empty() => Ok(Type::Long),
        "toList" | "collect" if args.is_empty() => Ok(Type::Sequence(Box::new(element.clone()))),
        "forEach" if args == [consumer.clone()] => Ok(Type::Void),
        "some" | "every" | "none" | "anyMatch" | "allMatch" | "noneMatch"
            if args == [predicate.clone()] =>
        {
            Ok(Type::Bool)
        }
        "findFirst" if args.is_empty() || args == [predicate.clone()] => {
            Ok(Type::Nullable(Box::new(element.clone())))
        }
        "sum"
            if args.is_empty()
                && matches!(element, Type::Int | Type::Long | Type::Byte | Type::Decimal) =>
        {
            Ok(if *element == Type::Decimal {
                Type::Decimal
            } else {
                Type::Long
            })
        }
        "reduce"
            if args
                == [
                    element.clone(),
                    func(vec![element.clone(), element.clone()], element.clone()),
                ] =>
        {
            Ok(element.clone())
        }
        "close" if args.is_empty() => Ok(Type::Void),
        _ => error(),
    })
}
