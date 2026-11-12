//! The bounded XML tree reader, strict numbers and the escaping writer
//! (plan 6d Task 2): hostile texts (document types, entities, deep nesting,
//! too many parts or attributes, other encodings, broken markup) are refused
//! with their sentence, bounded and never a panic; numbers are XSD-strict;
//! what the writer makes reads back equal.

mod common;

use common::{CountingAlloc, cases, measure};
use core_io::IoError;
use core_io::limits::{MAX_TEXT_BYTES, PackageLimits};
use core_io::xml::{
    Attr, NodeId, Tree, XmlWriter, boolean, double, double_or_inf, fmt_double, int,
};
use proptest::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const CHECK_EVERY: u64 = 65_536;

fn parse(text: &[u8]) -> Result<Tree, IoError> {
    Tree::parse(text, &PackageLimits::default(), &AtomicBool::new(false))
}

fn parse_with(text: &[u8], limits: &PackageLimits) -> Result<Tree, IoError> {
    Tree::parse(text, limits, &AtomicBool::new(false))
}

/// The refusal's sentence, or a failure naming what came instead.
fn refusal<T>(r: Result<T, IoError>) -> String {
    match r {
        Err(IoError::Refused(s)) => s,
        Err(e) => panic!("expected a refusal, got {e:?}"),
        Ok(_) => panic!("expected a refusal, got Ok"),
    }
}

fn damaged(why: &str) -> String {
    format!("This DAWproject file is damaged: {why}.")
}

fn kids(t: &Tree, n: NodeId) -> Vec<NodeId> {
    t.children(n).collect()
}

fn names(t: &Tree, n: NodeId) -> Vec<String> {
    t.children(n).map(|c| t.name(c).to_owned()).collect()
}

const SMALL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<!-- written by hand -->
<Project version="1.0">
  <?vizdaw note?>
  <Application name="Test" version="2"/>
  <Transport>
    <Tempo unit="bpm" value="120" id="tempo"/>
    <TimeSignature numerator="4" denominator="4"/>
  </Transport>
  <Structure>
    <Track contentType="notes" name="Piano" id="t1">
      <Channel role="regular" id="c1"/>
    </Track>
    <Track contentType="audio" name="Drums &amp; Bass" id="t2"/>
  </Structure>
  <Title>A &amp; B</Title>
  <Comment><![CDATA[x < y & z]]></Comment>
  <x:Odd xmlns:x="urn:unknown" x:attr="1"/>
  <Empty></Empty>
</Project>
"#;

#[test]
fn parses_a_small_project() {
    let t = parse(SMALL.as_bytes()).unwrap();
    let root = t.root();
    assert_eq!(t.name(root), "Project");
    assert_eq!(t.attr(root, "version"), Some("1.0"));
    assert_eq!(t.attr(root, "missing"), None);
    assert_eq!(
        names(&t, root),
        [
            "Application",
            "Transport",
            "Structure",
            "Title",
            "Comment",
            "x:Odd",
            "Empty"
        ]
    );
    let top = kids(&t, root);
    assert_eq!(t.attr(top[0], "name"), Some("Test"));
    assert_eq!(names(&t, top[1]), ["Tempo", "TimeSignature"]);
    let tempo = kids(&t, top[1])[0];
    assert_eq!(t.attr(tempo, "unit"), Some("bpm"));
    assert_eq!(t.attr(tempo, "value"), Some("120"));
    assert_eq!(t.attr(tempo, "id"), Some("tempo"));
    let tracks = kids(&t, top[2]);
    assert_eq!(t.attr(tracks[1], "name"), Some("Drums & Bass"));
    assert_eq!(names(&t, tracks[0]), ["Channel"]);
    assert!(kids(&t, tracks[1]).is_empty());
    assert_eq!(t.text(top[3]), "A & B");
    assert_eq!(t.text(top[4]), "x < y & z");
    assert_eq!(t.attr(top[5], "x:attr"), Some("1"));
    assert_eq!(t.text(top[6]), "");
    // Elements with children keep no text (their indentation).
    assert_eq!(t.text(root), "");
    assert_eq!(t.len(), 13);
}

const BILLION_LAUGHS: &str = r#"<?xml version="1.0"?>
<!DOCTYPE lolz [
 <!ENTITY lol "lol">
 <!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
 <!ENTITY lol3 "&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;">
 <!ENTITY lol4 "&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;">
 <!ENTITY lol5 "&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;">
 <!ENTITY lol6 "&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;">
 <!ENTITY lol7 "&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;">
 <!ENTITY lol8 "&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;">
 <!ENTITY lol9 "&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;">
]>
<lolz>&lol9;</lolz>
"#;

#[test]
fn doctype_and_entities_are_refused() {
    let doctype = damaged("it declares a document type");
    let entity = damaged("it uses an unknown entity");
    assert_eq!(refusal(parse(BILLION_LAUGHS.as_bytes())), doctype);
    // An external entity, and a bare document type.
    let external = r#"<!DOCTYPE a [<!ENTITY x SYSTEM "file:///etc/passwd">]><a>&x;</a>"#;
    assert_eq!(refusal(parse(external.as_bytes())), doctype);
    assert_eq!(refusal(parse(b"<!DOCTYPE a><a/>")), doctype);
    assert_eq!(refusal(parse(b"<a>&foo;</a>")), entity);
    assert_eq!(refusal(parse(br#"<a x="&foo;"/>"#)), entity);
    let t = parse(b"<a>&#x41;&amp;</a>").unwrap();
    assert_eq!(t.text(t.root()), "A&");
    let t = parse(br#"<a x="&#65;&lt;&gt;&quot;&apos;&#x1F3B9;"/>"#).unwrap();
    assert_eq!(t.attr(t.root(), "x"), Some("A<>\"'\u{1F3B9}"));
}

fn nested(depth: usize) -> String {
    format!("{}{}", "<a>".repeat(depth), "</a>".repeat(depth))
}

fn with_attrs(n: usize) -> String {
    let attrs: String = (0..n).map(|i| format!(" a{i}=\"{i}\"")).collect();
    format!("<e{attrs}/>")
}

#[test]
fn nesting_and_size_limits() {
    let t = parse(nested(256).as_bytes()).unwrap();
    assert_eq!(t.len(), 256);
    assert_eq!(
        refusal(parse(nested(257).as_bytes())),
        damaged("it is nested too deeply")
    );
    // Deep nesting far past the limit is refused, not recursed into.
    assert_eq!(
        refusal(parse(nested(100_000).as_bytes())),
        damaged("it is nested too deeply")
    );

    let limits = PackageLimits {
        max_nodes: 100,
        ..PackageLimits::default()
    };
    let doc = |n: usize| format!("<r>{}</r>", "<a/>".repeat(n - 1));
    assert_eq!(parse_with(doc(100).as_bytes(), &limits).unwrap().len(), 100);
    assert_eq!(
        refusal(parse_with(doc(101).as_bytes(), &limits)),
        damaged("it has too many parts")
    );

    let t = parse(with_attrs(64).as_bytes()).unwrap();
    assert_eq!(t.attr(t.root(), "a63"), Some("63"));
    assert_eq!(
        refusal(parse(with_attrs(65).as_bytes())),
        damaged("an element has too many attributes")
    );

    // An attribute of 64 KiB is read; one byte more is refused.
    let attr = |n: usize| format!("<a v=\"{}\"/>", "x".repeat(n));
    let t = parse(attr(MAX_TEXT_BYTES).as_bytes()).unwrap();
    assert_eq!(t.attr(t.root(), "v").map(str::len), Some(MAX_TEXT_BYTES));
    assert_eq!(
        refusal(parse(attr(MAX_TEXT_BYTES + 1).as_bytes())),
        damaged("an attribute is longer than 64 KiB")
    );
    // A huge attribute is refused without holding a copy of it.
    let huge = attr(32 * 1024 * 1024);
    let (r, seen) = measure(|| parse(huge.as_bytes()));
    assert_eq!(refusal(r), damaged("an attribute is longer than 64 KiB"));
    assert!(seen.peak < 1024 * 1024, "peak {}", seen.peak);

    // Text is kept up to 64 KiB, cut at a character boundary.
    let long = format!("<a>{}</a>", "é".repeat(MAX_TEXT_BYTES));
    let t = parse(long.as_bytes()).unwrap();
    let kept = t.text(t.root());
    assert!(kept.len() <= MAX_TEXT_BYTES && kept.len() > MAX_TEXT_BYTES - 2);
    assert!(kept.chars().all(|c| c == 'é'));
    assert!(t.text_cut(t.root()));

    // The tree's bytes are counted as it grows: past the budget it is
    // refused, and the parse never holds much more than the budget.
    let limits = PackageLimits {
        max_tree_bytes: 1024 * 1024,
        ..PackageLimits::default()
    };
    let many = format!("<r>{}</r>", "<Note key=\"60\" time=\"1\"/>".repeat(200_000));
    let (r, seen) = measure(|| parse_with(many.as_bytes(), &limits));
    assert_eq!(refusal(r), damaged("it has too many parts"));
    assert!(seen.peak < 2 * 1024 * 1024, "peak {}", seen.peak);
    let few = format!("<r>{}</r>", "<Note key=\"60\" time=\"1\"/>".repeat(1_000));
    assert_eq!(parse_with(few.as_bytes(), &limits).unwrap().len(), 1_001);

    // A huge text node of CRs (end-of-line normalised) is kept up to 64 KiB
    // without a copy of the whole node; so is one in CDATA, and one thrown
    // away in an element with children.
    let crs = "\r".repeat(32 * 1024 * 1024);
    for text in [
        format!("<a>{crs}</a>"),
        format!("<a><![CDATA[{crs}]]></a>"),
        format!("<a><b/>{crs}</a>"),
        format!("{crs}<a/>{crs}"),
    ] {
        let (r, seen) = measure(|| parse(text.as_bytes()));
        let t = r.unwrap();
        assert!(seen.peak < 1024 * 1024, "peak {}", seen.peak);
        let kept = t.text(t.root());
        assert!(
            kept.is_empty() || (kept.len() == MAX_TEXT_BYTES && kept.bytes().all(|b| b == b'\n'))
        );
    }

    // Element and attribute names over 1 KiB are refused before the parser
    // copies them.
    let broken = damaged("its XML is broken");
    let name = "n".repeat(64 * 1024 * 1024);
    for text in [
        format!("<{name}/>"),
        format!("<{name}>"),
        format!("<a></{name}>"),
        format!("<a {name}=\"1\"/>"),
    ] {
        let (r, seen) = measure(|| parse(text.as_bytes()));
        assert_eq!(refusal(r), broken);
        assert!(seen.peak < 1024 * 1024, "peak {}", seen.peak);
    }
    let n1k = "n".repeat(1024);
    let t = parse(format!("<{n1k} {n1k}=\"v\"></{n1k}>").as_bytes()).unwrap();
    assert_eq!(t.attr(t.root(), &n1k), Some("v"));
    let n1k1 = "n".repeat(1025);
    assert_eq!(refusal(parse(format!("<{n1k1}/>").as_bytes())), broken);
    assert_eq!(
        refusal(parse(format!("<a {n1k1}=\"v\"/>").as_bytes())),
        broken
    );
    // Long attribute values are not names.
    let t = parse(format!("<a v=\"{}\" w='{}'/>", "x".repeat(4096), "y".repeat(4096)).as_bytes())
        .unwrap();
    assert_eq!(t.attr(t.root(), "w").map(str::len), Some(4096));
}

#[test]
fn cut_and_dropped_text_are_recorded() {
    // Exactly 64 KiB is not cut; one byte more is.
    let at = |n: usize| format!("<a>{}</a>", "x".repeat(n));
    let t = parse(at(MAX_TEXT_BYTES).as_bytes()).unwrap();
    assert_eq!(t.text(t.root()).len(), MAX_TEXT_BYTES);
    assert!(!t.text_cut(t.root()));
    let t = parse(at(MAX_TEXT_BYTES + 1).as_bytes()).unwrap();
    assert!(t.text_cut(t.root()));
    // A cut made by a reference past the limit.
    let t = parse(format!("<a>{}&amp;</a>", "x".repeat(MAX_TEXT_BYTES)).as_bytes()).unwrap();
    assert!(t.text_cut(t.root()));
    assert_eq!(t.dropped_text(), 0);

    // Layout whitespace is nothing; mixed-content text is counted by run.
    let t = parse(SMALL.as_bytes()).unwrap();
    assert_eq!(t.dropped_text(), 0);
    assert!(!t.text_cut(t.root()));
    let t = parse(b"<Title>A<b/>B &amp; C<c/>  <d/>&#32;<![CDATA[D]]></Title>").unwrap();
    assert_eq!(t.dropped_text(), 3);
    assert_eq!(t.text(t.root()), "");
    assert!(!t.text_cut(t.root()));
}

#[test]
fn encodings() {
    let not_utf8 = damaged("it is not UTF-8");
    let t = parse(b"\xEF\xBB\xBF<a>x</a>").unwrap();
    assert_eq!(t.text(t.root()), "x");
    let t = parse(b"\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<a/>").unwrap();
    assert_eq!(t.name(t.root()), "a");
    assert!(parse(b"<?xml version='1.0' encoding='Utf-8'?><a/>").is_ok());
    assert_eq!(
        refusal(parse(b"<?xml version=\"1.0\" encoding=\"UTF-16\"?><a/>")),
        not_utf8
    );
    assert_eq!(
        refusal(parse(
            b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a/>"
        )),
        not_utf8
    );
    assert_eq!(refusal(parse(b"\xFF\xFE<\0a\0/\0>\0")), not_utf8);
    assert_eq!(refusal(parse(b"\xFE\xFF\0<\0a\0/\0>")), not_utf8);
    assert_eq!(refusal(parse(b"<a>x\xFFy</a>")), not_utf8);
    assert_eq!(refusal(parse(b"<a v=\"\xC3\"/>")), not_utf8);
}

#[test]
fn numbers_are_strict() {
    assert_eq!(double("1.5e3"), Some(1500.0));
    assert_eq!(double(".5"), Some(0.5));
    assert_eq!(double("5."), Some(5.0));
    assert_eq!(double(" 2 "), Some(2.0));
    assert_eq!(double("+3E-1"), Some(0.3));
    // Underflow rounds to zero, as XSD allows.
    assert_eq!(double("1e-999"), Some(0.0));
    let neg_zero = double("-0").unwrap();
    assert!(neg_zero == 0.0 && neg_zero.is_sign_negative());
    for bad in [
        "NaN", "INF", "inf", "-INF", "1e400", "-1e400", "0x10", "1_0", "1,5", "", " ", ".", "e5",
        "1e", "1e+", "--1", "1.2.3", "１",
    ] {
        assert_eq!(double(bad), None, "{bad:?}");
    }
    let digits = |n: usize| "1".repeat(n);
    assert!(double(&digits(64)).is_some());
    assert_eq!(double(&digits(65)), None);

    assert_eq!(double_or_inf("-inf"), Some(f64::NEG_INFINITY));
    assert_eq!(double_or_inf("inf"), Some(f64::INFINITY));
    assert_eq!(double_or_inf("INF"), Some(f64::INFINITY));
    assert_eq!(double_or_inf("-INF"), Some(f64::NEG_INFINITY));
    assert_eq!(double_or_inf("2.5"), Some(2.5));
    assert_eq!(double_or_inf("NaN"), None);
    assert_eq!(double_or_inf("Infinity"), None);

    assert_eq!(int("2147483647"), Some(i32::MAX));
    assert_eq!(int("-2147483648"), Some(i32::MIN));
    assert_eq!(int("+7"), Some(7));
    assert_eq!(int(" 12 "), Some(12));
    for bad in [
        "2147483648",
        "-2147483649",
        "1.0",
        "",
        "-",
        "0x1",
        "1e3",
        "١",
    ] {
        assert_eq!(int(bad), None, "{bad:?}");
    }
    assert_eq!(int(&format!("{}1", "0".repeat(64))), None);

    assert_eq!(boolean("true"), Some(true));
    assert_eq!(boolean("false"), Some(false));
    assert_eq!(boolean("1"), Some(true));
    assert_eq!(boolean("0"), Some(false));
    for bad in ["yes", "TRUE", "", "2"] {
        assert_eq!(boolean(bad), None, "{bad:?}");
    }
}

#[test]
fn structure_errors_are_refused() {
    let broken = damaged("its XML is broken");
    for bad in [
        &b"<a></b>"[..],
        b"<a>",
        b"<a/><b/>",
        b"<a/>x",
        b"",
        b"   ",
        b"text<a/>",
        b"</a>",
        b"<a",
        b"<a b=1/>",
        b"<a>&</a>",
        b"<a>&#0;</a>",
    ] {
        assert_eq!(
            refusal(parse(bad)),
            broken,
            "{:?}",
            String::from_utf8_lossy(bad)
        );
    }
    assert_eq!(
        refusal(parse(br#"<a x="1" x="2"/>"#)),
        damaged("an element repeats an attribute")
    );
    // A declaration anywhere but first.
    for bad in [
        &b"<a/><?xml version=\"1.0\"?>"[..],
        b"<!-- c --><?xml version=\"1.0\"?><a/>",
        b" <?xml version=\"1.0\"?><a/>",
        b"<?xml version=\"1.0\"?><?xml version=\"1.0\"?><a/>",
    ] {
        assert_eq!(
            refusal(parse(bad)),
            broken,
            "{:?}",
            String::from_utf8_lossy(bad)
        );
    }
    // Comments and whitespace after the root are fine.
    assert!(parse(b"<a/>\n<!-- end -->\n").is_ok());
}

#[test]
fn writer_escapes_and_round_trips() {
    let hostile = "<>&\"'\t\n\u{1}\u{202E}🎹\u{FFFE}\u{FFFF}";
    let expect = "<>&\"'\t\n\u{202E}🎹";
    let mut w = XmlWriter::new();
    w.open("Project", &[("version", Attr::Str("1.0"))]);
    w.empty(
        "Track",
        &[
            ("name", Attr::Str(hostile)),
            ("volume", Attr::F64(0.5)),
            ("count", Attr::I64(-3)),
            ("loud", Attr::Bool(true)),
        ],
    );
    w.text_element("Title", hostile);
    w.close();
    let out = w.finish();
    let t = parse(out.as_bytes()).unwrap();
    let kids = kids(&t, t.root());
    assert_eq!(t.attr(kids[0], "name"), Some(expect));
    assert_eq!(t.text(kids[1]), expect);
    assert_eq!(
        out,
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Project version=\"1.0\">\n  \
         <Track name=\"&lt;&gt;&amp;&quot;'&#9;&#10;\u{202E}🎹\" volume=\"0.5\" count=\"-3\" loud=\"true\"/>\n  \
         <Title>&lt;&gt;&amp;\"'\t\n\u{202E}🎹</Title>\n\
         </Project>\n"
    );

    // A carriage return in an attribute survives the reader's normalising.
    let mut w = XmlWriter::new();
    w.empty("a", &[("v", Attr::Str("x\r\ny"))]);
    let out = w.finish();
    assert!(out.contains("x&#13;&#10;y"));
    let t = parse(out.as_bytes()).unwrap();
    assert_eq!(t.attr(t.root(), "v"), Some("x\r\ny"));

    // Deterministic, and nested indentation is two spaces a level.
    let make = || {
        let mut w = XmlWriter::new();
        w.open("a", &[]);
        w.open("b", &[("k", Attr::F64(-0.0))]);
        w.empty("c", &[]);
        w.close();
        w.close();
        w.finish()
    };
    assert_eq!(make(), make());
    assert!(make().ends_with("<a>\n  <b k=\"0\">\n    <c/>\n  </b>\n</a>\n"));
}

fn round_trips(v: f64) {
    let s = fmt_double(v);
    assert!(s.len() <= 64, "{s}");
    assert_eq!(
        double(&s).map(f64::to_bits),
        Some(v.to_bits()),
        "{v} as {s}"
    );
}

#[test]
fn fmt_double_round_trips() {
    for v in [
        1.0 / 3.0,
        0.1,
        1e-300,
        9_007_199_254_740_992.0,
        65_535.0 / 65_535.0,
        f64::MAX,
        f64::MIN_POSITIVE,
        5e-324,
        -2.5,
        1e21,
        1e-7,
    ] {
        round_trips(v);
    }
    assert_eq!(fmt_double(15_360.0 / 15_360.0), "1");
    assert_eq!(fmt_double(-0.0), "0");
    assert_eq!(fmt_double(0.5), "0.5");
    assert_eq!(fmt_double(1000.0), "1000");
    assert_eq!(fmt_double(1e-300), "1e-300");
    for n in 0..=65_535u32 {
        round_trips(f64::from(n) / 65_535.0);
    }
}

fn big_text(elements: usize) -> String {
    let mut s = String::with_capacity(elements * 12 + 16);
    s.push_str("<r>");
    for _ in 1..elements {
        s.push_str("<n v=\"1\"/>");
    }
    s.push_str("</r>");
    s
}

#[test]
fn parse_stops_when_cancelled() {
    let text = big_text(300_000);
    // A flag already set: Cancelled before the first element.
    let set = AtomicBool::new(true);
    let mut checks = Vec::new();
    let r = Tree::debug_parse_observed(
        text.as_bytes(),
        &PackageLimits::default(),
        &set,
        &mut |events| checks.push(events),
    );
    assert_eq!(r.err(), Some(IoError::Cancelled));
    assert_eq!(checks, [0]);

    // A flag set from a second thread once the parse is under way: the
    // first check past the start hands over to the thread and waits for it
    // to set the flag, so the parse sees it within one check interval.
    let cancel = AtomicBool::new(false);
    let (go, wait_go) = mpsc::channel::<()>();
    let (done, wait_done) = mpsc::channel::<()>();
    let mut set_at = None;
    let mut last = 0;
    let r = std::thread::scope(|s| {
        let cancel = &cancel;
        s.spawn(move || {
            wait_go.recv().unwrap();
            cancel.store(true, Ordering::Relaxed);
            done.send(()).unwrap();
        });
        Tree::debug_parse_observed(
            text.as_bytes(),
            &PackageLimits::default(),
            cancel,
            &mut |events| {
                last = events;
                if events > 0 && set_at.is_none() {
                    set_at = Some(events);
                    go.send(()).unwrap();
                    wait_done.recv().unwrap();
                }
            },
        )
    });
    assert_eq!(r.err(), Some(IoError::Cancelled));
    let set_at = set_at.unwrap();
    assert!(last - set_at <= CHECK_EVERY, "{set_at} then {last}");
    assert_eq!(set_at, CHECK_EVERY);
    // Untouched, the same text parses.
    assert_eq!(parse(text.as_bytes()).unwrap().len(), 300_000);
}

const NAMES: &[&str] = &[
    "Project",
    "Application",
    "Transport",
    "Tempo",
    "Structure",
    "Track",
    "Channel",
    "Arrangement",
    "Lanes",
    "Clips",
    "Clip",
    "Notes",
    "Note",
    "Points",
    "RealPoint",
    "MetaData",
    "Title",
    "x:Odd",
];

const ATTR_VALUES: &[&str] = &[
    "1",
    "1e999",
    "NaN",
    "-0",
    "0x10",
    "&amp;",
    "&lt;",
    "&foo;",
    "&#65;",
    "&#0;",
    "a<b",
    "&",
    "\"",
    "&#xFFFFFFFF;",
];

const FRAGMENTS: &[&str] = &[
    "<!DOCTYPE a [<!ENTITY x \"y\">]>",
    "<!ENTITY lol \"lol\">",
    "]>",
    "&lol;",
    "&amp;",
    "<![CDATA[ <x> ]]>",
    "<!-- c -->",
    "<?pi x?>",
    "<?xml version=\"1.0\" encoding=\"UTF-16\"?>",
    "\u{FEFF}",
    "\u{0}",
    "</",
    "<",
    ">",
    "\"",
];

#[derive(Debug, Clone)]
enum Piece {
    Open(usize, Vec<(usize, usize)>),
    Close,
    Empty(usize),
    Text(String),
    Fragment(usize),
    Byte(u8),
}

fn piece() -> impl Strategy<Value = Piece> {
    prop_oneof![
        6 => (0..NAMES.len(), proptest::collection::vec((0..NAMES.len(), 0..ATTR_VALUES.len()), 0..4))
            .prop_map(|(n, a)| Piece::Open(n, a)),
        4 => Just(Piece::Close),
        3 => (0..NAMES.len()).prop_map(Piece::Empty),
        2 => "[a-z &<;#0-9]{0,12}".prop_map(Piece::Text),
        1 => (0..FRAGMENTS.len()).prop_map(Piece::Fragment),
        1 => any::<u8>().prop_map(Piece::Byte),
    ]
}

fn soup(pieces: &[Piece], deep: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut open = vec!["Lanes"; deep];
    out.extend_from_slice("<Lanes>".repeat(deep).as_bytes());
    for p in pieces {
        match p {
            Piece::Open(n, attrs) => {
                out.push(b'<');
                out.extend_from_slice(NAMES[*n].as_bytes());
                for (k, v) in attrs {
                    out.extend_from_slice(format!(" a{k}=\"{}\"", ATTR_VALUES[*v]).as_bytes());
                }
                out.push(b'>');
                open.push(NAMES[*n]);
            }
            Piece::Close => {
                let name = open.pop().unwrap_or("Project");
                out.extend_from_slice(format!("</{name}>").as_bytes());
            }
            Piece::Empty(n) => out.extend_from_slice(format!("<{}/>", NAMES[*n]).as_bytes()),
            Piece::Text(s) => out.extend_from_slice(s.as_bytes()),
            Piece::Fragment(f) => out.extend_from_slice(FRAGMENTS[*f].as_bytes()),
            Piece::Byte(b) => out.push(*b),
        }
    }
    while let Some(name) = open.pop() {
        out.extend_from_slice(format!("</{name}>").as_bytes());
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(512)))]

    #[test]
    fn random_xml_never_panics(
        pieces in proptest::collection::vec(piece(), 0..200),
        deep in prop_oneof![3 => 0usize..8, 1 => 240usize..300],
    ) {
        let text = soup(&pieces, deep);
        let started = Instant::now();
        match parse(&text) {
            Ok(t) => prop_assert!(t.len() >= 1),
            Err(IoError::Refused(s)) => {
                prop_assert!(s.starts_with("This DAWproject file is damaged: "), "{s}");
            }
            Err(e) => prop_assert!(false, "unexpected {e:?}"),
        }
        prop_assert!(started.elapsed() < Duration::from_secs(2));
    }
}

#[test]
fn attributes_are_listed_in_order() {
    let t = parse(b"<a x=\"1\" y=\"a&amp;b\"><b/><c z=\"3\"/></a>").expect("parses");
    let root = t.root();
    assert_eq!(
        t.attrs(root).collect::<Vec<_>>(),
        [("x", "1"), ("y", "a&b")]
    );
    let kids = kids(&t, root);
    assert_eq!(t.attrs(kids[0]).count(), 0);
    assert_eq!(t.attrs(kids[1]).collect::<Vec<_>>(), [("z", "3")]);
}
