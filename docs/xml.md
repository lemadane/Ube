# XML serialization and documents

Objects, lists, maps, and supported scalar values provide `.toXml()`. `Xml.toObject(text)` parses XML into a real `XmlDocument`; it does not recreate an application class automatically.

```ube
// Profile.ube
class Profile(
  int id,
  string name)
```

```ube
// Inside a method importing Profile:
final profile = Profile(
  42,
  'Alice & Bob')
final xml = profile.toXml()
// <Profile><id>42</id><name>Alice &amp; Bob</name></Profile>

final document = Xml.toObject(xml)
Console.printLine(
  document.root()
    .child('name')
    .text())
// Alice & Bob
```

See the [runnable example](../examples/xml/README.md).

## Default mapping

| Value | XML representation |
| --- | --- |
| Concrete class instance | Root named after its class; declared constructor fields become child elements |
| Nested data object | Its fields appear inside the containing field element |
| List | `<list>` root with one `<item>` child per value |
| Map | `<map>` root with one `<entry key="…">` child per entry |
| Scalar | `<value>` root containing escaped text |
| Null | An empty element with `xsi:nil="true"`; `xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"` is declared on the root |
| Empty list, map, or object | Empty container element |
| Empty string | Empty text content, without `xsi:nil` |

For example:

```ube
Console.printLine(
  [1, 'two', false].toXml())
// <list><item>1</item><item>two</item><item>false</item></list>

Console.printLine(
  ['display name': 'Alice'].toXml())
// <map><entry key="display name">Alice</entry></map>
```

Map keys stay attribute values, so they do not need to be valid XML tag names. String keys remain unchanged; numeric and boolean keys convert to strings using the same rules as JSON encoding. Null, object, collection, and resource keys are rejected, as are collisions after conversion. Map entry order and XML attribute order are not contracts.

Nested lists and maps retain containers: a list field contains item elements; a map field contains entry elements. An entry's value becomes its text, object fields, item children, or nested entry children, depending on its value. Empty containers and scalar types are not annotated with Ube type metadata. XML text alone cannot recover the original Ube types or distinguish every empty value without an application schema.

Fields include private constructor fields. Methods and computed getters are not called, and secrets are not automatically redacted. Repeated object references serialize as repeated values. Cycles are rejected. Serialization does not provide an atomic snapshot across a mutable object graph; coordinate concurrent mutations if needed.

## Optional customization

Create reusable `XmlOptions` and pass them to `.toXml(options)`:

```ube
final options = XmlOptions()
  .root('p:person')
  .namespace(
    'p',
    'urn:example:people')
  .attribute('id')
  .rename(
    'name',
    'p:displayName')

final xml = Profile(
  42,
  'Alice')
  .toXml(options)
```

This produces a namespaced `p:person` root, an unqualified `id="42"` attribute, and a `p:displayName` child containing `Alice`.

| Option | Meaning |
| --- | --- |
| `root(string qualifiedName)` | Override the root name |
| `namespace(string prefix, string uri)` | Declare a namespace on the root; an empty prefix selects the default namespace |
| `attribute(string field)` | Serialize that root object's field as an attribute |
| `rename(string field, string qualifiedName)` | Rename that root object's field, whether it becomes an attribute or element |
| `listItem(string qualifiedName)` | Override the item name for lists, including nested lists |

All methods return the options handle for chaining. Field rules apply only to the root data class. Nested objects use their defaults. Unknown root fields and duplicate expanded output field names raise `XmlError`. The serializer also reserves the XML Schema instance `nil` attribute for actual null values. Field rules cannot be applied to list, map, or scalar roots.

Attributes accept non-null strings, numbers, and booleans. Nulls and compound values cannot be represented as attributes and raise `XmlError`; keep them as child elements. Namespace declarations are configured with `namespace`, not mapped from fields.

Unprefixed elements inherit the default namespace. Unprefixed attributes have no namespace, as required by XML. Every used prefix must be bound when the document is serialized. The `xml` prefix has its standard fixed URI; `xmlns` is reserved for declarations; `xsi` is reserved by the serializer for the XML Schema instance namespace.

`.toXml()` is reserved for the automatic serializer; source methods and constructor fields with that name are rejected. Object layouts are unchanged; serialization uses shared compiler-provided metadata and produces output on demand.

## Parsing and traversal

```ube
final document = Xml.toObject(
  '<users><user id="42">Alice</user><user id="7">Bob</user></users>')
final root = document.root()
Console.printLine(
  root.childCount('user'))         // 2
Console.printLine(
  root.childAt(
    'user',
    1)
    .text())   // Bob
Console.printLine(
  root.child('user')
    .getAttribute('id')) // 42
```

`Xml.toObject` accepts one non-null UTF-8 string and returns an `XmlDocument`. Parsed values are document/element handles, not reconstructed Ube objects, lists, or maps. Construct application classes explicitly from the document when needed.

| API | Result |
| --- | --- |
| `document.root()` | Root `XmlElement` |
| `document.toXml()` / `document.toString()` | Actual serialized XML |
| `element.name()` | Qualified element name |
| `element.namespaceUri()` | Namespace URI, or an empty string for no namespace |
| `element.child(name)` | First matching child; missing children raise an error |
| `element.childAt(name, index)` | Matching child at a zero-based index |
| `element.childCount(name)` | Number of matching children |
| `element.childNs(uri, localName)` | First child matching an expanded namespace/local name, independent of prefix |
| `element.text()` / `element.getText()` | Descendant text content, including mixed content |
| `element.getAttribute(name)` | Attribute value; missing attributes raise an error |
| `element.isNull()` | Whether an attribute in the XML Schema instance namespace declares `nil="true"` or `nil="1"` |
| `element.toXml()` | Standalone XML subtree, including inherited namespace declarations |

Child name arguments use qualified names; `'*'` matches every child element. Indexes that are negative or out of bounds raise `XmlError`. XML text is returned as text; it is not silently converted into numeric or boolean Ube fields.

Document serialization preserves element/text order and namespace semantics. It can normalize empty tags, attribute order, namespace declaration placement, and quoting. Comments, processing instructions, and the XML declaration are not retained. Attribute and text escaping preserve Unicode, ampersands, angle brackets, and significant carriage returns; attribute whitespace is escaped so it survives XML normalization.

Document and element handles accept `.toXml()` with no options. Use document editing for explicit XML structures.

## Building documents explicitly

```ube
final document = Xml.create('p:user')
final root = document.root()
root.namespace(
  'p',
  'urn:example:people')
  .attribute(
    'id',
    '42')
root.element('p:name')
  .text('Alice & Bob')
Console.printLine(
  document.toXml())
```

`element(name)` appends a child, `text(value)` replaces an element's content, `attribute(name, value)` sets/replaces an attribute, and `namespace(prefix, uri)` sets a namespace declaration. These methods return element handles for chaining. Bind all prefixes before serializing; malformed namespace combinations are rejected by serialization.

Compatibility APIs remain available: `Xml.parse(text)` aliases `Xml.toObject(text)`; `Xml.stringify(document)` and `Xml.from(document)` serialize an XML document. `Xml.partialParse(text, XmlDocument)` and `Xml.to(text, XmlDocument)` are parsing aliases only; they do not perform partial parsing or typed application reconstruction.

## Errors and limits

- Invalid XML, malformed tags, duplicate expanded attributes, unknown entities, unbound prefixes, and DTDs raise `XmlParseError` during parsing. DTD and external entity resolution are disabled.
- Invalid names, namespace edits, missing fields/children, invalid handles, unsupported types, cycles, non-finite decimals, and serialization limits raise `XmlError`.
- XML 1.0 forbidden characters are rejected rather than silently removed or emitted as invalid character references.
- Input, stored document text, and encoded output are bounded to 16 MiB; nesting is limited to 64 levels and traversal/document size to 100,000 nodes. Namespace expansion into owned documents counts against the storage budget. Names are limited to 1,024 UTF-8 bytes.
- Runtime resources and anonymous/function layouts cannot be automatically encoded. Nullable scalar elements in ordinary collections retain the existing compiler restriction. Parsed JSON documents can be encoded using the map/list/scalar rules; XML handles are serialized directly and are not supported as fields inside automatically encoded data objects.
- Documents, element handles, options, and their detached builder nodes remain owned until runtime cleanup. There is no per-document disposal API yet. Limits are per operation/document and are not a hard process-memory budget.

The shared data traversal enforces the same scalar layout, reference synchronization, rooting, and collection type-metadata rules as JSON. See [JSON](json.md) for those rules.

## Validation

The [Ube Testicle suite](../examples/xml/tests/XmlTests.ube) contains 12 tests with Ube assertions, including Mockery-backed XML consumer tests. Run `./target/debug/ube-compiler test examples/xml/tests` from the repository root, or `ube test` from `examples/xml`. The Cargo suite also runs these same source files in an isolated project.

The [native end-to-end tests](../compiler/tests/xml_e2e.rs) compile and run Ube programs against the real runtime. They cover objects, nested lists/maps, nulls, customization, namespace traversal, builder edits, XML errors, unsupported values, and API validation. Automated CLI tests copy and run/build the shipped example against its documented output. Independent writer and reader executables also exchange an XML file, verify fresh-process round trips, and reject corrupted files. Native fixture tests also cover invalid numeric references, CDATA, mixed content, namespace shadowing, detached subtrees, attribute whitespace, live handle edits after GC, and byte/depth/node limits. Encoder limit failures and recovery run with both the default build and LLVM `-O2`. Runtime tests additionally cover Unicode/escaping, DTD rejection, malformed names, resource limits, and namespace aliases. The JSON persistence suite checks compatibility after sharing typed traversal.

```bash
cargo build -p ube-runtime
cargo test -p ube-runtime xml::tests
cargo test -p ube-compiler --test xml_e2e --test key_value_store_e2e
```
