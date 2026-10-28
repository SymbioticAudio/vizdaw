import Foundation
import Observation
import VizDAWCore

/// Fader, meter and pan arithmetic for the mixer window.
public enum MixerMath {
    /// Fader floor: at or below it the fader is −∞ (3a Plan Ruling 4).
    public static let floorDb = -144.0
    public static let maxDb = 12.0
    /// The lowest dB a fader position other than the bottom shows.
    public static let faderBottomDb = -60.0

    /// Fader position 0…1 for `db`: −∞ at 0, then linear in dB from −60
    /// to +12.
    public static func faderPosition(_ db: Double) -> Double {
        guard db > faderBottomDb else { return 0 }
        return min(1, (db - faderBottomDb) / (maxDb - faderBottomDb))
    }

    /// The dB of fader position `p` (the inverse of `faderPosition`).
    public static func faderDb(_ p: Double) -> Double {
        guard p > 0.001 else { return floorDb }
        let db = faderBottomDb + min(1, p) * (maxDb - faderBottomDb)
        return (db * 10).rounded() / 10
    }

    /// "−∞", "0.0", "+3.5", "-12.0".
    public static func dbLabel(_ db: Double) -> String {
        if db.isNaN || db <= floorDb { return "−∞" }
        let text = String(format: "%.1f", db)
        return db > 0 ? "+" + text : text
    }

    /// Meter fill 0…1 for a linear peak: −60 dBFS to +6 dBFS.
    public static func meterFill(_ peak: Float) -> Double {
        guard peak > 0, peak.isFinite else { return 0 }
        let db = 20 * log10(Double(peak))
        return max(0, min(1, (db + 60) / 66))
    }

    /// "C", "L50", "R25".
    public static func panLabel(_ pan: Double) -> String {
        let n = Int((abs(pan) * 100).rounded())
        if n == 0 { return "C" }
        return (pan < 0 ? "L" : "R") + "\(n)"
    }

    /// The largest peak a meter holds (+6 dBFS, the top of its scale).
    public static let maxMeterPeak: Float = 2

    /// NaN reads as silence for that poll; +inf and huge peaks clamp so a
    /// held meter always falls again.
    public static func sanitizedPeak(_ p: Float) -> Float {
        if p.isNaN || p < 0 { return 0 }
        return min(p, maxMeterPeak)
    }

    /// How much a held meter keeps per 30 Hz poll (it falls about 40 dB
    /// a second).
    public static let meterDecay: Float = 0.85
}

/// A meter's two channels as shown (peaks with a falling hold).
public struct MeterLevel: Equatable, Sendable {
    public var left: Float
    public var right: Float

    public init(left: Float, right: Float) {
        self.left = left
        self.right = right
    }

    /// No signal.
    public static let silent = MeterLevel(left: 0, right: 0)
}

/// One document's mixer window (spec §11): strips, slots, sends, routing,
/// meters at 30 Hz. Edits go through the session as undo steps; a drag
/// (fader, pan, send level) is one step.
@Observable
@MainActor
public final class MixerModel {
    public let document: DocumentModel
    public private(set) var snapshot: MixerSnapshot
    public private(set) var meters: [UInt64: MeterLevel] = [:]
    public private(set) var dspLoad: Float = 0
    public private(set) var dspPeak: Float = 0
    public private(set) var latencyFrames: UInt64 = 0
    /// Plugins the engine bypassed for faults (NaN output, errors).
    public private(set) var faulted: Set<UInt64> = []
    /// Slots safe mode keeps off.
    public private(set) var held: Set<UInt64> = []
    /// The instrument list (lanes play through these).
    public private(set) var destinations: [DestRow] = []
    /// The strip whose name is being edited.
    public private(set) var renamingStrip: UInt64?
    public var renameDraft = ""
    /// The open plugin browser.
    public var browser: BrowserTarget? {
        didSet { if browser == nil { browserModels = [:] } }
    }
    /// Times the mixer was fetched (tests check refetch rules).
    public private(set) var fetches = 0

    /// Where a plugin picked in the browser goes.
    public enum BrowserTarget: Equatable, Sendable {
        /// The end of a strip's insert chain.
        case insert(strip: UInt64)
        /// A new plugin instrument.
        case instrument
        /// An existing destination, which then plays the plugin.
        case destination(UInt64)

        /// The browser lists instruments for it (effects for an insert).
        public var wantsInstrument: Bool {
            if case .insert = self { return false }
            return true
        }
    }

    private var dragOpen = false
    private var dragStrip: UInt64?
    /// A refetch hit Busy and is still owed.
    private var refetchOwed = false
    @ObservationIgnored private var browserModels: [String: PluginBrowserModel] = [:]

    /// The browser model of the open popover: made once per presentation
    /// (so typed search text survives re-renders) and dropped on close.
    public func browserModel(
        _ kind: PluginKindTag, make: () -> PluginBrowserModel
    ) -> PluginBrowserModel {
        let key = "\(kind)"
        if let m = browserModels[key] { return m }
        let m = make()
        browserModels[key] = m
        return m
    }

    private var seen: (version: UInt64, slots: Int)

    public init(document: DocumentModel) throws {
        self.document = document
        snapshot = try document.session.mixer()
        seen = (document.snapshot.version, document.slotsVersion)
        held = Set((try? document.session.heldSlots()) ?? [])
        destinations = (try? document.session.destinations()) ?? []
        fetches = 1
    }

    private var session: any SessionProtocol { document.session }

    // MARK: Derived

    /// Every strip but master, in display order.
    public var strips: [StripRow] { snapshot.strips.filter { $0.kind != .master } }
    public var master: StripRow? { snapshot.strips.first { $0.kind == .master } }

    /// "Master" or the bus's name.
    public func outputName(_ strip: StripRow) -> String {
        guard let b = strip.output else { return "Master" }
        return snapshot.buses.first { $0.id == b }?.name ?? "Bus \(b)"
    }

    /// Buses `strip` may send or output to (not itself).
    public func targets(for strip: StripRow) -> [BusRow] {
        snapshot.buses.filter { $0.id != strip.id }
    }

    /// Strips that may feed a sidechain of `strip`'s inserts (not itself,
    /// not master).
    public func sidechainSources(for strip: StripRow) -> [StripRow] {
        strips.filter { $0.id != strip.id }
    }

    /// The lane an AudioPath strip's input controls edit (plan 5d-2
    /// Ruling 4): of the shown sequence's lanes that play through the
    /// strip's path, the first armed one, else the first; nil for other
    /// strips.
    public func inputLane(for strip: StripRow) -> LaneRow? {
        guard case .audioPath(let path) = strip.kind else { return nil }
        let through = document.lanes.filter { $0.audioOut == path }
        return through.first(where: \.arm) ?? through.first
    }

    public func stripName(_ id: UInt64) -> String {
        snapshot.strips.first { $0.id == id }?.name ?? "Strip \(id)"
    }

    /// "DSP 12% · peak 30% · PDC 10.7 ms".
    public var loadText: String {
        let rate = document.transport.sampleRate
        let pdc = rate > 0 ? String(format: " · PDC %.1f ms", Double(latencyFrames) / rate * 1000) : ""
        return String(format: "DSP %.0f%% · peak %.0f%%", dspLoad * 100, dspPeak * 100) + pdc
    }

    /// The held slots of `strip` (its "Load All").
    public func heldSlots(of strip: StripRow) -> [UInt64] {
        ([strip.instrument].compactMap { $0 } + strip.inserts).map(\.id).filter(held.contains)
    }

    // MARK: Polling

    /// Refetches the mixer when the document or its slots changed.
    public func refreshIfNeeded() {
        let now = (document.snapshot.version, document.slotsVersion)
        guard now != seen || refetchOwed else { return }
        refetch()
    }

    /// Reads meters, load and faults (the window calls it at 30 Hz).
    public func pollLevels() {
        guard let l = try? session.levels() else { return }
        var next: [UInt64: MeterLevel] = [:]
        for m in l.meters {
            let old = meters[m.strip] ?? MeterLevel(left: 0, right: 0)
            next[m.strip] = MeterLevel(
                left: max(MixerMath.sanitizedPeak(m.peakL), old.left * MixerMath.meterDecay),
                right: max(MixerMath.sanitizedPeak(m.peakR), old.right * MixerMath.meterDecay))
        }
        meters = next
        dspLoad = l.dspLoad
        dspPeak = l.dspPeak
        latencyFrames = l.latencyFrames
        faulted = Set(l.faulted)
    }

    // MARK: Intents

    public func addBus() {
        let names = Set(snapshot.buses.map(\.name))
        let name = (1...).lazy.map { "Bus \($0)" }.first { !names.contains($0) } ?? "Bus"
        command("Add Bus Failed") { try $0.addBus(name: name) }
    }

    public func removeBus(_ strip: UInt64) {
        command("Remove Bus Failed") { try $0.removeBus(strip: strip) }
    }

    public func beginRename(_ strip: StripRow) {
        renameDraft = strip.name
        renamingStrip = strip.id
    }

    public func commitRename() {
        guard let id = renamingStrip else { return }
        renamingStrip = nil
        let name = renameDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name != stripName(id) else { return }
        command("Rename Failed") { try $0.renameStrip(strip: id, name: name) }
    }

    public func cancelRename() { renamingStrip = nil }

    /// A drag starts: until `dragEnded`, edits are one undo step.
    public func dragStarted(_ label: String, strip: UInt64? = nil) {
        endOpenDrag()
        do {
            try session.beginEdit(label: label)
            dragOpen = true
            dragStrip = strip
        } catch {
            raise(AlertItem(error, title: "Edit Failed"))
        }
    }

    public func dragEnded() { endOpenDrag() }

    /// Ends the open drag, if any (the window closing, the dragged strip
    /// going away or the document closing must not leave the undo
    /// transaction open).
    public func endOpenDrag() {
        guard dragOpen else { return }
        dragOpen = false
        dragStrip = nil
        command("Edit Failed") { try $0.endEdit() }
    }

    public func setGain(_ strip: UInt64, db: Double) {
        command("Fader Failed") { try $0.setGain(strip: strip, gainDb: db) }
    }

    public func setPan(_ strip: UInt64, _ pan: Double) {
        command("Pan Failed") { try $0.setPan(strip: strip, pan: pan) }
    }

    public func toggleMute(_ strip: StripRow) {
        command("Mute Failed") { try $0.setStripMute(strip: strip.id, mute: !strip.mute) }
    }

    public func toggleSolo(_ strip: StripRow) {
        command("Solo Failed") { try $0.setStripSolo(strip: strip.id, solo: !strip.solo) }
    }

    public func setOutput(_ strip: UInt64, bus: UInt64?) {
        command("Output Failed") { try $0.setOutput(strip: strip, bus: bus) }
    }

    public func addSend(_ strip: UInt64, to bus: UInt64) {
        command("Send Failed") { try $0.addSend(strip: strip, target: bus) }
    }

    public func editSend(_ strip: UInt64, _ index: Int, _ change: (inout SendRow) -> Void) {
        guard let s = snapshot.strips.first(where: { $0.id == strip }), index < s.sends.count else {
            return
        }
        var send = s.sends[index]
        change(&send)
        command("Send Failed") { try $0.editSend(strip: strip, index: UInt32(index), send: send) }
    }

    public func removeSend(_ strip: UInt64, _ index: Int) {
        command("Send Failed") { try $0.removeSend(strip: strip, index: UInt32(index)) }
    }

    /// Adds the plugin picked in the browser where `browser` says.
    public func pick(_ plugin: PluginRefRow) {
        let target = browser
        browser = nil
        switch target {
        case .insert(let strip):
            command("Add Plugin Failed") { try $0.addInsert(strip: strip, index: nil, plugin: plugin) }
        case .instrument:
            command("New Instrument Failed") { try $0.addPluginInstrument(name: plugin.name, plugin: plugin) }
        case .destination(let dest):
            command("Instrument Failed") { try $0.setDestinationPlugin(dest: dest, plugin: plugin) }
        case nil:
            break
        }
    }

    public func removeInsert(_ slot: UInt64) {
        command("Remove Plugin Failed") { try $0.removeInsert(slot: slot) }
    }

    /// Drops slot `slot` at `index` of `strip` (drag to reorder).
    public func moveInsert(_ slot: UInt64, to strip: UInt64, at index: Int) {
        command("Move Plugin Failed") {
            try $0.moveInsert(slot: slot, strip: strip, index: UInt32(max(0, index)))
        }
    }

    public func toggleBypass(_ slot: SlotRow) {
        command("Bypass Failed") { try $0.setBypass(slot: slot.id, bypass: !slot.bypass) }
    }

    public func setSidechain(_ slot: UInt64, source: UInt64?) {
        command("Sidechain Failed") { try $0.setSidechain(slot: slot, source: source) }
    }

    /// Adds a strip for output `index` of `strip`'s instrument.
    public func addOutput(_ strip: StripRow, _ index: UInt16) {
        guard case .instrument(let dest) = strip.kind else { return }
        let name = "\(strip.name) \(Int(index) + 1)"
        command("Add Output Failed") {
            try $0.addInstrumentOutput(dest: dest, index: index, name: name)
        }
    }

    /// Loads held slots (safe mode's Load and Load All).
    public func load(_ slots: [UInt64]) {
        do {
            try session.loadPlugins(slots: slots)
        } catch {
            raise(AlertItem(error, title: "Load Failed"))
        }
        refetch()
    }

    // MARK: Plumbing

    private func command(_ title: String, _ body: (any SessionProtocol) throws -> DocSnapshot) {
        do {
            document.apply(try body(session))
        } catch {
            raise(AlertItem(error, title: title))
        }
        refetch()
    }

    /// Refetches the mixer. `Busy` (a plugin editor is opening in a nested
    /// run loop) skips silently: the previous state stays and the next tick
    /// tries again.
    private func refetch() {
        do {
            let snap = try session.mixer()
            let heldNow = Set(try session.heldSlots())
            let dests = try session.destinations()
            snapshot = snap
            held = heldNow
            destinations = dests
            fetches += 1
            if let d = dragStrip, !snap.strips.contains(where: { $0.id == d }) { endOpenDrag() }
        } catch {
            if case .Busy = error as? VizError {
                refetchOwed = true
                return
            }
            if document.alert == nil {
                document.alert = AlertItem(error, title: "Mixer Unavailable")
            }
        }
        refetchOwed = false
        seen = (document.snapshot.version, document.slotsVersion)
    }

    /// Sets `item` unless the same alert is already showing.
    private func raise(_ item: AlertItem) {
        if let a = document.alert, a.title == item.title, a.message == item.message { return }
        document.alert = item
    }
}
