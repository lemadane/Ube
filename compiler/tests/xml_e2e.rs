//! Real native XML parsing, document edits, and automatic typed-data serialization.
mod support;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};
use ube_compiler::{Lexer, Parser, TypeChecker};
struct Program {
    _temp: tempfile::TempDir,
    executable: PathBuf,
    llvm: PathBuf,
}
impl Program {
    fn new(source: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let entry = support::write_source(&temp.path().join("App.ube"), source);
        fs::write(temp.path().join(".gitignore"), ".build/\n").unwrap();
        let executable = temp.path().join("program");
        let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
            .arg("build")
            .arg(entry)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let llvm = PathBuf::from(
            stdout
                .lines()
                .find_map(|line| {
                    line.strip_prefix("LLVM IR generated at '")
                        .and_then(|path| path.strip_suffix("'"))
                })
                .expect("build must report its generated LLVM IR"),
        );
        Self {
            _temp: temp,
            executable,
            llvm,
        }
    }
    fn optimize(&self) {
        let runtime = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("libube_runtime.a");
        let output = Command::new("clang")
            .args(["-Wno-override-module", "-O2"])
            .arg(&self.llvm)
            .arg(runtime)
            .args(["-ldl", "-lpthread", "-lm"])
            .arg("-o")
            .arg(&self.executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new("timeout")
            .arg("90s")
            .arg(&self.executable)
            .args(args)
            .output()
            .unwrap()
    }
}
fn expect(output: Output, expected: &str) {
    assert!(
        output.status.success(),
        "status {:?}, stdout {}, stderr {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}
#[test]
fn objects_lists_maps_and_nulls_round_trip_through_real_xml() {
    let program = Program::new(
        r#"
    class Address(string city)
    class Profile(int id,string name,bool active,int? absent,int? zero,Address address,List scores,Map settings)
    class App {start(){
        final profile=Profile(42,'Alice <&> 世界',false,null,0,Address('東京'),[1,true,null],['theme':'dark','nested':['x':2]])
        Gc.collect()
        final xml=profile.toXml()
        final root=Xml.toObject(xml).root()
        print(root.name())
        print(root.child('id').text())
        print(root.child('name').text())
        print(root.child('active').text())
        print(root.child('absent').isNull())
        print(root.child('zero').isNull())
        print(root.child('zero').text())
        print(root.child('address').child('city').text())
        print(root.child('scores').childCount('item'))
        print(root.child('scores').childAt('item',1).text())
        print(root.child('scores').childAt('item',2).isNull())
        print(root.child('settings').childCount('entry'))
        print(Xml.toObject([1,'two',false].toXml()).root().childAt('item',1).text())
        print(Xml.toObject([1000L:'large','not a tag!':'value'].toXml()).root().childCount('entry'))
        print([].toXml())
        print(''.toXml())
        print(profile.toJson()==profile.toJson())
        final nil=null
        print(Xml.toObject(nil.toXml()).root().isNull())
    }}"#,
    );
    expect(program.run(&[]),"Profile\n42\nAlice <&> 世界\nfalse\ntrue\nfalse\n0\n東京\n3\ntrue\ntrue\n2\ntwo\n2\n<list/>\n<value></value>\ntrue\ntrue\n");
}

#[test]
fn options_customize_roots_attributes_namespaces_and_list_items() {
    let program = Program::new(
        r#"
    class Profile(int id,string name,List scores)
    class App {start(){
        final options=XmlOptions().root('p:person').namespace('p','urn:people').attribute('id').rename('name','p:displayName').listItem('score')
        final value=Profile(42,'Alice & Bob',[95,88]).toXml(options)
        final root=Xml.toObject(value).root()
        print(root.name())
        print(root.namespaceUri())
        print(root.getAttribute('id'))
        print(root.childNs('urn:people','displayName').text())
        print(root.child('scores').childAt('score',1).text())
        print(Xml.toObject([1,2].toXml(XmlOptions().root('numbers').listItem('n'))).root().childCount('n'))
        final plain=XmlOptions().root('person').namespace('','urn:default')
        print(Xml.toObject(Profile(1,'A',[]).toXml(plain)).root().child('name').namespaceUri())
    }}"#,
    );
    expect(
        program.run(&[]),
        "p:person\nurn:people\n42\nAlice & Bob\n88\n2\nurn:default\n",
    );
}

#[test]
fn document_building_and_parsing_preserve_real_content_and_namespaces() {
    let program = Program::new(
        r#"
    class App {start(){
        final doc=Xml.create('p:root')
        doc.root().namespace('p','urn:doc').attribute('id','42')
        doc.root().element('p:name').text('東京 <&>')
        doc.root().element('item').text('first')
        doc.root().element('item').text('second')
        final parsed=Xml.toObject(doc.toXml())
        print(parsed.root().name())
        print(parsed.root().getAttribute('id'))
        print(parsed.root().childNs('urn:doc','name').text())
        print(parsed.root().childAt('item',1).text())
        print(Xml.toObject(parsed.root().child('p:name').toXml()).root().namespaceUri())
        final mixed=Xml.toObject('<p:root xmlns:p="urn:p" xmlns:q="urn:p" q:id="7">before<p:child/>after</p:root>')
        print(mixed.root().text())
        print(mixed.root().getAttribute('q:id'))
        print(Xml.toObject(mixed.toXml()).root().getAttribute('q:id'))
        print(Xml.parse('<actual>not Lem</actual>').root().text())
        print(Xml.stringify(Xml.create('empty')))
        print(Xml.partialParse('<compat/>',XmlDocument).root().name())
    }}"#,
    );
    expect(
        program.run(&[]),
        "p:root\n42\n東京 <&>\nsecond\nurn:doc\nbeforeafter\n7\n7\nnot Lem\n<empty/>\ncompat\n",
    );
}

#[test]
fn malformed_xml_cycles_resources_and_invalid_customization_are_catchable() {
    let program = Program::new(
        r#"
    class Node(Node? next)
    class Profile(int id,string name)
    class App {start(){
        try {Xml.toObject('<root>')}catch(XmlParseError e){print('malformed')}
        try {Xml.toObject('<!DOCTYPE root [<!ENTITY x "boom">]><root>&x;</root>')}catch(XmlParseError e){print('dtd rejected')}
        try {Xml.toObject('<p:root/>')}catch(XmlParseError e){print('unbound prefix')}
        try {Xml.toObject('<root id="1" id="2"/>')}catch(XmlParseError e){print('duplicate attribute')}
        final list=[]
        list.add(list)
        try {list.toXml()}catch(XmlError e){print('cycle')}
        final cache=Cache.memory(4096)
        try {['cache':cache].toXml()}catch(XmlError e){print('resource')}
        cache.close()
        try {[Math.sqrt(-1)].toXml()}catch(XmlError e){print('nonfinite')}
        try {Profile(1,'A').toXml(XmlOptions().attribute('missing'))}catch(XmlError e){print('missing option field')}
        try {Profile(1,'A').toXml(XmlOptions().rename('name','id'))}catch(XmlError e){print('name collision')}
        try {Xml.create('bad name')}catch(XmlError e){print('invalid name')}
        try {XmlOptions().namespace('xsi','urn:wrong')}catch(XmlError e){print('reserved namespace')}
        try {Xml.toObject('<r/>').root().childAt('item',-1)}catch(XmlError e){print('bad index')}
        try {Xml.toObject('<r/>').root().getAttribute('missing')}catch(XmlError e){print('missing attribute')}
        print(Profile(7,'after').toXml())
    }}"#,
    );
    expect(program.run(&[]),"malformed\ndtd rejected\nunbound prefix\nduplicate attribute\ncycle\nresource\nnonfinite\nmissing option field\nname collision\ninvalid name\nreserved namespace\nbad index\nmissing attribute\n<Profile><id>7</id><name>after</name></Profile>\n");
}

#[test]
fn xml_signatures_receivers_and_reserved_methods_are_checked() {
    for statement in [
        "Xml.toObject(1)",
        "Xml.toObject()",
        "Xml.toObject('{}',XmlDocument)",
        "XmlDocument()",
        "XmlElement()",
        "Xml()",
        "XmlOptions(1)",
        "XmlOptions.root('x')",
        "XmlOptions().attribute(1)",
        "XmlOptions().namespace('p')",
        "42.toXml(1)",
        "Profile.toXml()",
        "Profile(1).toXml(XmlOptions(),XmlOptions())",
        "Xml.toObject('<r/>').toXml(XmlOptions())",
        "XmlDocument.root()",
        "XmlElement.text()",
        "Xml.toObject('<r/>').root().childAt('item','bad')",
        "Xml.toObject('<r/>').root().text(1)",
        "Xml.to('text',Profile)",
    ] {
        let source = format!("class Profile(int id) class App {{start(){{{statement}}}}}");
        let mut parser = Parser::new(Lexer::new(&source));
        let program = parser.parse_program();
        let mut checker = TypeChecker::new();
        assert!(
            !parser.diagnostics.is_empty() || !checker.check_program(&program),
            "accepted {statement}"
        );
    }
    for source in [
        "class Bad { string toXml(){return 'bad'} }",
        "class Bad(string toXml)",
    ] {
        let mut parser = Parser::new(Lexer::new(source));
        let program = parser.parse_program();
        let mut checker = TypeChecker::new();
        assert!(
            !checker.check_program(&program),
            "accepted reserved method/field: {source}"
        );
    }
}

#[test]
fn xml_encoding_and_document_handles_work_across_async_calls() {
    let program = Program::new(
        r#"
    class Profile(int id)
    class App {
        async string encode() { return Profile(7).toXml() }
        async XmlDocument document() { return Xml.toObject('<ready>ok</ready>') }
        async fail() { Xml.toObject('<broken>') }
        start() {
            print(await this.encode())
            final document=await this.document()
            print(document.root().text())
            try {await this.fail()}catch(XmlParseError e){print('worker XML error')}
            final json=Json.toObject('{"items":[1,2]}')
            print(Xml.toObject(json.toXml()).root().child('entry').childAt('item',1).text())
            try {Json.toObject(`{"bad":"\u0001"}`).toXml()}catch(XmlError e){print('XML control character rejected')}
        }
    }"#,
    );
    expect(
        program.run(&[]),
        "<Profile><id>7</id></Profile>\nok\nworker XML error\n2\nXML control character rejected\n",
    );
}

#[test]
fn shipped_xml_project_runs_and_builds_with_the_documented_output() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    for (path, contents) in [
        ("ube.toml", include_str!("../../examples/xml/ube.toml")),
        (
            "src/App.ube",
            include_str!("../../examples/xml/src/App.ube"),
        ),
        (
            "src/Profile.ube",
            include_str!("../../examples/xml/src/Profile.ube"),
        ),
        (".gitignore", ".build/\n"),
    ] {
        fs::write(project.path().join(path), contents).unwrap();
    }
    let expected = include_str!("../../examples/xml/README.md")
        .split_once("```text\n")
        .expect("XML example README must document its output")
        .1
        .split_once("```")
        .unwrap()
        .0;
    let cli_run = Command::new("timeout")
        .args([
            "90s",
            env!("CARGO_BIN_EXE_ube-compiler"),
            "run",
            "src/App.ube",
        ])
        .current_dir(project.path())
        .output()
        .unwrap();
    expect(cli_run, expected);

    let executable = project.path().join("xml-example");
    let build = Command::new("timeout")
        .args([
            "90s",
            env!("CARGO_BIN_EXE_ube-compiler"),
            "build",
            "src/App.ube",
            "-o",
        ])
        .arg(&executable)
        .current_dir(project.path())
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let native_run = Command::new("timeout")
        .arg("90s")
        .arg(&executable)
        .current_dir(project.path())
        .output()
        .unwrap();
    expect(native_run, expected);
}

#[test]
fn xml_file_round_trip_between_independent_native_processes() {
    let writer = Program::new(
        r#"
    class Address(string city)
    class Profile(long id,string name,Address address,List scores,Map metadata,int? missing)
    class App {start(){
        final profile=Profile(9007199254740993L,'Alice <&> 世界',Address('東京'),[true,null,3],['space key':'value <&>'],null)
        final options=XmlOptions().root('p:profile').namespace('p','urn:profile').attribute('id').rename('name','p:displayName').listItem('score')
        File.write(Console.args()[0],profile.toXml(options))
        print('saved')
    }}"#,
    );
    let reader = Program::new(
        r#"
    class App {start(){
        final document=Xml.toObject(File.read(Console.args()[0]))
        final root=document.root()
        print(root.name())
        print(root.namespaceUri())
        print(root.getAttribute('id'))
        print(root.childNs('urn:profile','displayName').text())
        print(root.child('address').child('city').text())
        print(root.child('scores').childCount('score'))
        print(root.child('scores').childAt('score',1).isNull())
        print(root.child('scores').childAt('score',2).text())
        print(root.child('metadata').child('entry').getAttribute('key'))
        print(root.child('metadata').child('entry').text())
        print(root.child('missing').isNull())
        final reparsed=Xml.toObject(document.toXml()).root()
        print(reparsed.childNs('urn:profile','displayName').text()==root.childNs('urn:profile','displayName').text())
    }}"#,
    );
    let files = tempfile::tempdir().unwrap();
    let path = files.path().join("profile.xml");
    let args = [path.to_str().unwrap()];
    expect(writer.run(&args), "saved\n");
    let stored = fs::read_to_string(&path).unwrap();
    assert!(stored.contains("Alice &lt;&amp;&gt; 世界"), "{stored}");
    assert!(stored.contains("xmlns:p=\"urn:profile\""), "{stored}");
    assert!(stored.contains("xsi:nil=\"true\""), "{stored}");
    let expected = "p:profile\nurn:profile\n9007199254740993\nAlice <&> 世界\n東京\n3\ntrue\n3\nspace key\nvalue <&>\ntrue\ntrue\n";
    expect(reader.run(&args), expected);
    // A second fresh process must read the file without writer-owned handles.
    expect(reader.run(&args), expected);

    fs::write(&path, "<profile><broken></profile>").unwrap();
    let malformed = reader.run(&args);
    assert_eq!(malformed.status.code(), Some(1), "{malformed:?}");
    assert!(
        String::from_utf8_lossy(&malformed.stderr).contains("XmlParseError"),
        "{}",
        String::from_utf8_lossy(&malformed.stderr)
    );
    assert!(malformed.stdout.is_empty(), "{malformed:?}");
}

#[test]
fn external_xml_fixtures_enforce_parser_limits_and_recover_after_errors() {
    let reader = Program::new(
        r#"
    class App {start(){
        try {
            final document=Xml.toObject(File.read(Console.args()[0]))
            final root=Xml.toObject(document.toXml()).root()
            print(root.name())
            print(root.text())
        }catch(XmlParseError error){print('rejected')}
        print(Xml.toObject('<healthy>after</healthy>').root().text())
    }}"#,
    );
    let fixtures = tempfile::tempdir().unwrap();
    let path = fixtures.path().join("input.xml");
    let valid = [
        (
            "<r><![CDATA[&#xD800;]]>&amp;#1114112;<!-- &#xD800; --><?pi &#x110000;?></r>",
            "r\n&#xD800;&#1114112;\nafter\n",
        ),
        (
            "<r>世界 &amp; &#x1F600; &#65;</r>",
            "r\n世界 & 😀 A\nafter\n",
        ),
        (
            "<?xml version=\"1.0\"?><r>a<![CDATA[<&>]]><!--ignored--><?pi ignored?>b<c/>d</r>",
            "r\na<&>bd\nafter\n",
        ),
        ("<r>line\r\nnext&#13;end</r>", "r\nline\nnext\rend\nafter\n"),
    ];
    for (xml, expected) in valid {
        fs::write(&path, xml).unwrap();
        expect(reader.run(&[path.to_str().unwrap()]), expected);
    }
    let invalid = [
        String::new(),
        "<a/><b/>".into(),
        "<r>&unknown;</r>".into(),
        "<r>&#0;</r>".into(),
        "<r>&#xD800;</r>".into(),
        "<r id=\"&#1114112;\"/>".into(),
        "<r>&#x110000;</r>".into(),
        "<r xmlns:p=\"urn:x\" xmlns:q=\"urn:x\" p:id=\"1\" q:id=\"2\"/>".into(),
        "<!DOCTYPE r SYSTEM \"file:///etc/passwd\"><r/>".into(),
        "<!DOCTYPE r [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><r>&x;</r>".into(),
        format!("{}{}", "<n>".repeat(66), "</n>".repeat(66)),
        format!("<r>{}</r>", "<n/>".repeat(100_000)),
        format!("<r>{}</r>", "x".repeat(16 * 1024 * 1024)),
    ];
    for (index, xml) in invalid.iter().enumerate() {
        fs::write(&path, xml).unwrap();
        let output = reader.run(&[path.to_str().unwrap()]);
        assert!(output.status.success(), "fixture {index}: {output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "rejected\nafter\n",
            "fixture {index}"
        );
    }
}

#[test]
fn namespace_scopes_null_semantics_and_detached_subtrees_round_trip() {
    let program = Program::new(
        r#"
    class App {start(){
        final doc=Xml.toObject('<r xmlns="urn:outer" xmlns:p="urn:one" xmlns:n="http://www.w3.org/2001/XMLSchema-instance"><p:a id="1"><p:b xmlns:p="urn:two">inside</p:b><plain xmlns="">outside</plain></p:a><missing n:nil="1"/><empty/><false n:nil="false"/></r>')
        final root=doc.root()
        print(root.namespaceUri())
        final a=root.childNs('urn:one','a')
        print(a.getAttribute('id'))
        print(a.childNs('urn:two','b').text())
        print(a.childNs('','plain').text())
        print(root.child('missing').isNull())
        print(root.child('empty').isNull())
        print(root.child('false').isNull())
        print(root.childCount('*'))
        final detached=Xml.toObject(a.toXml()).root()
        print(detached.namespaceUri())
        print(detached.childNs('urn:two','b').namespaceUri())
        print(detached.childNs('','plain').namespaceUri()=='')
        print(Xml.toObject(doc.toXml()).root().child('missing').isNull())
    }}"#,
    );
    expect(
        program.run(&[]),
        "urn:outer\n1\ninside\noutside\ntrue\nfalse\nfalse\n4\nurn:one\nurn:two\ntrue\ntrue\n",
    );
}

#[test]
fn options_are_root_scoped_reusable_and_reject_ambiguous_attributes() {
    let program = Program::new(
        r#"
    class Child(int id,string name)
    class Parent(int id,string name,Child child)
    class Nullable(int? id)
    class App {start(){
        final value=Parent(7,'outer',Child(8,'inner'))
        final options=XmlOptions().root('person').attribute('id').rename('name','label')
        final root=Xml.toObject(value.toXml(options)).root()
        print(root.getAttribute('id'))
        print(root.child('label').text())
        print(root.child('child').child('id').text())
        print(root.child('child').child('name').text())
        print(value.toXml(options)==value.toXml(options))
        print(Xml.toObject(value.toXml()).root().child('name').text())
        try {Nullable(null).toXml(XmlOptions().attribute('id'))}catch(XmlError e){print('null attribute')}
        try {value.toXml(XmlOptions().attribute('child'))}catch(XmlError e){print('object attribute')}
        try {value.toXml(XmlOptions().namespace('p','urn:x').namespace('q','urn:x').rename('id','p:same').rename('name','q:same'))}catch(XmlError e){print('expanded collision')}
        try {value.toXml(XmlOptions().attribute('id').namespace('n','http://www.w3.org/2001/XMLSchema-instance').rename('id','n:nil'))}catch(XmlError e){print('nil spoof')}
        try {value.toXml(XmlOptions().root('p:unbound'))}catch(XmlError e){print('unbound root')}
        print(Xml.toObject(value.toXml(options)).root().child('label').text())
    }}"#,
    );
    expect(program.run(&[]), "7\nouter\n8\ninner\ntrue\nouter\nnull attribute\nobject attribute\nexpanded collision\nnil spoof\nunbound root\nouter\n");
}

#[test]
fn document_edits_preserve_live_handles_and_attribute_whitespace() {
    let program = Program::new(
        r#"
    class App {start(){
        final doc=Xml.create('r')
        final root=doc.root()
        final child=root.element('item')
        child.text('before')
        root.attribute('value','quote " & < >\r\n\tend')
        Gc.collect()
        child.text('after')
        print(doc.root().child('item').text())
        print(Xml.toObject(doc.toXml()).root().getAttribute('value')=='quote " & < >\r\n\tend')
        child.element('nested').text('inside')
        print(child.text())
        child.text('replacement')
        print(child.childCount('*'))
        print(Xml.toObject(root.toXml()).root().child('item').text())
        try {root.child('absent')}catch(XmlError e){print('missing child')}
        try {root.childAt('item',1)}catch(XmlError e){print('past end')}
        print(child.getText())
    }}"#,
    );
    expect(
        program.run(&[]),
        "after\ntrue\nafterinside\n0\nreplacement\nmissing child\npast end\nreplacement\n",
    );
}

#[test]
fn encoding_limits_and_error_recovery_hold_with_optimized_native_code() {
    let program = Program::new(
        r#"
    class App {start(){
        var nested=[]
        var depth=0
        for(depth<66){nested=[nested] depth=depth+1}
        try {nested.toXml()}catch(XmlError e){print('depth limit')}
        final many=[]
        var count=0
        for(count<100001){many.add(count) count=count+1}
        try {many.toXml()}catch(XmlError e){print('node limit')}
        final text=File.read(Console.args()[0])
        try {text.toXml()}catch(XmlError e){print('output limit')}
        final root=Xml.toObject(['ok':[1,true,'世界']].toXml()).root()
        print(root.child('entry').childAt('item',2).text())
        print(Xml.toObject('&#xD800;'.toXml()).root().text())
    }}"#,
    );
    let files = tempfile::tempdir().unwrap();
    let path = files.path().join("large.txt");
    // Input is within the byte limit, but XML escaping expands it past 16 MiB.
    fs::write(&path, "&".repeat(4 * 1024 * 1024)).unwrap();
    let expected = "depth limit\nnode limit\noutput limit\n世界\n&#xD800;\n";
    expect(program.run(&[path.to_str().unwrap()]), expected);
    program.optimize();
    expect(program.run(&[path.to_str().unwrap()]), expected);
}

#[test]
fn shipped_ube_xml_tests_pass_with_testicle_and_mockery() {
    let project = tempfile::tempdir().unwrap();
    for (path, contents) in [
        ("ube.toml", include_str!("../../examples/xml/ube.toml")),
        (".gitignore", ".build/\n"),
        (
            "tests/XmlTests.ube",
            include_str!("../../examples/xml/tests/XmlTests.ube"),
        ),
        (
            "tests/xml_test_types/XmlProfile.ube",
            include_str!("../../examples/xml/tests/xml_test_types/XmlProfile.ube"),
        ),
        (
            "tests/xml_test_types/XmlSource.ube",
            include_str!("../../examples/xml/tests/xml_test_types/XmlSource.ube"),
        ),
        (
            "tests/xml_test_types/XmlConsumer.ube",
            include_str!("../../examples/xml/tests/xml_test_types/XmlConsumer.ube"),
        ),
    ] {
        let file = project.path().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, contents).unwrap();
    }
    let output = Command::new("timeout")
        .args(["180s", env!("CARGO_BIN_EXE_ube-compiler"), "test"])
        .current_dir(project.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Tests: 12 passed, 0 failed"), "{stdout}");
    for test in [
        "testObjectFieldsAndNulls",
        "testAsyncSerialization",
        "testConsumerUsesMockedSourceAndRealXmlParser",
        "testConsumerPropagatesMalformedXml",
        "testConsumerPropagatesSourceFailure",
    ] {
        assert!(stdout.contains(test), "missing test {test}: {stdout}");
    }
}
