import AppKit
import VizDAWAppModel

/// Lane headers on the left, the arrangement canvas on the right; they
/// scroll vertically together.
@MainActor
public final class ArrangementContainer: NSView {
    public let headers: LaneHeaderView
    public let canvas: ArrangementView

    public init(document: DocumentModel) {
        headers = LaneHeaderView(document: document)
        canvas = ArrangementView(document: document)
        super.init(frame: .zero)
        addSubview(headers)
        addSubview(canvas)
        canvas.onVerticalScroll = { [unowned self] y in self.headers.verticalOffset = y }
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }

    public override func layout() {
        super.layout()
        let w = LaneHeaderLayout.width
        headers.frame = NSRect(x: 0, y: 0, width: w, height: bounds.height)
        canvas.frame = NSRect(x: w, y: 0, width: max(bounds.width - w, 0), height: bounds.height)
    }

    public override func scrollWheel(with e: NSEvent) {
        canvas.scrollWheel(with: e)
    }
}
