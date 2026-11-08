//! Native object serialization, authoritative JSON persistence, stress and SIGKILL recovery.
mod support;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    time::Duration,
};
use ube_compiler::{Lexer, Parser, TypeChecker};
struct Program {
    _temp: tempfile::TempDir,
    executable: PathBuf,
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
        Self {
            _temp: temp,
            executable,
        }
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
fn native_object_json_and_store_round_trip_are_real() {
    let program = Program::new(
        r#"
    class Address(string city)
    class Profile(int id,string name,long large,decimal ratio,bool active,int? absent,int? zero,Address address)
    class App {start(){
        final store=KeyValueStore.open(Console.args()[0])
        final profile=Profile(-123,'Alice 世界 "quoted"',9007199254740993L,0.25,true,null,0,Address('東京'))
        Gc.collect()
        store.set('profile',profile.toJson())
        final parsed=Json.toObject(store.get('profile') ?? '')
        Console.printLine(parsed.getInt('id'))
        Console.printLine(parsed.getString('name'))
        Console.printLine(parsed.getLong('large'))
        Console.printLine(parsed.getDecimal('ratio'))
        Console.printLine(parsed.getBool('active'))
        Console.printLine(parsed.getInt('zero'))
        Console.printLine(parsed.getObject('address').getString('city'))
        try { parsed.getInt('absent') } catch(JsonError err){Console.printLine('null preserved')}
        Console.printLine(parsed.toJson()==profile.toJson())
        Console.printLine(Json.toObject(profile.toJson()).toJson()==profile.toJson())
        Console.printLine(Json.toObject('{"array":["a","b"],"objects":[{"id":7}]}').getArray('array').getString(1))
        Console.printLine(Json.toObject('{"array":["a","b"]}').getArray('array').len())
        Console.printLine(Json.toObject('{"objects":[{"id":7}]}').getArray('objects').get(0).getInt('id'))
        store.set('literal-null','null')
        Console.printLine(store.has('literal-null'))
        Console.printLine(store.get('literal-null') ?? 'missing')
        Console.printLine(store.get('missing') ?? 'missing')
        try {store.set('profile','not JSON')} catch(Error err){Console.printLine('invalid rejected')}
        Console.printLine(Json.toObject(store.get('profile') ?? '').getInt('id'))
        try {final duplicate=KeyValueStore.open(Console.args()[0])}catch(Error err){Console.printLine('owner protected')}
        store.close()
        store.close()
        try {store.has('profile')}catch(Error err){Console.printLine('closed rejected')}
        final reopened=KeyValueStore.open(Console.args()[0])
        Console.printLine(Json.toObject(reopened.get('profile') ?? '').getString('name'))
        Console.printLine(reopened.delete('profile'))
        Console.printLine(reopened.delete('profile'))
        reopened.close()
    }}"#,
    );
    let temp = tempfile::tempdir().unwrap();
    expect(program.run(&[temp.path().join("records.db").to_str().unwrap()]),"-123\nAlice 世界 \"quoted\"\n9007199254740993\n0.25\ntrue\n0\n東京\nnull preserved\ntrue\ntrue\nb\n2\n7\ntrue\nnull\nmissing\ninvalid rejected\n-123\nowner protected\nclosed rejected\nAlice 世界 \"quoted\"\ntrue\nfalse\n");
}
#[test]
fn serialization_rejects_cycles_resources_and_nonfinite_values_without_losing_state() {
    let program = Program::new(
        r#"
    class Node(var Node? next,string label)
    class Pair(Node left,Node right)
    class Resource(Cache cache)
    class App {start(){
        final node=Node(null,'leaf')
        final shared=Pair(node,node).toJson()
        Console.printLine(Json.toObject(shared).getObject('left').getString('label'))
        Console.printLine(Json.toObject(shared).getObject('right').getString('label'))
        node.next(node)
        try {node.toJson()}catch(JsonError err){Console.printLine('cycle rejected')}
        final cache=Cache.memory(4096)
        try {Resource(cache).toJson()}catch(JsonError err){Console.printLine('resource field rejected')}
        try {cache.toJson()}catch(JsonError err){Console.printLine('resource rejected')}
        Console.printLine([1,2].toJson())
        final invalid=Math.sqrt(-1)
        try {invalid.toJson()}catch(JsonError err){Console.printLine('nonfinite rejected')}
        try {Json.toObject('{bad')}catch(JsonParseError err){Console.printLine('parse rejected')}
        final number=42
        final flag=false
        Console.printLine(number.toJson())
        Console.printLine(flag.toJson())
        Console.printLine('quoted "text"'.toJson())
        var chain=Node(null,'end')
        var i=0
        for(i<100){chain=Node(chain,'node') i=i+1}
        try {chain.toJson()}catch(JsonError err){Console.printLine('depth rejected')}
        Console.printLine(Node(null,'after error').toJson())
        cache.close()
    }}"#,
    );
    let output = program.run(&[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines: String = String::from_utf8_lossy(&output.stdout).into();
    assert!(lines.starts_with("leaf\nleaf\ncycle rejected\nresource field rejected\nresource rejected\n[1,2]\nnonfinite rejected\nparse rejected\n42\nfalse\n\"quoted \\\"text\\\"\"\ndepth rejected\n"),"{lines}");
    let last = lines.lines().last().unwrap();
    let value: serde_json::Value = serde_json::from_str(last).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"label":"after error","next":null})
    );
}
const STRESS: &str = r#"
class Address(string city)
class Payload(int id,string text,long wide,decimal ratio,bool active,int? absent,Address address)
class App {
    string payload(int id){return Payload(id,'quotes " slash \\ newline\n 世界',9007199254740993L,0.25,true,null,Address('東京')).toJson()}
    async int work(KeyValueStore store,int worker){
        if(store.add('claim',worker.toJson())){store.increment('winners',1)}
        var i=0
        for(i<250){store.set('worker:${worker}:${i}',this.payload(i)) store.increment('counter',1) if(i % 50 == 0){Gc.collect()} i=i+1}
        return i
    }
    start(){
        final args=Console.args()
        final store=KeyValueStore.open(args[0])
        if(args[1]=='write'){
            store.set('deleted','{"old":1}') store.delete('deleted')
            store.set('updated','{"version":1}') store.set('updated','{"version":2}')
            var i=0
            for(i<5000){store.set('record:${i}',this.payload(i)) if(i % 100 == 0){Gc.collect()} Console.printLine(i) i=i+1}
        }
        if(args[1]=='verify' or args[1]=='verify-all'){
            var limit=1000
            if(args[1]=='verify-all'){limit=5000}
            var i=0
            for(i<limit){
                final value=Json.toObject(store.get('record:${i}') ?? '')
                if(value.getInt('id')!=i){throw 'wrong id'}
                if(value.getString('text')!='quotes " slash \\ newline\n 世界'){throw 'wrong text'}
                if(value.getLong('wide')!=9007199254740993L){throw 'wrong wide integer'}
                if(value.getDecimal('ratio')!=0.25){throw 'wrong decimal'}
                if(not value.getBool('active')){throw 'wrong boolean'}
                if(value.getObject('address').getString('city')!='東京'){throw 'wrong nested object'}
                try {value.getInt('absent') throw 'null was lost'}catch(JsonError err){}
                i=i+1
            }
            if(store.has('deleted')){throw 'deleted record resurrected'}
            if(Json.toObject(store.get('updated') ?? '').getInt('version')!=2){throw 'overwrite was lost'}
            Console.printLine(i)
        }
        if(args[1]=='concurrent'){
            store.set('counter','0') store.set('winners','0')
            final a=this.work(store,0) final b=this.work(store,1)
            final c=this.work(store,2) final d=this.work(store,3)
            await a await b await c await d
        }
        if(args[1]=='concurrent' or args[1]=='verify-concurrent'){
            Console.printLine(store.get('counter') ?? 'missing')
            Console.printLine(store.get('winners') ?? 'missing')
            var worker=0
            for(worker<4){var i=0 for(i<250){if(Json.toObject(store.get('worker:${worker}:${i}') ?? '').getInt('id')!=i){throw 'concurrent record lost'} i=i+1} worker=worker+1}
        }
        store.close()
    }
}"#;
struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn stress_5000_records_and_1000_atomic_concurrent_writes_survive_reopen() {
    let program = Program::new(STRESS);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("stress.db");
    let path = path.to_str().unwrap();
    let written = program.run(&[path, "write"]);
    assert!(
        written.status.success(),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&written.stdout).lines().count(),
        5000
    );
    expect(program.run(&[path, "verify-all"]), "5000\n");
    expect(program.run(&[path, "concurrent"]), "1000\n1\n");
    expect(program.run(&[path, "verify-concurrent"]), "1000\n1\n");
    expect(program.run(&[path, "verify-all"]), "5000\n");
}
#[cfg(unix)]
#[test]
fn stress_three_sigkill_recoveries_preserve_every_acknowledged_object() {
    let program = Program::new(STRESS);
    let temp = tempfile::tempdir().unwrap();
    for round in 0..3 {
        let path = temp.path().join(format!("crash-{round}.db"));
        let mut writer = Running(
            Command::new(&program.executable)
                .args([path.to_str().unwrap(), "write"])
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let stdout = writer.0.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        for id in 0..1000 {
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(30)).unwrap(),
                id.to_string()
            );
        }
        writer.0.kill().unwrap();
        writer.0.wait().unwrap();
        expect(program.run(&[path.to_str().unwrap(), "verify"]), "1000\n");
    }
}
#[test]
fn automatic_json_and_store_signatures_are_checked() {
    for statement in [
        "KeyValueStore()",
        "KeyValueStore.open(1)",
        "KeyValueStore.get('key')",
        "KeyValueStore.open('a').set('key',1)",
        "KeyValueStore.open('a').get(1)",
        "KeyValueStore.open('a').set('k','{}',100)",
        "Json.toJson()",
        "Json.stringify()",
        "Json.stringify(1)",
        "Json.from(1)",
        "Json.parse('{}')",
        "Json.toObject()",
        "Json.toObject(1)",
        "Json.toObject('{}', 1)",
        "42.toJson(1)",
    ] {
        let source = format!("class App {{start(){{{statement}}}}}");
        let mut parser = Parser::new(Lexer::new(&source));
        let program = parser.parse_program();
        let mut checker = TypeChecker::new();
        assert!(
            !parser.diagnostics.is_empty() || !checker.check_program(&program),
            "accepted {statement}"
        );
    }
}

#[test]
fn native_nullable_scalars_and_injected_storage_failures_preserve_records() {
    let program = Program::new(
        r#"
    class Scalars(byte code,bool? flag,decimal? ratio,long? counter,byte? absent)
    class App {start(){
        byte zero
        Console.printLine(Scalars(zero,false,0.0,0L,null).toJson())
        final path=Console.args()[0]
        final store=KeyValueStore.open(path)
        store.set('original','{"version":1}')
        store.close()
        final sql=SQLite.open(path)
        sql.execute('CREATE TRIGGER deny_write BEFORE INSERT ON records BEGIN SELECT RAISE(ABORT, \'injected failure\'); END')
        sql.close()
        final failed=KeyValueStore.open(path)
        try {failed.set('original','{"version":2}')}catch(Error err){Console.printLine('write failure reported')}
        Console.printLine(failed.get('original') ?? 'missing')
        failed.close()
        final reopened=KeyValueStore.open(path)
        Console.printLine(reopened.get('original') ?? 'missing')
        reopened.close()
        try {final wrong=Cache.open(path,4096)}catch(Error err){Console.printLine('cache format rejected')}
        final buffered=KeyValueStore.openBuffered(Console.args()[1])
        buffered.set('value','[1,true,null,{"nested":"yes"}]') buffered.close()
        final read=KeyValueStore.open(Console.args()[1])
        Console.printLine(read.get('value') ?? 'missing') read.close()
    }}"#,
    );
    let temp = tempfile::tempdir().unwrap();
    let output = program.run(&[
        temp.path().join("failure.db").to_str().unwrap(),
        temp.path().join("buffered.db").to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    let value: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"code":0,"flag":false,"ratio":0.0,"counter":0,"absent":null})
    );
    assert_eq!(
        lines.collect::<Vec<_>>(),
        vec![
            "write failure reported",
            "{\"version\":1}",
            "{\"version\":1}",
            "cache format rejected",
            "[1,true,null,{\"nested\":\"yes\"}]"
        ]
    );
}

#[test]
fn native_lists_serialize_scalars_objects_nested_lists_and_reject_cycles() {
    let program = Program::new(r#"
    class Person(int id,string name)
    class Group(List people)
    class App {start(){
        Console.printLine([].toJson())
        Console.printLine([1,-2,9007199254740993L,true,false,0.25,'世界 \"quoted\"',null].toJson())
        final people=[Person(42,'Alice')]
        final group=Group(people)
        Gc.collect()
        Console.printLine(group.toJson())
        final values=[1]
        values.add('two')
        values.add([true,false])
        Console.printLine(values.toJson())
        final cycle=[]
        cycle.add(cycle)
        try {cycle.toJson()}catch(JsonError err){Console.printLine('cycle rejected')}
        final cache=Cache.memory(4096)
        try {[cache].toJson()}catch(JsonError err){Console.printLine('resource rejected')}
        cache.close()
        try {[Math.sqrt(-1)].toJson()}catch(JsonError err){Console.printLine('nonfinite rejected')}
        Console.printLine([3].toJson())
        final added=[]
        added.add(Person(7,'Bob'))
        Gc.collect()
        Console.printLine(added.toJson())
        final doubled=for(x in [1,2,3]){yield x*2}
        Console.printLine(doubled.toJson())
    }}"#);
    let output=program.run(&[]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout=String::from_utf8_lossy(&output.stdout);
    let lines:Vec<_>=stdout.lines().collect();
    assert_eq!(serde_json::from_str::<serde_json::Value>(lines[0]).unwrap(),serde_json::json!([]));
    assert_eq!(serde_json::from_str::<serde_json::Value>(lines[1]).unwrap(),serde_json::json!([1,-2,9007199254740993i64,true,false,0.25,"世界 \"quoted\"",null]));
    assert_eq!(serde_json::from_str::<serde_json::Value>(lines[2]).unwrap(),serde_json::json!({"people":[{"id":42,"name":"Alice"}]}));
    assert_eq!(lines[3], "[1,\"two\",[true,false]]");
    assert_eq!(&lines[4..], &["cycle rejected","resource rejected","nonfinite rejected","[3]","[{\"id\":7,\"name\":\"Bob\"}]","[2,4,6]"]);
}

#[test]
fn native_maps_serialize_nested_values_and_validate_keys() {
    let program = Program::new(r#"
    class Person(int id,string name)
    class Record(Map data)
    class App {start(){
        final data=['person':Person(42,'世界'),'values':[1,true,null],'nested':['ok':false],'large':9007199254740993L,'ratio':0.25]
        Gc.collect()
        Console.printLine(Record(data).toJson())
        Console.printLine([1000L:95,1001L:88].toJson())
        Console.printLine([true:'yes',false:'no'].toJson())
        Console.printLine([0.25:'fraction'].toJson())
        Console.printLine([['name':'Alice'],['name':'Bob']].toJson())
        final children=[]
        final cycle=['children':children]
        children.add(cycle)
        try {cycle.toJson()}catch(JsonError err){Console.printLine('cycle rejected')}
        try {[1:'number','1':'string'].toJson()}catch(JsonError err){Console.printLine('collision rejected')}
        try {[Person(1,'key'):'bad'].toJson()}catch(JsonError err){Console.printLine('object key rejected')}
        try {[null:'bad'].toJson()}catch(JsonError err){Console.printLine('null key rejected')}
        try {[Math.sqrt(-1):'bad'].toJson()}catch(JsonError err){Console.printLine('nonfinite key rejected')}
        try {['bad':Math.sqrt(-1)].toJson()}catch(JsonError err){Console.printLine('nonfinite value rejected')}
        final cache=Cache.memory(4096)
        try {['cache':cache].toJson()}catch(JsonError err){Console.printLine('resource rejected')}
        cache.close()
        Console.printLine(['after':'error'].toJson())
    }}"#);
    let output=program.run(&[]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout=String::from_utf8_lossy(&output.stdout);
    let lines:Vec<_>=stdout.lines().collect();
    let expected=[
        serde_json::json!({"data":{"person":{"id":42,"name":"世界"},"values":[1,true,null],"nested":{"ok":false},"large":9007199254740993i64,"ratio":0.25}}),
        serde_json::json!({"1000":95,"1001":88}),
        serde_json::json!({"true":"yes","false":"no"}),
        serde_json::json!({"0.25":"fraction"}),
        serde_json::json!([{"name":"Alice"},{"name":"Bob"}]),
    ];
    for (line, expected) in lines.iter().zip(expected) {
        assert_eq!(serde_json::from_str::<serde_json::Value>(line).unwrap(), expected);
    }
    assert_eq!(&lines[5..], &["cycle rejected","collision rejected","object key rejected","null key rejected","nonfinite key rejected","nonfinite value rejected","resource rejected","{\"after\":\"error\"}"]);
}
