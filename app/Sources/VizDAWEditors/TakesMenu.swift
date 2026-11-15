import AppKit
import VizDAWAppModel
import VizDAWCore

/// "Takes ▸" (plan 5c-2 takes Ruling 5), shared by the track header's menu
/// and the arrangement's region menu: one item per take, in order, with
/// the playing take checked; then Rename… and Delete for the playing take,
/// Delete disabled when the track has one take. Each runs through
/// `TakeCommands`, so a refusal shows in the status line.
@MainActor
public enum TakesMenu {
    public static func item(document: DocumentModel, seq: UInt64, lane: LaneRow) -> NSMenuItem {
        let item = NSMenuItem(title: "Takes", action: nil, keyEquivalent: "")
        let sub = NSMenu()
        sub.autoenablesItems = false
        item.submenu = sub
        let track = lane.id
        for take in lane.takes {
            let choose = MenuAction(title: take.name) {
                TakeCommands.choose(seq: seq, lane: track, take: take.id, document)
            }
            sub.addItem(choose.item(checked: take.id == lane.activeTakeId))
        }
        sub.addItem(.separator())
        let playing = lane.takes.first { $0.id == lane.activeTakeId }
        let name = playing?.name ?? lane.activeTake
        let take = playing?.id ?? lane.activeTakeId
        let rename = MenuAction(title: "Rename “\(name)”…") {
            TakeCommands.renameAsking(seq: seq, lane: track, take: take, document)
        }
        let renameItem = rename.item(checked: false)
        renameItem.isEnabled = playing != nil
        sub.addItem(renameItem)
        let delete = MenuAction(title: "Delete “\(name)”") {
            TakeCommands.delete(seq: seq, lane: track, take: take, document)
        }
        let deleteItem = delete.item(checked: false)
        deleteItem.isEnabled = playing != nil && lane.takes.count > 1
        sub.addItem(deleteItem)
        return item
    }
}
