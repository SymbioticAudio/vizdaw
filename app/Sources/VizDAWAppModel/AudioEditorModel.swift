import CoreGraphics
import Foundation
import Observation
import VizDAWCore

/// The audio editor's typed boxes (plan 7c Ruling 7). The raw values are
/// the control channel's names.
public enum AudioEditorBox: String, CaseIterable, Sendable {
    case gain
    case fadeIn = "fade_in"
    /// A crossfade's length (plan 6c): the fade in of a crossfaded region.
    case crossfade
    case fadeOut = "fade_out"
    case start, length, peak

    /// The box's accessibility label.
    public var label: String {
        switch self {
        case .gain: "Region Gain"
        case .fadeIn: "Fade In Length"
        case .crossfade: "Crossfade Length"
        case .fadeOut: "Fade Out Length"
        case .start: "Region Start"
        case .length: "Region Length"
        case .peak: "Normalize Peak"
        }
    }

    /// What the status line says when the box refuses its text.
    public var hint: String {
        switch self {
        case .gain: AudioEditorEntry.gainHint
        case .fadeIn, .crossfade, .fadeOut: AudioEditorEntry.fadeHint
        case .start, .length: AudioEditorEntry.timeHint
        case .peak: AudioEditorEntry.peakHint
        }
    }

    /// Fades, the crossfade, start and length need the media's rate.
    var isTime: Bool { self != .gain && self != .peak }
}

/// The bottom pane's audio editor (plan 7c Rulings 1–3, 5, 7 and 9): the
/// selected audio region, the zoom (in memory only), typed commits and
/// normalize from the Peak box. Every commit is one `editor.edit`, so one
/// undo step; a value equal to the current one sends nothing.
@MainActor
@Observable
public final class AudioEditorModel {
    public static let emptyMessage = "Select one audio region to edit it."

    /// The waveform heights the Taller and Shorter buttons step through.
    static let amplitudeRange = 1...16

    @ObservationIgnored private unowned let document: DocumentModel

    public private(set) var axis = FrameAxis()
    public private(set) var amplitude = 1
    public private(set) var shownRegion: UInt64?
    /// The wave rect's width, as `sync(width:)` last gave it.
    public private(set) var width: CGFloat = 0
    /// The Normalize target (Ruling 9).
    public private(set) var peakDb = RegionCommands.defaultPeakDb

    public init(document: DocumentModel) {
        self.document = document
    }

    // MARK: The region and its axis

    /// The shown region's room before tick 0, in media frames (Ruling
    /// 13.8): 0 at the sequence start, when the time cannot be read or
    /// without a media row.
    public var startRoom: UInt64 {
        guard let r = document.editedRegion, let rate = r.media?.sampleRate, rate > 0,
            let s = try? document.editor.session.secondsAt(seq: r.lane.seq, ticks: [r.row.tick]).first,
            s.isFinite, s > 0
        else { return 0 }
        let f = (s * Double(rate)).rounded(.down)
        return f >= Double(UInt64.max) ? UInt64.max : UInt64(f)
    }

    /// `width` is the wave rect's width. A newly shown region is fitted;
    /// the same region keeps its zoom and scroll. While the width is 0 or
    /// less nothing is fitted or recorded as shown, so the first real width
    /// fits (Task 4 review M1).
    public func sync(width: CGFloat) {
        if self.width != width { self.width = width }
        let region = document.editedRegion
        let id = region?.row.id
        guard id != shownRegion else { return }
        guard width > 0 else { return }
        if let region {
            let fitted = FrameAxis.fit(
                offset: region.row.sourceOffset, length: region.row.frames, width: width,
                mediaFrames: Self.mediaFrames(region))
            if axis != fitted { axis = fitted }
        }
        shownRegion = id
    }

    private static func mediaFrames(_ r: EditedRegion) -> UInt64 {
        r.media?.frames ?? r.row.sourceOffset.addingReportingOverflow(r.row.frames).partialValue
    }

    // MARK: Zoom

    public func zoomIn() { zoom(by: 2, aroundX: width / 2) }

    public func zoomOut() { zoom(by: 0.5, aroundX: width / 2) }

    /// While the width is 0 or less nothing is fitted or recorded, as in
    /// `sync(width:)` (Task 5 review M4).
    public func fit() {
        guard width > 0, let r = document.editedRegion else { return }
        set(
            FrameAxis.fit(
                offset: r.row.sourceOffset, length: r.row.frames, width: width, mediaFrames: Self.mediaFrames(r)))
        if shownRegion != r.row.id { shownRegion = r.row.id }
    }

    /// `factor` above 1 zooms in, keeping the frame under `x`.
    public func zoom(by factor: Double, aroundX x: CGFloat) {
        guard let r = document.editedRegion else { return }
        set(axis.zoomed(by: factor, aroundX: x, width: width, mediaFrames: Self.mediaFrames(r)))
    }

    public func scroll(byPoints dx: CGFloat) {
        guard let r = document.editedRegion else { return }
        set(axis.scrolled(byPoints: dx, width: width, mediaFrames: Self.mediaFrames(r)))
    }

    public func taller() {
        let a = min(amplitude * 2, Self.amplitudeRange.upperBound)
        if a != amplitude { amplitude = a }
    }

    public func shorter() {
        let a = max(amplitude / 2, Self.amplitudeRange.lowerBound)
        if a != amplitude { amplitude = a }
    }

    private func set(_ a: FrameAxis) {
        if a != axis { axis = a }
    }

    // MARK: The control channel

    /// `state.get audio_editor` (Ruling 10). The last width given to
    /// `sync(width:)` is synced again first, so after a selection change the
    /// state names the new region and its fit even before the canvas
    /// redraws (a width of 0 keeps the default axis).
    public func stateRow() -> AudioEditorStateRow {
        sync(width: width)
        let bottom = document.editor.layout.views.current.bottom
        let shown = bottom.shown && bottom.view == .audioEditor
        var region: AudioEditorRegionRow?
        var boxes: [AudioEditorBoxRow] = []
        if let r = document.editedRegion {
            region = AudioEditorRegionRow(
                event: r.row.id, media: r.row.media ?? 0, name: r.media?.name ?? "", gainDb: r.row.gainDb,
                fadeIn: r.row.fadeIn, fadeOut: r.row.fadeOut, sourceOffset: r.row.sourceOffset,
                length: r.row.frames, mediaFrames: r.media?.frames ?? 0, sampleRate: r.media?.sampleRate ?? 0,
                offline: r.media == nil || r.media?.offline != nil, crossfade: r.row.crossfade,
                crossfadeWith: r.row.crossfade ? r.row.overlapWith : nil)
            boxes = AudioEditorBox.allCases.map { AudioEditorBoxRow(name: $0.rawValue, text: text($0)) }
        }
        return AudioEditorStateRow(
            shown: shown, region: region, message: region == nil ? Self.emptyMessage : nil,
            framesPerPoint: axis.framesPerPoint, originFrame: axis.originFrame, amplitude: UInt32(amplitude),
            boxes: boxes)
    }

    // MARK: Typed values

    private static let none = "\u{2013}"

    /// What `box` shows now: "–" for every box with no region, and for a
    /// time box without a media row. A crossfaded region's fade in shows in
    /// the Crossfade box, and the Fade In box shows "–" (plan 6c); without
    /// a crossfade the Crossfade box shows "–".
    public func text(_ box: AudioEditorBox) -> String {
        guard let r = document.editedRegion else { return Self.none }
        if box == .peak { return AudioEditorEntry.peakText(peakDb) }
        let row = r.row
        if box == .gain { return AudioEditorEntry.gainText(Double(row.gainDb)) }
        guard let rate = r.media?.sampleRate, rate > 0 else { return Self.none }
        switch box {
        case .fadeIn, .crossfade:
            guard row.crossfade == (box == .crossfade) else { return Self.none }
            return AudioEditorEntry.fadeText(frames: row.fadeIn.length, rate: rate)
        case .fadeOut: return AudioEditorEntry.fadeText(frames: row.fadeOut.length, rate: rate)
        case .start: return Formatting.time(Double(row.sourceOffset) / Double(rate))
        case .length: return Formatting.time(Double(row.frames) / Double(rate))
        case .gain, .peak: return Self.none
        }
    }

    /// Commits `text` to `box`: parsed, clamped and sent as one edit. Text
    /// equal to what the box shows, or a value equal to the current one,
    /// sends nothing and answers true. A refusal sets the box's hint as
    /// the status, sends nothing and answers false; the Crossfade box of a
    /// region without one, and the Fade In box of a crossfade, refuse with
    /// their own hints (plan 6c).
    @discardableResult
    public func type(_ box: AudioEditorBox, _ text: String) -> Bool {
        let editor = document.editor
        guard let r = document.editedRegion else {
            editor.status = Self.emptyMessage
            return false
        }
        if box.isTime, (r.media?.sampleRate ?? 0) == 0 {
            editor.status = AudioEditorEntry.noMediaHint
            return false
        }
        // The box shows a rounded value (fades in whole ms, gain in
        // tenths): its own text back moves nothing (Task 2 review M1). Both
        // sides are compared cleaned, as `NumberBox` commits its text with
        // U+2212 read as "-" (final review I1).
        if let typed = StripEntry.clean(text), typed == StripEntry.clean(self.text(box)) { return true }
        // A crossfade's length is typed in its own box (plan 6c); its "–"
        // back, above, moves nothing.
        if box == .crossfade, !r.row.crossfade {
            editor.status = AudioEditorEntry.noCrossfadeHint
            return false
        }
        if box == .fadeIn, r.row.crossfade {
            editor.status = AudioEditorEntry.crossfadeInHint
            return false
        }
        guard let parsed = command(box, text, r) else {
            editor.status = box.hint
            return false
        }
        guard let cmd = parsed else { return true }
        return editor.edit(cmd)
    }

    /// nil when refused; `.some(nil)` when the value is the current one.
    private func command(_ box: AudioEditorBox, _ text: String, _ r: EditedRegion) -> EditCommand?? {
        let row = r.row
        let rate = r.media?.sampleRate ?? 0
        let frames = Int64(clamping: row.frames)
        let offset = Int64(clamping: row.sourceOffset)
        switch box {
        case .gain:
            guard let db = AudioEditorEntry.gain(text) else { return nil }
            let g = Float(db)
            return .some(g == row.gainDb ? nil : .audio(edit: .setGain(event: row.id, gainDb: g)))
        case .peak:
            guard let db = AudioEditorEntry.peak(text) else { return nil }
            if peakDb != db { peakDb = db }
            return .some(nil)
        case .fadeIn, .fadeOut:
            guard let typed = AudioEditorEntry.fadeFrames(text, rate: rate) else { return nil }
            let other = box == .fadeIn ? row.fadeOut.length : row.fadeIn.length
            let room = max(frames - max(other, 0), 0)
            let length = min(typed, room)
            var fadeIn = row.fadeIn
            var fadeOut = row.fadeOut
            if box == .fadeIn { fadeIn.length = length } else { fadeOut.length = length }
            guard fadeIn != row.fadeIn || fadeOut != row.fadeOut else { return .some(nil) }
            return .some(.audio(edit: .setFades(event: row.id, fadeIn: fadeIn, fadeOut: fadeOut)))
        case .crossfade:
            // At least one frame, at most the room the overlap and the fade
            // out leave.
            guard let typed = AudioEditorEntry.fadeFrames(text, rate: rate) else { return nil }
            let length = min(max(typed, 1), max(RegionCommands.crossfadeRoom(row), 1))
            guard length != row.fadeIn.length else { return .some(nil) }
            return .some(
                .audio(
                    edit: .crossfade(event: row.id, length: .frames(frames: UInt64(length)), shape: row.fadeIn.shape)))
        case .start:
            guard let s = AudioEditorEntry.duration(text, bare: .s) else { return nil }
            let low = max(0, offset - Int64(clamping: startRoom))
            let high = max(low, offset + frames - 1)
            let start = min(max(AudioEditorEntry.frames(s, rate: rate), low), high)
            let front = start - offset
            return .some(front == 0 ? nil : .audio(edit: .trim(event: row.id, frontFrames: front, backFrames: 0)))
        case .length:
            guard let s = AudioEditorEntry.duration(text, bare: .s) else { return nil }
            let media = Int64(clamping: r.media?.frames ?? 0)
            let high = max(1, media - offset)
            let length = min(max(AudioEditorEntry.frames(s, rate: rate), 1), high)
            let back = frames - length
            return .some(back == 0 ? nil : .audio(edit: .trim(event: row.id, frontFrames: 0, backFrames: back)))
        }
    }

    /// A new shape for `.fadeIn`, `.fadeOut` or `.crossfade`, keeping its
    /// length; other boxes, the same shape, or `.crossfade` on a region
    /// without one, send nothing.
    public func setShape(_ box: AudioEditorBox, _ shape: FadeShapeRow) {
        guard box == .fadeIn || box == .fadeOut || box == .crossfade else { return }
        guard let row = document.editedRegion?.row else {
            document.editor.status = Self.emptyMessage
            return
        }
        if box == .crossfade {
            guard row.crossfade, row.fadeIn.shape != shape else { return }
            let length = UInt64(clamping: max(row.fadeIn.length, 1))
            document.editor.edit(.audio(edit: .crossfade(event: row.id, length: .frames(frames: length), shape: shape)))
            return
        }
        var fadeIn = row.fadeIn
        var fadeOut = row.fadeOut
        if box == .fadeIn { fadeIn.shape = shape } else { fadeOut.shape = shape }
        guard fadeIn != row.fadeIn || fadeOut != row.fadeOut else { return }
        document.editor.edit(.audio(edit: .setFades(event: row.id, fadeIn: fadeIn, fadeOut: fadeOut)))
    }

    /// Normalizes the shown region to the Peak box's level: no panel, ever.
    /// Busy and offline refusals go to the status line.
    @discardableResult
    public func normalize() -> KeyOutcome {
        guard document.editedRegion != nil else {
            document.editor.status = Self.emptyMessage
            return .unavailable(Self.emptyMessage)
        }
        return RegionCommands.normalize(peakDb: peakDb, document)
    }
}
