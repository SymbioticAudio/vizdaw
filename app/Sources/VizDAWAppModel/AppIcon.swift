import AppKit
import SwiftUI
import SymbioticIcons

/// The icons VizDAW shows, by meaning: the one place an icon is chosen
/// (plan 7b Task B3). Each draws its SF Symbol, `symbol`, the one the app
/// showed before 7b (owner decision, B3 fix round 2). `role` is the
/// SymbioticIcons role it would become, kept for a later swap; nothing
/// draws it yet.
public enum AppIcon: CaseIterable, Sendable {
    // Status.
    case warning, problem, fault, info
    /// The transport bar's Faults button (a dot, tinted by severity).
    case faultIndicator
    case mixer
    // Editing.
    case add, remove, confirm, close
    case zoomIn, zoomOut
    // Inputs and editors.
    case audio, drumGrid, pianoRoll, list
    case muted, loop, split, empty
    case record, armed, pencil, diskSlow
    /// The Record button while recording.
    case recording
    case expand, collapse, menuDown
    // The pane toggles.
    case leftPane, bottomPane, rightPane
    // The Settings tabs.
    case settingsEditing, settingsKeys, settingsAudio, settingsRecording, settingsAutomation, settingsSurfaces,
        settingsAppearance
    case eq
    // Beyond the plan's list (the transport bar and the learn banner).
    case play, stop, midiLearn

    public var role: IconRole {
        switch self {
        case .warning: .statusWarning
        case .problem: .statusProblem
        case .fault: .statusFault
        case .faultIndicator: .statusFault
        case .info: .statusInfo
        case .mixer: .viewMixer
        case .add: .editAdd
        case .remove: .editRemove
        case .confirm: .editConfirm
        case .close: .editClose
        case .zoomIn: .viewZoomIn
        case .zoomOut: .viewZoomOut
        case .audio: .inputAudio
        case .drumGrid: .editorDrumGrid
        case .pianoRoll: .editorPianoRoll
        case .list: .editorList
        case .muted: .statusAudioMuted
        case .loop: .modeLoop
        case .split: .viewSplit
        case .empty: .viewEmpty
        case .record: .transportRecord
        case .recording: .transportRecord
        case .armed: .recordArm
        case .pencil: .toolPencil
        case .diskSlow: .statusDiskSlow
        case .expand: .viewExpand
        case .collapse: .viewCollapse
        case .menuDown: .navDown
        case .leftPane: .viewLeftPane
        case .bottomPane: .viewBottomPane
        case .rightPane: .viewRightPane
        case .settingsEditing: .toolPencil
        case .settingsKeys: .settingsKeys
        case .settingsAudio: .settingsAudio
        case .settingsRecording: .transportRecord
        case .settingsAutomation: .editorEnvelope
        case .settingsSurfaces: .settingsSurfaces
        case .settingsAppearance: .settingsAppearance
        case .eq: .mixerEq
        case .play: .transportPlay
        case .stop: .transportStop
        case .midiLearn: .controlMidiLearn
        }
    }

    /// The SF Symbol the app draws: what it showed before 7b. pianoRoll,
    /// armed, pencil, settingsAppearance and eq had no icon then and show
    /// nowhere yet; theirs are placeholders.
    public var symbol: String {
        switch self {
        case .warning: "exclamationmark.triangle"
        case .problem: "exclamationmark.circle"
        case .fault: "xmark.octagon"
        case .info: "info.circle"
        case .faultIndicator: "circle.fill"
        case .mixer: "slider.vertical.3"
        case .add: "plus"
        case .remove: "minus.circle"
        case .confirm: "checkmark"
        case .close: "xmark"
        case .zoomIn: "plus.magnifyingglass"
        case .zoomOut: "minus.magnifyingglass"
        case .audio: "waveform"
        case .drumGrid: "square.grid.3x3"
        case .pianoRoll: "pianokeys"
        case .list: "music.note.list"
        case .muted: "speaker.slash"
        case .loop: "repeat"
        case .split: "rectangle.split.1x2"
        case .empty: "xmark.rectangle"
        case .record: "record.circle"
        case .armed: "record.circle"
        case .pencil: "pencil"
        case .diskSlow: "externaldrive.badge.exclamationmark"
        case .recording: "record.circle.fill"
        case .expand: "arrow.up.left.and.arrow.down.right"
        case .collapse: "arrow.down.right.and.arrow.up.left"
        case .menuDown: "chevron.down"
        case .leftPane: "sidebar.left"
        case .bottomPane: "rectangle.bottomthird.inset.filled"
        case .rightPane: "sidebar.right"
        case .settingsEditing: "pencil"
        case .settingsKeys: "keyboard"
        case .settingsAudio: "hifispeaker"
        case .settingsRecording: "record.circle"
        case .settingsAutomation: "slider.vertical.3"
        case .settingsSurfaces: "dial.medium"
        case .settingsAppearance: "paintpalette"
        case .eq: "slider.horizontal.3"
        case .play: "play.fill"
        case .stop: "stop.fill"
        case .midiLearn: "pianokeys"
        }
    }

    /// `Image(systemName: symbol)`.
    public var image: Image { Image(systemName: symbol) }

    /// `Label(title, systemImage: symbol)`.
    public func label(_ title: String) -> Label<Text, Image> { Label(title, systemImage: symbol) }

    /// `NSImage(systemSymbolName: symbol, accessibilityDescription:)`.
    public func nsImage(accessibilityDescription: String?) -> NSImage? {
        NSImage(systemSymbolName: symbol, accessibilityDescription: accessibilityDescription)
    }
}
