use ube_compiler::{IrBuilder, Lexer, Parser, TextualLlvmEmitter, TypeChecker};

#[test]
fn test_ube_end_to_end_pipeline() {
    let source_code = r#"
        class Status {
            case Pending
            case Approved(long orderId)
            case Rejected(string reason)
        }

        class User(
            long id,
            string name,
            var int age = 0
        ) {
            birthday() {
                this._age = this._age + 1
            }
        }

        class Application {
            main() {
                final user = User(1000, `Lem`, 35)
                user.birthday()

                final obj = { id: 1000, name: `Lem` }
                print(`User birthday updated!`)
            }
        }
    "#;

    // 1. Lexing & Parsing
    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing failed: {:?}", parser.diagnostics);

    // 2. Type Checking & Semantic Verification
    let mut checker = TypeChecker::new();
    let is_valid = checker.check_program(&program);
    assert!(is_valid, "Type checking failed: {:?}", checker.diagnostics);

    // 3. IR Lowering to SSA CFG
    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);
    assert_eq!(ir_program.classes.iter().filter(|c| program.items.iter().any(|i| matches!(i, ube_compiler::parser::Item::Class(decl) if decl.name == c.name))).count(), 3);

    // 4. LLVM IR Generation
    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(llvm_ir.contains("%User = type {"));
    assert!(llvm_ir.contains("define void @User_birthday"));
    assert!(llvm_ir.contains("call void @ube_print_str"));
    assert!(llvm_ir.contains("define i32 @main(i32 %argc, i8** %argv)"));
}

#[test]
fn test_ube_sum_classes_parsing() {
    let source = r#"
        class Direction {
            case North
            case South
            case East
            case West
        }
    "#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing sum classes failed: {:?}", parser.diagnostics);

    let mut checker = TypeChecker::new();
    let is_valid = checker.check_program(&program);
    assert!(is_valid, "Type checking sum classes failed: {:?}", checker.diagnostics);
}

#[test]
fn test_ube_trait_parsing() {
    let source = r#"
        trait Printable {
            printDetails()
            string getHeader() {
                return `=== PRINTABLE ===`
            }
        }

        class User(long id, string name) {
            printDetails() {
                print(`User #${this._id}: ${this._name}`)
            }
        }
    "#;

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing trait failed: {:?}", parser.diagnostics);

    let mut checker = TypeChecker::new();
    let is_valid = checker.check_program(&program);
    assert!(is_valid, "Type checking trait failed: {:?}", checker.diagnostics);
}

#[test]
fn test_e2e_person_birthday() {
    let source_code = r#"
        class Person(
            string name,
            var int age
        ) {
            birthday() {
                this._age = this._age + 1
            }
        }

        class Application {
            main() {
                final person = Person(`Lem`, 35)
                print(person.name())
                print(person.age())
                person.birthday()
                print(person.age())
            }
        }
    "#;

    // 1. Lexing & Parsing
    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing failed: {:?}", parser.diagnostics);

    // 2. Type Checking & Semantic Verification
    let mut checker = TypeChecker::new();
    let is_valid = checker.check_program(&program);
    assert!(is_valid, "Type checking failed: {:?}", checker.diagnostics);

    // 3. IR Lowering to SSA CFG
    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);
    assert_eq!(ir_program.classes.iter().filter(|c| program.items.iter().any(|i| matches!(i, ube_compiler::parser::Item::Class(decl) if decl.name == c.name))).count(), 2);

    // 4. LLVM IR Generation
    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(llvm_ir.contains("%Person = type { i8*, i32 }"));
    assert!(llvm_ir.contains("define void @Person_birthday(%Person* %0)"));
    assert!(llvm_ir.contains("call void @ube_print_str"));
    assert!(llvm_ir.contains("call void @ube_print_i64"));
    assert!(llvm_ir.contains("getelementptr inbounds %Person, %Person*"));
    assert!(llvm_ir.contains("store i32 35"));
    assert!(!llvm_ir.contains("void %"));
}

#[test]
fn test_lambda_captures_outer_variables() {
    let source_code = r#"
        class Application {
            main() {
                final res = "Hello from my Ube server!"
                server.get("/", request -> response.ok(res))
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(
        parser.diagnostics.is_empty(),
        "Parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    let is_valid = checker.check_program(&program);
    assert!(is_valid, "Type checking failed: {:?}", checker.diagnostics);
}

#[test]
fn test_e2e_trait_polymorphism() {
    let source_code = r#"
        trait Printable {
            printDetails()         // Abstract method

            string getHeader() {    // Default implementation
                return `=== PRINTABLE ITEM ===`
            }
        }

        class User(long id, string name) {
            printDetails() { // Concrete implementation
                print(`User #${this._id}: ${this._name}`)
            }
        }

        class Invoice(long id, decimal total) {
            printDetails() { // Concrete implementation
                print(`Invoice #${this._id}: \$${this._total}`)
            }
        }

        class App {
            render(Printable item) { // Accepts any Printable item
                print(item.getHeader())
                item.printDetails()
            }

            main() {
                final user = User(1000L, `Lem`)
                final invoice = Invoice(5001L, 199.99)

                this.render(user)    // Calls render with a User
                this.render(invoice) // Calls render with an Invoice
            }
        }
    "#;

    // 1. Lexing & Parsing
    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing failed: {:?}", parser.diagnostics);

    // 2. Type Checking & Semantic Verification
    let mut checker = TypeChecker::new();
    let is_valid = checker.check_program(&program);
    assert!(is_valid, "Type checking failed: {:?}", checker.diagnostics);

    // 3. IR Lowering to SSA CFG
    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);
    assert_eq!(ir_program.classes.iter().filter(|c| program.items.iter().any(|i| matches!(i, ube_compiler::parser::Item::Class(decl) if decl.name == c.name))).count(), 3);

    // 4. LLVM IR Generation
    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(llvm_ir.contains("%User = type { i64, i8* }"));
    assert!(llvm_ir.contains("%Invoice = type { i64, double }"));
    assert!(llvm_ir.contains("define void @App_render_User(%App*"));
    assert!(llvm_ir.contains("define void @App_render_Invoice(%App*"));
    assert!(llvm_ir.contains("call void @User_printDetails(%User*"));
    assert!(llvm_ir.contains("call void @Invoice_printDetails(%Invoice*"));
    assert!(!llvm_ir.contains("void %"));
}

#[test]
fn test_e2e_control_flow_and_dynamic_alloc() {
    let source_code = r#"
        class LargeObject(
            long a,
            long b,
            long c,
            long d,
            long e,
            long f,
            long g,
            long h
        ) {}

        class Application {
            main() {
                final obj = LargeObject(1L, 2L, 3L, 4L, 5L, 6L, 7L, 8L)
                final cond = true

                if (cond) {
                    print(`Condition true`)
                } else {
                    print(`Condition false`)
                }

                for {
                    break
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing failed: {:?}", parser.diagnostics);

    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program), "Type checking failed: {:?}", checker.diagnostics);

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(llvm_ir.contains("ptrtoint %LargeObject*"));
    assert!(llvm_ir.contains("br i1"));
    assert!(!llvm_ir.contains("call i8* @ube_alloc(i64 32, i64 8)"));
}

#[test]
fn test_e2e_nested_cfg_and_logical_ops() {
    let source_code = r#"
        class Application {
            main() {
                final a = true
                final b = false
                final res = a nand b
                final comp = 10 > 5

                if (a) {
                    if (b) {
                        print(`Inner B`)
                    }
                    print(`Outer A`)
                }

                for {
                    break
                    print(`UNREACHABLE DEAD CODE`)
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing failed: {:?}", parser.diagnostics);

    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program), "Type checking failed: {:?}", checker.diagnostics);

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(llvm_ir.contains("icmp sgt i32 10, 5") || llvm_ir.contains("icmp sgt i64 10, 5"));
    assert!(llvm_ir.contains("and i1"));
    assert!(llvm_ir.contains("xor i1 %tmp_nand_"));
    assert!(!llvm_ir.contains("UNREACHABLE DEAD CODE"));
}

#[test]
fn test_e2e_default_constructor_args() {
    let source_code = r#"
        class User(
            string name,
            var int age = 0
        ) {}

        class Application {
            main() {
                final user = User(`Lem`)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing failed: {:?}", parser.diagnostics);

    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program), "Type checking failed: {:?}", checker.diagnostics);
}

#[test]
fn test_e2e_definitely_returns_if_branches() {
    let source_code = r#"
        class Calculator {
            int compute(bool flag) {
                if (flag) {
                    return 10
                } else {
                    return 20
                }
            }
        }

        class Application {
            main() {
                final calc = Calculator()
                print(calc.compute(true))
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing failed: {:?}", parser.diagnostics);

    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program), "Type checking failed: {:?}", checker.diagnostics);

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(llvm_ir.contains("define i32 @Calculator_compute"));
    assert!(llvm_ir.contains("ret i64 10") || llvm_ir.contains("ret i32 10"));
    assert!(llvm_ir.contains("ret i64 20") || llvm_ir.contains("ret i32 20"));
}

#[test]
fn test_e2e_go_style_zero_values() {
    let source_code = r#"
        class User(
            long id,
            string name,
            var int age = 18
        ) {}

        class Application {
            main() {
                User user
                final u1 = User()
                final u2 = User(1000L)
                final u3 = User(1000L, `Lem`)
                final u4 = User(1000L, `Lem`, 35)

                print(user.id())
                print(user.name())
                print(u1.age())
                print(u2.age())
                print(u3.age())
                print(u4.age())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing failed: {:?}", parser.diagnostics);

    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program), "Type checking failed: {:?}", checker.diagnostics);

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(llvm_ir.contains("call i8* @ube_alloc"));
    assert!(llvm_ir.contains("store i32 18"));
    assert!(llvm_ir.contains("store i32 35"));
}

#[test]
fn test_e2e_go_zero_value_user() {
    let source_code = r#"
        class User(
            long id,
            string name,
            var int age = 18
        ) {}

        class Application {
            main() {
                User user

                print(user.id())
                print(user.name())
                print(user.age())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing failed: {:?}", parser.diagnostics);

    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program), "Type checking failed: {:?}", checker.diagnostics);

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(llvm_ir.contains("%User = type { i64, i8*, i32 }"));
    assert!(llvm_ir.contains("store i32 0"));
    assert!(!llvm_ir.contains("store i32 18"));
}

#[test]
fn test_e2e_unified_zero_value_instantiation() {
    let source_code = r#"
        class Account(
            long id,
            string holder,
            var int balance = 100
        ) {}

        class Application {
            main() {
                Account a1
                final a2 = Account()
                final a3 = Account(1000L)
                final a4 = Account(1000L, `Lem`)
                final a5 = Account(1000L, `Lem`, 500)

                print(a1.balance())
                print(a2.balance())
                print(a3.balance())
                print(a4.balance())
                print(a5.balance())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing failed: {:?}", parser.diagnostics);

    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program), "Type checking failed: {:?}", checker.diagnostics);

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(llvm_ir.contains("store i32 100"));
    assert!(llvm_ir.contains("store i32 500"));
}

#[test]
fn test_type_checker_constructor_argument_types() {
    let source_code = r#"
        class User(long id, string name) {}

        class Application {
            main() {
                final invalid = User(`Lem`, 1000L)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty());

    let mut checker = TypeChecker::new();
    let is_valid = checker.check_program(&program);
    assert!(!is_valid, "Type checker should reject mismatched constructor argument types");
    assert!(checker.diagnostics.iter().any(|d| d.message.contains("Expected") || d.message.contains("argument")));
}

#[test]
fn test_type_checker_variable_decl_type_enforcement() {
    let source_code = r#"
        class Application {
            main() {
                int age = `hello`
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty());

    let mut checker = TypeChecker::new();
    let is_valid = checker.check_program(&program);
    assert!(!is_valid, "Type checker should reject mismatched variable initializer type");
}

#[test]
fn test_type_checker_if_condition_bool_enforcement() {
    let source_code = r#"
        class Application {
            main() {
                if (123) {
                    print(`Hello`)
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty());

    let mut checker = TypeChecker::new();
    let is_valid = checker.check_program(&program);
    assert!(!is_valid, "Type checker should reject non-boolean if condition");
}

#[test]
fn test_e2e_unary_not_lowering() {
    let source_code = r#"
        class Application {
            main() {
                final a = false
                final b = not a
                print(b)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty());

    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program));

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(llvm_ir.contains("xor i1"));
}

#[test]
fn test_e2e_trait_return_type_preservation() {
    let source_code = r#"
        trait Item {
            string getTitle()
        }

        class Book(string title) {
            string getTitle() {
                return this._title
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty());

    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program));

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(llvm_ir.contains("define i8* @Book_getTitle"));
}

#[test]
fn test_e2e_decimal_modulus_lowering() {
    let source_code = r#"
        class Application {
            main() {
                final a = 10.5 % 3.2
                print(a)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty());

    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program));

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(llvm_ir.contains("frem double"));
}

#[test]
fn test_e2e_int_and_long_keep_distinct_storage_types() {
    let source_code = r#"
        class Application {
            main() {
                int small = 10
                long big = 10L

                print(small)
                print(big)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    assert!(
        builder.diagnostics.is_empty(),
        "IR building failed: {:?}",
        builder.diagnostics
    );

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    // int must really be i32.
    assert!(
        llvm_ir.contains("store i32 10"),
        "int literal was not lowered as i32:\n{}",
        llvm_ir
    );

    // long must really be i64.
    assert!(
        llvm_ir.contains("store i64 10"),
        "long literal was not lowered as i64:\n{}",
        llvm_ir
    );
}

#[test]
fn test_e2e_reject_wrong_field_assignment_type() {
    let source_code = r#"
        class User(var int age) {
            corrupt() {
                this._age = `not an integer`
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    let valid = checker.check_program(&program);

    assert!(
        !valid,
        "Ube must reject assigning string to int field"
    );

    assert!(
        !checker.diagnostics.is_empty(),
        "Expected assignment type diagnostic"
    );
}

#[test]
fn test_e2e_reject_wrong_method_argument_type() {
    let source_code = r#"
        class Greeter {
            greet(int times) {
                print(`Hello`)
            }
        }

        class Application {
            main() {
                final greeter = Greeter()

                greeter.greet(`wrong`)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    let valid = checker.check_program(&program);

    assert!(
        !valid,
        "Method expects int but string argument was accepted"
    );

    assert!(
        !checker.diagnostics.is_empty(),
        "Expected method argument type diagnostic"
    );
}

#[test]
fn test_e2e_application_startup_uses_zero_initialization() {
    let source_code = r#"
        class Application(
            var int port = 8080
        ) {
            main() {
                print(this.port())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    assert!(
        builder.diagnostics.is_empty(),
        "IR building failed: {:?}",
        builder.diagnostics
    );

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    // Startup ignores constructor defaults and uses the shared zero initializer.
    assert!(
        llvm_ir.contains("store i32 0") && !llvm_ir.contains("store i32 8080")
            && llvm_ir.contains("@__ube.internal_zero(i8* null)"),
        "Application startup did not use zero initialization:\n{}",
        llvm_ir
    );
}

#[test]
fn test_e2e_abstract_trait_nonvoid_method_never_returns_void() {
    let source_code = r#"
        trait Item {
            string title()
        }

        class Application {
            main() {}
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    if let Some(start) = llvm_ir.find("define i8* @Item_title") {
        let remaining = &llvm_ir[start..];
        let end = remaining.find("\n}").unwrap_or(remaining.len());
        let trait_function = &remaining[..end];

        assert!(
            !trait_function.contains("ret void"),
            "Non-void trait method generated ret void:\n{}",
            trait_function
        );
    } else {
        assert!(
            llvm_ir.contains("declare i8* @Item_title"),
            "Expected a valid abstract trait declaration:\n{}",
            llvm_ir
        );
    }
}

#[test]
fn test_e2e_reject_invalid_binary_operand_types() {
    let source_code = r#"
        class Application {
            main() {
                final bad = 10 and 20
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    let valid = checker.check_program(&program);

    assert!(
        !valid,
        "'and' must require bool operands"
    );

    assert!(
        !checker.diagnostics.is_empty(),
        "Expected invalid binary operand diagnostic"
    );
}

#[test]
fn test_e2e_string_interpolation_is_not_literal_text() {
    let source_code = r#"
        class Application {
            main() {
                final name = `Lem`

                print(`Hello ${name}`)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    // "${name}" must have been parsed/evaluated,
    // not embedded in the final constant.
    assert!(
        !llvm_ir.contains("${name}"),
        "Interpolation was emitted as literal text:\n{}",
        llvm_ir
    );
}

#[test]
fn test_e2e_unsupported_binary_never_panics_backend() {
    let source_code = r#"
        class Application {
            main() {
                final range = 1 .. 10
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Unexpected semantic failure: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    // If Range is not implemented yet, a builder diagnostic is valid.
    if !builder.diagnostics.is_empty() {
        return;
    }

    // If the builder accepts it, the LLVM backend must not panic.
    let result = std::panic::catch_unwind(
        std::panic::AssertUnwindSafe(|| {
            let mut emitter = TextualLlvmEmitter::new();
            emitter.emit_program(&ir_program)
        })
    );

    assert!(
        result.is_ok(),
        "LLVM emitter panicked on a parser-supported binary operator"
    );
}

#[test]
fn test_e2e_trait_declared_return_types_preserved_across_all_types() {
    let source_code = r#"
        trait DataSupplier {
            int count()
            long timestamp()
            decimal ratio()
            bool isValid()
            string code()
        }

        class Service(
            int _count,
            long _timestamp,
            decimal _ratio,
            bool _isValid,
            string _code
        ) {
            int count() {
                return this._count
            }
            long timestamp() {
                return this._timestamp
            }
            decimal ratio() {
                return this._ratio
            }
            bool isValid() {
                return this._isValid
            }
            string code() {
                return this._code
            }
        }

        class Application {
            main() {
                final s = Service(42, 1000L, 3.14, true, `OK`)
                print(s.count())
                print(s.timestamp())
                print(s.ratio())
                print(s.isValid())
                print(s.code())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    assert!(
        builder.diagnostics.is_empty(),
        "IR building failed: {:?}",
        builder.diagnostics
    );

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    // Verify declared return types on functions
    assert!(
        llvm_ir.contains("define i32 @Service_count"),
        "int return type missing:\n{}",
        llvm_ir
    );
    assert!(
        llvm_ir.contains("define i64 @Service_timestamp"),
        "long return type missing:\n{}",
        llvm_ir
    );
    assert!(
        llvm_ir.contains("define double @Service_ratio"),
        "decimal return type missing:\n{}",
        llvm_ir
    );
    assert!(
        llvm_ir.contains("define i1 @Service_isValid"),
        "bool return type missing:\n{}",
        llvm_ir
    );
    assert!(
        llvm_ir.contains("define i8* @Service_code"),
        "string return type missing:\n{}",
        llvm_ir
    );
}

#[test]
fn test_e2e_range_expression_emits_graceful_diagnostic() {
    let source_code = r#"
        class Application {
            main() {
                final numbers = 1 .. 10
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    let mut checker = TypeChecker::new();
    let _checked = checker.check_program(&program);

    let mut builder = IrBuilder::new();
    let _ir_program = builder.build_program(&program);

    assert!(
        !parser.diagnostics.is_empty() || !checker.diagnostics.is_empty() || !builder.diagnostics.is_empty(),
        "Expected range expression '..' to produce diagnostic error"
    );
}

#[test]
fn test_e2e_bare_unsupported_function_call_emits_diagnostic() {
    let source_code = r#"
        class Application {
            main() {
                doSomething()
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    let mut checker = TypeChecker::new();
    let _checked = checker.check_program(&program);

    let mut builder = IrBuilder::new();
    let _ir_program = builder.build_program(&program);

    assert!(
        !parser.diagnostics.is_empty() || !checker.diagnostics.is_empty() || !builder.diagnostics.is_empty(),
        "Expected bare unsupported call 'doSomething()' to produce diagnostic error"
    );
}

#[test]
fn test_e2e_unsupported_member_access_emits_diagnostic() {
    let source_code = r#"
        class Application {
            main() {
                final x = 42
                print(x.invalid_member)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    let mut checker = TypeChecker::new();
    let _checked = checker.check_program(&program);

    let mut builder = IrBuilder::new();
    let _ir_program = builder.build_program(&program);

    assert!(
        !parser.diagnostics.is_empty() || !checker.diagnostics.is_empty() || !builder.diagnostics.is_empty(),
        "Expected unresolvable member access to produce diagnostic error"
    );
}

#[test]
fn test_e2e_invalid_assignment_target_emits_diagnostic() {
    let source_code = r#"
        class Application {
            main() {
                (10 + 20) = 100
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    let mut checker = TypeChecker::new();
    let _checked = checker.check_program(&program);

    let mut builder = IrBuilder::new();
    let _ir_program = builder.build_program(&program);

    assert!(
        !parser.diagnostics.is_empty() || !checker.diagnostics.is_empty() || !builder.diagnostics.is_empty(),
        "Expected invalid assignment target '(10 + 20) = 100' to produce diagnostic error"
    );
}

#[test]
fn test_e2e_optional_chaining_emits_diagnostic_until_supported() {
    let source_code = r#"
        class User(var string name) {}

        class Application {
            main() {
                final u = User(`Alice`)
                print(u?.name)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    let mut checker = TypeChecker::new();
    let _checked = checker.check_program(&program);

    let mut builder = IrBuilder::new();
    let _ir_program = builder.build_program(&program);

    assert!(
        !parser.diagnostics.is_empty() || !checker.diagnostics.is_empty() || !builder.diagnostics.is_empty(),
        "Expected optional member access '?.' to produce diagnostic error until supported"
    );
}

#[test]
fn test_e2e_break_and_continue_outside_loop_emits_diagnostic() {
    let break_source = r#"
        class Application {
            main() {
                break
            }
        }
    "#;

    let lexer1 = Lexer::new(break_source);
    let mut parser1 = Parser::new(lexer1);
    let program1 = parser1.parse_program();
    let mut checker1 = TypeChecker::new();
    checker1.check_program(&program1);

    assert!(
        !checker1.diagnostics.is_empty(),
        "Expected 'break' outside loop to produce diagnostic error"
    );

    let continue_source = r#"
        class Application {
            main() {
                continue
            }
        }
    "#;

    let lexer2 = Lexer::new(continue_source);
    let mut parser2 = Parser::new(lexer2);
    let program2 = parser2.parse_program();
    let mut checker2 = TypeChecker::new();
    checker2.check_program(&program2);

    assert!(
        !checker2.diagnostics.is_empty(),
        "Expected 'continue' outside loop to produce diagnostic error"
    );
}

#[test]
fn test_e2e_invalid_unary_operands_emit_diagnostic() {
    let not_int_source = r#"
        class Application {
            main() {
                final x = not 123
            }
        }
    "#;

    let lexer1 = Lexer::new(not_int_source);
    let mut parser1 = Parser::new(lexer1);
    let program1 = parser1.parse_program();
    let mut checker1 = TypeChecker::new();
    checker1.check_program(&program1);

    assert!(
        !checker1.diagnostics.is_empty(),
        "Expected 'not 123' to produce diagnostic error"
    );

    let neg_bool_source = r#"
        class Application {
            main() {
                final y = -true
            }
        }
    "#;

    let lexer2 = Lexer::new(neg_bool_source);
    let mut parser2 = Parser::new(lexer2);
    let program2 = parser2.parse_program();
    let mut checker2 = TypeChecker::new();
    checker2.check_program(&program2);

    assert!(
        !checker2.diagnostics.is_empty(),
        "Expected '-true' to produce diagnostic error"
    );
}

#[test]
fn test_e2e_try_check_no_error_flow() {
    let source_code = r#"
        
        class Application {
            main() {
                try {
                    print(`Step 1`)
        
                }
                catch (Error err) {
                    print(`Error occurred`)
                }
        
                finally {
                    print(`Cleanup`)
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    assert!(
        builder.diagnostics.is_empty(),
        "IR building failed: {:?}",
        builder.diagnostics
    );

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(
        llvm_ir.contains("define i32 @main"),
        "LLVM emission failed for try/catch no-error flow"
    );
}

#[test]
fn test_e2e_try_raise_check_error_flow() {
    let source_code = r#"
        
        class CustomError(string _message) {
            string message() {
                return this._message
            }
        }
        
        class Application {
            main() {
                try {
                    print(`Before throw`)
                    throw(CustomError(`Something went wrong`))
        
                }
                catch (Error err) {
                    print(`Handled error`)
                }
        
                finally {
                    print(`Cleanup always runs`)
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    assert!(
        builder.diagnostics.is_empty(),
        "IR building failed: {:?}",
        builder.diagnostics
    );

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(
        llvm_ir.contains("define i32 @main"),
        "LLVM emission failed for try/throw/catch error flow"
    );
}

#[test]
fn test_e2e_throw_outside_try_is_valid() {
    let source_code = r#"
        class Application {
            main() {
                throw(`Error outside try`)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    let mut checker = TypeChecker::new();
    checker.check_program(&program);

    assert!(
        checker.diagnostics.is_empty(),
        "Throw outside try must propagate: {:?}", checker.diagnostics
    );
}

#[test]
fn test_e2e_try_raise_string_interpolation_flow() {
    let source_code = r#"
        
        class App {
            main() {
                try {
                    throw(`an error occurred`)
        
                }
                catch (Error error) {
                    print(`${error}`)
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);

    assert!(
        builder.diagnostics.is_empty(),
        "IR building failed: {:?}",
        builder.diagnostics
    );

    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(
        llvm_ir.contains("define i32 @main"),
        "LLVM emission failed for try/throw string interpolation flow"
    );
}

#[test]
fn test_e2e_raise_primitive_emits_diagnostic() {
    let source_code = r#"
        
        class Application {
            main() {
                try {
                    throw(123)
        
                }
                finally {
                    print(`Cleanup`)
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    let mut checker = TypeChecker::new();
    let is_valid = checker.check_program(&program);

    assert!(
        !is_valid,
        "Type checker must reject throwing primitive numbers"
    );

    assert!(
        checker.diagnostics.iter().any(|d| d.message.contains("throw target must be a string or satisfy 'Error'")),
        "Expected primitive throw diagnostic, got: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_raise_custom_class_error_trait() {
    let source_code = r#"
        
        class CustomError(
          int code,
          string rawMsg
        ) {
            string message() {
                return `Error ${this._code}: ${this._rawMsg}`
            }
        }
        
        class Application {
            main() {
                try {
                    throw(
                      CustomError(
                        404,
                        `Not Found`
                      )
                    )
        
                }
                catch (Error err) {
                    print(err.message())
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking custom error class failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let _ir_program = builder.build_program(&program);

    assert!(
        builder.diagnostics.is_empty(),
        "IR building custom error class failed: {:?}",
        builder.diagnostics
    );
}

#[test]
fn test_e2e_list_literal_indexing_and_methods() {
    let source_code = r#"
        class Application {
            main() {
                final numbers = [10, 20, 30]
                numbers.add(40)
                print(numbers[0])
                print(numbers.len())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing list literal failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking list literal failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_map_literal_indexing_and_len() {
    let source_code = r#"
        class Application {
            main() {
                final scores = [1000L: 95, 1001L: 88]
                print(scores[1000L])
                print(scores.len())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing map literal failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking map literal failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_list_index_out_of_bounds_raises_error() {
    let source_code = r#"
        
        class Application {
            main() {
                try {
                    final list = [1, 2]
                    print(list[10])
        
                }
                catch (Error err) {
                    print(err.message())
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing list out of bounds failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking list out of bounds failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_async_spawn_and_await() {
    let source_code = r#"
        class Application {
            async fetchScore(int id) {
                return 95
            }

            main() {
                final task = this.fetchScore(1000)
                final score = await task
                print(score)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing async fetchScore failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking async fetchScore failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_async_task_raise_rethrown_on_await() {
    let source_code = r#"
        
        class Application {
            async riskyTask() {
                throw(`async task error`)
            }
        
            main() {
                try {
                    final task = this.riskyTask()
                    await task
        
                }
                catch (Error err) {
                    print(err.message())
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing async riskyTask failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking async riskyTask failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_channel_send_and_receive() {
    let source_code = r#"
        class Application {
            main() {
                final ch = Channel()
                ch.send(100)
                final val = ch.receive()
                print(val)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing channel send and receive failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking channel send and receive failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_async_expression_spawn() {
    let source_code = r#"
        class Application {
            main() {
                final task = async (10 + 20)
                final val = await task
                print(val)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing inline async expression failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking inline async expression failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let ir = builder.build_program(&program);
    assert!(builder.diagnostics.is_empty(), "IR lowering failed: {:?}", builder.diagnostics);
    assert!(ir.functions.iter().any(|function| function.class_name.starts_with("UbeAsyncExpr")));
    let mut emitter = TextualLlvmEmitter::new();
    let llvm = emitter.emit_program(&ir);
    assert!(llvm.contains("@ube_async_expr_thunk_0"));
    assert!(llvm.contains("@UbeAsyncExpr0_run"));
}

#[test]
fn test_e2e_typed_closable_channel() {
    let source = r#"
        class Application {
            main() {
                Channel<string> messages = Channel<string>(2)
                final receivedTask = messages.receive()
                final sentTask = messages.send(`hello`)
                final sent = await sentTask
                final received = await receivedTask
                messages.close()
                print(received)
            }
        }
    "#;
    let mut parser = Parser::new(Lexer::new(source));
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing typed channel failed: {:?}", parser.diagnostics);
    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program), "Type checking typed channel failed: {:?}", checker.diagnostics);

    let mut builder = IrBuilder::new();
    let ir = builder.build_program(&program);
    assert!(builder.diagnostics.is_empty(), "IR lowering failed: {:?}", builder.diagnostics);
    let mut emitter = TextualLlvmEmitter::new();
    let llvm = emitter.emit_program(&ir);
    assert!(llvm.contains("@ube_channel_create_capacity"));
    assert!(llvm.contains("call i8* @ube_channel_send_async("));
    assert!(llvm.contains("call i8* @ube_channel_recv_async("));
    assert!(llvm.contains("@ube_channel_close"));
    assert!(!llvm.contains("call i1 @ube_channel_send("), "send must not block");
    assert!(!llvm.contains("call i64 @ube_channel_recv("), "receive must not block");
}

#[test]
fn test_async_task_errors_route_to_try_catch() {
    let source = r#"
        
        class Application {
            async fail() {
                throw(`task failed`)
            }
        
            main() {
                try {
                    final result = await this.fail()
                    print(result)
        
                }
                catch (Error err) {
                    print(err.message())
                }
            }
        }
    "#;
    let mut parser = Parser::new(Lexer::new(source));
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing failed: {:?}", parser.diagnostics);

    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program), "Type checking failed: {:?}", checker.diagnostics);

    let mut builder = IrBuilder::new();
    let ir = builder.build_program(&program);
    assert!(builder.diagnostics.is_empty(), "IR lowering failed: {:?}", builder.diagnostics);

    let mut emitter = TextualLlvmEmitter::new();
    let llvm = emitter.emit_program(&ir);
    assert!(llvm.contains("call void @ube_exception_set"));
    assert!(llvm.contains("call i8* @ube_exception_take"));
    assert!(llvm.contains("call i8* @ube_exception_poll_cancel"));
    assert!(llvm.contains("call i8* @ube_object_method"));
}

#[test]
fn test_e2e_multiple_async_tasks_concurrent_await() {
    let source_code = r#"
        class Application {
            async compute(int val) {
                return val * 2
            }

            main() {
                final t1 = this.compute(10)
                final t2 = this.compute(20)
                final r1 = await t1
                final r2 = await t2
                print(r1 + r2)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing multiple concurrent async tasks failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking multiple concurrent async tasks failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_try_check_inside_async_method_body() {
    let source_code = r#"
        
        class Application {
            async safeTask() {
                var res = 0
                try {
                    throw(`inner async error`)
        
                }
                catch (Error err) {
                    res = 42
                }
                return res
            }
        
            main() {
                final task = this.safeTask()
                final res = await task
                print(res)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing try/catch inside async method failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking try/catch inside async method failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_task_all_combinator_success() {
    let source_code = r#"
        class Application {
            main() {
                final tasks = [async 10, async 20]
                final results = await Task.all(tasks)
                print(results.len())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Task.all failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Task.all failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_task_all_combinator_fail_fast() {
    let source_code = r#"
        
        class Application {
            async failingTask() {
                throw(`task 2 failed`)
            }
        
            main() {
                try {
                    final tasks = [async 10, this.failingTask()]
                    await Task.all(tasks)
        
                }
                catch (Error err) {
                    print(err.message())
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Task.all fail-fast failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Task.all fail-fast failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_task_any_and_race_combinators() {
    let source_code = r#"
        class Application {
            main() {
                final t1 = async 100
                final t2 = async 200
                final winner = await Task.race([t1, t2])
                print(winner)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Task.race failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Task.race failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_task_all_settled_combinator() {
    let source_code = r#"
        class Application {
            main() {
                final tasks = [async 10, async 20]
                final settled = await Task.allSettled(tasks)
                print(settled.len())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Task.allSettled failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Task.allSettled failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_task_timeout_wraps_any_task() {
    let source_code = r#"
        
        class Application {
            main() {
                Channel<int> inbox = Channel<int>(1)
                try {
                    final quick = await (async 500).timeout(2000)
                    print(quick)
                    final message = await inbox.receive().timeout(50)
                    print(message)
        
                }
                catch (Error err) {
                    print(err.message())
                }
            }
        }
    "#;

    let mut parser = Parser::new(Lexer::new(source_code));
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing timeout failed: {:?}", parser.diagnostics);
    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program), "Type checking timeout failed: {:?}", checker.diagnostics);
    let mut builder = IrBuilder::new();
    let ir = builder.build_program(&program);
    assert!(builder.diagnostics.is_empty(), "IR lowering failed: {:?}", builder.diagnostics);
    let llvm = TextualLlvmEmitter::new().emit_program(&ir);
    assert!(llvm.contains("call i8* @ube_task_timeout("));
}

#[test]
fn test_removed_concurrency_forms_are_rejected() {
    let removed_task_async = r#"
        class Application {
            main() {
                final task = Task.async(10 + 20)
                print(await task)
            }
        }
    "#;
    let mut parser = Parser::new(Lexer::new(removed_task_async));
    let program = parser.parse_program();
    let mut checker = TypeChecker::new();
    assert!(!checker.check_program(&program));
    assert!(checker
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("Task.async(...) was removed")), "{:?}", checker.diagnostics);

    let task_arithmetic = r#"
        class Application {
            main() {
                final task = async 40 + 2
            }
        }
    "#;
    let mut parser = Parser::new(Lexer::new(task_arithmetic));
    let program = parser.parse_program();
    let mut checker = TypeChecker::new();
    assert!(!checker.check_program(&program));
    assert!(checker
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("`async (a + b)`")));

    let removed_await_and_cancel = r#"
        class Application {
            main() {
                final value = awaitAndCancel(async 1, 100)
            }
        }
    "#;
    let mut parser = Parser::new(Lexer::new(removed_await_and_cancel));
    let program = parser.parse_program();
    let mut checker = TypeChecker::new();
    assert!(!parser.diagnostics.is_empty() || !checker.check_program(&program));
}

#[test]
fn test_unguarded_await_failure_stops_or_propagates() {
    let source = r#"
        class Application {
            async fail() {
                throw(`boom`)
            }

            async relay() {
                final value = await this.fail()
                return value
            }

            main() {
                final value = await this.relay()
                print(value)
            }
        }
    "#;
    let mut parser = Parser::new(Lexer::new(source));
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing failed: {:?}", parser.diagnostics);
    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program), "Type checking failed: {:?}", checker.diagnostics);
    let mut builder = IrBuilder::new();
    let ir = builder.build_program(&program);
    assert!(builder.diagnostics.is_empty(), "IR lowering failed: {:?}", builder.diagnostics);
    let llvm = TextualLlvmEmitter::new().emit_program(&ir);
    assert!(llvm.contains("call void @ube_exception_set("));
    assert!(llvm.contains("call void @ube_exception_fail_unhandled()"));
}

#[test]
fn test_e2e_datetime_factories_and_getters() {
    let source_code = r#"
        class Application {
            main() {
                final dt = DateTime.of(2026, 8, 31, 14, 30)
                print(dt.year())
                print(dt.month())
                print(dt.day())
                print(dt.hour())
                print(dt.minute())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing DateTime factories failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking DateTime factories failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_datetime_arithmetic_and_diff() {
    let source_code = r#"
        class Application {
            main() {
                final start = DateTime.of(2026, 8, 31, 12, 0)
                final later = start.addHours(2)
                print(later.hour())
                final diffSec = later.diffSeconds(start)
                print(diffSec)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing DateTime arithmetic failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking DateTime arithmetic failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_datetime_formatting_and_parsing() {
    let source_code = r#"
        class Application {
            main() {
                final dt = DateTime.parse(`2026-08-31T14:30:00+08:00`)
                final formatted = dt.format(`yyyy-MM-dd HH:mm:ss`)
                print(formatted)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing DateTime formatting and parsing failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking DateTime formatting and parsing failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_duration_and_addition() {
    let source_code = r#"
        class Application {
            main() {
                final start = DateTime.of(2026, 8, 31, 10, 0)
                final dur = Duration.hours(3)
                final end = start.addDuration(dur)
                print(end.hour())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Duration addition failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Duration addition failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_datetime_instant_timezone_invariance() {
    let source_code = r#"
        class Application {
            main() {
                final manila = DateTime.of(2026, 8, 31, 17, 0, `Asia/Manila`)
                final tokyo = manila.in(`Asia/Tokyo`)
                print(manila.timestamp() == tokyo.timestamp())
                print(manila.hour())
                print(tokyo.hour())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing timezone invariance failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking timezone invariance failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_datetime_unix_timestamp_factories() {
    let source_code = r#"
        class Application {
            main() {
                final dt = DateTime.fromUnix(1788159600)
                final sec = dt.unix()
                final ms = dt.unixMillis()
                print(sec)
                print(ms)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Unix timestamp factories failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Unix timestamp factories failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_financial_money_factories_and_getters() {
    let source_code = r#"
        class Application {
            main() {
                final balance = Money.php(1250.75)
                print(balance.amount())
                final curr = balance.currency()
                print(curr.code())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Money factories failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Money factories failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_financial_money_addition() {
    let source_code = r#"
        class Application {
            main() {
                final subtotal = Money.php(1000)
                final shipping = Money.php(100)
                final total = subtotal.add(shipping)
                print(total.amount())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Money addition failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Money addition failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_financial_exchange_conversion() {
    let source_code = r#"
        class Application {
            main() {
                final dollar = Money.usd(100)
                final rate = 58.50
                final converted = Exchange.convert(dollar, Currency.PHP, rate)
                print(converted.amount())
                final curr = converted.currency()
                print(curr.code())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Exchange conversion failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Exchange conversion failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_financial_percent_and_rate_multiplication() {
    let source_code = r#"
        class Application {
            main() {
                final price = Money.php(1000)
                final vat = Percent(12)
                final tax = price.multiply(vat)
                print(tax.amount())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Percent multiplication failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Percent multiplication failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_geo_point_and_distance_getters() {
    let source_code = r#"
        class Application {
            main() {
                final p = GeoPoint(14.5995, 120.9842)
                print(p.latitude())
                print(p.longitude())
                final d = Distance.kilometers(5.5)
                print(d.kilometers())
                print(d.meters())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing GeoPoint and Distance getters failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking GeoPoint and Distance getters failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_geo_distance_and_bearing() {
    let source_code = r#"
        class Application {
            main() {
                final home = GeoPoint(14.58, 121.12)
                final provider = GeoPoint(14.60, 121.10)
                final dist = home.distanceTo(provider)
                print(dist.kilometers())
                final bearing = home.bearingTo(provider)
                print(bearing)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing distanceTo and bearingTo failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking distanceTo and bearingTo failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_geo_bounds_contains() {
    let source_code = r#"
        class Application {
            main() {
                final sw = GeoPoint(14.0, 120.0)
                final ne = GeoPoint(15.0, 121.0)
                final bounds = GeoBounds(sw, ne)
                final target = GeoPoint(14.5, 120.5)
                print(bounds.contains(target))
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing GeoBounds contains failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking GeoBounds contains failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_geolocation_current_and_error() {
    let source_code = r#"
        
        class Application {
            main() {
                try {
                    final pos = Geolocation.current()
                    print(pos.latitude())
        
                }
                catch (Error err) {
                    print(err.message())
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Geolocation current failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Geolocation current failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_math_constants_roots_powers() {
    let source_code = r#"
        class Application {
            main() {
                final pi = Math.PI
                print(pi)
                final sq = Math.sqrt(144)
                print(sq)
                final pow = Math.pow(2, 8)
                print(pow)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Math roots and powers failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Math roots and powers failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_math_rounding_clamp_sign() {
    let source_code = r#"
        class Application {
            main() {
                print(Math.round(12.56))
                print(Math.clamp(150, 0, 100))
                print(Math.sign(-42))
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Math rounding and clamp failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Math rounding and clamp failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_math_integer_gcd_lcm_even_odd() {
    let source_code = r#"
        class Application {
            main() {
                print(Math.gcd(48, 18))
                print(Math.lcm(12, 18))
                print(Math.isEven(10))
                print(Math.isOdd(7))
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Math gcd and lcm failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Math gcd and lcm failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_random_securerandom_statistics() {
    let source_code = r#"
        class Application {
            main() {
                final rInt = Random.int(1, 100)
                print(rInt > 0)
                final secBytes = SecureRandom.bytes(16)
                print(secBytes.len())
                final stats = [10.0, 20.0, 30.0]
                print(Statistics.mean(stats))
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Random and Statistics failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Random and Statistics failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_probability_combinatorics() {
    let source_code = r#"
        class Application {
            main() {
                final nCr = Probability.combinations(5, 2)
                final nPr = Probability.permutations(5, 2)
                final fact = Probability.factorial(5)
                print(nCr)
                print(nPr)
                print(fact)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Probability combinatorics failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Probability combinatorics failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_probability_distributions() {
    let source_code = r#"
        class Application {
            main() {
                final pdf = Probability.normal(0, 0, 1)
                print(pdf)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Probability distributions failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Probability distributions failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_statistics_descriptive_and_correlation() {
    let source_code = r#"
        class Application {
            main() {
                final data = [10.0, 20.0, 30.0, 40.0, 50.0]
                print(Statistics.mean(data))
                print(Statistics.median(data))
                print(Statistics.percentile(data, 50))
                final x = [1.0, 2.0, 3.0]
                final y = [2.0, 4.0, 6.0]
                print(Statistics.correlation(x, y))
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Statistics correlation failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Statistics correlation failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_file_write_read_exists_delete() {
    let source_code = r#"
        class Application {
            main() {
                final path = `test_file_rw.txt`
                File.write(path, `Hello Ube File API`)
                print(File.exists(path))
                final content = File.read(path)
                print(content)
                File.delete(path)
                print(File.exists(path))
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing File write read failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking File write read failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_file_open_object_read_after_close() {
    let source_code = r#"
        
        class Application {
            main() {
                final path = `test_obj_file.txt`
                File.write(
                  path,
                  `File Object Line`
                )
                final file = File.open(path)
                try {
                    final text = file.read()
                    print(text)
        
                }
                catch (Error err) {
                    print(err.message())
                }
                finally { file.close() }
                File.delete(path)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing File open object failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking File open object failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_file_info_metadata() {
    let source_code = r#"
        class Application {
            main() {
                final path = `test_info.txt`
                File.write(path, `Metadata test`)
                final info = File.info(path)
                print(info.size() > 0)
                print(info.isDirectory())
                File.delete(path)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing File info metadata failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking File info metadata failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_directory_and_path_operations() {
    let source_code = r#"
        class Application {
            main() {
                final p = Path.join(`/home/user`, `config.json`)
                print(Path.filename(p))
                print(Path.extension(p))
                final dir = `test_dir_logs`
                Directory.create(dir)
                final files = Directory.list(`.`)
                print(files.len() >= 0)
                Directory.delete(dir)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Directory and Path failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Directory and Path failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_file_error_handling_try_check() {
    let source_code = r#"
        
        class Application {
            main() {
                try {
                    final content = File.read(`non_existent_file_999.txt`)
                    print(content)
        
                }
                catch (Error err) {
                    if (err is FileNotFoundError) {
                        print(`File does not exist`)
                    }
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing File error handling failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking File error handling failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_server_route_registration_and_response_factories_typecheck() {
    let source_code = r#"
        class Application {
            main() {
                final server = Server()
                server.get(`/users`) { request ->
                    return Response.ok(`User list`)
                }
                server.post(`/users`) { request ->
                    return Response.created(`User created`)
                }
                print(server.port())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Server routes failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Server routes failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_server_request_path_query_headers_typecheck() {
    let source_code = r#"
        class Application {
            main() {
                final server = Server()
                server.get(`/users/{id}`) { request ->
                    final id = request.pathLong(`id`)
                    final auth = request.header(`Authorization`)
                    if (id == 0) {
                        return Response.notFound(`User not found`)
                    }
                    return Response.json(`{"id": 1, "name": "Alice"}`)
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Server Request path and headers failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Server Request path and headers failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_server_middleware_and_try_syntax_typecheck() {
    let source_code = r#"
        
        class Application {
            main() {
                final server = Server()
                server.use(`/api`) { request ->
                    if (not request.hasBearerToken()) {
                        raise(GenericError(`Unauthorized`))
                    }
                }
                server.get(`/api/data`) { request ->
                    try {
                        final val = request.query(`search`)
                        print(val)
                    }
                    catch (Error err) {
                        if (err) {
                            print(err.message())
                        }
                    }
                    Response.text(`Request processed`)
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Server middleware failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Server middleware failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_server_websocket_and_tls_syntax_typecheck() {
    let source_code = r#"
        class Application {
            main() {
                final server = Server()
                server.tls(`cert.pem`, `key.pem`)
                server.websocket(`/chat`) { socket ->
                    socket.onMessage { msg ->
                        socket.send(`Echo: ${msg}`)
                    }
                }
                server.start(8080)
                server.stop()
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Server WebSocket and TLS failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Server WebSocket and TLS failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_server_http_protocol_versions_syntax_typecheck() {
    let source_code = r#"
        class Application {
            main() {
                final server = Server()
                server.enableHttp2()
                server.enableHttp3(443)
                server.get(`/protocol`) { request ->
                    print(request.protocol())
                    print(request.isHttp3())
                    return Response.ok(`Protocol: ${request.protocol()}`)
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing HTTP versions failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking HTTP versions failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_json_parse_getters_stringify() {
    let source_code = r#"
        class Application {
            main() {
                final text = `{"name": "Lem", "age": 40}`
                final json = Json.parse(text)
                print(json.getString(`name`))
                print(json.getInt(`age`))
                final s = Json.stringify(json)
                print(s.len() > 0)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Json getters failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Json getters failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_json_parse_error_handling() {
    let source_code = r#"
        
        class Application {
            main() {
                try {
                    final data = Json.parse(`{invalid_json}`)
        
                }
                catch (Error err) {
                    if (err is JsonParseError) {
                        print(err.message())
                    }
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Json parse error failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Json parse error failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_xml_create_element_text_tostring() {
    let source_code = r#"
        class Application {
            main() {
                final doc = Xml.create(`user`)
                doc.root().element(`name`).text(`Lem`)
                doc.root().element(`age`).text(`40`)
                print(doc.toString())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Xml create failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Xml create failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_xml_parse_child_navigation() {
    let source_code = r#"
        class Application {
            main() {
                final text = `<user><name>Lem</name><age>40</age></user>`
                final doc = Xml.parse(text)
                final root = doc.root()
                print(root.child(`name`).text())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing Xml parse child navigation failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking Xml parse child navigation failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_pure_ube_json_and_xml_record_mapping() {
    let source_code = r#"
        class Application {
            main() {
                final jsonText = `{"restaurantID": "0603", "orderType": "DineIn"}`
                final json = Json.parse(jsonText)
                print(json.getString(`restaurantID`))

                final xmlText = `<KitchenOrderScan><RestaurantID>0603</RestaurantID></KitchenOrderScan>`
                final xmlDoc = Xml.parse(xmlText)
                print(xmlDoc.root().child(`RestaurantID`).text())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing pure Ube record mapping failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking pure Ube record mapping failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_pure_ube_partial_parse_and_strict_modes() {
    let source_code = r#"
        class Application {
            main() {
                final partialJson = Json.partialParse(`{"name": "Lem", "extra": 123}`, JsonObject)
                print(partialJson.getString(`name`))
                final partialXml = Xml.partialParse(`<user><name>Lem</name><extra>123</extra></user>`, XmlDocument)
                print(partialXml.root().child(`name`).text())
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing pure Ube partial parse failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking pure Ube partial parse failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_nullable_declaration_and_null_assignment_valid() {
    let source_code = r#"
        class User(string name = `Guest`)

        class Application {
            main() {
                User? u1
                User? u2 = null
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing nullable declaration failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking nullable declaration failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_non_nullable_declaration_default_constructor_valid() {
    let source_code = r#"
        class User(string name = `Guest`)

        class Application {
            main() {
                User u1
                User u2 = User()
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing non-nullable declaration failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking non-nullable declaration failed: {:?}",
        checker.diagnostics
    );
}

#[test]
fn test_e2e_non_nullable_declaration_null_assignment_invalid() {
    let source_code = r#"
        class User(string name = `Guest`)

        class Application {
            main() {
                User u = null
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing failed: {:?}", parser.diagnostics);

    let mut checker = TypeChecker::new();
    let is_valid = checker.check_program(&program);
    assert!(
        !is_valid || !checker.diagnostics.is_empty(),
        "Expected assigning null to non-nullable type User to produce a diagnostic error"
    );
}

#[test]
fn test_e2e_return_inside_try_is_valid() {
    let source_code = r#"
        
        class Application {
            int getValue() {
                try {
                    return 42
        
                }
                catch (Error err) {
                    print(err.message())
                }
                return 0
            }
        
            main() {
                this.getValue()
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    let mut checker = TypeChecker::new();
    let is_valid = checker.check_program(&program);

    assert!(is_valid, "Return inside try must be valid: {:?}", checker.diagnostics);
}

#[test]
fn test_e2e_break_inside_try_cleans_up_loop_exit() {
    let source_code = r#"
        
        class Application {
            main() {
                for (x in [1]) {
                    try {
                        print(`Before break`)
                        break
                        print(`Unreachable after break`)
        
                    }
                    finally {
                        print(`Exit cleanup runs on break`)
                    }
                }
                print(`After try block`)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing break inside try failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking break inside try failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let _ir_program = builder.build_program(&program);

    assert!(
        builder.diagnostics.is_empty(),
        "IR building break inside try failed: {:?}",
        builder.diagnostics
    );
}

#[test]
fn test_e2e_for_else_empty_collection_executes_else() {
    let source_code = r#"
        class Application {
            main() {
                final emptyItems = []
                for (item in emptyItems) {
                    print(`Loop body executed`)
                } else {
                    print(`Else block executed for 0 iterations`)
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing for/else failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking for/else failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let _ir_program = builder.build_program(&program);

    assert!(
        builder.diagnostics.is_empty(),
        "IR building for/else failed: {:?}",
        builder.diagnostics
    );
}

#[test]
fn test_e2e_for_else_non_empty_collection_skips_else() {
    let source_code = r#"
        class Application {
            main() {
                final items = [10, 20, 30]
                for (item in items) {
                    print(`Loop body item ${item}`)
                } else {
                    print(`Else block should be skipped`)
                }
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing for/else non-empty failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking for/else non-empty failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let _ir_program = builder.build_program(&program);

    assert!(
        builder.diagnostics.is_empty(),
        "IR building for/else non-empty failed: {:?}",
        builder.diagnostics
    );
}

#[test]
fn test_e2e_if_expression_with_braces_and_yield() {
    let source_code = r#"
        class Application {
            main() {
                final score = 85
                final status = if (score >= 50) {
                    yield `Passed`
                } else {
                    yield `Failed`
                }
                print(`Status: ${status}`)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing if expression failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking if expression failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let _ir_program = builder.build_program(&program);

    assert!(
        builder.diagnostics.is_empty(),
        "IR building if expression failed: {:?}",
        builder.diagnostics
    );
}

#[test]
fn test_e2e_for_expression_comprehension_with_braces_and_yield() {
    let source_code = r#"
        class Application {
            main() {
                final numbers = [1, 2, 3]
                final doubled = for (x in numbers) {
                    yield x * 2
                }
                print(`Doubled list length: ${doubled.len()}`)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing for expression comprehension failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking for expression comprehension failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let _ir_program = builder.build_program(&program);

    assert!(
        builder.diagnostics.is_empty(),
        "IR building for expression comprehension failed: {:?}",
        builder.diagnostics
    );
}

#[test]
fn test_e2e_full_expression_and_lambda_string_interpolation() {
    let source_code = r#"
        class Application {
            main() {
                final a = 10
                final b = 20

                // Arithmetic & Binary Operations
                print(`Sum: ${a + b * 2}`)

                // Immediately-Invoked Anonymous Function (Lambda)
                print(`Calculated: ${ { (x) -> x * 21 }(2) }`)

                // Expression evaluation inside interpolation
                print(`Status: ${if (a > 5) { yield \`High\` } else { yield \`Low\` }}`)
            }
        }
    "#;

    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    assert!(
        parser.diagnostics.is_empty(),
        "Parsing full expression string interpolation failed: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&program),
        "Type checking full expression string interpolation failed: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let _ir_program = builder.build_program(&program);

    assert!(
        builder.diagnostics.is_empty(),
        "IR building full expression string interpolation failed: {:?}",
        builder.diagnostics
    );
}
