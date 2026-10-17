import AppKit

/// Keyboard modifiers of a pointer event.
public struct Modifiers: OptionSet, Hashable, Sendable {
    public let rawValue: Int
    public init(rawValue: Int) { self.rawValue = rawValue }

    public static let shift = Modifiers(rawValue: 1)
    public static let control = Modifiers(rawValue: 2)
    public static let option = Modifiers(rawValue: 4)
    public static let command = Modifiers(rawValue: 8)

    @MainActor
    public init(event: NSEvent) {
        let f = event.modifierFlags
        var m: Modifiers = []
        if f.contains(.shift) { m.insert(.shift) }
        if f.contains(.control) { m.insert(.control) }
        if f.contains(.option) { m.insert(.option) }
        if f.contains(.command) { m.insert(.command) }
        self = m
    }
}

/// Pointer gestures (spec §3.5): a press becomes a drag once the pointer
/// moved more than `deadZone` points (the transaction opens then); release
/// ends it; Escape cancels it. A press released inside the dead zone is a
/// click. `Hit` is what the press landed on (editor-specific).
public struct ToolMachine<Hit: Equatable & Sendable>: Sendable {
    public static var deadZone: CGFloat { 3 }

    public enum Phase: Equatable, Sendable {
        case idle
        case pressed(hit: Hit, at: CGPoint, clicks: Int, modifiers: Modifiers)
        case dragging(hit: Hit, from: CGPoint, modifiers: Modifiers)
    }

    public enum Effect: Equatable, Sendable {
        case click(hit: Hit, at: CGPoint, clicks: Int, modifiers: Modifiers)
        case begin(hit: Hit, from: CGPoint, modifiers: Modifiers)
        case move(hit: Hit, from: CGPoint, to: CGPoint, modifiers: Modifiers)
        case end(hit: Hit, from: CGPoint, to: CGPoint, modifiers: Modifiers)
        case cancel(hit: Hit)
    }

    public private(set) var phase: Phase = .idle

    public init() {}

    /// A press. A drag still open (its release was lost) is cancelled.
    public mutating func down(_ hit: Hit, at p: CGPoint, clicks: Int, modifiers: Modifiers) -> [Effect] {
        var out: [Effect] = []
        if case .dragging(let h, _, _) = phase { out.append(.cancel(hit: h)) }
        phase = .pressed(hit: hit, at: p, clicks: clicks, modifiers: modifiers)
        return out
    }

    public mutating func drag(to p: CGPoint, modifiers: Modifiers) -> [Effect] {
        switch phase {
        case .idle:
            return []
        case let .pressed(hit, start, _, _):
            guard hypot(p.x - start.x, p.y - start.y) > Self.deadZone else { return [] }
            phase = .dragging(hit: hit, from: start, modifiers: modifiers)
            return [
                .begin(hit: hit, from: start, modifiers: modifiers),
                .move(hit: hit, from: start, to: p, modifiers: modifiers),
            ]
        case let .dragging(hit, start, _):
            phase = .dragging(hit: hit, from: start, modifiers: modifiers)
            return [.move(hit: hit, from: start, to: p, modifiers: modifiers)]
        }
    }

    public mutating func up(at p: CGPoint, modifiers: Modifiers) -> [Effect] {
        defer { phase = .idle }
        switch phase {
        case .idle:
            return []
        case let .pressed(hit, start, clicks, downModifiers):
            return [.click(hit: hit, at: start, clicks: clicks, modifiers: downModifiers)]
        case let .dragging(hit, start, _):
            return [.end(hit: hit, from: start, to: p, modifiers: modifiers)]
        }
    }

    /// Escape.
    public mutating func cancel() -> [Effect] {
        defer { phase = .idle }
        if case .dragging(let hit, _, _) = phase { return [.cancel(hit: hit)] }
        return []
    }
}
