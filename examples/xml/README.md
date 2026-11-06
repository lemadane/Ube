# XML example

Build the matching runtime and compiler from the repository root, then run:

```bash
cargo build -p ube-runtime -p ube-compiler
./target/debug/ube-compiler run examples/xml/src/Main.ube
```

Expected output:

```text
<Profile><id>42</id><name>Alice &amp; Bob</name><scores><item>95</item><item>88</item></scores></Profile>
Alice & Bob
88
p:person
urn:people
42
<map><entry key="display name">Alice</entry></map>
```

See the [XML reference](../../docs/xml.md) for nulls, lists/maps, namespaces, attributes, parser behavior, and limits. `Xml.toObject` returns a document; it does not recreate a `Profile` instance.

The automated end-to-end suite checks this project's `run` and `build` output in an isolated directory and tests XML files exchanged between separate native processes:

```bash
cargo test -p ube-compiler --test xml_e2e
```

## Tests written in Ube

From the repository root, run the 12 Testicle tests:

```bash
./target/debug/ube-compiler test examples/xml/tests
```

Or run `ube test` from `examples/xml` with the matching compiler installed. [XmlTests.ube](tests/XmlTests.ube) asserts object/list/map encoding, nulls, escaping, namespaces, document edits, GC handle survival, typed errors, cycles/resources, and async calls. Three Mockery tests inject an XML source into a consumer and verify successful parsing, malformed input, source failures, and call counts. The parser and serializer remain real implementations.

The Cargo XML suite also runs these checked-in Ube tests in an isolated project, so the workspace test command exercises the Testicle suite automatically.
