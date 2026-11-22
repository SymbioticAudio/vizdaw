use core_control::ErrorCode;
use core_control::registry::ArgValue;
use serde_json::json;
use vizdaw_app::Prefs;
use vizdaw_app::commands::run_app;
use vizdaw_app::transport_bar::{ItemKind, Size, TransportItem as I, TransportLayout, fit};

type Piece = (f32, bool, ItemKind);

fn content(w: f32) -> Piece {
    (w, false, ItemKind::Content)
}
fn fixed(w: f32) -> Piece {
    (w, true, ItemKind::Content)
}
fn sep() -> Piece {
    (1.0, false, ItemKind::Separator)
}

fn standard_pieces() -> Vec<Piece> {
    TransportLayout::standard()
        .items
        .iter()
        .map(|i| {
            let w = match i.name() {
                "cluster" => 230.0,
                "loop" | "record_options" | "faults" => 32.0,
                "click" => 70.0,
                "replace_overdub" => 80.0,
                "summary" => 120.0,
                "bar" => 80.0,
                "time" => 90.0,
                "tempo" => 70.0,
                "signature" => 50.0,
                "position_field" => 160.0,
                "edit_fields" => 300.0,
                "main_view" => 120.0,
                "panes" => 100.0,
                _ => 0.0,
            };
            (w, i.fixed(), i.kind())
        })
        .collect()
}

fn used(shown: &[usize], p: &[Piece], spacing: f32) -> f32 {
    shown.iter().map(|&i| p[i].0).sum::<f32>() + spacing * shown.len().saturating_sub(1) as f32
}

#[test]
fn nothing_is_cut() {
    let pieces = standard_pieces();
    assert_eq!(pieces.len(), 16);
    let content_set: Vec<usize> = (0..16)
        .filter(|&i| !pieces[i].1 && pieces[i].2 == ItemKind::Content)
        .collect();
    let (spacing, chevron) = (12.0, 28.0);
    let mut widths: Vec<f32> = (0..=200).map(|n| n as f32 * 10.0).collect();
    widths.extend([f32::NAN, -1.0, f32::INFINITY]);
    for w in widths {
        let f = fit(&pieces, w, spacing, chevron);
        for &c in &content_set {
            assert!(f.shown.contains(&c) ^ f.folded.contains(&c), "{w}: {c}");
        }
        assert!(f.folded.iter().all(|i| content_set.contains(i)), "{w}");
        assert!(f.shown.windows(2).all(|p| p[0] < p[1]), "{w}: bar order");
        assert!(f.folded.windows(2).all(|p| p[0] < p[1]), "{w}");
        assert!(f.shown.contains(&0), "{w}: the cluster shows");
        if !w.is_finite() {
            continue;
        }
        let need = used(&f.shown, &pieces, spacing)
            + if f.folded.is_empty() {
                0.0
            } else {
                spacing + chevron
            };
        assert!(need <= w || f.shown == [0], "{w}: {need}");
    }
    assert!(
        fit(&pieces, f32::INFINITY, spacing, chevron)
            .folded
            .is_empty()
    );
    assert_eq!(
        fit(&pieces, f32::NAN, spacing, chevron).folded.len(),
        content_set.len()
    );
    assert_eq!(fit(&pieces, 0.0, spacing, chevron).shown, [0]);
}

#[test]
fn folds_the_last_item_first() {
    let p = [content(50.0), fixed(100.0), content(50.0), content(50.0)];
    let folded = |w| fit(&p, w, 10.0, 20.0).folded;
    assert_eq!(folded(300.0), Vec::<usize>::new());
    assert_eq!(folded(260.0), [3]);
    assert_eq!(folded(200.0), [2, 3]);
    assert_eq!(folded(150.0), [0, 2, 3]);
    assert_eq!(folded(100.0), [0, 2, 3]);
    assert_eq!(fit(&p, 100.0, 10.0, 20.0).shown, [1]);
}

#[test]
fn spaces_and_separators_are_never_listed() {
    let space = (0.0, false, ItemKind::Space);
    let p = [
        fixed(100.0),
        content(50.0),
        sep(),
        space,
        content(50.0),
        sep(),
        sep(),
        content(50.0),
        sep(),
    ];
    for n in 0..=100 {
        let w = n as f32 * 5.0;
        let f = fit(&p, w, 10.0, 20.0);
        assert!(f.folded.iter().all(|&i| p[i].2 == ItemKind::Content), "{w}");
        assert!(!f.shown.contains(&8), "{w}: a separator at the edge");
        assert!(!f.shown.contains(&5) && !f.shown.contains(&6), "{w}");
        assert!(f.hidden_separators.contains(&8), "{w}");
        assert!(
            !f.hidden_separators.contains(&3),
            "{w}: a space is no separator"
        );
    }
    let row = [fixed(100.0), content(50.0), sep(), content(50.0)];
    assert_eq!(fit(&row, 400.0, 10.0, 20.0).shown, [0, 1, 2, 3]);
    let f = fit(&row, 200.0, 10.0, 20.0);
    assert_eq!(f.folded, [3]);
    assert_eq!(f.shown, [0, 1], "the separator before » is hidden");
    assert_eq!(f.hidden_separators, [2]);
}

#[test]
fn fit_survives_nan_zero_and_infinite_widths() {
    let p = [
        fixed(10.0),
        content(f32::NAN),
        content(f32::INFINITY),
        content(-5.0),
    ];
    for w in [f32::NAN, 0.0, -3.0, f32::NEG_INFINITY] {
        let f = fit(&p, w, f32::NAN, f32::INFINITY);
        assert_eq!((f.shown, f.folded), (vec![0], vec![1, 2, 3]));
    }
    assert!(fit(&p, f32::INFINITY, f32::NAN, f32::NAN).folded.is_empty());
    let f = fit(&p, 50.0, f32::NAN, f32::NAN);
    assert!(f.shown.contains(&0));
    assert!(fit(&[], 100.0, 1.0, 1.0).shown.is_empty());
}

#[test]
fn the_default_layout_is_todays_bar() {
    let names: Vec<&str> = TransportLayout::standard()
        .items
        .iter()
        .map(I::name)
        .collect();
    assert_eq!(
        names,
        [
            "cluster",
            "loop",
            "click",
            "replace_overdub",
            "record_options",
            "summary",
            "bar",
            "time",
            "tempo",
            "signature",
            "position_field",
            "edit_fields",
            "space",
            "faults",
            "main_view",
            "panes"
        ]
    );
    let std = TransportLayout::standard();
    assert!(std.items_for(false).iter().all(|i| !i.document_only()));
    assert_eq!(std.items_for(true).len(), 16);
    assert_eq!(std.items_for(false).len(), 13);
    assert_eq!(I::ReplaceOverdub.token(), "replace_overdub");
}

fn damaged(bad: serde_json::Value) {
    let dir = tempfile::tempdir().unwrap();
    let path = Prefs::file_in(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, json!({ "transportBar": bad }).to_string()).unwrap();
    let mut p = Prefs::at(&path);
    assert_eq!(p.transport_bar(), TransportLayout::standard(), "{bad}");
    assert_eq!(p.raw("transportBar"), Some(&bad), "{bad} is kept");
    let mine = TransportLayout::parse("cluster,loop,bar:large").unwrap();
    p.set_transport_bar(&mine).unwrap();
    assert_ne!(p.raw("transportBar"), Some(&bad));
    assert_eq!(Prefs::at(&path).transport_bar(), mine);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("\"version\": 1") && text.contains("bar:large"),
        "{text}"
    );
}

#[test]
fn a_damaged_preference_gives_the_default_and_is_kept() {
    for bad in [
        json!("{not json"),
        json!({"version": 99, "items": []}),
        json!({"version": 1, "items": ["loop"]}),
        json!({"version": 1, "items": ["cluster", "clik"]}),
        json!({"items": ["cluster"]}),
        json!(7),
    ] {
        damaged(bad);
    }
    // An unreadable file, and nothing stored: the default, nothing written.
    let dir = tempfile::tempdir().unwrap();
    let path = Prefs::file_in(dir.path());
    assert_eq!(
        Prefs::at(&path).transport_bar(),
        TransportLayout::standard()
    );
    assert!(!path.exists());
    assert_eq!(Prefs::in_memory().raw("transportBar"), None);
}

#[test]
fn a_stored_standard_follows_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = Prefs::file_in(dir.path());
    let mut p = Prefs::at(&path);
    let custom = TransportLayout::parse("cluster,loop").unwrap();
    p.set_transport_bar(&custom).unwrap();
    p.reset_transport_bar().unwrap();
    assert_eq!(p.raw("transportBar"), Some(&json!("standard")));
    p.set_transport_bar(&custom).unwrap();
    p.set_transport_bar(&TransportLayout::standard()).unwrap();
    assert_eq!(p.raw("transportBar"), Some(&json!("standard")));
    let back = Prefs::at(&path).transport_bar();
    assert_eq!(back, TransportLayout::standard());
    assert!(
        !back
            .items
            .iter()
            .any(|i| matches!(i, I::PunchFields | I::Device))
    );
    // A custom bar that names them keeps them.
    let old = TransportLayout::parse(
        "cluster,loop,bar,position_field,edit_fields,punch_fields,space,device,faults",
    )
    .unwrap();
    let mut q = Prefs::in_memory();
    q.set_transport_bar(&old).unwrap();
    assert_ne!(q.raw("transportBar"), Some(&json!("standard")));
    assert_eq!(q.transport_bar(), old);
    assert!(old.items.contains(&I::PunchFields) && old.items.contains(&I::Device));
    // Other keys survive a write.
    let mut r = Prefs::at(&path);
    r.set_transport_bar(&custom).unwrap();
    assert_eq!(Prefs::at(&path).transport_bar(), custom);
}

#[test]
fn items_text_parses() {
    fn message(text: &str) -> Option<String> {
        TransportLayout::parse(text).err()
    }
    let l = TransportLayout::parse("cluster,loop,bar:large").unwrap();
    assert_eq!(l.items, [I::Cluster, I::Loop, I::Bar(Size::Large)]);
    assert_eq!(l.tokens(), "cluster,loop,bar:large");
    assert_eq!(
        TransportLayout::parse(&TransportLayout::standard().tokens()).unwrap(),
        TransportLayout::standard()
    );
    assert_eq!(
        TransportLayout::parse(" cluster , space,separator, space ")
            .unwrap()
            .items
            .len(),
        4
    );
    assert_eq!(message("loop").unwrap(), "the bar needs cluster once");
    assert_eq!(
        message("cluster,cluster").unwrap(),
        "the bar needs cluster once"
    );
    assert_eq!(message("cluster,clik").unwrap(), "unknown item clik");
    assert_eq!(message("cluster,loop:large").unwrap(), "loop has no size");
    assert_eq!(message("cluster,bar:huge").unwrap(), "bar has no size huge");
    assert_eq!(
        message("cluster,loop,loop").unwrap(),
        "loop is in the bar twice"
    );
    assert_eq!(message("cluster,,loop").unwrap(), "an empty item");
    assert_eq!(message("").unwrap(), "an empty item");
    let spaces = vec!["space"; 64].join(",");
    assert_eq!(message(&spaces).unwrap(), "the bar needs cluster once");
    assert_eq!(
        message(&format!("cluster,{spaces}")).unwrap(),
        "at most 64 items"
    );
    assert!(message(&"x".repeat(5000)).unwrap().len() < 80);
    assert_eq!(message("cluster,Loop").unwrap(), "unknown item Loop");
    assert_eq!(
        message("cluster,replaceOverdub").unwrap(),
        "unknown item replaceOverdub"
    );
    // Commands.
    assert_eq!(
        message("cluster,command").unwrap(),
        "command needs a command id: command:<id>"
    );
    assert!(
        message("cluster,command:Bad Id")
            .unwrap()
            .starts_with("command has no command id")
    );
    let c = TransportLayout::parse("cluster,command:edit.undo,command:edit.redo:Go %2C 100%25")
        .unwrap();
    assert_eq!(
        c.items[2],
        I::Command {
            id: "edit.redo".into(),
            label: Some("Go , 100%".into())
        }
    );
    assert_eq!(
        c.tokens(),
        "cluster,command:edit.undo,command:edit.redo:Go %2C 100%25"
    );
    assert!(
        message(&format!("cluster,command:a:{}", "x".repeat(33)))
            .unwrap()
            .contains("at most 32")
    );
    assert!(message("cluster,command:a:").unwrap().contains("1 to 32"));
    assert!(
        message("cluster,command:a: pad")
            .unwrap()
            .contains("not blank or padded")
    );
}

fn items(v: &str) -> Vec<(String, ArgValue)> {
    vec![("items".into(), ArgValue::Text(v.into()))]
}

#[test]
fn set_transport_bar_over_the_channel() {
    let mut p = Prefs::in_memory();
    let ran = run_app("ui.set_transport_bar", &items("cluster,loop"), &mut p).unwrap();
    assert_eq!(ran.status.as_deref(), Some("transport bar: 2 items"));
    assert_eq!(p.transport_bar().items, [I::Cluster, I::Loop]);
    for bad in ["cluster,clik", "loop", &"cluster,".repeat(70)] {
        let e = run_app("ui.set_transport_bar", &items(bad), &mut p).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{bad}");
        assert!(
            e.message.starts_with("ui.set_transport_bar: "),
            "{}",
            e.message
        );
        assert_eq!(p.transport_bar().items.len(), 2, "unchanged");
    }
    let e = run_app("ui.set_transport_bar", &items("cluster,clik"), &mut p).unwrap_err();
    assert_eq!(e.message, "ui.set_transport_bar: unknown item clik");
    let e = run_app("ui.set_transport_bar", &[], &mut p).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert_eq!(
        e.message,
        "ui.set_transport_bar: `items` lists the bar's items, such as \"cluster,loop\""
    );
    let ran = run_app("ui.reset_transport_bar", &[], &mut p).unwrap();
    assert_eq!(ran.status.as_deref(), Some("transport bar: the default"));
    assert_eq!(p.transport_bar(), TransportLayout::standard());
    let e = run_app("edit.undo", &[], &mut p).unwrap_err();
    assert_eq!(e.code, ErrorCode::UnknownId);
}
