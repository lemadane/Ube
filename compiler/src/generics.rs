//! Reified generic specialization before semantic checking and native lowering.
//! Every concrete application has a distinct class identity and field layout.
use crate::diagnostics::Span;
use crate::parser::ast::*;
use crate::{Diagnostic, Lexer, Parser};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

type Env = BTreeMap<String, TypeSpec>;
const PREFIX: &str = "UbeGeneric_";

fn application(ty: &TypeSpec) -> Option<(&str, Vec<TypeSpec>)> {
    match ty {
        TypeSpec::Generic(n, t) => Some((n, vec![*t.clone()])),
        TypeSpec::Applied(n, ts) => Some((n, ts.clone())),
        _ => None,
    }
}
fn applied(n: &str, mut ts: Vec<TypeSpec>) -> TypeSpec {
    if ts.len() == 1 {
        TypeSpec::Generic(n.into(), Box::new(ts.remove(0)))
    } else {
        TypeSpec::Applied(n.into(), ts)
    }
}
fn spelling(t: &TypeSpec) -> String {
    match t {
        TypeSpec::Int => "int".into(),
        TypeSpec::Long => "long".into(),
        TypeSpec::Byte => "byte".into(),
        TypeSpec::Decimal => "decimal".into(),
        TypeSpec::Bool => "bool".into(),
        TypeSpec::String => "string".into(),
        TypeSpec::Class(n) => n.clone(),
        TypeSpec::Nullable(t) => format!("({})?", spelling(t)),
        TypeSpec::Generic(n, t) => format!("({n} of {})", spelling(t)),
        TypeSpec::Applied(n, ts) => format!(
            "({n} of {})",
            ts.iter().map(spelling).collect::<Vec<_>>().join(if n == "Map" { ":" } else { "," })
        ),
        TypeSpec::Union(ts) => ts.iter().map(spelling).collect::<Vec<_>>().join(" | "),
        TypeSpec::Function {
            params,
            return_type,
        } => format!(
            "Func({}) -> {}",
            params.iter().map(spelling).collect::<Vec<_>>().join(","),
            spelling(return_type)
        ),
    }
}
fn key(n: &str, ts: &[TypeSpec]) -> String {
    let source = format!(
        "{}<{}>",
        n,
        ts.iter().map(spelling).collect::<Vec<_>>().join(",")
    );
    format!("{PREFIX}{:x}", Sha256::digest(source.as_bytes()))
}
fn substitute(t: &TypeSpec, env: &Env) -> TypeSpec {
    match t {
        TypeSpec::Class(n) => env.get(n).cloned().unwrap_or(t.clone()),
        TypeSpec::Nullable(i) => TypeSpec::Nullable(Box::new(substitute(i, env))),
        TypeSpec::Generic(n, i) => applied(n, vec![substitute(i, env)]),
        TypeSpec::Applied(n, ts) => applied(n, ts.iter().map(|t| substitute(t, env)).collect()),
        TypeSpec::Union(ts) => TypeSpec::Union(ts.iter().map(|t| substitute(t, env)).collect()),
        TypeSpec::Function {
            params,
            return_type,
        } => TypeSpec::Function {
            params: params.iter().map(|t| substitute(t, env)).collect(),
            return_type: Box::new(substitute(return_type, env)),
        },
        _ => t.clone(),
    }
}
fn expr(kind: ExprKind) -> Expr {
    Expr {
        kind,
        span: Span::default(),
    }
}
fn id(n: &str) -> Expr {
    expr(ExprKind::Identifier(n.into()))
}
fn invoke(receiver: Expr, name: &str, args: Vec<Expr>) -> Expr {
    expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::MemberAccess {
            object: Box::new(receiver),
            member: name.into(),
            is_optional: false,
        })),
        args,
    })
}
fn parse_generated(source: &str) -> Vec<Item> {
    let mut parser = Parser::new(Lexer::new(source));
    let p = parser.parse_program();
    assert!(
        parser.diagnostics.is_empty(),
        "invalid internal generic template: {:?}\n{source}",
        parser.diagnostics
    );
    p.items
}

pub fn expand(program: &Program) -> (Program, Vec<Diagnostic>) {
    let mut e = Expander {
        method_depth: 0,
        builtins: crate::TypeChecker::new().builtin_signatures(),
        templates: BTreeMap::new(),
        concrete: BTreeMap::new(),
        traits: BTreeMap::new(),
        origins: BTreeMap::new(),
        unions: BTreeMap::new(),
        pending: VecDeque::new(),
        output: vec![],
        methods: vec![],
        instantiated: BTreeSet::new(),
        diagnostics: vec![],
    };
    for item in &program.items {
        match item {
            Item::Class(c) => {
                if c.name.starts_with(PREFIX) || c.name == "UbeTypedMap" || c.name == "UbeMapEntry"
                {
                    e.error(
                        "This type name is reserved for compiler-generated generic types",
                        c.span,
                    );
                }
                e.templates.insert(c.name.clone(), c.clone());
            }
            Item::Trait(t) => {
                e.traits.insert(t.name.clone(), t.clone());
            }
            _ => {}
        }
    }
    // Typed maps own typed entries. Presence is independent of a nullable value.
    for item in parse_generated(
        r#"
class UbeMapEntry<K,V>(K key, var V value)
class UbeTypedMap<K,V>(var (List of (UbeMapEntry of K,V)) entries = (List of (UbeMapEntry of K,V))()) {
    int length() { return this.entries().length() }
    bool containsKey(K key) {
        each entry in this.entries() { if entry.key() == key { return true } }
        return false
    }
    V? get(K key) {
        each entry in this.entries() { if entry.key() == key { return entry.value() } }
        return null
    }
    put(K key, V value) {
        each entry in this.entries() { if entry.key() == key { entry.value(value) return } }
        this.entries().add((UbeMapEntry of K,V)(key,value))
    }
    bool remove(K key) {
        (List of (UbeMapEntry of K,V)) remaining
        var found = false
        each entry in this.entries() {
            if entry.key() == key { found = true } else { remaining.add(entry) }
        }
        this.entries(remaining)
        return found
    }
    clear() { (List of (UbeMapEntry of K,V)) empty this.entries(empty) }
    (List of K) keys() {
        (List of K) result
        each entry in this.entries() { result.add(entry.key()) }
        return result
    }
    (List of V) values() {
        (List of V) result
        each entry in this.entries() { result.add(entry.value()) }
        return result
    }
}
"#,
    ) {
        if let Item::Class(c) = item {
            e.templates.insert(c.name.clone(), c);
        }
    }
    for item in &program.items {
        match item {
            Item::Class(c) if c.type_params.is_empty() => {
                e.concrete.insert(c.name.clone(), c.clone());
                e.pending.push_back((item.clone(), Env::new()));
            }
            Item::Trait(t) if t.type_params.is_empty() => {
                e.pending.push_back((item.clone(), Env::new()))
            }
            Item::Import(_) => e.output.push(item.clone()),
            _ => {}
        }
    }
    while let Some((item, bindings)) = e.pending.pop_front() {
        if e.instantiated.len() > 512 {
            e.error("Generic specialization exceeded 512 concrete applications (possibly expanding recursion)",Span::default());
            break;
        }
        match item {
            Item::Class(mut c) => {
                c.type_params.clear();
                for p in &mut c.primary_params {
                    p.type_spec = e.resolve(&substitute(&p.type_spec, &bindings), p.span);
                }
                for p in &mut c.static_fields {
                    p.type_spec = e.resolve(&substitute(&p.type_spec, &bindings), p.span);
                }
                for case in &mut c.cases {
                    for p in &mut case.params {
                        p.type_spec = e.resolve(&substitute(&p.type_spec, &bindings), p.span);
                    }
                }
                e.materialize_cases(&mut c);
                e.concrete.insert(c.name.clone(), c.clone());
                for p in &mut c.primary_params {
                    if let Some(v) = &mut p.default_value {
                        e.expression(v, &mut Env::new(), &bindings, &c.name, Some(&p.type_spec));
                    }
                }
                for p in &mut c.static_fields {
                    if let Some(v) = &mut p.initializer {
                        e.expression(v, &mut Env::new(), &bindings, &c.name, Some(&p.type_spec));
                    }
                }
                let mut methods = vec![];
                for mut m in c.methods.clone() {
                    if m.type_params.is_empty() {
                        e.method(&mut m, &bindings, &c.name);
                        methods.push(m);
                    }
                }
                c.methods = methods;
                e.output.push(Item::Class(c));
            }
            Item::Trait(mut t) => {
                t.type_params.clear();
                for m in &mut t.methods {
                    e.method(m, &bindings, &t.name);
                }
                e.output.push(Item::Trait(t));
            }
            _ => {}
        }
    }
    // Specializing a method can enqueue more classes or methods. Its body is processed immediately.
    for (owner, m) in e.methods {
        if let Some(Item::Class(c)) = e
            .output
            .iter_mut()
            .find(|i| matches!(i,Item::Class(c) if c.name==owner))
        {
            c.methods.push(m);
        }
    }
    (Program { items: e.output }, e.diagnostics)
}

struct Expander {
    method_depth: usize,
    builtins: std::collections::HashMap<String, crate::semantics::type_checker::ClassInfo>,
    templates: BTreeMap<String, ClassDecl>,
    concrete: BTreeMap<String, ClassDecl>,
    traits: BTreeMap<String, TraitDecl>,
    origins: BTreeMap<String, (String, Env)>,
    unions: BTreeMap<String, Vec<TypeSpec>>,
    pending: VecDeque<(Item, Env)>,
    output: Vec<Item>,
    methods: Vec<(String, MethodDecl)>,
    instantiated: BTreeSet<String>,
    diagnostics: Vec<Diagnostic>,
}
impl Expander {
    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics
            .push(Diagnostic::error(message, Some(span)));
    }
    fn bindings(&mut self, params: &[TypeParameter], args: &[TypeSpec], span: Span) -> Env {
        if params.len() != args.len() {
            self.error(
                format!(
                    "Expected {} type arguments, found {}",
                    params.len(),
                    args.len()
                ),
                span,
            );
        }
        params
            .iter()
            .zip(args)
            .map(|(p, t)| (p.name.clone(), t.clone()))
            .collect()
    }
    fn resolve(&mut self, t: &TypeSpec, span: Span) -> TypeSpec {
        match t {
            TypeSpec::Nullable(inner) => {
                let inner = self.resolve(inner, span);
                if matches!(inner, TypeSpec::Nullable(_)) {
                    inner
                } else {
                    TypeSpec::Nullable(Box::new(inner))
                }
            }
            TypeSpec::Function {
                params,
                return_type,
            } => TypeSpec::Function {
                params: params.iter().map(|t| self.resolve(t, span)).collect(),
                return_type: Box::new(self.resolve(return_type, span)),
            },
            TypeSpec::Union(members) => {
                let mut flat = vec![];
                for t in members {
                    let t = self.resolve(t, span);
                    if let TypeSpec::Class(n) = &t {
                        if let Some(ms) = self.unions.get(n) {
                            flat.extend(ms.clone());
                            continue;
                        }
                    }
                    flat.push(t);
                }
                flat.sort_by_key(spelling);
                flat.dedup();
                if flat.len() == 1 {
                    return flat.remove(0);
                }
                let name = key("Union", &flat);
                if !self.unions.contains_key(&name) {
                    self.unions.insert(name.clone(), flat.clone());
                    let fields = flat
                        .iter()
                        .enumerate()
                        .map(|(i, t)| format!("({})? slot{i}", spelling(t)))
                        .collect::<Vec<_>>()
                        .join(",");
                    let mut source = format!("class {name}(int tag,{fields}) {{\n");
                    for (i, t) in flat.iter().enumerate() {
                        let args = (0..flat.len())
                            .map(|j| {
                                if i == j {
                                    "value".into()
                                } else {
                                    "null".into()
                                }
                            })
                            .collect::<Vec<String>>()
                            .join(",");
                        source += &format!(
                            "static {name} from{i}({} value) {{ return {name}({i},{args}) }}\n",
                            spelling(t)
                        );
                        let unwrap = if matches!(t, TypeSpec::Nullable(_)) {
                            format!("this.slot{i}()")
                        } else {
                            format!("this.slot{i}() else this.fail{i}()")
                        };
                        source+=&format!("{} as{i}() {{ if this.tag() != {i} {{ throw 'Union member does not match' }} return {unwrap} }}\n{} fail{i}() {{ throw 'Union member does not match' }}\n",spelling(t),spelling(t));
                    }
                    source+=&format!("bool equals({name} other) {{ if this.tag() != other.tag() {{ return false }}\n");
                    for (i, _) in flat.iter().enumerate() {
                        source += &format!(
                            "if this.tag() == {i} {{ return this.as{i}() == other.as{i}() }}\n"
                        );
                    }
                    source += "return false\n} }";
                    for mut item in parse_generated(&source) {
                        if let Item::Class(c) = &mut item {
                            c.is_union = true;
                            self.concrete.insert(c.name.clone(), c.clone());
                        }
                        self.pending.push_back((item, Env::new()));
                    }
                }
                TypeSpec::Class(name)
            }
            _ if application(t).is_some() => {
                let (base, raw) = application(t).unwrap();
                let args: Vec<_> = raw.iter().map(|t| self.resolve(t, span)).collect();
                let base = if base == "Map" { "UbeTypedMap" } else { base };
                if let Some(mut c) = self.templates.get(base).cloned() {
                    let bindings = self.bindings(&c.type_params, &args, span);
                    if c.type_params.len() != args.len() {
                        return TypeSpec::Class(base.into());
                    }
                    let name = key(base, &args);
                    if self.instantiated.insert(name.clone()) {
                        self.origins
                            .insert(name.clone(), (base.into(), bindings.clone()));
                        let parameters = c.type_params.clone();
                        c.name = name.clone();
                        c.methods.extend(self.bound_checks(&parameters, &bindings));
                        self.concrete.insert(name.clone(), c.clone());
                        self.pending.push_back((Item::Class(c), bindings));
                    }
                    TypeSpec::Class(name)
                } else if let Some(mut tr) = self.traits.get(base).cloned() {
                    let bindings = self.bindings(&tr.type_params, &args, span);
                    let name = key(base, &args);
                    if self.instantiated.insert(name.clone()) {
                        tr.name = name.clone();
                        tr.type_params.clear();
                        self.origins
                            .insert(name.clone(), (base.into(), bindings.clone()));
                        self.traits.insert(name.clone(), tr.clone());
                        self.pending.push_back((Item::Trait(tr), bindings));
                    }
                    TypeSpec::Class(name)
                } else if [
                    "List",
                    "Stream",
                    "Async",
                    "Channel",
                    "OrmModel",
                    "OrmQuery",
                    "OrmProjection",
                    "OrmRows",
                ]
                .contains(&base)
                {
                    if args.len() != 1 {
                        self.error(format!("{base} expects 1 type argument"), span);
                    }
                    applied(base, args)
                } else {
                    self.error(format!("'{base}' is not a generic type"), span);
                    TypeSpec::Class(base.into())
                }
            }
            TypeSpec::Class(n)
                if self
                    .templates
                    .get(n)
                    .is_some_and(|c| !c.type_params.is_empty())
                    || self
                        .traits
                        .get(n)
                        .is_some_and(|c| !c.type_params.is_empty()) =>
            {
                self.error(format!("Generic type '{n}' requires type arguments"), span);
                t.clone()
            }
            _ => t.clone(),
        }
    }
    fn bound_checks(&self, params: &[TypeParameter], bindings: &Env) -> Vec<MethodDecl> {
        let mut checks = vec![];
        for (i, p) in params.iter().enumerate() {
            let Some(actual) = bindings.get(&p.name) else {
                continue;
            };
            for (j, bound) in p.bounds.iter().enumerate() {
                checks.push(MethodDecl {
                    type_params: vec![],
                    name: format!("UbeBound{i}_{j}"),
                    params: vec![Param {
                        name: "value".into(),
                        type_spec: actual.clone(),
                        span: Span::default(),
                    }],
                    return_type: None,
                    is_static: true,
                    is_async: false,
                    span: Span::default(),
                    body: vec![Stmt {
                        span: Span::default(),
                        kind: StmtKind::LocalDecl {
                            name: "requirement".into(),
                            is_mutable: false,
                            type_annotation: Some(substitute(bound, bindings)),
                            initializer: Some(id("value")),
                        },
                    }],
                });
            }
        }
        checks
    }
    fn materialize_cases(&mut self, c: &mut ClassDecl) {
        if c.cases.is_empty() {
            return;
        }
        let cases = std::mem::take(&mut c.cases);
        if !c.primary_params.is_empty() {
            self.error("A union class cannot also declare primary fields", c.span);
            return;
        }
        let mut fields = vec![ConstructorParam {
            name: "tag".into(),
            type_spec: TypeSpec::Int,
            is_var: false,
            default_value: None,
            span: c.span,
        }];
        for case in &cases {
            for p in &case.params {
                fields.push(ConstructorParam {
                    name: format!("{}{}", case.name, p.name),
                    type_spec: if matches!(p.type_spec, TypeSpec::Nullable(_)) {
                        p.type_spec.clone()
                    } else {
                        TypeSpec::Nullable(Box::new(p.type_spec.clone()))
                    },
                    is_var: false,
                    default_value: None,
                    span: p.span,
                });
            }
        }
        c.primary_params = fields;
        for (i, case) in cases.iter().enumerate() {
            let mut args = vec![expr(ExprKind::IntLiteral(i as i64))];
            for other in &cases {
                for p in &other.params {
                    args.push(if other.name == case.name {
                        id(&p.name)
                    } else {
                        expr(ExprKind::NullLiteral)
                    });
                }
            }
            c.methods.push(MethodDecl {
                type_params: vec![],
                name: case.name.clone(),
                params: case.params.clone(),
                return_type: Some(TypeSpec::Class(c.name.clone())),
                body: vec![Stmt {
                    span: c.span,
                    kind: StmtKind::Return(Some(expr(ExprKind::ClassInstantiation {
                        class_name: c.name.clone(),
                        generic_type: None,
                        args,
                    }))),
                }],
                is_async: false,
                is_static: true,
                span: c.span,
            });
            let mut source = format!(
                "class Generated {{ bool is{}() {{ return this.tag() == {i} }}\n",
                case.name
            );
            for p in &case.params {
                let field = format!("this.{}{}()", case.name, p.name);
                let unwrap = if matches!(p.type_spec, TypeSpec::Nullable(_)) {
                    field
                } else {
                    format!("{field} else this.fail{}{}()", case.name, p.name)
                };
                source+=&format!("{} unwrap{}{}() {{ if this.tag() != {i} {{ throw 'Union case does not match' }} return {unwrap} }}\n{} fail{}{}() {{ throw 'Union case does not match' }}\n",spelling(&p.type_spec),case.name,p.name,spelling(&p.type_spec),case.name,p.name);
            }
            source += "}";
            if let Item::Class(mut g) = parse_generated(&source).remove(0) {
                if case.params.len() == 1 {
                    for m in &mut g.methods {
                        if m.name == format!("unwrap{}{}", case.name, case.params[0].name) {
                            m.name = format!("unwrap{}", case.name);
                        }
                    }
                }
                c.methods.extend(g.methods);
            }
        }
    }
    fn method(&mut self, m: &mut MethodDecl, bindings: &Env, owner: &str) {
        let mut env = Env::new();
        env.insert("this".into(), TypeSpec::Class(owner.into()));
        for p in &mut m.params {
            p.type_spec = self.resolve(&substitute(&p.type_spec, bindings), p.span);
            env.insert(p.name.clone(), p.type_spec.clone());
        }
        m.return_type = m
            .return_type
            .as_ref()
            .map(|t| self.resolve(&substitute(t, bindings), m.span));
        self.statements(
            &mut m.body,
            &mut env,
            bindings,
            owner,
            m.return_type.as_ref(),
        );
    }
    fn signature(&mut self, owner: &str, name: &str) -> Option<MethodDecl> {
        if !self.concrete.contains_key(owner) && !self.templates.contains_key(owner) {
            if let Some(tr) = self.traits.get(owner) {
                let mut m = tr.methods.iter().find(|m| m.name == name)?.clone();
                let b = self
                    .origins
                    .get(owner)
                    .map(|(_, b)| b.clone())
                    .unwrap_or_default();
                for p in &mut m.params {
                    p.type_spec = substitute(&p.type_spec, &b);
                }
                m.return_type = m.return_type.map(|t| substitute(&t, &b));
                return Some(m);
            }
            let (params, result) = self.builtins.get(owner)?.methods.get(name)?;
            return Some(MethodDecl {
                type_params: vec![],
                name: name.into(),
                params: params
                    .iter()
                    .enumerate()
                    .map(|(i, t)| Param {
                        name: format!("arg{i}"),
                        type_spec: from_type(t),
                        span: Span::default(),
                    })
                    .collect(),
                return_type: if result == &crate::types::Type::Void {
                    None
                } else {
                    Some(from_type(result))
                },
                body: vec![],
                is_async: false,
                is_static: false,
                span: Span::default(),
            });
        }
        let mut c = self
            .concrete
            .get(owner)
            .or_else(|| self.templates.get(owner))?
            .clone();
        let mut binding = self
            .origins
            .get(owner)
            .map(|(_, b)| b.clone())
            .unwrap_or_default();
        if !c.cases.is_empty() {
            for case in &mut c.cases {
                for p in &mut case.params {
                    p.type_spec = self.resolve(&substitute(&p.type_spec, &binding), p.span);
                }
            }
            self.materialize_cases(&mut c);
        }
        if let Some(mut m) = c.methods.into_iter().find(|m| m.name == name) {
            for parameter in &m.type_params {
                binding.remove(&parameter.name);
            }
            for p in &mut m.params {
                p.type_spec = substitute(&p.type_spec, &binding);
            }
            m.return_type = m.return_type.map(|t| substitute(&t, &binding));
            Some(m)
        } else if let Some(p) = c.primary_params.iter().find(|p| p.name == name) {
            Some(MethodDecl {
                type_params: vec![],
                name: name.into(),
                params: vec![],
                return_type: Some(substitute(&p.type_spec, &binding)),
                body: vec![],
                is_async: false,
                is_static: false,
                span: p.span,
            })
        } else {
            None
        }
    }
    fn infer(
        &mut self,
        pattern: &TypeSpec,
        actual: &TypeSpec,
        params: &[TypeParameter],
        bindings: &mut Env,
        span: Span,
    ) {
        if let TypeSpec::Class(n) = pattern {
            if params.iter().any(|p| p.name == *n) {
                if let Some(previous) = bindings.get(n) {
                    if previous != actual {
                        self.error(
                            format!(
                                "Conflicting inferred types for '{n}': {} and {}",
                                spelling(previous),
                                spelling(actual)
                            ),
                            span,
                        );
                    }
                } else {
                    bindings.insert(n.clone(), actual.clone());
                }
                return;
            }
        }
        let actual = if let TypeSpec::Class(n) = actual {
            if let Some((base, b)) = self.origins.get(n) {
                if let Some(c) = self.templates.get(base) {
                    applied(
                        base,
                        c.type_params
                            .iter()
                            .filter_map(|p| b.get(&p.name).cloned())
                            .collect(),
                    )
                } else {
                    actual.clone()
                }
            } else {
                actual.clone()
            }
        } else {
            actual.clone()
        };
        match (pattern, &actual) {
            (TypeSpec::Nullable(p), TypeSpec::Nullable(a)) => {
                self.infer(p, a, params, bindings, span)
            }
            (TypeSpec::Nullable(p), a) => self.infer(p, a, params, bindings, span),
            (
                TypeSpec::Function {
                    params: p,
                    return_type: pr,
                },
                TypeSpec::Function {
                    params: a,
                    return_type: ar,
                },
            ) => {
                for (p, a) in p.iter().zip(a) {
                    self.infer(p, a, params, bindings, span);
                }
                self.infer(pr, ar, params, bindings, span);
            }
            _ => {
                if let (Some((pn, ps)), Some((an, as_))) =
                    (application(pattern), application(&actual))
                {
                    if pn == an || (pn == "Map" && an == "UbeTypedMap") {
                        for (p, a) in ps.iter().zip(as_) {
                            self.infer(p, &a, params, bindings, span);
                        }
                    }
                }
            }
        }
    }
    fn statements(
        &mut self,
        body: &mut [Stmt],
        env: &mut Env,
        bindings: &Env,
        owner: &str,
        result: Option<&TypeSpec>,
    ) {
        for stmt in body {
            match &mut stmt.kind {
                StmtKind::LocalDecl {
                    name,
                    type_annotation,
                    initializer,
                    ..
                } => {
                    *type_annotation = type_annotation
                        .as_ref()
                        .map(|t| self.resolve(&substitute(t, bindings), stmt.span));
                    let actual = initializer.as_mut().and_then(|v| {
                        if let (ExprKind::ListLiteral(values), Some(("List", types))) =
                            (&mut v.kind, type_annotation.as_ref().and_then(application))
                        {
                            for value in values {
                                self.expression(value, env, bindings, owner, types.first());
                            }
                            type_annotation.clone()
                        } else {
                            self.expression(v, env, bindings, owner, type_annotation.as_ref())
                        }
                    });
                    if let Some(ty) = type_annotation.clone().or(actual) {
                        env.insert(name.clone(), ty);
                    }
                }
                StmtKind::Expr(e) | StmtKind::Throw(e) => {
                    self.expression(e, env, bindings, owner, None);
                }
                StmtKind::Return(e) => {
                    if let Some(e) = e {
                        self.expression(e, env, bindings, owner, result);
                    }
                }
                StmtKind::Yield(e) => {
                    self.expression(e, env, bindings, owner, result);
                }
                StmtKind::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    // Capture a checked member once at branch entry. Reassignment of the
                    // outer union cannot invalidate the narrowed local's representation.
                    let narrowing = if let ExprKind::IsCheck {
                        expr: value,
                        target_type,
                        is_negated,
                    } = &condition.kind
                    {
                        if let ExprKind::Identifier(n) = &value.kind {
                            let target =
                                self.resolve(&substitute(target_type, bindings), condition.span);
                            if let Some(TypeSpec::Class(union)) = env.get(n) {
                                if let Some(ms) = self.unions.get(union) {
                                    ms.iter()
                                        .position(|m| m == &target)
                                        .map(|i| (n.clone(), target, Some(i), *is_negated))
                                } else {
                                    Some((n.clone(), target, None, *is_negated))
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    self.expression(condition, env, bindings, owner, None);
                    if let Some((n, t, i, negated)) = narrowing {
                        let narrowed = Stmt {
                            span: stmt.span,
                            kind: StmtKind::LocalDecl {
                                name: n.clone(),
                                is_mutable: false,
                                type_annotation: Some(t.clone()),
                                initializer: Some(if let Some(i) = i {
                                    invoke(id(&n), &format!("as{i}"), vec![])
                                } else {
                                    expr(ExprKind::Narrow {
                                        value: Box::new(id(&n)),
                                        target: t.clone(),
                                    })
                                }),
                            },
                        };
                        if !negated {
                            then_branch.insert(0, narrowed);
                        } else if let Some(eb) = else_branch {
                            eb.insert(0, narrowed);
                        }
                    }
                    self.statements(then_branch, &mut env.clone(), bindings, owner, result);
                    if let Some(eb) = else_branch {
                        self.statements(eb, &mut env.clone(), bindings, owner, result);
                    }
                }
                StmtKind::For {
                    loop_var,
                    index_var,
                    condition_or_iterable,
                    body,
                    else_branch,
                } => {
                    let t = condition_or_iterable
                        .as_mut()
                        .and_then(|e| self.expression(e, env, bindings, owner, None));
                    let mut scope = env.clone();
                    if let Some(n) = loop_var {
                        let el = t
                            .as_ref()
                            .and_then(application)
                            .and_then(|(n, mut ts)| {
                                if n == "List" {
                                    Some(ts.remove(0))
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(TypeSpec::Int);
                        scope.insert(n.clone(), el);
                    }
                    if let Some(n) = index_var {
                        scope.insert(n.clone(), TypeSpec::Int);
                    }
                    self.statements(body, &mut scope, bindings, owner, result);
                    if let Some(eb) = else_branch {
                        self.statements(eb, &mut env.clone(), bindings, owner, result);
                    }
                }
                StmtKind::Try {
                    body,
                    catches,
                    finally_body,
                } => {
                    self.statements(body, &mut env.clone(), bindings, owner, result);
                    for c in catches {
                        let mut scope = env.clone();
                        scope.insert(c.variable.clone(), TypeSpec::Class(c.type_name.clone()));
                        if let Some(e) = &mut c.condition {
                            self.expression(e, &mut scope, bindings, owner, None);
                        }
                        self.statements(&mut c.body, &mut scope, bindings, owner, result);
                    }
                    if let Some(b) = finally_body {
                        self.statements(b, &mut env.clone(), bindings, owner, result);
                    }
                }
                _ => {}
            }
        }
    }
    fn expression(
        &mut self,
        e: &mut Expr,
        env: &mut Env,
        bindings: &Env,
        owner: &str,
        expected: Option<&TypeSpec>,
    ) -> Option<TypeSpec> {
        let span = e.span;
        if self.instantiated.len() > 512 {
            self.error("Generic specialization limit exceeded", span);
            return None;
        }
        if let ExprKind::Assign { target, value } = &mut e.kind {
            if let ExprKind::IndexAccess { target: map, index } = &mut target.kind {
                let ty = self.expression(map, env, bindings, owner, None);
                if let Some(TypeSpec::Class(n)) = ty {
                    if self
                        .origins
                        .get(&n)
                        .is_some_and(|(base, _)| base == "UbeTypedMap")
                    {
                        *e = invoke(*map.clone(), "put", vec![*index.clone(), *value.clone()]);
                        return self.expression(e, env, bindings, owner, expected);
                    }
                }
            }
        }
        // A generic callee's application is attached to the call, not erased from its receiver.
        if let ExprKind::Call { callee, args } = &mut e.kind {
            let (mut target, explicit) = match &callee.kind {
                ExprKind::TypeApply { target, types } => (
                    *target.clone(),
                    Some(
                        types
                            .iter()
                            .map(|t| self.resolve(&substitute(t, bindings), span))
                            .collect::<Vec<_>>(),
                    ),
                ),
                _ => (*callee.clone(), None),
            };
            if let ExprKind::Identifier(n) = &target.kind {
                if self.templates.contains_key(n)
                    || ["List", "Map", "Channel"].contains(&n.as_str())
                {
                    let name = n.clone();
                    let raw = explicit.map(|ts| applied(&name, ts));
                    e.kind = ExprKind::ClassInstantiation {
                        class_name: name,
                        generic_type: raw,
                        args: args.clone(),
                    };
                    return self.expression(e, env, bindings, owner, expected);
                }
            }
            if let ExprKind::MemberAccess { object, member, .. } = &mut target.kind {
                if let ExprKind::Identifier(base) = &object.kind {
                    if let Some(c) = self
                        .templates
                        .get(base)
                        .filter(|c| !c.type_params.is_empty())
                        .cloned()
                    {
                        let mut b = Env::new();
                        if let Some(TypeSpec::Class(n)) = expected {
                            if let Some((source, bs)) = self.origins.get(n) {
                                if source == base {
                                    b = bs.clone();
                                }
                            }
                        }
                        let ps = c
                            .cases
                            .iter()
                            .find(|c| c.name == *member)
                            .map(|c| c.params.clone())
                            .or_else(|| {
                                c.methods
                                    .iter()
                                    .find(|m| m.name == *member)
                                    .map(|m| m.params.clone())
                            });
                        if let Some(ps) = ps {
                            for (a, p) in args.iter_mut().zip(ps) {
                                if let Some(t) = self.expression(a, env, bindings, owner, None) {
                                    self.infer(&p.type_spec, &t, &c.type_params, &mut b, span);
                                }
                            }
                        }
                        if c.type_params.iter().all(|p| b.contains_key(&p.name)) {
                            let ty = self.resolve(
                                &applied(
                                    base,
                                    c.type_params.iter().map(|p| b[&p.name].clone()).collect(),
                                ),
                                span,
                            );
                            if let TypeSpec::Class(n) = ty {
                                *object = Box::new(id(&n));
                            }
                        } else {
                            self.error(format!("Cannot infer type arguments for '{base}'; supply them explicitly"),span);
                        }
                    }
                }
                let receiver = self.expression(object, env, bindings, owner, None);
                if matches!(&receiver,Some(TypeSpec::Class(n)) if n=="Async")
                    && member == "start"
                    && args.len() == 1
                {
                    let callback = self.expression(&mut args[0], env, bindings, owner, None);
                    if let Some(TypeSpec::Function { return_type, .. }) = callback {
                        *callee = Box::new(target);
                        return self.coerce(
                            e,
                            Some(applied("Async", vec![*return_type])),
                            expected,
                        );
                    }
                }
                if let Some((base, ts)) = receiver.as_ref().and_then(application) {
                    if (base == "Async" || base == "Channel") && ts.len() == 1 {
                        for a in args.iter_mut() {
                            self.expression(
                                a,
                                env,
                                bindings,
                                owner,
                                if base == "Channel" && member == "send" {
                                    ts.first()
                                } else {
                                    None
                                },
                            );
                        }
                        let ty = match (base, member.as_str()) {
                            ("Async", "await") => Some(ts[0].clone()),
                            ("Async", "timeout") => receiver.clone(),
                            ("Async", "cancel" | "isCompleted" | "isCancelled") => {
                                Some(TypeSpec::Bool)
                            }
                            ("Channel", "send") => Some(applied("Async", vec![TypeSpec::Bool])),
                            ("Channel", "receive") => Some(applied("Async", ts.clone())),
                            _ => None,
                        };
                        *callee = Box::new(target);
                        return self.coerce(e, ty, expected);
                    }
                }
                if matches!(&receiver,Some(TypeSpec::Class(n)) if n=="Stream")
                    && (member == "of" || member == "range")
                {
                    let mut types = vec![];
                    for a in args.iter_mut() {
                        types.push(self.expression(a, env, bindings, owner, None));
                    }
                    let element = if member == "range" {
                        Some(TypeSpec::Int)
                    } else {
                        types.first().cloned().unwrap_or(Some(TypeSpec::Int))
                    };
                    *callee = Box::new(target);
                    return self.coerce(e, element.map(|t| applied("Stream", vec![t])), expected);
                }
                let class = match &receiver {
                    Some(TypeSpec::Class(n)) => Some(n.clone()),
                    _ => None,
                };
                let mut sig = class.as_ref().and_then(|n| self.signature(n, member));
                if let Some(m) = sig.as_ref().filter(|m| !m.type_params.is_empty()).cloned() {
                    let infer_arguments = explicit.is_none();
                    let mut b = if let Some(ts) = explicit {
                        self.bindings(&m.type_params, &ts, span)
                    } else {
                        Env::new()
                    };
                    for (a, p) in args.iter_mut().zip(&m.params) {
                        if let Some(t) = self.expression(a, env, bindings, owner, None) {
                            if infer_arguments {
                                self.infer(&p.type_spec, &t, &m.type_params, &mut b, span);
                            }
                        }
                    }
                    if m.type_params.iter().any(|p| !b.contains_key(&p.name)) {
                        if let (Some(r), Some(t)) = (&m.return_type, expected) {
                            self.infer(r, t, &m.type_params, &mut b, span);
                        }
                    }
                    if m.type_params.iter().any(|p| !b.contains_key(&p.name)) {
                        self.error(format!("Cannot infer all type arguments for '{member}'; supply them explicitly"),span);
                        return None;
                    }
                    let ts = m
                        .type_params
                        .iter()
                        .map(|p| b[&p.name].clone())
                        .collect::<Vec<_>>();
                    let specialized = key(member, &ts);
                    let class = class.unwrap();
                    let identity = format!("{class}.{specialized}");
                    if self.instantiated.insert(identity) {
                        if self.method_depth >= 32 {
                            self.error("Expanding recursive generic method specialization exceeds the depth limit",span);
                            return None;
                        }
                        self.method_depth += 1;
                        let mut method = m.clone();
                        method.type_params.clear();
                        method.name = specialized.clone();
                        // Include class substitutions in method bodies as well as signatures.
                        let mut substitutions = self
                            .origins
                            .get(&class)
                            .map(|(_, b)| b.clone())
                            .unwrap_or_default();
                        substitutions.extend(b.clone());
                        for mut check in self.bound_checks(&m.type_params, &b) {
                            check.name = format!("{}_{}", specialized, check.name);
                            self.method(&mut check, &substitutions, &class);
                            self.methods.push((class.clone(), check));
                        }
                        self.method(&mut method, &substitutions, &class);
                        self.method_depth -= 1;
                        self.methods.push((class.clone(), method));
                    }
                    let mut instantiated = m;
                    instantiated.type_params.clear();
                    for p in &mut instantiated.params {
                        p.type_spec = self.resolve(&substitute(&p.type_spec, &b), span);
                    }
                    instantiated.return_type = instantiated
                        .return_type
                        .map(|t| self.resolve(&substitute(&t, &b), span));
                    sig = Some(instantiated);
                    *member = specialized;
                } else if explicit.is_some() {
                    self.error(format!("Method '{member}' is not generic"), span);
                }
                if let Some(m) = sig {
                    for (i, a) in args.iter_mut().enumerate() {
                        let p = m.params.get(i).map(|p| self.resolve(&p.type_spec, span));
                        self.expression(a, env, bindings, owner, p.as_ref());
                    }
                    *callee = Box::new(target);
                    let mut t = m.return_type.map(|t| self.resolve(&t, span));
                    if m.is_async {
                        t = Some(applied("Async", vec![t.unwrap_or(TypeSpec::Int)]));
                    }
                    return self.coerce(e, t, expected);
                }
                // Built-in typed containers retain their element types across calls.
                if let Some((base, ts)) = receiver.as_ref().and_then(application) {
                    if (base == "List" || base == "Stream") && !ts.is_empty() {
                        let mut argument_types = vec![];
                        for (i, a) in args.iter_mut().enumerate() {
                            argument_types.push(self.expression(
                                a,
                                env,
                                bindings,
                                owner,
                                if member == "add" && i == 0 {
                                    ts.first()
                                } else {
                                    None
                                },
                            ));
                        }
                        let t = match member.as_str() {
                            "get" => Some(ts[0].clone()),
                            "length" => Some(TypeSpec::Int),
                            "stream" => Some(applied("Stream", ts.clone())),
                            "toList" | "collect" => Some(applied("List", ts.clone())),
                            "findFirst" => Some(TypeSpec::Nullable(Box::new(ts[0].clone()))),
                            "count" => Some(TypeSpec::Long),
                            "sum" => Some(if ts[0] == TypeSpec::Decimal {
                                TypeSpec::Decimal
                            } else {
                                TypeSpec::Long
                            }),
                            "reduce" => Some(ts[0].clone()),
                            "some" | "every" | "none" | "anyMatch" | "allMatch" | "noneMatch" => {
                                Some(TypeSpec::Bool)
                            }
                            "map" => argument_types.first().and_then(|t| {
                                if let Some(TypeSpec::Function { return_type, .. }) = t {
                                    Some(applied("Stream", vec![*return_type.clone()]))
                                } else {
                                    None
                                }
                            }),
                            "filter" | "peek" | "limit" | "skip" | "sort" | "unique" => {
                                receiver.clone()
                            }
                            _ => None,
                        };
                        *callee = Box::new(target);
                        return self.coerce(e, t, expected);
                    }
                }
            } else {
                if let Some(TypeSpec::Function {
                    params,
                    return_type,
                }) = self.expression(&mut target, env, bindings, owner, None)
                {
                    for (i, a) in args.iter_mut().enumerate() {
                        self.expression(a, env, bindings, owner, params.get(i));
                    }
                    *callee = Box::new(target);
                    return self.coerce(e, Some(*return_type), expected);
                }
            }
            for a in args {
                self.expression(a, env, bindings, owner, None);
            }
            *callee = Box::new(target);
            return None;
        }
        let actual = match &mut e.kind {
            ExprKind::Narrow { value, target } => {
                self.expression(value, env, bindings, owner, None);
                *target = self.resolve(&substitute(target, bindings), span);
                Some(target.clone())
            }
            ExprKind::TypeApply { target, types } => {
                let ts = types
                    .iter()
                    .map(|t| substitute(t, bindings))
                    .collect::<Vec<_>>();
                if let ExprKind::Identifier(n) = &target.kind {
                    let t = self.resolve(&applied(n, ts), span);
                    if let TypeSpec::Class(n) = &t {
                        e.kind = ExprKind::Identifier(n.clone());
                    }
                    Some(t)
                } else {
                    self.error("Type arguments must apply to a type or method call", span);
                    None
                }
            }
            ExprKind::ClassInstantiation {
                class_name,
                generic_type,
                args,
            } => {
                let mut name = if let Some(TypeSpec::Class(n)) = bindings.get(class_name) {
                    n.clone()
                } else {
                    class_name.clone()
                };
                if span != Span::default()
                    && self
                        .templates
                        .get(&name)
                        .is_some_and(|c| !c.cases.is_empty())
                {
                    self.error(
                        "Union values must be constructed through a declared case",
                        span,
                    );
                }
                let raw = generic_type.take().map(|t| {
                    if application(&t).is_some_and(|(n, _)| n == name) {
                        t
                    } else {
                        applied(&name, vec![t])
                    }
                });
                let mut resolved = raw
                    .as_ref()
                    .map(|t| self.resolve(&substitute(t, bindings), span));
                if resolved.is_none() {
                    if let Some(template) = self
                        .templates
                        .get(&name)
                        .filter(|c| !c.type_params.is_empty())
                        .cloned()
                    {
                        let mut b = Env::new();
                        if let Some(TypeSpec::Class(n)) = expected {
                            if let Some((base, bs)) = self.origins.get(n) {
                                if base == &name {
                                    b = bs.clone();
                                }
                            }
                        }
                        for (a, p) in args.iter_mut().zip(&template.primary_params) {
                            if let Some(t) = self.expression(a, env, bindings, owner, None) {
                                self.infer(&p.type_spec, &t, &template.type_params, &mut b, span);
                            }
                        }
                        if template.type_params.iter().all(|p| b.contains_key(&p.name)) {
                            resolved = Some(
                                self.resolve(
                                    &applied(
                                        &name,
                                        template
                                            .type_params
                                            .iter()
                                            .map(|p| b[&p.name].clone())
                                            .collect(),
                                    ),
                                    span,
                                ),
                            );
                        } else {
                            self.error(format!("Cannot infer type arguments for '{name}'"), span);
                        }
                    }
                }
                if let Some(TypeSpec::Class(n)) = &resolved {
                    name = n.clone();
                }
                if let Some(TypeSpec::Generic(n, t)) = &resolved {
                    if n == "List" || n == "Channel" {
                        *class_name = n.clone();
                        *generic_type = Some(*t.clone());
                        for a in args {
                            self.expression(a, env, bindings, owner, None);
                        }
                        return resolved;
                    }
                }
                if let Some(c) = self
                    .concrete
                    .get(&name)
                    .or_else(|| self.templates.get(&name))
                    .cloned()
                {
                    let b = self
                        .origins
                        .get(&name)
                        .map(|(_, b)| b.clone())
                        .unwrap_or_default();
                    for (i, a) in args.iter_mut().enumerate() {
                        let expected = c
                            .primary_params
                            .get(i)
                            .map(|p| self.resolve(&substitute(&p.type_spec, &b), span));
                        self.expression(a, env, bindings, owner, expected.as_ref());
                    }
                } else {
                    for a in args {
                        self.expression(a, env, bindings, owner, None);
                    }
                }
                *class_name = name.clone();
                Some(resolved.unwrap_or(TypeSpec::Class(name)))
            }
            ExprKind::IntLiteral(_) => Some(TypeSpec::Int),
            ExprKind::LongLiteral(_) => Some(TypeSpec::Long),
            ExprKind::DecimalLiteral(_) => Some(TypeSpec::Decimal),
            ExprKind::BoolLiteral(_) => Some(TypeSpec::Bool),
            ExprKind::StringLiteral(_) | ExprKind::TextBlock(_) => Some(TypeSpec::String),
            ExprKind::Identifier(n) => {
                if !env.contains_key(n) {
                    if let Some(TypeSpec::Class(concrete)) = bindings.get(n) {
                        *n = concrete.clone();
                    }
                }
                env.get(n).cloned().or_else(|| {
                    Some(TypeSpec::Class(if n == "static" {
                        owner.into()
                    } else {
                        n.clone()
                    }))
                })
            }
            ExprKind::This => Some(TypeSpec::Class(owner.into())),
            ExprKind::NullLiteral => {
                Some(TypeSpec::Nullable(Box::new(TypeSpec::Class("void".into()))))
            }
            ExprKind::ListLiteral(values) => {
                let element = expected
                    .and_then(application)
                    .filter(|(n, _)| *n == "List")
                    .map(|(_, ts)| ts[0].clone());
                for v in values.iter_mut() {
                    self.expression(v, env, bindings, owner, element.as_ref());
                }
                if let Some(t) = element {
                    let ty = applied("List", vec![t.clone()]);
                    let values = std::mem::take(values);
                    *e = self.literal_helper(
                        owner,
                        &ty,
                        values.iter().map(|_| t.clone()).collect(),
                        values,
                        false,
                    );
                    Some(ty)
                } else {
                    Some(TypeSpec::Class("List".into()))
                }
            }
            ExprKind::MapLiteral(pairs) => {
                let map = expected
                    .and_then(|t| {
                        if let TypeSpec::Class(n) = t {
                            self.origins.get(n)
                        } else {
                            None
                        }
                    })
                    .filter(|(n, _)| n == "UbeTypedMap")
                    .cloned();
                if let Some((_, b)) = map {
                    for (k, v) in pairs.iter_mut() {
                        self.expression(k, env, bindings, owner, b.get("K"));
                        self.expression(v, env, bindings, owner, b.get("V"));
                    }
                    let args = std::mem::take(pairs)
                        .into_iter()
                        .flat_map(|(k, v)| [k, v])
                        .collect::<Vec<_>>();
                    let params = (0..args.len())
                        .map(|i| b[if i % 2 == 0 { "K" } else { "V" }].clone())
                        .collect();
                    let ty = expected.unwrap().clone();
                    *e = self.literal_helper(owner, &ty, params, args, true);
                    Some(ty)
                } else {
                    for (k, v) in pairs {
                        self.expression(k, env, bindings, owner, None);
                        self.expression(v, env, bindings, owner, None);
                    }
                    Some(TypeSpec::Class("Map".into()))
                }
            }
            ExprKind::MemberAccess { object, member, .. } => {
                let t = self.expression(object, env, bindings, owner, None);
                if let Some(TypeSpec::Class(n)) = t {
                    self.signature(&n, member)
                        .and_then(|m| m.return_type)
                        .map(|t| self.resolve(&t, span))
                } else {
                    None
                }
            }
            ExprKind::IndexAccess { target, index } => {
                let t = self.expression(target, env, bindings, owner, None);
                self.expression(index, env, bindings, owner, None);
                if let Some(TypeSpec::Class(n)) = &t {
                    if self.origins.get(n).is_some_and(|(n, _)| n == "UbeTypedMap") {
                        *e = invoke(*target.clone(), "get", vec![*index.clone()]);
                        return self.expression(e, env, bindings, owner, expected);
                    }
                }
                t.as_ref()
                    .and_then(application)
                    .filter(|(n, _)| *n == "List")
                    .map(|(_, ts)| ts[0].clone())
            }
            ExprKind::Assign { target, value }
            | ExprKind::NullCoalescingAssign { target, value } => {
                let t = self.expression(target, env, bindings, owner, None);
                self.expression(value, env, bindings, owner, t.as_ref());
                t
            }
            ExprKind::Binary { op, left, right } => {
                let lt = self.expression(left, env, bindings, owner, None);
                let rt = self.expression(
                    right,
                    env,
                    bindings,
                    owner,
                    if matches!(op, BinaryOp::NullCoalescing) {
                        expected
                    } else {
                        None
                    },
                );
                if matches!(op, BinaryOp::Equal | BinaryOp::NotEqual) {
                    if let Some(TypeSpec::Class(n)) = &lt {
                        if self.unions.contains_key(n) && lt == rt {
                            let call = invoke(*left.clone(), "equals", vec![*right.clone()]);
                            e.kind = if matches!(op, BinaryOp::NotEqual) {
                                ExprKind::Unary {
                                    op: UnaryOp::Not,
                                    operand: Box::new(call),
                                }
                            } else {
                                call.kind
                            };
                        }
                    }
                    Some(TypeSpec::Bool)
                } else if matches!(
                    op,
                    BinaryOp::Less
                        | BinaryOp::LessEqual
                        | BinaryOp::Greater
                        | BinaryOp::GreaterEqual
                        | BinaryOp::And
                        | BinaryOp::Or
                ) {
                    Some(TypeSpec::Bool)
                } else if matches!(op, BinaryOp::NullCoalescing) {
                    lt.map(|t| if let TypeSpec::Nullable(i) = t { *i } else { t })
                        .or(rt)
                } else {
                    lt.or(rt)
                }
            }
            ExprKind::Unary { operand, op } => {
                let t = self.expression(operand, env, bindings, owner, None);
                if matches!(op, UnaryOp::Not) {
                    Some(TypeSpec::Bool)
                } else {
                    t
                }
            }
            ExprKind::IsCheck {
                expr: value,
                target_type,
                is_negated,
            } => {
                let from = self.expression(value, env, bindings, owner, None);
                *target_type = self.resolve(&substitute(target_type, bindings), span);
                if let Some(TypeSpec::Class(n)) = &from {
                    if let Some(members) = self.unions.get(n) {
                        if let Some(i) = members.iter().position(|t| t == target_type) {
                            e.kind = ExprKind::Binary {
                                op: if *is_negated {
                                    BinaryOp::NotEqual
                                } else {
                                    BinaryOp::Equal
                                },
                                left: Box::new(invoke(*value.clone(), "tag", vec![])),
                                right: Box::new(expr(ExprKind::IntLiteral(i as i64))),
                            };
                        } else {
                            e.kind = ExprKind::BoolLiteral(*is_negated);
                        }
                    }
                }
                Some(TypeSpec::Bool)
            }
            ExprKind::Conditional {
                condition,
                when_true,
                when_false,
                result_type,
            } => {
                *result_type = Default::default();
                self.expression(condition, env, bindings, owner, None);
                let a = self.expression(when_true, env, bindings, owner, expected);
                let b = self.expression(when_false, env, bindings, owner, expected);
                if a == b {
                    a
                } else {
                    expected.cloned()
                }
            }
            ExprKind::Closure { params, expression } => {
                let mut scope = env.clone();
                for p in params.iter_mut() {
                    p.type_spec = self.resolve(&substitute(&p.type_spec, bindings), span);
                    scope.insert(p.name.clone(), p.type_spec.clone());
                }
                let result = match expected {
                    Some(TypeSpec::Function { return_type, .. }) => Some(return_type.as_ref()),
                    _ => None,
                };
                let ret = self.expression(expression, &mut scope, bindings, owner, result)?;
                Some(TypeSpec::Function {
                    params: params.iter().map(|p| p.type_spec.clone()).collect(),
                    return_type: Box::new(ret),
                })
            }
            ExprKind::BlockClosure {
                params,
                body,
                return_type,
            } => {
                *return_type = Default::default();
                let mut scope = env.clone();
                for p in params.iter_mut() {
                    p.type_spec = self.resolve(&substitute(&p.type_spec, bindings), span);
                    scope.insert(p.name.clone(), p.type_spec.clone());
                }
                let result = match expected {
                    Some(TypeSpec::Function { return_type, .. }) => Some(return_type.as_ref()),
                    _ => None,
                };
                self.statements(body, &mut scope, bindings, owner, result);
                let ret = if let Some(t) = result {
                    Some(t.clone())
                } else {
                    body.iter_mut().find_map(|s| {
                        if let StmtKind::Return(Some(e)) = &mut s.kind {
                            self.expression(e, &mut scope, bindings, owner, None)
                        } else {
                            None
                        }
                    })
                };
                ret.map(|r| TypeSpec::Function {
                    params: params.iter().map(|p| p.type_spec.clone()).collect(),
                    return_type: Box::new(r),
                })
            }
            ExprKind::Lambda { body, .. } => {
                self.statements(body, &mut env.clone(), bindings, owner, None);
                None
            }
            ExprKind::TemplateLiteral(parts) => {
                for p in parts {
                    self.expression(p, env, bindings, owner, None);
                }
                Some(TypeSpec::String)
            }
            ExprKind::HtmlTemplate(parts) => {
                for (p, _) in parts {
                    self.expression(p, env, bindings, owner, None);
                }
                Some(TypeSpec::Class("Html".into()))
            }
            ExprKind::SqlTemplate(parts) => {
                for (p, _) in parts {
                    self.expression(p, env, bindings, owner, None);
                }
                Some(TypeSpec::Class("SQLStatement".into()))
            }
            ExprKind::RangeIterator { start, end, step } => {
                for e in [start, end, step] {
                    self.expression(e, env, bindings, owner, None);
                }
                None
            }
            ExprKind::AsyncSpawn(e) => self
                .expression(e, env, bindings, owner, None)
                .map(|t| applied("Async", vec![t])),
            ExprKind::Await(e) => self
                .expression(e, env, bindings, owner, None)
                .and_then(|t| application(&t).map(|(_, ts)| ts[0].clone())),
            ExprKind::AnonymousObject { fields } => {
                for (_, v) in fields {
                    self.expression(v, env, bindings, owner, None);
                }
                None
            }
            ExprKind::WithUpdate { base, updates } => {
                let t = self.expression(base, env, bindings, owner, None);
                for (_, v) in updates {
                    self.expression(v, env, bindings, owner, None);
                }
                t
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expression(condition, env, bindings, owner, None);
                self.statements(then_branch, &mut env.clone(), bindings, owner, expected);
                if let Some(b) = else_branch {
                    self.statements(b, &mut env.clone(), bindings, owner, expected);
                }
                expected.cloned()
            }
            ExprKind::For {
                loop_var,
                index_var,
                condition_or_iterable,
                body,
                else_branch,
            } => {
                let t = condition_or_iterable
                    .as_mut()
                    .and_then(|e| self.expression(e, env, bindings, owner, None));
                let mut scope = env.clone();
                if let Some(n) = loop_var {
                    let element = t
                        .as_ref()
                        .and_then(application)
                        .filter(|(n, _)| *n == "List")
                        .map(|(_, ts)| ts[0].clone())
                        .unwrap_or(TypeSpec::Int);
                    scope.insert(n.clone(), element);
                }
                if let Some(n) = index_var {
                    scope.insert(n.clone(), TypeSpec::Int);
                }
                self.statements(body, &mut scope, bindings, owner, None);
                if let Some(b) = else_branch {
                    self.statements(b, &mut env.clone(), bindings, owner, None);
                }
                Some(TypeSpec::Class("List".into()))
            }
            ExprKind::SwitchExpr { .. } => None,
            ExprKind::Call { .. } => unreachable!(),
        };
        self.coerce(e, actual, expected)
    }
    fn coerce(
        &mut self,
        e: &mut Expr,
        actual: Option<TypeSpec>,
        expected: Option<&TypeSpec>,
    ) -> Option<TypeSpec> {
        let target = match expected {
            Some(TypeSpec::Nullable(inner)) => Some(inner.as_ref()),
            other => other,
        };
        if let (Some(actual), Some(TypeSpec::Class(name))) = (&actual, target) {
            if let Some(members) = self.unions.get(name).cloned() {
                let is_null = matches!(actual,TypeSpec::Nullable(inner) if matches!(inner.as_ref(),TypeSpec::Class(n) if n=="void"));
                if let Some(i) = members
                    .iter()
                    .position(|t| t == actual || (is_null && matches!(t, TypeSpec::Nullable(_))))
                {
                    *e = invoke(id(name), &format!("from{i}"), vec![e.clone()]);
                    return target.cloned();
                }
                if let TypeSpec::Class(source) = actual {
                    if source != name {
                        if let Some(from) = self.unions.get(source).cloned() {
                            if from.iter().all(|t| members.contains(t)) {
                                let method_name = key("widen", &[actual.clone()]);
                                if self.instantiated.insert(format!("{name}.{method_name}")) {
                                    let mut source_code=format!("class Generated {{ static {name} {method_name}({source} value) {{\n");
                                    for (i, t) in from.iter().enumerate() {
                                        let j = members.iter().position(|m| m == t).unwrap();
                                        source_code+=&format!("if value.tag() == {i} {{ return static.from{j}(value.as{i}()) }}\n");
                                    }
                                    source_code += "throw 'Invalid union tag'\n} }";
                                    if let Item::Class(c) = parse_generated(&source_code).remove(0)
                                    {
                                        self.methods.push((name.clone(), c.methods[0].clone()));
                                    }
                                }
                                *e = invoke(id(name), &method_name, vec![e.clone()]);
                                return target.cloned();
                            }
                        }
                    }
                }
            }
        }
        actual
    }
    fn literal_helper(
        &mut self,
        owner: &str,
        ty: &TypeSpec,
        types: Vec<TypeSpec>,
        args: Vec<Expr>,
        map: bool,
    ) -> Expr {
        let name = format!("UbeLiteral{}", self.methods.len());
        let params = types
            .into_iter()
            .enumerate()
            .map(|(i, t)| Param {
                name: format!("arg{i}"),
                type_spec: t,
                span: Span::default(),
            })
            .collect::<Vec<_>>();
        let mut body = vec![Stmt {
            span: Span::default(),
            kind: StmtKind::LocalDecl {
                name: "result".into(),
                is_mutable: false,
                type_annotation: Some(ty.clone()),
                initializer: if map {
                    None
                } else {
                    Some(expr(ExprKind::ListLiteral(
                        params.iter().map(|p| id(&p.name)).collect(),
                    )))
                },
            },
        }];
        if map {
            for pair in params.chunks(2) {
                body.push(Stmt {
                    span: Span::default(),
                    kind: StmtKind::Expr(invoke(
                        id("result"),
                        "put",
                        pair.iter().map(|p| id(&p.name)).collect(),
                    )),
                });
            }
        }
        body.push(Stmt {
            span: Span::default(),
            kind: StmtKind::Return(Some(id("result"))),
        });
        self.methods.push((
            owner.into(),
            MethodDecl {
                type_params: vec![],
                name: name.clone(),
                params,
                return_type: Some(ty.clone()),
                body,
                is_async: false,
                is_static: true,
                span: Span::default(),
            },
        ));
        invoke(id("static"), &name, args)
    }
}

fn from_type(ty: &crate::types::Type) -> TypeSpec {
    use crate::types::Type;
    match ty {
        Type::Int => TypeSpec::Int,
        Type::Long => TypeSpec::Long,
        Type::Byte => TypeSpec::Byte,
        Type::Decimal => TypeSpec::Decimal,
        Type::Bool => TypeSpec::Bool,
        Type::String => TypeSpec::String,
        Type::Nullable(t) => TypeSpec::Nullable(Box::new(from_type(t))),
        Type::Async(t) => applied("Async", vec![from_type(t)]),
        Type::Channel(t) => applied("Channel", vec![from_type(t)]),
        Type::Sequence(t) => applied("List", vec![from_type(t)]),
        Type::Stream(t) => applied("Stream", vec![from_type(t)]),
        Type::Function {
            params,
            return_type,
        } => TypeSpec::Function {
            params: params.iter().map(from_type).collect(),
            return_type: Box::new(from_type(return_type)),
        },
        Type::Orm { model, kind, .. } => {
            applied(kind.class(), vec![TypeSpec::Class(model.clone())])
        }
        Type::Class { name } => TypeSpec::Class(name.clone()),
        _ => TypeSpec::Class(ty.to_string()),
    }
}
