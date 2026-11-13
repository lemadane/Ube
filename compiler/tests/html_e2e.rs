//! USSR tests through the real CLI and native executable.
use std::{fs, process::Command};

fn run(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let entry = directory.path().join("App.ube");
    fs::write(&entry, source).unwrap();
    let result = Command::new("timeout").args(["45s", env!("CARGO_BIN_EXE_ube-compiler"), "run"])
        .arg(entry).output().unwrap();
    assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
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
            Gc.collect()
            print(`${render()}`)
            print(`${html`<input required value="${title}">`}`)
        }
    }"#, "<ul><li title=\"&lt;script&gt;&amp;&quot;\">&lt;script&gt;&amp;&quot;<b>Done</b></li></ul>\n<p>&lt;b&gt;string&lt;/b&gt; 42 true</p>\n<div><em>captured</em></div>\n<input required value=\"&lt;script&gt;&amp;&quot;\">\n");
}

#[test]
fn dynamic_urls_are_validated_and_errors_are_catchable() {
    run(r#"class App { start() {
        final target = '/todos?q=a&b="x"'
        print(`${html`<a href="${target}">Todos</a>`}`)
        try {
            final bad = 'java\nscript:alert(1)'
            final fragment = html`<a href="${bad}">bad</a>`
            print('must not reach')
        } catch (GenericError error) { print('blocked') }
        final label = 'safe'
        print(`${html`<p>${label}</p>`}`)
    } }"#, "<a href=\"/todos?q=a&amp;b=&quot;x&quot;\">Todos</a>\nblocked\n<p>safe</p>\n");
}

#[test]
fn type_errors_and_unsafe_contexts_fail_cli_check() {
    let cases = [
        ("final x = html`<p>${missing}</p>`", "missing"),
        ("final x = html`<p>${[1, 2]}</p>`", "HTML interpolation"),
        ("final x = html`<p>${print('x')}</p>`", "HTML interpolation"),
        ("final x = html`<p title=\"${html`<b>x</b>`}\">x</p>`", "HTML interpolation"),
        ("final x = html`<script>${'x'}</script>`", "Interpolation in script"),
        ("final x = html`<style>${'x'}</style>`", "Interpolation in script"),
        ("final x = html`<p onclick=\"${'x'}\">x</p>`", "attribute 'onclick'"),
        ("final x = html`<p style=\"${'x'}\">x</p>`", "attribute 'style'"),
        ("final x = html`<!-- ${'x'} -->`", "comments"),
        ("final x = html`<${'p'}>x</p>`", "tag"),
        ("final x = html`<p title=${'x'}>x</p>`", "quoted"),
        ("final x = html`<a href=\"java${'script'}:x\">x</a>`", "entire quoted attribute"),
        ("final x = html`<svg><title>x</title></svg>`", "Unsupported HTML tag"),
        ("final x = html`<div></p>`", "Mismatched"),
        ("final x = Html()", "Create Html"),
        ("final x = Response.html('<b>unsafe</b>')", "expects one Html"),
        ("final x = this.render(42)", "argument"),
        ("final x = this.render()", "expects 1 arguments"),
        ("final x = this.render('a', 'b')", "expects 1 arguments"),
        ("final x = this.consume('not Html')", "argument"),
    ];
    for (body, diagnostic) in cases {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("App.ube");
        fs::write(&entry, format!("class App {{ Html render(string title) {{ return html`<p>${{title}}</p>` }} consume(Html value) {{ }} start() {{ {body} }} }}")).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler")).arg("check").arg(entry).output().unwrap();
        let message = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert!(!output.status.success() && message.contains(diagnostic), "{body}: {message}");
    }
}
