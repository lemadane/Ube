//! USSR tests through the real CLI and native executable.
use std::{fs, process::Command};

fn run(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let entry = directory.path().join("App.ube");
    fs::write(&entry, source).unwrap();
    let result = Command::new("timeout")
        .args(["45s", env!("CARGO_BIN_EXE_ube-compiler"), "run"])
        .arg(entry)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), expected);
}

#[test]
fn escaped_values_typed_components_and_conditionals_execute_natively() {
    run(r#"class App {
        Html todo(string title, bool completed) {
            return html`<li title="${title}">${title}${completed then html`<b>Done</b>` else html`<i>Pending</i>`}</li>`
        }
        start() {
            final title = '<script>&"'
            final page = html`<ul>${this.todo(title, true)}</ul>`
            print(`${page}`)
            print(`${html`<p>${`<b>string</b>`} ${42} ${true}</p>`}`)
            final fragment = html`<em>captured</em>`
            final render = () -> html`<div>${fragment}</div>`
            GarbageCollector.collect()
            print(`${render()}`)
            print(`${html`<input required value="${title}">`}`)
        }
    }"#, "<ul><li title=\"&lt;script&gt;&amp;&quot;\">&lt;script&gt;&amp;&quot;<b>Done</b></li></ul>\n<p>&lt;b&gt;string&lt;/b&gt; 42 true</p>\n<div><em>captured</em></div>\n<input required value=\"&lt;script&gt;&amp;&quot;\">\n");
}

#[test]
fn dynamic_urls_are_validated_and_errors_are_catchable() {
    run(
        r#"class App { start() {
        final target = '/todos?q=a&b="x"'
        print(`${html`<a href="${target}">Todos</a>`}`)
        try {
            final bad = 'java\nscript:alert(1)'
            final fragment = html`<a href="${bad}">bad</a>`
            print('must not reach')
        } catch (GenericError error) { print('blocked') }
        final label = 'safe'
        print(`${html`<p>${label}</p>`}`)
    } }"#,
        "<a href=\"/todos?q=a&amp;b=&quot;x&quot;\">Todos</a>\nblocked\n<p>safe</p>\n",
    );
}

#[test]
fn htmx_data_aliases_preserve_attribute_safety() {
    for attribute in [
        "data-hx-on:click",
        "data-hx-on--before-request",
        "data-hx-on",
        "data-hx-vals",
        "data-hx-headers",
        "data-hx-trigger",
        "DATA-HX-ON:CLICK",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("App.ube");
        fs::write(&entry, format!(
            "class App {{ start() {{ final value = 'alert(1)' final x = html`<button {attribute}=\"${{value}}\">Go</button>` }} }}"
        )).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
            .arg("check")
            .arg(entry)
            .output()
            .unwrap();
        let diagnostic = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.status.success() && diagnostic.contains("is not supported"),
            "{attribute}: {diagnostic}"
        );
    }
    run(
        r#"class App { start() {
            final path = '/toggle?a=1&b=2'
            final target = 'closest li'
            final label = '<Todo>'
            print(`${html`<button data-hx-post="${path}" data-hx-target="${target}" data-label="${label}">Go</button>`}`)
            try {
                final bad = 'javascript:alert(1)'
                final fragment = html`<button data-hx-get="${bad}">Go</button>`
                print('must not reach')
            } catch (GenericError error) { print('blocked') }
        } }"#,
        "<button data-hx-post=\"/toggle?a=1&amp;b=2\" data-hx-target=\"closest li\" data-label=\"&lt;Todo&gt;\">Go</button>\nblocked\n",
    );
}

#[test]
fn type_errors_and_unsafe_contexts_fail_cli_check() {
    let cases = [
        ("final x = html`<p>${missing}</p>`", "missing"),
        ("final x = html`<p>${[1, 2]}</p>`", "HTML interpolation"),
        ("final x = html`<p>${print('x')}</p>`", "HTML interpolation"),
        (
            "final x = html`<p title=\"${html`<b>x</b>`}\">x</p>`",
            "HTML interpolation",
        ),
        (
            "final x = html`<script>${'x'}</script>`",
            "Interpolation in script",
        ),
        (
            "final x = html`<style>${'x'}</style>`",
            "Interpolation in script",
        ),
        (
            "final x = html`<p onclick=\"${'x'}\">x</p>`",
            "attribute 'onclick'",
        ),
        (
            "final x = html`<p style=\"${'x'}\">x</p>`",
            "attribute 'style'",
        ),
        ("final x = html`<!-- ${'x'} -->`", "comments"),
        ("final x = html`<${'p'}>x</p>`", "tag"),
        ("final x = html`<p title=${'x'}>x</p>`", "quoted"),
        (
            "final x = html`<a href=\"java${'script'}:x\">x</a>`",
            "entire quoted attribute",
        ),
        (
            "final x = html`<button data-hx-post=\"/todos/${42}\">Go</button>`",
            "entire quoted attribute",
        ),
        (
            "final x = html`<svg><title>x</title></svg>`",
            "Unsupported HTML tag",
        ),
        ("final x = html`<div></p>`", "Mismatched"),
        ("final x = Html()", "Create Html"),
        (
            "final x = Response.html('<b>unsafe</b>')",
            "expects one Html",
        ),
        ("final x = this.render(42)", "argument"),
        ("final x = this.render()", "expects 1 arguments"),
        ("final x = this.render('a', 'b')", "expects 1 arguments"),
        ("final x = this.consume('not Html')", "argument"),
    ];
    for (body, diagnostic) in cases {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("App.ube");
        fs::write(&entry, format!("class App {{ Html render(string title) {{ return html`<p>${{title}}</p>` }} consume(Html value) {{ }} start() {{ {body} }} }}")).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
            .arg("check")
            .arg(entry)
            .output()
            .unwrap();
        let message = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.status.success() && message.contains(diagnostic),
            "{body}: {message}"
        );
    }
}

#[test]
fn ordinary_loops_compose_typed_fragments() {
    run(
        r#"class App { start() {
        var rows = html``
        for (i = 1 to 3) { rows = html`${rows}<li>${i}</li>` }
        print(`${html`<ul>${rows}</ul>`}`)
        final escaped = `${html`<b>text</b>`}`
        print(`${html`<p>${escaped}</p>`}`)
    } }"#,
        "<ul><li>1</li><li>2</li><li>3</li></ul>\n<p>&lt;b&gt;text&lt;/b&gt;</p>\n",
    );
}

#[test]
fn html_cannot_be_forged_by_return_types_or_declarations() {
    for (file, source) in [
        (
            "App",
            "class App { Html render() { return '<b>unsafe</b>' } start() {} }",
        ),
        (
            "App",
            "class App { start() { final x = true then html`<b>x</b>` else 'text' } }",
        ),
        ("App", "class App { Html render() { return print('unsafe') } start() {} }"),
        ("App", "class App { Html render() { yield '<b>unsafe</b>' } start() {} }"),
        ("App", "class App { start() { final x = html`<p>${if (true) { yield html`<b>ok</b>` } else { yield '<b>unsafe</b>' }}</p>` } }"),
        ("App", "class App { start() { final x = html`<p>${if (true) { yield html`<b>ok</b>` }}</p>` } }"),
        ("Html", "class Html {}"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join(format!("{file}.ube"));
        fs::write(&entry, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
            .arg("check")
            .arg(entry)
            .output()
            .unwrap();
        assert!(!output.status.success(), "unexpectedly accepted: {source}");
    }
}

#[test]
fn shipped_todo_components_build_through_cli() {
    let dir = tempfile::tempdir().unwrap();
    for (name, source) in [
        ("App.ube", include_str!("../../examples/ussr/App.ube")),
        ("Todo.ube", include_str!("../../examples/ussr/Todo.ube")),
        (
            "TodoPage.ube",
            include_str!("../../examples/ussr/TodoPage.ube"),
        ),
    ] {
        fs::write(dir.path().join(name), source).unwrap();
    }
    fs::write(dir.path().join(".gitignore"), ".build/\n").unwrap();
    let result = Command::new("timeout")
        .args([
            "45s",
            env!("CARGO_BIN_EXE_ube-compiler"),
            "build",
            "App.ube",
            "-o",
            "todo",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(dir.path().join("todo").is_file());
}

#[test]
fn canvas_templates_support_dimensions_and_escaped_fallback_content() {
    run(
        r#"class App { start() {
            final fallback = '<enable canvas>'
            print(`${html`<canvas width="${384}" height="${288}" aria-label="Game">${fallback}</canvas>`}`)
        } }"#,
        "<canvas width=\"384\" height=\"288\" aria-label=\"Game\">&lt;enable canvas&gt;</canvas>\n",
    );
}
