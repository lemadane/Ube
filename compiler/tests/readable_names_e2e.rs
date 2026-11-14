mod support;
use std::process::Command;
use ube_compiler::{lexer::Lexer, parser::Parser, semantics::TypeChecker};

#[test]
fn readable_math_time_and_collector_names_run_natively() {
    let directory = tempfile::tempdir().unwrap();
    let entry = support::write_source(&directory.path().join("App.ube"), r#"
        class App {
            start() {
                print(Math.absolute(-3) == 3.0)
                print(Math.squareRoot(9) == 3.0)
                print(Math.cubeRoot(8) == 2.0)
                print(Math.power(2, 3) == 8.0)
                print(Math.exponential(0) == 1.0)
                print(Math.ceiling(1.2) == 2.0)
                print(Math.greatestCommonDivisor(12, 8) == 4)
                print(Math.leastCommonMultiple(12, 8) == 24)
                final earlier = DateTime.fromUnixMilliseconds(1000)
                final later = DateTime.fromUnixMilliseconds(4000)
                print(earlier.unixMilliseconds() == 1000)
                print(later.differenceInSeconds(earlier) == 3)
                GarbageCollector.collect()
                print(GarbageCollector.objects() >= 0L)
                print(GarbageCollector.reclaimed() >= 0L)
            }
        }
    "#);
    let output = Command::new("timeout")
        .args(["60s", env!("CARGO_BIN_EXE_ube-compiler"), "run"])
        .arg(entry).output().unwrap();
    assert!(output.status.success(), "{}\n{}",
        String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "true\n".repeat(12));
}

#[test]
fn replaced_names_are_rejected() {
    for statement in [
        "Gc.collect()", "DateTime.fromUnixMillis(0)",
        "DateTime.now().unixMillis()", "DateTime.now().diffSeconds(DateTime.now())",
        "Math.abs(-1)", "Math.sqrt(4)", "Math.cbrt(8)", "Math.pow(2, 3)",
        "Math.exp(0)", "Math.ceil(1.2)", "Math.gcd(12, 8)", "Math.lcm(12, 8)",
        "RestInPeace.given().caFile('certificate.pem')",
        "response.elapsedMs()", "element.childNs('urn:test', 'child')",
    ] {
        let source = format!("class App {{ probe(RestInPeaceResponse response, XmlElement element) {{ {statement} }} start() {{}} }}");
        let mut parser = Parser::new(Lexer::new(&source));
        let program = parser.parse_program();
        assert!(parser.diagnostics.is_empty(), "{statement}: {:?}", parser.diagnostics);
        let mut checker = TypeChecker::new();
        assert!(!checker.check_program(&program), "accepted {statement}");
    }
}
