import AppKit
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// Finds the app's view classes and checks that each clips to its bounds
/// (plan tracks Task 1, Ruling 1). macOS 14 does not clip by default, so a
/// view that fills a rectangle past its frame paints over its neighbours
/// (the editor canvas covered the lane headers).
enum ClipScan {
    struct ClassDecl: Equatable {
        let name: String
        let superName: String
        let body: String
        let file: String
    }

    /// The three controls whose focus ring is drawn outside their bounds.
    static let allowed: Set<String> = ["TransportTextField", "ListCellField", "HoldButtonView"]

    static let appKitView = try! NSRegularExpression(pattern: #"^(NS\w*View|NSButton|NSTextField|NSControl)$"#)
    static let declaration = try! NSRegularExpression(
        pattern: #"\bclass\s+(\w+)\s*(?:<[^>{]*>)?\s*:\s*([\w.]+)"#)
    static let clipLine = try! NSRegularExpression(pattern: #"^\s*(self\.)?clipsToBounds\s*=\s*true"#)
    static let drawOverride = try! NSRegularExpression(pattern: #"func\s+draw\s*\("#)

    static func matches(_ r: NSRegularExpression, _ s: String) -> Bool {
        r.firstMatch(in: s, range: NSRange(s.startIndex..., in: s)) != nil
    }

    /// The text from the first `{` at or after `start` to its match, skipping
    /// comments and string literals; nil if it never closes.
    static func braced(_ text: [Character], from start: Int) -> String? {
        var i = start
        var depth = 0
        var begin: Int?
        while i < text.count {
            let c = text[i]
            if c == "/", i + 1 < text.count, text[i + 1] == "/" {
                while i < text.count, text[i] != "\n" { i += 1 }
                continue
            }
            if c == "/", i + 1 < text.count, text[i + 1] == "*" {
                i += 2
                while i + 1 < text.count, !(text[i] == "*" && text[i + 1] == "/") { i += 1 }
                i += 2
                continue
            }
            if c == "\"" {
                if i + 2 < text.count, text[i + 1] == "\"", text[i + 2] == "\"" {
                    i += 3
                    while i + 2 < text.count, !(text[i] == "\"" && text[i + 1] == "\"" && text[i + 2] == "\"") {
                        i += text[i] == "\\" ? 2 : 1
                    }
                    i += 3
                } else {
                    i += 1
                    while i < text.count, text[i] != "\"", text[i] != "\n" { i += text[i] == "\\" ? 2 : 1 }
                    i += 1
                }
                continue
            }
            if c == "{" {
                if depth == 0 { begin = i }
                depth += 1
            } else if c == "}" {
                depth -= 1
                if depth == 0, let b = begin { return String(text[b...i]) }
            }
            i += 1
        }
        return nil
    }

    /// Every `class Name: Super` in `source`, with its brace-matched body.
    static func declarations(in source: String, file: String) -> [ClassDecl] {
        let text = Array(source)
        let ns = source as NSString
        var out: [ClassDecl] = []
        var offset = 0
        for line in source.components(separatedBy: "\n") {
            defer { offset += line.utf16.count + 1 }
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("//") { continue }
            let range = NSRange(location: offset, length: line.utf16.count)
            guard let m = declaration.firstMatch(in: ns as String, range: range) else { continue }
            let name = ns.substring(with: m.range(at: 1))
            guard !["var", "func", "let"].contains(name) else { continue }
            let sup = ns.substring(with: m.range(at: 2)).split(separator: ".").last.map(String.init) ?? ""
            // The body starts at the first `{` after the declaration's end.
            let end = source.index(source.startIndex, offsetBy: m.range.location + m.range.length)
            let charIndex = source.distance(from: source.startIndex, to: end)
            guard let body = braced(text, from: charIndex) else { continue }
            out.append(ClassDecl(name: name, superName: sup, body: body, file: file))
        }
        return out
    }

    /// The classes that are view subclasses, resolved as a fixed point (a
    /// class whose super is a collected class is collected).
    static func collected(_ all: [ClassDecl]) -> [ClassDecl] {
        var names = Set<String>()
        var changed = true
        while changed {
            changed = false
            for d in all where !names.contains(d.name) {
                if matches(appKitView, d.superName) || names.contains(d.superName) {
                    names.insert(d.name)
                    changed = true
                }
            }
        }
        return all.filter { names.contains($0.name) }
    }

    static func setsClip(_ d: ClassDecl) -> Bool {
        d.body.components(separatedBy: "\n").contains { matches(clipLine, $0) }
    }

    /// "file: Class" for each collected class that neither sets
    /// `clipsToBounds = true` on itself nor inherits from one that does,
    /// and is not an allowed control without a `draw(_:)` of its own.
    static func violations(_ all: [ClassDecl]) -> [String] {
        let views = collected(all)
        let byName = Dictionary(views.map { ($0.name, $0) }, uniquingKeysWith: { a, _ in a })
        func clips(_ d: ClassDecl, _ seen: Set<String> = []) -> Bool {
            if setsClip(d) { return true }
            guard !seen.contains(d.name), let up = byName[d.superName] else { return false }
            return clips(up, seen.union([d.name]))
        }
        return views.filter { d in
            if allowed.contains(d.name) { return matches(drawOverride, d.body) }
            return !clips(d)
        }.map { "\($0.file): \($0.name)" }
    }

    static func sources() throws -> [ClassDecl] {
        var out: [ClassDecl] = []
        let fm = FileManager.default
        for dir in ["VizDAW", "VizDAWEditors", "VizDAWAppModel"] {
            let root = SourceScan.sourcesRoot.appendingPathComponent(dir, isDirectory: true)
            guard let walker = fm.enumerator(at: root, includingPropertiesForKeys: nil) else {
                throw SourceScan.ScanError(description: "cannot list \(root.path)")
            }
            var files: [URL] = []
            for case let url as URL in walker where url.pathExtension == "swift" { files.append(url) }
            for url in files.sorted(by: { $0.path < $1.path }) {
                let name = url.path.replacingOccurrences(of: SourceScan.sourcesRoot.path + "/", with: "")
                out += declarations(in: try String(contentsOf: url, encoding: .utf8), file: name)
            }
        }
        return out
    }
}

@MainActor
@Suite struct ClipScanTests {
    @Test func everyViewSubclassClipsToItsBounds() throws {
        // The self-check first: the scan sees these snippets as it should.
        let snippet = """
            final class Bad: NSView {
                init() { super.init(frame: .zero); let v = NSView(); v.clipsToBounds = true }
            }
            final class Good: NSView {
                init() { super.init(frame: .zero)
                    clipsToBounds = true
                }
            }
            final class Child: Good { }
            final class Plain: NSObject { }
            final class Field: NSTextField { }
            final class DrawingField: NSTextField { override func draw(_ r: NSRect) {} }
            // final class Commented: NSView { }
            """
        let decls = ClipScan.declarations(in: snippet, file: "x.swift")
        #expect(decls.map(\.name) == ["Bad", "Good", "Child", "Plain", "Field", "DrawingField"])
        #expect(ClipScan.collected(decls).map(\.name) == ["Bad", "Good", "Child", "Field", "DrawingField"])
        #expect(
            ClipScan.violations(decls) == ["x.swift: Bad", "x.swift: Field", "x.swift: DrawingField"],
            "another object's clipsToBounds does not count; Child inherits Good's")
        let allowed = ClipScan.violations(decls.map {
            ClipScan.ClassDecl(
                name: $0.name == "Field" ? "ListCellField" : $0.name == "DrawingField" ? "HoldButtonView" : $0.name,
                superName: $0.superName, body: $0.body, file: $0.file)
        })
        #expect(allowed == ["x.swift: Bad", "x.swift: HoldButtonView"], "an allowed control must not draw itself")

        let all = try ClipScan.sources()
        let found = ClipScan.collected(all)
        #expect(found.count > 25, "the scan found the app's views: \(found.count)")
        for name in ["EditorCanvas", "CanvasLayerView", "LaneHeaderView", "ArrangementContainer", "ArrangementView"] {
            #expect(found.contains { $0.name == name }, "\(name) is collected")
        }
        let bad = ClipScan.violations(all)
        #expect(bad.isEmpty, "views that do not set clipsToBounds = true (\(bad.count)): \n\(bad.joined(separator: "\n"))")
    }

    /// The ring (3 pt on every side) of `field` lies inside every clipping
    /// ancestor, in window coordinates.
    private func ringFits(_ field: NSView) -> Bool {
        let ring = field.convert(field.bounds.insetBy(dx: -3, dy: -3), to: nil)
        var a = field.superview
        while let v = a {
            if v.clipsToBounds || v is NSClipView, !v.convert(v.bounds, to: nil).contains(ring) { return false }
            a = v.superview
        }
        return true
    }

    private func fields<T: NSView>(_ type: T.Type, in v: NSView) -> [T] {
        v.subviews.flatMap { s in (s as? T).map { [$0] } ?? [] } + v.subviews.flatMap { fields(type, in: $0) }
    }

    @Test func focusRingControlsHaveRoomInsideTheirClippingAncestors() throws {
        // The transport fields.
        let m = try DocumentModel(session: demoFake())
        let t = TransportFieldsView(document: m)
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 700, height: 30), styleMask: [.titled], backing: .buffered,
            defer: true)
        w.isReleasedWhenClosed = false
        w.contentView = t
        t.layoutSubtreeIfNeeded()
        let transport = fields(TransportTextField.self, in: t)
        #expect(transport.count == TransportField.allCases.count)
        #expect(t.clipsToBounds, "the container clips, so the fields must sit inside it")
        for f in transport { #expect(ringFits(f), "\(f.title) ring: \(f.frame) in \(t.bounds)") }

        // The list's cells.
        let f = rollFake()
        f.laneEventTable["3/31"] = LaneEvents(
            seq: 3, lane: 31, rev: 2,
            events: [noteRow(1, tick: 0, pitch: 60), noteRow(2, tick: 15_360, pitch: 64), noteRow(4, tick: 30_720, pitch: 67)])
        let dm = try DocumentModel(session: f)
        dm.editor.openDetail(.lane(seq: 3, lane: 31), kind: .list)
        let list = ListEditorView(model: dm.list(.pane))
        let lw = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 900, height: 300), styleMask: [.titled], backing: .buffered,
            defer: true)
        lw.isReleasedWhenClosed = false
        lw.contentView = list
        list.layout()
        list.table.layoutSubtreeIfNeeded()
        #expect(list.numberOfRows(in: list.table) == 3)
        let cells = fields(ListCellField.self, in: list)
        #expect(cells.count >= 3, "cells were made: \(cells.count)")
        for c in cells {
            #expect(c.superview?.clipsToBounds == true, "the row clips, so the cell must sit inside it")
            #expect(ringFits(c), "cell ring: \(c.frame) in \(String(describing: c.superview?.bounds))")
        }
    }
}
