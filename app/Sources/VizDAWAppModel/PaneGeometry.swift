import CoreGraphics
import Foundation

/// The frames of layout A's regions, in the area under the transport bar (top-left origin).
public struct PaneFrames: Equatable {
    public var centre: CGRect
    public var left, bottom, right: CGRect?
    /// Panes dropped for this frame only; the saved `shown` flags are unchanged.
    public var hidden: [PaneSide]

    public init(centre: CGRect, left: CGRect? = nil, bottom: CGRect? = nil,
                right: CGRect? = nil, hidden: [PaneSide] = []) {
        self.centre = centre
        self.left = left
        self.bottom = bottom
        self.right = right
        self.hidden = hidden
    }
}

extension PaneFrames {
    /// The size `side` shows at in these frames (its width, the bottom
    /// pane's height), or nil when it is not shown.
    public func size(_ side: PaneSide) -> Double? {
        switch side {
        case .left: left.map { Double($0.width) }
        case .bottom: bottom.map { Double($0.height) }
        case .right: right.map { Double($0.width) }
        }
    }

    /// `setup` as these frames show it: each shown pane at its displayed
    /// (perhaps shrunk) size, a pane dropped for this frame hidden. A
    /// divider drag starts from it, so its first change does not jump
    /// (Task 2 review M3).
    public func displayed(_ setup: PaneSetup) -> PaneSetup {
        var s = setup
        for side in PaneSide.allCases {
            if let v = size(side) { s[side].size = v } else { s[side].shown = false }
        }
        return s
    }
}

public enum PaneGeometry {
    public static let centreMinimum = CGSize(width: 240, height: 160)
    public static let bottomMinimumWidth: CGFloat = 240
    public static let divider: CGFloat = 1

    /// Sizes after shrinking, or nil when the shown panes cannot fit.
    private static func fit(_ size: CGSize, shown: Set<PaneSide>, setup: PaneSetup) -> [PaneSide: CGFloat]? {
        func clamped(_ side: PaneSide) -> CGFloat {
            let r = ViewLayouts.sizeRange[side] ?? 0...0
            let v = setup[side].size
            return CGFloat(min(max(v.isFinite ? v : r.lowerBound, r.lowerBound), r.upperBound))
        }
        func lower(_ side: PaneSide) -> CGFloat { CGFloat(ViewLayouts.sizeRange[side]?.lowerBound ?? 0) }
        var out: [PaneSide: CGFloat] = [:]
        for side in shown { out[side] = clamped(side) }
        let d = divider
        let leftCost = (out[.left] ?? -d) + d
        // Right: the column beside the left pane must hold the centre (or the bottom
        // pane's minimum) plus a divider plus the right pane.
        if let r = out[.right] {
            let avail = size.width - leftCost
            out[.right] = max(lower(.right), min(r, avail - centreMinimum.width - d))
        }
        if let b = out[.bottom] {
            out[.bottom] = max(lower(.bottom), min(b, size.height - d - centreMinimum.height))
        }
        if let l = out[.left] {
            let need = out[.right].map { centreMinimum.width + d + $0 } ?? centreMinimum.width
            out[.left] = max(lower(.left), min(l, size.width - d - need))
        }
        // Verify.
        let x0 = (out[.left] ?? -d) + d
        let rightCost = out[.right].map { $0 + d } ?? 0
        guard size.width - x0 - rightCost >= centreMinimum.width else { return nil }
        if let b = out[.bottom] {
            guard size.height - d - b >= centreMinimum.height,
                  size.width - x0 - rightCost >= bottomMinimumWidth else { return nil }
        } else {
            guard size.height >= centreMinimum.height else { return nil }
        }
        return out
    }

    public static func frames(in size: CGSize, setup: PaneSetup) -> PaneFrames {
        var shown = Set(PaneSide.allCases.filter { setup[$0].shown })
        var hidden: [PaneSide] = []
        var sizes = fit(size, shown: shown, setup: setup)
        for side in [PaneSide.right, .left, .bottom] where sizes == nil {
            guard shown.contains(side) else { continue }
            shown.remove(side)
            hidden.append(side)
            sizes = fit(size, shown: shown, setup: setup)
        }
        let s = sizes ?? [:]
        let d = divider
        let x0 = s[.left].map { $0 + d } ?? 0
        let bottomH = s[.bottom]
        let rightW = s[.right]
        let columnW = size.width - x0
        let centre = CGRect(
            x: x0, y: 0,
            width: columnW - (bottomH == nil ? rightW.map { $0 + d } ?? 0 : 0),
            height: bottomH.map { size.height - $0 - d } ?? size.height)
        let left = s[.left].map { CGRect(x: 0, y: 0, width: $0, height: size.height) }
        let bottom = bottomH.map {
            CGRect(x: x0, y: size.height - $0, width: columnW - (rightW.map { $0 + d } ?? 0), height: $0)
        }
        let right = rightW.map {
            CGRect(x: size.width - $0, y: bottomH.map { size.height - $0 } ?? 0,
                   width: $0, height: bottomH ?? size.height)
        }
        return PaneFrames(centre: centre, left: left, bottom: bottom, right: right, hidden: hidden)
    }

    /// The new, clamped size of `side` after a divider drag of `delta` points.
    /// Left grows with a positive delta, right with a negative one, bottom with a positive one
    /// (the divider passes `-translation.height`). A non-finite delta keeps the size.
    public static func drag(_ setup: PaneSetup, side: PaneSide, by delta: CGFloat, in size: CGSize) -> Double {
        let range = ViewLayouts.sizeRange[side] ?? 0...0
        let current = setup[side].size
        let start = current.isFinite ? min(max(current, range.lowerBound), range.upperBound) : range.lowerBound
        guard delta.isFinite else { return start }
        let signed = side == .right ? -Double(delta) : Double(delta)
        // Never grow past what leaves the centre its minimum.
        let d = Double(divider)
        var upper = range.upperBound
        switch side {
        case .left:
            let others = setup.right.shown ? Double(centreMinimum.width) + d + setup.right.size
                : Double(centreMinimum.width)
            upper = min(upper, Double(size.width) - d - others)
        case .right:
            let left = setup.left.shown ? setup.left.size + d : 0
            upper = min(upper, Double(size.width) - left - Double(centreMinimum.width) - d)
        case .bottom:
            upper = min(upper, Double(size.height) - d - Double(centreMinimum.height))
        }
        upper = max(upper, range.lowerBound)
        return min(max(start + signed, range.lowerBound), upper)
    }

    /// The frame a new document window opens at when it has no saved one
    /// (owner, 2026-11-05): 1440×900, or 90% of the screen's visible frame
    /// where that is smaller, each side on its own, centred in the visible
    /// frame. Never under the window's own minimum, 700×450.
    public static func defaultDocumentFrame(visible: CGRect) -> CGRect {
        let w = max(min(1440, visible.width * 0.9), 700)
        let h = max(min(900, visible.height * 0.9), 450)
        return CGRect(x: visible.midX - w / 2, y: visible.midY - h / 2, width: w, height: h)
    }

    /// A saved `[x, y, width, height]` if it is four finite numbers, at least 700×450,
    /// shows at least 100×100 on some screen and has the strip of its title bar
    /// (the top 30 pt) on a screen, so it can be dragged (final review n1);
    /// otherwise nil.
    public static func sanitizedFrame(_ f: [Double]?, screens: [CGRect]) -> CGRect? {
        guard let f, f.count == 4, f.allSatisfy(\.isFinite), f[2] >= 700, f[3] >= 450 else { return nil }
        let r = CGRect(x: f[0], y: f[1], width: f[2], height: f[3])
        let ok = screens.contains {
            let i = $0.intersection(r)
            return !i.isNull && i.width >= 100 && i.height >= 100
        }
        let titleBar = CGRect(x: r.minX, y: r.maxY - 30, width: r.width, height: 30)
        let reachable = screens.contains {
            let i = $0.intersection(titleBar)
            return !i.isNull && i.width >= 100 && i.height >= 30
        }
        return ok && reachable ? r : nil
    }
}

/// What a divider asks of the cursor stack.
public enum CursorOp: Equatable, Sendable { case push, pop }

/// The resize cursor a pane divider pushes while the pointer is over it.
/// Pushes and pops always balance: it pops only what it pushed, so a drag
/// that leaves the line, or a divider that goes away, never leaves the
/// stack unbalanced.
public struct DividerCursor: Equatable, Sendable {
    private var pushed = false
    /// Whether the pointer was last over the line.
    public private(set) var inside = false

    public init() {}

    public mutating func hover(_ inside: Bool) -> CursorOp? {
        self.inside = inside
        return settle(inside)
    }

    /// A drag ended with the pointer `inside` the line or not.
    public mutating func dragEnded(inside: Bool) -> CursorOp? {
        self.inside = inside
        return settle(inside)
    }

    /// The divider left the screen.
    public mutating func disappear() -> CursorOp? {
        inside = false
        return settle(false)
    }

    private mutating func settle(_ want: Bool) -> CursorOp? {
        guard want != pushed else { return nil }
        pushed = want
        return want ? .push : .pop
    }
}
