import Foundation
import VizDAWCore

/// What the app does with surface events (plan 5f-1 Ruling 12): commands
/// run, selection and arm change the document, jog wheels locate.
@MainActor public final class SurfaceRelay {
    /// How long jog detents are summed into one locate.
    public static let jogWindow: Duration = .milliseconds(150)

    private let now: () -> ContinuousClock.Instant
    /// Detents not yet located by, and when the first of them came.
    private var jogDetents: Int64 = 0
    private var jogSince: ContinuousClock.Instant?

    public init(now: @escaping () -> ContinuousClock.Instant = { ContinuousClock.now }) {
        self.now = now
    }

    /// Select and arm on `m`; a device's commands through `perform`,
    /// programmatic ones through `quietly`; jog detents summed.
    ///
    /// A surface button is not a keystroke (ruling T9-1): while the events
    /// apply, a text field having the keyboard holds nothing back.
    public func apply(
        _ events: [SurfaceEventRow], _ m: DocumentModel, perform: (String) -> Void, quietly: (String) -> Void
    ) {
        guard !events.isEmpty else { return }
        let was = m.surfaceActing
        m.surfaceActing = true
        defer { m.surfaceActing = was }
        for e in events {
            // Detents waiting for the window are located first, so a Play
            // or Record right after a spin starts from the jogged place and
            // no locate lands mid-pass (review I1).
            if case .jog = e {} else { locatePending(m) }
            switch e {
            case .command(let id, let programmatic):
                // `surface.inject` and `midi.inject` never take the key
                // path, which could open a dialog (pre-flight I9).
                if programmatic { quietly(id) } else { perform(id) }
            case .select(let strip):
                m.selectedStrips = [strip]
            case .arm(_, let path):
                arm(path, m)
            case .jog(let detents):
                if jogSince == nil { jogSince = now() }
                jogDetents += Int64(detents)
            }
        }
    }

    /// Locates by the summed detents once `jogWindow` passed since the
    /// first unapplied one (`DocumentModel.idle` calls it): each locate
    /// lands a writing pass, so a spun wheel is one locate (pre-flight M3).
    public func flush(_ m: DocumentModel) {
        guard let since = jogSince, now() - since >= Self.jogWindow else { return }
        locatePending(m)
    }

    /// Locates by the detents not yet located by, from where the transport
    /// stands now: a fresh read, since the core may have moved it (a
    /// surface's marker jump) since the last poll (review M2).
    private func locatePending(_ m: DocumentModel) {
        guard jogSince != nil else { return }
        let detents = jogDetents
        jogSince = nil
        jogDetents = 0
        guard detents != 0 else { return }
        m.poll()
        let target = max(0, m.transport.tick + detents * MeterMath.ppq / 4)
        do {
            try m.moveTo(target)
        } catch {
            m.editor.status = AlertItem(error, title: "Locate Failed").message
        }
    }

    /// Toggles the arm of the strip's input lane, as the lane header does;
    /// a strip that is no AudioPath (`path: nil`) does nothing.
    private func arm(_ path: UInt64?, _ m: DocumentModel) {
        guard let path, let seq = m.selectedSeq, let lane = m.inputLane(path: path) else { return }
        _ = m.editor.edit(.lane(edit: .setFlags(seq: seq, lane: lane.id, mute: nil, solo: nil, arm: !lane.arm)))
    }
}
