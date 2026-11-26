import Foundation
import VizDAWCore

/// The accessibility labels of the group tag, the VCA offset readout and
/// the VCA strip's controls (groups-vcas-transport Task 9), here so the
/// tests reach them (pre-flight F9).
public enum VcaStripLabels {
    public static func group(_ name: String) -> String { "Group \(name)" }
    public static func offset(_ text: String) -> String { "VCA offset \(text)" }
    /// A VCA's label, as the core's `vca_label`: its name when that is
    /// "VCA" or starts with "VCA ", else "VCA ‹name›".
    public static func title(_ name: String) -> String {
        name == "VCA" || name.hasPrefix("VCA ") ? name : "VCA \(name)"
    }
    public static func fader(_ name: String) -> String { "Fader of \(title(name))" }
    public static func level(_ name: String) -> String { "Level of \(title(name))" }
    public static func mute(_ name: String) -> String { "Mute \(title(name))" }
    public static func solo(_ name: String) -> String { "Solo \(title(name))" }
    public static let mode = "VCA automation mode"
    /// The strip's "VCA" mark: a VCA mutes or solos it (spec §2a).
    public static func mark(_ strip: String) -> String { "VCA mute or solo on \(strip)" }
    /// "1 member", "3 members".
    public static func members(_ n: Int) -> String { n == 1 ? "1 member" : "\(n) members" }
}

extension AutomationModes {
    /// A VCA's modes, in menu order (Ruling 6; the registry's
    /// `VCA_MODE_NAMES`, pre-flight F10): Write and Trim are not among
    /// them.
    public static let vca: [AutomationModeRow] = [.off, .read, .touch, .latch]

    /// The registry's name of `m` ("off" … "trim").
    nonisolated public static func name(_ m: AutomationModeRow) -> String { title(m).lowercased() }
}

extension DocumentModel {
    /// The selected strips a group or VCA may take (Ruling 11): in ID
    /// order, without master and without strips the mixer no longer has.
    public func selectedMemberStrips() -> [UInt64] {
        guard let mixer = try? session.mixer() else { return selectedStrips.sorted() }
        let members = Set(mixer.strips.filter { $0.kind != .master }.map(\.id))
        return selectedStrips.intersection(members).sorted()
    }

    /// `strips` as a `strips` argument ("2,3"); none for an empty list.
    public static func stripsArg(_ strips: [UInt64]) -> [ControlArg] {
        guard !strips.isEmpty else { return [] }
        return [ControlArg(name: "strips", value: .text(value: strips.map(String.init).joined(separator: ",")))]
    }
}

/// Fader groups and VCAs in the mixer (groups-vcas-transport Task 9):
/// group tags, the VCA section, the members' offsets and the Mixer menu.
/// Every edit is one `runMixer` call, so the core's rules (one group per
/// strip, the members' limits) decide; a refusal shows its alert.
extension MixerModel {
    public var groups: [GroupRow] { snapshot.groups }
    public var vcas: [VcaRow] { snapshot.vcas }

    /// The group `s` is in.
    public func group(_ s: StripRow) -> GroupRow? {
        guard let g = s.group else { return nil }
        return groups.first { $0.id == g }
    }

    public func vca(_ id: UInt64) -> VcaRow? { vcas.first { $0.id == id } }

    // MARK: Offsets

    /// The sum of `s`'s VCAs' shown values (live, the core's, heard, the
    /// snapshot's), in dB: the floor when any is at it or the sum reaches
    /// it, 0 in none. It re-derives `StripRow.vcaOffsetDb` so the readout
    /// follows a VCA drag (pre-flight F25).
    public func vcaOffsetDb(_ s: StripRow) -> Double {
        var sum = 0.0
        for id in s.vcas {
            guard let v = vca(id) else { continue }
            let db = vcaGainDb(v)
            if db.isNaN || db <= MixerMath.floorDb { return MixerMath.floorDb }
            sum += db
        }
        return sum <= MixerMath.floorDb ? MixerMath.floorDb : sum
    }

    /// "VCA −3.0", "VCA +2.0", "VCA −∞"; nil when the offset reads 0.0.
    public static func vcaOffsetText(_ db: Double) -> String? {
        if db.isNaN || db <= MixerMath.floorDb { return "VCA \u{2212}\u{221E}" }
        let text = String(format: "%.1f", abs(db))
        guard text != "0.0" else { return nil }
        return "VCA " + (db < 0 ? "\u{2212}" : "+") + text
    }

    /// A VCA mutes or solos `s` (the snapshot's mark).
    public func vcaMark(_ s: StripRow) -> Bool { s.vcaMark }

    // MARK: The selection

    /// The selected strips a group or VCA may take (`selectedMemberStrips`).
    public var selectedMembers: [UInt64] {
        let members = Set(strips.map(\.id))
        return document.selectedStrips.intersection(members).sorted()
    }

    /// A strip menu's strips: the selection when it holds `s`, else `s`.
    public func menuStrips(_ s: StripRow) -> [UInt64] {
        guard s.kind != .master else { return [] }
        let sel = selectedMembers
        return sel.contains(s.id) ? sel : [s.id]
    }

    /// Group takes `strips` when there is one and none is in a group (the
    /// core refuses a strip in another group, Ruling 3).
    public func canGroup(_ strips: [UInt64]) -> Bool {
        !strips.isEmpty && strips.allSatisfy { id in snapshot.strips.first { $0.id == id }?.group == nil }
    }

    /// Mixer › Group.
    public var canGroupSelection: Bool { canGroup(selectedMembers) }

    /// The one group every selected strip is in (Mixer › Ungroup), if any.
    public var selectionGroup: GroupRow? {
        let sel = selectedMembers
        guard let first = sel.first, let row = snapshot.strips.first(where: { $0.id == first }),
            let g = group(row)
        else { return nil }
        return sel.allSatisfy { id in snapshot.strips.first { $0.id == id }?.group == g.id } ? g : nil
    }

    /// Of `strips`, those `v` does not hold yet (assigning a member again is
    /// refused).
    public func assignable(_ strips: [UInt64], to v: VcaRow) -> [UInt64] {
        strips.filter { !v.members.contains($0) }
    }

    /// The VCAs holding any of `strips`.
    public func vcasHolding(_ strips: [UInt64]) -> [VcaRow] {
        vcas.filter { v in strips.contains { v.members.contains($0) } }
    }

    // MARK: Edits

    /// `mixer.new_group` with the selected strips.
    public func groupSelection() { newGroup(selectedMembers) }

    /// `mixer.new_group` of `strips`.
    public func newGroup(_ strips: [UInt64]) {
        run("mixer.new_group", DocumentModel.stripsArg(strips), "Group Failed")
    }

    /// Mixer › Ungroup: deletes the selection's group.
    public func ungroupSelection() {
        guard let g = selectionGroup else { return }
        deleteGroup(g.id)
    }

    public func deleteGroup(_ group: UInt64) {
        run("mixer.delete_group", [Self.id("group", group)], "Ungroup Failed")
    }

    public func removeFromGroup(_ group: UInt64, _ strips: [UInt64]) {
        guard !strips.isEmpty else { return }
        run("mixer.remove_from_group", [Self.id("group", group)] + DocumentModel.stripsArg(strips), "Group Failed")
    }

    /// The group's mute or solo link switch.
    public func setGroupLinks(_ group: UInt64, mute: Bool? = nil, solo: Bool? = nil) {
        var args = [Self.id("group", group)]
        if let mute { args.append(ControlArg(name: "mute", value: .flag(value: mute))) }
        if let solo { args.append(ControlArg(name: "solo", value: .flag(value: solo))) }
        guard args.count > 1 else { return }
        run("mixer.set_group_links", args, "Group Failed")
    }

    /// `mixer.new_vca`, with the selected strips when `fromSelection`.
    public func newVca(fromSelection: Bool) {
        newVca(fromSelection ? selectedMembers : [])
    }

    public func newVca(_ strips: [UInt64]) {
        run("mixer.new_vca", DocumentModel.stripsArg(strips), "New VCA Failed")
    }

    /// Assigns `strips` to VCA `vca` (`on`) or removes them from it.
    public func assign(_ strips: [UInt64], toVca vca: UInt64, _ on: Bool) {
        guard !strips.isEmpty else { return }
        run(
            on ? "mixer.assign_vca" : "mixer.unassign_vca", [Self.id("vca", vca)] + DocumentModel.stripsArg(strips),
            "VCA Failed")
    }

    public func deleteVca(_ vca: UInt64) {
        run("mixer.delete_vca", [Self.id("vca", vca)], "Delete VCA Failed")
    }

    public func toggleVcaMute(_ v: VcaRow) { setVca(v.id, ControlArg(name: "mute", value: .flag(value: !v.mute))) }
    public func toggleVcaSolo(_ v: VcaRow) { setVca(v.id, ControlArg(name: "solo", value: .flag(value: !v.solo))) }

    public func setVcaMode(_ v: VcaRow, _ mode: AutomationModeRow) {
        guard mode != v.automation else { return }
        setVca(v.id, ControlArg(name: "mode", value: .text(value: AutomationModes.name(mode))))
    }

    /// A typed VCA level, as one click on its fader (`typeLevel`'s rule).
    public func typeVcaLevel(_ v: VcaRow, _ text: String) -> Bool {
        guard let db = StripEntry.level(text) else {
            document.editor.status = StripEntry.levelHint
            return false
        }
        let t = AutomationTargetRow.vcaFader(vca: v.id)
        guard !isTouched(t) else {
            document.editor.status = StripEntry.touchedHint
            return false
        }
        click(t, AutomationLaw.faderNorm(db))
        return true
    }

    // MARK: Renaming a VCA

    public func beginVcaRename(_ v: VcaRow, in p: MixerPlacement = .window) {
        vcaRenamePlacement = p
        renameDraft = v.name
        renamingVca = v.id
    }

    public func isRenamingVca(_ vca: UInt64, in p: MixerPlacement) -> Bool {
        renamingVca == vca && vcaRenamePlacement == p
    }

    public func commitVcaRename() {
        guard let id = renamingVca else { return }
        renamingVca = nil
        let name = renameDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name != vca(id)?.name else { return }
        run(
            "mixer.rename_vca", [Self.id("vca", id), ControlArg(name: "name", value: .text(value: name))],
            "Rename Failed")
    }

    public func cancelVcaRename() { renamingVca = nil }

    // MARK: Plumbing

    private func setVca(_ vca: UInt64, _ arg: ControlArg) {
        run("mixer.set_vca", [Self.id("vca", vca), arg], "VCA Failed")
    }

    private static func id(_ name: String, _ id: UInt64) -> ControlArg {
        ControlArg(name: name, value: .int(value: Int64(clamping: id)))
    }

    private func run(_ id: String, _ args: [ControlArg], _ title: String) {
        command(title) { try $0.runMixer(id: id, args: args) }
    }
}
