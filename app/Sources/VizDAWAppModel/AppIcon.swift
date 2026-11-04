import AppKit
import SymbioticIcons
import SymbioticTokens

/// The icons VizDAW shows, by meaning, each drawn from a SymbioticIcons role
/// (plan 7b Task B3). The app uses no SF Symbols: with SymbioticBrand
/// registered a role draws its branded glyph, else its standard icon.
public enum AppIcon: CaseIterable, Sendable {
    // Status.
    case warning, problem, fault, info
    case mixer
    // Editing.
    case add, remove, confirm, close
    case zoomIn, zoomOut
    // Inputs and editors.
    case audio, drumGrid, pianoRoll, list
    case muted, loop, split, empty
    case record, armed, pencil, diskSlow
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

    /// The SF Symbol this icon replaced ("" if it had none), so the owner can
    /// return any icon to its original symbol in one line. Unused for now.
    /// Where one case replaced several symbols, this is the main one: `fault`
    /// also replaced the Faults button's "circle.fill", `record` the
    /// recording state's "record.circle.fill".
    public var originalSymbol: String {
        switch self {
        case .warning: "exclamationmark.triangle"
        case .problem: "exclamationmark.circle"
        case .fault: "xmark.octagon"
        case .info: "info.circle"
        case .mixer: "slider.vertical.3"
        case .add: "plus"
        case .remove: "minus.circle"
        case .confirm: "checkmark"
        case .close: "xmark"
        case .zoomIn: "plus.magnifyingglass"
        case .zoomOut: "minus.magnifyingglass"
        case .audio: "waveform"
        case .drumGrid: "square.grid.3x3"
        case .pianoRoll: ""
        case .list: "music.note.list"
        case .muted: "speaker.slash"
        case .loop: "repeat"
        case .split: "rectangle.split.1x2"
        case .empty: "xmark.rectangle"
        case .record: "record.circle"
        case .armed: ""
        case .pencil: ""
        case .diskSlow: "externaldrive.badge.exclamationmark"
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
        case .settingsAppearance: ""
        case .eq: ""
        case .play: "play.fill"
        case .stop: "stop.fill"
        case .midiLearn: "pianokeys"
        }
    }

    /// A template image of the icon, for AppKit and for the places SwiftUI
    /// draws only images (menu items, Settings tabs).
    @MainActor
    public func image(size: CGFloat, theme: Theme) -> NSImage {
        NSImage.icon(role, size: size, theme: theme)
    }
}
