import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
private func panel() throws -> (FakeSession, DocumentModel, SelectModifyModel) {
    let f = rollFake()
    let m = try DocumentModel(session: f)
    return (f, m, m.selectModify)
}

@MainActor
private func selectChords(_ m: DocumentModel, range: TickRange? = nil) {
    m.editor.setSelection(SelectionRow(timeRange: range, lanes: [LaneRef(seq: 3, lane: 31)], events: [1]))
}

@MainActor
@Suite struct SelectModifyModelTests {
    @Test func openingATaskPresetsAndPresentsThePanel() throws {
        let (_, m, s) = try panel()
        var presented = 0
        s.onPresent = { presented += 1 }
        s.open(.quantize)
        #expect(s.isOpen && s.task == .quantize && presented == 1)
        #expect(s.config == SelectModifyConfig.preset(.quantize))
        selectChords(m, range: TickRange(start: 0, end: 61_440))
        s.open(.selectByRule)
        #expect(s.config.select.conditions.last?.test == .timeRange(start: 0, end: 61_440))
        s.close()
        #expect(!s.isOpen && !s.isKey)
    }

    @Test func applySendsOneCommandWithTheOpeningListsFilter() throws {
        let (f, m, s) = try panel()
        selectChords(m)
        let hidden = ViewFilterRow(hiddenKinds: [.cc], hiddenLanes: [])
        s.open(.transpose, filter: hidden)
        s.setModifyText("7", 0)
        #expect(s.apply())
        #expect(f.edits.last == .selectAndModify(rule: nil, mode: .select, modify: .transposeInterval(semitones: 7), toCopy: false))
        #expect(f.editFilters.last == hidden)
    }

    @Test func applyWithoutLanesSaysWhatToSelect() throws {
        let (f, m, s) = try panel()
        s.open(.selectByRule)
        #expect(!s.apply())
        #expect(m.editor.status == SelectModifyModel.noLanesMessage && f.edits.isEmpty)
        selectChords(m)
        #expect(s.apply())
        #expect(f.edits.count == 1)
    }

    @Test func applyWithNothingTurnedOnSaysSo() throws {
        let (f, m, s) = try panel()
        s.open(.change)
        s.config.modify.enabled = false
        #expect(!s.apply())
        #expect(m.editor.status == SelectModifyModel.nothingMessage && f.edits.isEmpty)
    }

    @Test func typedFieldsKeepDraftsAndRefuseBadText() throws {
        let (f, m, s) = try panel()
        selectChords(m)
        s.open(.change)
        s.setModifyText("abc", 0)
        #expect(s.modifyText(0) == "abc" && s.invalid == ["m0"])
        #expect(s.config.modify.op == .change(target: .velocity, op: .add, amount: .constant(value: 0)))
        #expect(!s.apply())
        #expect(m.editor.status == SelectModifyModel.invalidMessage && f.edits.isEmpty)
        s.setModifyText("10", 0)
        #expect(s.invalid.isEmpty)
        #expect(s.config.modify.op == .change(target: .velocity, op: .add, amount: .constant(value: 5120)))
        s.setModifyKind(.move)
        #expect(s.drafts.isEmpty && s.modifyText(0) == "0.0")
    }

    @Test func conditionsAndChangeOperationsEdit() throws {
        let (_, m, s) = try panel()
        s.open(.change)
        s.addCondition(.velocity)
        #expect(s.config.select.enabled && s.config.select.conditions.count == 1)
        s.setConditionText("100", 0, 0)
        s.setComparison(.equal, at: 0)
        #expect(s.config.select.conditions[0].test == .velocity(low: 100 << 9, high: (100 << 9) | 511))
        #expect(s.conditionText(0, 1) == "100")
        s.setTarget(.duration)
        s.setOp(.scale)
        #expect(s.config.modify.op == .change(target: .duration, op: .scale, amount: .constant(value: 100)))
        s.setAmount(.line)
        s.setCurve(.sCurve)
        #expect(s.config.modify.op == .change(target: .duration, op: .scale, amount: .line(from: 0, to: 0, curve: .sCurve)))
        s.setModifyKind(.diatonic)
        s.setScale(root: 2, scale: .dorian)
        #expect(s.config.modify.op == .transposeDiatonic(steps: 1, root: 2, mask: Scale.dorian.mask))
        s.setModifyKind(.noteMap)
        s.setNoteMapText("C2>D2")
        #expect(s.config.modify.op == .noteMap(map: [NoteMapEntry(from: 36, to: 38)]))
        s.removeCondition(at: 0)
        #expect(s.config.select.conditions.isEmpty)
        withExtendedLifetime(m) {}
    }
    @Test func templatesSaveLoadRunAndDelete() throws {
        let (f, m, s) = try panel()
        selectChords(m)
        s.open(.transpose)
        s.setModifyText("12", 0)
        s.templateName = "  "
        #expect(!s.saveTemplate() && m.editor.status == SelectModifyModel.noNameMessage)
        s.templateName = "Up"
        #expect(s.saveTemplate())
        let t = ModifyTemplateRow(name: "Up", rule: nil, mode: .select, modify: .transposeInterval(semitones: 12), toCopy: false)
        #expect(f.edits.last == .settings(edit: .saveTemplate(template: t)))
        s.open(.move)
        #expect(s.run(t))
        #expect(f.edits.last == .selectAndModify(rule: nil, mode: .select, modify: .transposeInterval(semitones: 12), toCopy: false))
        s.deleteTemplate("Up")
        #expect(f.edits.last == .settings(edit: .deleteTemplate(name: "Up")))
        s.load(t)
        #expect(s.config == SelectModifyConfig(template: t) && s.templateName == "Up")
    }

    @Test func modifyAgainSendsModifyAgain() throws {
        let (f, m, s) = try panel()
        #expect(s.modifyAgain())
        #expect(f.edits.last == .modifyAgain)
        withExtendedLifetime(m) {}
    }
    @Test func randomAmountsGetANewSeedAtEachApply() throws {
        let (f, m, s) = try panel()
        selectChords(m)
        var seeds: [UInt64] = [11, 22]
        s.nextSeed = { seeds.removeFirst() }
        s.open(.change)
        s.config.modify.op = .change(target: .velocity, op: .set, amount: .random(low: 0, high: 10, seed: 0))
        s.apply()
        s.apply()
        let used = f.edits.compactMap { e -> UInt64? in
            guard case .selectAndModify(_, _, .change(_, _, .random(_, _, let seed))?, _) = e else { return nil }
            return seed
        }
        #expect(used == [11, 22])
    }

    @Test func aKeyPanelTurnsTheTransportKeysOff() throws {
        let (_, m, s) = try panel()
        s.isKey = true
        #expect(!m.transportKeysEnabled && m.editor.panelKey)
        s.isKey = false
        #expect(m.transportKeysEnabled)
    }

    @Test func exactModeSelectsTheDraggedEventsAndAppliesOffGridAmountsUnedited() throws {
        let (f, m, s) = try panel()
        let off: Int64 = 1_234  // not on the length display grid
        let req = ModifyRequest(
            task: .change,
            config: {
                var c = SelectModifyConfig.preset(.change)
                c.modify.op = .change(target: .duration, op: .add, amount: .constant(value: off))
                return c
            }(), events: [7, 8], lanes: [LaneKey(3, 31)], range: nil, droppedAxis: .vertical)
        s.open(req)
        #expect(s.notice == DroppedAxis.vertical.message)
        #expect(s.apply())
        #expect(f.selectionSets.last?.events == [7, 8])
        #expect(
            f.edits.last
                == .selectAndModify(
                    rule: nil, mode: .select,
                    modify: .change(target: .duration, op: .add, amount: .constant(value: off)), toCopy: false))
        #expect(s.notice == nil)
        withExtendedLifetime(m) {}
    }
    @Test func exactModeSelectionIsSetBeforeTheCommand() throws {
        let (f, m, s) = try panel()
        s.open(
            ModifyRequest(
                task: .move, config: .preset(.move), events: [1], lanes: [LaneKey(3, 31)], range: nil))
        #expect(s.apply())
        #expect(f.selectionSets.last?.events == [1] && f.selectionSets.last?.lanes == [LaneRef(seq: 3, lane: 31)])
        withExtendedLifetime(m) {}
    }
    @Test func deleteTemplateTrimsTheName() throws {
        let (f, m, s) = try panel()
        s.deleteTemplate("  Up ")
        #expect(f.edits.last == .settings(edit: .deleteTemplate(name: "Up")))
        withExtendedLifetime(m) {}
    }
    @Test func modifyAgainHelpSaysInPlace() {
        #expect(SelectModifyModel.modifyAgainHelp == "Modify Again changes the selection in place.")
    }
}
