use crate::builtins::canonical_builtin_class_name;
use crate::diagnostics::Diagnostic;
use crate::parser::*;
use crate::resolver::{Symbol, SymbolTable};
use crate::types::Type;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct ClassInfo {
    pub name: String,
    pub fields: HashMap<String, (Type, bool)>,
    pub methods: HashMap<String, (Vec<Type>, Type)>,
    pub param_types: Vec<Type>,
    pub min_constructor_args: usize,
    pub max_constructor_args: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitInfo {
    pub name: String,
    pub methods: HashMap<String, (Vec<Type>, Type)>,
    pub defaults: std::collections::HashSet<String>,
}

pub struct TypeChecker {
    symbol_table: SymbolTable,
    classes: HashMap<String, ClassInfo>,
    traits: HashMap<String, TraitInfo>,
    current_class: Option<String>,
    current_return_type: Option<Type>,
    in_expression_closure: bool,
    closure_returns: Option<Vec<Type>>,
    in_static_context: bool,
    orm_declarations: HashMap<String, ClassDecl>,
    zero_init_classes: std::collections::HashSet<String>,
    loop_depth: usize,
    pub diagnostics: Vec<Diagnostic>,
}

impl TypeChecker {
    pub fn new() -> Self {
        let mut checker = Self {
            symbol_table: SymbolTable::new(),
            classes: HashMap::new(),
            traits: HashMap::new(),
            current_class: None,
            current_return_type: None,
            in_expression_closure: false,
            closure_returns: None,
            in_static_context: false,
            orm_declarations: HashMap::new(),
            zero_init_classes: Default::default(),
            loop_depth: 0,
            diagnostics: Vec::new(),
        };
        checker.register_builtins();
        checker
    }

    fn register_builtins(&mut self) {
        for (name, methods) in [("Mockery", vec![]), ("Log", vec!["info", "warn", "debug", "error", "setLevel"]), ("Console", vec!["print", "printLine"])] {
            self.classes.insert(name.into(), ClassInfo {
                name: name.into(), fields: HashMap::new(),
                methods: methods.into_iter().map(|name| (name.into(), (vec![Type::String], Type::Void))).collect(),
                param_types: vec![], min_constructor_args: 0, max_constructor_args: 0,
            });
        }

        for method in ["read", "readLine"] {
            self.classes.get_mut("Console").unwrap().methods.insert(
                method.into(), (vec![], Type::Nullable(Box::new(Type::String))),
            );
        }

        self.classes.get_mut("Console").unwrap().methods.insert("args".into(), (vec![], Type::Class { name: "ConsoleArgs".into() }));
        self.classes.insert("ConsoleArgs".into(), ClassInfo {
            name: "ConsoleArgs".into(), fields: HashMap::new(),
            methods: crate::builtins::console_args_methods().into_iter().map(|m| (m.name.into(), (m.params, m.result))).collect(),
            param_types: vec![], min_constructor_args: 0, max_constructor_args: 0,
        });
        if let Some(log) = self.classes.get_mut("Log") {
            log.methods.insert("put".into(), (vec![Type::String, Type::String], Type::Void));
            log.methods.insert("get".into(), (vec![Type::String], Type::Nullable(Box::new(Type::String))));
            log.methods.insert("remove".into(), (vec![Type::String], Type::Void));
            log.methods.insert("clear".into(), (vec![], Type::Void));
        }

        let mut error_trait_methods = HashMap::new();
        error_trait_methods.insert("message".into(), (Vec::new(), Type::String));
        self.traits.insert(
            "Error".into(),
            TraitInfo {
                name: "Error".into(),
                methods: error_trait_methods,
                defaults: Default::default(),
            },
        );

        let mut generic_error_methods = HashMap::new();
        generic_error_methods.insert("message".into(), (Vec::new(), Type::String));
        generic_error_methods.insert("stacktrace".into(), (Vec::new(), Type::String));
        let mut generic_error_fields = HashMap::new();
        generic_error_fields.insert("_message".into(), (Type::String, false));

        self.classes.insert(
            "GenericError".into(),
            ClassInfo {
                name: "GenericError".into(),
                fields: generic_error_fields,
                methods: generic_error_methods,
                param_types: vec![Type::String],
                min_constructor_args: 1,
                max_constructor_args: 1,
            },
        );

        self.classes.insert("Testicle".into(), ClassInfo {
            name: "Testicle".into(), fields: HashMap::new(), methods: HashMap::new(),
            param_types: Vec::new(), min_constructor_args: 0, max_constructor_args: 0,
        });
        self.classes.insert("Gc".into(), ClassInfo {
            name: "Gc".into(), fields: HashMap::new(),
            methods: [("collect".into(), (Vec::new(), Type::Bool)), ("objects".into(), (Vec::new(), Type::Long)), ("reclaimed".into(), (Vec::new(), Type::Long))].into_iter().collect(),
            param_types: Vec::new(), min_constructor_args: 0, max_constructor_args: 0,
        });
        let mut channel_methods = HashMap::new();
        channel_methods.insert(
            "send".into(),
            (vec![Type::Int], Type::Async(Box::new(Type::Bool))),
        );
        channel_methods.insert(
            "receive".into(),
            (Vec::new(), Type::Async(Box::new(Type::Int))),
        );
        channel_methods.insert("close".into(), (Vec::new(), Type::Void));
        self.classes.insert(
            "Channel".into(),
            ClassInfo {
                name: "Channel".into(),
                fields: HashMap::new(),
                methods: channel_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut task_methods = HashMap::new();
        task_methods.insert(
            "all".into(),
            (
                vec![Type::Class {
                    name: "List".into(),
                }],
                Type::Async(Box::new(Type::Class {
                    name: "List".into(),
                })),
            ),
        );
        task_methods.insert(
            "any".into(),
            (
                vec![Type::Class {
                    name: "List".into(),
                }],
                Type::Async(Box::new(Type::Int)),
            ),
        );
        task_methods.insert(
            "race".into(),
            (
                vec![Type::Class {
                    name: "List".into(),
                }],
                Type::Async(Box::new(Type::Int)),
            ),
        );
        task_methods.insert(
            "allSettled".into(),
            (
                vec![Type::Class {
                    name: "List".into(),
                }],
                Type::Async(Box::new(Type::Class {
                    name: "TaskOutcomes".into(),
                })),
            ),
        );
        task_methods.insert("cancel".into(), (Vec::new(), Type::Bool));

        self.classes.insert(
            "Async".into(),
            ClassInfo {
                name: "Async".into(),
                fields: HashMap::new(),
                methods: task_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut datetime_methods = HashMap::new();
        datetime_methods.insert(
            "now".into(),
            (
                Vec::new(),
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );
        datetime_methods.insert(
            "date".into(),
            (
                vec![Type::Int, Type::Int, Type::Int],
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );
        datetime_methods.insert(
            "time".into(),
            (
                vec![Type::Int, Type::Int],
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );
        datetime_methods.insert(
            "of".into(),
            (
                vec![Type::Int, Type::Int, Type::Int, Type::Int, Type::Int],
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );
        datetime_methods.insert(
            "utc".into(),
            (
                Vec::new(),
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );
        datetime_methods.insert(
            "parse".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );
        datetime_methods.insert(
            "fromUnix".into(),
            (
                vec![Type::Int],
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );
        datetime_methods.insert(
            "fromUnixMillis".into(),
            (
                vec![Type::Int],
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );

        datetime_methods.insert("year".into(), (Vec::new(), Type::Int));
        datetime_methods.insert("month".into(), (Vec::new(), Type::Int));
        datetime_methods.insert("day".into(), (Vec::new(), Type::Int));
        datetime_methods.insert("hour".into(), (Vec::new(), Type::Int));
        datetime_methods.insert("minute".into(), (Vec::new(), Type::Int));
        datetime_methods.insert("second".into(), (Vec::new(), Type::Int));
        datetime_methods.insert("timestamp".into(), (Vec::new(), Type::Int));
        datetime_methods.insert("unix".into(), (Vec::new(), Type::Int));
        datetime_methods.insert("unixMillis".into(), (Vec::new(), Type::Int));
        datetime_methods.insert("unixNanos".into(), (Vec::new(), Type::Int));
        datetime_methods.insert("zone".into(), (Vec::new(), Type::String));
        datetime_methods.insert(
            "in".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );
        datetime_methods.insert(
            "addDays".into(),
            (
                vec![Type::Int],
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );
        datetime_methods.insert(
            "addHours".into(),
            (
                vec![Type::Int],
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );
        datetime_methods.insert(
            "addMinutes".into(),
            (
                vec![Type::Int],
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );
        datetime_methods.insert(
            "addDuration".into(),
            (
                vec![Type::Class {
                    name: "Duration".into(),
                }],
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );
        datetime_methods.insert(
            "diffSeconds".into(),
            (
                vec![Type::Class {
                    name: "DateTime".into(),
                }],
                Type::Int,
            ),
        );
        datetime_methods.insert(
            "diffDays".into(),
            (
                vec![Type::Class {
                    name: "DateTime".into(),
                }],
                Type::Int,
            ),
        );
        datetime_methods.insert("format".into(), (vec![Type::String], Type::String));

        self.classes.insert(
            "DateTime".into(),
            ClassInfo {
                name: "DateTime".into(),
                fields: HashMap::new(),
                methods: datetime_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut duration_methods = HashMap::new();
        duration_methods.insert(
            "seconds".into(),
            (
                vec![Type::Int],
                Type::Class {
                    name: "Duration".into(),
                },
            ),
        );
        duration_methods.insert(
            "minutes".into(),
            (
                vec![Type::Int],
                Type::Class {
                    name: "Duration".into(),
                },
            ),
        );
        duration_methods.insert(
            "hours".into(),
            (
                vec![Type::Int],
                Type::Class {
                    name: "Duration".into(),
                },
            ),
        );
        duration_methods.insert(
            "days".into(),
            (
                vec![Type::Int],
                Type::Class {
                    name: "Duration".into(),
                },
            ),
        );
        duration_methods.insert("totalSeconds".into(), (Vec::new(), Type::Int));

        self.classes.insert(
            "Duration".into(),
            ClassInfo {
                name: "Duration".into(),
                fields: HashMap::new(),
                methods: duration_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut currency_methods = HashMap::new();
        currency_methods.insert("code".into(), (Vec::new(), Type::String));
        currency_methods.insert(
            "PHP".into(),
            (
                Vec::new(),
                Type::Class {
                    name: "Currency".into(),
                },
            ),
        );
        currency_methods.insert(
            "USD".into(),
            (
                Vec::new(),
                Type::Class {
                    name: "Currency".into(),
                },
            ),
        );
        currency_methods.insert(
            "EUR".into(),
            (
                Vec::new(),
                Type::Class {
                    name: "Currency".into(),
                },
            ),
        );
        currency_methods.insert(
            "JPY".into(),
            (
                Vec::new(),
                Type::Class {
                    name: "Currency".into(),
                },
            ),
        );
        currency_methods.insert(
            "GBP".into(),
            (
                Vec::new(),
                Type::Class {
                    name: "Currency".into(),
                },
            ),
        );

        self.classes.insert(
            "Currency".into(),
            ClassInfo {
                name: "Currency".into(),
                fields: HashMap::new(),
                methods: currency_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut money_methods = HashMap::new();
        money_methods.insert(
            "php".into(),
            (
                vec![Type::Decimal],
                Type::Class {
                    name: "Money".into(),
                },
            ),
        );
        money_methods.insert(
            "usd".into(),
            (
                vec![Type::Decimal],
                Type::Class {
                    name: "Money".into(),
                },
            ),
        );
        money_methods.insert(
            "eur".into(),
            (
                vec![Type::Decimal],
                Type::Class {
                    name: "Money".into(),
                },
            ),
        );
        money_methods.insert(
            "jpy".into(),
            (
                vec![Type::Decimal],
                Type::Class {
                    name: "Money".into(),
                },
            ),
        );
        money_methods.insert(
            "gbp".into(),
            (
                vec![Type::Decimal],
                Type::Class {
                    name: "Money".into(),
                },
            ),
        );
        money_methods.insert("amount".into(), (Vec::new(), Type::Decimal));
        money_methods.insert("amountText".into(), (Vec::new(), Type::String));
        money_methods.insert("parse".into(), (vec![Type::String, Type::String], Type::Class { name: "Money".into() }));
        money_methods.insert("multiplyPercent".into(), (vec![Type::String], Type::Class { name: "Money".into() }));
        money_methods.insert("convertExact".into(), (vec![Type::String, Type::String], Type::Class { name: "Money".into() }));
        money_methods.insert(
            "currency".into(),
            (
                Vec::new(),
                Type::Class {
                    name: "Currency".into(),
                },
            ),
        );
        money_methods.insert(
            "add".into(),
            (
                vec![Type::Class {
                    name: "Money".into(),
                }],
                Type::Class {
                    name: "Money".into(),
                },
            ),
        );
        money_methods.insert(
            "subtract".into(),
            (
                vec![Type::Class {
                    name: "Money".into(),
                }],
                Type::Class {
                    name: "Money".into(),
                },
            ),
        );
        money_methods.insert(
            "multiply".into(),
            (
                vec![Type::Class {
                    name: "Percent".into(),
                }],
                Type::Class {
                    name: "Money".into(),
                },
            ),
        );

        self.classes.insert(
            "Money".into(),
            ClassInfo {
                name: "Money".into(),
                fields: HashMap::new(),
                methods: money_methods,
                param_types: vec![
                    Type::Decimal,
                    Type::Class {
                        name: "Currency".into(),
                    },
                ],
                min_constructor_args: 2,
                max_constructor_args: 2,
            },
        );

        let mut exchange_methods = HashMap::new();
        exchange_methods.insert(
            "convert".into(),
            (
                vec![
                    Type::Class {
                        name: "Money".into(),
                    },
                    Type::Class {
                        name: "Currency".into(),
                    },
                    Type::Decimal,
                ],
                Type::Class {
                    name: "Money".into(),
                },
            ),
        );

        self.classes.insert(
            "Exchange".into(),
            ClassInfo {
                name: "Exchange".into(),
                fields: HashMap::new(),
                methods: exchange_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut percent_methods = HashMap::new();
        percent_methods.insert(
            "of".into(),
            (
                vec![Type::Decimal],
                Type::Class {
                    name: "Percent".into(),
                },
            ),
        );
        percent_methods.insert("value".into(), (Vec::new(), Type::Decimal));

        self.classes.insert(
            "Percent".into(),
            ClassInfo {
                name: "Percent".into(),
                fields: HashMap::new(),
                methods: percent_methods,
                param_types: vec![Type::Decimal],
                min_constructor_args: 1,
                max_constructor_args: 1,
            },
        );

        let mut rate_methods = HashMap::new();
        rate_methods.insert(
            "percent".into(),
            (
                vec![Type::Decimal],
                Type::Class {
                    name: "Rate".into(),
                },
            ),
        );
        rate_methods.insert("value".into(), (Vec::new(), Type::Decimal));

        self.classes.insert(
            "Rate".into(),
            ClassInfo {
                name: "Rate".into(),
                fields: HashMap::new(),
                methods: rate_methods,
                param_types: vec![Type::Decimal],
                min_constructor_args: 1,
                max_constructor_args: 1,
            },
        );

        let mut geopoint_methods = HashMap::new();
        geopoint_methods.insert("latitude".into(), (Vec::new(), Type::Decimal));
        geopoint_methods.insert("longitude".into(), (Vec::new(), Type::Decimal));
        geopoint_methods.insert(
            "distanceTo".into(),
            (
                vec![Type::Class {
                    name: "GeoPoint".into(),
                }],
                Type::Class {
                    name: "Distance".into(),
                },
            ),
        );
        geopoint_methods.insert(
            "bearingTo".into(),
            (
                vec![Type::Class {
                    name: "GeoPoint".into(),
                }],
                Type::Decimal,
            ),
        );

        self.classes.insert(
            "GeoPoint".into(),
            ClassInfo {
                name: "GeoPoint".into(),
                fields: HashMap::new(),
                methods: geopoint_methods,
                param_types: vec![Type::Decimal, Type::Decimal],
                min_constructor_args: 2,
                max_constructor_args: 2,
            },
        );

        let mut distance_methods = HashMap::new();
        distance_methods.insert(
            "meters".into(),
            (
                vec![Type::Decimal],
                Type::Class {
                    name: "Distance".into(),
                },
            ),
        );
        distance_methods.insert(
            "kilometers".into(),
            (
                vec![Type::Decimal],
                Type::Class {
                    name: "Distance".into(),
                },
            ),
        );
        distance_methods.insert(
            "miles".into(),
            (
                vec![Type::Decimal],
                Type::Class {
                    name: "Distance".into(),
                },
            ),
        );

        self.classes.insert(
            "Distance".into(),
            ClassInfo {
                name: "Distance".into(),
                fields: HashMap::new(),
                methods: distance_methods,
                param_types: vec![Type::Decimal],
                min_constructor_args: 0,
                max_constructor_args: 1,
            },
        );

        let mut geobounds_methods = HashMap::new();
        geobounds_methods.insert(
            "southwest".into(),
            (
                Vec::new(),
                Type::Class {
                    name: "GeoPoint".into(),
                },
            ),
        );
        geobounds_methods.insert(
            "northeast".into(),
            (
                Vec::new(),
                Type::Class {
                    name: "GeoPoint".into(),
                },
            ),
        );
        geobounds_methods.insert(
            "contains".into(),
            (
                vec![Type::Class {
                    name: "GeoPoint".into(),
                }],
                Type::Bool,
            ),
        );

        self.classes.insert(
            "GeoBounds".into(),
            ClassInfo {
                name: "GeoBounds".into(),
                fields: HashMap::new(),
                methods: geobounds_methods,
                param_types: vec![
                    Type::Class {
                        name: "GeoPoint".into(),
                    },
                    Type::Class {
                        name: "GeoPoint".into(),
                    },
                ],
                min_constructor_args: 2,
                max_constructor_args: 2,
            },
        );

        let mut geolocation_methods = HashMap::new();
        geolocation_methods.insert(
            "current".into(),
            (
                Vec::new(),
                Type::Class {
                    name: "GeoPoint".into(),
                },
            ),
        );

        self.classes.insert(
            "Geolocation".into(),
            ClassInfo {
                name: "Geolocation".into(),
                fields: HashMap::new(),
                methods: geolocation_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut err_methods = HashMap::new();
        err_methods.insert("message".into(), (Vec::new(), Type::String));
        err_methods.insert("stacktrace".into(), (Vec::new(), Type::String));

        self.classes.insert(
            "LocationPermissionError".into(),
            ClassInfo {
                name: "LocationPermissionError".into(),
                fields: HashMap::new(),
                methods: err_methods.clone(),
                param_types: vec![Type::String],
                min_constructor_args: 0,
                max_constructor_args: 1,
            },
        );

        self.classes.insert(
            "LocationUnavailableError".into(),
            ClassInfo {
                name: "LocationUnavailableError".into(),
                fields: HashMap::new(),
                methods: err_methods,
                param_types: vec![Type::String],
                min_constructor_args: 0,
                max_constructor_args: 1,
            },
        );

        let mut math_methods = HashMap::new();
        math_methods.insert("PI".into(), (Vec::new(), Type::Decimal));
        math_methods.insert("E".into(), (Vec::new(), Type::Decimal));
        math_methods.insert("TAU".into(), (Vec::new(), Type::Decimal));

        math_methods.insert("abs".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert(
            "min".into(),
            (vec![Type::Decimal, Type::Decimal], Type::Decimal),
        );
        math_methods.insert(
            "max".into(),
            (vec![Type::Decimal, Type::Decimal], Type::Decimal),
        );
        math_methods.insert("sqrt".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert("cbrt".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert(
            "pow".into(),
            (vec![Type::Decimal, Type::Decimal], Type::Decimal),
        );

        math_methods.insert("floor".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert("ceil".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert("round".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert("truncate".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert(
            "clamp".into(),
            (
                vec![Type::Decimal, Type::Decimal, Type::Decimal],
                Type::Decimal,
            ),
        );
        math_methods.insert("sign".into(), (vec![Type::Decimal], Type::Int));

        math_methods.insert("sin".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert("cos".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert("tan".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert("asin".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert("acos".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert("atan".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert(
            "atan2".into(),
            (vec![Type::Decimal, Type::Decimal], Type::Decimal),
        );
        math_methods.insert("radians".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert("degrees".into(), (vec![Type::Decimal], Type::Decimal));

        math_methods.insert("log".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert("log10".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert("log2".into(), (vec![Type::Decimal], Type::Decimal));
        math_methods.insert("exp".into(), (vec![Type::Decimal], Type::Decimal));

        math_methods.insert("isNaN".into(), (vec![Type::Decimal], Type::Bool));
        math_methods.insert("isInfinite".into(), (vec![Type::Decimal], Type::Bool));
        math_methods.insert("isFinite".into(), (vec![Type::Decimal], Type::Bool));
        math_methods.insert("isEven".into(), (vec![Type::Int], Type::Bool));
        math_methods.insert("isOdd".into(), (vec![Type::Int], Type::Bool));
        math_methods.insert("gcd".into(), (vec![Type::Int, Type::Int], Type::Int));
        math_methods.insert("lcm".into(), (vec![Type::Int, Type::Int], Type::Int));

        self.classes.insert(
            "Math".into(),
            ClassInfo {
                name: "Math".into(),
                fields: HashMap::new(),
                methods: math_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut random_methods = HashMap::new();
        random_methods.insert("int".into(), (vec![Type::Int, Type::Int], Type::Int));
        random_methods.insert("decimal".into(), (Vec::new(), Type::Decimal));
        random_methods.insert("bool".into(), (Vec::new(), Type::Bool));
        random_methods.insert("uuid".into(), (Vec::new(), Type::String));
        random_methods.insert(
            "bytes".into(),
            (
                vec![Type::Int],
                Type::Class {
                    name: "List".into(),
                },
            ),
        );

        self.classes.insert(
            "Random".into(),
            ClassInfo {
                name: "Random".into(),
                fields: HashMap::new(),
                methods: random_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut secure_random_methods = HashMap::new();
        secure_random_methods.insert(
            "bytes".into(),
            (
                vec![Type::Int],
                Type::Class {
                    name: "List".into(),
                },
            ),
        );

        self.classes.insert(
            "SecureRandom".into(),
            ClassInfo {
                name: "SecureRandom".into(),
                fields: HashMap::new(),
                methods: secure_random_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut statistics_methods = HashMap::new();
        statistics_methods.insert(
            "mean".into(),
            (
                vec![Type::Class {
                    name: "List".into(),
                }],
                Type::Decimal,
            ),
        );
        statistics_methods.insert(
            "median".into(),
            (
                vec![Type::Class {
                    name: "List".into(),
                }],
                Type::Decimal,
            ),
        );
        statistics_methods.insert(
            "variance".into(),
            (
                vec![Type::Class {
                    name: "List".into(),
                }],
                Type::Decimal,
            ),
        );
        statistics_methods.insert(
            "standardDeviation".into(),
            (
                vec![Type::Class {
                    name: "List".into(),
                }],
                Type::Decimal,
            ),
        );
        statistics_methods.insert(
            "percentile".into(),
            (
                vec![
                    Type::Class {
                        name: "List".into(),
                    },
                    Type::Decimal,
                ],
                Type::Decimal,
            ),
        );
        statistics_methods.insert(
            "covariance".into(),
            (
                vec![
                    Type::Class {
                        name: "List".into(),
                    },
                    Type::Class {
                        name: "List".into(),
                    },
                ],
                Type::Decimal,
            ),
        );
        statistics_methods.insert(
            "correlation".into(),
            (
                vec![
                    Type::Class {
                        name: "List".into(),
                    },
                    Type::Class {
                        name: "List".into(),
                    },
                ],
                Type::Decimal,
            ),
        );

        self.classes.insert(
            "Statistics".into(),
            ClassInfo {
                name: "Statistics".into(),
                fields: HashMap::new(),
                methods: statistics_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut probability_methods = HashMap::new();
        probability_methods.insert("factorial".into(), (vec![Type::Int], Type::Int));
        probability_methods.insert(
            "permutations".into(),
            (vec![Type::Int, Type::Int], Type::Int),
        );
        probability_methods.insert(
            "combinations".into(),
            (vec![Type::Int, Type::Int], Type::Int),
        );

        probability_methods.insert(
            "normal".into(),
            (
                vec![Type::Decimal, Type::Decimal, Type::Decimal],
                Type::Decimal,
            ),
        );
        probability_methods.insert(
            "binomial".into(),
            (vec![Type::Int, Type::Int, Type::Decimal], Type::Decimal),
        );
        probability_methods.insert(
            "poisson".into(),
            (vec![Type::Int, Type::Decimal], Type::Decimal),
        );
        probability_methods.insert(
            "uniform".into(),
            (
                vec![Type::Decimal, Type::Decimal, Type::Decimal],
                Type::Decimal,
            ),
        );

        self.classes.insert(
            "Probability".into(),
            ClassInfo {
                name: "Probability".into(),
                fields: HashMap::new(),
                methods: probability_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        for &name in crate::builtins::SQLITE_CLASSES {
            self.classes.insert(name.into(), ClassInfo {
                name: name.into(), fields: HashMap::new(),
                methods: crate::builtins::sqlite_methods(name).into_iter()
                    .map(|m| (m.name.into(), (m.params, m.result))).collect(),
                param_types: vec![], min_constructor_args: 0, max_constructor_args: 0,
            });
        }

        for &name in crate::builtins::SQL_CLASSES {
            self.classes.insert(name.into(), ClassInfo {
                name: name.into(), fields: HashMap::new(),
                methods: crate::builtins::sql_methods(name).into_iter().map(|m| (m.name.into(), (m.params, m.result))).collect(),
                param_types: vec![], min_constructor_args: 0, max_constructor_args: 0,
            });
        }
        for &name in crate::rest_in_peace::CLASSES {
            self.classes.insert(name.into(), ClassInfo {
                name: name.into(), fields: HashMap::new(),
                methods: crate::rest_in_peace::methods(name).into_iter().map(|m| (m.name.into(), (m.params, m.result))).collect(),
                param_types: vec![], min_constructor_args: 0, max_constructor_args: 0,
            });
        }
        for name in ["TaskOutcome", "TaskOutcomes"] {
            self.classes.insert(name.into(), ClassInfo {
                name: name.into(), fields: HashMap::new(),
                methods: crate::builtins::task_outcome_methods(name).into_iter().map(|m| (m.name.into(), (m.params, m.result))).collect(),
                param_types: vec![], min_constructor_args: 0, max_constructor_args: 0,
            });
        }
        self.classes.insert("KeyValueStore".into(), ClassInfo {
            name: "KeyValueStore".into(), fields: HashMap::new(),
            methods: crate::builtins::key_value_store_methods().into_iter().map(|m| (m.name.into(), (m.params, m.result))).collect(),
            param_types: vec![], min_constructor_args: 0, max_constructor_args: 0,
        });
        self.classes.insert("Cache".into(), ClassInfo {
            name: "Cache".into(), fields: HashMap::new(),
            methods: crate::builtins::cache_methods().into_iter().map(|m| (m.name.into(), (m.params, m.result))).collect(),
            param_types: vec![], min_constructor_args: 0, max_constructor_args: 0,
        });
        for &name in crate::builtins::OBSERVABILITY_CLASSES {
            self.classes.insert(name.into(), ClassInfo {
                name: name.into(), fields: HashMap::new(),
                methods: crate::builtins::observability_methods(name).into_iter().map(|m| (m.name.into(), (m.params, m.result))).collect(),
                param_types: vec![], min_constructor_args: 0, max_constructor_args: 0,
            });
        }
        self.classes.insert("Env".into(), ClassInfo {
            name: "Env".into(), fields: HashMap::new(),
            methods: crate::builtins::env_methods().into_iter().map(|m| (m.name.into(), (m.params, m.result))).collect(),
            param_types: vec![], min_constructor_args: 0, max_constructor_args: 0,
        });
        for &name in crate::builtins::AUTH_CLASSES {
            self.classes.insert(name.into(), ClassInfo {
                name: name.into(), fields: HashMap::new(),
                methods: crate::builtins::auth_methods(name).into_iter().map(|m| (m.name.into(), (m.params, m.result))).collect(),
                param_types: vec![], min_constructor_args: 0, max_constructor_args: 0,
            });
        }
        let mut file_methods = HashMap::new();
        file_methods.insert(
            "open".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "File".into(),
                },
            ),
        );
        file_methods.insert("read".into(), (vec![Type::String], Type::String));
        file_methods.insert(
            "write".into(),
            (vec![Type::String, Type::String], Type::Void),
        );
        file_methods.insert(
            "append".into(),
            (vec![Type::String, Type::String], Type::Void),
        );
        file_methods.insert("delete".into(), (vec![Type::String], Type::Void));
        file_methods.insert("exists".into(), (vec![Type::String], Type::Bool));
        file_methods.insert(
            "info".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "FileInfo".into(),
                },
            ),
        );
        file_methods.insert("close".into(), (Vec::new(), Type::Void));

        self.classes.insert(
            "File".into(),
            ClassInfo {
                name: "File".into(),
                fields: HashMap::new(),
                methods: file_methods,
                param_types: vec![Type::String],
                min_constructor_args: 0,
                max_constructor_args: 1,
            },
        );

        let mut fileinfo_methods = HashMap::new();
        fileinfo_methods.insert("size".into(), (Vec::new(), Type::Long));
        fileinfo_methods.insert(
            "createdAt".into(),
            (
                Vec::new(),
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );
        fileinfo_methods.insert(
            "modifiedAt".into(),
            (
                Vec::new(),
                Type::Class {
                    name: "DateTime".into(),
                },
            ),
        );
        fileinfo_methods.insert("isDirectory".into(), (Vec::new(), Type::Bool));

        self.classes.insert(
            "FileInfo".into(),
            ClassInfo {
                name: "FileInfo".into(),
                fields: HashMap::new(),
                methods: fileinfo_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut directory_methods = HashMap::new();
        directory_methods.insert("create".into(), (vec![Type::String], Type::Void));
        directory_methods.insert("delete".into(), (vec![Type::String], Type::Void));
        directory_methods.insert(
            "list".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "List".into(),
                },
            ),
        );

        self.classes.insert(
            "Directory".into(),
            ClassInfo {
                name: "Directory".into(),
                fields: HashMap::new(),
                methods: directory_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut path_methods = HashMap::new();
        path_methods.insert(
            "join".into(),
            (vec![Type::String, Type::String], Type::String),
        );
        path_methods.insert("filename".into(), (vec![Type::String], Type::String));
        path_methods.insert("extension".into(), (vec![Type::String], Type::String));

        self.classes.insert(
            "Path".into(),
            ClassInfo {
                name: "Path".into(),
                fields: HashMap::new(),
                methods: path_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let file_errors = vec![
            "RestInPeaceError",
            "MockeryError",
            "AssertionError",
            "MoneyError",
            "CurrencyMismatchError",
            "SQLError",
            "SQLiteError",
            "FileError",
            "FileNotFoundError",
            "PermissionError",
            "AlreadyExistsError",
            "InvalidPathError",
            "DiskFullError",
        ];
        for err_name in file_errors {
            let mut err_methods = HashMap::new();
            err_methods.insert("message".into(), (Vec::new(), Type::String));
            err_methods.insert("stacktrace".into(), (Vec::new(), Type::String));

            self.classes.insert(
                err_name.into(),
                ClassInfo {
                    name: err_name.into(),
                    fields: HashMap::new(),
                    methods: err_methods,
                    param_types: vec![Type::String],
                    min_constructor_args: 0,
                    max_constructor_args: 1,
                },
            );
        }

        let response_handler_type = Type::Function {
            params: vec![Type::Class {
                name: "Request".into(),
            }],
            return_type: Box::new(Type::Class {
                name: "Response".into(),
            }),
        };
        let mut server_methods = HashMap::new();
        server_methods.insert(
            "get".into(),
            (
                vec![Type::String, response_handler_type.clone()],
                Type::Void,
            ),
        );
        server_methods.insert(
            "post".into(),
            (
                vec![Type::String, response_handler_type.clone()],
                Type::Void,
            ),
        );
        server_methods.insert(
            "put".into(),
            (
                vec![Type::String, response_handler_type.clone()],
                Type::Void,
            ),
        );
        server_methods.insert(
            "patch".into(),
            (
                vec![Type::String, response_handler_type.clone()],
                Type::Void,
            ),
        );
        server_methods.insert(
            "delete".into(),
            (
                vec![Type::String, response_handler_type.clone()],
                Type::Void,
            ),
        );
        server_methods.insert(
            "options".into(),
            (
                vec![Type::String, response_handler_type.clone()],
                Type::Void,
            ),
        );
        server_methods.insert(
            "head".into(),
            (
                vec![Type::String, response_handler_type.clone()],
                Type::Void,
            ),
        );
        server_methods.insert(
            "use".into(),
            (
                vec![
                    Type::String,
                    Type::Class {
                        name: "Function".into(),
                    },
                ],
                Type::Void,
            ),
        );
        server_methods.insert(
            "static".into(),
            (vec![Type::String, Type::String], Type::Void),
        );
        server_methods.insert(
            "websocket".into(),
            (
                vec![
                    Type::String,
                    Type::Class {
                        name: "Function".into(),
                    },
                ],
                Type::Void,
            ),
        );
        server_methods.insert("start".into(), (vec![Type::Int], Type::Void));
        server_methods.insert("stop".into(), (Vec::new(), Type::Void));
        server_methods.insert("port".into(), (Vec::new(), Type::Int));
        server_methods.insert("tls".into(), (vec![Type::String, Type::String], Type::Void));
        server_methods.insert("enableHttp2".into(), (Vec::new(), Type::Void));
        server_methods.insert("enableHttp3".into(), (vec![Type::Int], Type::Void));
        server_methods.insert("http3Port".into(), (vec![], Type::Int));

        self.classes.insert(
            "Server".into(),
            ClassInfo {
                name: "Server".into(),
                fields: HashMap::new(),
                methods: server_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let session_methods = HashMap::from([
            ("get".into(), (vec![Type::String], Type::Nullable(Box::new(Type::String)))),
            ("set".into(), (vec![Type::String, Type::String], Type::Void)),
            ("remove".into(), (vec![Type::String], Type::Void)),
            ("regenerate".into(), (vec![], Type::Void)),
            ("destroy".into(), (vec![], Type::Void)),
        ]);
        self.classes.insert("Session".into(), ClassInfo {
            name: "Session".into(), fields: HashMap::new(), methods: session_methods,
            param_types: vec![], min_constructor_args: 0, max_constructor_args: 0,
        });
        let mut request_methods = HashMap::new();
        request_methods.insert("session".into(), (vec![], Type::Class { name: "Session".into() }));
        request_methods.insert("path".into(), (vec![Type::String], Type::String));
        request_methods.insert("pathLong".into(), (vec![Type::String], Type::Long));
        request_methods.insert("pathInt".into(), (vec![Type::String], Type::Int));
        request_methods.insert("query".into(), (vec![Type::String], Type::String));
        request_methods.insert("queryDecimal".into(), (vec![Type::String], Type::Decimal));
        request_methods.insert("header".into(), (vec![Type::String], Type::String));
        request_methods.insert("cookie".into(), (vec![Type::String], Type::String));
        request_methods.insert("hasBearerToken".into(), (Vec::new(), Type::Bool));
        request_methods.insert("body".into(), (Vec::new(), Type::String));
        request_methods.insert("method".into(), (Vec::new(), Type::String));
        request_methods.insert("url".into(), (Vec::new(), Type::String));
        request_methods.insert("ip".into(), (Vec::new(), Type::String));
        request_methods.insert("protocol".into(), (Vec::new(), Type::String));
        request_methods.insert("isHttp3".into(), (Vec::new(), Type::Bool));

        self.classes.insert(
            "Request".into(),
            ClassInfo {
                name: "Request".into(),
                fields: HashMap::new(),
                methods: request_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut response_methods = HashMap::new();
        response_methods.insert(
            "ok".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "Response".into(),
                },
            ),
        );
        response_methods.insert(
            "json".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "Response".into(),
                },
            ),
        );
        response_methods.insert(
            "html".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "Response".into(),
                },
            ),
        );
        response_methods.insert(
            "text".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "Response".into(),
                },
            ),
        );
        response_methods.insert(
            "created".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "Response".into(),
                },
            ),
        );
        response_methods.insert(
            "notFound".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "Response".into(),
                },
            ),
        );
        response_methods.insert(
            "internalServerError".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "Response".into(),
                },
            ),
        );
        response_methods.insert(
            "unauthorized".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "Response".into(),
                },
            ),
        );
        response_methods.insert(
            "redirect".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "Response".into(),
                },
            ),
        );
        response_methods.insert(
            "status".into(),
            (
                vec![Type::Int],
                Type::Class {
                    name: "Response".into(),
                },
            ),
        );

        self.classes.insert(
            "Response".into(),
            ClassInfo {
                name: "Response".into(),
                fields: HashMap::new(),
                methods: response_methods,
                param_types: vec![Type::Int, Type::String],
                min_constructor_args: 0,
                max_constructor_args: 2,
            },
        );

        let mut cookie_methods = HashMap::new();
        cookie_methods.insert("name".into(), (Vec::new(), Type::String));
        cookie_methods.insert("value".into(), (Vec::new(), Type::String));

        self.classes.insert(
            "Cookie".into(),
            ClassInfo {
                name: "Cookie".into(),
                fields: HashMap::new(),
                methods: cookie_methods,
                param_types: vec![Type::String, Type::String],
                min_constructor_args: 2,
                max_constructor_args: 2,
            },
        );

        let mut websocket_methods = HashMap::new();
        websocket_methods.insert(
            "onMessage".into(),
            (
                vec![Type::Class {
                    name: "Function".into(),
                }],
                Type::Void,
            ),
        );
        websocket_methods.insert("send".into(), (vec![Type::String], Type::Void));
        websocket_methods.insert("close".into(), (Vec::new(), Type::Void));

        self.classes.insert(
            "WebSocket".into(),
            ClassInfo {
                name: "WebSocket".into(),
                fields: HashMap::new(),
                methods: websocket_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut json_methods = HashMap::new();
        json_methods.insert(
            "toObject".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "JsonObject".into(),
                },
            ),
        );
        json_methods.insert(
            "partialParse".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "JsonObject".into(),
                },
            ),
        );
        json_methods.insert(
            "to".into(),
            (
                vec![Type::String, Type::String],
                Type::Class {
                    name: "JsonObject".into(),
                },
            ),
        );

        self.classes.insert(
            "Json".into(),
            ClassInfo {
                name: "Json".into(),
                fields: HashMap::new(),
                methods: json_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut jsonobject_methods = HashMap::new();
        jsonobject_methods.insert("getString".into(), (vec![Type::String], Type::String));
        jsonobject_methods.insert("getInt".into(), (vec![Type::String], Type::Int));
        jsonobject_methods.insert("getLong".into(), (vec![Type::String], Type::Long));
        jsonobject_methods.insert("getDecimal".into(), (vec![Type::String], Type::Decimal));
        jsonobject_methods.insert("getBool".into(), (vec![Type::String], Type::Bool));
        jsonobject_methods.insert(
            "getObject".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "JsonObject".into(),
                },
            ),
        );
        jsonobject_methods.insert(
            "getArray".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "JsonArray".into(),
                },
            ),
        );

        self.classes.insert(
            "JsonObject".into(),
            ClassInfo {
                name: "JsonObject".into(),
                fields: HashMap::new(),
                methods: jsonobject_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut jsonarray_methods = HashMap::new();
        jsonarray_methods.insert("len".into(), (Vec::new(), Type::Int));
        jsonarray_methods.insert(
            "get".into(),
            (
                vec![Type::Int],
                Type::Class {
                    name: "JsonObject".into(),
                },
            ),
        );
        jsonarray_methods.insert("getString".into(), (vec![Type::Int], Type::String));

        self.classes.insert(
            "JsonArray".into(),
            ClassInfo {
                name: "JsonArray".into(),
                fields: HashMap::new(),
                methods: jsonarray_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        for name in crate::builtins::XML_CLASSES {
            self.classes.insert((*name).into(), ClassInfo {
                name:(*name).into(), fields:HashMap::new(),
                methods:crate::builtins::xml_methods(name).into_iter().map(|m|(m.name.into(),(m.params,m.result))).collect(),
                param_types:vec![],min_constructor_args:0,max_constructor_args:0,
            });
        }

        let format_errors = vec![
            "JsonError",
            "JsonParseError",
            "MissingFieldError",
            "InvalidTypeError",
            "XmlError",
            "XmlParseError",
        ];
        for err_name in format_errors {
            let mut err_methods = HashMap::new();
            err_methods.insert("message".into(), (Vec::new(), Type::String));
            err_methods.insert("stacktrace".into(), (Vec::new(), Type::String));

            self.classes.insert(
                err_name.into(),
                ClassInfo {
                    name: err_name.into(),
                    fields: HashMap::new(),
                    methods: err_methods,
                    param_types: vec![Type::String],
                    min_constructor_args: 0,
                    max_constructor_args: 1,
                },
            );
        }
    }

    pub fn check_program(&mut self, program: &Program) -> bool {
        self.diagnostics.clear();
        for item in &program.items {
            let declaration = match item {
                Item::Class(class) => Some((&class.name, class.span)),
                Item::Trait(contract) => Some((&contract.name, contract.span)),
                Item::Import(_) => None,
            };
            if let Some((name, span)) = declaration {
                if name == "Func" {
                    self.diagnostics.push(Diagnostic::error("Func is reserved for function types", Some(span)));
                }
                if name.starts_with(crate::mockery::PREFIX) {
                    self.diagnostics.push(Diagnostic::error("UbeMockery_ type names are reserved for generated mocks", Some(span)));
                }
            }
        }
        let expanded = crate::mockery::expand(program);
        let program = &expanded;
        self.orm_declarations = program.items.iter().filter_map(|item| if let Item::Class(c) = item { Some((c.name.clone(), c.clone())) } else { None }).collect();
        self.zero_init_classes = program.items.iter().filter_map(|item| {
            if let Item::Class(class) = item { Some(class.name.clone()) } else { None }
        }).collect();

        for item in &program.items {
            match item {
                Item::Trait(tr) => {
                    let mut methods = HashMap::new();
                    for method in &tr.methods {
                        let param_types = method
                            .params
                            .iter()
                            .map(|p| self.type_spec_to_type(&p.type_spec))
                            .collect();
                        let return_ty = method
                            .return_type
                            .as_ref()
                            .map_or(Type::Void, |t| self.type_spec_to_type(t));
                        methods.insert(method.name.clone(), (param_types, return_ty));
                    }
                    self.traits.insert(
                        tr.name.clone(),
                        TraitInfo {
                            name: tr.name.clone(),
                            methods,
                            defaults: tr.methods.iter().filter(|m| !m.body.is_empty()).map(|m| m.name.clone()).collect(),
                        },
                    );
                }
                Item::Class(cls) => {
                    let mut fields = HashMap::new();
                    let mut methods = HashMap::new();
                    let max_constructor_args = cls.primary_params.len();
                    let min_constructor_args = cls
                        .primary_params
                        .iter()
                        .filter(|p| p.default_value.is_none())
                        .count();

                    for reserved in ["toJson", "toXml"] {
                        if cls.methods.iter().any(|m|m.name==reserved)||cls.primary_params.iter().any(|p|p.name==reserved)||cls.static_fields.iter().any(|f|f.name==reserved) {
                            self.diagnostics.push(Diagnostic::error(format!("{reserved} is reserved for automatic serialization"),Some(cls.span)));
                        }
                    }
                    for param in &cls.primary_params {
                        let field_name = format!("_{}", param.name);
                        let field_ty = self.type_spec_to_type(&param.type_spec);
                        fields.insert(field_name.clone(), (field_ty.clone(), param.is_var));

                        methods.insert(param.name.clone(), (Vec::new(), field_ty));
                    }

                    for case in &cls.cases {
                        let param_types = case
                            .params
                            .iter()
                            .map(|p| self.type_spec_to_type(&p.type_spec))
                            .collect();
                        methods.insert(
                            case.name.clone(),
                            (
                                param_types,
                                Type::Class {
                                    name: cls.name.clone(),
                                },
                            ),
                        );
                    }

                    for method in &cls.methods {
                        let param_types = method
                            .params
                            .iter()
                            .map(|p| self.type_spec_to_type(&p.type_spec))
                            .collect();
                        let return_ty = method
                            .return_type
                            .as_ref()
                            .map_or(Type::Void, |t| self.type_spec_to_type(t));
                        let return_ty = if method.is_async {
                            Type::Async(Box::new(if return_ty == Type::Void {
                                Type::Int
                            } else {
                                return_ty
                            }))
                        } else {
                            return_ty
                        };
                        methods.insert(method.name.clone(), (param_types, return_ty));
                    }

                    let param_types = cls
                        .primary_params
                        .iter()
                        .map(|p| self.type_spec_to_type(&p.type_spec))
                        .collect();

                    self.classes.insert(
                        cls.name.clone(),
                        ClassInfo {
                            name: cls.name.clone(),
                            fields,
                            methods,
                            param_types,
                            min_constructor_args,
                            max_constructor_args,
                        },
                    );
                }
                _ => {}
            }
        }

        let entry_class = program.items.iter().filter_map(|item| {
            let Item::Class(cls) = item else { return None };
            cls.methods.iter().find(|method| method.name.eq_ignore_ascii_case("start"))
                .map(|method| (cls.name.clone(), method.is_static))
        }).min_by_key(|(name, _)| name != "App");
        for item in &program.items {
            if let Item::Class(cls) = item {
                if entry_class.as_ref().is_some_and(|(name, is_static)| name == &cls.name && !is_static) {
                    if let Err(message) = self.check_zero_value(&Type::Class { name: cls.name.clone() }, &mut Vec::new()) {
                        self.diagnostics.push(Diagnostic::error(message, Some(cls.span)));
                    }
                }
                self.check_class(cls);
            }
        }

        self.diagnostics.is_empty()
    }

    fn check_class(&mut self, cls: &ClassDecl) {
        self.current_class = Some(cls.name.clone());
        let mut names = std::collections::HashSet::new();
        for name in cls.primary_params.iter().map(|p| &p.name)
            .chain(cls.methods.iter().map(|m| &m.name))
            .chain(cls.static_fields.iter().map(|f| &f.name)) {
            if !names.insert(name) {
                self.diagnostics.push(Diagnostic::error(format!("Duplicate member '{}'", name), Some(cls.span)));
            }
        }
        self.in_static_context = true;
        for field in &cls.static_fields {
            let expected = self.type_spec_to_type(&field.type_spec);
            if let Some(initializer) = &field.initializer {
                if let Some(actual) = self.check_expr(cls, initializer) {
                    if !self.is_assignable_to(&actual, &expected) {
                        self.diagnostics.push(Diagnostic::error(format!("Static field '{}' expects '{}', found '{}'", field.name, expected, actual), Some(field.span)));
                    }
                }
            } else if let Err(message) = self.check_zero_value(&expected, &mut Vec::new()) {
                self.diagnostics.push(Diagnostic::error(message, Some(field.span)));
            }
        }
        self.in_static_context = false;

        for method in &cls.methods {
            self.check_method(cls, method);
        }

        self.current_class = None;
    }

    fn stmt_definitely_returns(&self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Return(_) | StmtKind::Yield(_) | StmtKind::Throw(_) => true,
            StmtKind::Try { body, catches, finally_body } => {
                finally_body.as_ref().is_some_and(|b| b.iter().any(|s| self.stmt_definitely_returns(s))) ||
                (body.iter().any(|s| self.stmt_definitely_returns(s)) && catches.iter().all(|c| c.body.iter().any(|s| self.stmt_definitely_returns(s))))
            }
            StmtKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                let then_returns = then_branch.iter().any(|s| self.stmt_definitely_returns(s));
                let else_returns = else_branch.as_ref().map_or(false, |eb| {
                    eb.iter().any(|s| self.stmt_definitely_returns(s))
                });
                then_returns && else_returns
            }
            _ => false,
        }
    }

    fn check_method(&mut self, cls: &ClassDecl, method: &MethodDecl) {
        self.in_static_context = method.is_static;
        let mut return_type = method
            .return_type
            .as_ref()
            .map_or(Type::Void, |t| self.type_spec_to_type(t));
        if method.is_async && return_type == Type::Void {
            return_type = Type::Int;
        }
        self.current_return_type = Some(return_type.clone());

        if return_type != Type::Void && !method.is_async {
            let has_return = method
                .body
                .iter()
                .any(|stmt| self.stmt_definitely_returns(stmt));
            if !has_return {
                self.diagnostics.push(Diagnostic::error(
                    format!(
                        "Method '{}' must return a value of type '{:?}'",
                        method.name, return_type
                    ),
                    Some(method.span),
                ));
            }
        }

        self.symbol_table.push_scope();

        for param in &method.params {
            let p_ty = self.type_spec_to_type(&param.type_spec);
            self.symbol_table.insert(Symbol {
                name: param.name.clone(),
                ty: p_ty,
                is_mutable: false,
            });
        }

        for stmt in &method.body {
            self.check_stmt(cls, stmt);
        }

        self.symbol_table.pop_scope();
        self.current_return_type = None;
        self.in_static_context = false;
    }

    fn check_zero_value(&self, ty: &Type, visiting: &mut Vec<String>) -> Result<(), String> {
        match ty {
            Type::Int | Type::Long | Type::Byte | Type::Decimal | Type::Bool | Type::String
            | Type::Nullable(_) => Ok(()),
            Type::Class { name } if self.zero_init_classes.contains(name) => {
                if visiting.contains(name) {
                    return Err(format!("cannot zero-initialize recursive non-nullable class '{}'; make the recursive field nullable", name));
                }
                visiting.push(name.clone());
                if let Some(info) = self.classes.get(name) {
                    for (field_ty, _) in info.fields.values() {
                        self.check_zero_value(field_ty, visiting)?;
                    }
                }
                visiting.pop();
                Ok(())
            }
            _ => Err(format!("type '{}' requires an explicit initializer; it has no implicit zero value", ty)),
        }
    }

    fn check_stmt(&mut self, cls: &ClassDecl, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::LocalDecl {
                name,
                is_mutable,
                type_annotation,
                initializer,
            } => {
                let init_ty = if let Some(init) = initializer {
                    self.check_expr(cls, init)
                } else {
                    None
                };

                let declared_ty = type_annotation
                    .as_ref()
                    .map(|ann| self.type_spec_to_type(ann));
                if initializer.is_none() {
                    if let Some(ty) = &declared_ty {
                        if let Err(message) = self.check_zero_value(ty, &mut Vec::new()) {
                            self.diagnostics.push(Diagnostic::error(message, Some(stmt.span)));
                        }
                    }
                }

                if let (Some(decl_ty), Some(ity)) = (&declared_ty, &init_ty) {
                    if !self.is_assignable_to(ity, decl_ty) {
                        self.diagnostics.push(Diagnostic::error(
                            format!(
                                "Mismatched initializer type: expected '{:?}', found '{:?}'",
                                decl_ty, ity
                            ),
                            Some(stmt.span),
                        ));
                    }
                }

                let var_ty = declared_ty.or(init_ty).unwrap_or(Type::Int);

                self.symbol_table.insert(Symbol {
                    name: name.clone(),
                    ty: var_ty,
                    is_mutable: *is_mutable,
                });
            }
            StmtKind::Expr(expr) => {
                self.check_expr(cls, expr);
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_ty = self.check_expr(cls, condition);
                if let Some(cty) = cond_ty {
                    if !self.is_valid_condition_type(&cty) {
                        self.diagnostics.push(Diagnostic::error(
                            format!(
                                "Condition must be Bool or truthy expression, found '{:?}'",
                                cty
                            ),
                            Some(condition.span),
                        ));
                    }
                }
                self.symbol_table.push_scope();
                for s in then_branch {
                    self.check_stmt(cls, s);
                }
                self.symbol_table.pop_scope();

                if let Some(eb) = else_branch {
                    self.symbol_table.push_scope();
                    for s in eb {
                        self.check_stmt(cls, s);
                    }
                    self.symbol_table.pop_scope();
                }
            }
            StmtKind::For {
                loop_var,
                index_var,
                condition_or_iterable,
                body,
                else_branch,
            } => {
                self.symbol_table.push_scope();
                let element_ty = if loop_var.is_some() && condition_or_iterable.as_ref().is_some_and(|expr| matches!(self.check_expr(cls, expr), Some(Type::Class { name }) if name == "ConsoleArgs")) { Type::String } else { Type::Int };
                if let Some(lvar) = loop_var {
                    self.symbol_table.insert(Symbol {
                        name: lvar.clone(),
                        ty: element_ty,
                        is_mutable: false,
                    });
                }
                if let Some(ivar) = index_var {
                    self.symbol_table.insert(Symbol {
                        name: ivar.clone(),
                        ty: Type::Int,
                        is_mutable: false,
                    });
                }
                if let Some(cond) = condition_or_iterable {
                    let cond_ty = self.check_expr(cls, cond);
                    if loop_var.is_none() {
                        if let Some(cty) = cond_ty {
                            if !self.is_valid_condition_type(&cty) {
                                self.diagnostics.push(Diagnostic::error(
                                    format!("Loop condition must be Bool or truthy expression, found '{:?}'", cty),
                                    Some(cond.span),
                                ));
                            }
                        }
                    }
                }
                self.loop_depth += 1;
                for s in body {
                    self.check_stmt(cls, s);
                }
                self.loop_depth -= 1;
                self.symbol_table.pop_scope();

                if let Some(eb) = else_branch {
                    self.symbol_table.push_scope();
                    for s in eb {
                        self.check_stmt(cls, s);
                    }
                    self.symbol_table.pop_scope();
                }
            }
            StmtKind::Break => {
                if self.loop_depth == 0 {
                    self.diagnostics.push(Diagnostic::error(
                        "'break' can only be used inside a loop".to_string(),
                        Some(stmt.span),
                    ));
                }
            }
            StmtKind::Continue => {
                if self.loop_depth == 0 {
                    self.diagnostics.push(Diagnostic::error(
                        "'continue' can only be used inside a loop".to_string(),
                        Some(stmt.span),
                    ));
                }
            }
            StmtKind::Return(expr_opt) => {
                if self.in_expression_closure {
                    self.diagnostics.push(Diagnostic::error(
                        "Expression closures return their expression; use yield inside an if or for expression",
                        Some(stmt.span),
                    ));
                    return;
                }
                if self.closure_returns.is_some() {
                    let ty = match expr_opt {
                        Some(expr) => self.check_expr(cls, expr),
                        None => Some(Type::Void),
                    };
                    if let Some(ty) = ty {
                        if ty == Type::Void && expr_opt.is_some() {
                            self.diagnostics.push(Diagnostic::error("Use a bare return after a void expression in a block lambda", Some(stmt.span)));
                        }
                        self.closure_returns.as_mut().unwrap().push(ty);
                    }
                    return;
                }
                if let Some(expected_ret) = self.current_return_type.clone() {
                    if expected_ret == Type::Void {
                        if expr_opt.is_some() {
                            self.diagnostics.push(Diagnostic::error(
                                "Cannot return a value from a method returning Void".to_string(),
                                Some(stmt.span),
                            ));
                        }
                    } else {
                        if let Some(e) = expr_opt {
                            let actual_ty = self.check_expr(cls, e);
                            if let Some(act) = actual_ty {
                                if !self.is_assignable_to(&act, &expected_ret)
                                    && (act != Type::Void || matches!(expected_ret, Type::Function { .. })) {
                                    self.diagnostics.push(Diagnostic::error(
                                        format!(
                                            "Mismatched return type: expected '{:?}', found '{:?}'",
                                            expected_ret, act
                                        ),
                                        Some(stmt.span),
                                    ));
                                }
                            }
                        } else {
                            self.diagnostics.push(Diagnostic::error(
                                format!(
                                    "Non-void method must return a value of type '{:?}'",
                                    expected_ret
                                ),
                                Some(stmt.span),
                            ));
                        }
                    }
                }
            }
            StmtKind::Yield(expr) => {
                self.check_expr(cls, expr);
            }
            StmtKind::Try { body, catches, finally_body } => {
                self.symbol_table.push_scope();
                for s in body { self.check_stmt(cls, s); }
                self.symbol_table.pop_scope();
                let mut prior: Vec<String> = Vec::new();
                for handler in catches {
                    let valid = self.satisfies_trait(&handler.type_name, "Error");
                    if !valid {
                        self.diagnostics.push(Diagnostic::error(
                            format!("catch type '{}' must be a class or trait satisfying Error (string message())", handler.type_name), Some(handler.span)));
                    }
                    if prior.iter().any(|earlier| earlier == &handler.type_name || self.satisfies_trait(&handler.type_name, earlier)) {
                        self.diagnostics.push(Diagnostic::error("Unreachable catch: an earlier handler already covers this type", Some(handler.span)));
                    }
                    if handler.condition.is_none() { prior.push(handler.type_name.clone()); }
                    self.symbol_table.push_scope();
                    self.symbol_table.insert(Symbol { name: handler.variable.clone(), ty: Type::Class { name: handler.type_name.clone() }, is_mutable: false });
                    if let Some(condition) = &handler.condition {
                        if self.check_expr(cls, condition) != Some(Type::Bool) {
                            self.diagnostics.push(Diagnostic::error("catch condition must have type bool", Some(condition.span)));
                        }
                    }
                    for s in &handler.body { self.check_stmt(cls, s); }
                    self.symbol_table.pop_scope();
                }
                if let Some(body) = finally_body {
                    self.symbol_table.push_scope();
                    for s in body { self.check_stmt(cls, s); }
                    self.symbol_table.pop_scope();
                }
            }
            StmtKind::Throw(expr) => {
                let throw_ty = self.check_expr(cls, expr);
                if let Some(rty) = throw_ty {
                    let is_valid = match &rty {
                        Type::String => true,
                        Type::Class { name } => {
                            self.satisfies_trait(name, "Error")
                        }
                        _ => false,
                    };
                    if !is_valid {
                        self.diagnostics.push(Diagnostic::error(
                            format!("throw target must be a string or satisfy 'Error', found primitive type '{:?}'", rty),
                            Some(stmt.span),
                        ));
                    }
                }
            }
        }
    }

    fn check_expr(&mut self, cls: &ClassDecl, expr: &Expr) -> Option<Type> {
        match &expr.kind {
            ExprKind::IntLiteral(_) => Some(Type::Int),
            ExprKind::LongLiteral(_) => Some(Type::Long),
            ExprKind::DecimalLiteral(_) => Some(Type::Decimal),
            ExprKind::StringLiteral(_) | ExprKind::TextBlock(_) => Some(Type::String),
            ExprKind::TemplateLiteral(parts) => {
                for part in parts {
                    if self.check_expr(cls, part) == Some(Type::Void) {
                        self.diagnostics.push(Diagnostic::error("Cannot interpolate a void expression", Some(part.span)));
                    }
                }
                Some(Type::String)
            }
            ExprKind::BoolLiteral(_) => Some(Type::Bool),
            ExprKind::NullLiteral => Some(Type::Nullable(Box::new(Type::Void))),
            ExprKind::This => {
                if self.in_static_context {
                    self.diagnostics.push(Diagnostic::error("'this' is unavailable in a static method or initializer", Some(expr.span)));
                    None
                } else { Some(Type::Class { name: cls.name.clone() }) }
            },
            ExprKind::Identifier(name) => {
                if name == "static" { return Some(Type::Class { name: cls.name.clone() }); }
                if self.symbol_table.lookup(name).is_none() && cls.static_fields.iter().any(|f| f.name == *name) {
                    self.diagnostics.push(Diagnostic::error(format!("static field '{}' must be accessed through 'static.'", name), Some(expr.span)));
                    return None;
                }
                let field_name = if name.starts_with('_') {
                    name.clone()
                } else {
                    format!("_{}", name)
                };
                if let Some(info) = self.classes.get(&cls.name) {
                    if info.fields.contains_key(&field_name) || info.fields.contains_key(name) {
                        self.diagnostics.push(Diagnostic::error(
                            format!("instance field '{}' must be accessed through 'this'", name),
                            Some(expr.span),
                        ));
                        return None;
                    }
                }

                if let Some(sym) = self.symbol_table.lookup(name) {
                    Some(sym.ty.clone())
                } else if let Some(class_name) = canonical_builtin_class_name(name) {
                    Some(Type::Class {
                        name: class_name.into(),
                    })
                } else if self.classes.contains_key(name) {
                    Some(Type::Class { name: name.clone() })
                } else {
                    self.diagnostics.push(Diagnostic::error(
                        crate::builtins::builtin_class_case_suggestion(name)
                            .map(|canonical| format!("Built-in classes require capitalization; use {}", canonical))
                            .unwrap_or_else(|| format!("Undeclared identifier '{}'", name)),
                        Some(expr.span),
                    ));
                    None
                }
            }
            ExprKind::MemberAccess { object, member, .. } => {
                let obj_ty = self.check_expr(cls, object)?;
                if let Some(result) = self.check_static_member(cls, object, &obj_ty, member, expr.span, None, None) { return result; }
                match obj_ty {
                    Type::Class { name } => {
                        if let Some(info) = self.classes.get(&name) {
                            let field_name = if member.starts_with('_') {
                                member.clone()
                            } else {
                                format!("_{}", member)
                            };
                            if let Some((field_ty, _)) = info
                                .fields
                                .get(&field_name)
                                .or_else(|| info.fields.get(member))
                            {
                                if member.starts_with('_') && name != cls.name {
                                    self.diagnostics.push(Diagnostic::error(
                                        format!("field '{}' is private to class '{}'", member, name),
                                        Some(expr.span),
                                    ));
                                    return None;
                                }
                                if !member.starts_with('_') {
                                    self.diagnostics.push(Diagnostic::error(
                                        format!("public field '{}' must be read using '{}()'", member, member),
                                        Some(expr.span),
                                    ));
                                    return None;
                                }
                                return Some(field_ty.clone());
                            }
                            if let Some((_, ret_ty)) = info.methods.get(member) {
                                return Some(ret_ty.clone());
                            }
                        }
                        if let Some(tr) = self.traits.get(&name) {
                            if let Some((_, ret_ty)) = tr.methods.get(member) {
                                return Some(ret_ty.clone());
                            }
                        }
                        self.diagnostics.push(Diagnostic::error(
                            format!("Member '{}' not found on target '{}'", member, name),
                            Some(expr.span),
                        ));
                        None
                    }
                    Type::Async(_) if matches!(member.as_str(), "cancel" | "isCompleted" | "isCancelled") => Some(Type::Bool),
                    Type::Async(_) if member == "timeout" => Some(obj_ty.clone()),
                    Type::Channel(_) if matches!(member.as_str(), "send" | "receive" | "close") => {
                        Some(Type::Void)
                    }
                    _ => None,
                }
            }
            ExprKind::Call { callee, args } => {
                if !matches!(callee.kind, ExprKind::MemberAccess { .. }) {
                    if let ExprKind::Identifier(name) = &callee.kind {
                        if self.symbol_table.lookup(name).is_none() {
                            self.diagnostics.push(Diagnostic::error(
                                "Unqualified method calls require 'this.' or 'static.'; only Func values can be called directly",
                                Some(callee.span),
                            ));
                            return None;
                        }
                    }
                    let callee_type = self.check_expr(cls, callee)?;
                    let Type::Function { params, return_type } = callee_type else {
                        self.diagnostics.push(Diagnostic::error(
                            format!("Cannot call '{}'; expected a Func value", callee_type),
                            Some(callee.span),
                        ));
                        return None;
                    };
                    if params.len() != args.len() {
                        self.diagnostics.push(Diagnostic::error(
                            format!("Func expects {} arguments, found {}", params.len(), args.len()),
                            Some(expr.span),
                        ));
                    }
                    for (index, arg) in args.iter().enumerate() {
                        if let Some(actual) = self.check_expr(cls, arg) {
                            if let Some(expected) = params.get(index) {
                                if actual == Type::Void || !self.is_assignable_to(&actual, expected) {
                                    self.diagnostics.push(Diagnostic::error(
                                        format!("Func argument {} expects '{}', found '{}'", index + 1, expected, actual),
                                        Some(arg.span),
                                    ));
                                }
                            }
                        }
                    }
                    return Some(*return_type);
                }
                if matches!(&callee.kind, ExprKind::MemberAccess { object, member, .. } if member == "mock" && matches!(&object.kind, ExprKind::Identifier(name) if name == "Mockery" && self.symbol_table.lookup(name).is_none())) {
                    if let [Expr { kind: ExprKind::Identifier(contract), .. }] = args.as_slice() {
                        let name = crate::mockery::class_name(contract);
                        if self.symbol_table.lookup(contract).is_none() && self.traits.contains_key(contract) && self.classes.contains_key(&name) {
                            return Some(Type::Class { name });
                        }
                    }
                    self.diagnostics.push(Diagnostic::error("Mockery.mock requires a declared synchronous trait with primitive or implicit-void returns (mockeryId and _mockeryId are reserved)", Some(expr.span)));
                    return None;
                }
                let mut arg_types = Vec::new();
                for arg in args {
                    if let Some(t) = self.check_expr(cls, arg) {
                        arg_types.push(t);
                    }
                }
                match &callee.kind {
                    ExprKind::MemberAccess { object, member, .. } => {
                        if let Some(obj_ty) = self.check_expr(cls, object) {
                            if let Some(result) = self.check_static_member(cls, object, &obj_ty, member, expr.span, Some(&arg_types), None) { return result; }
                            if member == "stacktrace" && matches!(&obj_ty, Type::Class { name } if name == "Error" || self.satisfies_trait(name, "Error")) {
                                let class_receiver = matches!(&object.kind, ExprKind::Identifier(name) if self.symbol_table.lookup(name).is_none() && (self.classes.contains_key(name) || self.traits.contains_key(name)));
                                if class_receiver || !args.is_empty() {
                                    self.diagnostics.push(Diagnostic::error("stacktrace requires an error instance call with no arguments", Some(expr.span)));
                                }
                                return Some(Type::String);
                            }
                            if member == "toXml" {
                                let class_receiver=matches!(&object.kind,ExprKind::Identifier(name) if self.symbol_table.lookup(name).is_none()&&(self.classes.contains_key(name)||self.traits.contains_key(name)));
                                let document=matches!(&obj_ty,Type::Class{name} if name=="XmlDocument"||name=="XmlElement");
                                if class_receiver||obj_ty==Type::Void||args.len()>1||(document&&!args.is_empty())||(!args.is_empty()&&arg_types!=vec![Type::Class{name:"XmlOptions".into()}]) {
                                    self.diagnostics.push(Diagnostic::error("toXml requires an instance and optionally one XmlOptions value; XML document/element handles accept no options",Some(expr.span)));
                                }
                                if matches!(obj_ty,Type::AnonymousObject{..}|Type::Function{..}) {
                                    self.diagnostics.push(Diagnostic::error("XML requires a declared data class; anonymous/function layouts are unsupported",Some(expr.span)));
                                }
                                return Some(Type::String);
                            }
                            if member == "toJson" {
                                if matches!(obj_ty, Type::AnonymousObject { .. } | Type::Function { .. }) {
                                    self.diagnostics.push(Diagnostic::error("JSON requires a declared data class or parsed JSON document; anonymous/function layouts are unsupported", Some(expr.span)));
                                }
                                let class_receiver = matches!(&object.kind, ExprKind::Identifier(name) if self.symbol_table.lookup(name).is_none() && self.classes.contains_key(name));
                                if class_receiver || !args.is_empty() || obj_ty == Type::Void {
                                    self.diagnostics.push(Diagnostic::error("toJson requires an instance call with no arguments", Some(expr.span)));
                                }
                                return Some(Type::String);
                            }
                            if let Some(result) = self.check_orm_call(&obj_ty, object, member, args, &arg_types, expr.span) { return result; }
                            if let Type::Class { name } = &obj_ty {
                                if (name == "List" || name == "Map") && member == "len" && args.is_empty() {
                                    return Some(Type::Int);
                                }
                                let class_receiver = matches!(&object.kind, ExprKind::Identifier(n) if self.symbol_table.lookup(n).is_none() && canonical_builtin_class_name(n).is_some());
                                if !class_receiver && args.is_empty() {
                                    if name == "Distance" && ["meters", "kilometers", "miles"].contains(&member.as_str()) {
                                        return Some(Type::Decimal);
                                    }

                                }
                                if name == "Server" && ["enableHttp3", "http3Port"].contains(&member.as_str()) {
                                    let (params, result) = self.classes[name].methods[member].clone();
                                    if class_receiver || args.len() != params.len() {
                                        self.diagnostics.push(Diagnostic::error(format!("Server.{} requires an instance and {} arguments", member, params.len()), Some(expr.span)));
                                        return None;
                                    }
                                    if !arg_types.iter().zip(&params).all(|(actual, expected)| self.is_assignable_to(actual, expected)) {
                                        self.diagnostics.push(Diagnostic::error("HTTP/3 port must be an int", Some(expr.span)));
                                    }
                                    return Some(result);
                                }
                                if crate::builtins::SQL_CLASSES.contains(&name.as_str()) {
                                    let Some(method) = crate::builtins::sql_call_method(name, member, &arg_types) else {
                                        self.diagnostics.push(Diagnostic::error(format!("Unknown {} method '{}'", name, member), Some(expr.span)));
                                        return None;
                                    };
                                    if class_receiver != method.is_static {
                                        self.diagnostics.push(Diagnostic::error("SQL factory methods require the class; other methods require an instance", Some(expr.span)));
                                        return None;
                                    }
                                    if args.len() != method.params.len() || arg_types.len() != method.params.len() {
                                        self.diagnostics.push(Diagnostic::error(format!("{}.{} expects {} arguments", name, member, method.params.len()), Some(expr.span)));
                                        return None;
                                    }
                                    if !arg_types.iter().zip(&method.params).all(|(actual, expected)| self.is_assignable_to(actual, expected)) {
                                        self.diagnostics.push(Diagnostic::error("Invalid SQL method argument types; handles and strings must be non-null", Some(expr.span)));
                                    }
                                    return Some(method.result);
                                }
                                if crate::rest_in_peace::CLASSES.contains(&name.as_str()) {
                                    let Some(method) = crate::rest_in_peace::methods(name).into_iter().find(|m| m.name == member) else {
                                        self.diagnostics.push(Diagnostic::error("Unknown RestInPeace method", Some(expr.span))); return None;
                                    };
                                    if class_receiver != method.is_static || args.len() != method.params.len() || arg_types.len() != method.params.len()
                                        || !arg_types.iter().zip(&method.params).all(|(actual, expected)| actual == expected) {
                                        self.diagnostics.push(Diagnostic::error("Invalid RestInPeace call: use class factories and instance methods with matching non-null arguments", Some(expr.span)));
                                    }
                                    return Some(method.result);
                                }
                                if crate::builtins::AUTH_CLASSES.contains(&name.as_str()) {
                                    let Some(method) = crate::builtins::auth_methods(name).into_iter().find(|m| m.name == member) else {
                                        self.diagnostics.push(Diagnostic::error(format!("Unknown {} method '{}'", name, member), Some(expr.span)));
                                        return None;
                                    };
                                    if class_receiver != method.is_static {
                                        self.diagnostics.push(Diagnostic::error("Authentication factory methods require the class; other methods require an instance", Some(expr.span)));
                                        return None;
                                    }
                                    if args.len() != method.params.len() || arg_types.len() != method.params.len() {
                                        self.diagnostics.push(Diagnostic::error(format!("{}.{} expects {} arguments", name, member, method.params.len()), Some(expr.span)));
                                        return None;
                                    }
                                    if !arg_types.iter().zip(&method.params).all(|(actual, expected)| self.is_assignable_to(actual, expected)) {
                                        self.diagnostics.push(Diagnostic::error("Invalid authentication method argument types; strings must be non-null", Some(expr.span)));
                                    }
                                    return Some(method.result);
                                }
                                if name == "Async" && member == "start" {
                                    if !class_receiver {
                                        self.diagnostics.push(Diagnostic::error("Async.start requires a class call", Some(expr.span)));
                                    }
                                    if args.len() != 1 {
                                        self.diagnostics.push(Diagnostic::error("Async.start expects one zero-argument Func", Some(expr.span)));
                                        return None;
                                    }
                                    let Some(Type::Function { params, return_type }) = arg_types.first() else {
                                        self.diagnostics.push(Diagnostic::error("Async.start expects a zero-argument Func", Some(args[0].span)));
                                        return None;
                                    };
                                    if !params.is_empty() {
                                        self.diagnostics.push(Diagnostic::error("Async.start expects a zero-argument Func", Some(args[0].span)));
                                    }
                                    return Some(Type::Async(return_type.clone()));
                                }
                                if name == "Async" && member == "startAll" {
                                    if !class_receiver {
                                        self.diagnostics.push(Diagnostic::error("Async.startAll requires a class call", Some(expr.span)));
                                    }
                                    let valid = matches!(args.as_slice(), [Expr { kind: ExprKind::ListLiteral(operations), .. }]
                                        if operations.iter().all(|operation| matches!(self.check_expr(cls, operation), Some(Type::Function { params, .. }) if params.is_empty())));
                                    if !valid {
                                        self.diagnostics.push(Diagnostic::error("Async.startAll expects one list literal containing zero-argument Func values", Some(expr.span)));
                                    }
                                    return Some(Type::Async(Box::new(Type::Class { name: "List".into() })));
                                }
                                if name == "Async" && member == "completed" {
                                    if !class_receiver || args.len() != 1 || matches!(arg_types.first(), None | Some(Type::Void)) {
                                        self.diagnostics.push(Diagnostic::error("Async.completed expects one value", Some(expr.span)));
                                        return None;
                                    }
                                    return Some(Type::Async(Box::new(arg_types[0].clone())));
                                }
                                if name == "Async" && member == "failed" {
                                    let valid = class_receiver && args.len() == 1 && matches!(arg_types.first(), Some(Type::Class { name }) if name == "Error" || self.satisfies_trait(name, "Error"));
                                    if !valid {
                                        self.diagnostics.push(Diagnostic::error("Async.failed expects one Error", Some(expr.span)));
                                    }
                                    return Some(Type::Async(Box::new(Type::Void)));
                                }
                                if name == "Async" && member == "delay" {
                                    let valid = class_receiver && args.len() == 1 && matches!(arg_types.first(), Some(Type::Int | Type::Long));
                                    if !valid {
                                        self.diagnostics.push(Diagnostic::error("Async.delay expects one Int argument: milliseconds", Some(expr.span)));
                                    }
                                    return Some(Type::Async(Box::new(Type::Void)));
                                }
                                if name == "Async" && ["all", "any", "race", "allSettled"].contains(&member.as_str()) {
                                    if !class_receiver { self.diagnostics.push(Diagnostic::error("Async combinators require a class call", Some(expr.span))); }
                                    if let Some(list) = args.first() {
                                        if let ExprKind::ListLiteral(elements) = &list.kind {
                                            for element in elements {
                                                if !matches!(self.check_expr(cls, element), Some(Type::Async(_))) {
                                                    self.diagnostics.push(Diagnostic::error("Async combinators accept only task elements", Some(element.span)));
                                                }
                                            }
                                        }
                                    }
                                }
                                if ["TaskOutcome", "TaskOutcomes"].contains(&name.as_str()) {
                                    let method = crate::builtins::task_outcome_methods(name).into_iter().find(|m| m.name == member);
                                    if class_receiver || method.as_ref().is_none_or(|m| m.params != arg_types || args.len() != m.params.len()) {
                                        self.diagnostics.push(Diagnostic::error("Invalid task outcome method call", Some(expr.span)));
                                    }
                                    return method.map(|m| m.result);
                                }
                                if crate::builtins::XML_CLASSES.contains(&name.as_str()) {
                                    if name=="XmlElement"&&member=="text"&&args.is_empty() {
                                        if class_receiver {self.diagnostics.push(Diagnostic::error("XML text requires an instance",Some(expr.span)));}
                                        return Some(Type::String);
                                    }
                                    let method=crate::builtins::xml_methods(name).into_iter().find(|m|m.name==member);
                                    let valid=method.as_ref().is_some_and(|method|method.is_static==class_receiver&&method.params==arg_types&&method.params.len()==args.len());
                                    let target_valid= !["partialParse","to"].contains(&member.as_str())||name!="Xml"||matches!(args.get(1).map(|arg|&arg.kind),Some(ExprKind::Identifier(target)) if target=="XmlDocument"&&self.symbol_table.lookup(target).is_none());
                                    if !valid||!target_valid {self.diagnostics.push(Diagnostic::error("Invalid XML method, receiver, or arguments; Xml.toObject(string) returns a document, not a reconstructed application class",Some(expr.span)));}
                                    return method.map(|method|method.result);
                                }
                                if name == "Json" {
                                    let valid = match member.as_str() {
                                        "toObject" => args.len()==1 && arg_types==vec![Type::String],
                                        "partialParse" | "to" => args.len()==2 && arg_types.first()==Some(&Type::String)
                                            && matches!(&args[1].kind, ExprKind::Identifier(name) if name=="JsonObject" && self.symbol_table.lookup(name).is_none()),
                                        _ => false,
                                    };
                                    if !class_receiver || !valid {
                                        self.diagnostics.push(Diagnostic::error("Use Json.toObject(string); typed reconstruction is not implemented (partialParse/to currently accept only JsonObject)", Some(expr.span)));
                                    }
                                    return Some(Type::Class { name:"JsonObject".into() });
                                }
                                if name=="JsonObject" || name=="JsonArray" {
                                    let method=self.classes[name].methods.get(member).cloned();
                                    if class_receiver || method.as_ref().is_none_or(|(params,_)| params!=&arg_types || params.len()!=args.len()) {
                                        self.diagnostics.push(Diagnostic::error("Invalid JSON document method arguments; an instance is required", Some(expr.span)));
                                    }
                                    return method.map(|(_,result)|result);
                                }
                                if name == "KeyValueStore" {
                                    let Some(method) = crate::builtins::key_value_store_methods().into_iter().find(|m| m.name == member) else {
                                        self.diagnostics.push(Diagnostic::error(format!("Unknown KeyValueStore method '{member}'"), Some(expr.span))); return None;
                                    };
                                    if class_receiver != method.is_static || args.len()!=method.params.len() || arg_types.len()!=method.params.len()
                                        || !arg_types.iter().zip(&method.params).all(|(actual, expected)| self.is_assignable_to(actual, expected)) {
                                        self.diagnostics.push(Diagnostic::error(format!("KeyValueStore.{member} requires {} with the declared arguments", if method.is_static { "a static call" } else { "an instance call" }), Some(expr.span)));
                                    }
                                    return Some(method.result);
                                }
                                if name == "Cache" {
                                    let Some(method) = crate::builtins::cache_methods().into_iter().find(|m| m.name == member) else {
                                        self.diagnostics.push(Diagnostic::error(format!("Unknown Cache method '{member}'"), Some(expr.span)));
                                        return None;
                                    };
                                    if class_receiver != method.is_static || args.len() != method.params.len() || arg_types.len() != method.params.len()
                                        || !arg_types.iter().zip(&method.params).all(|(actual, expected)| self.is_assignable_to(actual, expected)) {
                                        self.diagnostics.push(Diagnostic::error(format!("Cache.{member} requires {} with the declared argument types", if method.is_static { "a static call" } else { "an instance call" }), Some(expr.span)));
                                    }
                                    return Some(method.result);
                                }
                                if crate::builtins::OBSERVABILITY_CLASSES.contains(&name.as_str()) {
                                    let Some(method) = crate::builtins::observability_methods(name).into_iter().find(|m| m.name == member) else {
                                        self.diagnostics.push(Diagnostic::error(format!("Unknown {name} method '{member}'"), Some(expr.span)));
                                        return None;
                                    };
                                    if !class_receiver || args.len() != method.params.len() || arg_types.len() != method.params.len()
                                        || !arg_types.iter().zip(&method.params).all(|(actual, expected)| self.is_assignable_to(actual, expected)) {
                                        self.diagnostics.push(Diagnostic::error(format!("{name}.{member} requires a static call with the declared argument types"), Some(expr.span)));
                                    }
                                    return Some(method.result);
                                }
                                if name == "Env" {
                                    let Some(method) = crate::builtins::env_methods().into_iter().find(|m| m.name == member) else {
                                        self.diagnostics.push(Diagnostic::error(format!("Unknown Env method '{}'", member), Some(expr.span)));
                                        return None;
                                    };
                                    if !class_receiver || args.len() != 1 || arg_types != vec![Type::String] {
                                        self.diagnostics.push(Diagnostic::error("Env.get, Env.require, and Env.has require a static call with one non-null string", Some(expr.span)));
                                    }
                                    return Some(method.result);
                                }
                                if name == "Mockery" {
                                    return self.check_mockery_call(cls, member, args, &arg_types, class_receiver, expr.span);
                                }
                                if name == "Testicle" {
                                    let valid = match member.as_str() {
                                        "isTrue" | "isFalse" => arg_types == vec![Type::Bool],
                                        "fail" => arg_types == vec![Type::String],
                                        "equal" | "notEqual" => arg_types.len() == 2 && arg_types[0] == arg_types[1]
                                            && matches!(arg_types[0], Type::Int | Type::Long | Type::Byte | Type::Decimal | Type::Bool | Type::String),
                                        _ => false,
                                    };
                                    if !class_receiver || !valid || arg_types.len() != args.len() {
                                        self.diagnostics.push(Diagnostic::error("Testicle requires a static call: isTrue/isFalse(bool), equal/notEqual(two values of the same primitive type), or fail(string)", Some(expr.span)));
                                    }
                                    return Some(Type::Void);
                                }
                                if name == "Gc" {
                                    if !class_receiver || !args.is_empty() { self.diagnostics.push(Diagnostic::error("Gc methods require a class call with no arguments", Some(expr.span))); }
                                    return self.classes["Gc"].methods.get(member).map(|(_, result)| result.clone()).or_else(|| { self.diagnostics.push(Diagnostic::error("Unknown Gc method", Some(expr.span))); None });
                                }
                                if name == "Money" {
                                    let static_method = matches!(member.as_str(), "php" | "usd" | "eur" | "jpy" | "gbp" | "parse");
                                    let method = self.classes["Money"].methods.get(member).cloned();
                                    if let Some((params, result)) = method {
                                        if class_receiver != static_method || args.len() != params.len() || arg_types.len() != params.len() {
                                            self.diagnostics.push(Diagnostic::error(format!("Money.{} requires {} with {} arguments", member, if static_method { "a class call" } else { "an instance" }, params.len()), Some(expr.span)));
                                        } else if !arg_types.iter().zip(&params).all(|(actual, expected)| self.is_assignable_to(actual, expected)) {
                                            self.diagnostics.push(Diagnostic::error("Money arguments must match their non-null parameter types", Some(expr.span)));
                                        }
                                        return Some(result);
                                    }
                                }
                                if name == "Session" || (name == "Request" && matches!(member.as_str(), "session" | "body")) {
                                    if class_receiver {
                                        self.diagnostics.push(Diagnostic::error(format!("{}.{} requires an instance", name, member), Some(expr.span)));
                                        return None;
                                    }
                                    let Some((params, result)) = self.classes[name].methods.get(member).cloned() else {
                                        self.diagnostics.push(Diagnostic::error(format!("Unknown Session method '{}'", member), Some(expr.span)));
                                        return None;
                                    };
                                    if args.len() != params.len() || arg_types.len() != params.len() {
                                        self.diagnostics.push(Diagnostic::error(format!("{}.{} expects {} arguments", name, member, params.len()), Some(expr.span)));
                                    } else if !arg_types.iter().zip(&params).all(|(actual, expected)| actual == expected) {
                                        self.diagnostics.push(Diagnostic::error("Session keys and values must be non-null strings", Some(expr.span)));
                                    }
                                    return Some(result);
                                }
                                if name == "ConsoleArgs" {
                                    let method = crate::builtins::console_args_methods().into_iter().find(|m| m.name == member);
                                    if class_receiver || method.as_ref().is_none_or(|m| m.params != arg_types || args.len() != m.params.len()) {
                                        self.diagnostics.push(Diagnostic::error("ConsoleArgs supports instance calls len() and get(int)", Some(expr.span)));
                                    }
                                    return method.map(|m| m.result);
                                }
                                if name == "Log" || name == "Console" {
                                    let static_call = matches!(&object.kind, ExprKind::Identifier(n) if self.symbol_table.lookup(n).is_none() && canonical_builtin_class_name(n).is_some());
                                    if name == "Console" && member == "args" {
                                        if !static_call || !args.is_empty() {
                                            self.diagnostics.push(Diagnostic::error("Console.args requires a class call with no arguments", Some(expr.span)));
                                        }
                                        return Some(Type::Class { name: "ConsoleArgs".into() });
                                    }
                                    if name == "Console" && ["read", "readLine"].contains(&member.as_str()) {
                                        if !static_call || !args.is_empty() {
                                            self.diagnostics.push(Diagnostic::error(format!("Console.{} requires a class call with no arguments", member), Some(expr.span)));
                                            return None;
                                        }
                                        return Some(Type::Nullable(Box::new(Type::String)));
                                    }
                                    if name == "Log" && ["put", "get", "remove", "clear"].contains(&member.as_str()) {
                                        let (params, result) = self.classes["Log"].methods[member].clone();
                                        if !static_call || args.len() != params.len() {
                                            self.diagnostics.push(Diagnostic::error(format!("Log.{} requires a class call with {} arguments", member, params.len()), Some(expr.span)));
                                            return None;
                                        }
                                        if !arg_types.iter().zip(&params).all(|(actual, expected)| actual == expected) {
                                            self.diagnostics.push(Diagnostic::error("Log context keys and values must be non-null strings", Some(expr.span)));
                                        }
                                        return Some(result);
                                    }
                                    let known = if name == "Log" { ["info", "warn", "debug", "error", "setLevel"].contains(&member.as_str()) } else { ["print", "printLine"].contains(&member.as_str()) };
                                    if !static_call || !known || args.len() != 1 {
                                        self.diagnostics.push(Diagnostic::error(format!("{}.{} requires a class call with exactly one value", name, member), Some(expr.span)));
                                        return None;
                                    }
                                    if let Some(ty) = arg_types.first() {
                                        if (member == "setLevel" && *ty != Type::String) || (member != "setLevel" && !self.is_output_type(ty)) {
                                            self.diagnostics.push(Diagnostic::error("Output requires a scalar, nullable scalar, string, or Error value; setLevel requires a string", Some(expr.span)));
                                        }
                                    }
                                    return Some(Type::Void);
                                }
                                if crate::builtins::SQLITE_CLASSES.contains(&name.as_str()) {
                                    if let Some(method) = crate::builtins::sqlite_methods(name).into_iter().find(|m| m.name == member) {
                                        let static_call = matches!(&object.kind, ExprKind::Identifier(n) if self.symbol_table.lookup(n).is_none() && canonical_builtin_class_name(n).is_some());
                                        if static_call != method.is_static {
                                            self.diagnostics.push(Diagnostic::error("SQLite factory methods require the class; other SQLite methods require an instance", Some(expr.span)));
                                            return None;
                                        }
                                        if args.len() != method.params.len() {
                                            self.diagnostics.push(Diagnostic::error(format!("{}.{} expects {} arguments, got {}", name, member, method.params.len(), args.len()), Some(expr.span)));
                                            return None;
                                        }
                                    }
                                }
                            }
                            if matches!(&obj_ty, Type::Class { name } if name == "List" || name == "Map") && arg_types.iter().any(|ty| ty.nullable_scalar_inner().is_some() || matches!(ty, Type::Async(inner) if inner.nullable_scalar_inner().is_some())) {
                                self.diagnostics.push(Diagnostic::error("Nullable scalar values and tasks with nullable scalar results are not supported in untyped collection storage yet", Some(expr.span)));
                            }
                            if matches!(obj_ty, Type::Async(_)) && matches!(member.as_str(), "cancel" | "isCompleted" | "isCancelled") {
                                if !args.is_empty() {
                                    self.diagnostics.push(Diagnostic::error(format!("Async.{member} expects no arguments"), Some(expr.span)));
                                }
                                return Some(Type::Bool);
                            }
                            if matches!(obj_ty, Type::Async(_)) && member == "timeout" {
                                let valid = args.len() == 1
                                    && arg_types.len() == 1
                                    && matches!(arg_types[0], Type::Int | Type::Long);
                                if !valid {
                                    self.diagnostics.push(Diagnostic::error(
                                        "Async.timeout expects one Int argument: the timeout in milliseconds",
                                        Some(expr.span),
                                    ));
                                }
                                return Some(obj_ty.clone());
                            }
                            if let Type::Channel(element) = &obj_ty {
                                return match member.as_str() {
                                    "send" => {
                                        if let Some(actual) = arg_types.first() {
                                            if !self.is_assignable_to(actual, element) {
                                                self.diagnostics.push(Diagnostic::error(
                                                    format!("Channel expects '{}', but send received '{}'", element, actual),
                                                    Some(expr.span),
                                                ));
                                            }
                                        } else {
                                            self.diagnostics.push(Diagnostic::error(
                                                "Channel.send requires one value",
                                                Some(expr.span),
                                            ));
                                        }
                                        Some(Type::Async(Box::new(Type::Bool)))
                                    }
                                    "receive" if args.is_empty() => {
                                        Some(Type::Async(element.clone()))
                                    }
                                    "close" if args.is_empty() => Some(Type::Void),
                                    _ => {
                                        self.diagnostics.push(Diagnostic::error(
                                            format!("Invalid Channel method call '{}'", member),
                                            Some(expr.span),
                                        ));
                                        None
                                    }
                                };
                            }
                            if let Type::Class { ref name } = obj_ty {
                                if name == "Async" && member == "async" {
                                    self.diagnostics.push(Diagnostic::error(
                                        "Async.async(...) was removed; start a task with `async expr` instead",
                                        Some(expr.span),
                                    ));
                                    return None;
                                }
                                if let Some(info) = self.traits.get(name).cloned() {
                                    if let Some((params, result)) = info.methods.get(member) {
                                        if params.len() != args.len() || !arg_types.iter().zip(params).all(|(a,p)| self.is_assignable_to(a,p)) {
                                            self.diagnostics.push(Diagnostic::error("Invalid trait method arguments", Some(expr.span)));
                                        }
                                        return Some(result.clone());
                                    }
                                    self.diagnostics.push(Diagnostic::error(format!("Member '{}' not found on trait '{}'", member, name), Some(expr.span)));
                                }
                                if let Some(info) = self.classes.get(name).cloned() {
                                    if let Some((field_ty, is_mutable)) = info.fields.get(&format!("_{}", member)) {
                                        if matches!(&object.kind, ExprKind::Identifier(identifier)
                                            if self.symbol_table.lookup(identifier).is_none()) {
                                            self.diagnostics.push(Diagnostic::error(
                                                format!("field accessor '{}' requires an instance of '{}'", member, name), Some(expr.span)));
                                            return None;
                                        }
                                        if !args.is_empty() {
                                            if !is_mutable {
                                                self.diagnostics.push(Diagnostic::error(
                                                    format!("cannot set immutable field '{}'", member), Some(expr.span)));
                                            } else if args.len() != 1 {
                                                self.diagnostics.push(Diagnostic::error(
                                                    format!("setter '{}(value)' requires exactly one argument", member), Some(expr.span)));
                                            } else if let Some(actual) = arg_types.first() {
                                                if !self.is_assignable_to(actual, field_ty) {
                                                    self.diagnostics.push(Diagnostic::error(
                                                        format!("setter '{}' expects '{}', found '{}'", member, field_ty, actual), Some(expr.span)));
                                                }
                                            }
                                            return Some(Type::Void);
                                        }
                                    }
                                    if let Some((param_types, ret_ty)) =
                                        info.methods.get(member).cloned()
                                    {
                                        for (i, (arg_ty, param_ty)) in
                                            arg_types.iter().zip(param_types.iter()).enumerate()
                                        {
                                            if !self.is_assignable_to(arg_ty, param_ty) {
                                                self.diagnostics.push(Diagnostic::error(
                                                    format!("Expected '{:?}' for argument {} of method '{}', but got '{:?}'", param_ty, i + 1, member, arg_ty),
                                                    Some(expr.span),
                                                ));
                                            }
                                        }
                                        return Some(ret_ty);
                                    }
                                    self.diagnostics.push(Diagnostic::error(format!("Member '{}' not found on class '{}'", member, name), Some(expr.span)));
                                }
                            }
                        }
                    }
                    _ => {}
                }
                Some(Type::Void)
            }
            ExprKind::NullCoalescingAssign { target, value } => {
                let read = self.check_expr(cls, target)?;
                let assignment = Expr { kind: ExprKind::Assign { target: target.clone(), value: value.clone() }, span: expr.span };
                self.check_expr(cls, &assignment);
                if !matches!(read, Type::Nullable(_)) {
                    self.diagnostics.push(Diagnostic::error("??= requires a nullable target", Some(target.span)));
                }
                Some(read)
            }
            ExprKind::Assign { target, value } => {
                let val_ty = self.check_expr(cls, value)?;
                match &target.kind {
                    ExprKind::MemberAccess { object, member, .. } => {
                        let obj_ty = self.check_expr(cls, object);
                        if let Some(ty) = &obj_ty {
                            if let Some(result) = self.check_static_member(cls, object, ty, member, target.span, None, Some(&val_ty)) { return result; }
                        }
                        let target_cls = match obj_ty {
                            Some(Type::Class { name }) => name,
                            _ => cls.name.clone(),
                        };
                        if let Some(info) = self.classes.get(&target_cls) {
                            let field_info = info
                                .fields
                                .get(member)
                                .or_else(|| info.fields.get(&format!("_{}", member)));
                            if let Some((field_ty, is_mutable)) = field_info {
                                if member.starts_with('_') && target_cls != cls.name {
                                    self.diagnostics.push(Diagnostic::error(
                                        format!("field '{}' is private to class '{}'", member, target_cls),
                                        Some(target.span),
                                    ));
                                    return None;
                                }
                                if !member.starts_with('_') {
                                    self.diagnostics.push(Diagnostic::error(
                                        format!("public field '{}' must be written using '{}(value)'", member, member),
                                        Some(target.span),
                                    ));
                                    return None;
                                }
                                if !is_mutable {
                                    self.diagnostics.push(Diagnostic::error(
                                        format!("cannot reassign immutable field '{}'", member),
                                        Some(target.span),
                                    ));
                                }
                                if !self.is_assignable_to(&val_ty, field_ty) {
                                    self.diagnostics.push(Diagnostic::error(
                                        format!(
                                            "Cannot assign '{:?}' to field '{}' of type '{:?}'",
                                            val_ty, member, field_ty
                                        ),
                                        Some(target.span),
                                    ));
                                }
                            }
                        }
                    }
                    ExprKind::Identifier(name) => {
                        if self.symbol_table.lookup(name).is_none() {
                            self.check_expr(cls, target);
                        }
                        if let Some(sym) = self.symbol_table.lookup(name) {
                            if !sym.is_mutable {
                                self.diagnostics.push(Diagnostic::error(
                                    format!("cannot reassign immutable variable '{}'", name),
                                    Some(target.span),
                                ));
                            }
                            if !self.is_assignable_to(&val_ty, &sym.ty) {
                                self.diagnostics.push(Diagnostic::error(
                                    format!(
                                        "Cannot assign '{:?}' to variable '{}' of type '{:?}'",
                                        val_ty, name, sym.ty
                                    ),
                                    Some(target.span),
                                ));
                            }
                        }
                    }
                    _ => {
                        self.diagnostics.push(Diagnostic::error(
                            "left side of assignment is not assignable".to_string(),
                            Some(target.span),
                        ));
                    }
                }
                Some(val_ty)
            }
            ExprKind::ClassInstantiation {
                class_name,
                generic_type,
                args,
            } => {
                if cls.methods.iter().any(|m| m.is_static && m.name == *class_name) {
                    self.diagnostics.push(Diagnostic::error(format!("static method '{}' must be accessed through 'static.'", class_name), Some(expr.span)));
                    return None;
                }
                let class_name = canonical_builtin_class_name(class_name).unwrap_or(class_name);
                if class_name == "Mockery" || class_name.starts_with(crate::mockery::PREFIX) {
                    self.diagnostics.push(Diagnostic::error("Use Mockery.mock(TraitName) to create a mock", Some(expr.span)));
                    return None;
                }
                if class_name == "Testicle" {
                    self.diagnostics.push(Diagnostic::error("Testicle does not create instances; use static assertion methods", Some(expr.span)));
                    return None;
                }
                if ["Json", "JsonObject", "JsonArray"].contains(&class_name) {
                    self.diagnostics.push(Diagnostic::error("Use Json.toObject to create a JSON document", Some(expr.span))); return None;
                }
                if class_name == "KeyValueStore" {
                    self.diagnostics.push(Diagnostic::error("Use KeyValueStore.open or openBuffered", Some(expr.span))); return None;
                }
                if class_name == "Cache" {
                    self.diagnostics.push(Diagnostic::error("Use Cache.memory, Cache.open, Cache.openBuffered, or Cache.connect; Cache does not create instances directly", Some(expr.span)));
                    return None;
                }
                if crate::builtins::OBSERVABILITY_CLASSES.contains(&class_name) {
                    self.diagnostics.push(Diagnostic::error(format!("{class_name} does not create instances; use its static methods"), Some(expr.span)));
                    return None;
                }
                if ["Env", "TaskOutcome", "TaskOutcomes", "ConsoleArgs"].contains(&class_name) {
                    self.diagnostics.push(Diagnostic::error(format!("{class_name} does not create instances; use its task or environment API"), Some(expr.span)));
                    return None;
                }
                if ["Xml","XmlDocument","XmlElement"].contains(&class_name) {
                    self.diagnostics.push(Diagnostic::error("Use Xml.toObject(text) or Xml.create(root); XML handles cannot be constructed",Some(expr.span)));return None;
                }
                if class_name == "Gc" {
                    self.diagnostics.push(Diagnostic::error("Use Gc.collect(), Gc.objects(), or Gc.reclaimed(); Gc does not create instances", Some(expr.span)));
                    return None;
                }
                if class_name == "Log" || class_name == "Console" {
                    self.diagnostics.push(Diagnostic::error("Use log(value), Log.info(value), or Console.printLine(value); these built-ins do not create instances", Some(expr.span)));
                    return None;
                }
                if ["Orm", "OrmModel", "OrmQuery", "OrmProjection", "OrmRows"].contains(&class_name) {
                    self.diagnostics.push(Diagnostic::error("Use Orm.model(db, ModelClass, table); ORM handles cannot be constructed", Some(expr.span)));
                    return None;
                }
                if crate::builtins::SQL_CLASSES.contains(&class_name) {
                    self.diagnostics.push(Diagnostic::error("Use SQL.open(url), SQLParams.create(), or connection methods; SQL handles cannot be constructed", Some(expr.span)));
                    return None;
                }
                if crate::rest_in_peace::CLASSES.contains(&class_name) {
                    self.diagnostics.push(Diagnostic::error("Use RestInPeace.given() or RestInPeace.at(baseUrl); REST handles cannot be constructed", Some(expr.span)));
                    return None;
                }
                if crate::builtins::AUTH_CLASSES.contains(&class_name) {
                    self.diagnostics.push(Diagnostic::error("Use Jwt static methods or OAuth2.create(); authentication handles cannot be constructed", Some(expr.span)));
                    return None;
                }
                if class_name == "Session" {
                    self.diagnostics.push(Diagnostic::error("Use request.session() to access a Session; Session cannot be constructed", Some(expr.span)));
                    return None;
                }
                if crate::builtins::SQLITE_CLASSES.contains(&class_name) {
                    self.diagnostics.push(Diagnostic::error("Use SQLite.open(path) or connection.query(sql) to create SQLite handles", Some(expr.span)));
                    return None;
                }
                if class_name == "Channel" {
                    if args.len() > 1 {
                        self.diagnostics.push(Diagnostic::error(
                            "Channel<T> accepts at most one capacity argument",
                            Some(expr.span),
                        ));
                    }
                    for arg in args {
                        if let Some(actual) = self.check_expr(cls, arg) {
                            if actual != Type::Int && actual != Type::Long {
                                self.diagnostics.push(Diagnostic::error(
                                    format!(
                                        "Channel capacity must be an integer, found '{}'",
                                        actual
                                    ),
                                    Some(arg.span),
                                ));
                            }
                        }
                    }
                    let element = generic_type
                        .as_ref()
                        .map(|ty| self.type_spec_to_type(ty))
                        .unwrap_or(Type::Int);
                    let supported_element = match &element {
                        Type::Void => false,
                        Type::Nullable(inner) => matches!(
                            inner.as_ref(),
                            Type::Int | Type::Long | Type::Byte | Type::Bool | Type::Decimal
                                | Type::String
                                | Type::Class { .. }
                                | Type::Async(_)
                                | Type::Channel(_)
                                | Type::Bytes
                                | Type::InputStream
                                | Type::OutputStream
                                | Type::FileStream
                        ),
                        _ => true,
                    };
                    if !supported_element {
                        self.diagnostics.push(Diagnostic::error(
                            format!("Channel payload type '{}' cannot be represented by the channel ABI", element),
                            Some(expr.span),
                        ));
                    }
                    return Some(Type::Channel(Box::new(element)));
                }
                let mut arg_types = Vec::new();
                for arg in args {
                    if let Some(t) = self.check_expr(cls, arg) {
                        arg_types.push(t);
                    }
                }
                if let Some(info) = self.classes.get(class_name).cloned() {
                    if args.len() > info.max_constructor_args {
                        self.diagnostics.push(Diagnostic::error(
                            format!(
                                "Class '{}' constructor expected at most {} arguments, got {}",
                                class_name,
                                info.max_constructor_args,
                                args.len()
                            ),
                            Some(expr.span),
                        ));
                    } else {
                        for (i, (arg_ty, param_ty)) in
                            arg_types.iter().zip(info.param_types.iter()).enumerate()
                        {
                            if !self.is_assignable_to(arg_ty, param_ty) {
                                self.diagnostics.push(Diagnostic::error(
                                    format!(
                                        "Expected '{:?}' for argument {} of '{}', but got '{:?}'",
                                        param_ty,
                                        i + 1,
                                        class_name,
                                        arg_ty
                                    ),
                                    Some(expr.span),
                                ));
                            }
                        }
                    }
                } else if !self.traits.contains_key(class_name) {
                    self.diagnostics.push(Diagnostic::error(
                        format!("Unknown class '{}'", class_name),
                        Some(expr.span),
                    ));
                }
                Some(Type::Class {
                    name: class_name.to_string(),
                })
            }
            ExprKind::Binary { op, left, right } => {
                let l_ty = self.check_expr(cls, left);
                let r_ty = self.check_expr(cls, right);
                if (l_ty.as_ref().is_some_and(|ty| ty.nullable_scalar_inner().is_some()) || r_ty.as_ref().is_some_and(|ty| ty.nullable_scalar_inner().is_some())) && !matches!(op, BinaryOp::Equal | BinaryOp::NotEqual | BinaryOp::NullCoalescing) {
                    self.diagnostics.push(Diagnostic::error("Use ?? to provide a non-null scalar before arithmetic or ordering comparisons", Some(expr.span)));
                }
                match op {
                    BinaryOp::NullCoalescing => {
                        if let Some(Type::Nullable(inner)) = l_ty {
                            if let Some(right) = r_ty {
                                if !self.is_assignable_to(&right, &inner) {
                                    self.diagnostics.push(Diagnostic::error("Incompatible fallback for nullable value", Some(expr.span)));
                                }
                            }
                            Some(*inner)
                        } else { l_ty }
                    }
                    BinaryOp::And | BinaryOp::Or => {
                        if let (Some(lt), Some(rt)) = (&l_ty, &r_ty) {
                            if lt != &Type::Bool || rt != &Type::Bool {
                                self.diagnostics.push(Diagnostic::error(
                                    format!("Operator '{:?}' requires Bool operands, found '{:?}' and '{:?}'", op, lt, rt),
                                    Some(expr.span),
                                ));
                            }
                        }
                        Some(Type::Bool)
                    }
                    BinaryOp::Equal
                    | BinaryOp::NotEqual
                    | BinaryOp::Less
                    | BinaryOp::LessEqual
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEqual => Some(Type::Bool),
                    _ => {
                        let is_concurrent = |ty: &Option<Type>| {
                            matches!(ty, Some(Type::Async(_) | Type::Channel(_)))
                        };
                        if is_concurrent(&l_ty) || is_concurrent(&r_ty) {
                            self.diagnostics.push(Diagnostic::error(
                                "Cannot do arithmetic on an Async or Channel; `await` it first, or write `async (a + b)` to run the whole expression as a task",
                                Some(expr.span),
                            ));
                        }
                        l_ty.or(r_ty).or(Some(Type::Int))
                    }
                }
            }
            ExprKind::Unary { op, operand } => {
                let op_ty = self.check_expr(cls, operand);
                if let Some(ty) = &op_ty {
                    match op {
                        UnaryOp::Not => {
                            if ty != &Type::Bool {
                                self.diagnostics.push(Diagnostic::error(
                                    format!(
                                        "Unary 'not' operator requires Bool operand, found '{:?}'",
                                        ty
                                    ),
                                    Some(expr.span),
                                ));
                            }
                            Some(Type::Bool)
                        }
                        UnaryOp::Neg => {
                            if ty != &Type::Int && ty != &Type::Long && ty != &Type::Decimal {
                                self.diagnostics.push(Diagnostic::error(
                                    format!(
                                        "Unary '-' operator requires numeric operand, found '{:?}'",
                                        ty
                                    ),
                                    Some(expr.span),
                                ));
                            }
                            Some(ty.clone())
                        }
                    }
                } else {
                    Some(Type::Bool)
                }
            }
            ExprKind::AnonymousObject { fields } => {
                let mut field_types = Vec::new();
                for (name, fexpr) in fields {
                    if let Some(fty) = self.check_expr(cls, fexpr) {
                        field_types.push((name.clone(), fty));
                    }
                }
                Some(Type::AnonymousObject { field_types })
            }
            ExprKind::ListLiteral(elements) => {
                for elem in elements {
                    if let Some(ty) = self.check_expr(cls, elem) {
                        if ty.nullable_scalar_inner().is_some() || matches!(&ty, Type::Async(inner) if inner.nullable_scalar_inner().is_some()) {
                            self.diagnostics.push(Diagnostic::error("Nullable scalar values and tasks with nullable scalar results are not supported in untyped List storage yet", Some(elem.span)));
                        }
                    }
                }
                Some(Type::Class {
                    name: "List".into(),
                })
            }
            ExprKind::MapLiteral(pairs) => {
                for (k, v) in pairs {
                    for value in [k, v] {
                        if let Some(ty) = self.check_expr(cls, value) {
                            if ty.nullable_scalar_inner().is_some() || matches!(&ty, Type::Async(inner) if inner.nullable_scalar_inner().is_some()) {
                                self.diagnostics.push(Diagnostic::error("Nullable scalar values and tasks with nullable scalar results are not supported in untyped Map storage yet", Some(value.span)));
                            }
                        }
                    }
                }
                Some(Type::Class { name: "Map".into() })
            }
            ExprKind::IndexAccess { target, index } => {
                let target_ty = self.check_expr(cls, target);
                let index_ty = self.check_expr(cls, index);
                if let Some(Type::Class { ref name }) = target_ty {
                    if name == "ConsoleArgs" {
                        if index_ty != Some(Type::Int) { self.diagnostics.push(Diagnostic::error("ConsoleArgs index must be int", Some(index.span))); }
                        return Some(Type::String);
                    }
                    if name == "TaskOutcomes" {
                        if index_ty != Some(Type::Int) { self.diagnostics.push(Diagnostic::error("TaskOutcomes index must be int", Some(index.span))); }
                        return Some(Type::Class { name: "TaskOutcome".into() });
                    }
                    if name == "Map" {
                        return Some(Type::Nullable(Box::new(Type::Int)));
                    }
                }
                Some(Type::Int)
            }
            ExprKind::AsyncSpawn(inner) => match self.check_expr(cls, inner) {
                Some(result_type) => Some(Type::Async(Box::new(result_type))),
                None => None,
            },
            ExprKind::Await(inner) => match self.check_expr(cls, inner) {
                Some(Type::Async(result)) => Some(*result),
                Some(other) => {
                    self.diagnostics.push(Diagnostic::error(
                        format!(".await() requires an Async, found '{:?}'", other),
                        Some(inner.span),
                    ));
                    None
                }
                None => None,
            },
            ExprKind::BlockClosure { params, body, return_type } => {
                let saved_symbols = self.symbol_table.clone();
                let saved_loop_depth = std::mem::replace(&mut self.loop_depth, 0);
                let saved_expression = std::mem::replace(&mut self.in_expression_closure, false);
                let saved_returns = self.closure_returns.replace(Vec::new());
                let saved_return_type = self.current_return_type.take();
                self.symbol_table.freeze_bindings();
                self.symbol_table.push_scope();
                let mut param_types = Vec::new();
                for param in params {
                    let ty = self.type_spec_to_type(&param.type_spec);
                    if ty == Type::Void {
                        self.diagnostics.push(Diagnostic::error("Func parameters cannot have type void", Some(param.span)));
                    }
                    param_types.push(ty.clone());
                    if !self.symbol_table.insert(Symbol { name: param.name.clone(), ty, is_mutable: false }) {
                        self.diagnostics.push(Diagnostic::error(format!("Duplicate Func parameter '{}'", param.name), Some(param.span)));
                    }
                }
                for statement in body {
                    self.check_stmt(cls, statement);
                }
                let returns = self.closure_returns.take().unwrap();
                let inferred = returns.first().cloned().unwrap_or(Type::Void);
                if returns.iter().any(|ty| ty != &inferred) {
                    self.diagnostics.push(Diagnostic::error("Block lambda returns must have the same type", Some(expr.span)));
                }
                if inferred != Type::Void && !body.iter().any(block_statement_returns) {
                    self.diagnostics.push(Diagnostic::error("Block lambda must return a value on every path", Some(expr.span)));
                }
                *return_type.borrow_mut() = Some(inferred.clone());
                self.symbol_table = saved_symbols;
                self.loop_depth = saved_loop_depth;
                self.in_expression_closure = saved_expression;
                self.closure_returns = saved_returns;
                self.current_return_type = saved_return_type;
                Some(Type::Function { params: param_types, return_type: Box::new(inferred) })
            }
            ExprKind::Closure { params, expression } => {
                let saved_symbols = self.symbol_table.clone();
                let saved_loop_depth = self.loop_depth;
                let saved_closure = self.in_expression_closure;
                self.loop_depth = 0;
                self.in_expression_closure = true;
                self.symbol_table.freeze_bindings();
                self.symbol_table.push_scope();
                let mut param_types = Vec::new();
                for param in params {
                    let ty = self.type_spec_to_type(&param.type_spec);
                    if ty == Type::Void {
                        self.diagnostics.push(Diagnostic::error("Func parameters cannot have type void", Some(param.span)));
                    }
                    param_types.push(ty.clone());
                    if !self.symbol_table.insert(Symbol { name: param.name.clone(), ty, is_mutable: false }) {
                        self.diagnostics.push(Diagnostic::error(format!("Duplicate Func parameter '{}'", param.name), Some(param.span)));
                    }
                }
                let result = self.check_expr(cls, expression);
                self.symbol_table = saved_symbols;
                self.loop_depth = saved_loop_depth;
                self.in_expression_closure = saved_closure;
                result.map(|return_type| Type::Function { params: param_types, return_type: Box::new(return_type) })
            }
            ExprKind::Lambda { parameter, body } => {
                let outer_closure = std::mem::replace(&mut self.in_expression_closure, false);
                let saved_returns = self.closure_returns.take();
                let outer_return_type = self.current_return_type.clone();
                let return_type = Type::Class {
                    name: "Response".into(),
                };
                self.current_return_type = Some(return_type.clone());
                self.symbol_table.push_scope();
                self.symbol_table.insert(Symbol {
                    name: parameter.clone(),
                    ty: Type::Class {
                        name: "Request".into(),
                    },
                    is_mutable: false,
                });

                for (index, statement) in body.iter().enumerate() {
                    if index + 1 == body.len() {
                        if let StmtKind::Expr(result) = &statement.kind {
                            let actual_type = self.check_expr(cls, result);
                            if actual_type
                                .as_ref()
                                .is_some_and(|ty| !self.is_assignable_to(ty, &return_type))
                            {
                                self.diagnostics.push(Diagnostic::error(
                                    format!("Server route handler must return '{}'", return_type),
                                    Some(result.span),
                                ));
                            }
                            continue;
                        }
                    }
                    self.check_stmt(cls, statement);
                }

                self.symbol_table.pop_scope();
                self.current_return_type = outer_return_type;
                self.in_expression_closure = outer_closure;
                self.closure_returns = saved_returns;
                Some(Type::Function {
                    params: vec![Type::Class {
                        name: "Request".into(),
                    }],
                    return_type: Box::new(return_type),
                })
            }
            ExprKind::Conditional { condition, when_true, when_false, result_type } => {
                if let Some(ty) = self.check_expr(cls, condition) {
                    if !self.is_valid_condition_type(&ty) {
                        self.diagnostics.push(Diagnostic::error(
                            format!("Conditional requires a Bool or truthy condition, found '{}'", ty), Some(condition.span)));
                    }
                }
                let left = self.check_expr(cls, when_true);
                let right = self.check_expr(cls, when_false);
                let joined = left.as_ref().zip(right.as_ref()).and_then(|(a, b)| Type::conditional_result(a, b));
                if joined.is_none() && left.is_some() && right.is_some() {
                    self.diagnostics.push(Diagnostic::error(
                        format!("Conditional branches require compatible non-void values, found '{}' and '{}'", left.unwrap(), right.unwrap()), Some(expr.span)));
                }
                *result_type.borrow_mut() = joined.clone();
                joined
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_ty = self.check_expr(cls, condition);
                if let Some(cty) = cond_ty {
                    if !self.is_valid_condition_type(&cty) {
                        self.diagnostics.push(Diagnostic::error(
                            format!(
                                "If condition must be Bool or truthy expression, found '{:?}'",
                                cty
                            ),
                            Some(condition.span),
                        ));
                    }
                }
                let mut yield_ty = None;
                self.symbol_table.push_scope();
                for s in then_branch {
                    self.check_stmt(cls, s);
                    if let StmtKind::Yield(expr) = &s.kind {
                        yield_ty = self.check_expr(cls, expr);
                    }
                }
                self.symbol_table.pop_scope();

                if let Some(eb) = else_branch {
                    self.symbol_table.push_scope();
                    for s in eb {
                        self.check_stmt(cls, s);
                        if let StmtKind::Yield(expr) = &s.kind {
                            let e_ty = self.check_expr(cls, expr);
                            if yield_ty.is_none() {
                                yield_ty = e_ty;
                            }
                        }
                    }
                    self.symbol_table.pop_scope();
                }
                yield_ty.or(Some(Type::Int))
            }
            ExprKind::For {
                loop_var,
                index_var,
                condition_or_iterable,
                body,
                else_branch,
            } => {
                self.symbol_table.push_scope();
                let element_ty = if loop_var.is_some() && condition_or_iterable.as_ref().is_some_and(|expr| matches!(self.check_expr(cls, expr), Some(Type::Class { name }) if name == "ConsoleArgs")) { Type::String } else { Type::Int };
                if let Some(lvar) = loop_var {
                    self.symbol_table.insert(Symbol {
                        name: lvar.clone(),
                        ty: element_ty,
                        is_mutable: false,
                    });
                }
                if let Some(ivar) = index_var {
                    self.symbol_table.insert(Symbol {
                        name: ivar.clone(),
                        ty: Type::Int,
                        is_mutable: false,
                    });
                }
                if let Some(cond) = condition_or_iterable {
                    self.check_expr(cls, cond);
                }
                self.loop_depth += 1;
                for s in body {
                    self.check_stmt(cls, s);
                }
                self.loop_depth -= 1;
                self.symbol_table.pop_scope();

                if let Some(eb) = else_branch {
                    self.symbol_table.push_scope();
                    for s in eb {
                        self.check_stmt(cls, s);
                    }
                    self.symbol_table.pop_scope();
                }

                Some(Type::Class {
                    name: "List".into(),
                })
            }
            _ => None,
        }
    }

    pub fn satisfies_trait(&self, source: &str, target: &str) -> bool {
        let Some(required) = self.traits.get(target) else { return false; };
        let methods = self.classes.get(source).map(|c| &c.methods)
            .or_else(|| self.traits.get(source).map(|t| &t.methods));
        methods.is_some_and(|methods| required.methods.iter().all(|(name, signature)| {
            let is_static = self.orm_declarations.get(source)
                .is_some_and(|class| class.methods.iter().any(|method| method.name == *name && method.is_static));
            !is_static && methods.get(name)
                .map_or_else(|| required.defaults.contains(name), |actual| actual == signature)
        }))
    }

    fn is_output_type(&self, ty: &Type) -> bool {
        match ty {
            Type::String | Type::Int | Type::Long | Type::Byte | Type::Decimal | Type::Bool => true,
            Type::Nullable(inner) => matches!(inner.as_ref(), Type::Void | Type::String | Type::Int | Type::Long | Type::Byte | Type::Decimal | Type::Bool),
            Type::Class { name } => self.satisfies_trait(name, "Error"),
            _ => false,
        }
    }

    fn is_valid_condition_type(&self, ty: &Type) -> bool {
        matches!(
            ty,
            Type::Bool | Type::Nullable(_) | Type::Class { .. } | Type::AnonymousObject { .. }
        )
    }

    fn is_assignable_to(&self, from: &Type, to: &Type) -> bool {
        if matches!(to, Type::Function { .. }) {
            return from == to;
        }
        if from == to || from == &Type::Void {
            return true;
        }
        match (from, to) {
            (Type::Int, Type::Long) | (Type::Long, Type::Int) => true,
            (Type::Int, Type::Decimal) | (Type::Long, Type::Decimal) => true,
            (Type::Int, Type::Class { name }) if name == "Function" => true,
            (Type::Class { name: from_name }, Type::Class { name: to_name }) => {
                from_name == to_name || self.satisfies_trait(from_name, to_name)
            }
            (Type::Orm { model: a, kind: ak, .. }, Type::Orm { model: b, kind: bk, joins }) if a == b && ak == bk && joins.is_empty() => true,
            (Type::Channel(from), Type::Channel(to)) => self.is_assignable_to(from, to),
            (from_ty, Type::Nullable(inner)) => {
                from_ty == &Type::Nullable(Box::new(Type::Void))
                    || self.is_assignable_to(from_ty, inner)
            }
            _ => false,
        }
    }

    fn type_spec_to_type(&self, spec: &TypeSpec) -> Type {
        match spec {
            TypeSpec::Int => Type::Int,
            TypeSpec::Decimal => Type::Decimal,
            TypeSpec::Byte => Type::Byte,
            TypeSpec::Long => Type::Long,
            TypeSpec::Bool => Type::Bool,
            TypeSpec::String => Type::String,
            TypeSpec::Class(name) => {
                if name == "void" {
                    Type::Void
                } else {
                    Type::Class {
                        name: canonical_builtin_class_name(name).unwrap_or(name).into(),
                    }
                }
            }
            TypeSpec::Function { params, return_type } => Type::Function {
                params: params.iter().map(|p| self.type_spec_to_type(p)).collect(),
                return_type: Box::new(self.type_spec_to_type(return_type)),
            },
            TypeSpec::Generic(name, element) if name == "Async" => {
                Type::Async(Box::new(self.type_spec_to_type(element)))
            }
            TypeSpec::Generic(name, element) if name == "Channel" => {
                Type::Channel(Box::new(self.type_spec_to_type(element)))
            }
            TypeSpec::Generic(name, element) if crate::orm::generic(name, element).is_some() => crate::orm::generic(name, element).unwrap(),
            TypeSpec::Generic(name, _) => Type::Class {
                name: canonical_builtin_class_name(name).unwrap_or(name).into(),
            },
            TypeSpec::Nullable(inner) => Type::Nullable(Box::new(self.type_spec_to_type(inner))),
        }
    }
}

include!("orm.rs");

include!("mockery.rs");

impl TypeChecker {
    /// Return Some for user static members or an invalid class receiver;
    /// leave ordinary instance members and built-in APIs to their existing checks.
    fn check_static_member(
        &mut self, cls: &ClassDecl, object: &Expr, object_ty: &Type,
        member: &str, span: crate::diagnostics::Span,
        arguments: Option<&[Type]>, assigned: Option<&Type>,
    ) -> Option<Option<Type>> {
        let Type::Class { name } = object_ty else { return None; };
        let declaration = self.orm_declarations.get(name)?.clone();
        let class_receiver = matches!(&object.kind, ExprKind::Identifier(n)
            if n == "static" || (self.symbol_table.lookup(n).is_none() && self.orm_declarations.contains_key(n)));
        let field = declaration.static_fields.iter().find(|f| f.name == member);
        let method = declaration.methods.iter().find(|m| m.name == member && m.is_static);
        if field.is_none() && method.is_none() {
            if class_receiver && !declaration.cases.iter().any(|c| c.name == member) {
                self.diagnostics.push(Diagnostic::error(format!("'{}' is not a static member of '{}'; requires an instance of '{}'", member, name, name), Some(span)));
                return Some(None);
            }
            return None;
        }
        let mut fail = |message: String| {
            self.diagnostics.push(Diagnostic::error(message, Some(span)));
            Some(None)
        };
        if !class_receiver { return fail(format!("static member '{}' requires 'static.' inside '{}' or '{}.' outside it", member, name, name)); }
        if name == &cls.name && !matches!(&object.kind, ExprKind::Identifier(n) if n == "static") {
            return fail(format!("static member '{}' must be accessed through 'static.' inside '{}'", member, name));
        }
        if member.starts_with('_') && name != &cls.name { return fail(format!("static member '{}' is private to class '{}'", member, name)); }
        if let Some(field) = field {
            if arguments.is_some() { return fail(format!("static field '{}' is not a method", member)); }
            let ty = self.type_spec_to_type(&field.type_spec);
            if let Some(actual) = assigned {
                if !field.is_mutable {
                    self.diagnostics.push(Diagnostic::error(format!("cannot reassign immutable static field '{}'", member), Some(span)));
                }
                if !self.is_assignable_to(actual, &ty) {
                    self.diagnostics.push(Diagnostic::error(format!("Static field '{}' expects '{}', found '{}'", member, ty, actual), Some(span)));
                }
            }
            return Some(Some(ty));
        }
        let method = method.unwrap();
        let Some(arguments) = arguments else { return fail(format!("static method '{}' requires a call", member)); };
        if assigned.is_some() { return fail(format!("static method '{}' is not assignable", member)); }
        let expected: Vec<_> = method.params.iter().map(|p| self.type_spec_to_type(&p.type_spec)).collect();
        if arguments.len() != expected.len() || !arguments.iter().zip(&expected).all(|(a, e)| self.is_assignable_to(a, e)) {
            self.diagnostics.push(Diagnostic::error(format!("Invalid arguments for static method '{}.{}'", name, member), Some(span)));
        }
        let ty = method.return_type.as_ref().map_or(Type::Void, |t| self.type_spec_to_type(t));
        Some(Some(if method.is_async { Type::Async(Box::new(if ty == Type::Void { Type::Int } else { ty })) } else { ty }))
    }
}

// Yield produces an expression value; it does not exit a block lambda.
fn block_statement_returns(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Return(_) | StmtKind::Throw(_) => true,
        StmtKind::If { then_branch, else_branch, .. } => {
            then_branch.iter().any(block_statement_returns)
                && else_branch.as_ref().is_some_and(|body| body.iter().any(block_statement_returns))
        }
        StmtKind::Try { body, catches, finally_body } => {
            finally_body.as_ref().is_some_and(|body| body.iter().any(block_statement_returns))
                || (body.iter().any(block_statement_returns)
                    && catches.iter().all(|catch| catch.body.iter().any(block_statement_returns)))
        }
        _ => false,
    }
}
