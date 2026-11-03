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
            loop_depth: 0,
            diagnostics: Vec::new(),
        };
        checker.register_builtins();
        checker
    }

    fn register_builtins(&mut self) {
        let mut console_methods = HashMap::new();
        console_methods.insert("print".into(), (vec![Type::String], Type::Void));

        self.classes.insert(
            "Console".into(),
            ClassInfo {
                name: "Console".into(),
                fields: HashMap::new(),
                methods: console_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

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

        let mut channel_methods = HashMap::new();
        channel_methods.insert(
            "send".into(),
            (vec![Type::Int], Type::Task(Box::new(Type::Bool))),
        );
        channel_methods.insert(
            "receive".into(),
            (Vec::new(), Type::Task(Box::new(Type::Int))),
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
                Type::Task(Box::new(Type::Class {
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
                Type::Task(Box::new(Type::Int)),
            ),
        );
        task_methods.insert(
            "race".into(),
            (
                vec![Type::Class {
                    name: "List".into(),
                }],
                Type::Task(Box::new(Type::Int)),
            ),
        );
        task_methods.insert(
            "allSettled".into(),
            (
                vec![Type::Class {
                    name: "List".into(),
                }],
                Type::Task(Box::new(Type::Class {
                    name: "List".into(),
                })),
            ),
        );
        task_methods.insert("cancel".into(), (Vec::new(), Type::Bool));

        self.classes.insert(
            "Task".into(),
            ClassInfo {
                name: "Task".into(),
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

        let mut request_methods = HashMap::new();
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
            "parse".into(),
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
            "stringify".into(),
            (
                vec![Type::Class {
                    name: "JsonObject".into(),
                }],
                Type::String,
            ),
        );
        json_methods.insert(
            "from".into(),
            (
                vec![Type::Class {
                    name: "JsonObject".into(),
                }],
                Type::String,
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

        let mut xml_methods = HashMap::new();
        xml_methods.insert(
            "parse".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "XmlDocument".into(),
                },
            ),
        );
        xml_methods.insert(
            "partialParse".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "XmlDocument".into(),
                },
            ),
        );
        xml_methods.insert(
            "create".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "XmlDocument".into(),
                },
            ),
        );
        xml_methods.insert(
            "stringify".into(),
            (
                vec![Type::Class {
                    name: "XmlDocument".into(),
                }],
                Type::String,
            ),
        );
        xml_methods.insert(
            "from".into(),
            (
                vec![Type::Class {
                    name: "XmlDocument".into(),
                }],
                Type::String,
            ),
        );
        xml_methods.insert(
            "to".into(),
            (
                vec![Type::String, Type::String],
                Type::Class {
                    name: "XmlDocument".into(),
                },
            ),
        );

        self.classes.insert(
            "Xml".into(),
            ClassInfo {
                name: "Xml".into(),
                fields: HashMap::new(),
                methods: xml_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut xmldoc_methods = HashMap::new();
        xmldoc_methods.insert(
            "root".into(),
            (
                Vec::new(),
                Type::Class {
                    name: "XmlElement".into(),
                },
            ),
        );
        xmldoc_methods.insert("toString".into(), (Vec::new(), Type::String));

        self.classes.insert(
            "XmlDocument".into(),
            ClassInfo {
                name: "XmlDocument".into(),
                fields: HashMap::new(),
                methods: xmldoc_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

        let mut xmlem_methods = HashMap::new();
        xmlem_methods.insert(
            "child".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "XmlElement".into(),
                },
            ),
        );
        xmlem_methods.insert(
            "element".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "XmlElement".into(),
                },
            ),
        );
        xmlem_methods.insert(
            "text".into(),
            (
                vec![Type::String],
                Type::Class {
                    name: "XmlElement".into(),
                },
            ),
        );
        xmlem_methods.insert("getText".into(), (Vec::new(), Type::String));

        self.classes.insert(
            "XmlElement".into(),
            ClassInfo {
                name: "XmlElement".into(),
                fields: HashMap::new(),
                methods: xmlem_methods,
                param_types: Vec::new(),
                min_constructor_args: 0,
                max_constructor_args: 0,
            },
        );

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
                            Type::Task(Box::new(if return_ty == Type::Void {
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

        for item in &program.items {
            if let Item::Class(cls) = item {
                self.check_class(cls);
            }
        }

        self.diagnostics.is_empty()
    }

    fn check_class(&mut self, cls: &ClassDecl) {
        self.current_class = Some(cls.name.clone());

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
                if let Some(lvar) = loop_var {
                    self.symbol_table.insert(Symbol {
                        name: lvar.clone(),
                        ty: Type::Int,
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
                                if act != expected_ret && act != Type::Void {
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
            ExprKind::Print {
                expr: inner_expr, ..
            } => {
                self.check_expr(cls, inner_expr);
                Some(Type::Void)
            }
            ExprKind::IntLiteral(_) => Some(Type::Int),
            ExprKind::LongLiteral(_) => Some(Type::Long),
            ExprKind::DecimalLiteral(_) => Some(Type::Decimal),
            ExprKind::StringLiteral(_) => Some(Type::String),
            ExprKind::BoolLiteral(_) => Some(Type::Bool),
            ExprKind::NullLiteral => Some(Type::Nullable(Box::new(Type::Void))),
            ExprKind::This => Some(Type::Class {
                name: cls.name.clone(),
            }),
            ExprKind::Identifier(name) => {
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
                        format!("Undeclared identifier '{}'", name),
                        Some(expr.span),
                    ));
                    None
                }
            }
            ExprKind::MemberAccess { object, member, .. } => {
                let obj_ty = self.check_expr(cls, object)?;
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
                    Type::Task(_) if member == "cancel" => Some(Type::Bool),
                    Type::Task(_) if member == "timeout" => Some(obj_ty.clone()),
                    Type::Channel(_) if matches!(member.as_str(), "send" | "receive" | "close") => {
                        Some(Type::Void)
                    }
                    _ => None,
                }
            }
            ExprKind::Call { callee, args } => {
                let mut arg_types = Vec::new();
                for arg in args {
                    if let Some(t) = self.check_expr(cls, arg) {
                        arg_types.push(t);
                    }
                }
                match &callee.kind {
                    ExprKind::MemberAccess { object, member, .. } => {
                        if let Some(obj_ty) = self.check_expr(cls, object) {
                            if matches!(obj_ty, Type::Task(_)) && member == "cancel" {
                                return Some(Type::Bool);
                            }
                            if matches!(obj_ty, Type::Task(_)) && member == "timeout" {
                                let valid = args.len() == 1
                                    && arg_types.len() == 1
                                    && matches!(arg_types[0], Type::Int | Type::Long);
                                if !valid {
                                    self.diagnostics.push(Diagnostic::error(
                                        "Task.timeout expects one Int argument: the timeout in milliseconds",
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
                                        Some(Type::Task(Box::new(Type::Bool)))
                                    }
                                    "receive" if args.is_empty() => {
                                        Some(Type::Task(element.clone()))
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
                                if name == "Task" && member == "async" {
                                    self.diagnostics.push(Diagnostic::error(
                                        "Task.async(...) was removed; start a task with `async expr` instead",
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
            ExprKind::Assign { target, value } => {
                let val_ty = self.check_expr(cls, value)?;
                match &target.kind {
                    ExprKind::MemberAccess { object, member, .. } => {
                        let obj_ty = self.check_expr(cls, object);
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
                let class_name = canonical_builtin_class_name(class_name).unwrap_or(class_name);
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
                            Type::String
                                | Type::Class { .. }
                                | Type::Task(_)
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
                    BinaryOp::And
                    | BinaryOp::Nand
                    | BinaryOp::Or
                    | BinaryOp::Nor
                    | BinaryOp::Xor
                    | BinaryOp::Xnor => {
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
                            matches!(ty, Some(Type::Task(_) | Type::Channel(_)))
                        };
                        if is_concurrent(&l_ty) || is_concurrent(&r_ty) {
                            self.diagnostics.push(Diagnostic::error(
                                "Cannot do arithmetic on a Task or Channel; `await` it first, or write `async (a + b)` to run the whole expression as a task",
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
                    self.check_expr(cls, elem);
                }
                Some(Type::Class {
                    name: "List".into(),
                })
            }
            ExprKind::MapLiteral(pairs) => {
                for (k, v) in pairs {
                    self.check_expr(cls, k);
                    self.check_expr(cls, v);
                }
                Some(Type::Class { name: "Map".into() })
            }
            ExprKind::IndexAccess { target, index } => {
                let target_ty = self.check_expr(cls, target);
                self.check_expr(cls, index);
                if let Some(Type::Class { ref name }) = target_ty {
                    if name == "Map" {
                        return Some(Type::Nullable(Box::new(Type::Int)));
                    }
                }
                Some(Type::Int)
            }
            ExprKind::AsyncSpawn(inner) => match self.check_expr(cls, inner) {
                Some(result_type) => Some(Type::Task(Box::new(result_type))),
                None => None,
            },
            ExprKind::Await(inner) => match self.check_expr(cls, inner) {
                Some(Type::Task(result)) => Some(*result),
                Some(other) => {
                    self.diagnostics.push(Diagnostic::error(
                        format!("await requires a Task, found '{:?}'", other),
                        Some(inner.span),
                    ));
                    None
                }
                None => None,
            },
            ExprKind::Lambda { parameter, body } => {
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
                Some(Type::Function {
                    params: vec![Type::Class {
                        name: "Request".into(),
                    }],
                    return_type: Box::new(return_type),
                })
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
                if let Some(lvar) = loop_var {
                    self.symbol_table.insert(Symbol {
                        name: lvar.clone(),
                        ty: Type::Int,
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
        methods.is_some_and(|methods| required.methods.iter().all(|(name, signature)| methods.get(name).map_or_else(|| required.defaults.contains(name), |actual| actual == signature)))
    }

    fn is_valid_condition_type(&self, ty: &Type) -> bool {
        matches!(
            ty,
            Type::Bool | Type::Nullable(_) | Type::Class { .. } | Type::AnonymousObject { .. }
        )
    }

    fn is_assignable_to(&self, from: &Type, to: &Type) -> bool {
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
            TypeSpec::Generic(name, element) if name == "Channel" => {
                Type::Channel(Box::new(self.type_spec_to_type(element)))
            }
            TypeSpec::Generic(name, _) => Type::Class {
                name: canonical_builtin_class_name(name).unwrap_or(name).into(),
            },
            TypeSpec::Nullable(inner) => Type::Nullable(Box::new(self.type_spec_to_type(inner))),
        }
    }
}
