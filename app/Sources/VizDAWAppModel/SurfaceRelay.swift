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
    /// An `apply` is running: a nested one (a dialog's modal run loop
    /// still ticks) defers its events to it (final review R3).
    private var applying = false
    private var deferred: [SurfaceEventRow] = []

    /// Commands that can open a dialog (those `ControlCommands` answers
    /// `dialog_required` for): run without the keyboard bypass, which
    /// would otherwise hold for as long as the dialog is open (final
    /// review R3). They have no keyboard guard to bypass.
    public static let dialogCommands: Set<String> = [
        "file.open", "file.close", "file.save", "file.save_as", "file.project_settings", "keys.save", "keys.load",
        "media.import", "media.relink", "media.rename", "record.recover", RegionCommands.normalize,
        BounceCommands.id,
    ]

    public init(now: @escaping () -> ContinuousClock.Instant = { ContinuousClock.now }) {
        self.now = now
    }

    /// Select and arm on `m`; a device's commands through `perform`,
    /// programmatic ones through `quietly`; jog detents summed.
    ///
    /// A surface button is not a keystroke (ruling T9-1): while each event
    /// applies, a text field having the keyboard holds nothing back. Events
    /// that arrive while one runs (a nested call) apply after it, in order
    /// (final review R3).
    public func apply(
        _ events: [SurfaceEventRow], _ m: DocumentModel, perform: (String) -> Void, quietly: (String) -> Void
    ) {
        guard !events.isEmpty else { return }
        if applying {
            deferred.append(contentsOf: events)
            return
        }
        applying = true
        defer { applying = false }
        var batch = events
        while !batch.isEmpty {
            for e in batch { handle(e, m, perform: perform, quietly: quietly) }
            batch = deferred
            deferred = []
        }
    }

    private func handle(
        _ e: SurfaceEventRow, _ m: DocumentModel, perform: (String) -> Void, quietly: (String) -> Void
    ) {
        // Detents waiting for the window are located first, so a Play or
        // Record right after a spin starts from the jogged place and no
        // locate lands mid-pass (review I1).
        if case .jog = e {} else { acting(m) { locatePending(m) } }
        switch e {
        case .command(let id, let programmatic):
            // `surface.inject` and `midi.inject` never take the key path,
            // which could open a dialog (pre-flight I9).
            if programmatic {
                acting(m) { quietly(id) }
            } else if Self.dialogCommands.contains(id) {
                perform(id)
            } else {
                acting(m) { perform(id) }
            }
        case .select(let strip):
            m.selectedStrips = [strip]
        case .arm(_, let path):
            acting(m) { arm(path, m) }
        case .countIn:
            // The keyboard's Count-in (plan 5f-4 Ruling 16).
            m.record.toggleCountIn()
        case .jog(let detents):
            if jogSince == nil { jogSince = now() }
            jogDetents += Int64(detents)
        }
    }

    /// Runs `body` with the surfaces' keyboard bypass on (ruling T9-1).
    private func acting(_ m: DocumentModel, _ body: () -> Void) {
        let was = m.surfaceActing
        m.surfaceActing = true
        defer { m.surfaceActing = was }
        body()
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
